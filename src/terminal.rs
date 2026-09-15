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
    /// Click-to-type latch: set when the terminal grid is clicked, cleared
    /// the moment any real widget (editor, find box, explorer field) owns
    /// keyboard focus. Keystrokes are forwarded only while `active` holds
    /// and nothing else is focused, so typing code can never leak into the
    /// shell and vice versa.
    pub active: bool,
    /// Hidden entirely (Ctrl+Tab): no terminal UI at all, the editor takes
    /// the full column. Distinct from `collapsed` (header strip).
    pub hidden: bool,
    /// Terminal height in px, adjusted by dragging the editor/terminal
    /// divider. Persisted across frames.
    pub term_h: f32,
    /// Focus state seen at the end of the last frame; used to tell an
    /// explicit focus grab (click, Ctrl+F) apart from egui's Tab focus
    /// theft, which must be handed back so shell completion keeps working.
    focus_clear: bool,
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
            active: false,
            hidden: false,
            term_h: 280.0,
            focus_clear: true,
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

    /// Send a full shell line (used by the editor Run button).
    pub fn send_line(&mut self, line: &str) {
        if self.collapsed {
            self.collapsed = false;
        }
        self.active = true;
        let mut s = line.to_string();
        if !s.ends_with('\r') && !s.ends_with('\n') {
            s.push('\r');
        }
        self.send_bytes(s.as_bytes());
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
                            if params.first() == Some(&b'>') {
                                replies.push(b"\x1b[>0;0;0c".to_vec());
                            } else {
                                replies.push(b"\x1b[?62;c".to_vec());
                            }
                        }
                        // DECRQM (?mode$p): answer "not set" so the shell never
                        // blocks waiting for a mode report.
                        b'p' if params.first() == Some(&b'?') && params.last() == Some(&b'$') => {
                            let mode = String::from_utf8_lossy(&params[1..params.len() - 1]);
                            replies.push(format!("\x1b[?{mode};2$y").into_bytes());
                        }
                        // Kitty keyboard query (?u): report unsupported.
                        // Plain `u` (restore cursor) needs no reply.
                        b'u' if params.first() == Some(&b'?') => {
                            replies.push(b"\x1b[?0u".to_vec());
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

    /// Small vector-style maximize/restore icon (no font glyph needed).
    fn maximize_icon(ui: &mut eframe::egui::Ui, fullscreen: bool) -> eframe::egui::Response {
        use eframe::egui::{Sense, Stroke, vec2};
        let (rect, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
        if ui.is_rect_visible(rect) {
            let painter = ui.painter_at(rect);
            if resp.hovered() {
                painter.rect_filled(rect, 3.0, ui.visuals().widgets.hovered.bg_fill);
            }
            let stroke = Stroke::new(1.5, ui.visuals().text_color());
            if fullscreen {
                painter.rect_stroke(
                    eframe::egui::Rect::from_min_size(rect.min + vec2(7.0, 3.0), vec2(9.0, 9.0)),
                    1.0,
                    stroke,
                    eframe::egui::StrokeKind::Middle,
                );
                painter.rect_stroke(
                    eframe::egui::Rect::from_min_size(rect.min + vec2(3.0, 7.0), vec2(9.0, 9.0)),
                    1.0,
                    stroke,
                    eframe::egui::StrokeKind::Middle,
                );
            } else {
                painter.rect_stroke(
                    eframe::egui::Rect::from_min_size(rect.min + vec2(4.0, 4.0), vec2(12.0, 12.0)),
                    1.0,
                    stroke,
                    eframe::egui::StrokeKind::Middle,
                );
            }
        }
        resp
    }

    pub fn ui(&mut self, ui: &mut eframe::egui::Ui, cwd: &PathBuf) {
        self.ensure_started(cwd);
        self.poll();

        let is_active = self.active;

        ui.horizontal(|ui| {
            // Tab pill, like the reference terminal header.
            eframe::egui::Frame::NONE
                .fill(crate::theme::tab_active())
                .corner_radius(6.0)
                .inner_margin(eframe::egui::Margin::symmetric(6, 2))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            eframe::egui::RichText::new(">_")
                                .small()
                                .strong()
                                .color(crate::theme::accent()),
                        );
                        ui.label("Terminal");
                    });
                });
            if ui
                .small_button(if self.collapsed { "+" } else { "-" })
                .on_hover_text("collapse / expand")
                .clicked()
            {
                self.collapsed = !self.collapsed;
            }
            ui.label(
                eframe::egui::RichText::new(if self.running {
                    "powershell.exe"
                } else {
                    "stopped"
                })
                .small()
                .color(crate::theme::dim_text()),
            );
            if is_active {
                ui.label(
                    eframe::egui::RichText::new("●")
                        .small()
                        .color(crate::theme::accent()),
                );
            } else {
                ui.label(
                    eframe::egui::RichText::new("click to type")
                        .small()
                        .color(crate::theme::dim_text()),
                );
            }
            ui.with_layout(
                eframe::egui::Layout::right_to_left(eframe::egui::Align::Center),
                |ui| {
                    if ui
                        .small_button("restart")
                        .on_hover_text("restart powershell")
                        .clicked()
                    {
                        self.started = false;
                        self.rx = None;
                        self.writer = None;
                        self._child = None;
                        self._master = None;
                        self.parser = vt100::Parser::new(ROWS, COLS, SCROLLBACK);
                        self.ensure_started(cwd);
                    }
                    if ui
                        .small_button("clear")
                        .on_hover_text("clear screen")
                        .clicked()
                    {
                        self.parser = vt100::Parser::new(ROWS, COLS, SCROLLBACK);
                    }
                    // Shell picker look, like the reference (single shell).
                    eframe::egui::Frame::NONE
                        .fill(crate::theme::tab_active())
                        .corner_radius(6.0)
                        .inner_margin(eframe::egui::Margin::symmetric(8, 2))
                        .show(ui, |ui| {
                            ui.label(
                                eframe::egui::RichText::new("powershell")
                                    .small()
                                    .color(crate::theme::dim_text()),
                            );
                        });
                    let max_resp = Self::maximize_icon(ui, self.fullscreen);
                    if max_resp
                        .on_hover_text(if self.fullscreen {
                            "restore terminal"
                        } else {
                            "maximize terminal"
                        })
                        .clicked()
                    {
                        self.fullscreen = !self.fullscreen;
                        if self.fullscreen {
                            self.collapsed = false;
                        }
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

        let job = term_job(self.parser.screen());
        // No input widget at all: the grid itself is the click-to-type area.
        // `active` latches on grid click. Forwarding is additionally gated on
        // no other widget holding keyboard focus, so keystrokes never leak
        // between the shell and the editor in either direction.
        //
        // Why not a plain egui focus id? Two verified egui 0.36 behaviors
        // forbid it: (1) a requested-but-never-interacted id is dropped by
        // the dead-man's switch in `Memory::end_pass`, so a dummy id loses
        // "focus" ~1 frame after the click; (2) the focus-lock filter type
        // (`EventFilter`) is crate-private, so Tab/arrows/Escape can't be
        // locked to a custom widget the way `egui_tty` does on newer egui.
        let grid_max = (ui.available_height() - 4.0).max(60.0);
        let mut grid_clicked = false;
        eframe::egui::ScrollArea::vertical()
            .id_salt("snor_term_grid")
            .max_height(grid_max)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.push_id("snor_term_grid_label", |ui| {
                    let resp = ui.add(
                        eframe::egui::Label::new(job)
                            .extend()
                            .sense(eframe::egui::Sense::click()),
                    );
                    grid_clicked = resp.clicked();
                });
            });
        if grid_clicked {
            self.active = true;
        }

        // Type-directly-in-terminal: while latched active and no real widget
        // (editor, find box, explorer field) owns the keyboard, printable
        // text + paste + special keys go straight to the pty.
        use eframe::egui::{Event, Key};
        let events = ui.input(|i| i.events.clone());
        let pointer_busy = ui.input(|i| i.pointer.any_click());
        let focused_none = ui.memory(|m| m.focused().is_none());

        // Tab/Shift+Tab from an unfocused state is grabbed by the first
        // widget that wants focus (`Memory::interested_in_focus`), which
        // would yank keystrokes into the editor and break shell completion.
        // Hand it back when no click was involved; the Tab byte itself is
        // still forwarded below.
        let tab_pressed = events.iter().any(|e| {
            matches!(
                e,
                Event::Key {
                    key: Key::Tab,
                    pressed: true,
                    ..
                }
            )
        });
        if self.active
            && !focused_none
            && self.focus_clear
            && tab_pressed
            && !pointer_busy
            && let Some(id) = ui.memory(|m| m.focused())
        {
            ui.memory_mut(|m| m.surrender_focus(id));
        }
        let clear_now = ui.memory(|m| m.focused().is_none());
        if self.active && clear_now {
            for ev in &events {
                match ev {
                    Event::Text(text) => {
                        self.send_bytes(text.as_bytes());
                    }
                    Event::Paste(text) => {
                        self.send_bytes(text.as_bytes());
                    }
                    Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        // Never steal editor save / find.
                        if matches!(key, Key::S | Key::F) && (modifiers.ctrl || modifiers.command)
                        {
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
        } else if !clear_now {
            // A real widget owns the keyboard: drop the latch so the next
            // keystroke can't leak into the shell.
            self.active = false;
        }
        self.focus_clear = clear_now;
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

    #[test]
    fn query_responder_handles_split_sequences() {
        let mut t = Terminal::new();
        t.respond_to_queries(b"\x1b[");
        assert!(!t.inq.is_empty(), "partial sequence must be kept");
        t.respond_to_queries(b"6n");
        t.respond_to_queries(b"\x1b[?2026$p");
        t.respond_to_queries(b"\x1b[c");
        t.respond_to_queries(b"plain text without escapes");
        assert!(t.inq.len() <= 64);
    }

    fn headless_raw() -> eframe::egui::RawInput {
        eframe::egui::RawInput {
            screen_rect: Some(eframe::egui::Rect::from_min_size(
                eframe::egui::Pos2::ZERO,
                eframe::egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        }
    }

    /// Proves the previous bug: a focus id that is requested but never
    /// attached to an interacted widget is dropped by egui's dead-man's
    /// switch (`Memory::end_pass`), so it can NOT gate terminal input.
    #[test]
    fn uninteracted_focus_id_does_not_survive() {
        use eframe::egui;
        let ctx = egui::Context::default();
        let dummy = egui::Id::new("snor_dummy_focus_probe");
        ctx.run_ui(headless_raw(), |ui| {
            ui.label("idle");
            ui.memory_mut(|m| m.request_focus(dummy));
        })
        .drop_without_applying_deltas();
        assert!(ctx.memory(|m| m.has_focus(dummy)));
        // Two plain frames with no interact of `dummy`.
        for _ in 0..2 {
            ctx.run_ui(headless_raw(), |ui| {
                ui.label("idle");
            })
            .drop_without_applying_deltas();
        }
        assert!(
            !ctx.memory(|m| m.has_focus(dummy)),
            "dummy focus must be dropped; gating terminal input on it loses keystrokes"
        );
    }

    /// Proves the fixed mechanism: an interacted widget's own response id
    /// (the pattern the terminal grid now relies on via its latched
    /// `active` flag + real-focus gate) keeps requested focus across frames.
    #[test]
    fn interacted_label_keeps_requested_focus() {
        use eframe::egui;
        let ctx = egui::Context::default();
        let mut grid_id: Option<egui::Id> = None;
        for frame in 0..5 {
            ctx.run_ui(headless_raw(), |ui| {
                let resp = ui.add(egui::Label::new("grid").sense(egui::Sense::click()));
                if frame == 0 {
                    resp.request_focus();
                }
                grid_id = Some(resp.id);
            })
            .drop_without_applying_deltas();
            assert!(
                ctx.memory(|m| m.has_focus(grid_id.unwrap())),
                "interacted label must retain focus on frame {frame}"
            );
        }
    }

    /// End-to-end through the real ConPTY powershell: a typed line must be
    /// echoed back on the vt100 screen. Covers spawn, write, read, poll and
    /// the DSR/CPR query responder — everything except the OS key event.
    #[cfg(windows)]
    #[test]
    fn pty_powershell_echo_roundtrip() {
        use std::time::{Duration, Instant};
        let mut t = Terminal::new();
        t.ensure_started(&std::env::temp_dir());
        let t0 = Instant::now();
        while t.total_bytes == 0 && t.error.is_none() && t0.elapsed() < Duration::from_secs(10)
        {
            std::thread::sleep(Duration::from_millis(50));
            t.poll();
        }
        assert!(t.error.is_none(), "pty spawn failed: {:?}", t.error);
        assert!(
            t.total_bytes > 0,
            "powershell printed nothing in 10s; cannot verify input path"
        );
        t.send_bytes(b"echo SNORPTYOK123\r");
        let t1 = Instant::now();
        let mut seen = false;
        while t1.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(50));
            t.poll();
            if t.parser.screen().contents().contains("SNORPTYOK123") {
                seen = true;
                break;
            }
        }
        assert!(
            seen,
            "typed line never echoed — shell not responding to pty input"
        );
    }
}

