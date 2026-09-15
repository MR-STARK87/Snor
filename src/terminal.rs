use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};

const ROWS: u16 = 24;
const COLS: u16 = 80;
const SCROLLBACK: usize = 1000;

fn vt_color(c: vt100::Color, is_bg: bool) -> eframe::egui::Color32 {
    use eframe::egui::Color32 as C;
    match c {
        vt100::Color::Default => {
            if is_bg {
                C::TRANSPARENT
            } else {
                C::from_rgb(0xD5, 0xDA, 0xE2)
            }
        }
        vt100::Color::Rgb(r, g, b) => C::from_rgb(r, g, b),
        vt100::Color::Idx(i) => match i {
            0 => C::from_rgb(0x1E, 0x23, 0x2C),
            1 => C::from_rgb(0xE0, 0x6C, 0x75),
            2 => C::from_rgb(0x7D, 0xD3, 0xA8),
            3 => C::from_rgb(0xE5, 0xC0, 0x7B),
            4 => C::from_rgb(0x7A, 0xA2, 0xF7),
            5 => C::from_rgb(0xC6, 0x78, 0xDD),
            6 => C::from_rgb(0x56, 0xB6, 0xC2),
            7 => C::from_rgb(0xAB, 0xB2, 0xBF),
            8 => C::from_rgb(0x5C, 0x63, 0x70),
            9 => C::from_rgb(0xE0, 0x6C, 0x75),
            10 => C::from_rgb(0x98, 0xC3, 0x79),
            11 => C::from_rgb(0xE5, 0xC0, 0x7B),
            12 => C::from_rgb(0x61, 0xAF, 0xEF),
            13 => C::from_rgb(0xC6, 0x78, 0xDD),
            14 => C::from_rgb(0x56, 0xB6, 0xC2),
            15 => C::from_rgb(0xFF, 0xFF, 0xFF),
            16..=231 => {
                let n = i - 16;
                let r = (n / 36) % 6;
                let g = (n / 6) % 6;
                let b = n % 6;
                let v = |x: u8| if x == 0 { 0 } else { 55 + 40 * x };
                C::from_rgb(v(r), v(g), v(b))
            }
            _ => {
                let v = 8 + 10 * (i - 232);
                C::from_rgb(v, v, v)
            }
        },
    }
}

fn term_job(screen: &vt100::Screen) -> eframe::egui::text::LayoutJob {
    use eframe::egui::text::{LayoutJob, TextFormat};
    let mut job = LayoutJob::default();
    let mono = eframe::egui::FontId::monospace(12.5);
    let (cur_row, cur_col) = screen.cursor_position();
    for row in 0..ROWS {
        let mut run = String::new();
        let mut run_fg = vt100::Color::Default;
        let mut run_bg = vt100::Color::Default;
        let mut run_bold = false;
        let mut started = false;
        let flush = |job: &mut LayoutJob,
                     run: &mut String,
                     fg: vt100::Color,
                     bg: vt100::Color,
                     bold: bool| {
            if run.is_empty() {
                return;
            }
            let mut color = vt_color(fg, false);
            if bold && matches!(fg, vt100::Color::Default | vt100::Color::Idx(0..=7)) {
                color = eframe::egui::Color32::WHITE;
            }
            job.append(
                run.as_str(),
                0.0,
                TextFormat {
                    font_id: mono.clone(),
                    color,
                    background: vt_color(bg, true),
                    ..Default::default()
                },
            );
            run.clear();
        };
        for col in 0..COLS {
            let (mut fg, mut bg, mut bold, mut text) = match screen.cell(row, col) {
                Some(cell) => (
                    cell.fgcolor(),
                    cell.bgcolor(),
                    cell.bold(),
                    cell.contents().to_string(),
                ),
                None => (
                    vt100::Color::Default,
                    vt100::Color::Default,
                    false,
                    " ".to_string(),
                ),
            };
            if text.is_empty() {
                text = " ".to_string();
            }
            if row == cur_row && col == cur_col {
                bg = vt100::Color::Rgb(0x7D, 0xD3, 0xA8);
                fg = vt100::Color::Rgb(0x10, 0x12, 0x17);
                bold = false;
            }
            if !started {
                run_fg = fg;
                run_bg = bg;
                run_bold = bold;
                started = true;
            }
            if fg != run_fg || bg != run_bg || bold != run_bold {
                flush(&mut job, &mut run, run_fg, run_bg, run_bold);
                run_fg = fg;
                run_bg = bg;
                run_bold = bold;
            }
            run.push_str(&text);
        }
        flush(&mut job, &mut run, run_fg, run_bg, run_bold);
        if row + 1 < ROWS {
            job.append(
                "\n",
                0.0,
                TextFormat {
                    font_id: mono.clone(),
                    color: eframe::egui::Color32::TRANSPARENT,
                    ..Default::default()
                },
            );
        }
    }
    job
}

pub struct Terminal {
    parser: vt100::Parser,
    rx: Option<Receiver<Vec<u8>>>,
    writer: Option<Box<dyn Write + Send>>,
    _child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
    _master: Option<Box<dyn portable_pty::MasterPty + Send>>,
    pub error: Option<String>,
    pub running: bool,
    pub collapsed: bool,
    pub fullscreen: bool,
    pub total_bytes: u64,
    pub total_chunks: u64,
    inq: Vec<u8>,
    started: bool,
}

impl Terminal {
    pub fn new() -> Self {
        Self {
            parser: vt100::Parser::new(ROWS, COLS, SCROLLBACK),
            rx: None,
            writer: None,
            _child: None,
            _master: None,
            error: None,
            running: false,
            collapsed: false,
            fullscreen: false,
            total_bytes: 0,
            total_chunks: 0,
            inq: Vec::new(),
            started: false,
        }
    }

    pub fn ensure_started(&mut self, cwd: &PathBuf) {
        if self.started {
            return;
        }
        self.started = true;
        self.spawn(cwd);
    }

    fn spawn(&mut self, cwd: &PathBuf) {
        let pty_system = native_pty_system();
        let pair = match pty_system.openpty(PtySize {
            rows: ROWS,
            cols: COLS,
            pixel_width: 0,
            pixel_height: 0,
        }) {
            Ok(p) => p,
            Err(e) => {
                self.error = Some(format!("pty open failed: {e}"));
                return;
            }
        };
        let mut cmd = CommandBuilder::new("powershell.exe");
        cmd.args(["-NoLogo", "-NoProfile", "-NoExit"]);
        cmd.cwd(cwd);
        let child = match pair.slave.spawn_command(cmd) {
            Ok(c) => c,
            Err(e) => {
                self.error = Some(format!("spawn powershell failed: {e}"));
                return;
            }
        };
        let mut reader = match pair.master.try_clone_reader() {
            Ok(r) => r,
            Err(e) => {
                self.error = Some(format!("pty reader failed: {e}"));
                return;
            }
        };
        let writer = match pair.master.take_writer() {
            Ok(w) => w,
            Err(e) => {
                self.error = Some(format!("pty writer failed: {e}"));
                return;
            }
        };
        let (tx, rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        self.rx = Some(rx);
        self.writer = Some(writer);
        self._child = Some(child);
        self._master = Some(pair.master);
        self.running = true;
        self.error = None;
    }

    fn send_bytes(&mut self, bytes: &[u8]) {
        if let Some(w) = self.writer.as_mut()
            && let Err(e) = w.write_all(bytes).and_then(|_| w.flush())
        {
            self.error = Some(format!("pty write failed: {e}"));
        }
    }

    /// Map a pressed key to pty bytes. Returns None to leave the event alone
    /// (notably Ctrl+S stays free for editor save, plain chars come via Text).
    fn key_to_bytes(
        key: eframe::egui::Key,
        modifiers: eframe::egui::Modifiers,
    ) -> Option<&'static [u8]> {
        use eframe::egui::Key;
        if modifiers.ctrl || modifiers.command {
            return match key {
                Key::C => Some(&[0x03]),
                Key::D => Some(&[0x04]),
                Key::Z => Some(&[0x1a]),
                Key::L => Some(&[0x0c]),
                _ => None,
            };
        }
        if modifiers.alt {
            return None;
        }
        match key {
            Key::Enter => Some(b"\r"),
            Key::Backspace => Some(&[0x7f]),
            Key::Tab => {
                if modifiers.shift {
                    Some(b"\x1b[Z")
                } else {
                    Some(b"\t")
                }
            }
            Key::Escape => Some(&[0x1b]),
            Key::ArrowUp => Some(b"\x1b[A"),
            Key::ArrowDown => Some(b"\x1b[B"),
            Key::ArrowRight => Some(b"\x1b[C"),
            Key::ArrowLeft => Some(b"\x1b[D"),
            Key::Home => Some(b"\x1b[H"),
            Key::End => Some(b"\x1b[F"),
            Key::Delete => Some(b"\x1b[3~"),
            Key::Insert => Some(b"\x1b[2~"),
            Key::PageUp => Some(b"\x1b[5~"),
            Key::PageDown => Some(b"\x1b[6~"),
            _ => None,
        }
    }

    /// Answer host queries (DSR cursor report, DSR status, DA) so shells
    /// like PowerShell/PSReadLine don't block waiting for a reply.
    fn respond_to_queries(&mut self, chunk: &[u8]) {
        self.inq.extend_from_slice(chunk);
        if self.inq.len() > 4096 {
            let excess = self.inq.len() - 4096;
            self.inq.drain(..excess);
        }
        let (replies, consumed_upto) = {
            let (row, col) = self.parser.screen().cursor_position();
            let mut replies: Vec<Vec<u8>> = Vec::new();
            let buf = &self.inq;
            let mut i = 0;
            let mut last_complete = 0;
            while i < buf.len() {
                if buf[i] == 0x1b && i + 1 < buf.len() && buf[i + 1] == b'[' {
                    let mut j = i + 2;
                    while j < buf.len()
                        && matches!(
                            buf[j],
                            b'0'..=b'9' | b';' | b'?' | b'>' | b'!' | b'$' | b'"' | b' ' | b'\''
                        )
                    {
                        j += 1;
                    }
                    if j >= buf.len() {
                        break; // incomplete sequence, keep as tail
                    }
                    let params = &buf[i + 2..j];
                    match buf[j] {
                        b'n' if params == b"6" => {
                            replies.push(format!("\x1b[{};{}R", row + 1, col + 1).into_bytes());
                        }
                        b'n' if params == b"5" => {
                            replies.push(b"\x1b[0n".to_vec());
                        }
                        b'c' => {
                            replies.push(b"\x1b[?62;c".to_vec());
                        }
                        _ => {}
                    }
                    i = j + 1;
                    last_complete = i;
                } else {
                    i += 1;
                }
            }
            (replies, last_complete)
        };
        let keep_from = consumed_upto.max(self.inq.len().saturating_sub(64));
        self.inq.drain(..keep_from);
        for reply in replies {
            self.send_bytes(&reply);
        }
    }

    pub fn poll(&mut self) {
        let chunks: Vec<Vec<u8>> = if let Some(rx) = &self.rx {
            let mut out = Vec::new();
            while let Ok(chunk) = rx.try_recv() {
                out.push(chunk);
            }
            out
        } else {
            Vec::new()
        };
        if chunks.is_empty() {
            return;
        }
        {
            for chunk in &chunks {
                self.total_chunks += 1;
                self.total_bytes += chunk.len() as u64;
                self.parser.process(chunk);
                self.respond_to_queries(chunk);
            }
        }
    }

    pub fn ui(&mut self, ui: &mut eframe::egui::Ui, cwd: &PathBuf) {
        self.ensure_started(cwd);
        self.poll();

        let focus_id = ui.make_persistent_id("snor_term_focus");
        let is_focused = ui.memory(|m| m.has_focus(focus_id));

        ui.horizontal(|ui| {
            if ui
                .small_button(if self.fullscreen { "unmax" } else { "max" })
                .clicked()
            {
                self.fullscreen = !self.fullscreen;
                if self.fullscreen {
                    self.collapsed = false;
                }
            }
            if ui
                .small_button(if self.collapsed { "+" } else { "-" })
                .clicked()
            {
                self.collapsed = !self.collapsed;
            }
            ui.heading("Terminal");
            ui.label(
                eframe::egui::RichText::new(if self.running {
                    "powershell.exe"
                } else {
                    "stopped"
                })
                .small()
                .color(crate::theme::dim_text()),
            );
            if is_focused {
                ui.label(
                    eframe::egui::RichText::new("●")
                        .small()
                        .color(crate::theme::accent()),
                );
            }
            ui.with_layout(
                eframe::egui::Layout::right_to_left(eframe::egui::Align::Center),
                |ui| {
                    if ui.small_button("clear").clicked() {
                        self.parser = vt100::Parser::new(ROWS, COLS, SCROLLBACK);
                    }
                    if ui.small_button("restart").clicked() {
                        self.started = false;
                        self.rx = None;
                        self.writer = None;
                        self._child = None;
                        self._master = None;
                        self.parser = vt100::Parser::new(ROWS, COLS, SCROLLBACK);
                        self.ensure_started(cwd);
                    }
                },
            );
        });

        if let Some(err) = &self.error {
            ui.colored_label(eframe::egui::Color32::from_rgb(0xE0, 0x6C, 0x75), err);
        }

        if self.collapsed {
            return;
        }

        ui.separator();

        let focus_id = ui.make_persistent_id("snor_term_focus");

        let job = term_job(self.parser.screen());
        let grid_max = (ui.available_height() - 12.0).max(60.0);
        eframe::egui::ScrollArea::vertical()
            .id_salt("snor_term_grid")
            .max_height(grid_max)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let resp = ui.add(
                    eframe::egui::Label::new(job)
                        .extend()
                        .sense(eframe::egui::Sense::click()),
                );
                if resp.clicked() {
                    ui.memory_mut(|m| m.request_focus(focus_id));
                }
            });

        if ui.memory(|m| m.has_focus(focus_id)) {
            use eframe::egui::{Event, Key};
            let events = ui.input(|i| i.events.clone());
            for ev in &events {
                match ev {
                    Event::Text(text) => {
                        self.send_bytes(text.as_bytes());
                    }
                    Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        // Never steal editor save.
                        if *key == Key::S && (modifiers.ctrl || modifiers.command) {
                            continue;
                        }
                        if let Some(bytes) = Self::key_to_bytes(*key, *modifiers) {
                            self.send_bytes(bytes);
                            ui.input_mut(|i| {
                                i.consume_key(*modifiers, *key);
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        if let Some(child) = self._child.as_mut() {
            let _ = child.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Terminal;
    use eframe::egui::{Key, Modifiers};

    #[test]
    fn key_mapping_covers_terminal_basics() {
        let plain = Modifiers::NONE;
        assert_eq!(
            Terminal::key_to_bytes(Key::Enter, plain),
            Some(b"\r".as_slice())
        );
        assert_eq!(
            Terminal::key_to_bytes(Key::Backspace, plain),
            Some([0x7f].as_slice())
        );
        assert_eq!(
            Terminal::key_to_bytes(Key::Tab, plain),
            Some(b"\t".as_slice())
        );
        assert_eq!(
            Terminal::key_to_bytes(Key::Escape, plain),
            Some([0x1b].as_slice())
        );
        assert_eq!(
            Terminal::key_to_bytes(Key::ArrowUp, plain),
            Some(b"\x1b[A".as_slice())
        );
        assert_eq!(
            Terminal::key_to_bytes(Key::ArrowLeft, plain),
            Some(b"\x1b[D".as_slice())
        );
        assert_eq!(
            Terminal::key_to_bytes(Key::Delete, plain),
            Some(b"\x1b[3~".as_slice())
        );
    }

    #[test]
    fn key_mapping_respects_app_shortcuts() {
        let ctrl = Modifiers::CTRL;
        // Ctrl+S must stay free for editor save.
        assert_eq!(Terminal::key_to_bytes(Key::S, ctrl), None);
        // Ctrl+C interrupts the pty child.
        assert_eq!(
            Terminal::key_to_bytes(Key::C, ctrl),
            Some([0x03].as_slice())
        );
        // Plain letters travel via Text events, not keys.
        assert_eq!(Terminal::key_to_bytes(Key::A, Modifiers::NONE), None);
    }
}
