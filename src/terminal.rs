use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};

const ROWS: u16 = 24;
const COLS: u16 = 80;
const SCROLLBACK: usize = 1000;

/// Width reserved on the right of the terminal header for the status hint and
/// the icon cluster, so the tab strip can be bounded and leave them on the
/// same line. Measured off a live capture: the shell pill inks 58pt, the four
/// 20pt buttons plus their spacing about 110pt, and "click to type" 68pt.
const TERM_HDR_RIGHT_W: f32 = 250.0;

/// Tab label for the n-th shell ever spawned.
///
/// The first is bare "powershell"; later ones are numbered from 2, so a
/// single tab never reads "powershell 1". Numbering counts shells spawned
/// rather than tabs currently open, so closing and reopening does not reuse
/// a label that is still on screen.
fn shell_title(n: usize) -> String {
    if n <= 1 {
        "powershell".to_owned()
    } else {
        format!("powershell {n}")
    }
}

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
    let mono = eframe::egui::FontId::monospace(TERM_FONT_SIZE);
    // The screen's own size, not the 80x24 defaults: panes resize their
    // session, and the grid must render exactly what the shell owns.
    let (rows, cols) = screen.size();
    let (cur_row, cur_col) = screen.cursor_position();
    for row in 0..rows {
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
        for col in 0..cols {
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
                bg = vt100::Color::Rgb(0xBC, 0xDF, 0x9C);
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
        if row + 1 < rows {
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

/// One shell: its pty, its reader thread, its scrollback and its screen.
///
/// Everything the terminal used to hold directly lives here now, so the panel
/// can hold several at once and switch between them. The methods that touch
/// the parser or the pty live in the `impl Session` blocks below, split around
/// `impl Terminal`; that is why there is more than one of each.
struct Session {
    /// Stable handle. Tabs index `sessions` by position, but Flow Mode panes
    /// hold this id instead, so closing or adding a shell cannot strand a
    /// pane on the wrong session when the vec shifts.
    id: u64,
    /// Working directory the shell was spawned in. A pane created from
    /// another inherits its cwd, so each agent keeps its own project root
    /// and the focused pane can offer it back to the explorer.
    cwd: PathBuf,
    /// Tab label. The first shell is just "powershell"; later ones are
    /// numbered from 2, so a single tab never reads "powershell 1".
    title: String,
    /// Live terminal dimensions in cells. The pane on screen owns these:
    /// whenever its pixel rect implies different rows/columns,
    /// [`Session::apply_size`] resizes the parser *and* the real ConPTY, so
    /// full-screen TUIs reflow instead of being clipped. Start at the
    /// classic 80x24 until a pane measures them.
    rows: u16,
    cols: u16,
    parser: vt100::Parser,
    rx: Option<Receiver<Vec<u8>>>,
    writer: Option<Box<dyn Write + Send>>,
    _child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
    _master: Option<Box<dyn portable_pty::MasterPty + Send>>,
    error: Option<String>,
    running: bool,
    total_bytes: u64,
    total_chunks: u64,
    inq: Vec<u8>,
    started: bool,
}

impl Session {
    fn new(title: String, id: u64) -> Self {
        Self {
            id,
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            title,
            parser: vt100::Parser::new(ROWS, COLS, SCROLLBACK),
            rx: None,
            writer: None,
            _child: None,
            _master: None,
            error: None,
            running: false,
            total_bytes: 0,
            total_chunks: 0,
            inq: Vec::new(),
            started: false,
            rows: ROWS,
            cols: COLS,
        }
    }

    /// Resize this shell to `rows` x `cols` cells. True when something
    /// changed: the vt100 screen is resized for rendering and the live
    /// ConPTY is resized through portable-pty so the child process (and
    /// any TUI it runs) learns the new size at once. The process itself is
    /// never touched — no restart, no new buffer. Cheap enough to call
    /// every frame: unchanged dimensions are a no-op.
    fn apply_size(&mut self, rows: u16, cols: u16) -> bool {
        if self.rows == rows && self.cols == cols {
            return false;
        }
        self.rows = rows;
        self.cols = cols;
        self.parser.screen_mut().set_size(rows, cols);
        if let Some(master) = self._master.as_ref() {
            let _ = master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
        true
    }
}

pub struct Terminal {
    sessions: Vec<Session>,
    /// Index into `sessions`. Meaningless while `sessions` is empty — closing
    /// the last tab takes the whole section away — so every access goes
    /// through `session()`/`session_mut()`, which are `Option`.
    active_tab: usize,
    pub collapsed: bool,
    pub fullscreen: bool,
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
    /// One-shot: scroll the tab strip so the active pill is visible.
    ///
    /// Set when the active tab changes (a new shell, or a click on another
    /// pill) and cleared the next frame. Doing it every frame instead would
    /// pin the strip and stop the user scrolling it by hand.
    reveal_active_tab: bool,
    /// Next stable [`Session`] id. Ids are never reused within a run, so a
    /// Flow Mode pane can hold one without fear of it silently pointing at
    /// a different shell later.
    next_session_id: u64,
    /// Flow Mode's pane grid. Kept across toggles so re-entering restores
    /// the arrangement and sizes instead of rebuilding them.
    flow_grid: FlowGrid,
    /// Session id with keyboard focus in Flow Mode. `None` means "first
    /// pane"; always resolved through live sessions, never trusted blind.
    flow_focus: Option<u64>,
    /// Why the last shell creation refused (the 4-shell cap). Shown subtly
    /// in the tab strip and the flow area; cleared by the next success.
    notice: Option<String>,
}

impl Terminal {
    pub fn new() -> Self {
        Self {
            sessions: vec![Session::new(shell_title(1), 1)],
            active_tab: 0,
            collapsed: false,
            fullscreen: false,
            active: false,
            hidden: false,
            term_h: 280.0,
            focus_clear: true,
            reveal_active_tab: false,
            next_session_id: 2,
            flow_grid: FlowGrid::default(),
            flow_focus: None,
            notice: None,
        }
    }

    /// Title for the next shell: the lowest number not already on screen.
    ///
    /// Deliberately not a monotonic counter. That read badly the moment tabs
    /// were closed: emptying the panel and opening a shell again produced
    /// "powershell 6", "powershell 7" and so on, a sequence that depends on
    /// history the user cannot see. Counting the free slot instead keeps the
    /// labels unique and restarts at "powershell" once the panel is empty.
    fn next_free_title(&self) -> String {
        let mut n = 1;
        loop {
            let candidate = shell_title(n);
            if !self.sessions.iter().any(|s| s.title == candidate) {
                return candidate;
            }
            n += 1;
        }
    }

    /// Open another shell and switch to it. Refuses past [`MAX_SESSIONS`]
    /// with a `notice`, leaving every live shell alone. Returns the new
    /// session's id so Flow Mode can place it in a pane.
    fn new_tab(&mut self, cwd: &PathBuf) -> Option<u64> {
        if self.sessions.len() >= MAX_SESSIONS {
            self.notice = Some(format!(
                "at the {MAX_SESSIONS}-terminal limit — close one first"
            ));
            return None;
        }
        self.notice = None;
        let id = self.next_session_id;
        self.next_session_id += 1;
        let mut session = Session::new(self.next_free_title(), id);
        session.cwd = cwd.clone();
        session.started = true;
        session.spawn(cwd);
        self.sessions.push(session);
        self.active_tab = self.sessions.len() - 1;
        self.active = true;
        // A new shell you cannot see is not a new shell.
        self.collapsed = false;
        self.reveal_active_tab = true;
        Some(id)
    }

    /// Append a shell without opening a real pty, so the tab bookkeeping can
    /// be tested hermetically. Everything except the `spawn` call is the same
    /// as `new_tab`, and the title comes from the same `next_free_title`, so a
    /// numbering change cannot pass the tests while breaking the app.
    #[cfg(test)]
    fn open_stub_tab(&mut self) -> Option<u64> {
        if self.sessions.len() >= MAX_SESSIONS {
            self.notice = Some(format!(
                "at the {MAX_SESSIONS}-terminal limit — close one first"
            ));
            return None;
        }
        self.notice = None;
        let id = self.next_session_id;
        self.next_session_id += 1;
        self.sessions.push(Session::new(self.next_free_title(), id));
        self.active_tab = self.sessions.len() - 1;
        self.active = true;
        self.collapsed = false;
        self.reveal_active_tab = true;
        Some(id)
    }

    /// Close a tab.
    ///
    /// The last one may be closed. That empties the panel and takes the whole
    /// terminal section away (`hidden`); `reveal` brings it back with a fresh
    /// shell. There is no "a shell must always exist" rule on purpose — the
    /// terminal is optional, and Ctrl+Tab is how it comes back.
    fn close_tab(&mut self, index: usize) {
        if index >= self.sessions.len() {
            return;
        }
        self.sessions.remove(index);
        if self.sessions.is_empty() {
            self.active_tab = 0;
            self.active = false;
            self.hidden = true;
            return;
        }
        self.active_tab = self.active_tab.min(self.sessions.len() - 1);
    }

    /// Bring the panel back with a shell in it.
    ///
    /// Closing the last tab takes the section away, so anything that wants a
    /// terminal — Ctrl+Tab, the status-bar toggle, the Run button — has to be
    /// able to ask for one rather than assume it exists.
    pub fn reveal(&mut self, cwd: &PathBuf) {
        self.hidden = false;
        self.collapsed = false;
        if self.sessions.is_empty() {
            self.new_tab(cwd);
        }
    }

    /// Start the active tab's shell if it is still lazy. Each session
    /// remembers the directory it was created in, so a shell always starts
    /// where its tab or pane expects it — even if the workspace root moved
    /// on since.
    pub fn ensure_started(&mut self) {
        let i = self.active_tab;
        if let Some(session) = self.sessions.get_mut(i)
            && !session.started
        {
            session.started = true;
            let cwd = session.cwd.clone();
            session.spawn(&cwd);
        }
    }

    /// Start every unstarted shell. Flow Mode shows several sessions at
    /// once, so one tab's laziness cannot gate the others.
    pub fn ensure_started_all(&mut self) {
        for session in &mut self.sessions {
            if !session.started {
                session.started = true;
                let cwd = session.cwd.clone();
                session.spawn(&cwd);
            }
        }
    }

    /// Session id backing `index`, if any. Lets Flow Mode translate a pane
    /// (which holds ids) into the tab strip's positional world.
    fn id_at(&self, index: usize) -> Option<u64> {
        self.sessions.get(index).map(|s| s.id)
    }

    /// Position of the session holding `id`, if it still exists.
    fn index_of(&self, id: u64) -> Option<usize> {
        self.sessions.iter().position(|s| s.id == id)
    }

    /// Make `id` the visible tab (normal mode). Used when leaving Flow
    /// Mode so the tab strip lands on the pane that had focus.
    pub fn focus_session(&mut self, id: u64) {
        if let Some(i) = self.index_of(id) {
            self.active_tab = i;
            self.active = true;
            self.reveal_active_tab = true;
        }
    }

    /// Drain every session, not just the visible one.
    ///
    /// A background shell still answers prompts and still writes to its pty;
    /// leaving its channel unread would grow it without bound, and the tab
    /// would come back to the wrong screen when you switched to it.
    pub fn poll(&mut self) {
        for session in &mut self.sessions {
            session.poll();
        }
    }
}

impl Session {
    fn spawn(&mut self, cwd: &PathBuf) {
        let pty_system = native_pty_system();
        let pair = match pty_system.openpty(PtySize {
            rows: self.rows,
            cols: self.cols,
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
}

impl Terminal {
    fn session(&self) -> Option<&Session> {
        self.sessions.get(self.active_tab)
    }

    fn session_mut(&mut self) -> Option<&mut Session> {
        self.sessions.get_mut(self.active_tab)
    }

    /// Forward bytes to the active shell.
    fn send_bytes(&mut self, bytes: &[u8]) {
        if let Some(session) = self.session_mut() {
            session.send_bytes(bytes);
        }
    }

    /// Send a full shell line (used by the editor Run button).
    ///
    /// Goes to the active shell: that is the one whose output the user is
    /// watching, and the one the Run button's result should land in. Spawns a
    /// shell first if the panel was emptied, so Run always has somewhere to go.
    pub fn send_line(&mut self, line: &str, cwd: &PathBuf) {
        if self.sessions.is_empty() {
            self.new_tab(cwd);
        }
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
}

impl Session {
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

    /// Take whatever this shell's reader thread has produced.
    fn poll(&mut self) {
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
}

impl Terminal {
    pub fn ui(&mut self, ui: &mut eframe::egui::Ui, cwd: &PathBuf) {
        // Closing the last tab takes the section away entirely, and `hidden`
        // is what stops `ui()` being called — but guard here too, so no future
        // call path can index an empty session list.
        if self.sessions.is_empty() {
            return;
        }
        self.ensure_started();
        self.poll();

        let is_active = self.active;
        let accent = crate::theme::accent();

        // Read what the header needs before opening the closure. Borrowing
        // `self.sessions` for the tab strip while the same closure also wants
        // `&mut self` for close/new does not work, and cloning a handful of
        // short titles per frame is cheaper than restructuring around it.
        let titles: Vec<String> = self.sessions.iter().map(|s| s.title.clone()).collect();
        let active_tab = self.active_tab;
        let error = self.session().and_then(|s| s.error.clone());
        let reveal_active_tab = self.reveal_active_tab;

        let mut switch_to: Option<usize> = None;
        let mut close_tab: Option<usize> = None;
        let mut open_tab = false;

        ui.horizontal(|ui| {
            // One pill per shell, in a row that scrolls once there are more
            // than fit — the same treatment the editor's file tabs get. The
            // reference has a single tab with a "+" beside it; this is that
            // row, able to hold more than one.
            //
            // The width is bounded rather than left to `available_width`
            // because the row shares its line with the right-hand controls:
            // an unbounded scroll area claims the whole line and pushes them
            // off it.
            eframe::egui::ScrollArea::horizontal()
                .id_salt("snor_term_tabs")
                .max_width((ui.available_width() - TERM_HDR_RIGHT_W).max(140.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (idx, title) in titles.iter().enumerate() {
                            let active = idx == active_tab;
                            let pill = eframe::egui::Frame::NONE
                                .fill(if active {
                                    crate::theme::tab_active()
                                } else {
                                    eframe::egui::Color32::TRANSPARENT
                                })
                                .stroke(if active {
                                    eframe::egui::Stroke::new(1.0, crate::theme::hairline())
                                } else {
                                    eframe::egui::Stroke::NONE
                                })
                                .corner_radius(6.0)
                                .inner_margin(eframe::egui::Margin::symmetric(8, 3))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            eframe::egui::RichText::new(">_")
                                                .size(12.0)
                                                .family(crate::theme::medium())
                                                .color(accent),
                                        );
                                        let label = eframe::egui::RichText::new(title)
                                            .size(12.5)
                                            .color(if active {
                                                crate::theme::text()
                                            } else {
                                                crate::theme::dim_text()
                                            });
                                        if ui
                                            .add(
                                                eframe::egui::Label::new(label)
                                                    .sense(eframe::egui::Sense::click()),
                                            )
                                            .clicked()
                                        {
                                            switch_to = Some(idx);
                                        }
                                        if crate::icons::icon_button(
                                            ui,
                                            16.0,
                                            "close terminal",
                                            crate::icons::close_x,
                                        )
                                        .clicked()
                                        {
                                            close_tab = Some(idx);
                                        }
                                    });
                                });
                            // Bring the active pill into view when the
                            // selection changed. Without this, opening a shell
                            // on a strip that is already full leaves the new
                            // tab clipped at the edge, so the shell you just
                            // asked for is the one you cannot see.
                            if active && reveal_active_tab {
                                pill.response
                                    .scroll_to_me(Some(eframe::egui::Align::Center));
                            }
                        }
                    });
                });
            // One-shot: consumed, so the strip is free to be scrolled by hand
            // from the next frame on.
            self.reveal_active_tab = false;
            // Outside the scroll area on purpose. Inside it, the "+" is laid
            // out after the last pill, so once there are more tabs than fit it
            // scrolls off the strip along with them and there is no longer any
            // way to open another one.
            if crate::icons::icon_button(ui, 20.0, "new terminal", crate::icons::plus).clicked() {
                open_tab = true;
            }

            // Applied after the strip is built, so the list is not mutated
            // while it is being iterated.
            if let Some(i) = switch_to {
                self.active_tab = i;
                self.active = true;
                self.reveal_active_tab = true;
            }
            if let Some(i) = close_tab {
                self.close_tab(i);
            }
            if open_tab {
                self.new_tab(cwd);
            }

            // A refused shell (the 4-terminal cap) says so here, small and
            // out of the way, while every live shell keeps running.
            if let Some(note) = &self.notice {
                ui.label(
                    eframe::egui::RichText::new(note)
                        .size(11.0)
                        .color(crate::theme::faint()),
                );
            }
            if is_active {
                // Painted, not typed: egui's bundled fonts have no
                // dependable bullet coverage, and a missing glyph renders
                // as tofu.
                let (slot, _) = ui.allocate_exact_size(
                    eframe::egui::vec2(12.0, 12.0),
                    eframe::egui::Sense::hover(),
                );
                ui.painter_at(slot)
                    .circle_filled(slot.center(), 3.5, accent);
            } else {
                ui.label(
                    eframe::egui::RichText::new("click to type")
                        .size(11.5)
                        .color(crate::theme::faint()),
                );
            }
            ui.with_layout(
                eframe::egui::Layout::right_to_left(eframe::egui::Align::Center),
                |ui| {
                    if crate::icons::icon_button(ui, 20.0, "clear screen", |p, r, c| {
                        crate::icons::trash(p, r, c)
                    })
                    .clicked()
                        && let Some(session) = self.session_mut()
                    {
                        let (rows, cols) = (session.rows, session.cols);
                        session.parser = vt100::Parser::new(rows, cols, SCROLLBACK);
                    }
                    let fullscreen = self.fullscreen;
                    if crate::icons::icon_button(
                        ui,
                        20.0,
                        if fullscreen {
                            "restore terminal"
                        } else {
                            "maximize terminal"
                        },
                        |p, r, c| crate::icons::maximize(p, r, c, fullscreen),
                    )
                    .clicked()
                    {
                        self.fullscreen = !self.fullscreen;
                        if self.fullscreen {
                            self.collapsed = false;
                        }
                    }
                    // An icon, not a `small_button`: the reference's terminal
                    // header is icons only, and a default-chrome text button
                    // between two flat glyphs read as bolted on.
                    if crate::icons::icon_button(ui, 20.0, "restart powershell", |p, r, c| {
                        crate::icons::refresh(p, r.shrink(2.0), c)
                    })
                    .clicked()
                    {
                        // Restart this shell only. The other tabs keep their
                        // own ptys and their own scrollback.
                        if let Some(session) = self.session_mut() {
                            session.started = false;
                            session.rx = None;
                            session.writer = None;
                            session._child = None;
                            session._master = None;
                            let (rows, cols) = (session.rows, session.cols);
                            session.parser = vt100::Parser::new(rows, cols, SCROLLBACK);
                            session.error = None;
                        }
                        self.ensure_started();
                    }
                    // Shell picker look, like the reference (single shell).
                    eframe::egui::Frame::NONE
                        .fill(crate::theme::tab_active())
                        .stroke(eframe::egui::Stroke::new(1.0, crate::theme::hairline()))
                        .corner_radius(6.0)
                        .inner_margin(eframe::egui::Margin::symmetric(9, 3))
                        .show(ui, |ui| {
                            ui.label(
                                eframe::egui::RichText::new("powershell")
                                    .size(11.5)
                                    .color(crate::theme::dim_text()),
                            );
                        });
                    // Collapse / expand.
                    //
                    // Lives here, not beside the tab strip's "+": the two were
                    // adjacent, and while the panel was collapsed the toggle
                    // drew a "+" of its own, so the header showed two identical
                    // plus glyphs side by side. A chevron also states what the
                    // control does — it moves the panel edge — where a plus
                    // read as "add", which is the neighbouring button's job.
                    let collapsed = self.collapsed;
                    if crate::icons::icon_button(
                        ui,
                        20.0,
                        if collapsed {
                            "expand terminal"
                        } else {
                            "collapse terminal"
                        },
                        |p, r, c| crate::icons::chevron_v(p, r.shrink(5.0), !collapsed, c),
                    )
                    .clicked()
                    {
                        self.collapsed = !self.collapsed;
                    }
                },
            );
        });

        if let Some(err) = &error {
            ui.colored_label(crate::theme::danger(), err);
        }

        if self.collapsed {
            return;
        }

        ui.separator();

        let job = match self.session() {
            Some(session) => term_job(session.parser.screen()),
            None => return,
        };
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
        // Size the live shell to the grid it is about to paint: rows and
        // columns come from these pixels, so window resizes reflow the
        // ConPTY instead of clipping it. Unchanged dimensions are a no-op.
        let cell = term_cell_size(ui);
        let (rows, cols) = term_size_for_pixels(ui.available_width().max(0.0), grid_max, cell);
        if let Some(session) = self.session_mut() {
            session.apply_size(rows, cols);
        }
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

        // Type-directly-in-terminal is shared with Flow Mode panes: the
        // event loop lives in `forward_events`, with `None` targeting the
        // active tab here and a session id targeting the focused pane there.
        self.forward_events(ui, None);
    }
}

impl Terminal {
    /// Forward bytes to `target`, or to the active tab when `None`.
    fn send_bytes_to(&mut self, target: Option<u64>, bytes: &[u8]) {
        match target {
            Some(id) => {
                if let Some(session) = self.sessions.iter_mut().find(|s| s.id == id) {
                    session.send_bytes(bytes);
                }
            }
            None => self.send_bytes(bytes),
        }
    }

    /// Type-directly-in-terminal: while latched active and no real widget
    /// (editor, find box, explorer field) owns the keyboard, printable
    /// text + paste + special keys go straight to the pty.
    ///
    /// `target` selects the shell: `None` is the active tab (normal mode),
    /// `Some(id)` the focused Flow Mode pane. Either way the same latch,
    /// focus-theft surrender and app-shortcut guards apply, so typing can
    /// never leak between shells or into the editor.
    fn forward_events(&mut self, ui: &mut eframe::egui::Ui, target: Option<u64>) {
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
                        self.send_bytes_to(target, text.as_bytes());
                    }
                    Event::Paste(text) => {
                        self.send_bytes_to(target, text.as_bytes());
                    }
                    Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        // Never steal editor save / find, and never steal Dim
                        // Mode: Ctrl+Shift+D is an app toggle handled in
                        // `SnorApp::ui` before the terminal sees the key. Plain
                        // Ctrl+D, which the shell needs for EOF, has no Shift
                        // and still forwards.
                        if matches!(key, Key::S | Key::F) && (modifiers.ctrl || modifiers.command) {
                            continue;
                        }
                        if matches!(key, Key::D)
                            && modifiers.shift
                            && (modifiers.ctrl || modifiers.command)
                        {
                            continue;
                        }
                        if let Some(bytes) = Self::key_to_bytes(*key, *modifiers) {
                            self.send_bytes_to(target, bytes);
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

// --- Flow Mode -----------------------------------------------------------
//
// Flow Mode is a presentation state over the sessions above: a recursive
// layout tree of panes (each showing one live session) and draggable
// splits. It never spawns, kills or recreates shells — entering, leaving,
// resizing, focusing and closing panes only rearrange rectangles and ids.
// A long-running agent cannot tell the mode changed: its `Session` (pty,
// reader thread, scrollback, cwd) is the same object throughout.

/// Flow Mode layout: an adaptive grid over live session ids.
///
/// The shape follows the pane count — 1 fills the window, 2 sit side by
/// side, 3 tile two over one full-width pane, 4 tile 2x2 — so the window is
/// always filled intelligently instead of growing one tall column. Column
/// widths and row heights are shared fractions (one vertical divider owns a
/// whole column's width, one horizontal divider a whole row's height),
/// kept across toggles and never forced back to even after a manual drag.
///
/// Nothing here owns a shell — cells borrow sessions by id — so the grid
/// can be reshaped freely without touching a single process.
#[derive(Debug, Clone, Default)]
pub struct FlowGrid {
    /// Pane session ids in focus-cycle order: left to right, top to bottom.
    order: Vec<u64>,
    /// Column width fractions, summing to 1.
    col_w: Vec<f32>,
    /// Row height fractions, summing to 1.
    row_h: Vec<f32>,
}

/// Grab band between flow panes.
const FLOW_GAP: f32 = 6.0;

/// At most four live shells. Flow Mode's adaptive grid tops out at 2x2,
/// and four ConPTY agents are plenty for one window; the cap stops an
/// accidental cascade of shells. A refused creation leaves a `notice`
/// instead of failing silently, and never touches the live shells.
pub const MAX_SESSIONS: usize = 4;
/// A manual resize sticks: the grid never forces panes back to equal sizing
/// while its membership is unchanged. Fractions below this would strand a
/// pane too small to grab back.
const FLOW_FRAC_MIN: f32 = 0.12;

/// Columns per row for `n` panes: the adaptive tiling. One pane fills the
/// window, two sit side by side, three tile two over one full-width pane,
/// four tile 2x2. Pure, so the tiling stays testable without a window.
fn flow_shape(n: usize) -> Vec<usize> {
    match n {
        0 => vec![],
        1 => vec![1],
        2 => vec![2],
        3 => vec![2, 1],
        _ => vec![2, 2],
    }
}

impl FlowGrid {
    fn len(&self) -> usize {
        self.order.len()
    }

    fn contains(&self, id: u64) -> bool {
        self.order.contains(&id)
    }

    /// Adopt `ids` as the pane set: survivors keep their slots (and every
    /// manual size), newcomers append, the dead drop out. Fractions reset
    /// to even only when the shape changes — adding or closing a pane
    /// reflows, while focusing or toggling never touches a size.
    fn set_panes(&mut self, ids: &[u64]) {
        let before = flow_shape(self.order.len());
        let mut order: Vec<u64> = self
            .order
            .iter()
            .copied()
            .filter(|id| ids.contains(id))
            .collect();
        for id in ids.iter().copied() {
            if !order.contains(&id) {
                order.push(id);
            }
        }
        self.order = order;
        if flow_shape(self.order.len()) != before {
            self.even_out();
        }
        self.normalize();
    }

    /// Even fractions for the current shape.
    fn even_out(&mut self) {
        let shape = flow_shape(self.order.len());
        let ncols = shape.iter().copied().max().unwrap_or(1).max(1);
        self.col_w = vec![1.0 / ncols as f32; ncols];
        self.row_h = vec![1.0 / shape.len().max(1) as f32; shape.len().max(1)];
    }

    /// Renormalize after drags so float rounding cannot drift the totals.
    fn normalize(&mut self) {
        let sum_w: f32 = self.col_w.iter().sum();
        if sum_w > 0.0 {
            for w in &mut self.col_w {
                *w /= sum_w;
            }
        }
        let sum_h: f32 = self.row_h.iter().sum();
        if sum_h > 0.0 {
            for h in &mut self.row_h {
                *h /= sum_h;
            }
        }
    }

    /// Drag the divider after column `i` by `dx` pixels of `total_w`. The
    /// two neighbours trade width; every other column is untouched and the
    /// pair keeps its sum, so one drag cannot wreck the layout.
    fn drag_col(&mut self, i: usize, dx: f32, total_w: f32) {
        if i + 1 >= self.col_w.len() || total_w <= 0.0 {
            return;
        }
        let pair = self.col_w[i] + self.col_w[i + 1];
        let a = (self.col_w[i] + dx / total_w).clamp(FLOW_FRAC_MIN, pair - FLOW_FRAC_MIN);
        if a < FLOW_FRAC_MIN || a > pair - FLOW_FRAC_MIN {
            return;
        }
        self.col_w[i] = a;
        self.col_w[i + 1] = pair - a;
    }

    /// Drag the divider after row `r` by `dy` pixels of `total_h`. Same
    /// trade as [`FlowGrid::drag_col`], vertically.
    fn drag_row(&mut self, r: usize, dy: f32, total_h: f32) {
        if r + 1 >= self.row_h.len() || total_h <= 0.0 {
            return;
        }
        let pair = self.row_h[r] + self.row_h[r + 1];
        let a = (self.row_h[r] + dy / total_h).clamp(FLOW_FRAC_MIN, pair - FLOW_FRAC_MIN);
        if a < FLOW_FRAC_MIN || a > pair - FLOW_FRAC_MIN {
            return;
        }
        self.row_h[r] = a;
        self.row_h[r + 1] = pair - a;
    }

    /// Session id at (`row`, `col`), if a pane lives there.
    fn cell_at(&self, row: usize, col: usize) -> Option<u64> {
        let shape = flow_shape(self.order.len());
        let start: usize = shape.iter().take(row).sum();
        self.order.get(start + col).copied()
    }

    /// Pixel rect of the cell at (`row`, `col`) inside `area`. A lone cell
    /// in a short row (the third pane) spans the full width. Pure geometry,
    /// so pane math stays testable without a window.
    fn cell_rect(&self, area: eframe::egui::Rect, row: usize, col: usize) -> eframe::egui::Rect {
        use eframe::egui::{Rect, pos2};
        let shape = flow_shape(self.order.len());
        let rows = self.row_h.len().max(1);
        let y0 = area.top() + area.height() * self.row_h.iter().take(row.min(rows)).sum::<f32>();
        let y1 =
            area.top() + area.height() * self.row_h.iter().take((row + 1).min(rows)).sum::<f32>();
        let (x0, x1) = if shape.get(row).copied().unwrap_or(0) <= 1 {
            (area.left(), area.right())
        } else {
            let cols = self.col_w.len().max(1);
            let x0 =
                area.left() + area.width() * self.col_w.iter().take(col.min(cols)).sum::<f32>();
            let x1 = area.left()
                + area.width() * self.col_w.iter().take((col + 1).min(cols)).sum::<f32>();
            (x0, x1)
        };
        Rect::from_min_max(pos2(x0, y0), pos2(x1, y1))
    }

    /// Grab band for the divider after column `i`. It spans only rows that
    /// actually split there — in the 3-pane layout the vertical divider
    /// stops where the full-width pane begins.
    fn col_div_rect(&self, area: eframe::egui::Rect, i: usize) -> Option<eframe::egui::Rect> {
        use eframe::egui::{Rect, pos2};
        if i + 1 >= self.col_w.len() {
            return None;
        }
        let shape = flow_shape(self.order.len());
        let x = area.left() + area.width() * self.col_w.iter().take(i + 1).sum::<f32>();
        let mut span: Option<(f32, f32)> = None;
        for (r, &count) in shape.iter().enumerate() {
            if count > i + 1 {
                let rows = self.row_h.len().max(1);
                let y0 =
                    area.top() + area.height() * self.row_h.iter().take(r.min(rows)).sum::<f32>();
                let y1 = area.top()
                    + area.height() * self.row_h.iter().take((r + 1).min(rows)).sum::<f32>();
                span = Some((span.map(|s| s.0).unwrap_or(y0), y1));
            }
        }
        span.map(|(y0, y1)| {
            Rect::from_min_max(pos2(x - FLOW_GAP * 0.5, y0), pos2(x + FLOW_GAP * 0.5, y1))
        })
    }

    /// Grab band for the divider after row `r`: always full width, since
    /// every multi-row shape stacks full-width bands.
    fn row_div_rect(&self, area: eframe::egui::Rect, r: usize) -> Option<eframe::egui::Rect> {
        use eframe::egui::{Rect, pos2};
        if r + 1 >= self.row_h.len() {
            return None;
        }
        let rows = self.row_h.len().max(1);
        let y = area.top() + area.height() * self.row_h.iter().take((r + 1).min(rows)).sum::<f32>();
        Some(Rect::from_min_max(
            pos2(area.left(), y - FLOW_GAP * 0.5),
            pos2(area.right(), y + FLOW_GAP * 0.5),
        ))
    }
}

/// Face every terminal grid renders in, measured and painted from the same
/// constant so the two can never disagree about how big a cell is.
const TERM_FONT_SIZE: f32 = 12.5;

/// Pixels per terminal cell, measured off the live font. Panes divide their
/// pixel rects by these to learn their real rows and columns.
fn term_cell_size(ui: &eframe::egui::Ui) -> (f32, f32) {
    use eframe::egui::{Color32, FontId};
    let mono = FontId::monospace(TERM_FONT_SIZE);
    let w = ui
        .painter()
        .layout_no_wrap("MMMMMMMMMM".to_owned(), mono.clone(), Color32::WHITE)
        .size()
        .x
        / 10.0;
    let h = ui.ctx().fonts_mut(|f| f.row_height(&mono));
    (w.max(1.0), h.max(1.0))
}

/// Smallest shell worth sizing: below this a TUI cannot lay out.
const MIN_TERM_COLS: u16 = 20;
const MIN_TERM_ROWS: u16 = 6;
/// Largest shell worth sizing: above this ConPTY and vt100 just burn RAM.
const MAX_TERM_COLS: u16 = 400;
const MAX_TERM_ROWS: u16 = 200;

/// Rows and columns for a `width` x `height` pixel pane measured in `cell
/// pixels. Pure, so pane math stays testable without a window.
fn term_size_for_pixels(width: f32, height: f32, cell: (f32, f32)) -> (u16, u16) {
    let cols = (width / cell.0).floor() as u16;
    let rows = (height / cell.1).floor() as u16;
    (
        rows.clamp(MIN_TERM_ROWS, MAX_TERM_ROWS),
        cols.clamp(MIN_TERM_COLS, MAX_TERM_COLS),
    )
}

/// Last ~32 chars of a working directory for pane headers. Char-wise, so
/// multi-byte paths cannot panic the slice.
fn short_cwd(path: &std::path::Path) -> String {
    const KEEP: usize = 32;
    let text = path.to_string_lossy();
    let n = text.chars().count();
    if n <= KEEP {
        text.into_owned()
    } else {
        let tail: String = text.chars().skip(n - (KEEP - 1)).collect();
        format!("...{tail}")
    }
}

impl Terminal {
    /// Session id with keyboard focus in Flow Mode: the pinned focus while
    /// it is still on screen and alive, else the first live pane.
    pub fn flow_target_id(&self) -> Option<u64> {
        if let Some(id) = self.flow_focus
            && self.flow_grid.contains(id)
            && self.index_of(id).is_some()
        {
            return Some(id);
        }
        self.flow_grid
            .order
            .iter()
            .copied()
            .find(|id| self.index_of(*id).is_some())
    }

    /// Working directory of the focused flow pane, for the explorer to adopt
    /// when the user returns to normal mode.
    pub fn flow_focused_cwd(&self) -> Option<PathBuf> {
        let id = self.flow_target_id()?;
        self.sessions
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.cwd.clone())
    }

    /// Enter Flow Mode: reconcile the kept grid with live sessions. Dead
    /// panes drop out, shells with no pane append, and an untouched session
    /// set keeps every manual size.
    pub fn flow_enter(&mut self, default_cwd: &PathBuf) {
        if self.sessions.is_empty() {
            let _ = self.new_tab(default_cwd);
        }
        let live: Vec<u64> = self.sessions.iter().map(|s| s.id).collect();
        self.flow_grid.set_panes(&live);
        // Continuity: focus follows the normal-mode tab when it is on
        // screen, so entering Flow Mode does not yank focus elsewhere.
        if let Some(current) = self.id_at(self.active_tab)
            && self.flow_grid.contains(current)
        {
            self.flow_focus = Some(current);
        }
    }

    /// Add a pane with a fresh shell beside the focused one. The new shell
    /// inherits the focused pane's directory, so each agent keeps its own
    /// project root. Never mirrors a buffer: every pane owns its shell.
    /// Refuses past [`MAX_SESSIONS`] with a `notice`, harming nothing.
    pub fn flow_add_pane(&mut self, default_cwd: &PathBuf) {
        // Guarantees a reconciled grid; a no-op when it is already valid.
        self.flow_enter(default_cwd);
        if self.sessions.len() >= MAX_SESSIONS {
            self.notice = Some(format!(
                "at the {MAX_SESSIONS}-terminal limit — close one first"
            ));
            return;
        }
        let anchor = self.flow_target_id();
        let cwd = anchor
            .and_then(|a| {
                self.sessions
                    .iter()
                    .find(|s| s.id == a)
                    .map(|s| s.cwd.clone())
            })
            .unwrap_or_else(|| default_cwd.clone());
        let id = self.next_session_id;
        self.next_session_id += 1;
        let mut session = Session::new(self.next_free_title(), id);
        session.cwd = cwd.clone();
        session.started = true;
        session.spawn(&cwd);
        self.sessions.push(session);
        self.notice = None;
        let live: Vec<u64> = self.sessions.iter().map(|s| s.id).collect();
        self.flow_grid.set_panes(&live);
        self.flow_focus = Some(id);
        self.active = true;
    }

    /// Hermetic twin of [`Terminal::flow_add_pane`] for tests: same
    /// placement, cwd inheritance and focus logic, no pty.
    #[cfg(test)]
    fn flow_add_stub(&mut self) {
        if self.sessions.is_empty() {
            self.open_stub_tab();
        }
        self.flow_enter(&PathBuf::from("."));
        if self.sessions.len() >= MAX_SESSIONS {
            self.notice = Some(format!(
                "at the {MAX_SESSIONS}-terminal limit — close one first"
            ));
            return;
        }
        let anchor = self.flow_target_id();
        let id = match self.open_stub_tab() {
            Some(id) => id,
            None => return,
        };
        // Mirror the real path: the new pane inherits the anchor's
        // directory rather than the test runner's.
        if let Some(cwd) = anchor.and_then(|a| {
            self.sessions
                .iter()
                .find(|s| s.id == a)
                .map(|s| s.cwd.clone())
        }) && let Some(next) = self.sessions.iter_mut().find(|s| s.id == id)
        {
            next.cwd = cwd;
        }
        self.notice = None;
        let live: Vec<u64> = self.sessions.iter().map(|s| s.id).collect();
        self.flow_grid.set_panes(&live);
        self.flow_focus = Some(id);
        self.active = true;
    }

    /// Close the focused pane. The shell closes exactly the way closing its
    /// tab would — same `close_tab` path, no special process handling — and
    /// the grid reflows the survivors into the freed space.
    pub fn flow_close_focused(&mut self) {
        let Some(id) = self.flow_target_id() else {
            return;
        };
        let order: Vec<u64> = self
            .flow_grid
            .order
            .iter()
            .copied()
            .filter(|o| *o != id)
            .collect();
        self.flow_grid.set_panes(&order);
        if let Some(i) = self.index_of(id) {
            self.close_tab(i);
        }
        if self.flow_focus == Some(id) {
            self.flow_focus = self.flow_target_id();
        }
    }

    /// Move focus between panes in grid order, wrapping around.
    pub fn flow_step_focus(&mut self, delta: isize) {
        let panes: Vec<u64> = self
            .flow_grid
            .order
            .iter()
            .copied()
            .filter(|id| self.index_of(*id).is_some())
            .collect();
        if panes.is_empty() {
            return;
        }
        let cur = self
            .flow_target_id()
            .and_then(|id| panes.iter().position(|l| *l == id))
            .unwrap_or(0) as isize;
        self.flow_focus = Some(panes[(cur + delta).rem_euclid(panes.len() as isize) as usize]);
        self.active = true;
    }

    /// Leave Flow Mode: land the tab strip on the focused pane. The layout
    /// itself is kept, so toggling back restores splits and sizes.
    pub fn flow_exit_sync(&mut self) {
        if let Some(id) = self.flow_target_id() {
            self.focus_session(id);
        }
    }

    /// Flow Mode workspace: every on-screen session at once, over essentially
    /// the whole window. A respawned single pane covers the only empty case
    /// (the last pane closed with it), so this never draws a dead end.
    pub fn flow_ui(&mut self, ui: &mut eframe::egui::Ui, default_cwd: &PathBuf) {
        self.ensure_started_all();
        self.poll();
        if self.sessions.is_empty() {
            let _ = self.new_tab(default_cwd);
        }
        let live: Vec<u64> = self.sessions.iter().map(|s| s.id).collect();
        self.flow_grid.set_panes(&live);
        let rect = ui.available_rect_before_wrap();
        // The recessed slab behind everything, like the normal terminal, so
        // the panes read as one continuous workspace.
        ui.painter()
            .rect_filled(rect, 0.0, crate::theme::surface_recessed());
        // Split borrows: the grid and the sessions travel side by side into
        // the renderer instead of fighting over `&mut self`.
        let cell = term_cell_size(ui);
        let Self {
            sessions,
            flow_grid,
            flow_focus,
            active,
            ..
        } = self;
        // The view borrows grid and sessions side by side; the scope ends
        // those borrows before input handling needs `self`.
        {
            let mut view = FlowView {
                grid: flow_grid,
                sessions,
                focus: flow_focus,
                latched: active,
                cell,
            };
            view.render(ui, rect);
        }
        // A refused creation says so here, small and out of the way, while
        // every live shell keeps running underneath.
        if let Some(note) = &self.notice {
            ui.painter().text(
                rect.min + eframe::egui::vec2(8.0, 2.0),
                eframe::egui::Align2::LEFT_TOP,
                note,
                eframe::egui::FontId::proportional(11.0),
                crate::theme::faint(),
            );
        }
        let target = self.flow_target_id();
        self.forward_events(ui, target);
    }
}

/// The mutable pieces [`Terminal::flow_ui`] threads through the grid:
/// the layout to draw and drag, sessions to draw into it, focus to read
/// and set, the click-to-type latch, and measured cell pixels for sizing.
struct FlowView<'a> {
    grid: &'a mut FlowGrid,
    sessions: &'a mut Vec<Session>,
    focus: &'a mut Option<u64>,
    latched: &'a mut bool,
    cell: (f32, f32),
}

impl<'a> FlowView<'a> {
    /// Draw every pane, then every divider. Column dividers own widths,
    /// row dividers own heights; each drag trades space between its two
    /// neighbours only, so one drag can never wreck the layout.
    fn render(&mut self, ui: &mut eframe::egui::Ui, area: eframe::egui::Rect) {
        let shape = flow_shape(self.grid.len());
        for (r, &count) in shape.iter().enumerate() {
            for c in 0..count {
                if let Some(id) = self.grid.cell_at(r, c) {
                    let rect = self.grid.cell_rect(area, r, c);
                    self.render_pane(ui, id, rect);
                }
            }
        }
        for i in 0..self.grid.col_w.len().saturating_sub(1) {
            let band = self.grid.col_div_rect(area, i);
            if let Some(band) = band {
                let resp = ui.interact(
                    band,
                    eframe::egui::Id::new(("snor_flow_col", i)),
                    eframe::egui::Sense::drag(),
                );
                if resp.dragged() {
                    // Per-frame deltas accumulate onto the fractions, the
                    // way the explorer grip accumulates onto its width.
                    self.grid.drag_col(i, resp.drag_delta().x, area.width());
                }
                self.paint_divider(ui, band, true, &resp);
                resp.on_hover_cursor(eframe::egui::CursorIcon::ResizeHorizontal);
            }
        }
        for r in 0..self.grid.row_h.len().saturating_sub(1) {
            let band = self.grid.row_div_rect(area, r);
            if let Some(band) = band {
                let resp = ui.interact(
                    band,
                    eframe::egui::Id::new(("snor_flow_row", r)),
                    eframe::egui::Sense::drag(),
                );
                if resp.dragged() {
                    self.grid.drag_row(r, resp.drag_delta().y, area.height());
                }
                self.paint_divider(ui, band, false, &resp);
                resp.on_hover_cursor(eframe::egui::CursorIcon::ResizeVertical);
            }
        }
    }

    fn paint_divider(
        &self,
        ui: &mut eframe::egui::Ui,
        band: eframe::egui::Rect,
        vertical: bool,
        resp: &eframe::egui::Response,
    ) {
        let (tint, width) = if resp.dragged() {
            (crate::theme::accent(), 2.0)
        } else if resp.hovered() {
            (crate::theme::text(), 2.0)
        } else {
            (crate::theme::hairline(), 1.0)
        };
        let stroke = eframe::egui::Stroke::new(width, tint);
        if vertical {
            ui.painter().vline(band.center().x, band.y_range(), stroke);
        } else {
            ui.painter().hline(band.x_range(), band.center().y, stroke);
        }
    }

    /// One pane: a small header (focus dot, title, directory) over the
    /// shell's own grid. The grid is the click target — clicking focuses the
    /// pane and latches typing, exactly like the normal terminal.
    fn render_pane(&mut self, ui: &mut eframe::egui::Ui, id: u64, rect: eframe::egui::Rect) {
        use eframe::egui::{Align, Layout};
        let focused = *self.focus == Some(id);
        // Snapshot the header first; the borrow ends before any widget.
        let (title, cwd_name, error) = match self.sessions.iter().find(|s| s.id == id) {
            Some(s) => (s.title.clone(), short_cwd(&s.cwd), s.error.clone()),
            None => return,
        };
        ui.scope_builder(
            eframe::egui::UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::top_down(Align::LEFT)),
            |ui| {
                ui.horizontal(|ui| {
                    if focused {
                        // Painted, not typed: egui's bundled fonts have no
                        // dependable bullet coverage, and a missing glyph
                        // renders as tofu. Same treatment as the tab strip.
                        let (slot, _) = ui.allocate_exact_size(
                            eframe::egui::vec2(10.0, 10.0),
                            eframe::egui::Sense::hover(),
                        );
                        ui.painter_at(slot).circle_filled(
                            slot.center(),
                            3.5,
                            crate::theme::accent(),
                        );
                    }
                    ui.label(
                        eframe::egui::RichText::new(title)
                            .size(12.0)
                            .color(if focused {
                                crate::theme::text()
                            } else {
                                crate::theme::dim_text()
                            }),
                    );
                    ui.label(
                        eframe::egui::RichText::new(cwd_name)
                            .size(11.0)
                            .color(crate::theme::faint()),
                    );
                });
                if let Some(err) = error {
                    ui.colored_label(crate::theme::danger(), err);
                }
                // Size the real shell to this pane before painting it: rows
                // and columns come from these very pixels, so the ConPTY
                // and the grid always agree and TUIs reflow instead of
                // clipping. Unchanged dimensions are a no-op inside.
                let avail = ui.available_size();
                let (rows, cols) =
                    term_size_for_pixels(avail.x.max(0.0), avail.y.max(0.0), self.cell);
                if let Some(s) = self.sessions.iter_mut().find(|s| s.id == id) {
                    s.apply_size(rows, cols);
                }
                let job = match self.sessions.iter().find(|s| s.id == id) {
                    Some(s) => term_job(s.parser.screen()),
                    None => return,
                };
                let grid_max = (ui.available_height() - 2.0).max(40.0);
                let mut clicked = false;
                eframe::egui::ScrollArea::vertical()
                    .id_salt(("snor_flow_grid", id))
                    .max_height(grid_max)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.push_id(("snor_flow_grid_label", id), |ui| {
                            let resp = ui.add(
                                eframe::egui::Label::new(job)
                                    .extend()
                                    .sense(eframe::egui::Sense::click()),
                            );
                            clicked = resp.clicked();
                        });
                    });
                if clicked {
                    *self.focus = Some(id);
                    *self.latched = true;
                }
            },
        );
        // The only focus chrome: a hairline that warms to a dimmed accent.
        // No glow, no banner — the shell text stays the loudest thing.
        let border = if focused {
            crate::theme::accent().gamma_multiply(0.45)
        } else {
            crate::theme::hairline()
        };
        ui.painter().rect_stroke(
            rect,
            4.0,
            eframe::egui::Stroke::new(1.0, border),
            eframe::egui::StrokeKind::Middle,
        );
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        // Every shell, not just the visible one: a background tab's
        // powershell would otherwise outlive the app that spawned it.
        for session in &mut self.sessions {
            if let Some(child) = session._child.as_mut() {
                let _ = child.kill();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Session, Terminal, flow_shape, term_size_for_pixels};
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
        sm(&mut t).respond_to_queries(b"\x1b[");
        assert!(!s(&t).inq.is_empty(), "partial sequence must be kept");
        sm(&mut t).respond_to_queries(b"6n");
        sm(&mut t).respond_to_queries(b"\x1b[?2026$p");
        sm(&mut t).respond_to_queries(b"\x1b[c");
        sm(&mut t).respond_to_queries(b"plain text without escapes");
        assert!(s(&t).inq.len() <= 64);
    }

    /// The active shell, for tests where "there is a shell" is the invariant.
    /// `session()` itself is `Option` because the panel can be emptied.
    fn s(t: &Terminal) -> &Session {
        t.session().expect("test expects a live session")
    }

    fn sm(t: &mut Terminal) -> &mut Session {
        t.session_mut().expect("test expects a live session")
    }

    /// Tabs are independent, and switching reads each shell's own screen.
    #[test]
    fn tabs_spawn_and_switch() {
        let mut t = Terminal::new();
        assert_eq!(t.sessions.len(), 1);
        assert_eq!(t.active_tab, 0);
        assert_eq!(s(&t).title, "powershell");

        // A second tab is titled "powershell 2", not "powershell 1", and
        // becomes active on creation.
        t.open_stub_tab();
        assert_eq!(t.sessions.len(), 2);
        assert_eq!(t.active_tab, 1);
        assert_eq!(s(&t).title, "powershell 2");

        // Switching back reads the first shell's own screen.
        t.active_tab = 0;
        assert_eq!(s(&t).title, "powershell");
    }

    /// Closing the last tab takes the whole section away, and `reveal` brings
    /// it back with a fresh shell. There is no "a shell must always exist"
    /// rule — the terminal is optional.
    #[test]
    fn closing_the_last_tab_hides_the_panel_and_reveal_restores_it() {
        let cwd = std::env::temp_dir();
        let mut t = Terminal::new();
        assert!(!t.hidden);

        t.close_tab(0);
        assert!(t.sessions.is_empty(), "the last shell must be closable");
        assert!(t.hidden, "an empty terminal takes the section away");
        assert!(!t.active, "no shell means nothing to type into");
        assert!(t.session().is_none(), "no session to hand out");

        // An out-of-range index on an empty panel is a no-op, not a panic.
        t.close_tab(9);
        assert!(t.sessions.is_empty());

        t.reveal(&cwd);
        assert!(!t.hidden);
        assert!(!t.collapsed);
        assert_eq!(t.sessions.len(), 1, "reveal must spawn a fresh shell");
        assert_eq!(s(&t).title, "powershell", "and restart the numbering");
    }

    /// Closing a tab *before* the active one must not shift the selection
    /// onto a different shell than the user was looking at.
    #[test]
    fn closing_an_earlier_tab_keeps_the_same_shell_selected() {
        let mut t = Terminal::new();
        t.open_stub_tab();
        t.open_stub_tab();
        t.active_tab = 2;
        assert_eq!(s(&t).title, "powershell 3");
        t.close_tab(0);
        // Index 2 clamped to the new last index — the same shell, now at 1.
        assert_eq!(t.sessions.len(), 2);
        assert_eq!(s(&t).title, "powershell 3");
    }

    /// Numbering takes the lowest free slot rather than counting ever upward.
    /// A monotonic counter produced "powershell 6", "powershell 7" after the
    /// panel had been emptied, a sequence the user cannot account for.
    #[test]
    fn tab_numbers_take_the_lowest_free_slot() {
        let mut t = Terminal::new();
        t.open_stub_tab();
        t.open_stub_tab();
        assert_eq!(titles(&t), ["powershell", "powershell 2", "powershell 3"]);

        // Closing a middle tab frees its number for the next shell, and no
        // label is ever duplicated.
        t.close_tab(1);
        t.open_stub_tab();
        assert_eq!(titles(&t), ["powershell", "powershell 3", "powershell 2"]);

        // Emptying the panel resets the sequence entirely.
        for i in (0..t.sessions.len()).rev() {
            t.close_tab(i);
        }
        assert!(t.sessions.is_empty());
        t.open_stub_tab();
        assert_eq!(titles(&t), ["powershell"], "numbering must restart");
    }

    fn titles(t: &Terminal) -> Vec<&str> {
        t.sessions.iter().map(|s| s.title.as_str()).collect()
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
        t.ensure_started();
        let t0 = Instant::now();
        while s(&t).total_bytes == 0
            && s(&t).error.is_none()
            && t0.elapsed() < Duration::from_secs(10)
        {
            std::thread::sleep(Duration::from_millis(50));
            t.poll();
        }
        assert!(s(&t).error.is_none(), "pty spawn failed: {:?}", s(&t).error);
        assert!(
            s(&t).total_bytes > 0,
            "powershell printed nothing in 10s; cannot verify input path"
        );
        t.send_bytes(b"echo SNORPTYOK123\r");
        let t1 = Instant::now();
        let mut seen = false;
        while t1.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(50));
            t.poll();
            if s(&t).parser.screen().contents().contains("SNORPTYOK123") {
                seen = true;
                break;
            }
        }
        assert!(
            seen,
            "typed line never echoed — shell not responding to pty input"
        );
    }

    // --- Flow Mode -----------------------------------------------------
    //
    // All hermetic: stub sessions never spawn a pty, so these prove the
    // layout and state logic without hardware. The live pty test above
    // proves shells actually run; Flow Mode never touches that path.

    fn enter(t: &mut Terminal) {
        let root = std::path::PathBuf::from(".");
        t.flow_enter(&root);
    }

    fn leaves(t: &Terminal) -> Vec<u64> {
        t.flow_grid.order.clone()
    }

    /// One terminal survives the round trip: same session, same tab, and —
    /// critically — never started by the mode change (stubs prove no spawn
    /// path runs; live shells prove the same by keeping their processes).
    #[test]
    fn flow_single_terminal_roundtrip() {
        let mut t = Terminal::new();
        assert_eq!(t.sessions.len(), 1);
        let id = s(&t).id;
        enter(&mut t);
        assert_eq!(leaves(&t), vec![id]);
        assert_eq!(t.flow_target_id(), Some(id));
        t.flow_exit_sync();
        assert_eq!(t.active_tab, 0);
        assert_eq!(s(&t).id, id, "the session must be the same object");
        assert!(!s(&t).started, "entering must not spawn anything");
    }

    /// Four shells tile into four panes; re-entering keeps them.
    #[test]
    fn flow_tiles_four_sessions() {
        let mut t = Terminal::new();
        t.open_stub_tab();
        t.open_stub_tab();
        t.open_stub_tab();
        enter(&mut t);
        assert_eq!(leaves(&t).len(), 4);
        enter(&mut t);
        assert_eq!(leaves(&t).len(), 4, "re-enter must not rebuild");
    }

    /// Splits add panes and closing reflows: survivors fill the freed
    /// space until one pane is left.
    #[test]
    fn flow_split_close_reflows() {
        let mut t = Terminal::new();
        enter(&mut t);
        t.flow_add_stub();
        t.flow_add_stub();
        assert_eq!(leaves(&t).len(), 3);
        t.flow_close_focused();
        assert_eq!(leaves(&t).len(), 2);
        t.flow_close_focused();
        assert_eq!(leaves(&t).len(), 1);
        assert_eq!(t.flow_grid.len(), 1);
    }

    /// Divider drags trade space between neighbours only: the pair keeps
    /// its sum, every other fraction is untouched, and clamps hold.
    #[test]
    fn flow_divider_drag_trades_neighbours() {
        let mut t = Terminal::new();
        t.open_stub_tab();
        t.open_stub_tab();
        t.open_stub_tab();
        enter(&mut t);
        assert_eq!(t.flow_grid.col_w, vec![0.5, 0.5]);
        t.flow_grid.drag_col(0, 100.0, 1000.0);
        assert!((t.flow_grid.col_w[0] - 0.6).abs() < 1e-6);
        assert!((t.flow_grid.col_w[1] - 0.4).abs() < 1e-6);
        assert!((t.flow_grid.col_w.iter().sum::<f32>() - 1.0).abs() < 1e-6);
        t.flow_grid.drag_row(0, 80.0, 800.0);
        assert!((t.flow_grid.row_h[0] - 0.6).abs() < 1e-6);
        assert!((t.flow_grid.row_h[1] - 0.4).abs() < 1e-6);
        // Clamps hold at the extremes: no ungrabbable slivers.
        t.flow_grid.drag_col(0, -10000.0, 1000.0);
        assert!(t.flow_grid.col_w[0] >= 0.12);
        assert!(t.flow_grid.col_w[1] >= 0.12);
        // Out-of-range dividers are no-ops, not panics.
        t.flow_grid.drag_col(9, 50.0, 1000.0);
        t.flow_grid.drag_row(9, 50.0, 800.0);
    }

    /// Focus cycles through panes in grid order and wraps both ways.
    #[test]
    fn flow_focus_steps_and_wraps() {
        let mut t = Terminal::new();
        enter(&mut t);
        t.flow_add_stub();
        t.flow_add_stub();
        let order = leaves(&t);
        assert_eq!(order.len(), 3);
        t.flow_focus = Some(order[0]);
        t.flow_step_focus(1);
        assert_eq!(t.flow_target_id(), Some(order[1]));
        t.flow_step_focus(1);
        assert_eq!(t.flow_target_id(), Some(order[2]));
        t.flow_step_focus(1);
        assert_eq!(t.flow_target_id(), Some(order[0]), "must wrap forward");
        t.flow_step_focus(-1);
        assert_eq!(t.flow_target_id(), Some(order[2]), "must wrap backward");
    }

    /// Leaving lands the tab strip on the focused pane's session.
    #[test]
    fn flow_exit_sync_lands_tab_on_focused_pane() {
        let mut t = Terminal::new();
        t.open_stub_tab();
        t.open_stub_tab();
        enter(&mut t);
        let order = leaves(&t);
        t.flow_focus = Some(order[2]);
        t.flow_exit_sync();
        assert_eq!(s(&t).id, order[2]);
    }

    /// Entering follows the normal-mode tab when it is on screen.
    #[test]
    fn flow_enter_follows_the_active_tab() {
        let mut t = Terminal::new();
        let second = t.open_stub_tab().expect("stub must open");
        t.active_tab = 0;
        enter(&mut t);
        assert_eq!(t.flow_target_id(), Some(s(&t).id));
        t.active_tab = 1;
        // A kept layout re-syncs focus to the tab on re-entry.
        enter(&mut t);
        assert_eq!(t.flow_target_id(), Some(second));
    }

    /// Dead shells are pruned, never drawn: closing a tab outside Flow
    /// Mode cannot leave a dangling pane behind.
    #[test]
    fn flow_prunes_dead_sessions() {
        let mut t = Terminal::new();
        t.open_stub_tab();
        enter(&mut t);
        assert_eq!(leaves(&t).len(), 2);
        t.close_tab(0);
        enter(&mut t);
        assert_eq!(leaves(&t).len(), 1);
        assert!(t.flow_target_id().is_some());
    }

    /// Shells opened outside Flow Mode (normal-mode tabs) get a pane on
    /// the next entry instead of hiding.
    #[test]
    fn flow_adopts_tabs_opened_outside() {
        let mut t = Terminal::new();
        enter(&mut t);
        assert_eq!(leaves(&t).len(), 1);
        let extra = t.open_stub_tab().expect("stub must open");
        enter(&mut t);
        let got = leaves(&t);
        assert_eq!(got.len(), 2);
        assert!(got.contains(&extra));
    }

    /// Manual resizes survive toggling: the kept grid reconciles, it never
    /// rebuilds, while the session set is unchanged.
    #[test]
    fn flow_keeps_manual_sizes_across_toggles() {
        let mut t = Terminal::new();
        enter(&mut t);
        t.flow_add_stub();
        t.flow_grid.drag_col(0, 200.0, 1000.0);
        assert!((t.flow_grid.col_w[0] - 0.7).abs() < 1e-6);
        enter(&mut t);
        assert!(
            (t.flow_grid.col_w[0] - 0.7).abs() < 1e-6,
            "re-entering must keep the manual size"
        );
    }

    /// Each pane keeps its own directory, and the focused one reports it
    /// for the explorer to adopt on the way back to normal mode.
    #[test]
    fn flow_panes_keep_their_directories() {
        let mut t = Terminal::new();
        t.sessions[0].cwd = std::path::PathBuf::from(r"C:\Projects\Backend");
        t.open_stub_tab();
        t.sessions[1].cwd = std::path::PathBuf::from(r"C:\Projects\Frontend");
        enter(&mut t);
        let order = leaves(&t);
        t.flow_focus = Some(order[0]);
        assert_eq!(
            t.flow_focused_cwd(),
            Some(std::path::PathBuf::from(r"C:\Projects\Backend"))
        );
        t.flow_step_focus(1);
        assert_eq!(
            t.flow_focused_cwd(),
            Some(std::path::PathBuf::from(r"C:\Projects\Frontend"))
        );
    }

    /// New panes inherit the focused pane's directory.
    #[test]
    fn flow_split_inherits_focused_directory() {
        let mut t = Terminal::new();
        t.sessions[0].cwd = std::path::PathBuf::from(r"C:\Projects\Mobile");
        enter(&mut t);
        t.flow_add_stub();
        let id = t.flow_target_id().unwrap();
        let session = t.sessions.iter().find(|s| s.id == id).unwrap();
        assert_eq!(session.cwd, std::path::PathBuf::from(r"C:\Projects\Mobile"));
    }

    /// Mode changes, splits and closes never start, restart or drop a
    /// shell: ids are stable, `started` flags untouched, byte counts kept.
    #[test]
    fn flow_never_touches_processes() {
        let mut t = Terminal::new();
        t.open_stub_tab();
        let before: Vec<(u64, bool, u64)> = t
            .sessions
            .iter()
            .map(|s| (s.id, s.started, s.total_bytes))
            .collect();
        enter(&mut t);
        t.flow_add_stub();
        t.flow_step_focus(1);
        t.flow_close_focused();
        t.flow_exit_sync();
        enter(&mut t);
        for (id, started, bytes) in before {
            let s = t.sessions.iter().find(|s| s.id == id);
            // The closed pane's session is gone by design (same as closing
            // its tab); every survivor must be byte-identical.
            if let Some(s) = s {
                assert_eq!((s.started, s.total_bytes), (started, bytes));
            }
        }
    }

    /// Adaptive tiling: 1 fills, 2 split side by side, 3 tiles two over
    /// one full-width pane, 4 tiles 2x2 — and every cell lands exactly
    /// inside the window with no gaps and no overlaps.
    #[test]
    fn flow_grid_tiles_intelligently() {
        use eframe::egui::Rect;
        assert_eq!(flow_shape(0), Vec::<usize>::new());
        assert_eq!(flow_shape(1), vec![1]);
        assert_eq!(flow_shape(2), vec![2]);
        assert_eq!(flow_shape(3), vec![2, 1]);
        assert_eq!(flow_shape(4), vec![2, 2]);
        let area = Rect::from_min_max(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::pos2(1000.0, 800.0),
        );
        let mut t = Terminal::new();
        t.open_stub_tab();
        t.open_stub_tab();
        enter(&mut t);
        // Three panes: two halves on top, one full-width pane below.
        let g = &t.flow_grid;
        let a = g.cell_rect(area, 0, 0);
        let b = g.cell_rect(area, 0, 1);
        let c = g.cell_rect(area, 1, 0);
        assert_eq!((a.width(), b.width()), (500.0, 500.0));
        assert_eq!((a.height(), b.height()), (400.0, 400.0));
        assert_eq!((c.min.x, c.max.x, c.height()), (0.0, 1000.0, 400.0));
        assert_eq!(c.min.y, 400.0);
        // The column divider spans only the split row, the row divider
        // spans the full width.
        let col = g.col_div_rect(area, 0).expect("column divider");
        assert_eq!((col.min.y, col.max.y), (0.0, 400.0));
        assert_eq!(col.width(), 6.0);
        let row = g.row_div_rect(area, 0).expect("row divider");
        assert_eq!((row.min.x, row.max.x), (0.0, 1000.0));
        assert_eq!(row.height(), 6.0);
        // Four panes: even quadrants that exactly fill the window.
        t.flow_add_stub();
        let g = &t.flow_grid;
        let mut corners = 0;
        for (r, c) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            let cell = g.cell_rect(area, r, c);
            assert_eq!((cell.width(), cell.height()), (500.0, 400.0));
            if cell.min.x == 0.0 || cell.max.x == 1000.0 {
                corners += 1;
            }
        }
        assert_eq!(corners, 4);
    }

    /// Pixel rects become cell counts: panes learn their real rows and
    /// columns, clamped to shells worth running.
    #[test]
    fn flow_pixels_become_cells() {
        // A 10px cell: 1000x800px is exactly 100x80 cells.
        assert_eq!(term_size_for_pixels(1000.0, 800.0, (10.0, 10.0)), (80, 100));
        // Degenerate rects bottom out instead of vanishing.
        assert_eq!(term_size_for_pixels(0.0, 0.0, (10.0, 10.0)), (6, 20));
        // Absurd rects top out instead of burning RAM.
        assert_eq!(term_size_for_pixels(1e6, 1e6, (10.0, 10.0)), (200, 400));
    }

    /// Resizing a shell updates its stored size and its vt100 screen, and
    /// repeats are free: no redundant work while dragging.
    #[test]
    fn flow_apply_size_updates_screen_once() {
        let mut t = Terminal::new();
        let id = s(&t).id;
        let session = t.sessions.iter_mut().find(|s| s.id == id).unwrap();
        assert_eq!((session.rows, session.cols), (24, 80));
        assert!(session.apply_size(40, 120));
        assert_eq!((session.rows, session.cols), (40, 120));
        assert_eq!(session.parser.screen().size(), (40, 120));
        assert!(!session.apply_size(40, 120), "repeat must be a no-op");
    }

    /// The 4-shell cap refuses loudly and harms nothing: no fifth shell,
    /// no dead panes, every live session untouched.
    #[test]
    fn flow_cap_refuses_the_fifth_shell() {
        use super::MAX_SESSIONS;
        let mut t = Terminal::new();
        for _ in 0..MAX_SESSIONS {
            enter(&mut t);
            t.flow_add_stub();
        }
        assert_eq!(t.sessions.len(), MAX_SESSIONS);
        assert_eq!(leaves(&t).len(), MAX_SESSIONS);
        t.flow_add_stub();
        assert_eq!(t.sessions.len(), MAX_SESSIONS);
        assert!(t.notice.is_some());
        assert_eq!(leaves(&t).len(), MAX_SESSIONS);
        // Normal tabs obey the same cap.
        assert!(t.open_stub_tab().is_none());
        assert_eq!(t.sessions.len(), MAX_SESSIONS);
    }

    /// Header paths never slice mid-character, whatever the OS reports.
    #[test]
    fn flow_short_cwd_is_char_safe() {
        use super::short_cwd;
        assert_eq!(short_cwd(&std::path::PathBuf::from(r"C:\a")), r"C:\a");
        // Multi-byte content without escape soup: é built from its code
        // point, so the source stays plain ASCII. Must not panic and must
        // stay short.
        let accent = char::from_u32(0xe9).unwrap_or('e');
        let long = std::path::PathBuf::from(format!(
            "C:\\Projets\\caf{accent}-long{tail}",
            tail = accent.to_string().repeat(12)
        ));
        let short = short_cwd(&long);
        assert!(short.chars().count() <= 32);
    }
}
