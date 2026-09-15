use eframe::egui;
use std::cell::RefCell;
use tree_sitter_highlight::{Highlight, HighlightConfiguration, HighlightEvent, Highlighter};

const TS_LIMIT_BYTES: usize = 100_000;

const NAMES: &[&str] = &[
    "attribute",
    "comment",
    "constant",
    "constant.builtin",
    "constructor",
    "embedded",
    "function",
    "function.builtin",
    "keyword",
    "module",
    "number",
    "operator",
    "property",
    "property.builtin",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "punctuation.special",
    "string",
    "string.special",
    "tag",
    "type",
    "type.builtin",
    "variable",
    "variable.builtin",
    "variable.parameter",
];

fn color_for(name: &str) -> (egui::Color32, bool) {
    let c = |r, g, b| egui::Color32::from_rgb(r, g, b);
    match name {
        "keyword" => (c(0x7A, 0xA2, 0xF7), false),
        "string" | "string.special" => (c(0x98, 0xC3, 0x79), false),
        "comment" => (c(0x7A, 0x85, 0x77), true),
        "number" | "constant" | "constant.builtin" => (c(0xFF, 0x9E, 0x64), false),
        "function" | "function.builtin" | "constructor" => (c(0xE5, 0xC0, 0x7B), false),
        "type" | "type.builtin" | "module" | "tag" => (c(0xC6, 0x78, 0xDD), false),
        "attribute" => (c(0xC6, 0x78, 0xDD), false),
        "operator" => (c(0x89, 0xDC, 0xFF), false),
        _ => (c(0xD5, 0xDA, 0xE2), false),
    }
}

fn make_config(
    name: &str,
    lang: tree_sitter_language::LanguageFn,
    highlights: &str,
    injections: &str,
) -> Option<HighlightConfiguration> {
    let mut config =
        HighlightConfiguration::new(lang.into(), name, highlights, injections, "").ok()?;
    config.configure(NAMES);
    Some(config)
}

struct Configs {
    rust: Option<HighlightConfiguration>,
    json: Option<HighlightConfiguration>,
    js: Option<HighlightConfiguration>,
    toml: Option<HighlightConfiguration>,
}

impl Configs {
    fn new() -> Self {
        Self {
            rust: make_config(
                "rust",
                tree_sitter_rust::LANGUAGE,
                tree_sitter_rust::HIGHLIGHTS_QUERY,
                tree_sitter_rust::INJECTIONS_QUERY,
            ),
            json: make_config(
                "json",
                tree_sitter_json::LANGUAGE,
                tree_sitter_json::HIGHLIGHTS_QUERY,
                "",
            ),
            js: make_config(
                "javascript",
                tree_sitter_javascript::LANGUAGE,
                tree_sitter_javascript::HIGHLIGHT_QUERY,
                tree_sitter_javascript::INJECTIONS_QUERY,
            ),
            toml: make_config(
                "toml",
                tree_sitter_toml_ng::LANGUAGE,
                tree_sitter_toml_ng::HIGHLIGHTS_QUERY,
                "",
            ),
        }
    }

    fn for_lang(&self, lang: &str) -> Option<&HighlightConfiguration> {
        match lang {
            "rust" => self.rust.as_ref(),
            "json" => self.json.as_ref(),
            "js" | "ts" => self.js.as_ref(),
            "toml" => self.toml.as_ref(),
            _ => None,
        }
    }
}

thread_local! {
    static HIGHLIGHTER: RefCell<Highlighter> = RefCell::new(Highlighter::new());
    static CONFIGS: Configs = Configs::new();
}

/// Tree-sitter highlight to LayoutJob. Returns None if unsupported/over limit/failed
/// so the caller can fall back to the keyword highlighter.
pub fn ts_job(text: &str, lang: &str) -> Option<egui::text::LayoutJob> {
    use egui::text::{LayoutJob, TextFormat};
    if text.len() > TS_LIMIT_BYTES || !text.is_char_boundary(text.len()) {
        return None;
    }
    CONFIGS.with(|configs| {
        let config = configs.for_lang(lang)?;
        HIGHLIGHTER.with(|cell| {
            let mut highlighter = cell.borrow_mut();
            let highlights = highlighter
                .highlight(config, text.as_bytes(), None, None, |_| None)
                .ok()?;
            let mono = egui::FontId::monospace(13.0);
            let mut job = LayoutJob::default();
            let mut stack: Vec<(egui::Color32, bool)> = Vec::new();
            let normal = (egui::Color32::from_rgb(0xD5, 0xDA, 0xE2), false);
            for event in highlights {
                match event.ok()? {
                    HighlightEvent::Source { start, end } => {
                        let (color, italics) = stack.last().copied().unwrap_or(normal);
                        // Tree-sitter gives byte offsets; guard against any
                        // non-boundary slice so a weird file can't crash us.
                        let slice = text.get(start..end).unwrap_or("");
                        job.append(
                            slice,
                            0.0,
                            TextFormat {
                                color,
                                font_id: mono.clone(),
                                italics,
                                ..Default::default()
                            },
                        );
                    }
                    HighlightEvent::HighlightStart(Highlight(idx)) => {
                        let name = NAMES.get(idx).copied().unwrap_or("");
                        stack.push(color_for(name));
                    }
                    HighlightEvent::HighlightEnd => {
                        stack.pop();
                    }
                }
            }
            Some(job)
        })
    })
}
