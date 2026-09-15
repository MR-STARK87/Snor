use notify::{RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::icons;
use crate::theme;

const SKIP_DIRS: &[&str] = &["target", ".git", "node_modules", ".idea"];

// --- Row geometry ------------------------------------------------------
//
// Every figure below was read off the reference mock by finding the ink
// extents of the glyphs in a row, then dividing by 1.25 (the mock renders at
// 125%: its 331px explorer panel is 264.8pt, against our 260pt default).
//
// Row pitch is 35px = 28pt. egui adds `item_spacing.y` between allocated
// rows, so ROW_H is the pitch minus that spacing.
//
// The figures are *not* a tidy progression, because the reference's are not:
// its chevron step and its glyph step differ by a couple of points. Rather
// than round them into a clean lattice that would be wrong at one end of the
// tree, each column is pinned to where the mock actually puts it.
/// Height of one tree row's box, before egui's 4pt inter-row spacing.
///
/// The reference's rows step 34px, so the box is 34/1.25 - 4 = 23.2pt.
const ROW_H: f32 = 23.2;
/// Left edge of the level-0 chevron column, inside the panel's frame margin.
///
/// Solved from the reference's level-1 chevron, whose ink centres on x=98px:
/// that is 44.8pt from the panel's left edge, minus the 8pt frame, minus
/// [`INDENT`] for the level step, minus half of [`CHEVRON_BOX`].
const PAD: f32 = 9.0;
/// Horizontal step per tree level, for chevrons, glyphs and labels alike.
///
/// The reference moves a child's *label* 26px (20.8pt) right of its parent's,
/// and its glyph column with it. Note this is not the chevron's step — see
/// [`ROOT_CHEVRON`].
const INDENT: f32 = 21.75;
/// Width of the disclosure-chevron column that folders get and files skip.
///
/// Both kinds put their glyph at `chevron_left + CHEVRON_COL`, so a folder and
/// a sibling file share a glyph column: the reference's `src` folder ink spans
/// x 117..136 and the `main.rs` badge ink x 145..160, both at level 1 and 2 of
/// the same subtree, and both land inside this column.
const CHEVRON_COL: f32 = 21.5;
/// Distance from a node's glyph column to its label.
///
/// The reference puts the level-1 label at x=152px, i.e. 80pt from the panel's
/// left edge; with the glyph column at 52.25pt and the frame's 8pt, that
/// leaves this. Checked at level 2: the model predicts a label at 101.75pt
/// against the mock's 100.8pt.
const GLYPH_COL: f32 = 27.75;
/// Box the disclosure chevron is drawn in. The reference's collapsed chevron
/// inks 6x11px and its expanded one 10x6px, which is this box through
/// [`icons::chevron`]'s 0.70 x 1.36 aspect.
const CHEVRON_BOX: f32 = 12.0;
/// Centre of the *root* row's chevron, relative to the row's left edge.
///
/// The root row is not on the level-0 grid: the reference drops it 3.75pt
/// further right than [`PAD`] alone would put it (x=75.5px, against 71px for a
/// chevron on the lattice), because the root has no glyph column to line up
/// with and is inset on its own terms.
const ROOT_CHEVRON: f32 = 18.8;
/// The root row's label, relative to the row's left edge. Measured at x=95px.
const ROOT_LABEL: f32 = 34.4;
/// The selection pill stops this far short of the panel's right edge (13px in
/// the reference). Measured against the panel, not the row, because the row
/// ends inside the panel's frame margin — see [`PANEL_MARGIN`].
const PILL_RIGHT_GAP: f32 = 10.4;
/// egui's default panel frame inset, which the tree's rows sit inside. Needed
/// only to translate "10.4pt from the panel's edge" into a row-local x.
const PANEL_MARGIN: f32 = 8.0;
/// The selection pill is taller than its row box: 34px in the reference
/// against a 35px pitch, i.e. it fills the row plus its spacing. This is why
/// the pill is painted with the unclipped painter — `painter_at(row)` would
/// trim the bleed away.
const PILL_BLEED: f32 = 1.6;
/// Label size for ordinary rows. The reference's "README.md" has an 11px cap
/// height and ours measures the same at this size.
const ROW_TEXT: f32 = 12.5;
/// The root row is set as a heading: its cap height is 13px against 11px for
/// the rows below it.
const ROOT_TEXT: f32 = 14.5;
/// Size of the knocked-out letter inside a file badge. The reference's letter
/// fills most of its 12.8x13.6pt badge, which is larger than a monospace face
/// would give at the box's own height. It is also heavier than egui's bundled
/// monospace can draw, so this matches the reference's *size* and not its
/// weight — the one place in the tree where the two knowingly differ.
const BADGE_LETTER: f32 = 10.0;

// --- Header geometry ---------------------------------------------------
/// Hit box for a header action. The reference's glyphs are 11.2pt wide and
/// their centres sit 32pt apart, which is this box plus egui's 8pt spacing.
const HEADER_BTN: f32 = 26.0;
/// The reference's rightmost glyph stops 17.6pt short of the panel's edge;
/// the panel frame already supplies 8pt of that.
const HEADER_RIGHT_PAD: f32 = 9.6;
/// Air between the explorer header and the first tree row.
///
/// The reference inks its root row 6px below where the header's own spacing
/// would leave it, with no rule in between to account for the difference.
const TREE_TOP_GAP: f32 = 4.8;

// --- Footer geometry ---------------------------------------------------
//
// Expressed as fractions of the explorer's width, measured off the reference
// mock, so the block keeps the same proportions whether the panel is at its
// 260pt default or dragged out to 520. In the reference the mascot's ink is
// inset 50/332 of the panel and spans 124/332 of it, the caption shares the
// mascot's left edge, and the caption's baseline sits 20/332 above the
// status bar.
//
// The fractions were then calibrated against a screenshot of this app: the
// reference is a raster mock, so its figures are ink extents, while ours are
// layout boxes, and the two differ by the mascot's empty bottom band and the
// caption font's descender space.
/// Left inset of both the mascot and its caption.
const FOOTER_PAD: f32 = 0.150;
/// Width of the mascot block (the glyph fills it edge to edge).
const FOOTER_MASCOT: f32 = 0.400;
/// Gap below the caption, before the status bar. Larger than the reference's
/// 6% because it is measured from the caption's *row* bottom rather than its
/// ink, and the row carries the font's descender space.
const FOOTER_BOTTOM_GAP: f32 = 0.094;
/// Caption text size in points. Deliberately fixed rather than proportional:
/// scaling it with the panel would look shouty, and at the default width this
/// reproduces the reference's caption width almost exactly.
const FOOTER_CAPTION_SIZE: f32 = 11.0;

/// What the user did with an inline name field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InlineOutcome {
    /// Still typing. Leave the row up.
    Open,
    /// Enter, or the commit button.
    Commit,
    /// Escape, the cancel button, or focus leaving the row.
    Cancel,
}

/// A one-line name field with its commit/cancel buttons.
///
/// Focus is the whole reason this is a function rather than six lines inline.
/// The field is opened by a button elsewhere in the panel, and egui's
/// `text_edit_singleline` does not take focus on its own — so the row used to
/// appear with nothing focused, `lost_focus()` could never fire, and clicking
/// away left it sitting there until the user found "cancel". Asking for focus
/// once (not every frame, which would make it impossible to give away) fixes
/// both halves.
fn inline_name_field(
    ui: &mut egui::Ui,
    value: &mut String,
    focus_pending: &mut bool,
    commit_label: &str,
) -> InlineOutcome {
    let mut commit = false;
    let mut cancel = false;
    let mut lost = false;

    let rect = ui
        .horizontal(|ui| {
            // Right-to-left so the buttons claim their width *first* and the
            // field is sized against what is left. Laid out the other way the
            // field asks for `spacing().text_edit_width` (280pt) against a
            // ~244pt panel, takes every point of it, and pushes both buttons
            // past the panel's edge where they are clipped away entirely.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("cancel").clicked() {
                    cancel = true;
                }
                if ui.small_button(commit_label).clicked() {
                    commit = true;
                }
                let resp = ui.text_edit_singleline(value);
                if std::mem::take(focus_pending) {
                    resp.request_focus();
                }
                if resp.lost_focus() {
                    lost = true;
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        commit = true;
                    }
                }
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    cancel = true;
                }
            });
        })
        .response
        .rect;

    if commit {
        return InlineOutcome::Commit;
    }
    if cancel {
        return InlineOutcome::Cancel;
    }
    if lost {
        // Focus went elsewhere — but a press on this row's *own* buttons also
        // steals focus, and a button only reports `clicked()` on release. So
        // the pointer still being over the row has to count as "not yet gone",
        // otherwise the row would be torn down before the button ever fired.
        let over_row = ui
            .ctx()
            .input(|i| i.pointer.latest_pos())
            .is_some_and(|p| rect.expand(2.0).contains(p));
        if !over_row {
            return InlineOutcome::Cancel;
        }
    }
    InlineOutcome::Open
}

/// Height the footer will claim at the bottom of the panel.
///
/// The caller needs this *before* the footer is laid out, because the
/// scrolling tree above it is sized against the remaining space and the
/// footer is pinned to the bottom. The caption's row height comes from the
/// live font metrics rather than a guessed multiplier, so the reservation
/// stays exact if the font ever changes.
fn footer_height(ui: &egui::Ui, panel_w: f32) -> f32 {
    let caption = ui
        .ctx()
        .fonts_mut(|f| f.row_height(&egui::FontId::proportional(FOOTER_CAPTION_SIZE)));
    FOOTER_BOTTOM_GAP * panel_w + caption + crate::mascot::height_for(FOOTER_MASCOT * panel_w)
}

/// Left edge of a node's chevron slot, relative to the row's left edge.
/// Folders only; files skip this column.
fn chevron_left(depth: usize) -> f32 {
    PAD + depth as f32 * INDENT
}

/// Centre of a node's chevron slot. Split out from [`chevron_left`] because
/// both the root row and the selection pill are positioned by a chevron's
/// centre rather than its edge.
fn chevron_center(depth: usize) -> f32 {
    chevron_left(depth) + CHEVRON_BOX * 0.5
}

/// Left edge of a node's glyph slot, relative to the row's left edge.
///
/// Every node — folder or file — puts its glyph one chevron-column in, so a
/// folder and a sibling file line up, and each level indents past the level
/// above it.
fn glyph_left(depth: usize) -> f32 {
    chevron_left(depth) + CHEVRON_COL
}

/// Left edge of a node's label, relative to the row's left edge.
fn label_left(depth: usize) -> f32 {
    glyph_left(depth) + GLYPH_COL
}

/// Left edge of a node's selection pill, relative to the row's left edge.
///
/// The reference's pill reaches back past its own row to the *parent's*
/// chevron: for `main.rs` at level 2 it starts at x=98px, which is exactly
/// where `src`'s chevron sits one level up. Anchoring on the row's own column
/// instead would start it 20pt too far right. Clamped at [`PAD`] so the root's
/// own pill cannot run off the panel's edge.
fn pill_left(depth: usize) -> f32 {
    if depth == 0 {
        PAD
    } else {
        chevron_center(depth - 1).max(PAD)
    }
}

fn should_skip(name: &str) -> bool {
    SKIP_DIRS.contains(&name)
}

#[derive(Debug, Clone)]
pub struct FileNode {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub children: Vec<FileNode>,
}

fn build_nodes(dir: &Path, depth: usize) -> Vec<FileNode> {
    if depth > 8 {
        return Vec::new();
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name().to_string_lossy().to_string();
        if name == ".git" {
            continue;
        }
        if should_skip(&name) {
            continue;
        }
        let path = entry.path();
        let is_dir = path.is_dir();
        if is_dir {
            let children = build_nodes(&path, depth + 1);
            dirs.push(FileNode {
                path,
                name,
                is_dir: true,
                children,
            });
        } else {
            files.push(FileNode {
                path,
                name,
                is_dir: false,
                children: Vec::new(),
            });
        }
        if dirs.len() + files.len() > 2000 {
            break;
        }
    }
    dirs.sort_by_key(|a| a.name.to_lowercase());
    files.sort_by_key(|a| a.name.to_lowercase());
    dirs.extend(files);
    dirs
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CreateMode {
    File,
    Dir,
}

pub struct FileTree {
    pub root: PathBuf,
    pub nodes: Vec<FileNode>,
    pub selected: Option<PathBuf>,
    expanded: HashSet<PathBuf>,
    _watcher: Option<notify::RecommendedWatcher>,
    rx: Option<Receiver<notify::Result<notify::Event>>>,
    last_event: Option<Instant>,
    needs_refresh: bool,
    create_mode: Option<CreateMode>,
    create_parent: PathBuf,
    create_name: String,
    /// Ask the create/rename field for focus on its first frame.
    ///
    /// One-shot on purpose: the inline row is opened by a button, and a text
    /// edit that is never focused reports `lost_focus()` never, so clicking
    /// away could not close it. Requesting focus every frame would instead
    /// make it impossible to *give* focus away, which is the same bug.
    inline_focus_pending: bool,
    rename_target: Option<PathBuf>,
    rename_buf: String,
    delete_target: Option<PathBuf>,
    /// Set by the header's "open folder" button. The shell polls this with
    /// [`FileTree::take_open_request`] rather than the tree opening the picker
    /// itself, because switching roots also re-seeds the editor and terminal
    /// working directory, which the tree does not own.
    open_requested: bool,
    pub error: Option<String>,
    pub opened_file: Option<PathBuf>,
}

impl FileTree {
    pub fn new(root: PathBuf) -> Self {
        let (tx, rx): (
            Sender<notify::Result<notify::Event>>,
            Receiver<notify::Result<notify::Event>>,
        ) = std::sync::mpsc::channel();
        let watcher = match notify::recommended_watcher(move |ev| {
            let _ = tx.send(ev);
        }) {
            Ok(mut w) => {
                if w.watch(&root, RecursiveMode::Recursive).is_ok() {
                    Some(w)
                } else {
                    None
                }
            }
            Err(_) => None,
        };
        let nodes = build_nodes(&root, 0);
        let mut expanded = HashSet::new();
        expanded.insert(root.clone());
        Self {
            root: root.clone(),
            nodes,
            selected: None,
            expanded,
            _watcher: watcher,
            rx: Some(rx),
            last_event: None,
            needs_refresh: false,
            create_mode: None,
            create_parent: root,
            create_name: String::new(),
            inline_focus_pending: false,
            rename_target: None,
            rename_buf: String::new(),
            delete_target: None,
            open_requested: false,
            error: None,
            opened_file: None,
        }
    }

    /// Take a pending "open folder" click, clearing the flag.
    pub fn take_open_request(&mut self) -> bool {
        std::mem::take(&mut self.open_requested)
    }

    /// Whether `path` is currently shown expanded. Only meaningful for folders.
    pub fn is_expanded(&self, path: &Path) -> bool {
        self.expanded.contains(path)
    }

    /// Flip a folder open/closed. Split out of the click handler so the
    /// behaviour is testable without a live `Ui`.
    pub fn toggle_expanded(&mut self, path: &Path) {
        if !self.expanded.remove(path) {
            self.expanded.insert(path.to_path_buf());
        }
    }

    pub fn set_root(&mut self, root: PathBuf) {
        if let Some(w) = self._watcher.as_mut() {
            let _ = w.unwatch(&self.root);
            let _ = w.watch(&root, RecursiveMode::Recursive);
        }
        self.create_parent = root.clone();
        self.root = root.clone();
        self.selected = None;
        self.expanded.clear();
        self.expanded.insert(root);
        self.create_mode = None;
        self.create_name.clear();
        self.rename_target = None;
        self.delete_target = None;
        self.error = None;
        self.opened_file = None;
        self.refresh();
    }

    pub fn begin_create_at_root(&mut self, dir: bool) {
        let root = self.root.clone();
        self.begin_create(
            root,
            if dir {
                CreateMode::Dir
            } else {
                CreateMode::File
            },
        );
    }

    pub fn refresh(&mut self) {
        self.nodes = build_nodes(&self.root, 0);
        self.needs_refresh = false;
        self.last_event = None;
    }

    pub fn poll_watcher(&mut self) {
        if let Some(rx) = &self.rx {
            let mut got = false;
            while rx.try_recv().is_ok() {
                got = true;
            }
            if got {
                self.last_event = Some(Instant::now());
            }
        }
        if let Some(t) = self.last_event {
            if t.elapsed() > Duration::from_millis(300) {
                self.refresh();
            }
        } else if self.needs_refresh {
            self.refresh();
        }
    }

    fn begin_create(&mut self, parent: PathBuf, mode: CreateMode) {
        self.create_parent = parent;
        self.create_mode = Some(mode);
        self.create_name.clear();
        self.inline_focus_pending = true;
        self.error = None;
    }

    fn begin_rename(&mut self, path: PathBuf) {
        self.rename_buf = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        self.rename_target = Some(path);
        self.inline_focus_pending = true;
        self.error = None;
    }

    fn do_create(&mut self) {
        let name = self.create_name.trim().to_string();
        if name.is_empty() || name.contains(['/', '\\']) {
            self.error = Some("invalid name".to_string());
            return;
        }
        let target = self.create_parent.join(&name);
        if target.exists() {
            self.error = Some("already exists".to_string());
            return;
        }
        let res = match self.create_mode {
            Some(CreateMode::Dir) => std::fs::create_dir_all(&target),
            _ => std::fs::write(&target, ""),
        };
        match res {
            Ok(()) => {
                self.error = None;
                self.create_mode = None;
                self.create_name.clear();
                self.expanded.insert(self.create_parent.clone());
                self.selected = Some(target);
                self.refresh();
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn do_rename(&mut self) {
        let old = match self.rename_target.clone() {
            Some(p) => p,
            None => return,
        };
        let new_name = self.rename_buf.trim().to_string();
        if new_name.is_empty() || new_name.contains(['/', '\\']) {
            self.error = Some("invalid name".to_string());
            return;
        }
        let new_path = match old.parent() {
            Some(p) => p.join(&new_name),
            None => return,
        };
        if new_path.exists() {
            self.error = Some("already exists".to_string());
            return;
        }
        match std::fs::rename(&old, &new_path) {
            Ok(()) => {
                self.error = None;
                self.rename_target = None;
                self.selected = Some(new_path);
                self.refresh();
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn do_delete(&mut self) {
        let target = match self.delete_target.clone() {
            Some(p) => p,
            None => return,
        };
        let res = if target.is_dir() {
            std::fs::remove_dir_all(&target)
        } else {
            std::fs::remove_file(&target)
        };
        match res {
            Ok(()) => {
                self.error = None;
                self.delete_target = None;
                if self.selected.as_ref() == Some(&target) {
                    self.selected = None;
                }
                self.refresh();
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    /// Draw the project root as a row of its own, the way the reference does:
    /// a disclosure chevron and the folder's name, set larger than the rows
    /// below it, with no glyph — the rows underneath are already indented one
    /// level in, so an icon there would only repeat the panel header.
    ///
    /// Returns whether the root is expanded, so the caller can skip the tree
    /// entirely when it is collapsed.
    fn render_root_row(&mut self, ui: &mut egui::Ui) -> bool {
        let root = self.root.clone();
        let name = root
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| root.display().to_string());
        let base = ui.cursor().left();
        let row_w = ui.available_width().max(80.0);
        let (row, resp) = ui.allocate_exact_size(egui::vec2(row_w, ROW_H), egui::Sense::click());
        let is_open = self.is_expanded(&root);

        if ui.is_rect_visible(row) {
            let painter = ui.painter_at(row);
            if resp.hovered() {
                let pill = egui::Rect::from_min_max(
                    egui::pos2(base + pill_left(0), row.top() - PILL_BLEED),
                    egui::pos2(
                        row.right() + PANEL_MARGIN - PILL_RIGHT_GAP,
                        row.bottom() + PILL_BLEED,
                    ),
                );
                ui.painter()
                    .rect_filled(pill, 5.0, egui::Color32::from_rgb(0x19, 0x21, 0x1F));
            }
            icons::chevron(
                &painter,
                egui::Rect::from_center_size(
                    egui::pos2(base + ROOT_CHEVRON, row.center().y),
                    egui::vec2(CHEVRON_BOX, CHEVRON_BOX),
                ),
                if is_open { 1.0 } else { 0.0 },
                theme::glyph(),
            );
            let font = egui::FontId::proportional(ROOT_TEXT);
            let text_x = base + ROOT_LABEL;
            let label = icons::truncate(&painter, &name, font.clone(), (row.right() - text_x).max(8.0));
            painter.text(
                egui::pos2(text_x, row.center().y),
                egui::Align2::LEFT_CENTER,
                label,
                font,
                theme::text_bright(),
            );
        }

        if resp.clicked() {
            self.toggle_expanded(&root);
        }
        resp.context_menu(|ui| {
            if ui.button("new file here").clicked() {
                self.begin_create(root.clone(), CreateMode::File);
                ui.close();
            }
            if ui.button("new folder here").clicked() {
                self.begin_create(root.clone(), CreateMode::Dir);
                ui.close();
            }
        });
        is_open
    }

    fn render_nodes(&mut self, ui: &mut egui::Ui, nodes: Vec<FileNode>, depth: usize) {
        for node in nodes {
            let base = ui.cursor().left();
            let row_w = ui.available_width().max(80.0);
            let (row, resp) =
                ui.allocate_exact_size(egui::vec2(row_w, ROW_H), egui::Sense::click());

            let is_open = node.is_dir && self.is_expanded(&node.path);
            let selected = self.selected.as_ref() == Some(&node.path);

            if ui.is_rect_visible(row) {
                let painter = ui.painter_at(row);
                // The pill starts at the row's own chevron column and bleeds
                // into the inter-row spacing, which is what the reference's
                // 34px pill against a 35px pitch amounts to. It stops short of
                // the panel's right edge rather than running to it.
                let pill = egui::Rect::from_min_max(
                    egui::pos2(base + pill_left(depth), row.top() - PILL_BLEED),
                    egui::pos2(
                        row.right() + PANEL_MARGIN - PILL_RIGHT_GAP,
                        row.bottom() + PILL_BLEED,
                    ),
                );
                if selected {
                    // `ui.painter()`, not `painter`: the row-clipped painter
                    // would cut off the bleed above and below.
                    ui.painter().rect_filled(pill, 5.0, theme::tab_active());
                } else if resp.hovered() {
                    ui.painter()
                        .rect_filled(pill, 5.0, egui::Color32::from_rgb(0x19, 0x21, 0x1F));
                }
                // The reference's tree is pale warm line art. The glyph ink
                // does *not* change with selection — only the badge's fill
                // does — so this is one colour for every row.
                let glyph = theme::glyph();
                if node.is_dir {
                    icons::chevron(
                        &painter,
                        egui::Rect::from_center_size(
                            egui::pos2(base + chevron_center(depth), row.center().y),
                            egui::vec2(CHEVRON_BOX, CHEVRON_BOX),
                        ),
                        if is_open { 1.0 } else { 0.0 },
                        glyph,
                    );
                }
                // Glyph slots are sized per kind, from the reference's ink
                // extents: a folder is 20x17px, a badge 16x17px, a document
                // 17x20px. They are centred on the row, so the kinds sit on a
                // common optical baseline.
                let (gw, gh) = if node.is_dir {
                    (16.0, 13.6)
                } else if theme::file_letter(&node.name).is_some() {
                    (12.8, 13.6)
                } else {
                    (13.6, 16.0)
                };
                let slot = egui::Rect::from_center_size(
                    egui::pos2(base + glyph_left(depth) + gw * 0.5, row.center().y),
                    egui::vec2(gw, gh),
                );
                if node.is_dir {
                    icons::folder(&painter, slot, glyph);
                } else {
                    match theme::file_letter(&node.name) {
                        Some(letter) => {
                            // Always a *solid* badge with the letter knocked
                            // out of it — never an outline. Only the fill
                            // tracks selection: accent when selected, the
                            // light grey otherwise.
                            let fill = if selected {
                                theme::accent()
                            } else {
                                theme::badge_fill()
                            };
                            icons::badge_filled(
                                &painter,
                                slot,
                                letter,
                                BADGE_LETTER,
                                fill,
                                theme::on_accent(),
                            );
                        }
                        None => icons::doc(&painter, slot, glyph),
                    }
                }
                let text_x = base + label_left(depth);
                let font = egui::FontId::proportional(ROW_TEXT);
                let label =
                    icons::truncate(&painter, &node.name, font.clone(), (row.right() - text_x).max(8.0));
                painter.text(
                    egui::pos2(text_x, row.center().y),
                    egui::Align2::LEFT_CENTER,
                    label,
                    font,
                    // Unselected labels are as bright as a folder's, not
                    // dimmed: the reference keeps `dim_text` for the panel's
                    // captions and lifts the selected row one step further.
                    if selected {
                        theme::text_strong()
                    } else {
                        theme::text()
                    },
                );
            }

            if resp.clicked() {
                self.selected = Some(node.path.clone());
                if node.is_dir {
                    self.toggle_expanded(&node.path);
                } else {
                    self.opened_file = Some(node.path.clone());
                }
            }
            let menu_path = node.path.clone();
            let is_dir = node.is_dir;
            resp.context_menu(|ui| {
                if is_dir {
                    if ui.button("new file here").clicked() {
                        self.begin_create(menu_path.clone(), CreateMode::File);
                        ui.close();
                    }
                    if ui.button("new folder here").clicked() {
                        self.begin_create(menu_path.clone(), CreateMode::Dir);
                        ui.close();
                    }
                    ui.separator();
                }
                if ui.button("rename").clicked() {
                    self.begin_rename(menu_path.clone());
                    ui.close();
                }
                if ui.button("delete").clicked() {
                    self.delete_target = Some(menu_path.clone());
                    self.error = None;
                    ui.close();
                }
            });

            if is_open {
                let kids = node.children.clone();
                let top = ui.cursor().top();
                self.render_nodes(ui, kids, depth + 1);
                let bottom = ui.cursor().top();
                // Elbow guide running down the children, as in the reference.
                ui.painter().vline(
                    base + chevron_center(depth),
                    egui::Rangef::new(top, bottom),
                    egui::Stroke::new(1.0, theme::hairline()),
                );
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.poll_watcher();

        // The reference leaves a band of air above the header row: its label's
        // centre sits 21.6pt below the panel's top edge.
        ui.add_space(8.0);

        // One right-to-left row, so the buttons claim their width *first* and
        // the label is what gives way when the panel is squeezed. Laid out
        // left-to-right the label took its full width before the buttons were
        // placed, and past roughly 150pt of panel the two overlapped — the
        // heading ran straight under the icons.
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // The reference's two header glyphs sit 17.6pt in from the
                // panel's right edge and 32pt apart. Right-to-left, so this
                // reads "open folder | rescan | new file" across the header:
                // the action that swaps the whole tree sits furthest from the
                // edge, away from the frequent ones.
                ui.add_space(HEADER_RIGHT_PAD);
                if icons::icon_button(ui, HEADER_BTN, "new file", icons::plus).clicked() {
                    self.begin_create_at_root(false);
                }
                if icons::icon_button(ui, HEADER_BTN, "rescan", |p, r, c| {
                    icons::refresh(p, r.shrink(7.0), c)
                })
                .clicked()
                {
                    self.refresh();
                }
                if icons::icon_button(ui, HEADER_BTN, "open folder", |p, r, c| {
                    icons::folder_open(p, r.shrink(7.0), c)
                })
                .clicked()
                {
                    self.open_requested = true;
                }
                // The glyph and the title go in a *nested left-to-right* row.
                // Two things are load-bearing here. First, the nesting: in the
                // right-to-left flow above, the label was added before the
                // glyph and took the space the glyph needed, so at narrow
                // widths the glyph was squeezed to nothing and disappeared.
                // Left-to-right the glyph claims its 18pt first and the label
                // truncates into whatever is left.
                //
                // Second, `with_layout` rather than `horizontal`: `Ui::horizontal`
                // *inherits* the parent's direction — `horizontal_with_main_wrap_dyn`
                // reads `self.placer.prefer_right_to_left()` and picks the layout
                // from it — so a nested `horizontal` here is right-to-left too
                // and draws the title before its own folder. This has to say
                // which way it means.
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let (slot, _) =
                        ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                    if ui.is_rect_visible(slot) {
                        icons::folder(
                            &ui.painter_at(slot),
                            egui::Rect::from_center_size(slot.center(), egui::vec2(17.6, 15.0)),
                            theme::text(),
                        );
                    }
                    let head = ui.add(
                        egui::Label::new(
                            egui::RichText::new("Workspace").size(14.0).color(theme::text()),
                        )
                        .truncate(),
                    );
                    head.context_menu(|ui| {
                        if ui.button("new file here").clicked() {
                            self.begin_create(self.root.clone(), CreateMode::File);
                            ui.close();
                        }
                        if ui.button("new folder here").clicked() {
                            self.begin_create(self.root.clone(), CreateMode::Dir);
                            ui.close();
                        }
                    });
                });
            });
        });

        if let Some(mode) = self.create_mode {
            ui.separator();
            ui.label(format!(
                "new {} in {}",
                if mode == CreateMode::Dir {
                    "folder"
                } else {
                    "file"
                },
                self.create_parent.display()
            ));
            match inline_name_field(
                ui,
                &mut self.create_name,
                &mut self.inline_focus_pending,
                "create",
            ) {
                InlineOutcome::Commit => self.do_create(),
                InlineOutcome::Cancel => {
                    self.create_mode = None;
                    self.create_name.clear();
                }
                InlineOutcome::Open => {}
            }
        }

        if self.rename_target.is_some() {
            ui.separator();
            ui.label("rename to:");
            match inline_name_field(
                ui,
                &mut self.rename_buf,
                &mut self.inline_focus_pending,
                "apply",
            ) {
                InlineOutcome::Commit => self.do_rename(),
                InlineOutcome::Cancel => self.rename_target = None,
                InlineOutcome::Open => {}
            }
        }

        if let Some(err) = &self.error {
            ui.colored_label(theme::danger(), err);
        }

        // No rule between the header and the tree. The reference has one, but
        // it belongs to the *editor's* tab bar and stops at the panel's edge —
        // scanning the mock's explorer column for horizontal lines finds only
        // the title bar's, so drawing one here pushed the whole tree 8px down.
        //
        // What the mock does have is air: with the rule gone the root row
        // landed 6px *above* where the reference inks it, so that gap is put
        // back explicitly.
        ui.add_space(TREE_TOP_GAP);

        // The footer is pinned to the bottom of the panel, so reserve its
        // height before sizing the scrolling tree above it.
        let panel_w = ui.available_width();
        let tree_h = (ui.available_height() - footer_height(ui, panel_w)).max(60.0);
        egui::ScrollArea::vertical()
            .id_salt("snor_tree_scroll")
            .max_height(tree_h)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let nodes = self.nodes.clone();
                // The root gets a row of its own, so everything below it starts
                // one level in.
                if self.render_root_row(ui) {
                    if nodes.is_empty() {
                        ui.label(
                            egui::RichText::new("empty folder — right-click Workspace for options")
                                .color(theme::dim_text()),
                        );
                    } else {
                        self.render_nodes(ui, nodes, 1);
                    }
                }
            });

        // Footer, matched to the reference: left-aligned rather than centred,
        // sized as a fraction of the panel, caption under the mascot, and a
        // gap before the status bar. No rule above it — in the reference the
        // only lines down here are the status bar's own top edge.
        //
        // There is no space added between the mascot and the caption: the
        // mascot's blobs stop short of the bottom of the block they are given,
        // and that empty band *is* the gap the reference shows between the
        // feet and the text. Adding more on top of it doubled the gap.
        ui.horizontal(|ui| {
            ui.add_space(FOOTER_PAD * panel_w);
            crate::mascot::snorlax(ui, FOOTER_MASCOT * panel_w)
                .on_hover_text("Rest. Then build again.");
        });
        ui.horizontal(|ui| {
            ui.add_space(FOOTER_PAD * panel_w);
            ui.label(
                egui::RichText::new("Rest. Then build again.")
                    .size(FOOTER_CAPTION_SIZE)
                    .color(theme::moss()),
            );
        });
        ui.add_space(FOOTER_BOTTOM_GAP * panel_w);

        // Delete confirm modal
        if let Some(target) = self.delete_target.clone() {
            egui::Window::new("confirm delete")
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.label(format!("delete {}?", target.display()));
                    ui.horizontal(|ui| {
                        if ui.button("delete").clicked() {
                            self.do_delete();
                        }
                        if ui.button("cancel").clicked() {
                            self.delete_target = None;
                        }
                    });
                });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: files used to skip the chevron column in the wrong place, so
    /// a child row's badge landed *left* of its parent's folder glyph.
    #[test]
    fn child_rows_indent_past_their_parents_glyph() {
        for d in 0..6 {
            assert!(
                chevron_left(d) + CHEVRON_BOX <= glyph_left(d),
                "level {d}: the chevron box overlaps its own glyph column"
            );
            assert!(
                glyph_left(d + 1) > glyph_left(d),
                "level {} does not indent past level {d}",
                d + 1
            );
            assert!(
                chevron_left(d + 1) > chevron_left(d),
                "level {} chevron does not indent",
                d + 1
            );
            assert!(
                pill_left(d) >= PAD,
                "level {d}: the selection pill runs off the panel's left edge"
            );
        }
    }

    /// Pins every derived column to the reference mock's measured ink.
    ///
    /// The mock's panel interior starts at x=42px and renders at 125%, so a
    /// pixel position converts to `(x - 42) / 1.25` points from the panel's
    /// left edge; the row itself sits inside egui's 8pt frame margin. Without
    /// this, a "cleaner" constant would silently slide a whole column.
    #[test]
    fn columns_match_the_reference_ink() {
        const FRAME: f32 = 8.0;
        let from_panel = |x_px: f32| (x_px - 42.0) / 1.25;

        // Measured: level-1 chevron ink 94..102px, folder ink 117..136px,
        // label starts 152px; level-2 badge ink 145..160px, label 178px; the
        // selected level-2 pill starts at 98px.
        let checks: [(&str, f32, f32); 6] = [
            ("level-1 chevron centre", FRAME + chevron_center(1), 98.0),
            ("level-1 folder centre", FRAME + glyph_left(1) + 8.0, 126.5),
            ("level-1 label", FRAME + label_left(1), 152.0),
            ("level-2 badge centre", FRAME + glyph_left(2) + 6.4, 152.5),
            ("level-2 label", FRAME + label_left(2), 178.0),
            ("level-2 pill left", FRAME + pill_left(2), 98.0),
        ];
        for (what, got, want_px) in checks {
            let want = from_panel(want_px);
            assert!(
                (got - want).abs() < 1.0,
                "{what}: {got:.2}pt, reference {want:.2}pt"
            );
        }
    }

    #[test]
    fn lists_project_root() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let nodes = build_nodes(&root, 0);
        let names: Vec<_> = nodes.iter().map(|n| n.name.as_str()).collect();
        assert!(names.contains(&"Cargo.toml"), "names: {names:?}");
        assert!(names.contains(&"src"), "names: {names:?}");
        assert!(!names.contains(&"target"), "names: {names:?}");
        assert!(!names.contains(&".git"), "names: {names:?}");
        let src = nodes.iter().find(|n| n.name == "src").unwrap();
        assert!(src.is_dir && !src.children.is_empty());
    }

    /// Regression: the tree used to drive `CollapsingHeader::open(Some(..))`
    /// every frame, which takes egui's click-to-toggle branch out of the
    /// picture (`if let Some(open) = open { .. } else if clicked`), so no
    /// folder could ever be expanded.
    #[test]
    fn folders_toggle_open_and_closed() {
        let dir = std::env::temp_dir().join("snor_tree_toggle");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("inner")).unwrap();
        std::fs::write(dir.join("inner").join("a.txt"), "x").unwrap();

        let mut tree = FileTree::new(dir.clone());
        let inner = dir.join("inner");
        assert!(!tree.is_expanded(&inner));
        tree.toggle_expanded(&inner);
        assert!(tree.is_expanded(&inner), "folder did not open");
        tree.toggle_expanded(&inner);
        assert!(!tree.is_expanded(&inner), "folder did not close");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
