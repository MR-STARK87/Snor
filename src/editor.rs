use eframe::egui;
use std::path::{Path, PathBuf};

const LARGE_FILE_BYTES: usize = 500_000;
const PLAIN_HIGHLIGHT_BYTES: usize = 200_000;

/// Distance between code lines, and therefore between gutter numbers.
///
/// The reference mock sets 27.5px between lines against a 17px ink height —
/// a leading of roughly 1.6x. At 125% that is 22pt. egui's own default is
/// close to 1.2x, which is why our code looked cramped beside the mock.
const CODE_LINE_H: f32 = 22.0;

/// Code line spacing at a given face size.
///
/// [`CODE_LINE_H`] is the leading at the settings default; scaling it keeps the
/// 1.6x ratio the reference measured at (27.5px between lines against a 17px
/// ink height) when the panel changes the face, instead of a bigger font
/// crowding its own rows together.
fn code_line_h(font_size: f32) -> f32 {
    CODE_LINE_H * (font_size / crate::settings::EDITOR_FONT_DEFAULT)
}

fn language_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "rs" => "rust",
        "toml" => "toml",
        "json" => "json",
        "js" | "mjs" | "cjs" => "js",
        "ts" | "mts" | "tsx" => "ts",
        "py" => "py",
        "ps1" => "ps1",
        "md" => "md",
        _ => "txt",
    }
}

fn keywords(lang: &str) -> &'static [&'static str] {
    match lang {
        "rust" => &[
            "fn", "let", "mut", "pub", "struct", "enum", "impl", "trait", "use", "mod", "crate",
            "self", "Self", "return", "if", "else", "match", "for", "while", "loop", "in", "where",
            "type", "const", "static", "ref", "move", "async", "await", "dyn", "true", "false",
            "Some", "None", "Ok", "Err", "as",
        ],
        "js" | "ts" => &[
            "function",
            "const",
            "let",
            "var",
            "return",
            "if",
            "else",
            "for",
            "while",
            "class",
            "import",
            "export",
            "from",
            "new",
            "true",
            "false",
            "null",
            "undefined",
            "this",
            "async",
            "await",
        ],
        "py" => &[
            "def", "class", "return", "if", "else", "elif", "for", "while", "import", "from", "as",
            "True", "False", "None", "and", "or", "not", "in", "with", "lambda",
        ],
        "toml" | "json" => &["true", "false", "null"],
        _ => &[],
    }
}

fn color_keyword() -> egui::Color32 {
    egui::Color32::from_rgb(0x7A, 0xA2, 0xF7)
}
fn color_string() -> egui::Color32 {
    egui::Color32::from_rgb(0x98, 0xC3, 0x79)
}
fn color_comment() -> egui::Color32 {
    egui::Color32::from_rgb(0x7A, 0x85, 0x77)
}
fn color_number() -> egui::Color32 {
    egui::Color32::from_rgb(0xFF, 0x9E, 0x64)
}
fn color_normal() -> egui::Color32 {
    egui::Color32::from_rgb(0xD5, 0xDA, 0xE2)
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn tab_badge(ui: &mut egui::Ui, filename: &str) {
    let (slot, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
    if !ui.is_rect_visible(slot) {
        return;
    }
    let painter = ui.painter_at(slot);
    // The reference fills the badge with the accent colour and knocks the
    // letter out in the panel colour; non-source files get a page glyph.
    match crate::theme::file_letter(filename) {
        Some(letter) => crate::icons::badge_filled(
            &painter,
            slot,
            letter,
            9.5,
            crate::theme::accent(),
            crate::theme::on_accent(),
        ),
        None => crate::icons::doc(&painter, slot.shrink(1.0), crate::theme::dim_text()),
    }
}

/// The Run button: a play triangle plus a label, sized to its own text.
fn run_button(ui: &mut egui::Ui) -> egui::Response {
    let accent = crate::theme::accent();
    let galley =
        ui.painter()
            .layout_no_wrap("Run".to_owned(), egui::FontId::proportional(13.0), accent);
    let pad = egui::vec2(10.0, 4.0);
    let tri = 11.0;
    let gap = 6.0;
    let size = egui::vec2(
        pad.x * 2.0 + tri + gap + galley.size().x,
        (pad.y * 2.0 + galley.size().y).max(pad.y * 2.0 + tri),
    );
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(
            rect,
            6.0,
            if resp.hovered() {
                egui::Color32::from_rgb(0x25, 0x38, 0x2E)
            } else {
                crate::theme::tab_active()
            },
        );
        painter.rect_stroke(
            rect,
            6.0,
            egui::Stroke::new(1.0, crate::theme::hairline()),
            egui::StrokeKind::Middle,
        );
        crate::icons::play(
            &painter,
            egui::Rect::from_center_size(
                egui::pos2(rect.left() + pad.x + tri * 0.5, rect.center().y),
                egui::vec2(tri, tri),
            ),
            accent,
        );
        painter.galley(
            egui::pos2(
                rect.left() + pad.x + tri + gap,
                rect.center().y - galley.size().y * 0.5,
            ),
            galley,
            accent,
        );
    }
    resp.on_hover_text("send `cargo run` to the terminal")
}

/// Keyword fallback highlighter.
///
/// `font_size` is threaded in rather than fixed here: the editor face is a
/// setting, and this job is what `syntax::ts_job`'s spans are compared against,
/// so a hardcoded size would make an unsupported file change size on open.
pub fn highlight_job(text: &str, lang: &str, font_size: f32) -> egui::text::LayoutJob {
    use egui::text::{LayoutJob, TextFormat};
    let mut job = LayoutJob::default();
    if text.len() > PLAIN_HIGHLIGHT_BYTES {
        job.append(
            text,
            0.0,
            TextFormat {
                color: color_normal(),
                font_id: egui::FontId::monospace(font_size),
                ..Default::default()
            },
        );
        return job;
    }
    let kw = keywords(lang);
    let mono = egui::FontId::monospace(font_size);
    let fmt_normal = TextFormat {
        color: color_normal(),
        font_id: mono.clone(),
        ..Default::default()
    };
    let fmt_kw = TextFormat {
        color: color_keyword(),
        font_id: mono.clone(),
        ..Default::default()
    };
    let fmt_str = TextFormat {
        color: color_string(),
        font_id: mono.clone(),
        ..Default::default()
    };
    let fmt_com = TextFormat {
        color: color_comment(),
        font_id: mono.clone(),
        italics: true,
        ..Default::default()
    };
    let fmt_num = TextFormat {
        color: color_number(),
        font_id: mono.clone(),
        ..Default::default()
    };

    let text_len = text.len();
    let mut i = 0;
    let mut word = String::new();
    let flush_word = |word: &mut String, job: &mut LayoutJob| {
        if word.is_empty() {
            return;
        }
        let is_kw = kw.contains(&word.as_str());
        job.append(
            word.as_str(),
            0.0,
            if is_kw {
                fmt_kw.clone()
            } else {
                fmt_normal.clone()
            },
        );
        word.clear();
    };

    while i < text_len {
        // SAFETY: `i` is always advanced by full `char` lengths below (or over
        // ASCII bytes like '\n'), so it stays on a char boundary and this
        // `chars().next()` never panics on multi-byte text (e.g. em-dash in .md).
        let c: char = match text[i..].chars().next() {
            Some(ch) => ch,
            None => break,
        };
        let clen = c.len_utf8();
        // line comments: // for most, # for toml/py/ps1/md
        let hash_comment = matches!(lang, "toml" | "py" | "ps1" | "md");
        if c == '/' && text[i..].starts_with("//") && !hash_comment {
            flush_word(&mut word, &mut job);
            let start = i;
            while i < text_len && text.as_bytes()[i] != b'\n' {
                // '\n' is 1 byte; scan char-wise so `i` stays on a boundary.
                let ch = text[i..].chars().next().unwrap();
                i += ch.len_utf8();
            }
            if let Some(slice) = text.get(start..i) {
                job.append(slice, 0.0, fmt_com.clone());
            }
            continue;
        }
        if c == '#' && hash_comment {
            flush_word(&mut word, &mut job);
            let start = i;
            while i < text_len && text.as_bytes()[i] != b'\n' {
                let ch = text[i..].chars().next().unwrap();
                i += ch.len_utf8();
            }
            if let Some(slice) = text.get(start..i) {
                job.append(slice, 0.0, fmt_com.clone());
            }
            continue;
        }
        if c == '"' || c == '\'' {
            flush_word(&mut word, &mut job);
            let quote = c;
            let start = i;
            i += clen;
            while i < text_len {
                let d: char = match text[i..].chars().next() {
                    Some(ch) => ch,
                    None => break,
                };
                if d == '\\' {
                    // escape: skip backslash + next char (whatever its width)
                    i += d.len_utf8();
                    if let Some(next) = text[i..].chars().next() {
                        i += next.len_utf8();
                    }
                    continue;
                }
                i += d.len_utf8();
                if d == quote {
                    break;
                }
                if quote == '\'' && d == '\n' {
                    break;
                }
            }
            let end = i.min(text_len);
            if let Some(slice) = text.get(start..end) {
                job.append(slice, 0.0, fmt_str.clone());
            }
            continue;
        }
        if c.is_ascii_digit() && word.is_empty() {
            let start = i;
            while i < text_len {
                let b = text.as_bytes()[i];
                if b.is_ascii_alphanumeric() || b == b'.' || b == b'_' {
                    i += 1;
                } else {
                    break;
                }
            }
            if let Some(slice) = text.get(start..i) {
                job.append(slice, 0.0, fmt_num.clone());
            }
            continue;
        }
        if is_word_char(c) {
            word.push(c);
            i += clen;
            continue;
        }
        flush_word(&mut word, &mut job);
        if let Some(slice) = text.get(i..i + clen) {
            job.append(slice, 0.0, fmt_normal.clone());
        }
        i += clen;
    }
    flush_word(&mut word, &mut job);
    job
}

/// How often the open buffers are compared against the disk.
///
/// Cheap enough to be boring: one `stat` per open tab per second, no file is
/// re-read unless its stamp moved, and nothing on screen is touched unless it
/// did. The alternative — the filesystem watcher the explorer already runs —
/// only covers the workspace root, so a file opened from elsewhere through the
/// dialog would be unwatched. Stat-ing every open buffer covers both cases with
/// no second watcher to keep in step.
const DISK_POLL: std::time::Duration = std::time::Duration::from_secs(1);

/// How long a transient note ("reloaded foo.rs") stays on screen.
const NOTE_TTL: std::time::Duration = std::time::Duration::from_secs(6);

/// What a file looked like the last time we read or wrote it.
///
/// Length plus mtime is the dependency-free way to notice that somebody else —
/// an agent in the terminal, most likely — has rewritten a file we hold in
/// memory. It is not a hash, and the gap is worth stating: a rewrite that lands
/// inside the same timestamp tick *and* keeps the byte count is invisible.
/// NTFS timestamps are 100ns, so that is a theoretical miss rather than a
/// practical one, and the price of closing it would be re-reading every open
/// file every second to compare contents.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct DiskStamp {
    len: u64,
    mtime: Option<std::time::SystemTime>,
}

/// The current stamp of `path`, or `None` when it cannot be measured.
///
/// A `None` silences external-change detection for that buffer rather than
/// making it fire on every tick: a file we cannot stat must not be reported as
/// constantly changing.
fn disk_stamp(path: &Path) -> Option<DiskStamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some(DiskStamp {
        len: meta.len(),
        mtime: meta.modified().ok(),
    })
}

pub struct OpenBuffer {
    pub path: PathBuf,
    pub lang: String,
    pub text: String,
    pub dirty: bool,
    pub too_large: bool,
    lines: usize,
    /// Stamp of the bytes we believe are on disk — written when the buffer is
    /// opened and again on every save.
    stamp: Option<DiskStamp>,
    /// The file changed underneath us while this buffer had unsaved edits.
    ///
    /// Surfaced as a banner offering "reload from disk" or "keep mine", never
    /// resolved silently: reloading throws away what the user typed, and saving
    /// throws away what the agent wrote. Both are the user's to choose.
    pub conflict: bool,
}

impl OpenBuffer {
    fn open(path: PathBuf) -> anyhow::Result<Self> {
        let meta = std::fs::metadata(&path)?;
        let too_large = meta.len() > LARGE_FILE_BYTES as u64;
        let text = std::fs::read_to_string(&path).unwrap_or_else(|_| String::from("<binary>"));
        let lang = language_for(&path).to_string();
        let lines = ropey::Rope::from_str(&text).len_lines();
        Ok(Self {
            stamp: disk_stamp(&path),
            path,
            lang,
            text,
            dirty: false,
            too_large,
            conflict: false,
            lines,
        })
    }

    fn save(&mut self) -> anyhow::Result<()> {
        std::fs::write(&self.path, &self.text)?;
        self.dirty = false;
        // Re-stamp from disk rather than assuming: the write is what we now
        // believe is there, and reading the file back is what proves it. It
        // also clears any pending conflict — this save is the resolution.
        self.stamp = disk_stamp(&self.path);
        self.conflict = false;
        Ok(())
    }

    /// Re-read this file, keeping the buffer's identity, cursor-friendly text
    /// and place in the tab strip. Only ever called for a clean buffer: a dirty
    /// one goes to the conflict banner instead.
    fn reload(&mut self) -> bool {
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return false;
        };
        self.text = text;
        self.dirty = false;
        self.conflict = false;
        self.refresh_lines();
        let stamp = disk_stamp(&self.path);
        self.too_large = stamp.is_some_and(|s| s.len > LARGE_FILE_BYTES as u64);
        self.stamp = stamp;
        true
    }

    fn name(&self) -> String {
        self.path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    fn line_count(&self) -> usize {
        self.lines
    }

    fn refresh_lines(&mut self) {
        self.lines = self.text.bytes().filter(|&b| b == b'\n').count() + 1;
    }
}

pub fn find_line_matches(text: &str, query: &str) -> Vec<usize> {
    if query.is_empty() {
        return Vec::new();
    }
    let q = query.to_lowercase();
    text.lines()
        .enumerate()
        .filter(|(_, line)| line.to_lowercase().contains(&q))
        .map(|(idx, _)| idx)
        .take(2000)
        .collect()
}

pub struct Editor {
    tabs: Vec<OpenBuffer>,
    active: usize,
    pub error: Option<String>,
    pub saved_tick: u64,
    find_open: bool,
    find_query: String,
    find_hits: Vec<usize>,
    find_pos: usize,
    find_focus_req: bool,
    pub cursor_line: usize,
    pub cursor_col: usize,
    /// Set by the Run button; the app shell consumes it to send
    /// `cargo run` to the terminal.
    pub want_run: bool,
    /// Set by the empty state's "New file". The editor cannot reach the file
    /// tree, so the shell consumes this to reveal the Workspace panel and
    /// open its inline name field.
    pub want_new_file: bool,
    /// Set by the empty state's "Open from Workspace": reveal the panel so a
    /// file can be picked. Separate from `want_new_file` because revealing is
    /// all it should do — starting the create flow as well would put a text
    /// field in front of someone who asked to browse.
    pub want_workspace: bool,
    /// When the open buffers were last compared against the disk. `None` until
    /// the first frame, so the first check runs immediately rather than a
    /// second after launch.
    disk_checked: Option<std::time::Instant>,
    /// Tab whose close is waiting on the user because it holds unsaved edits.
    /// `Some` while the confirm box is up; only ever one at a time.
    pending_close: Option<usize>,
    /// A short-lived line of feedback beside the file's metadata, currently
    /// only "reloaded <file>" — the one case where the editor changed the text
    /// without the user typing, which is worth saying out loud.
    note: Option<(String, std::time::Instant)>,
    /// Code face, in points. A setting; see [`Editor::set_font_size`].
    font_size: f32,
}

impl Editor {
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active: 0,
            error: None,
            saved_tick: 0,
            find_open: false,
            find_query: String::new(),
            find_hits: Vec::new(),
            find_pos: 0,
            find_focus_req: false,
            cursor_line: 1,
            cursor_col: 1,
            want_run: false,
            want_new_file: false,
            want_workspace: false,
            disk_checked: None,
            pending_close: None,
            note: None,
            font_size: crate::settings::EDITOR_FONT_DEFAULT,
        }
    }

    /// Set the code face, clamped to the range the settings file allows.
    ///
    /// The leading follows the face through [`code_line_h`], so a larger font
    /// gets proportionally more room and does not set its rows on top of each
    /// other. Nothing is re-laid out here: the text edit's layouter is rebuilt
    /// every frame, so the next frame already draws at the new size.
    pub fn set_font_size(&mut self, points: f32) {
        self.font_size = points.clamp(
            crate::settings::EDITOR_FONT_MIN,
            crate::settings::EDITOR_FONT_MAX,
        );
    }

    pub fn open_file(&mut self, path: PathBuf) {
        if let Some(idx) = self.tabs.iter().position(|t| t.path == path) {
            self.active = idx;
            return;
        }
        match OpenBuffer::open(path) {
            Ok(buf) => {
                self.tabs.push(buf);
                self.active = self.tabs.len() - 1;
                self.error = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    /// Close tab `idx`, keeping `active` on a real tab.
    ///
    /// Returns `true` while tabs remain. A `false` means the list is now empty
    /// and **the caller must stop drawing the frame**: everything below the tab
    /// bar indexes `tabs`, and an empty list makes `tabs.len() - 1` underflow.
    /// That is `usize`, so it is a panic rather than a negative index — the bug
    /// that made "close the last open file" take the whole app down.
    ///
    /// The `is_empty` guard at the top of `ui()` cannot catch it. That guard
    /// runs before the tab bar is drawn; the close is applied *during* the
    /// frame, so the list goes empty with most of the frame still to draw.
    pub fn close_tab(&mut self, idx: usize) -> bool {
        if idx >= self.tabs.len() {
            return !self.tabs.is_empty();
        }
        self.tabs.remove(idx);
        if self.tabs.is_empty() {
            self.active = 0;
            self.find_open = false;
            // The status bar keeps drawing `Ln x, Col y` from these fields, and
            // nothing else resets them while the editor is empty — `empty_state`
            // draws no code area, and the readout below the tab bar is skipped
            // by the caller's early return. Without this the bar would advertise
            // the closed file's last cursor position indefinitely.
            self.cursor_line = 1;
            self.cursor_col = 1;
            return false;
        }
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
        true
    }

    /// Ask to close a tab, putting the question to the user when it has
    /// unsaved edits.
    ///
    /// Returns the same thing [`Editor::close_tab`] does — `false` when the
    /// list has just been emptied and the caller must stop drawing the frame —
    /// and `true` while the confirm box is up, because nothing has been
    /// removed yet and there is still a tab to draw.
    fn request_close_tab(&mut self, idx: usize) -> bool {
        if self.tabs.get(idx).is_some_and(|t| t.dirty) {
            self.pending_close = Some(idx);
            return true;
        }
        self.close_tab(idx)
    }

    /// True while any open buffer has unsaved edits. What the quit prompt and
    /// the window's close button key off.
    pub fn has_unsaved(&self) -> bool {
        self.tabs.iter().any(|t| t.dirty)
    }

    /// Names of the files with unsaved edits, for the quit prompt. Capped so a
    /// dozen modified files cannot turn the box into a wall of text.
    pub fn unsaved_names(&self) -> Vec<String> {
        self.tabs
            .iter()
            .filter(|t| t.dirty)
            .map(|t| t.name())
            .take(6)
            .collect()
    }

    /// Compare every open buffer against the disk, once a second.
    ///
    /// The timing lives here and the decision-making lives in
    /// [`Editor::reconcile_with_disk`] so a test can prove the reload and the
    /// conflict without sleeping for a second — a timing-dependent assertion is
    /// one that fails on a loaded machine.
    fn poll_disk(&mut self) {
        let due = match self.disk_checked {
            Some(at) => at.elapsed() >= DISK_POLL,
            None => true,
        };
        if !due {
            return;
        }
        self.disk_checked = Some(std::time::Instant::now());
        self.reconcile_with_disk();
    }

    /// Reconcile each open buffer with what is actually on disk.
    ///
    /// Clean buffer: reload it, and say so — text changing without a keystroke
    /// is exactly the kind of thing that reads as a bug if it happens silently.
    /// Dirty buffer: raise the conflict banner and touch nothing.
    ///
    /// A half-written file is self-healing rather than guarded against: if an
    /// agent truncates and rewrites, we may catch the empty middle, and the next
    /// tick sees a stamp that moved again and reloads the finished version.
    fn reconcile_with_disk(&mut self) {
        for i in 0..self.tabs.len() {
            let Some(stamp) = disk_stamp(&self.tabs[i].path) else {
                continue;
            };
            if self.tabs[i].stamp == Some(stamp) {
                continue;
            }
            if self.tabs[i].dirty {
                self.tabs[i].conflict = true;
                continue;
            }
            if self.tabs[i].reload() {
                let name = self.tabs[i].name();
                self.note = Some((format!("reloaded {name}"), std::time::Instant::now()));
            }
        }
    }

    /// The unsaved-changes confirm box for a tab close.
    ///
    /// A modal rather than an inline strip: the answer decides whether text the
    /// user typed still exists, so it must not be possible to miss it or to
    /// click past it while working.
    fn close_confirm_modal(&mut self, ui: &egui::Ui) {
        let Some(idx) = self.pending_close else {
            return;
        };
        let Some(name) = self.tabs.get(idx).map(|t| t.name()) else {
            self.pending_close = None;
            return;
        };
        #[derive(PartialEq)]
        enum Choice {
            Save,
            Discard,
            Cancel,
        }
        let mut choice: Option<Choice> = None;
        egui::Window::new("unsaved changes")
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.label(format!("{name} has unsaved changes."));
                ui.horizontal(|ui| {
                    if ui.button("save").clicked() {
                        choice = Some(Choice::Save);
                    }
                    if ui.button("discard").clicked() {
                        choice = Some(Choice::Discard);
                    }
                    if ui.button("cancel").clicked() {
                        choice = Some(Choice::Cancel);
                    }
                });
            });
        match choice {
            Some(Choice::Save) => {
                // `save_active` works off `active`, so point it at the tab being
                // closed and put the selection back afterwards. If the save
                // fails the box stays up and nothing is removed.
                let previous = self.active;
                self.active = idx;
                self.save_active();
                self.active = previous;
                if !self.tabs.get(idx).is_some_and(|t| t.dirty) {
                    self.pending_close = None;
                    self.close_tab(idx);
                }
            }
            Some(Choice::Discard) => {
                self.pending_close = None;
                self.close_tab(idx);
            }
            Some(Choice::Cancel) => self.pending_close = None,
            None => {}
        }
    }

    pub fn active_lang(&self) -> String {
        self.tabs
            .get(self.active)
            .map(|b| match b.lang.as_str() {
                "rust" => "Rust".to_string(),
                "toml" => "TOML".to_string(),
                "json" => "JSON".to_string(),
                "js" => "JavaScript".to_string(),
                "ts" => "TypeScript".to_string(),
                "py" => "Python".to_string(),
                "ps1" => "PowerShell".to_string(),
                "md" => "Markdown".to_string(),
                _ => "Text".to_string(),
            })
            .unwrap_or_else(|| "—".to_string())
    }

    /// What the editor shows when nothing is open.
    ///
    /// The editor's chrome — tab strip, separator, gutter, code area — is not
    /// drawn at all. An empty tab strip above an empty gutter reads as a
    /// broken editor rather than an idle one, and the gutter's line numbers
    /// especially look like a file that failed to load. What replaces it is a
    /// short invitation with the two things a person actually wants next.
    fn empty_state(&mut self, ui: &mut egui::Ui, workdir: &std::path::Path) {
        if let Some(err) = &self.error {
            ui.colored_label(crate::theme::danger(), err);
        }
        // Fill the column: a content-sized area would let the editor collapse
        // and drag the split divider up with it.
        egui::ScrollArea::both()
            .id_salt("snor_editor_empty")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    // Roughly a third down, which reads as centred without
                    // needing the content's measured height.
                    ui.add_space((ui.available_height() * 0.3).max(24.0));

                    let (glyph, _) =
                        ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
                    if ui.is_rect_visible(glyph) {
                        crate::icons::doc(
                            &ui.painter_at(glyph),
                            egui::Rect::from_center_size(glyph.center(), egui::vec2(26.0, 33.0)),
                            crate::theme::faint(),
                        );
                    }

                    ui.add_space(16.0);
                    ui.label(
                        egui::RichText::new("Nothing open yet")
                            .size(17.0)
                            .family(crate::theme::medium())
                            .color(crate::theme::text()),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(
                            "Pick a file from the Workspace panel, or start a new one.",
                        )
                        .size(12.5)
                        .color(crate::theme::dim_text()),
                    );
                    ui.add_space(22.0);

                    // The two actions, centred as a pair. One rect is
                    // reserved for the whole row and divided between the
                    // buttons; see `widgets::prompt_button_at` for why they
                    // are not laid out with a nested horizontal layout.
                    let gap = 10.0;
                    let w_new = crate::widgets::prompt_button_width(ui, "New file");
                    let w_open = crate::widgets::prompt_button_width(ui, "Open from Workspace");
                    let h = crate::widgets::prompt_button_height(ui);
                    let (row, _) = ui.allocate_exact_size(
                        egui::vec2(w_new + gap + w_open, h),
                        egui::Sense::hover(),
                    );
                    let new_rect = egui::Rect::from_min_size(row.min, egui::vec2(w_new, h));
                    let open_rect = egui::Rect::from_min_size(
                        egui::pos2(row.min.x + w_new + gap, row.min.y),
                        egui::vec2(w_open, h),
                    );
                    if crate::widgets::prompt_button_at(ui, new_rect, "New file", true).clicked() {
                        self.want_new_file = true;
                    }
                    if crate::widgets::prompt_button_at(ui, open_rect, "Open from Workspace", false)
                        .clicked()
                    {
                        self.want_workspace = true;
                    }

                    ui.add_space(14.0);
                    // Tertiary, and the reason it exists: the file dialog lives
                    // on the tab strip's "+", and the strip is not drawn while
                    // nothing is open, so without this there is no way left to
                    // reach a file outside the project root.
                    let browse = crate::widgets::clickable_label(
                        ui,
                        egui::RichText::new("or browse for a file elsewhere…")
                            .size(11.5)
                            .color(crate::theme::faint()),
                    );
                    if browse.clicked()
                        && let Some(path) =
                            rfd::FileDialog::new().set_directory(workdir).pick_file()
                    {
                        self.open_file(path);
                    }
                });
            });
    }

    fn recompute_find(&mut self) {
        self.find_hits = match self.tabs.get(self.active) {
            Some(buf) => find_line_matches(&buf.text, self.find_query.trim()),
            None => Vec::new(),
        };
        self.find_pos = 0;
    }

    fn find_step(&mut self, ui: &egui::Ui, dir: i32) {
        if self.find_hits.is_empty() {
            return;
        }
        let n = self.find_hits.len() as i32;
        self.find_pos = (self.find_pos as i32 + dir).rem_euclid(n) as usize;
        self.jump_to_hit(ui);
    }

    fn jump_to_hit(&mut self, ui: &egui::Ui) {
        use eframe::egui::text::{CCursor, CCursorRange};
        use eframe::egui::widgets::text_edit::TextEditState;
        let Some(buf) = self.tabs.get(self.active) else {
            return;
        };
        let Some(&line) = self.find_hits.get(self.find_pos) else {
            return;
        };
        let mut byte: usize = buf.text.lines().take(line).map(|l| l.len() + 1).sum();
        byte = byte.min(buf.text.len());
        while byte > 0 && !buf.text.is_char_boundary(byte) {
            byte -= 1;
        }
        let ch = buf.text.get(..byte).map(|s| s.chars().count()).unwrap_or(0);
        let id = ui.make_persistent_id("snor_editor_text");
        let mut state = TextEditState::load(ui.ctx(), id).unwrap_or_default();
        state
            .cursor
            .set_char_range(Some(CCursorRange::one(CCursor::new(ch))));
        state.store(ui.ctx(), id);
        ui.memory_mut(|m| m.request_focus(id));
    }

    fn save_active(&mut self) {
        if let Some(buf) = self.tabs.get_mut(self.active) {
            match buf.save() {
                Ok(()) => {
                    self.error = None;
                    self.saved_tick += 1;
                }
                Err(e) => self.error = Some(e.to_string()),
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, workdir: &std::path::Path) {
        // Drawn before everything else, and outside the empty check below: the
        // box outlives a frame that returns early, and the only way to lose it
        // would be to draw it after the one path that skips the rest of `ui()`.
        self.close_confirm_modal(ui);
        // Nothing open: draw none of the editor's own chrome. See
        // `empty_state` for why an empty tab strip and gutter are worse than
        // no editor at all.
        if self.tabs.is_empty() {
            self.find_open = false;
            self.empty_state(ui, workdir);
            return;
        }
        // Files change under us while an agent runs in the terminal. One stat
        // per open buffer per second, and a reload or a banner when one moved.
        self.poll_disk();

        // Tab bar: lang badge + name pills, file picker, Run button.
        ui.horizontal(|ui| {
            let mut close_idx: Option<usize> = None;
            let mut tab_switched = false;
            egui::ScrollArea::horizontal()
                .id_salt("snor_tabs")
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (idx, tab) in self.tabs.iter().enumerate() {
                            let name = tab
                                .path
                                .file_name()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_default();
                            let label = if tab.dirty {
                                format!("{name} *")
                            } else {
                                name.clone()
                            };
                            let active = idx == self.active;
                            // The pill itself is the highlight, so the label
                            // must not draw egui's own selection background.
                            egui::Frame::NONE
                                .fill(if active {
                                    crate::theme::tab_active()
                                } else {
                                    egui::Color32::TRANSPARENT
                                })
                                .stroke(if active {
                                    egui::Stroke::new(1.0, crate::theme::hairline())
                                } else {
                                    egui::Stroke::NONE
                                })
                                .corner_radius(6.0)
                                .inner_margin(egui::Margin::symmetric(6, 3))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        tab_badge(ui, &name);
                                        let text = egui::RichText::new(label).size(12.5).color(
                                            if active {
                                                crate::theme::text()
                                            } else {
                                                crate::theme::dim_text()
                                            },
                                        );
                                        // Same as the terminal's tab strip.
                                        if crate::widgets::clickable_label(ui, text).clicked()
                                        {
                                            self.active = idx;
                                            tab_switched = true;
                                        }
                                        if crate::icons::icon_button(
                                            ui,
                                            16.0,
                                            "close tab",
                                            crate::icons::close_x,
                                        )
                                        .clicked()
                                        {
                                            close_idx = Some(idx);
                                        }
                                    });
                                });
                        }
                        if crate::icons::icon_button(ui, 20.0, "open file", |p, r, c| {
                            crate::icons::plus(p, r, c)
                        })
                        .clicked()
                            && let Some(path) =
                                rfd::FileDialog::new().set_directory(workdir).pick_file()
                        {
                            self.open_file(path);
                            tab_switched = true;
                        }
                    });
                });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if run_button(ui).clicked() {
                    self.want_run = true;
                }
            });
            if let Some(idx) = close_idx {
                // Asks instead of closing when the tab has unsaved edits, so a
                // tab can survive the click. `false` means the list just went
                // empty, so nothing is left to switch to — `recompute_find()`
                // below indexes the active tab, and the frame returns early
                // right after this block anyway.
                if self.request_close_tab(idx) {
                    tab_switched = true;
                }
            }
            if tab_switched {
                self.recompute_find();
            }
        });
        // Closing the last tab empties the list *mid-frame*. The `is_empty`
        // guard at the top of `ui()` has already run by the time this close is
        // applied, so the rest of the frame has to be skipped by hand.
        //
        // Without this, the metadata row further down evaluates
        // `self.tabs.len() - 1` on an empty list. `usize` cannot go negative,
        // so that subtraction overflows — a panic in debug builds, which is
        // what "close the last open file" used to do to the whole app. The
        // next frame redraws through the normal empty-state path.
        if self.tabs.is_empty() {
            self.find_open = false;
            return;
        }
        ui.separator();

        if let Some(err) = &self.error {
            ui.colored_label(crate::theme::danger(), err);
        }

        if ui.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::CTRL,
                egui::Key::F,
            ))
        }) {
            self.find_open = !self.find_open;
            self.find_focus_req = true;
            self.recompute_find();
        }

        if self.find_open {
            let find_id = ui.make_persistent_id("snor_find_query");
            if self.find_focus_req {
                ui.memory_mut(|m| m.request_focus(find_id));
                self.find_focus_req = false;
            }
            let mut close_find = false;
            let mut step = 0;
            let mut field_focused = false;
            ui.horizontal(|ui| {
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.find_query)
                        .id(find_id)
                        .hint_text("find in file")
                        .desired_width(220.0),
                );
                if resp.changed() {
                    self.recompute_find();
                }
                let n = self.find_hits.len();
                if n > 0 {
                    ui.label(egui::RichText::new(format!("{}/{}", self.find_pos + 1, n)).small());
                    if ui.small_button("prev").clicked() {
                        step = -1;
                    }
                    if ui.small_button("next").clicked() {
                        step = 1;
                    }
                } else {
                    ui.label(
                        egui::RichText::new("no match")
                            .small()
                            .color(crate::theme::dim_text()),
                    );
                }
                if ui.small_button("x").clicked() {
                    close_find = true;
                }
                let ff = ui.memory(|m| m.has_focus(find_id));
                field_focused = ff;
                if ff && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    step = if ui.input(|i| i.modifiers.shift) {
                        -1
                    } else {
                        1
                    };
                }
            });
            if field_focused && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close_find = true;
            }
            if close_find {
                self.find_open = false;
            } else if step != 0 {
                self.find_step(ui, step);
            }
            ui.separator();
        }

        let mut want_save = false;
        {
            let buf = &self.tabs[self.active.min(self.tabs.len() - 1)];
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "{}  {} lines  {} bytes{}",
                        buf.lang,
                        buf.line_count(),
                        buf.text.len(),
                        if buf.too_large { "  LARGE" } else { "" }
                    ))
                    .small()
                    .color(crate::theme::dim_text()),
                );
                // Text the user did not type has just appeared, so it says so.
                // Expires on its own; a note that has to be dismissed would be
                // worse than the silence it replaces.
                let note = self.note.as_ref().filter(|(_, at)| at.elapsed() < NOTE_TTL);
                if let Some((text, _)) = note {
                    ui.label(
                        egui::RichText::new(text.as_str())
                            .small()
                            .color(crate::theme::accent()),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("save (Ctrl+S)").clicked() {
                        want_save = true;
                    }
                });
            });
        }
        if self
            .note
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() >= NOTE_TTL)
        {
            self.note = None;
        }

        // The file moved underneath us and this buffer has edits of its own.
        //
        // Nothing is decided for the user: reloading throws away what they
        // typed and saving throws away what the agent wrote, and the app has no
        // way to know which of the two is the work they care about.
        if self.tabs.get(self.active).is_some_and(|b| b.conflict) {
            let mut reload = false;
            let mut keep = false;
            egui::Frame::NONE
                .fill(crate::theme::surface_title())
                .corner_radius(6.0)
                .inner_margin(egui::Margin::symmetric(8, 4))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("changed on disk while you were editing")
                                .size(12.0)
                                .color(crate::theme::danger()),
                        );
                        if ui.small_button("reload from disk").clicked() {
                            reload = true;
                        }
                        if ui.small_button("keep mine").clicked() {
                            keep = true;
                        }
                    });
                });
            if reload
                && let Some(buf) = self.tabs.get_mut(self.active)
                && buf.reload()
            {
                let name = self.tabs[self.active].name();
                self.note = Some((format!("reloaded {name}"), std::time::Instant::now()));
            }
            if keep
                && let Some(buf) = self.tabs.get_mut(self.active)
            {
                // Accepted as-is, so stop asking about *this* change: the
                // stamp is moved up to what is on disk now, and the next
                // external edit raises the banner again. Saving will still
                // overwrite the file, which is what "keep mine" means.
                buf.conflict = false;
                buf.stamp = disk_stamp(&buf.path);
            }
        }

        if ui.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::CTRL,
                egui::Key::S,
            ))
        }) {
            want_save = true;
        }
        if want_save {
            self.save_active();
        }

        let Some(buf) = self.tabs.get_mut(self.active) else {
            return;
        };
        let lang = buf.lang.clone();
        let editable = !buf.too_large;
        let line_count = buf.line_count();
        let editor_id = ui.make_persistent_id("snor_editor_text");
        let mut text_changed = false;
        // Read once, before the closures below. `buf` holds a mutable borrow of
        // `self.tabs`, so `self` cannot be asked for these later — and the
        // gutter and the code have to agree on the face or the numbers drift
        // off their rows.
        let font_size = self.font_size;
        let line_h = code_line_h(font_size);
        // Gutter + code share one vertical scroll so numbers stay glued to
        // rows; the code scrolls horizontally on its own. Wrapping is off
        // (one buffer line == one visual row) so the gutter can't drift.
        egui::ScrollArea::vertical()
            .id_salt("snor_editor_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    if line_count <= 6000 {
                        // Must be an explicit vertical layout: `ui.scope` would
                        // inherit the enclosing `horizontal_top`, and the whole
                        // gutter would run off to the right, taking the code
                        // with it.
                        ui.scope_builder(
                            egui::UiBuilder::new()
                                .layout(egui::Layout::top_down(egui::Align::LEFT)),
                            |ui| {
                                let mono = egui::FontId::monospace(font_size);
                                // The gutter has to advance in step with the
                                // code, whose rows are forced to `line_h`.
                                // A label's own height comes from the font, so
                                // the difference goes into the item spacing.
                                let row = ui.ctx().fonts_mut(|f| f.row_height(&mono));
                                ui.style_mut().spacing.item_spacing =
                                    egui::vec2(0.0, (line_h - row).max(0.0));
                                for n in 1..=line_count {
                                    ui.label(
                                        egui::RichText::new(format!("{n:>4} "))
                                            .font(mono.clone())
                                            .color(crate::theme::faint()),
                                    );
                                }
                            },
                        );
                        ui.separator();
                    }
                    egui::ScrollArea::horizontal()
                        .id_salt("snor_editor_hscroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let mut layouter =
                                |ui: &egui::Ui, text: &dyn egui::TextBuffer, _wrap: f32| {
                                    let mut job =
                                        crate::syntax::ts_job(text.as_str(), &lang, font_size)
                                            .unwrap_or_else(|| {
                                                highlight_job(text.as_str(), &lang, font_size)
                                            });
                                    job.wrap.max_width = f32::INFINITY;
                                    // The reference sets 27.5px between code
                                    // lines against a 17px ink height, i.e. a
                                    // leading of about 1.6x. egui's default is
                                    // nearer 1.2x, which reads cramped beside
                                    // it. Set per section because that is where
                                    // the layout actually reads it from.
                                    for section in &mut job.sections {
                                        section.format.line_height = Some(line_h);
                                    }
                                    ui.fonts_mut(|f| f.layout_job(job))
                                };
                            let resp = ui.add(
                                egui::TextEdit::multiline(&mut buf.text)
                                    .id(editor_id)
                                    .code_editor()
                                    .desired_width(f32::INFINITY)
                                    .frame(egui::Frame::NONE)
                                    .margin(egui::Margin::ZERO)
                                    .layouter(&mut layouter)
                                    .interactive(editable),
                            );
                            if resp.changed() {
                                buf.dirty = true;
                                buf.refresh_lines();
                                text_changed = true;
                            }
                        });
                });
            });
        // Cursor readout for the status bar (char offset -> line/col).
        let target = egui::widgets::text_edit::TextEditState::load(ui.ctx(), editor_id)
            .and_then(|state| state.cursor.char_range())
            .map(|range| range.primary.index.0)
            .unwrap_or(0);
        let (line, col) = {
            let mut line = 1;
            let mut col = 1;
            for (n, ch) in buf.text.chars().enumerate() {
                if n >= target {
                    break;
                }
                if ch == '\n' {
                    line += 1;
                    col = 1;
                } else {
                    col += 1;
                }
            }
            (line, col)
        };
        if text_changed && self.find_open {
            self.recompute_find();
        }
        self.cursor_line = line;
        self.cursor_col = col;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_edit_save_roundtrip() {
        let dir = std::env::temp_dir().join("snor_editor_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("hello.rs");
        std::fs::write(&path, "fn main() {}\n").unwrap();
        let mut ed = Editor::new();
        ed.open_file(path.clone());
        assert_eq!(ed.tabs.len(), 1);
        assert!(!ed.tabs[0].dirty);
        ed.tabs[0].text.push_str("// edited\n");
        ed.tabs[0].dirty = true;
        ed.save_active();
        assert!(!ed.tabs[0].dirty);
        let back = std::fs::read_to_string(&path).unwrap();
        assert!(back.contains("// edited"), "saved text missing");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn closing_the_last_tab_empties_the_list_without_underflow() {
        // Regression. `Editor::ui` used to close a tab inline and then carry on
        // drawing the frame, where `self.tabs[self.active.min(self.tabs.len() - 1)]`
        // ran against a now-empty list. `usize` cannot go negative, so closing
        // the only open file panicked with "attempt to subtract with overflow"
        // and took the whole app down with it. `close_tab` now reports the empty
        // case so the caller can bail out of the frame before that line is
        // reached.
        let dir = std::env::temp_dir().join("snor_editor_close_test");
        let _ = std::fs::create_dir_all(&dir);
        let mut ed = Editor::new();

        // Closing with nothing open is a no-op, not a panic.
        assert!(!ed.close_tab(0));
        assert!(ed.tabs.is_empty());

        for i in 0..3 {
            let path = dir.join(format!("f{i}.rs"));
            std::fs::write(&path, "fn main() {}\n").unwrap();
            ed.open_file(path);
        }
        assert_eq!(ed.tabs.len(), 3);
        assert_eq!(ed.active, 2, "the newest tab is the active one");

        // Closing a tab *before* the active one must not steal the selection:
        // the same file stays open, just at a lower index.
        let active_path = ed.tabs[ed.active].path.clone();
        assert!(ed.close_tab(0));
        assert_eq!(ed.tabs.len(), 2);
        assert_eq!(ed.active, 1);
        assert_eq!(ed.tabs[ed.active].path, active_path);

        // Closing the active tab hands over to the last remaining one.
        assert!(ed.close_tab(1));
        assert_eq!(ed.tabs.len(), 1);
        assert_eq!(ed.active, 0);

        // The one that used to take the app down.
        ed.cursor_line = 42;
        ed.cursor_col = 7;
        assert!(!ed.close_tab(0), "an emptied list must report false");
        assert!(ed.tabs.is_empty());
        assert_eq!(ed.active, 0);
        assert!(!ed.find_open, "the find bar must not outlive the last tab");
        assert_eq!(
            (ed.cursor_line, ed.cursor_col),
            (1, 1),
            "the status bar must not keep the closed file's position"
        );

        // Out-of-range stays safe, empty or not.
        assert!(!ed.close_tab(7));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_matches_lines_case_insensitive() {
        let text = "fn alpha() {}\nlet beta = 1;\n// ALPHA note\n";
        assert_eq!(find_line_matches(text, "alpha"), vec![0, 2]);
        assert_eq!(find_line_matches(text, "zzz"), Vec::<usize>::new());
        assert_eq!(find_line_matches(text, ""), Vec::<usize>::new());
    }

    #[test]
    fn highlight_unicode_does_not_panic() {
        // Regression: old byte-wise highlighter sliced mid-char and panicked
        // on .md files (README has an em-dash). Must survive multi-byte text.
        let text = "# title — with em-dash\ncafé \"naïve 🎉\" '#hash' // cömment\nlet x = 123;\nemoji 🎉 test — ok\n";
        for lang in ["md", "rust", "toml", "txt", "py", "js"] {
            let job = highlight_job(text, lang, crate::settings::EDITOR_FONT_DEFAULT);
            assert!(!job.sections.is_empty(), "empty job for {lang}");
        }
        // The real project README previously crashed the app on click.
        let readme = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("README.md");
        if let Ok(contents) = std::fs::read_to_string(&readme) {
            let _ = highlight_job(&contents, "md", crate::settings::EDITOR_FONT_DEFAULT);
            let _ = crate::syntax::ts_job(&contents, "md", crate::settings::EDITOR_FONT_DEFAULT)
                .unwrap_or_else(|| {
                    highlight_job(&contents, "md", crate::settings::EDITOR_FONT_DEFAULT)
                });
        }
        // Opening it as a buffer must not error either.
        let mut ed = Editor::new();
        ed.open_file(readme);
        assert!(ed.error.is_none(), "open README failed: {:?}", ed.error);
    }

    /// A clean buffer whose file changed on disk reloads itself, and says so.
    /// This is the agent-in-the-terminal case: it rewrites a file the editor is
    /// holding, and the alternative to reloading is showing stale text and then
    /// silently reverting the agent on the next Ctrl+S.
    #[test]
    fn an_externally_rewritten_file_reloads_itself() {
        let dir = std::env::temp_dir().join("snor_editor_reload_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("lib.rs");
        std::fs::write(&path, "fn a() {}\n").unwrap();

        let mut ed = Editor::new();
        ed.open_file(path.clone());
        // Nothing moved: the check must stay quiet, or the note would fire on
        // every tick and mean nothing.
        ed.reconcile_with_disk();
        assert_eq!(ed.tabs[0].text, "fn a() {}\n");
        assert!(ed.note.is_none(), "an unchanged file must not report");

        // The agent rewrites it. Different length as well as different mtime,
        // so the assertion cannot depend on filesystem timestamp resolution.
        std::fs::write(&path, "fn a() {}\nfn b() {}\n").unwrap();
        ed.reconcile_with_disk();
        assert_eq!(ed.tabs[0].text, "fn a() {}\nfn b() {}\n");
        assert!(!ed.tabs[0].dirty);
        assert!(ed.tabs[0].line_count() >= 2, "line count must follow the text");
        assert!(
            ed.note.as_ref().is_some_and(|(t, _)| t.contains("lib.rs")),
            "a silent reload reads as a bug; the note is the receipt"
        );

        // Repeated checks with nothing moving do not re-report.
        ed.note = None;
        ed.reconcile_with_disk();
        assert!(ed.note.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A dirty buffer is never reloaded: the file changed, but so did the
    /// buffer, and the app cannot know which of the two edits matters. It
    /// raises the banner and leaves every byte alone.
    #[test]
    fn a_dirty_buffer_reports_a_conflict_instead_of_reloading() {
        let dir = std::env::temp_dir().join("snor_editor_conflict_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("main.rs");
        std::fs::write(&path, "fn main() {}\n").unwrap();

        let mut ed = Editor::new();
        ed.open_file(path.clone());
        ed.tabs[0].text.push_str("// mine\n");
        ed.tabs[0].dirty = true;
        ed.tabs[0].refresh_lines();

        std::fs::write(&path, "fn main() {}\n// agent\n").unwrap();
        ed.reconcile_with_disk();

        assert!(ed.tabs[0].conflict, "a dirty buffer must report, not reload");
        assert!(
            ed.tabs[0].text.contains("// mine") && !ed.tabs[0].text.contains("// agent"),
            "the user's edits must survive the check untouched"
        );
        assert!(ed.has_unsaved());

        // Saving is the resolution: the write is what we now believe is on
        // disk, so the banner clears and does not come back for this change.
        ed.save_active();
        assert!(!ed.tabs[0].conflict);
        ed.reconcile_with_disk();
        assert!(!ed.tabs[0].conflict, "the banner must not re-raise on our own save");
        assert!(std::fs::read_to_string(&path).unwrap().contains("// mine"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Closing a tab with unsaved edits asks first, and nothing is removed
    /// until the question is answered. A clean tab still closes on the click.
    #[test]
    fn closing_a_tab_with_unsaved_edits_asks_first() {
        let dir = std::env::temp_dir().join("snor_editor_close_guard_test");
        let _ = std::fs::create_dir_all(&dir);
        let mut ed = Editor::new();
        for i in 0..2 {
            let path = dir.join(format!("g{i}.rs"));
            std::fs::write(&path, "fn main() {}\n").unwrap();
            ed.open_file(path);
        }

        // Clean: the close happens and the list is reported as non-empty.
        assert!(ed.request_close_tab(0));
        assert_eq!(ed.tabs.len(), 1);

        // Dirty: the tab stays and the question is queued.
        ed.tabs[0].text.push_str("// wip\n");
        ed.tabs[0].dirty = true;
        assert!(ed.request_close_tab(0));
        assert_eq!(ed.tabs.len(), 1, "an unsaved tab must survive the click");
        assert_eq!(ed.pending_close, Some(0));
        assert_eq!(ed.unsaved_names(), vec!["g1.rs".to_string()]);

        // "discard" is the only path that removes it.
        ed.pending_close = None;
        assert!(!ed.close_tab(0), "the last tab still reports the empty list");
        assert!(!ed.has_unsaved());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
