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

/// Which shell a new tab starts.
///
/// Snor used to hardcode `powershell.exe -NoLogo -NoProfile -NoExit`. A good
/// default is not the same thing as the only option: the agents people run
/// here are as likely to be started from `pwsh`, and on Windows a lot of real
/// work happens in WSL or Git Bash. Adding one is a few lines of
/// `CommandBuilder`; what makes it worth having is that the tab title can then
/// say which shell a tab is, so a window full of shells stays legible.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ShellKind {
    #[default]
    PowerShell,
    Pwsh,
    Cmd,
    GitBash,
    Wsl,
}

impl ShellKind {
    /// Menu order: the Windows default first, then the ones that are installed
    /// on purpose.
    pub const ALL: [ShellKind; 5] = [
        ShellKind::PowerShell,
        ShellKind::Pwsh,
        ShellKind::Cmd,
        ShellKind::GitBash,
        ShellKind::Wsl,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ShellKind::PowerShell => "PowerShell",
            ShellKind::Pwsh => "PowerShell 7",
            ShellKind::Cmd => "Command Prompt",
            ShellKind::GitBash => "Git Bash",
            ShellKind::Wsl => "WSL",
        }
    }

    /// What a tab is called. Short, lower case, no version: the tab says which
    /// shell it is, because with five of them available a bare "powershell 3"
    /// is a guess about a shell the user picked deliberately.
    fn title_name(self) -> &'static str {
        match self {
            ShellKind::PowerShell => "powershell",
            ShellKind::Pwsh => "pwsh",
            ShellKind::Cmd => "cmd",
            ShellKind::GitBash => "bash",
            ShellKind::Wsl => "wsl",
        }
    }

    fn program(self) -> &'static str {
        match self {
            ShellKind::PowerShell => "powershell.exe",
            ShellKind::Pwsh => "pwsh.exe",
            ShellKind::Cmd => "cmd.exe",
            ShellKind::GitBash => "bash.exe",
            ShellKind::Wsl => "wsl.exe",
        }
    }

    /// `-NoExit` for the PowerShell family so the window persists the way a
    /// terminal is expected to; `-i -l` for Git Bash, which needs both to give
    /// a login shell with its completion loaded.
    fn args(self) -> &'static [&'static str] {
        match self {
            ShellKind::PowerShell | ShellKind::Pwsh => &["-NoLogo", "-NoProfile", "-NoExit"],
            ShellKind::GitBash => &["-i", "-l"],
            ShellKind::Cmd | ShellKind::Wsl => &[],
        }
    }

    /// The shells this machine can plausibly start right now.
    fn installed() -> Vec<ShellKind> {
        installed_from(
            &std::env::var("PATH").unwrap_or_default(),
            std::env::var("COMSPEC").ok().as_deref(),
        )
    }
}

/// Which of [`ShellKind::ALL`] are actually present, decided by looking for the
/// executable on `PATH`.
///
/// A lookup rather than a probe: launching `pwsh -c exit` to see whether it
/// exists would spawn a process on every start, and spawning things to draw a
/// menu is exactly what a lean app should not do. Split out from
/// [`ShellKind::installed`] so the rule can be tested without planting executables
/// on the machine running the tests.
///
/// `powershell.exe` is taken as present whatever the lookup says: it ships with
/// Windows, and a machine with a trimmed `PATH` must not end up with no shell
/// listed at all.
fn installed_from(path_env: &str, comspec: Option<&str>) -> Vec<ShellKind> {
    let dirs: Vec<&str> = path_env
        .split(';')
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .collect();
    let on_path = |exe: &str| -> bool {
        let exe = exe.to_lowercase();
        dirs.iter().any(|dir| {
            std::fs::read_dir(dir)
                .map(|entries| {
                    entries
                        .flatten()
                        .any(|e| e.file_name().to_string_lossy().to_lowercase() == exe)
                })
                .unwrap_or(false)
        })
    };
    ShellKind::ALL
        .iter()
        .copied()
        .filter(|shell| match shell {
            ShellKind::PowerShell => true,
            ShellKind::Cmd => {
                comspec.is_some_and(|c| std::path::Path::new(c).is_file()) || on_path("cmd.exe")
            }
            ShellKind::Pwsh => on_path("pwsh.exe"),
            ShellKind::GitBash => on_path("bash.exe"),
            ShellKind::Wsl => on_path("wsl.exe"),
        })
        .collect()
}

/// Tab label for the n-th shell of its kind.
///
/// The first is bare ("powershell", "pwsh"); later ones are numbered from 2,
/// so a single tab never reads "powershell 1".
fn shell_title(n: usize, shell: ShellKind) -> String {
    let name = shell.title_name();
    if n <= 1 {
        name.to_owned()
    } else {
        format!("{name} {n}")
    }
}

/// What one read from a shell told us that is not screen content.
///
/// The two signals a shell has for "I am trying to get your attention": a bell
/// (`BEL`) and a title assignment (`ESC ] 0 ; … BEL` or `ST`, which is how
/// agents like opencode announce what they are doing). vt100 0.16 exposes
/// neither, so they are read off the same bytes the parser is fed.
#[derive(Default, PartialEq, Eq, Debug)]
struct Notifications {
    bell: bool,
    title: Option<String>,
}

/// Longest tail kept for a sequence split across two reads. An OSC title is
/// short; anything longer than this was not a title.
const NOTIF_TAIL_MAX: usize = 512;

/// Read a chunk for bells and title assignments, holding an unterminated
/// escape in `tail` for the next chunk.
///
/// Chunk boundaries are wherever the pty reader happened to flush, so a title
/// routinely arrives in two pieces. Keeping the tail — rather than scanning each
/// chunk in isolation — is what makes a split sequence work; the tail is only
/// ever kept from an incomplete `ESC` onward, so nothing is counted twice.
fn scan_notifications(tail: &mut Vec<u8>, chunk: &[u8]) -> Notifications {
    tail.extend_from_slice(chunk);
    let mut out = Notifications::default();
    let buf = std::mem::take(tail);
    let mut i = 0;
    while i < buf.len() {
        if buf[i] == 0x07 {
            out.bell = true;
            i += 1;
            continue;
        }
        if buf[i] != 0x1b {
            i += 1;
            continue;
        }
        // An ESC with nothing after it is the first half of a sequence.
        if i + 1 >= buf.len() {
            break;
        }
        if buf[i + 1] != b']' {
            // Some other escape (a colour, a cursor move). Skipped by one so the
            // next byte is still examined — a BEL can follow immediately.
            i += 1;
            continue;
        }
        // OSC: `ESC ] Ps ; Pt (BEL | ESC \)`.
        let body = i + 2;
        let mut end = None;
        let mut j = body;
        while j < buf.len() {
            if buf[j] == 0x07 {
                end = Some((j, j + 1));
                break;
            }
            if buf[j] == 0x1b && j + 1 < buf.len() && buf[j + 1] == b'\\' {
                end = Some((j, j + 2));
                break;
            }
            if buf[j] == 0x1b && j + 1 >= buf.len() {
                break;
            }
            j += 1;
        }
        let Some((stop, resume)) = end else {
            // Unterminated: keep from the ESC so the next chunk can finish it.
            break;
        };
        let payload = String::from_utf8_lossy(&buf[body..stop]).to_string();
        // `Ps;Pt`: 0 = icon and title, 1 = icon, 2 = title. Anything else is a
        // sequence for a terminal emulator feature this app does not implement.
        if let Some((ps, pt)) = payload.split_once(';')
            && matches!(ps, "0" | "1" | "2")
            && !pt.trim().is_empty()
        {
            out.title = Some(pt.to_string());
        }
        i = resume;
    }
    // Either the incomplete sequence, or nothing at all.
    let keep_from = if i < buf.len() { i } else { buf.len() };
    let kept = &buf[keep_from..];
    let kept = if kept.len() > NOTIF_TAIL_MAX {
        &kept[kept.len() - NOTIF_TAIL_MAX..]
    } else {
        kept
    };
    *tail = kept.to_vec();
    out
}

/// A text selection over the *displayed* terminal grid, in cells.
///
/// Cells rather than characters because a cell is what the pty was sized to and
/// what `term_job` walks. `anchor` is where the press landed and `head` is where
/// the pointer is now, so a selection dragged up-and-left is the same thing as
/// the one dragged down-and-right; normalisation happens at read time.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Selection {
    anchor: (u16, u16),
    head: (u16, u16),
}

impl Selection {
    fn new(at: (u16, u16)) -> Self {
        Self {
            anchor: at,
            head: at,
        }
    }

    /// True while the press has not moved: a click, which must not leave a
    /// one-cell selection behind for the next copy to pick up.
    fn is_click(&self) -> bool {
        self.anchor == self.head
    }

    /// `(first, last)` in reading order.
    fn normalized(&self) -> ((u16, u16), (u16, u16)) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

}

/// The copied form of a selection: one line per selected row, trailing blanks
/// removed.
///
/// This trimming is the whole reason the selection is ours rather than egui's.
/// The grid is a fixed-width rectangle, so every row is padded with spaces out
/// to the last column for rendering, and a copy that kept that padding would
/// paste a wall of trailing whitespace into whatever prompt it was aimed at.
///
/// Reads through `screen.cell`, which means it copies what is *on screen* —
/// history when the terminal is scrolled back, which is the only thing the user
/// could have meant by selecting it.
fn selection_text(screen: &vt100::Screen, sel: Selection) -> String {
    let (start, end) = sel.normalized();
    let cols = screen.size().1;
    let mut lines: Vec<String> = Vec::new();
    for row in start.0..=end.0 {
        let first = if row == start.0 { start.1 } else { 0 };
        let last = if row == end.0 {
            end.1
        } else {
            cols.saturating_sub(1)
        };
        let mut line = String::new();
        for col in first..=last {
            if let Some(cell) = screen.cell(row, col) {
                line.push_str(cell.contents());
            }
        }
        lines.push(line.trim_end().to_string());
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

/// A pane counts as working while it has written something in the last few
/// seconds. Ten is long enough that a shell printing its prompt looks alive and
/// a command that takes a moment to start does not look dead.
const PANE_ACTIVE_SECS: u64 = 10;

/// How long a pane has been quiet, in as few characters as still say something.
///
/// Coarse on purpose: the question is "is this agent working or waiting", and
/// seconds only matter in the first minute after something happens.
fn short_idle(idle: std::time::Duration) -> String {
    let secs = idle.as_secs();
    match secs {
        0..=9 => "now".to_string(),
        10..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m", secs / 60),
        _ => format!("{}h", secs / 3600),
    }
}

/// Text scale bounds for the terminal grid.
const ZOOM_MIN: f32 = 0.6;
const ZOOM_MAX: f32 = 2.4;

/// Terminal-wide view state, shared by the tabbed layout and Flow Mode panes
/// because both draw the same grid at the same scale.
struct ViewState {
    /// Multiplier on [`TERM_FONT_SIZE`]. 1.0 is the size the mock was measured
    /// at, so nothing about the default layout moves.
    zoom: f32,
    /// Sub-row wheel remainder, in points.
    ///
    /// A trackpad delivers a few points per frame; converting each frame's delta
    /// to whole rows on its own would round every one of them to nothing and a
    /// slow two-finger scroll would do absolutely nothing.
    wheel_acc: f32,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            wheel_acc: 0.0,
        }
    }
}

/// What the wheel over a terminal grid means.
enum WheelIntent {
    None,
    /// Rows to move through history, positive for older output.
    Scroll(i32),
    /// A zoom factor to multiply by, straight from egui's own gesture handling.
    Zoom(f32),
}

/// Translate the wheel over `rect` into history movement or a zoom.
///
/// egui computes the Ctrl-wheel zoom factor for us (`InputState::zoom_delta`,
/// which is 1.0 when nothing was scrolled), so the speed and smoothing stay the
/// window's own rather than a second invention here. Neither the scroll delta nor
/// the zoom factor is used anywhere else in this app, so consuming the scroll
/// delta is enough to stop anything else reacting to the same gesture.
fn wheel_intent(
    ui: &mut eframe::egui::Ui,
    rect: eframe::egui::Rect,
    cell_h: f32,
    view: &mut ViewState,
) -> WheelIntent {
    if !ui.rect_contains_pointer(rect) {
        return WheelIntent::None;
    }
    let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
    if (zoom - 1.0).abs() > 1e-4 {
        ui.input_mut(|i| i.smooth_scroll_delta = eframe::egui::Vec2::ZERO);
        return WheelIntent::Zoom(zoom);
    }
    if scroll == 0.0 {
        return WheelIntent::None;
    }
    ui.input_mut(|i| i.smooth_scroll_delta = eframe::egui::Vec2::ZERO);
    // Positive is content moving down, i.e. scrolling back into history.
    let cell_h = cell_h.max(1.0);
    view.wheel_acc += scroll;
    let rows = (view.wheel_acc / cell_h).trunc();
    view.wheel_acc -= rows * cell_h;
    if rows == 0.0 {
        return WheelIntent::None;
    }
    WheelIntent::Scroll(rows as i32)
}

/// Which cell of a grid the pointer is over, if it is over one at all.
///
/// Origin is the top-left of the drawn grid, not the pane: the grid is where
/// the shell's cells are, and the header band above it is not selectable.
fn cell_at_pos(
    origin: eframe::egui::Pos2,
    cell: (f32, f32),
    pos: eframe::egui::Pos2,
) -> (u16, u16) {
    let col = ((pos.x - origin.x) / cell.0.max(1.0)).floor().max(0.0) as u16;
    let row = ((pos.y - origin.y) / cell.1.max(1.0)).floor().max(0.0) as u16;
    (row, col)
}

/// One find-in-terminal hit.
///
/// `offset` is how far back in the history it was found, which is also how
/// jumping to a hit works: scrolling *to* a match and finding matches are the
/// same question asked twice, so one number answers both.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct FindHit {
    offset: usize,
    row: u16,
    col: u16,
    cells: u16,
}

/// Most hits kept. A cap rather than "all of them" because the walk below is
/// bounded work per keystroke; a shell that printed ten thousand matching lines
/// is already beyond what anyone scrolls through by hand.
const FIND_CAP: usize = 200;

/// Find `query` in the visible screen and the whole scrollback.
///
/// vt100 has no search and no history-length getter, so this walks the offsets
/// it can reach: asking for the oldest line and reading back where the grid put
/// the view is how the size of the history is learned (the request is clamped),
/// and then each screenful is read in turn. The walk is one pass per frame of
/// history, which is why the caller only runs it when the query changes — not
/// every frame — and why the result count is capped.
///
/// Leaves the screen at `at`, the offset the user was already looking at.
/// Matches are non-overlapping and returned oldest first, so the last element is
/// the most recent match — the one worth landing on by default.
fn find_hits(screen: &mut vt100::Screen, query: &str, at: usize, cap: usize) -> Vec<FindHit> {
    let needle: Vec<char> = query.trim().to_lowercase().chars().collect();
    if needle.is_empty() {
        return Vec::new();
    }
    let (rows, cols) = screen.size();
    screen.set_scrollback(usize::MAX);
    let oldest = screen.scrollback();
    let mut hits: Vec<FindHit> = Vec::new();
    // One row per history offset, not a whole screenful.
    //
    // Every line of history is visible in `rows` different windows, at a
    // different row in each, so scanning a full window per offset would report
    // the same line up to 24 times — and "next" would step through one match 24
    // times before moving on. Each offset adds exactly one line that the offset
    // below it did not show, and that line is its row 0: the window at offset o
    // covers the `rows` lines ending `o` above the bottom, so increasing the
    // offset by one reveals the top row and drops the bottom one. The live
    // screen is the final window, and is scanned whole at offset 0.
    for offset in (1..=oldest).rev() {
        screen.set_scrollback(offset);
        scan_row(screen, 0, cols, &needle, offset, &mut hits);
        if hits.len() >= cap {
            break;
        }
    }
    if hits.len() < cap {
        screen.set_scrollback(0);
        for row in 0..rows {
            scan_row(screen, row, cols, &needle, 0, &mut hits);
            if hits.len() >= cap {
                break;
            }
        }
    }
    screen.set_scrollback(at);
    hits
}

/// Push every non-overlapping match of `needle` found in one row.
///
/// Cells rather than a lower-cased copy of the row: a wide glyph's trailing half
/// is then simply not a character, and the column arithmetic stays exact.
fn scan_row(
    screen: &vt100::Screen,
    row: u16,
    cols: u16,
    needle: &[char],
    offset: usize,
    hits: &mut Vec<FindHit>,
) {
    let mut col = 0;
    while col < cols {
        let mut wanted = 0;
        let mut at_col = col;
        while at_col < cols && wanted < needle.len() {
            let text = screen
                .cell(row, at_col)
                .map(|cell| cell.contents())
                .unwrap_or("");
            let matches = text
                .chars()
                .next()
                .map(|ch| ch.to_lowercase().next() == Some(needle[wanted]))
                .unwrap_or(false);
            if matches {
                wanted += 1;
                at_col += 1;
            } else {
                break;
            }
        }
        if wanted == needle.len() {
            hits.push(FindHit {
                offset,
                row,
                col,
                cells: (at_col - col).max(1),
            });
            // Non-overlapping, so a search for "aa" in "aaaa" is two hits and
            // not three.
            col = at_col;
        } else {
            col += 1;
        }
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

/// Background of the block cursor, and the ink inside it.
const CURSOR_BG: eframe::egui::Color32 =
    eframe::egui::Color32::from_rgb(0xBC, 0xDF, 0x9C);
const CURSOR_FG: eframe::egui::Color32 =
    eframe::egui::Color32::from_rgb(0x10, 0x12, 0x17);
/// Selection tint. Translucent on purpose: the shell's own colours have to
/// stay readable through a selection, or reading output means deselecting first.
///
/// Functions rather than constants because `from_rgba_unmultiplied` is not a
/// `const fn` in egui 0.36, and hand-premultiplying these into constants would
/// trade a readable colour for a mystery one.
fn selection_bg() -> eframe::egui::Color32 {
    eframe::egui::Color32::from_rgba_unmultiplied(0xBC, 0xDF, 0x9C, 0x50)
}
/// Current find hit. Much stronger than the selection tint, because this one has
/// to be findable at a glance across a screen of coloured output.
fn search_bg() -> eframe::egui::Color32 {
    eframe::egui::Color32::from_rgba_unmultiplied(0xE5, 0xC0, 0x7B, 0xC0)
}
const SEARCH_FG: eframe::egui::Color32 =
    eframe::egui::Color32::from_rgb(0x10, 0x12, 0x17);

/// How [`term_job`] should mark the grid up beyond what the shell itself said.
#[derive(Clone, Copy)]
struct Markup {
    /// Cell size to draw at: [`TERM_FONT_SIZE`] scaled by the terminal's zoom.
    font_size: f32,
    /// Selection to tint, as `(row_from, col_from, row_to, col_to)` inclusive.
    selection: Option<(u16, u16, u16, u16)>,
    /// The current find hit, as `(row, col, cells)`.
    search: Option<(u16, u16, u16)>,
    /// Whether to draw the block cursor at all. False while scrolled back: the
    /// live cursor is not in history, and painting it there would put a caret
    /// over text the shell is not editing.
    cursor: bool,
}

fn term_job(screen: &vt100::Screen, markup: Markup) -> eframe::egui::text::LayoutJob {
    use eframe::egui::Color32;
    use eframe::egui::text::{LayoutJob, TextFormat};
    let mut job = LayoutJob::default();
    let mono = eframe::egui::FontId::monospace(markup.font_size);
    // The screen's own size, not the 80x24 defaults: panes resize their
    // session, and the grid must render exactly what the shell owns.
    let (rows, cols) = screen.size();
    let (cur_row, cur_col) = screen.cursor_position();
    // Runs are flushed on a *colour* change, so the overrides (cursor, selection,
    // search) are resolved into the colour first and then compared. Deciding in
    // `vt100::Color` space and converting afterwards meant the cursor had to be
    // smuggled in as a fake RGB cell colour.
    for row in 0..rows {
        let mut run = String::new();
        let mut run_fg = Color32::TRANSPARENT;
        let mut run_bg = Color32::TRANSPARENT;
        let mut started = false;
        for col in 0..cols {
            let (fg, bg, bold, mut text) = match screen.cell(row, col) {
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
            let mut fg32 = vt_color(fg, false);
            let mut bg32 = vt_color(bg, true);
            // Bold on a plain or dim colour is how a shell asks for "bright".
            if bold && matches!(fg, vt100::Color::Default | vt100::Color::Idx(0..=7)) {
                fg32 = Color32::WHITE;
            }
            if markup.cursor && row == cur_row && col == cur_col {
                bg32 = CURSOR_BG;
                fg32 = CURSOR_FG;
            } else if let Some((sr, sc, slen)) = markup.search
                && row == sr
                && col >= sc
                && col < sc.saturating_add(slen)
            {
                bg32 = search_bg();
                fg32 = SEARCH_FG;
            } else if let Some((r0, c0, r1, c1)) = markup.selection
                && (row > r0 || (row == r0 && col >= c0))
                && (row < r1 || (row == r1 && col <= c1))
            {
                bg32 = selection_bg();
            }
            if !started {
                run_fg = fg32;
                run_bg = bg32;
                started = true;
            }
            if fg32 != run_fg || bg32 != run_bg {
                if !run.is_empty() {
                    job.append(
                        run.as_str(),
                        0.0,
                        TextFormat {
                            font_id: mono.clone(),
                            color: run_fg,
                            background: run_bg,
                            ..Default::default()
                        },
                    );
                    run.clear();
                }
                run_fg = fg32;
                run_bg = bg32;
            }
            run.push_str(&text);
        }
        if !run.is_empty() {
            job.append(
                run.as_str(),
                0.0,
                TextFormat {
                    font_id: mono.clone(),
                    color: run_fg,
                    background: run_bg,
                    ..Default::default()
                },
            );
        }
        if row + 1 < rows {
            job.append(
                "\n",
                0.0,
                TextFormat {
                    font_id: mono.clone(),
                    color: Color32::TRANSPARENT,
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
    ///
    /// The auto-numbered name is kept even after a rename, because it is what
    /// [`Terminal::next_free_title`] compares against to hand out unique
    /// numbers; `custom_title` is what gets drawn.
    title: String,
    /// A name the user gave this tab, which outranks the auto-numbered one.
    custom_title: Option<String>,
    /// Which shell this session is running, so a restart restarts the same one.
    shell: ShellKind,
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
    /// How far back the view sits, in rows; 0 is the live screen.
    ///
    /// The offset itself lives in the vt100 grid (`set_scrollback`), which is
    /// what makes `cell(row, col)` return history — so a scrolled-back terminal
    /// renders through exactly the same loop as a live one. This copy is the
    /// *intent*, because the grid clamps what it is given to the history it
    /// actually has, and vt100 0.16 exposes no length to clamp against.
    scroll: usize,
    /// Text selection over the displayed grid, if the user has one.
    selection: Option<Selection>,
    /// The shell has rung the bell or retitled itself since this pane was last
    /// focused. The one piece of state that makes several agents easier to
    /// supervise than one: which of them wants me now.
    attention: bool,
    /// Last title the shell announced, for change detection. The *first* title a
    /// PowerShell shell sends is its own name on startup, which must not count as
    /// news or every new tab would open demanding attention.
    last_title: Option<String>,
    /// The first title ever announced, kept so `pane_title` can tell "this shell
    /// introduced itself" apart from "this shell has something to say".
    first_title: Option<String>,
    /// When this shell last wrote anything. A supervisor's most useful number:
    /// a pane that has said nothing for two minutes is not one to wait on.
    last_output: Option<std::time::Instant>,
    /// Tail of an escape sequence split across two reads.
    notif_tail: Vec<u8>,
}

impl Session {
    fn new(title: String, id: u64, shell: ShellKind) -> Self {
        Self {
            id,
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            title,
            custom_title: None,
            shell,
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
            scroll: 0,
            selection: None,
            attention: false,
            last_title: None,
            first_title: None,
            last_output: None,
            notif_tail: Vec::new(),
            rows: ROWS,
            cols: COLS,
        }
    }

    /// What the tab strip and any other label should call this shell: the user's
    /// name for it if there is one, the auto-numbered one otherwise.
    fn label(&self) -> &str {
        self.custom_title.as_deref().unwrap_or(&self.title)
    }

    /// A name for the pane, if the shell has renamed itself away from whatever it
    /// called itself at startup.
    ///
    /// The startup title is not worth drawing: PowerShell and pwsh announce
    /// "Windows PowerShell" / "PowerShell 7" by themselves, which is a brand on
    /// every pane's header and no information at all. A *later* title is an agent
    /// narrating its own work — `opencode` retitles the terminal as it edits — and
    /// that is the one thing worth the header space.
    ///
    /// This is not the duplication the header is forbidden from making (see
    /// `render_pane`): that bug was writing the shell's *path line* out a second
    /// time, one row above the line the shell prints itself. A self-description
    /// is a different fact.
    fn pane_title(&self) -> Option<&str> {
        let current = self.last_title.as_deref()?;
        if current.trim().is_empty() || self.first_title.as_deref() == Some(current) {
            return None;
        }
        Some(current)
    }

    /// How long since this shell last wrote anything, if it ever has.
    fn idle_for(&self) -> Option<std::time::Duration> {
        self.last_output.map(|at| at.elapsed())
    }

    /// Put the grid at the scroll offset this session is scrolled to, and read
    /// back what it granted.
    ///
    /// `set_scrollback` clamps to the history that exists, and the history is
    /// not something vt100 0.16 exposes a length for, so asking and reading back
    /// is both the clamp and the way to notice that output has pushed the oldest
    /// line out of the buffer. Called each render, so a shell that keeps printing
    /// cannot leave the view pinned above the oldest line there is.
    fn apply_scroll(&mut self) {
        self.parser.screen_mut().set_scrollback(self.scroll);
        self.scroll = self.parser.screen().scrollback();
    }

    /// Move through history: positive is older output. Leaves the offset where
    /// it lands, clamped by the grid itself.
    fn scroll_by(&mut self, rows: i32) {
        let want = (self.scroll as i32 + rows).max(0) as usize;
        self.parser.screen_mut().set_scrollback(want);
        self.scroll = self.parser.screen().scrollback();
    }

    /// Snap to the live screen. Any keystroke means the user is done reading
    /// history — the alternative is typing into a shell whose output they are not
    /// looking at.
    fn scroll_to_bottom(&mut self) {
        if self.scroll != 0 {
            self.scroll = 0;
            self.apply_scroll();
        }
    }

    /// The selection as the renderer wants it: `(row_from, col_from, row_to,
    /// col_to)`, inclusive and in reading order.
    fn selection_range(&self) -> Option<(u16, u16, u16, u16)> {
        let sel = self.selection?;
        if sel.is_click() {
            return None;
        }
        let ((r0, c0), (r1, c1)) = sel.normalized();
        Some((r0, c0, r1, c1))
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
    /// in the tab strip, and — in Flow Mode — in the status bar beside the
    /// "Flow" word; cleared by the next success.
    notice: Option<String>,
    /// Which shell a newly opened tab starts.
    shell: ShellKind,
    /// The shells this machine can actually start, resolved once. Looked up
    /// rather than probed so nothing is spawned to draw a menu, and cached
    /// because it cannot change while the app runs.
    shells: Vec<ShellKind>,
    /// Text scale and wheel state, shared by the tabbed layout and Flow panes.
    view: ViewState,
    /// Tab being renamed: its session id and the text in the field. A modal
    /// rather than an inline edit because the strip scrolls, and an inline field
    /// in a scrolling row is a field that can scroll out of view mid-rename.
    renaming: Option<(u64, String)>,
    /// Find-in-terminal: open, what is being looked for, where it was found, and
    /// which hit is current.
    find_open: bool,
    find_query: String,
    find_hits: Vec<FindHit>,
    find_pos: usize,
    /// Focus the find field on the next frame — one-shot, so opening the bar
    /// puts the caret in it and nothing re-steals focus afterwards.
    find_focus_req: bool,
    /// Same one-shot, for the rename field.
    rename_focus: bool,
}

impl Terminal {
    pub fn new() -> Self {
        let shell = ShellKind::default();
        Self {
            sessions: vec![Session::new(shell_title(1, shell), 1, shell)],
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
            shell,
            shells: ShellKind::installed(),
            view: ViewState::default(),
            renaming: None,
            find_open: false,
            find_query: String::new(),
            find_hits: Vec::new(),
            find_pos: 0,
            find_focus_req: false,
            rename_focus: false,
        }
    }

    /// Choose the shell for shells opened from now on. Live sessions are left
    /// exactly as they are: this is not a restart, and an agent mid-task must
    /// not be interrupted by a menu click.
    fn choose_shell(&mut self, shell: ShellKind) {
        self.shell = shell;
    }

    /// One zoom step, clamped.
    ///
    /// The new scale reaches the shells on the next frame, through the ordinary
    /// sizing path — `term_cell_size` is read by both layouts before they resize
    /// anything, so a zoom is indistinguishable from a window resize as far as
    /// the ptys are concerned, which is exactly what makes it safe.
    fn zoom_by(&mut self, factor: f32) {
        self.view.zoom = (self.view.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
    }

    /// Clear a session's attention flag: its bell or its title was noticed.
    ///
    /// Called wherever focus lands on a session, and only there. Clearing it on
    /// *visibility* would defeat the feature in Flow Mode, where every pane is
    /// on screen at once — the whole question is which one wants the user, not
    /// which ones can be seen.
    pub fn mark_seen(&mut self, id: u64) {
        if let Some(session) = self.sessions.iter_mut().find(|s| s.id == id) {
            session.attention = false;
        }
    }

    /// Name a tab, or hand it back to its automatic name.
    ///
    /// The auto-numbered `title` is deliberately untouched: that is what
    /// [`Terminal::next_free_title`] compares against to hand out numbers that
    /// do not collide, so renaming one tab cannot make the next shell reuse a
    /// label that is already on screen.
    pub fn rename_session(&mut self, id: u64, name: &str) {
        if let Some(session) = self.sessions.iter_mut().find(|s| s.id == id) {
            let trimmed = name.trim();
            session.custom_title = (!trimmed.is_empty()).then(|| trimmed.to_string());
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
            let candidate = shell_title(n, self.shell);
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
        let mut session = Session::new(self.next_free_title(), id, self.shell);
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

    /// Open a shell in `cwd`, make it the active tab, and bring the section
    /// back if it had been closed off. The entry point for the explorer's
    /// "open terminal here".
    ///
    /// `new_tab` already clears `collapsed` on the grounds that a new shell you
    /// cannot see is not a new shell; `hidden` is the stronger form of the same
    /// problem and is cleared here, for the same reason.
    pub fn open_at(&mut self, cwd: &PathBuf) {
        self.hidden = false;
        self.new_tab(cwd);
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
        self.sessions
            .push(Session::new(self.next_free_title(), id, self.shell));
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
        let mut cmd = CommandBuilder::new(self.shell.program());
        cmd.args(self.shell.args());
        cmd.cwd(cwd);
        let child = match pair.slave.spawn_command(cmd) {
            Ok(c) => c,
            Err(e) => {
                self.error = Some(format!("spawn {} failed: {e}", self.shell.label()));
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
        // Typing means the user is done reading history: a shell whose output you
        // cannot see must not be the one you are typing into.
        self.scroll_to_bottom();
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
                // Ctrl+C interrupts — except with Shift held, which is the copy
                // gesture every terminal shares. That one is handled by the
                // terminal itself, so it must not reach the shell; Ctrl+Shift+V
                // falls through to `None` for the same reason.
                Key::C if !modifiers.shift => Some(&[0x03]),
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
        for chunk in &chunks {
            self.ingest(chunk);
        }
    }

    /// One read from the pty: parse it, answer any host query in it, and read it
    /// a second time for the two things a shell uses to ask for attention.
    ///
    /// One method rather than a loop body in `poll` so the notification path can
    /// be tested against a real parser with no pty behind it — which is the only
    /// way to prove the bell/title rule without waiting on a child process.
    fn ingest(&mut self, chunk: &[u8]) {
        self.total_chunks += 1;
        self.total_bytes += chunk.len() as u64;
        self.last_output = Some(std::time::Instant::now());
        self.parser.process(chunk);
        self.respond_to_queries(chunk);
        // vt100 parses neither of these, so they are taken off the same bytes.
        // Reading them here rather than at render time is what makes an
        // attention flag arrive from a tab that is in the background — the whole
        // point of the flag.
        let notes = scan_notifications(&mut self.notif_tail, chunk);
        if notes.bell {
            self.attention = true;
        }
        if let Some(title) = notes.title {
            // Only a *change* of title is news. The first title a shell sends is
            // its own name on startup, and treating that as an event would make
            // every new tab open already demanding attention. It is remembered
            // separately so `pane_title` can make the same distinction.
            if self.first_title.is_none() {
                self.first_title = Some(title.clone());
            }
            if self.last_title.as_deref().is_some_and(|t| t != title) {
                self.attention = true;
            }
            self.last_title = Some(title);
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
        //
        // `label()` rather than `title` because a renamed tab draws its own
        // name; `title` stays the auto-numbered one for uniqueness checks.
        let labels: Vec<String> = self.sessions.iter().map(|s| s.label().to_string()).collect();
        // Bell/title flags and ids, so the strip can mark attention and a
        // right-click can name a shell without holding a borrow of the list.
        let attention: Vec<bool> = self.sessions.iter().map(|s| s.attention).collect();
        let ids: Vec<u64> = self.sessions.iter().map(|s| s.id).collect();
        // Each tab's directory as well, for its hover hint. Auto context
        // switching makes the tab you click decide which project the explorer
        // shows, and "powershell 3" says nothing about which project that is —
        // so the one piece of information the click depends on was the one
        // piece the strip did not show.
        let dirs: Vec<String> = self.sessions.iter().map(|s| short_cwd(&s.cwd)).collect();
        let active_tab = self.active_tab;
        let error = self.session().and_then(|s| s.error.clone());
        let reveal_active_tab = self.reveal_active_tab;

        let mut switch_to: Option<usize> = None;
        let mut close_tab: Option<usize> = None;
        let mut open_tab = false;
        // A rename waiting to be opened, applied after the strip is built.
        let mut rename: Option<(u64, String)> = None;
        // A shell chosen from the picker, applied after the row is built so the
        // default for new tabs cannot change mid-strip.
        let mut pick_shell: Option<ShellKind> = None;

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
                        for (idx, label) in labels.iter().enumerate() {
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
                                        // A shell that rang its bell or retitled
                                        // itself since this tab was last looked
                                        // at says so before its own name. Painted
                                        // rather than typed for the usual reason:
                                        // egui's bundled fonts have no dependable
                                        // bullet coverage.
                                        if attention.get(idx).copied().unwrap_or(false) {
                                            let (dot, _) = ui.allocate_exact_size(
                                                eframe::egui::vec2(10.0, 10.0),
                                                eframe::egui::Sense::hover(),
                                            );
                                            if ui.is_rect_visible(dot) {
                                                ui.painter_at(dot)
                                                    .circle_filled(dot.center(), 3.0, accent);
                                            }
                                        }
                                        let text = eframe::egui::RichText::new(label)
                                            .size(12.5)
                                            .color(if active {
                                                crate::theme::text()
                                            } else {
                                                crate::theme::dim_text()
                                            });
                                        // A tab is a button, not selectable
                                        // text — see `widgets::clickable_label`
                                        // for the three separate things that
                                        // has to mean.
                                        // The hover hint teaches the chord in the
                                        // one place the mark actually appears.
                                        let hint = if attention.get(idx).copied().unwrap_or(false) {
                                            format!(
                                                "{}\nwaiting for you (Ctrl+Shift+A)",
                                                dirs.get(idx).cloned().unwrap_or_default()
                                            )
                                        } else {
                                            dirs.get(idx).cloned().unwrap_or_default()
                                        };
                                        let resp =
                                            crate::widgets::clickable_label(ui, text).on_hover_text(hint);
                                        // Rename and close, on the tab itself. The
                                        // close cross is 16pt and sits inside a
                                        // pill that scrolls; a right-click does
                                        // not have to be aimed at it.
                                        resp.context_menu(|ui| {
                                            if ui.button("rename").clicked() {
                                                if let Some(id) = ids.get(idx) {
                                                    rename = Some((*id, label.clone()));
                                                }
                                                ui.close();
                                            }
                                            if ui.button("close").clicked() {
                                                close_tab = Some(idx);
                                                ui.close();
                                            }
                                        });
                                        if resp.clicked() {
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
            if let Some(shell) = pick_shell {
                self.choose_shell(shell);
            }
            if let Some((id, name)) = rename {
                self.renaming = Some((id, name));
                self.rename_focus = true;
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
                        // A fresh parser has no history and no cursor at the old
                        // offset, so the scroll state has to be reset with it or
                        // the view would sit at a coordinate that no longer means
                        // anything.
                        session.scroll = 0;
                        session.selection = None;
                    }
                    // Find in the shell's output, history included. An icon
                    // rather than a chord: Ctrl+F is the editor's, and the whole
                    // Ctrl+Shift family is Flow, Dim and the pane commands.
                    if crate::icons::icon_button(ui, 20.0, "find in terminal", |p, r, c| {
                        crate::icons::magnifier(p, r.shrink(2.0), c)
                    })
                    .clicked()
                    {
                        self.find_open = true;
                        self.find_focus_req = true;
                        self.recompute_find(true);
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
                            session.scroll = 0;
                            session.selection = None;
                        }
                        self.ensure_started();
                    }
                    // Shell picker: the pill names the shell of the tab you are
                    // looking at, and the menu chooses which shell the next tab
                    // starts — two different questions, which is why the pill
                    // does not simply show the choice.
                    //
                    // This used to be a dead-looking chip reading "powershell"
                    // because there was one shell. With five, a control that
                    // cannot be clicked is the kind of static label that reads as
                    // broken the moment it disagrees with what is running.
                    let chosen = self.shell;
                    let open_shell = self.session().map(|s| s.shell).unwrap_or(chosen);
                    let shells = self.shells.clone();
                    eframe::egui::Frame::NONE
                        .fill(crate::theme::tab_active())
                        .stroke(eframe::egui::Stroke::new(1.0, crate::theme::hairline()))
                        .corner_radius(6.0)
                        .inner_margin(eframe::egui::Margin::symmetric(9, 3))
                        .show(ui, |ui| {
                            let resp = crate::widgets::clickable_label(
                                ui,
                                eframe::egui::RichText::new(open_shell.label())
                                    .size(11.5)
                                    .color(crate::theme::dim_text()),
                            )
                            .on_hover_text("choose the shell for new tabs");
                            eframe::egui::Popup::menu(&resp).show(|ui| {
                                ui.label(
                                    eframe::egui::RichText::new("new tabs start")
                                        .size(11.0)
                                        .color(crate::theme::faint()),
                                );
                                for shell in shells.iter().copied() {
                                    if ui.selectable_label(shell == chosen, shell.label()).clicked()
                                    {
                                        pick_shell = Some(shell);
                                        ui.close();
                                    }
                                }
                            });
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

        // The tab on screen is being watched, so its flag clears every frame:
        // the mark exists to point at shells you are *not* looking at. In Flow
        // Mode this is the focused pane instead — see `flow_ui` — because there
        // every pane is visible and only one has your attention.
        if let Some(id) = self.id_at(self.active_tab) {
            self.mark_seen(id);
        }

        self.rename_modal(ui);
        // Drawn before the collapsed check on purpose: a collapsed panel is a
        // header strip, and a search whose field is hidden with it is a search
        // that cannot be closed.
        self.find_bar(ui);

        if let Some(err) = &error {
            ui.colored_label(crate::theme::danger(), err);
        }

        if self.collapsed {
            return;
        }

        ui.separator();
        // Same air the Flow panes leave under their focus band, so the shell's
        // first line sits the same distance below its header in both views.
        // Inserted *before* the grid is measured, so `grid_max` and the rows
        // derived from it both shrink by this much and the PTY stays in step.
        ui.add_space(GRID_TOP_GAP);

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
        //
        // Measured from the *inset* rect, not the panel's raw width: the shell
        // is told how many columns the padded grid can actually show, so the
        // last column does not wrap onto a row of its own.
        let grid_rect = inset_grid_sides(ui.available_rect_before_wrap());
        let cell = term_cell_size(ui, self.view.zoom);
        let (rows, cols) = term_size_for_pixels(grid_rect.width().max(0.0), grid_max, cell);
        if let Some(session) = self.session_mut() {
            session.apply_size(rows, cols);
        }
        // Wheel over the grid reads history, or zooms the text with Ctrl held.
        // Neither is the shell's business, and neither existed before this:
        // the grid used to sit in a `ScrollArea` whose content was exactly one
        // screenful, so scrolling it moved nothing at all.
        match wheel_intent(ui, grid_rect, cell.1, &mut self.view) {
            WheelIntent::Scroll(by) => {
                if let Some(session) = self.session_mut() {
                    session.scroll_by(by);
                }
            }
            WheelIntent::Zoom(factor) => self.zoom_by(factor),
            WheelIntent::None => {}
        }
        let zoom = self.view.zoom;
        // The current find hit, if there is one. It is only painted when it sits
        // at the offset being drawn — a hit found further back is not on screen,
        // and tinting a cell on the live screen because of it would point at the
        // wrong text entirely.
        let search = self
            .find_open
            .then(|| self.find_hits.get(self.find_pos).copied())
            .flatten();
        let job = match self.session_mut() {
            Some(session) => {
                // Before the render, so what is drawn is what was asked for:
                // the grid clamps the offset whenever output pushes it out of
                // the buffer.
                session.apply_scroll();
                let markup = Markup {
                    font_size: TERM_FONT_SIZE * zoom,
                    selection: session.selection_range(),
                    search: search
                        .filter(|hit| hit.offset == session.scroll)
                        .map(|hit| (hit.row, hit.col, hit.cells)),
                    cursor: session.scroll == 0,
                };
                term_job(session.parser.screen(), markup)
            }
            None => return,
        };
        let mut grid_clicked = false;
        let mut drag_from: Option<(u16, u16)> = None;
        let mut drag_to: Option<(u16, u16)> = None;
        let mut drag_done = false;
        let mut copy_clicked = false;
        let mut paste_clicked = false;
        // A plain scope, not a `ScrollArea`: history is shown by asking the vt100
        // screen for it, so a second scroller inside the same rect would fight
        // that — and it would eat the wheel events this now reads. The label is
        // the only thing in here, and it is sized to fit exactly.
        ui.scope_builder(
            eframe::egui::UiBuilder::new()
                .max_rect(grid_rect)
                .layout(eframe::egui::Layout::top_down(eframe::egui::Align::LEFT)),
            |ui| {
                ui.push_id("snor_term_grid_label", |ui| {
                    let resp = ui.add(
                        eframe::egui::Label::new(job)
                            .extend()
                            // Selection is ours, not egui's: a `Label`'s own
                            // selection copies the grid's full-width padding and
                            // would fight the tint painted below.
                            .selectable(false)
                            .sense(eframe::egui::Sense::click_and_drag()),
                    );
                    grid_clicked = resp.clicked();
                    let at = resp
                        .interact_pointer_pos()
                        .map(|pos| cell_at_pos(grid_rect.min, cell, pos));
                    if resp.drag_started() {
                        drag_from = at;
                    } else if resp.dragged() {
                        drag_to = at;
                    }
                    if resp.drag_stopped() {
                        drag_done = true;
                    }
                    resp.context_menu(|ui| {
                        if ui.button("copy").clicked() {
                            copy_clicked = true;
                            ui.close();
                        }
                        if ui.button("paste").clicked() {
                            paste_clicked = true;
                            ui.close();
                        }
                    });
                });
            },
        );
        if grid_clicked {
            self.active = true;
        }
        let dragging = drag_from.is_some() || drag_to.is_some() || drag_done;
        if dragging
            && let Some(session) = self.session_mut()
        {
            if let Some(start) = drag_from {
                session.selection = Some(Selection::new(start));
            } else if let Some(head) = drag_to
                && let Some(sel) = session.selection.as_mut()
            {
                sel.head = head;
            }
            // A press that never moved is a click, not a selection: leaving a
            // one-cell selection behind would make the next copy take a single
            // character the user never chose.
            if drag_done && session.selection.is_some_and(|s| s.is_click()) {
                session.selection = None;
            }
        }
        // Ctrl+Shift+C copies rather than interrupts. The other terminal
        // convention is the one everyone already has in their fingers, and it is
        // the only way to keep Ctrl+C as SIGINT without a collision;
        // `key_to_bytes` refuses the shifted form so it never reaches the shell.
        let copy_key = ui.input_mut(|i| {
            i.consume_shortcut(&eframe::egui::KeyboardShortcut::new(
                eframe::egui::Modifiers::CTRL | eframe::egui::Modifiers::SHIFT,
                eframe::egui::Key::C,
            ))
        });
        if copy_clicked || copy_key {
            let text = self
                .session()
                .and_then(|s| s.selection.map(|sel| selection_text(s.parser.screen(), sel)));
            if let Some(text) = text
                && !text.is_empty()
            {
                ui.ctx().copy_text(text);
            }
        }
        let paste_key = ui.input_mut(|i| {
            i.consume_shortcut(&eframe::egui::KeyboardShortcut::new(
                eframe::egui::Modifiers::CTRL | eframe::egui::Modifiers::SHIFT,
                eframe::egui::Key::V,
            ))
        });
        if paste_clicked || paste_key {
            // Asking the backend for the clipboard, which arrives as
            // `Event::Paste` and is forwarded to the pty by `forward_events`
            // like any other paste — egui has no direct clipboard read, and this
            // is the path the shell already understands.
            self.active = true;
            ui.ctx()
                .send_viewport_cmd(eframe::egui::ViewportCommand::RequestPaste);
        }

        // Type-directly-in-terminal is shared with Flow Mode panes: the
        // event loop lives in `forward_events`, with `None` targeting the
        // active tab here and a session id targeting the focused pane there.
        self.forward_events(ui, None);
    }
}

impl Terminal {
    /// Run the search again, and land on a hit.
    ///
    /// `newest` decides which end to land on. When the query changes the newest
    /// match is the interesting one — the traceback that just scrolled past —
    /// and a step from there behaves like a step from anywhere else.
    fn recompute_find(&mut self, newest: bool) {
        let query = self.find_query.clone();
        let mut hits = Vec::new();
        if let Some(session) = self.session_mut() {
            let at = session.scroll;
            hits = find_hits(session.parser.screen_mut(), &query, at, FIND_CAP);
        }
        self.find_hits = hits;
        self.find_pos = if newest && !self.find_hits.is_empty() {
            self.find_hits.len() - 1
        } else {
            0
        };
        self.jump_to_hit();
    }

    /// Move to another hit, wrapping at both ends.
    fn find_step(&mut self, dir: i32) {
        if self.find_hits.is_empty() {
            return;
        }
        let n = self.find_hits.len() as i32;
        self.find_pos = (self.find_pos as i32 + dir).rem_euclid(n) as usize;
        self.jump_to_hit();
    }

    /// Scroll the active shell to the current hit.
    ///
    /// The tint is the renderer's job: it reads `find_hits[find_pos]` and paints
    /// it only when that offset is the screenful being drawn, which is what stops
    /// a hit in history from tinting an unrelated cell of the live screen.
    fn jump_to_hit(&mut self) {
        let Some(hit) = self.find_hits.get(self.find_pos).copied() else {
            return;
        };
        if let Some(session) = self.session_mut() {
            session.parser.screen_mut().set_scrollback(hit.offset);
            session.scroll = session.parser.screen().scrollback();
            // A selection and a search are two readings of the same grid;
            // leaving the old one tinted under the new hit is just noise.
            session.selection = None;
        }
    }

    /// The find bar: search the active shell's output, history included.
    fn find_bar(&mut self, ui: &mut eframe::egui::Ui) {
        if !self.find_open {
            return;
        }
        let field = ui.make_persistent_id("snor_term_find");
        if std::mem::take(&mut self.find_focus_req) {
            ui.memory_mut(|m| m.request_focus(field));
        }
        let mut close = false;
        let mut step = 0;
        let mut requery = false;
        ui.horizontal(|ui| {
            let resp = ui.add(
                eframe::egui::TextEdit::singleline(&mut self.find_query)
                    .id(field)
                    .hint_text("find in terminal (history included)")
                    .desired_width(240.0),
            );
            if resp.changed() {
                requery = true;
            }
            let total = self.find_hits.len();
            if total > 0 {
                ui.label(
                    eframe::egui::RichText::new(format!("{}/{}", self.find_pos + 1, total))
                        .small()
                        .color(crate::theme::dim_text()),
                );
                if ui.small_button("prev").clicked() {
                    step = -1;
                }
                if ui.small_button("next").clicked() {
                    step = 1;
                }
            } else if !self.find_query.trim().is_empty() {
                ui.label(
                    eframe::egui::RichText::new("no match")
                        .small()
                        .color(crate::theme::faint()),
                );
            }
            if ui.small_button("x").clicked() {
                close = true;
            }
            if ui.memory(|m| m.has_focus(field)) {
                if ui.input(|i| i.key_pressed(eframe::egui::Key::Escape)) {
                    close = true;
                }
                if ui.input(|i| i.key_pressed(eframe::egui::Key::Enter)) {
                    step = if ui.input(|i| i.modifiers.shift) { -1 } else { 1 };
                }
            }
        });
        if requery {
            self.recompute_find(true);
        } else if close {
            self.find_open = false;
        } else if step != 0 {
            self.find_step(step);
        }
    }

    /// The rename box, if a tab is being renamed.
    fn rename_modal(&mut self, ui: &eframe::egui::Ui) {
        let Some((id, mut name)) = self.renaming.clone() else {
            return;
        };
        let field = ui.make_persistent_id("snor_term_rename");
        // Taken before the closure, so the caret lands in the field on the frame
        // the box opens and is never re-stolen afterwards.
        let focus = std::mem::take(&mut self.rename_focus);
        let mut apply = false;
        let mut cancel = false;
        eframe::egui::Window::new("rename terminal")
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                if focus {
                    ui.memory_mut(|m| m.request_focus(field));
                }
                let resp = ui.add(
                    eframe::egui::TextEdit::singleline(&mut name)
                        .id(field)
                        .desired_width(220.0)
                        .hint_text("tab name"),
                );
                if resp.lost_focus() && ui.input(|i| i.key_pressed(eframe::egui::Key::Enter)) {
                    apply = true;
                }
                if ui.input(|i| i.key_pressed(eframe::egui::Key::Escape)) {
                    cancel = true;
                }
                ui.horizontal(|ui| {
                    if ui.button("rename").clicked() {
                        apply = true;
                    }
                    if ui.button("cancel").clicked() {
                        cancel = true;
                    }
                });
                ui.label(
                    eframe::egui::RichText::new("an empty name restores the automatic one")
                        .small()
                        .color(crate::theme::faint()),
                );
            });
        if cancel {
            self.renaming = None;
        } else if apply {
            self.rename_session(id, &name);
            self.renaming = None;
        } else {
            // Kept, so a click elsewhere in the window does not throw away what
            // has been typed.
            self.renaming = Some((id, name));
        }
    }

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

/// Height of a Flow pane's header row — the focus band and its marker. No
/// text: the shell prints its own prompt, so naming the pane here only ever
/// duplicated it. Reserved out of the pane's rectangle before its rows and
/// columns are computed, so the PTY is never told it has a line the grid does
/// not actually show.
const PANE_HEADER_H: f32 = 18.0;
/// Air between a terminal's header and the shell's first row.
///
/// The prompt used to sit flush against whatever sat above it — the pane's
/// focus band in Flow mode, the tab strip's separator in normal mode — which
/// read as text crammed under a rule. Deliberately small: this is breathing
/// room, not a margin. Both views read this one constant so the shell's first
/// line lands the same distance down in either, rather than each growing its
/// own private nudge.
const GRID_TOP_GAP: f32 = 5.0;
/// Air between a terminal's own side edges and its first and last column.
///
/// The prompt used to start flush against the pane's vertical boundary — the
/// rule above the tab strip in normal mode, the pane's own outline in Flow mode
/// — so the first glyph of `PS C:\...>` read as touching the edge. Same idea as
/// `GRID_TOP_GAP`, measured on the other axis, and deliberately a different
/// number: a line of text wants less air above it than a column of text wants
/// beside it.
///
/// Applied to **both** sides. Padding only the left would leave a line that
/// fills the grid running into the right edge, which reads as a bug rather than
/// a margin.
///
/// Like the header height, this is spent *before* the shell is sized: the
/// columns come from the padded width, so the PTY is never promised a column
/// the padded grid cannot show.
const GRID_SIDE_GAP: f32 = 8.0;
/// Everything a pane spends above its first grid row: the painted header band
/// plus the air under it.
///
/// `size_panes` and the renderer both read this one value rather than adding
/// the two constants up separately, because the two *must* agree. They are the
/// same measurement taken from opposite ends — the PTY's row count and the
/// pixels the grid is actually given — and the bug this guards against is
/// exactly that they drift: the header was originally held open as a side
/// effect of laying out a label row, so deleting the labels shrank the chrome
/// in the renderer while `size_panes` kept subtracting the old height. The
/// shell was then promised fewer rows than the pane could show, which is the
/// same class of mismatch as the frozen half-resized TUI frame.
const PANE_TOP_CHROME: f32 = PANE_HEADER_H + GRID_TOP_GAP;
/// Slack the renderer leaves under a pane's grid inside its scroll area.
/// Subtracted alongside the header for the same reason.
const PANE_GRID_PAD: f32 = 4.0;

/// Pixels per terminal cell, measured off the live font. Panes divide their
/// pixel rects by these to learn their real rows and columns.
fn term_cell_size(ui: &eframe::egui::Ui, zoom: f32) -> (f32, f32) {
    use eframe::egui::{Color32, FontId};
    let mono = FontId::monospace(TERM_FONT_SIZE * zoom);
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

/// A pane's rectangle with [`GRID_SIDE_GAP`] taken off the left and right.
///
/// `size_panes` sizes the shell from this rect and `render_pane` draws the grid
/// into it, so both read the same function instead of each subtracting the gap
/// by hand. That is the lesson of the header-height bug documented on
/// `PANE_TOP_CHROME`: two sites computing one measurement separately will drift,
/// and the drift is invisible until a shell is told it has a column the grid
/// does not draw.
fn inset_grid_sides(rect: eframe::egui::Rect) -> eframe::egui::Rect {
    // Clamped to half the width, so a pane narrower than its own padding still
    // yields a sane (non-inverted) rect rather than a negative width that would
    // clamp every column count up to `MIN_TERM_COLS`.
    let gap = GRID_SIDE_GAP.min(rect.width() / 2.0);
    eframe::egui::Rect::from_min_max(
        eframe::egui::pos2(rect.left() + gap, rect.top()),
        eframe::egui::pos2(rect.right() - gap, rect.bottom()),
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

    /// The session the user is working in, with its directory: the focused pane
    /// in Flow Mode, the active tab otherwise. `flow` is passed in rather than
    /// read from a field because the terminal has no mode flag — the shell owns
    /// that decision, and `flow_target_id` answers in either mode.
    ///
    /// This is what auto context switching follows. `session.cwd` is the
    /// directory a shell was *spawned* in, not one it has `cd`-ed into since:
    /// nothing here parses the shell's own output, so a pane that changes
    /// directory by hand keeps the directory it started with. That is the
    /// honest limit of the feature — see `SnorApp::sync_context_root`.
    pub fn context_session(&self, flow: bool) -> Option<(u64, PathBuf)> {
        // A hidden terminal has no focused tab to speak of, and the section is
        // not on screen to prove otherwise — leaving the explorer where it is
        // beats re-rooting it to something the user cannot see.
        if self.hidden || self.sessions.is_empty() {
            return None;
        }
        if flow {
            let id = self.flow_target_id()?;
            self.sessions
                .iter()
                .find(|s| s.id == id)
                .map(|s| (s.id, s.cwd.clone()))
        } else {
            self.session().map(|s| (s.id, s.cwd.clone()))
        }
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
        let mut session = Session::new(self.next_free_title(), id, self.shell);
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

    /// Focus the next pane that wants attention, in grid order, wrapping.
    ///
    /// The keyboard counterpart to the attention mark. With four panes up, "who
    /// needs me" is the question a supervisor actually has, and walking all four
    /// to answer it is what this removes. Nothing happens and nothing is
    /// reported when no pane is flagged: silence is the honest answer in the
    /// common case where the pane you are already looking at is the only one with
    /// news, and a notice on every press would be worse than no notice.
    pub fn flow_step_attention(&mut self) {
        let order: Vec<u64> = self
            .flow_grid
            .order
            .iter()
            .copied()
            .filter(|id| self.index_of(*id).is_some())
            .collect();
        if order.is_empty() {
            return;
        }
        // Search from the pane after the focused one, so repeated presses walk
        // the flags in order instead of bouncing between two of them.
        let start = self
            .flow_target_id()
            .and_then(|id| order.iter().position(|o| *o == id))
            .map(|i| i + 1)
            .unwrap_or(0);
        for step in 0..order.len() {
            let id = order[(start + step) % order.len()];
            if self.sessions.iter().any(|s| s.id == id && s.attention) {
                self.flow_focus = Some(id);
                self.active = true;
                return;
            }
        }
    }

    /// Focus the next tab that wants attention, wrapping around the strip. The
    /// normal-mode half of the same question.
    pub fn step_attention_tab(&mut self) {
        let n = self.sessions.len();
        if n == 0 {
            return;
        }
        for step in 1..=n {
            let i = (self.active_tab + step) % n;
            if self.sessions[i].attention {
                self.active_tab = i;
                self.active = true;
                self.reveal_active_tab = true;
                return;
            }
        }
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
        if self.sessions.is_empty() {
            let _ = self.new_tab(default_cwd);
        }
        let live: Vec<u64> = self.sessions.iter().map(|s| s.id).collect();
        self.flow_grid.set_panes(&live);
        let rect = ui.available_rect_before_wrap();

        // Size every pane *before* draining the ptys, not during the render.
        //
        // The renderer used to call `apply_size` as it drew each pane, which
        // resized the ConPTY after that frame's output had already been read.
        // The TUI's response to the new size — a full repaint — therefore did
        // not reach the grid until the following poll, and if the shell then
        // went quiet the torn half-resized frame stayed on screen until some
        // unrelated event forced a repaint. OpenCode's logo was the visible
        // casualty: the top half redrawn at the new width, the bottom half
        // still the old one, frozen until a keypress.
        //
        // Measuring first closes that gap: any output the resize provokes is
        // drained by the `poll()` below, in the same frame it was asked for.
        let cell = term_cell_size(ui, self.view.zoom);
        self.size_panes(rect, cell);

        self.poll();

        // The recessed slab behind everything, like the normal terminal, so
        // the panes read as one continuous workspace.
        ui.painter()
            .rect_filled(rect, 0.0, crate::theme::surface_recessed());
        // Split borrows: the grid and the sessions travel side by side into
        // the renderer instead of fighting over `&mut self`.
        let Self {
            sessions,
            flow_grid,
            flow_focus,
            active,
            view,
            ..
        } = self;
        // The view borrows grid and sessions side by side; the scope ends
        // those borrows before input handling needs `self`.
        {
            let mut flow = FlowView {
                grid: flow_grid,
                sessions,
                focus: flow_focus,
                latched: active,
                view,
            };
            flow.render(ui, rect);
        }
        // A refused creation is reported by the caller instead of here.
        //
        // Painting the text into `rect`'s top-left corner put it directly on
        // top of the first pane's header — pane title, cwd and the notice
        // interleaved into unreadable mush at exactly the moment the user
        // needed to read it. `flow_ui` owns only the pane grid, so the note
        // travels out through `notice_text()` and the shell draws it in the
        // status bar, where "Flow" already lives and nothing can collide.
        let target = self.flow_target_id();
        // Focused means watched: only the pane with focus clears its flag here.
        // Every pane is visible in this mode, so clearing on visibility would
        // clear all four and the mark would mean nothing when it mattered.
        if let Some(id) = target {
            self.mark_seen(id);
        }
        self.forward_events(ui, target);
    }

    /// Give every visible pane's shell the rows and columns its rectangle
    /// implies, before anything is drawn.
    ///
    /// The header row a pane draws for itself is subtracted here, and so is
    /// the two points the renderer leaves under the grid, so the PTY's size
    /// matches the cells the pane will actually show. Sizing to the raw rect
    /// instead told the shell it had one row more than the grid displayed,
    /// which pushed a TUI's last line out of the visible area.
    fn size_panes(&mut self, area: eframe::egui::Rect, cell: (f32, f32)) {
        let mut sizes: Vec<(u64, u16, u16)> = Vec::new();
        for (r, &count) in flow_shape(self.flow_grid.len()).iter().enumerate() {
            for c in 0..count {
                let Some(id) = self.flow_grid.cell_at(r, c) else {
                    continue;
                };
                let rect = self.flow_grid.cell_rect(area, r, c);
                // The header band and the air under it, as one measurement.
                let grid_h = (rect.height() - PANE_TOP_CHROME - PANE_GRID_PAD).max(0.0);
                // Side air comes off the width through the same helper
                // `render_pane` draws with, so the columns handed to the shell
                // are the columns the padded grid shows.
                let grid_w = inset_grid_sides(rect).width();
                let (rows, cols) = term_size_for_pixels(grid_w, grid_h, cell);
                sizes.push((id, rows, cols));
            }
        }
        // Applied in one pass, so a pane that is not on screen — a background
        // session — keeps whatever size it last had and is never resized by
        // accident.
        for (id, rows, cols) in sizes {
            if let Some(s) = self.sessions.iter_mut().find(|s| s.id == id) {
                s.apply_size(rows, cols);
            }
        }
    }

    /// The pending refusal note, if any. The status bar shows it while Flow
    /// Mode is up; normal mode still shows it inline in the tab strip.
    pub fn notice_text(&self) -> Option<&str> {
        self.notice.as_deref()
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
    /// Terminal-wide view state: panes draw the same grid, so they share the
    /// zoom with the tabbed layout rather than inventing a second one.
    view: &'a mut ViewState,
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
            // Idle seams carry the pane_edge colour at 1.5pt rather than a
            // 1pt hairline: at hairline weight a four-pane grid read as one
            // continuous sheet with faint marks on it, which is the opposite
            // of what a tiling workspace is for.
            (crate::theme::pane_edge(), 1.5)
        };
        let stroke = eframe::egui::Stroke::new(width, tint);
        if vertical {
            ui.painter().vline(band.center().x, band.y_range(), stroke);
        } else {
            ui.painter().hline(band.x_range(), band.center().y, stroke);
        }
    }

    /// One pane: a small header (focus marker, prompt) over the shell's own
    /// grid. The grid is the click target — clicking focuses the pane and
    /// latches typing, exactly like the normal terminal.
    fn render_pane(&mut self, ui: &mut eframe::egui::Ui, id: u64, rect: eframe::egui::Rect) {
        use eframe::egui::{Align, Layout};
        let focused = *self.focus == Some(id);
        // Snapshot the header first; the borrow ends before any widget.
        let (error, attention, title, idle) = match self.sessions.iter().find(|s| s.id == id) {
            Some(s) => (
                s.error.clone(),
                s.attention,
                s.pane_title().map(str::to_owned),
                s.idle_for(),
            ),
            None => return,
        };
        ui.scope_builder(
            eframe::egui::UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::top_down(Align::LEFT)),
            |ui| {
                // The header band carries focus and nothing else.
                //
                // It used to name the pane — "powershell C:\My work folder\
                // Snor", later a prompt-shaped "PS C:\...\Snor>". Both were
                // the shell's own first line written out a second time: the
                // shell prints `PS C:\My work folder\Snor>` itself, one row
                // below, so every new pane opened with two identical path
                // entries stacked. The shell's line cannot be taken away, so
                // the copy is the one that goes. Dimming it only made the
                // duplication quieter, which is not the same as fixing it.
                //
                // What is left is what the header was actually for: a strip
                // that says which pane is live. The pane's directory is still
                // legible from the shell's own prompt, and the explorer
                // follows the focused terminal (`SnorApp::sync_context_root`),
                // so the workspace names the project too.
                if focused {
                    let header = eframe::egui::Rect::from_min_size(
                        rect.min,
                        eframe::egui::vec2(rect.width(), PANE_HEADER_H),
                    );
                    ui.painter()
                        .rect_filled(header, 0.0, crate::theme::surface_title());
                    let bar = eframe::egui::Rect::from_min_size(
                        eframe::egui::pos2(rect.left() + 3.0, rect.top() + 3.0),
                        eframe::egui::vec2(2.0, PANE_HEADER_H - 6.0),
                    );
                    ui.painter().rect_filled(bar, 1.0, crate::theme::accent());
                }
                // This shell wants you: it rang, or it retitled itself, since
                // the pane was last focused. Drawn whether or not the pane has
                // focus, because an unfocused pane is exactly the one that needs
                // pointing at — and that is the whole reason Flow Mode can hold
                // four agents without being cycled through to find out which one
                // is waiting.
                if attention {
                    let dot = eframe::egui::pos2(
                        rect.right() - 12.0,
                        rect.top() + PANE_HEADER_H * 0.5,
                    );
                    let p = ui.painter_at(eframe::egui::Rect::from_center_size(
                        dot,
                        eframe::egui::vec2(9.0, 9.0),
                    ));
                    p.circle_filled(dot, 3.5, crate::theme::accent());
                }
                // The header band, painted: what the shell says it is doing (if
                // it has renamed itself away from its own startup name) on the
                // left, and how long it has been quiet on the right. Both go into
                // the band `size_panes` already reserves, so neither can move the
                // grid below it — the same invariant the focus cue observes.
                let font = eframe::egui::FontId::proportional(11.0);
                if let Some(title) = title.as_deref() {
                    let max_w = (rect.width() - 76.0).max(24.0);
                    let text = crate::icons::truncate(ui.painter(), title, font.clone(), max_w);
                    let galley = ui
                        .painter()
                        .layout_no_wrap(text, font.clone(), crate::theme::dim_text());
                    let y = rect.top() + (PANE_HEADER_H - galley.size().y) * 0.5;
                    ui.painter().galley(
                        eframe::egui::pos2(rect.left() + 10.0, y),
                        galley,
                        crate::theme::dim_text(),
                    );
                }
                if let Some(idle) = idle {
                    // Accent while it is actually writing, faint once it stops.
                    // One element rather than a dot plus a number: two marks a
                    // few points apart in a 18pt band read as one confusing one.
                    let color = if idle.as_secs() < PANE_ACTIVE_SECS {
                        crate::theme::accent()
                    } else {
                        crate::theme::faint()
                    };
                    let galley = ui.painter().layout_no_wrap(short_idle(idle), font, color);
                    let y = rect.top() + (PANE_HEADER_H - galley.size().y) * 0.5;
                    // The attention dot's slot is reserved whether or not the dot
                    // is drawn, so the number does not jump sideways when a shell
                    // rings.
                    let x = (rect.right() - 24.0 - galley.size().x).max(rect.left() + 4.0);
                    ui.painter().galley(eframe::egui::pos2(x, y), galley, color);
                }
                if let Some(err) = error {
                    ui.colored_label(crate::theme::danger(), err);
                }
                // The header is *painted*, not laid out, so its height has to
                // be reserved by hand. The label row that used to sit here was
                // the only thing holding the space open — removing the text
                // silently let the grid climb into the band, which both put the
                // prompt back against the top edge and left `size_panes`
                // promising the shell fewer rows than the pane could show.
                // `PANE_TOP_CHROME` is the same measurement `size_panes` uses.
                ui.add_space(PANE_TOP_CHROME);
                // The shell was already sized to this pane by `size_panes`,
                // before the ptys were drained — see `flow_ui`. Sizing here
                // instead would resize the ConPTY after its output for this
                // frame had been read, leaving a torn frame on screen.
                // The grid is drawn into the side-inset rect, so the pane's own
                // outline does not have the prompt's first glyph sitting on it.
                let grid_rect = inset_grid_sides(ui.available_rect_before_wrap());
                let cell = term_cell_size(ui, self.view.zoom);
                // Wheel in a pane means the same as in the tabbed layout:
                // history, or zoom with Ctrl. Both are handled before the label
                // so the gesture never reaches the shell.
                match wheel_intent(ui, grid_rect, cell.1, self.view) {
                    WheelIntent::Scroll(by) => {
                        if let Some(session) = self.sessions.iter_mut().find(|s| s.id == id) {
                            session.scroll_by(by);
                        }
                    }
                    WheelIntent::Zoom(factor) => {
                        self.view.zoom = (self.view.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
                    }
                    WheelIntent::None => {}
                }
                let job = match self.sessions.iter_mut().find(|s| s.id == id) {
                    Some(s) => {
                        s.apply_scroll();
                        let markup = Markup {
                            font_size: TERM_FONT_SIZE * self.view.zoom,
                            // Panes have no drag selection of their own — the
                            // tabbed grid is where selecting and copying lives —
                            // but the field is carried through so both layouts
                            // render through the same function.
                            selection: s.selection_range(),
                            search: None,
                            cursor: s.scroll == 0,
                        };
                        term_job(s.parser.screen(), markup)
                    }
                    None => return,
                };
                let mut clicked = false;
                // A plain scope, matching the tabbed layout: history is the
                // vt100 screen's, so an inner scroller would only fight it.
                ui.scope_builder(
                    eframe::egui::UiBuilder::new()
                        .max_rect(grid_rect)
                        .layout(eframe::egui::Layout::top_down(eframe::egui::Align::LEFT)),
                    |ui| {
                        ui.push_id(("snor_flow_grid_label", id), |ui| {
                            let resp = ui.add(
                                eframe::egui::Label::new(job)
                                    .extend()
                                    .selectable(false)
                                    .sense(eframe::egui::Sense::click()),
                            );
                            clicked = resp.clicked();
                        });
                    },
                );
                if clicked {
                    *self.focus = Some(id);
                    *self.latched = true;
                }
            },
        );
        // The pane's outline. Unfocused it is the pane seam — deliberately
        // brighter than the hairline used elsewhere, because two recessed
        // slabs meeting edge to edge have nothing else to tell them apart.
        // Focused it warms to a dimmed accent, which is the second half of
        // the focus cue: the bar says *which* pane, the outline says the
        // pane is live.
        let border = if focused {
            crate::theme::accent().gamma_multiply(0.55)
        } else {
            crate::theme::pane_edge()
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
    use super::{
        FIND_CAP, Notifications, Selection, Session, ShellKind, Terminal, cell_at_pos, find_hits,
        PANE_ACTIVE_SECS, flow_shape, installed_from, scan_notifications, selection_text,
        shell_title, short_idle, term_size_for_pixels,
    };
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

    // --- Scrollback, selection, search, attention, shells ----------------

    /// Hermetic: `ingest` is the same call `poll` makes, so these exercise the
    /// real parser and the real notification scan with no pty behind them.
    fn labels(t: &Terminal) -> Vec<&str> {
        t.sessions.iter().map(|s| s.label()).collect()
    }

    /// History is the grid's, not a second buffer: the offset moves the vt100
    /// screen, `cell(row, col)` returns what scrolled off, and the clamp is the
    /// grid's own because vt100 0.16 exposes no history length to clamp against.
    #[test]
    fn wheel_history_holds_what_has_scrolled_off_the_screen() {
        let mut t = Terminal::new();
        for i in 0..40 {
            sm(&mut t).ingest(format!("line{i}\r\n").as_bytes());
        }
        assert_eq!(s(&t).scroll, 0, "a live shell starts at the bottom");

        sm(&mut t).scroll_by(5);
        assert_eq!(s(&t).scroll, 5);
        sm(&mut t).apply_scroll();
        // Five rows back: nothing was drawn there before, and now it holds the
        // line that scrolled off the top.
        assert!(
            s(&t).parser.screen().cell(0, 0).is_some(),
            "the screen must still answer for rows in history"
        );

        // Asking for more than exists lands on the oldest line, not above it,
        // and asking twice does not drift further.
        sm(&mut t).scroll_by(10_000);
        let oldest = s(&t).scroll;
        assert!(oldest > 5, "40 lines through a 24-row screen is history");
        sm(&mut t).scroll_by(10_000);
        assert_eq!(s(&t).scroll, oldest, "the clamp must hold");

        // Typing means the user is done reading history.
        sm(&mut t).scroll_to_bottom();
        assert_eq!(s(&t).scroll, 0);
    }

    /// A selection copies text, not a rectangle of trailing spaces. The grid is
    /// padded to its full width for rendering, and that padding is exactly what
    /// makes a copied line paste as garbage.
    #[test]
    fn a_selection_copies_without_the_grid_padding() {
        let mut t = Terminal::new();
        sm(&mut t).ingest(b"alpha beta\r\nsecond line\r\n");

        let one = Selection {
            anchor: (0, 0),
            head: (0, 4),
        };
        assert_eq!(selection_text(s(&t).parser.screen(), one), "alpha");

        let two = Selection {
            anchor: (0, 6),
            head: (1, 5),
        };
        assert_eq!(selection_text(s(&t).parser.screen(), two), "beta\nsecond");

        // Blank rows at the end are dropped rather than copied as empty lines.
        let trailing = Selection {
            anchor: (0, 6),
            head: (3, 0),
        };
        assert_eq!(
            selection_text(s(&t).parser.screen(), trailing),
            "beta\nsecond line"
        );

        // A press that never moves is a click, and leaves nothing to copy.
        let click = Selection::new((1, 1));
        assert!(click.is_click());
        assert_eq!(click.normalized(), ((1, 1), (1, 1)));
    }

    /// Search reaches into the scrollback, reports cells rather than bytes, and
    /// leaves the view where the user had it.
    #[test]
    fn find_reaches_into_the_scrollback_and_restores_the_view() {
        let mut t = Terminal::new();
        sm(&mut t).ingest(b"hello world\r\n");
        let live = find_hits(sm(&mut t).parser.screen_mut(), "world", 0, FIND_CAP);
        assert_eq!(
            live,
            vec![super::FindHit {
                offset: 0,
                row: 0,
                col: 6,
                cells: 5
            }],
            "a match on the live screen reports where the renderer must tint"
        );

        for i in 0..40 {
            let line = if i == 3 {
                format!("needle at {i}\r\n")
            } else {
                format!("line{i}\r\n")
            };
            sm(&mut t).ingest(line.as_bytes());
        }
        // The user is reading history, not the live screen.
        sm(&mut t).scroll_by(7);
        let viewed = s(&t).scroll;

        let hits = find_hits(
            sm(&mut t).parser.screen_mut(),
            "needle",
            viewed,
            FIND_CAP,
        );
        assert_eq!(hits.len(), 1, "one match in the whole history");
        assert!(
            hits[0].offset > 0,
            "the match has scrolled off the live screen"
        );
        assert_eq!(
            s(&t).parser.screen().scrollback(),
            viewed,
            "searching must not move the view it was asked about"
        );
        assert_eq!(
            find_hits(
                sm(&mut t).parser.screen_mut(),
                "NEEDLE",
                viewed,
                FIND_CAP
            )
            .len(),
            1,
            "case-insensitive"
        );

        // Jumping to a hit scrolls to it, so the tint and the view agree.
        t.find_hits = hits;
        t.find_pos = 0;
        t.jump_to_hit();
        assert_eq!(s(&t).scroll, t.find_hits[0].offset);
    }

    /// The rule that makes the flag worth having: a shell announcing its own name
    /// at startup is not news, a shell retitling itself later is, and a bell
    /// always is.
    #[test]
    fn a_bell_and_a_title_change_raise_attention_but_startup_titles_do_not() {
        let mut t = Terminal::new();
        sm(&mut t).ingest(b"\x1b]0;Windows PowerShell\x07");
        assert!(
            !s(&t).attention,
            "the shell's own startup title must not demand attention"
        );

        sm(&mut t).ingest(b"\x1b]2;agent: reading src/main.rs\x07");
        assert!(s(&t).attention, "an agent retitling itself is the point");

        let id = s(&t).id;
        t.mark_seen(id);
        assert!(!s(&t).attention, "looking at it clears it");

        sm(&mut t).ingest(b"\x07");
        assert!(s(&t).attention, "a bell is unambiguous");
    }

    /// A title arrives in whatever pieces the pty reader flushed, so a sequence
    /// split across two reads still has to be read exactly once.
    #[test]
    fn an_escape_split_across_reads_is_read_once() {
        let mut tail = Vec::new();
        assert_eq!(
            scan_notifications(&mut tail, b"\x1b]2;agen"),
            Notifications::default()
        );
        assert!(!tail.is_empty(), "the half sequence must be kept");

        let notes = scan_notifications(&mut tail, b"t done\x07");
        assert_eq!(notes.title.as_deref(), Some("agent done"));
        assert!(
            !notes.bell,
            "the BEL that ends a title assignment is a terminator, not a bell"
        );
        assert!(tail.is_empty(), "nothing left over");

        // The other terminator, and an ordinary bell with no title.
        let mut tail = Vec::new();
        let notes = scan_notifications(&mut tail, b"\x1b]2;x\x1b\\\x07");
        assert_eq!(notes.title.as_deref(), Some("x"));
        assert!(notes.bell);

        // A title assignment for a feature this app does not implement ("11" is
        // a window colour) is not a name.
        let mut tail = Vec::new();
        assert_eq!(
            scan_notifications(&mut tail, b"\x1b]11;#282c34\x07"),
            Notifications::default()
        );
        assert!(tail.is_empty());
    }

    /// Renaming changes what is drawn, never the auto-numbering: `title` is what
    /// `next_free_title` compares against, so a renamed tab cannot make the next
    /// shell reuse a name already on screen.
    #[test]
    fn renaming_a_tab_leaves_the_numbering_alone() {
        let mut t = Terminal::new();
        t.open_stub_tab();
        let id = s(&t).id;

        t.rename_session(id, "  agent-auth  ");
        assert_eq!(labels(&t), ["powershell", "agent-auth"]);

        t.open_stub_tab();
        assert_eq!(
            titles(&t),
            ["powershell", "powershell 2", "powershell 3"],
            "numbering must not care about names the user gave"
        );

        // An empty name hands the tab back to its automatic label.
        t.rename_session(id, "   ");
        assert_eq!(labels(&t)[1], "powershell 2");
        // Renaming something that no longer exists is a no-op, not a panic.
        t.rename_session(9_999, "ghost");
        assert_eq!(labels(&t)[1], "powershell 2");
    }

    /// Shell detection is a lookup, not a probe: nothing is spawned to draw the
    /// menu, and a machine with a trimmed PATH still gets a shell.
    #[test]
    fn shell_detection_lists_what_is_actually_installed() {
        let dir = std::env::temp_dir().join("snor_shell_probe_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pwsh.exe"), b"").unwrap();
        std::fs::write(dir.join("bash.exe"), b"").unwrap();
        let path = dir.to_string_lossy().to_string();

        let shells = installed_from(&path, None);
        assert!(shells.contains(&ShellKind::PowerShell), "ships with Windows");
        assert!(shells.contains(&ShellKind::Pwsh));
        assert!(shells.contains(&ShellKind::GitBash));
        assert!(!shells.contains(&ShellKind::Wsl), "not installed here");
        assert!(
            !shells.contains(&ShellKind::Cmd),
            "cmd needs COMSPEC or its own entry on PATH"
        );

        // Windows filenames are case-insensitive, so the lookup must be too.
        std::fs::write(dir.join("WSL.EXE"), b"").unwrap();
        assert!(installed_from(&path, None).contains(&ShellKind::Wsl));

        // A COMSPEC that exists is enough for cmd.
        let cmd = dir.join("cmd.exe");
        std::fs::write(&cmd, b"").unwrap();
        let comspec = cmd.to_string_lossy().to_string();
        assert!(installed_from(&path, Some(&comspec)).contains(&ShellKind::Cmd));

        // The menu is never empty, whatever the environment says.
        assert_eq!(installed_from("", None), vec![ShellKind::PowerShell]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Every shell has to name a program to spawn: a variant with an empty
    /// program would fail at spawn time, in a menu the user cannot debug.
    #[test]
    fn every_shell_names_a_program_and_a_tab_title() {
        for shell in ShellKind::ALL {
            assert!(shell.program().ends_with(".exe"), "{:?}", shell);
            assert!(!shell.title_name().is_empty(), "{:?}", shell);
            assert!(!shell.label().is_empty(), "{:?}", shell);
            // Only Git Bash takes login/interactive flags; nothing here should
            // start a shell that reads a profile and prints a banner over the
            // grid on every new tab.
            if shell != ShellKind::GitBash {
                assert!(
                    !shell.args().contains(&"-l"),
                    "{:?} must not start a login shell",
                    shell
                );
            }
        }
        // The tab title of the default shell is what every existing label,
        // test and screenshot already says.
        assert_eq!(shell_title(1, ShellKind::default()), "powershell");
        assert_eq!(shell_title(2, ShellKind::Pwsh), "pwsh 2");
    }

    /// A pane's own title is only worth drawing once the shell has renamed itself
    /// away from the name it announced on startup — otherwise every pane would
    /// carry a brand in its header.
    #[test]
    fn a_pane_shows_a_title_only_once_the_shell_renames_itself() {
        let mut t = Terminal::new();
        assert!(s(&t).pane_title().is_none(), "nothing said yet");

        sm(&mut t).ingest(b"\x1b]0;Windows PowerShell\x07");
        assert!(
            s(&t).pane_title().is_none(),
            "the shell introducing itself is not a title"
        );

        sm(&mut t).ingest(b"\x1b]0;opencode: editing src/main.rs\x07");
        assert_eq!(
            s(&t).pane_title(),
            Some("opencode: editing src/main.rs"),
            "an agent narrating its work is"
        );

        // A whitespace-only title never reaches the session — the scanner drops
        // it — so the last real name stays on the header instead of blanking it.
        sm(&mut t).ingest(b"\x1b]0;   \x07");
        assert_eq!(s(&t).pane_title(), Some("opencode: editing src/main.rs"));
    }

    /// The number a supervisor actually reads: how long since this shell said
    /// anything. Coarse, and "now" doubles as the definition of working.
    #[test]
    fn idle_time_reads_the_way_a_supervisor_thinks() {
        use std::time::Duration;
        assert_eq!(short_idle(Duration::from_secs(0)), "now");
        assert_eq!(
            short_idle(Duration::from_secs(PANE_ACTIVE_SECS - 1)),
            "now",
            "the whole active window must read as working"
        );
        assert_eq!(short_idle(Duration::from_secs(PANE_ACTIVE_SECS)), "10s");
        assert_eq!(short_idle(Duration::from_secs(59)), "59s");
        assert_eq!(short_idle(Duration::from_secs(60)), "1m");
        assert_eq!(short_idle(Duration::from_secs(3599)), "59m");
        assert_eq!(short_idle(Duration::from_secs(3600)), "1h");

        // A shell that has not written yet reports nothing rather than "now".
        let t = Terminal::new();
        assert!(s(&t).idle_for().is_none());
    }

    /// "Who needs me" walks the flagged panes only, in grid order, and does
    /// nothing at all when there is nothing to walk to.
    #[test]
    fn attention_navigation_visits_only_the_panes_that_want_you() {
        let mut t = Terminal::new();
        t.flow_add_stub();
        t.flow_add_stub();
        assert_eq!(t.sessions.len(), 3);

        // Nothing flagged: the focus must not move.
        let before = t.flow_target_id();
        t.flow_step_attention();
        assert_eq!(t.flow_target_id(), before, "silence is not a destination");

        // Flag the first and third panes only.
        let ids: Vec<u64> = t.sessions.iter().map(|s| s.id).collect();
        for session in &mut t.sessions {
            session.attention = session.id == ids[0] || session.id == ids[2];
        }
        t.flow_focus = Some(ids[0]);
        t.flow_step_attention();
        assert_eq!(t.flow_target_id(), Some(ids[2]), "skips the quiet pane");
        // Wrapping: from the last flagged one, back to the first.
        t.flow_step_attention();
        assert_eq!(t.flow_target_id(), Some(ids[0]), "and wraps");

        // The walk always starts *after* the pane that counts as focused, and an
        // unfocused terminal already resolves to the first live pane — so from
        // that default the first press leads to the third, not to the pane you
        // are sitting on.
        t.flow_focus = None;
        assert_eq!(t.flow_target_id(), Some(ids[0]), "the default focus");
        t.flow_step_attention();
        assert_eq!(t.flow_target_id(), Some(ids[2]));
    }

    /// The tab strip's half of the same question.
    #[test]
    fn attention_navigation_switches_to_the_next_tab_that_wants_you() {
        let mut t = Terminal::new();
        t.open_stub_tab();
        t.open_stub_tab();
        t.active_tab = 0;

        t.step_attention_tab();
        assert_eq!(t.active_tab, 0, "no flags, no movement");

        t.sessions[1].attention = true;
        t.step_attention_tab();
        assert_eq!(t.active_tab, 1);
        assert!(t.active, "landing on a shell means it can be typed into");
        assert!(t.reveal_active_tab, "and the strip scrolls it into view");

        // Past the end it wraps to whatever is flagged behind it.
        t.sessions[1].attention = false;
        t.sessions[2].attention = true;
        t.active_tab = 1;
        t.step_attention_tab();
        assert_eq!(t.active_tab, 2);
        t.sessions[0].attention = true;
        t.step_attention_tab();
        assert_eq!(t.active_tab, 0, "wraps to the start");

        // Clearing the flags takes them out of the rotation again, and the
        // selection stays where it was rather than moving to an arbitrary tab.
        t.sessions[0].attention = false;
        t.sessions[2].attention = false;
        let here = t.active_tab;
        t.step_attention_tab();
        assert_eq!(t.active_tab, here, "nothing flagged means no movement");
    }

    /// Pointer to cell, which is what selection starts from.
    #[test]
    fn a_pointer_lands_on_the_cell_under_it() {
        use eframe::egui::pos2;
        let origin = pos2(10.0, 20.0);
        let cell = (8.0, 16.0);
        assert_eq!(cell_at_pos(origin, cell, pos2(10.0, 20.0)), (0, 0));
        assert_eq!(cell_at_pos(origin, cell, pos2(17.9, 35.9)), (0, 0));
        assert_eq!(cell_at_pos(origin, cell, pos2(18.0, 36.0)), (1, 1));
        // Above or left of the grid clamps rather than wrapping to the end.
        assert_eq!(cell_at_pos(origin, cell, pos2(0.0, 0.0)), (0, 0));
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

    /// Each pane keeps its own directory, and the focused one reports it for
    /// the explorer to adopt.
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
            t.context_session(true).map(|(_, cwd)| cwd),
            Some(std::path::PathBuf::from(r"C:\Projects\Backend"))
        );
        t.flow_step_focus(1);
        assert_eq!(
            t.context_session(true).map(|(_, cwd)| cwd),
            Some(std::path::PathBuf::from(r"C:\Projects\Frontend"))
        );
    }

    /// Outside Flow Mode the context is the active *tab*, not the pane grid —
    /// the two answer from different state, and auto context switching would
    /// follow the wrong one if this were wired to `flow_target_id` alone.
    #[test]
    fn context_session_follows_the_active_tab_in_normal_mode() {
        let mut t = Terminal::new();
        t.sessions[0].cwd = std::path::PathBuf::from(r"C:\Projects\One");
        let second = t.open_stub_tab().expect("stub tab");
        t.sessions[1].cwd = std::path::PathBuf::from(r"C:\Projects\Two");

        t.active_tab = 0;
        assert_eq!(
            t.context_session(false).map(|(_, cwd)| cwd),
            Some(std::path::PathBuf::from(r"C:\Projects\One"))
        );
        t.active_tab = 1;
        let (id, cwd) = t.context_session(false).expect("a context");
        assert_eq!(id, second, "the id is what the caller keys its sync on");
        assert_eq!(cwd, std::path::PathBuf::from(r"C:\Projects\Two"));
    }

    /// A hidden terminal reports no context: the section is not on screen, so
    /// re-rooting the explorer to it would be a change the user cannot see.
    #[test]
    fn hidden_terminal_reports_no_context() {
        let mut t = Terminal::new();
        t.hidden = true;
        assert!(t.context_session(false).is_none());
        assert!(t.context_session(true).is_none());
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

    /// The header is reserved before the shell is sized, so the PTY never
    /// reports a row the pane's grid does not draw.
    ///
    /// This is the invariant behind the frozen torn-TUI bug: sizing from the
    /// pane's raw rectangle told a shell it had `PANE_HEADER_H` more rows
    /// than the scroll area would show, and a TUI's last line landed off the
    /// bottom of the visible grid.
    #[test]
    fn flow_pane_sizing_reserves_its_header() {
        use super::{PANE_GRID_PAD, PANE_TOP_CHROME};
        let cell = (10.0, 10.0);
        let pane_h = 400.0;
        // 400px with a header and the grid's slack left is not 40 rows.
        let (rows, _) = term_size_for_pixels(1000.0, pane_h, cell);
        assert_eq!(rows, 40);
        // `PANE_TOP_CHROME` is the one value the renderer reserves and this
        // subtraction uses, so the two cannot drift — the header was once held
        // open as a side effect of laying out a label row, and deleting the
        // labels shrank the chrome in the renderer while this kept subtracting
        // the old height, promising the shell fewer rows than the pane showed.
        let grid_h = pane_h - PANE_TOP_CHROME - PANE_GRID_PAD;
        let (rows_reserved, _) = term_size_for_pixels(1000.0, grid_h, cell);
        assert_eq!(rows_reserved, 37, "the header must cost the shell its rows");
        assert!(
            rows_reserved < rows,
            "reserving the header can only shrink the grid, never grow it"
        );
    }

    /// The same invariant on the other axis: the side air is spent before the
    /// shell is sized, so the columns it is given are the columns the padded
    /// grid can draw.
    ///
    /// The failure this guards is subtle — a shell sized to the unpadded width
    /// has one more column than the grid shows, so a full-width line wraps and
    /// the last character lands on a row of its own, which looks like the shell
    /// mis-measuring rather than like padding.
    #[test]
    fn flow_pane_sizing_reserves_its_side_gaps() {
        use super::{GRID_SIDE_GAP, inset_grid_sides};
        let cell = (10.0, 10.0);
        let rect = eframe::egui::Rect::from_min_size(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::vec2(1000.0, 400.0),
        );
        let (_, cols) = term_size_for_pixels(rect.width(), 400.0, cell);
        assert_eq!(cols, 100);

        let padded = inset_grid_sides(rect);
        assert_eq!(padded.left(), GRID_SIDE_GAP);
        assert_eq!(padded.right(), rect.right() - GRID_SIDE_GAP);
        assert_eq!(padded.top(), rect.top(), "the vertical edges must not move");
        assert_eq!(
            padded.bottom(),
            rect.bottom(),
            "the vertical edges must not move"
        );

        let (_, cols_padded) = term_size_for_pixels(padded.width(), 400.0, cell);
        assert_eq!(cols_padded, 98, "the side air must cost the shell its columns");
        assert!(cols_padded < cols);
    }

    /// A pane narrower than its own padding must not invert its rectangle.
    #[test]
    fn inset_grid_sides_survives_a_pane_narrower_than_its_padding() {
        use super::{GRID_SIDE_GAP, inset_grid_sides};
        let thin = eframe::egui::Rect::from_min_size(
            eframe::egui::pos2(5.0, 5.0),
            eframe::egui::vec2(GRID_SIDE_GAP, 40.0),
        );
        let padded = inset_grid_sides(thin);
        assert_eq!(padded.width(), 0.0, "the gap clamps to half the width");
        assert!(padded.left() <= padded.right(), "the rect must not invert");
        assert_eq!(padded.top(), thin.top());
        assert_eq!(padded.bottom(), thin.bottom());
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
        assert!(
            t.notice_text().is_some(),
            "the status bar reads the refusal through notice_text()"
        );
        assert_eq!(leaves(&t).len(), MAX_SESSIONS);
        // Normal tabs obey the same cap.
        assert!(t.open_stub_tab().is_none());
        assert_eq!(t.sessions.len(), MAX_SESSIONS);
        // Closing a pane frees the slot, and the survivor's creation clears
        // the note, so the status bar cannot keep shouting after the fix.
        t.flow_close_focused();
        assert!(t.sessions.len() < MAX_SESSIONS);
        t.flow_add_stub();
        assert!(t.notice.is_none());
        assert!(t.notice_text().is_none());
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
