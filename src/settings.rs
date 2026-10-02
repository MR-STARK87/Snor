//! User settings: one small file, read by hand.
//!
//! Snor persisted nothing before this module. `eframe` is built without the
//! `persistence` feature, so the window re-centred itself every launch and the
//! terminal's `Ctrl+wheel` zoom was forgotten the moment the process died. A
//! settings panel therefore needs a file format, a reader and a writer before
//! its first toggle can do anything.
//!
//! Two decisions worth stating, because both were live questions:
//!
//! * **One file in the user profile, not one per project.** `%APPDATA%\Snor\
//!   settings.toml` holds the whole thing. Nothing here is project-scoped, so a
//!   per-project file would be a second format, a second lookup and a second way
//!   for the app to disagree with itself.
//! * **Hand-rolled `key = value`, not a TOML crate.** The format is nine lines
//!   of scalar, and every field is total: a missing key falls back to the
//!   default, an unparseable one falls back to the default, and an unknown key
//!   is ignored. Pulling in a parser to read that would be a dependency earning
//!   its keep on nothing. The `.toml` extension is kept because the file *is*
//!   TOML-shaped and a reader can edit it as such; this reader is simply the
//!   subset Snor writes.
//!
//! * **One key is written by the app, not by a panel row.** `last_folder` is
//!   set when the user opens a folder and read at the next launch, so the
//!   workspace comes back instead of being re-guessed. That is what makes this
//!   file more than preferences: it is the only memory the app has of where it
//!   was. See [`Settings::last_folder`].
//!
//! Every default is byte-identical to the constant it replaces, so the existing
//! test suite proves the settings plumbing changed no behaviour. See the
//! clamping bounds: they exist because these values end up in a font id, a
//! `vt100` buffer size and a `Powershell.exe` invocation, and a settings file
//! is a text file a user can edit.

use std::path::PathBuf;

use crate::terminal::ShellKind;

/// Face a terminal grid renders in, in points, at zoom 1.0.
///
/// The default is `terminal`'s own constant rather than a second `12.5`: the
/// grid measures and paints cells from that one, so a settings default that
/// disagreed with it would render a size nobody chose.
pub const TERMINAL_FONT_DEFAULT: f32 = crate::terminal::TERM_FONT_SIZE;
pub const TERMINAL_FONT_MIN: f32 = 8.0;
pub const TERMINAL_FONT_MAX: f32 = 28.0;

/// Rows of history a `vt100::Parser` keeps. Defaulted from the terminal's
/// constant for the same reason as the font.
pub const SCROLLBACK_DEFAULT: usize = crate::terminal::SCROLLBACK;
/// Below this a scrollback is not history; above it the parser simply eats RAM.
pub const SCROLLBACK_MIN: usize = 100;
pub const SCROLLBACK_MAX: usize = 100_000;

/// Editor code face, in points.
pub const EDITOR_FONT_DEFAULT: f32 = 13.0;
pub const EDITOR_FONT_MIN: f32 = 9.0;
pub const EDITOR_FONT_MAX: f32 = 28.0;

/// Multiplier on egui's own point scale.
pub const UI_SCALE_DEFAULT: f32 = 1.0;
pub const UI_SCALE_MIN: f32 = 0.8;
pub const UI_SCALE_MAX: f32 = 1.6;

/// The keys this file understands, in the order [`Settings::render`] writes
/// them. One list drives the write and the tests walk it, so a key cannot be
/// added to the file's shape without a renderer to match — see
/// [`Settings::value_for`].
///
/// `last_folder` is last because it is the one key that is not a preference: it
/// is written when a folder is opened, and its position in the file is a
/// promise to every file already on disk.
const KEYS: [&str; 9] = [
    "shell",
    "terminal_font",
    "scrollback",
    "editor_font",
    "show_hidden",
    "follow_terminal",
    "dim_level",
    "ui_scale",
    "last_folder",
];

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// Which shell a newly opened terminal tab starts.
    pub shell: ShellKind,
    /// Terminal grid face, in points, at zoom 1.0. `Ctrl+wheel` scales this
    /// live and the result is what gets written back, so the last zoom a user
    /// chose is the one they come back to.
    pub terminal_font: f32,
    /// Rows of scrollback for *new* shells. An existing session sized its
    /// parser at spawn, so this takes effect on the next one.
    pub scrollback: usize,
    /// Editor code face, in points.
    pub editor_font: f32,
    /// Show dotfiles in the explorer. `target/.git/node_modules/.idea` stay
    /// skipped either way — that list is a performance decision, not a
    /// preference.
    pub show_hidden: bool,
    /// Re-root the explorer onto the focused terminal's directory as focus
    /// moves between sessions.
    pub follow_terminal: bool,
    /// Backlight percent applied while Dim Mode is active.
    pub dim_level: u8,
    /// Multiplier on egui's point scale.
    pub ui_scale: f32,
    /// The folder the user opened last, and the one the next launch reopens.
    ///
    /// `None` is a real state, not a missing value: it is what a first run
    /// looks like, and it is what the app asks about rather than guessing a
    /// directory — the guess being how a developer got their build tree, and an
    /// installed user the install folder, on screen as "the workspace". See
    /// `app::resolve_workspace` for the remembered path that has since been
    /// moved or deleted.
    ///
    /// Not clamped, deliberately: the only questions worth asking of a path
    /// are answered where it is used, and both of those are `is_dir`. A value
    /// that fails it is a folder that is not there, which the first-run screen
    /// reports by name.
    pub last_folder: Option<PathBuf>,
}

impl Default for Settings {
    /// Exactly the behaviour the app had before settings existed.
    ///
    /// Written out by hand rather than derived: `#[derive(Default)]` on an
    /// `f32` yields `0.0`, and a `ui_scale` of zero is a window with no size
    /// and a `terminal_font` of zero is a grid of zero-width cells.
    fn default() -> Self {
        Self {
            shell: ShellKind::default(),
            terminal_font: TERMINAL_FONT_DEFAULT,
            scrollback: SCROLLBACK_DEFAULT,
            editor_font: EDITOR_FONT_DEFAULT,
            show_hidden: false,
            follow_terminal: true,
            dim_level: crate::brightness::DEFAULT_DIM_LEVEL,
            ui_scale: UI_SCALE_DEFAULT,
            last_folder: None,
        }
    }
}

impl Settings {
    /// Force every field into the range the rest of the app can survive.
    ///
    /// Called on everything loaded from disk and again before writing, so a
    /// hand-edited file cannot reach a font id, a buffer size or a viewport
    /// command with a value that breaks them.
    pub fn clamped(mut self) -> Self {
        self.terminal_font = self
            .terminal_font
            .clamp(TERMINAL_FONT_MIN, TERMINAL_FONT_MAX);
        self.editor_font = self.editor_font.clamp(EDITOR_FONT_MIN, EDITOR_FONT_MAX);
        self.scrollback = self.scrollback.clamp(SCROLLBACK_MIN, SCROLLBACK_MAX);
        self.ui_scale = self.ui_scale.clamp(UI_SCALE_MIN, UI_SCALE_MAX);
        // 0 would black the panel out, and Dim Mode would then have no way to
        // say it was on; the same floor the brightness backend uses.
        self.dim_level = crate::brightness::clamp_level(self.dim_level);
        self
    }

    /// Parse the file format. Total: every field that is absent or malformed
    /// keeps its default, so a half-written file still opens the app.
    ///
    /// Unknown keys are ignored rather than rejected. The file is one a user
    /// may edit by hand and may edit *before* upgrading, and refusing to start
    /// over a key from a newer version would be the wrong trade.
    pub fn parse(text: &str) -> Self {
        let mut out = Settings::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim().trim_matches('"').trim();
            match key {
                "shell" => {
                    if let Some(shell) = shell_from_key(value) {
                        out.shell = shell;
                    }
                }
                "terminal_font" => assign_f32(&mut out.terminal_font, value),
                "scrollback" => {
                    if let Ok(v) = value.parse::<usize>() {
                        out.scrollback = v;
                    }
                }
                "editor_font" => assign_f32(&mut out.editor_font, value),
                "show_hidden" => assign_bool(&mut out.show_hidden, value),
                "follow_terminal" => assign_bool(&mut out.follow_terminal, value),
                "dim_level" => {
                    if let Ok(v) = value.parse::<u8>() {
                        out.dim_level = v;
                    }
                }
                "ui_scale" => assign_f32(&mut out.ui_scale, value),
                // Empty means "no folder chosen", not "the empty path": the
                // file writes an empty value for that state, so `None` has to
                // read back as `None` or the key would be the one field in the
                // file that cannot survive a save.
                "last_folder" => {
                    out.last_folder = (!value.is_empty()).then(|| PathBuf::from(value));
                }
                _ => {}
            }
        }
        out.clamped()
    }

    /// The file's value for one key, or `None` for a key this version does not
    /// write. The one `match` behind [`Settings::render`].
    fn value_for(&self, key: &str) -> Option<String> {
        let value = match key {
            "shell" => shell_key(self.shell).to_string(),
            "terminal_font" => fmt_f32(self.terminal_font),
            "scrollback" => self.scrollback.to_string(),
            "editor_font" => fmt_f32(self.editor_font),
            "show_hidden" => self.show_hidden.to_string(),
            "follow_terminal" => self.follow_terminal.to_string(),
            "dim_level" => self.dim_level.to_string(),
            "ui_scale" => fmt_f32(self.ui_scale),
            // Empty when there is none. `render` writes every key, and
            // "written as empty" is how the file says `None` without needing a
            // second format for it.
            "last_folder" => self
                .last_folder
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            _ => return None,
        };
        Some(value)
    }

    /// Render the file. Every key in [`KEYS`] is written every time, in that
    /// order, so the file is a complete description of the settings rather than
    /// a diff against whatever the defaults happened to be.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("# Snor settings. Edit freely; the app rewrites this file.\n");
        out.push_str("# Ranges: terminal_font 8-28, editor_font 9-28, ");
        out.push_str("scrollback 100-100000, ui_scale 0.8-1.6, dim_level 1-100.\n\n");
        out.push_str("# last_folder is written when you open a folder; clear it to be asked again.\n\n");
        for key in KEYS {
            if let Some(value) = self.value_for(key) {
                out.push_str(&format!("{key} = {value}\n"));
            }
        }
        out
    }

    /// Where the file lives: `%APPDATA%\Snor\settings.toml`.
    ///
    /// Falls back to the working directory when `APPDATA` is unset, which is
    /// the one case where the app is not on Windows — and a settings file
    /// beside the binary beats no settings file at all.
    pub fn path() -> PathBuf {
        match std::env::var_os("APPDATA") {
            Some(appdata) if !appdata.is_empty() => {
                PathBuf::from(appdata).join("Snor").join("settings.toml")
            }
            _ => PathBuf::from("snor-settings.toml"),
        }
    }

    /// Read the settings file, plus the one reading complaint there is to make.
    ///
    /// Never fails: an unreadable or malformed file is treated as "no
    /// preferences yet" rather than as a reason not to start. The user can still
    /// open the app and fix it from the panel. The path is a parameter so this
    /// can be tested against a temp file instead of the real profile.
    ///
    /// `parse` is total and silent on purpose — a bad value keeps its default
    /// rather than blocking startup — but an unrecognised `shell = …` is worth
    /// naming once: the file says `shell = nu`, every launch starts PowerShell
    /// instead, and nothing anywhere says why. Every other key is either clamped
    /// into range (and so visibly wrong on screen) or ignored without a
    /// consequence the user could notice. Returns the settings and, when it
    /// applies, the complaint.
    pub fn load_from_with_warning(path: &std::path::Path) -> (Self, Option<String>) {
        match std::fs::read_to_string(path) {
            Ok(text) => (Self::parse(&text), unknown_shell(&text)),
            Err(_) => (Self::default(), None),
        }
    }

    /// Write the settings file, creating its directory if needed.
    ///
    /// Returns the error rather than swallowing it: the panel shows it, because
    /// a toggle that silently fails to persist across restarts is worse than
    /// one that says why.
    pub fn save(&self) -> Result<(), String> {
        self.save_to(&Self::path())
    }

    /// See [`Settings::save`].
    pub fn save_to(&self, path: &std::path::Path) -> Result<(), String> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
        }
        std::fs::write(path, self.clone().clamped().render())
            .map_err(|e| format!("could not write {}: {e}", path.display()))
    }
}

/// Assign an `f32` only when it parses. A malformed value leaves the field at
/// its default rather than at zero.
fn assign_f32(slot: &mut f32, value: &str) {
    if let Ok(v) = value.parse::<f32>()
        && v.is_finite()
    {
        *slot = v;
    }
}

/// `true`/`false`, and the `1`/`0` someone may well have typed instead.
fn assign_bool(slot: &mut bool, value: &str) {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => *slot = true,
        "false" | "0" | "no" | "off" => *slot = false,
        _ => {}
    }
}

/// Stable key for [`ShellKind`]. Deliberately *not* `label()`: the label is a
/// menu string ("PowerShell 7") and is free to change, while a key is a promise
/// to every settings file already on disk.
fn shell_key(shell: ShellKind) -> &'static str {
    match shell {
        ShellKind::PowerShell => "powershell",
        ShellKind::Pwsh => "pwsh",
        ShellKind::Cmd => "cmd",
        ShellKind::GitBash => "gitbash",
        ShellKind::Wsl => "wsl",
    }
}

fn shell_from_key(key: &str) -> Option<ShellKind> {
    match key.to_ascii_lowercase().as_str() {
        "powershell" => Some(ShellKind::PowerShell),
        "pwsh" => Some(ShellKind::Pwsh),
        "cmd" | "command_prompt" => Some(ShellKind::Cmd),
        "gitbash" | "git_bash" | "bash" => Some(ShellKind::GitBash),
        "wsl" => Some(ShellKind::Wsl),
        _ => None,
    }
}

/// The `shell = …` value in `text` that names no shell this version knows.
///
/// [`Settings::parse`] ignores it silently, which is the right default for this
/// format and wrong for this one key: a shell is a deliberate choice, and one
/// that quietly reverts to PowerShell on every launch reads as the app being
/// broken rather than as a typo. Returns the offending value so the settings
/// panel can name it. Total: anything that is not a `shell` line is skipped.
fn unknown_shell(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "shell" {
            continue;
        }
        let value = value.trim().trim_matches('"').trim();
        if !value.is_empty() && shell_from_key(value).is_none() {
            return Some(format!(
                "settings: `shell = {value}` names no shell this version knows — starting {} instead",
                ShellKind::default().label()
            ));
        }
    }
    None
}

/// Render an `f32` without an exponent or a trailing `.0`.
///
/// `format!("{}", 12.5_f32)` is fine, but a value that round-trips through
/// clamping can come out as `12.500001`, and a settings file full of that reads
/// like a bug. Three decimals is finer than any knob here.
fn fmt_f32(v: f32) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s.is_empty() { "0".to_string() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_behaviour_the_app_had_before_settings() {
        let d = Settings::default();
        assert_eq!(d.shell, ShellKind::PowerShell);
        assert_eq!(d.terminal_font, 12.5);
        assert_eq!(d.scrollback, 1000);
        assert_eq!(d.editor_font, 13.0);
        assert!(!d.show_hidden);
        assert!(d.follow_terminal);
        assert_eq!(d.dim_level, 10);
        assert_eq!(d.ui_scale, 1.0);
    }

    #[test]
    fn a_settings_file_round_trips() {
        let s = Settings {
            shell: ShellKind::GitBash,
            terminal_font: 15.5,
            scrollback: 4000,
            editor_font: 14.0,
            show_hidden: true,
            follow_terminal: false,
            dim_level: 25,
            ui_scale: 1.25,
            // A path with a space in it, because the round trip that matters
            // most is the one the parser would break first.
            last_folder: Some(PathBuf::from(r"C:\My work folder\Snor")),
        };

        let text = s.render();
        let back = Settings::parse(&text);
        assert_eq!(back, s, "render/parse must be lossless:\n{text}");
    }

    #[test]
    fn a_round_trip_survives_every_piece_of_the_file_being_missing() {
        // An empty file is "no preferences yet", not an error.
        assert_eq!(Settings::parse(""), Settings::default());
        assert_eq!(Settings::parse("\n\n  \n"), Settings::default());
    }

    #[test]
    fn comments_blank_lines_and_stray_text_are_ignored() {
        let text = "\
# a comment
; another comment

shell = pwsh
this line has no equals sign
terminal_font = 14
";
        let s = Settings::parse(text);
        assert_eq!(s.shell, ShellKind::Pwsh);
        assert_eq!(s.terminal_font, 14.0);
        // Everything unmentioned keeps its default.
        assert_eq!(s.scrollback, SCROLLBACK_DEFAULT);
    }

    #[test]
    fn an_unknown_key_is_ignored_rather_than_fatal() {
        // A file written by a newer version must not stop this one starting.
        let s = Settings::parse("editor_font = 14\nsome_future_knob = 7\n");
        assert_eq!(s.editor_font, 14.0);
        assert_eq!(s.scrollback, SCROLLBACK_DEFAULT);
    }

    #[test]
    fn a_malformed_value_falls_back_to_its_default_not_to_zero() {
        let s = Settings::parse(
            "terminal_font = wide\nscrollback = lots\nshow_hidden = perhaps\nui_scale =\n",
        );
        assert_eq!(s.terminal_font, TERMINAL_FONT_DEFAULT);
        assert_eq!(s.scrollback, SCROLLBACK_DEFAULT);
        assert!(!s.show_hidden);
        assert_eq!(s.ui_scale, UI_SCALE_DEFAULT);
    }

    #[test]
    fn out_of_range_values_are_clamped_on_load() {
        let s = Settings::parse(
            "terminal_font = 900\nscrollback = 999999999\neditor_font = 0.1\nui_scale = 50\ndim_level = 0\n",
        );
        assert_eq!(s.terminal_font, TERMINAL_FONT_MAX);
        assert_eq!(s.scrollback, SCROLLBACK_MAX);
        assert_eq!(s.editor_font, EDITOR_FONT_MIN);
        assert_eq!(s.ui_scale, UI_SCALE_MAX);
        // 0 would black the screen out and leave Dim Mode no way to say so.
        assert_eq!(s.dim_level, 1);
    }

    #[test]
    fn whitespace_quotes_and_case_around_a_bool_do_not_matter() {
        for text in [
            "show_hidden = true",
            "  show_hidden=true  ",
            "show_hidden = \"true\"",
            "show_hidden = TRUE",
            "show_hidden = 1",
        ] {
            assert!(Settings::parse(text).show_hidden, "failed on {text:?}");
        }
        for text in ["show_hidden = false", "show_hidden = 0", "show_hidden = off"] {
            assert!(!Settings::parse(text).show_hidden, "failed on {text:?}");
        }
    }

    #[test]
    fn an_unknown_shell_name_keeps_the_default_rather_than_guessing() {
        let s = Settings::parse("shell = fish");
        assert_eq!(s.shell, ShellKind::PowerShell);
    }

    #[test]
    fn every_shell_kind_survives_a_round_trip() {
        for shell in ShellKind::ALL {
            let s = Settings { shell, ..Settings::default() };
            assert_eq!(Settings::parse(&s.render()).shell, shell);
        }
    }

    #[test]
    fn render_writes_every_key_so_the_file_is_a_complete_description() {
        let text = Settings::default().render();
        for key in KEYS {
            assert!(
                text.lines().any(|l| l.trim_start().starts_with(key)),
                "key `{key}` missing from rendered settings:\n{text}"
            );
        }
    }

    #[test]
    fn every_listed_key_has_a_renderer() {
        // The list drives the file's shape; this is what stops a key being
        // added to it and then silently never written.
        let s = Settings::default();
        for key in KEYS {
            assert!(s.value_for(key).is_some(), "no renderer for `{key}`");
        }
    }

    #[test]
    fn a_saved_file_is_read_back_identical() {
        // The whole point: a toggle survives a restart. Uses a real temp file
        // rather than the real profile, so running the tests cannot rewrite
        // the developer's own settings.
        let dir = std::env::temp_dir().join(format!(
            "snor_settings_test_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let path = dir.join("nested").join("settings.toml");
        let _ = std::fs::remove_dir_all(&dir);

        let s = Settings {
            editor_font: 16.0,
            show_hidden: true,
            ..Settings::default()
        };
        s.save_to(&path).expect("save must create the directory and write");

        assert_eq!(Settings::load_from_with_warning(&path).0, s);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_loads_the_defaults_instead_of_failing() {
        let path = std::env::temp_dir().join("snor-settings-that-does-not-exist.toml");
        let _ = std::fs::remove_file(&path);
        assert_eq!(Settings::load_from_with_warning(&path).0, Settings::default());
    }

    /// A remembered folder has to survive a save/load, not just the renderer.
    #[test]
    fn a_remembered_folder_with_spaces_survives_a_round_trip() {
        // This repository's own path. A Windows path with a space in it is not
        // a hypothetical case here, and a parser that split on whitespace
        // would take the first half of it.
        let path = PathBuf::from(r"C:\My work folder\Snor");
        let s = Settings {
            last_folder: Some(path.clone()),
            ..Settings::default()
        };
        assert_eq!(Settings::parse(&s.render()).last_folder, Some(path));
    }

    #[test]
    fn an_empty_remembered_folder_means_none_was_ever_chosen() {
        assert_eq!(Settings::default().last_folder, None);
        assert_eq!(Settings::parse("last_folder =").last_folder, None);
        assert_eq!(Settings::parse("last_folder = \"\"").last_folder, None);
        // The key is still written, so the file describes itself completely.
        assert!(
            Settings::default()
                .render()
                .lines()
                .any(|line| line.trim_start().starts_with("last_folder")),
            "the key must be present even when empty"
        );
    }

    /// Hand-quoted, because a user copying a path out of Explorer gets a
    /// quoted one, and the same tolerance every other value already has.
    #[test]
    fn a_quoted_folder_path_is_accepted() {
        assert_eq!(
            Settings::parse(r#"last_folder = "C:\My work folder\Snor""#).last_folder,
            Some(PathBuf::from(r"C:\My work folder\Snor"))
        );
    }

    #[test]
    fn saving_clamps_so_a_hand_edited_file_cannot_persist_a_breaking_value() {
        let s = Settings { ui_scale: 99.0, ..Settings::default() };
        let text = s.clone().clamped().render();
        assert_eq!(Settings::parse(&text).ui_scale, UI_SCALE_MAX);
    }

    #[test]
    fn floats_render_without_an_exponent_or_a_trailing_zero() {
        assert_eq!(fmt_f32(12.5), "12.5");
        assert_eq!(fmt_f32(13.0), "13");
        assert_eq!(fmt_f32(1.0), "1");
        assert_eq!(fmt_f32(0.0), "0");
    }

    /// A shell name nobody recognises is the one value the reader reports.
    ///
    /// `parse` keeps the default either way; the difference is that the user is
    /// told, rather than left wondering why `shell = nu` starts PowerShell every
    /// launch.
    #[test]
    fn an_unknown_shell_is_named_rather_than_silently_ignored() {
        let bad = "shell = nu\nscrollback = 500\n";
        assert_eq!(Settings::parse(bad).shell, ShellKind::PowerShell);
        let warning = unknown_shell(bad).expect("an unknown shell must be reported");
        assert!(warning.contains("nu"), "the warning must name the value: {warning}");

        // A known shell, an empty value and a commented line are all silent.
        assert!(unknown_shell("shell = wsl\n").is_none());
        assert!(unknown_shell("shell =\n").is_none());
        assert!(unknown_shell("# shell = nu\n").is_none());
        assert!(unknown_shell("").is_none());
    }
}
