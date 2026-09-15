use eframe::egui;
use std::path::{Path, PathBuf};

const LARGE_FILE_BYTES: usize = 500_000;
const PLAIN_HIGHLIGHT_BYTES: usize = 200_000;

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
    egui::Color32::from_rgb(0x7D, 0xD3, 0xA8)
}
fn color_string() -> egui::Color32 {
    egui::Color32::from_rgb(0xD9, 0xA8, 0x6C)
}
fn color_comment() -> egui::Color32 {
    egui::Color32::from_rgb(0x8B, 0x94, 0xA3)
}
fn color_number() -> egui::Color32 {
    egui::Color32::from_rgb(0x7A, 0xA2, 0xF7)
}
fn color_normal() -> egui::Color32 {
    egui::Color32::from_rgb(0xD5, 0xDA, 0xE2)
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

pub fn highlight_job(text: &str, lang: &str) -> egui::text::LayoutJob {
    use egui::text::{LayoutJob, TextFormat};
    let mut job = LayoutJob::default();
    if text.len() > PLAIN_HIGHLIGHT_BYTES {
        job.append(
            text,
            0.0,
            TextFormat {
                color: color_normal(),
                font_id: egui::FontId::monospace(13.0),
                ..Default::default()
            },
        );
        return job;
    }
    let kw = keywords(lang);
    let mono = egui::FontId::monospace(13.0);
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

    let bytes = text.as_bytes();
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

    while i < bytes.len() {
        let c = bytes[i] as char;
        // line comments: // for most, # for toml/py/ps1/md
        let hash_comment = matches!(lang, "toml" | "py" | "ps1" | "md");
        if c == '/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' && !hash_comment {
            flush_word(&mut word, &mut job);
            let start = i;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            job.append(&text[start..i], 0.0, fmt_com.clone());
            continue;
        }
        if c == '#' && hash_comment {
            flush_word(&mut word, &mut job);
            let start = i;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            job.append(&text[start..i], 0.0, fmt_com.clone());
            continue;
        }
        if c == '"' || c == '\'' {
            flush_word(&mut word, &mut job);
            let quote = c;
            let start = i;
            i += 1;
            while i < bytes.len() {
                let d = bytes[i] as char;
                if d == '\\' {
                    i += 2;
                    continue;
                }
                i += 1;
                if d == quote {
                    break;
                }
                if quote == '\'' && (d == '\n') {
                    break;
                }
            }
            let end = i.min(text.len());
            job.append(&text[start..end], 0.0, fmt_str.clone());
            continue;
        }
        if c.is_ascii_digit() && word.is_empty() {
            let start = i;
            while i < bytes.len()
                && ((bytes[i] as char).is_ascii_alphanumeric()
                    || bytes[i] == b'.'
                    || bytes[i] == b'_')
            {
                i += 1;
            }
            job.append(&text[start..i], 0.0, fmt_num.clone());
            continue;
        }
        if is_word_char(c) {
            word.push(c);
            i += 1;
            continue;
        }
        flush_word(&mut word, &mut job);
        job.append(&text[i..i + 1], 0.0, fmt_normal.clone());
        i += 1;
    }
    flush_word(&mut word, &mut job);
    job
}

pub struct OpenBuffer {
    pub path: PathBuf,
    pub lang: String,
    pub text: String,
    pub dirty: bool,
    pub too_large: bool,
    lines: usize,
}

impl OpenBuffer {
    fn open(path: PathBuf) -> anyhow::Result<Self> {
        let meta = std::fs::metadata(&path)?;
        let too_large = meta.len() > LARGE_FILE_BYTES as u64;
        let text = std::fs::read_to_string(&path).unwrap_or_else(|_| String::from("<binary>"));
        let lang = language_for(&path).to_string();
        let lines = ropey::Rope::from_str(&text).len_lines();
        Ok(Self {
            path,
            lang,
            text,
            dirty: false,
            too_large,
            lines,
        })
    }

    fn save(&mut self) -> anyhow::Result<()> {
        std::fs::write(&self.path, &self.text)?;
        self.dirty = false;
        Ok(())
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
        }
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
        let byte: usize = buf.text.lines().take(line).map(|l| l.len() + 1).sum();
        let byte = byte.min(buf.text.len());
        let ch = buf.text[..byte].chars().count();
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

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let mut close_idx: Option<usize> = None;
            let mut tab_switched = false;
            for (idx, tab) in self.tabs.iter().enumerate() {
                let name = tab
                    .path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                let label = if tab.dirty { format!("{name} *") } else { name };
                if ui.selectable_label(idx == self.active, label).clicked() {
                    self.active = idx;
                    tab_switched = true;
                }
                if ui.small_button("x").clicked() {
                    close_idx = Some(idx);
                }
            }
            if self.tabs.is_empty() {
                let _ = ui.selectable_label(true, "no file — double-click in Explorer");
            } else if ui.small_button("+").clicked() {
                // no-op: open via explorer
            }
            if let Some(idx) = close_idx {
                self.tabs.remove(idx);
                if self.active >= self.tabs.len() && !self.tabs.is_empty() {
                    self.active = self.tabs.len() - 1;
                }
                tab_switched = true;
            }
            if tab_switched {
                self.recompute_find();
            }
        });
        ui.separator();

        if let Some(err) = &self.error {
            ui.colored_label(egui::Color32::from_rgb(0xE0, 0x6C, 0x75), err);
        }

        if self.tabs.is_empty() {
            self.find_open = false;
            egui::ScrollArea::both()
                .id_salt("snor_editor_empty")
                .show(ui, |ui| {
                    ui.monospace("// open a file from Explorer to edit.");
                    ui.monospace("// tabs + tree-sitter highlight + Ctrl+S to save.");
                });
            return;
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
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("save (Ctrl+S)").clicked() {
                        want_save = true;
                    }
                });
            });
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
        let editor_id = ui.make_persistent_id("snor_editor_text");
        let mut text_changed = false;
        egui::ScrollArea::both()
            .id_salt("snor_editor_text")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
                    let mut job = crate::syntax::ts_job(text.as_str(), &lang)
                        .unwrap_or_else(|| highlight_job(text.as_str(), &lang));
                    job.wrap.max_width = wrap_width;
                    ui.fonts_mut(|f| f.layout_job(job))
                };
                let resp = ui.add(
                    egui::TextEdit::multiline(&mut buf.text)
                        .id(editor_id)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .frame(egui::Frame::NONE)
                        .layouter(&mut layouter)
                        .interactive(editable),
                );
                if resp.changed() {
                    buf.dirty = true;
                    buf.refresh_lines();
                    text_changed = true;
                }
            });
        if text_changed && self.find_open {
            self.recompute_find();
        }
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
    fn find_matches_lines_case_insensitive() {
        let text = "fn alpha() {}\nlet beta = 1;\n// ALPHA note\n";
        assert_eq!(find_line_matches(text, "alpha"), vec![0, 2]);
        assert_eq!(find_line_matches(text, "zzz"), Vec::<usize>::new());
        assert_eq!(find_line_matches(text, ""), Vec::<usize>::new());
    }
}
