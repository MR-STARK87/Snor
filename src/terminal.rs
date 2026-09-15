use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};

const ROWS: u16 = 24;
const COLS: u16 = 80;

pub struct Terminal {
    parser: vt100::Parser,
    rx: Option<Receiver<Vec<u8>>>,
    writer: Option<Box<dyn Write + Send>>,
    _child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
    _master: Option<Box<dyn portable_pty::MasterPty + Send>>,
    input: String,
    pub error: Option<String>,
    pub running: bool,
    started: bool,
}

impl Terminal {
    pub fn new() -> Self {
        Self {
            parser: vt100::Parser::new(ROWS, COLS, 2000),
            rx: None,
            writer: None,
            _child: None,
            _master: None,
            input: String::new(),
            error: None,
            running: false,
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
        cmd.args(["-NoLogo", "-NoExit"]);
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

    fn send_line(&mut self) {
        let line = std::mem::take(&mut self.input);
        let mut out = line.into_bytes();
        out.push(b'\r');
        self.send_bytes(&out);
    }

    pub fn poll(&mut self) {
        if let Some(rx) = &self.rx {
            let mut any = false;
            while let Ok(chunk) = rx.try_recv() {
                self.parser.process(&chunk);
                any = true;
            }
            let _ = any;
        }
    }

    pub fn ui(&mut self, ui: &mut eframe::egui::Ui, cwd: &PathBuf) {
        self.ensure_started(cwd);
        self.poll();

        ui.horizontal(|ui| {
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
            ui.with_layout(
                eframe::egui::Layout::right_to_left(eframe::egui::Align::Center),
                |ui| {
                    if ui.small_button("clear").clicked() {
                        self.parser = vt100::Parser::new(ROWS, COLS, 2000);
                    }
                    if ui.small_button("Ctrl+C").clicked() {
                        self.send_bytes(&[0x03]);
                    }
                    if ui.small_button("restart").clicked() {
                        self.started = false;
                        self.rx = None;
                        self.writer = None;
                        self._child = None;
                        self._master = None;
                        self.parser = vt100::Parser::new(ROWS, COLS, 2000);
                        self.ensure_started(cwd);
                    }
                },
            );
        });

        if let Some(err) = &self.error {
            ui.colored_label(eframe::egui::Color32::from_rgb(0xE0, 0x6C, 0x75), err);
        }

        ui.separator();

        let contents = self.parser.screen().contents();
        eframe::egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                ui.monospace(contents);
            });

        ui.separator();
        ui.horizontal(|ui| {
            ui.label("$");
            let resp = ui.add(
                eframe::egui::TextEdit::singleline(&mut self.input)
                    .desired_width(f32::INFINITY)
                    .hint_text("type command, Enter to send (try: opencode --help)"),
            );
            if resp.lost_focus() && ui.input(|i| i.key_pressed(eframe::egui::Key::Enter)) {
                self.send_line();
                ui.memory_mut(|m| m.request_focus(resp.id));
            }
            if ui.small_button("send").clicked() {
                self.send_line();
            }
        });
        ui.horizontal(|ui| {
            for (label, cmd) in [("dir", "dir\r"), ("opencode --help", "opencode --help\r")] {
                if ui.small_button(label).clicked() {
                    self.send_bytes(cmd.as_bytes());
                }
            }
        });
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        if let Some(child) = self._child.as_mut() {
            use portable_pty::ChildKiller;
            let _ = child.kill();
        }
    }
}
