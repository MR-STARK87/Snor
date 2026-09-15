use notify::{RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::icons;
use crate::theme;

const SKIP_DIRS: &[&str] = &["target", ".git", "node_modules", ".idea"];

/// Height of one tree row. The reference's rows are noticeably taller than
/// egui's default label height, which is what makes the glyphs breathe.
const ROW_H: f32 = 20.0;
/// Left padding before the first glyph column.
const PAD: f32 = 6.0;
/// Horizontal offset per tree level.
///
/// Must exceed [`CHEVRON_COL`]: a file's badge starts one chevron-column in, so
/// a child would otherwise land *left* of its parent's glyph.
const INDENT: f32 = 15.0;
/// Width of the disclosure-chevron column that folders get and files skip, so
/// that a folder's glyph and a sibling file's glyph line up.
const CHEVRON_COL: f32 = 13.0;
/// Width of the file/folder glyph column.
const GLYPH_COL: f32 = 21.0;

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

/// Left edge of a node's glyph slot, relative to the row's left edge.
///
/// Every node — folder or file — puts its glyph one chevron-column in, so a
/// folder and a sibling file line up, and each level indents past the level
/// above it.
fn glyph_left(depth: usize) -> f32 {
    chevron_left(depth) + CHEVRON_COL
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

#[derive(Debug, PartialEq, Eq)]
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
        self.error = None;
    }

    fn begin_rename(&mut self, path: PathBuf) {
        self.rename_buf = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        self.rename_target = Some(path);
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
                if selected {
                    painter.rect_filled(row, 5.0, theme::tab_active());
                } else if resp.hovered() {
                    painter.rect_filled(row, 5.0, egui::Color32::from_rgb(0x19, 0x21, 0x1F));
                }
                // Selected rows get the accent; everything else stays in the
                // muted outline grey, as in the reference.
                let glyph = if selected {
                    theme::accent()
                } else {
                    theme::outline()
                };
                if node.is_dir {
                    icons::chevron(
                        &painter,
                        egui::Rect::from_center_size(
                            egui::pos2(base + chevron_left(depth) + CHEVRON_COL * 0.5, row.center().y),
                            egui::vec2(12.0, 12.0),
                        ),
                        if is_open { 1.0 } else { 0.0 },
                        glyph,
                    );
                }
                let slot = egui::Rect::from_min_size(
                    egui::pos2(base + glyph_left(depth), row.center().y - 7.0),
                    egui::vec2(14.0, 14.0),
                );
                if node.is_dir {
                    icons::folder(&painter, slot, glyph);
                } else {
                    match theme::file_letter(&node.name) {
                        Some(letter) => icons::badge_outlined(&painter, slot, letter, glyph),
                        None => icons::doc(&painter, slot, glyph),
                    }
                }
                let text_x = base + glyph_left(depth) + GLYPH_COL;
                let font = egui::FontId::proportional(12.5);
                let label =
                    icons::truncate(&painter, &node.name, font.clone(), (row.right() - text_x).max(8.0));
                painter.text(
                    egui::pos2(text_x, row.center().y),
                    egui::Align2::LEFT_CENTER,
                    label,
                    font,
                    if selected {
                        theme::text()
                    } else {
                        theme::dim_text()
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
                    base + chevron_left(depth) + CHEVRON_COL * 0.5,
                    egui::Rangef::new(top, bottom),
                    egui::Stroke::new(1.0, theme::hairline()),
                );
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.poll_watcher();

        ui.horizontal(|ui| {
            let (slot, _) = ui.allocate_exact_size(egui::vec2(16.0, 18.0), egui::Sense::hover());
            if ui.is_rect_visible(slot) {
                icons::folder(
                    &ui.painter_at(slot),
                    egui::Rect::from_center_size(slot.center(), egui::vec2(15.0, 14.0)),
                    theme::text(),
                );
            }
            let head = ui.label(
                egui::RichText::new("Explorer")
                    .size(13.0)
                    .color(theme::text()),
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
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Right-to-left, so this reads "open folder | rescan | new file"
                // across the header: the action that swaps the whole tree sits
                // furthest from the edge, away from the frequent ones.
                if icons::icon_button(ui, 20.0, "new file", icons::plus).clicked() {
                    self.begin_create_at_root(false);
                }
                if icons::icon_button(ui, 20.0, "rescan", icons::refresh).clicked() {
                    self.refresh();
                }
                if icons::icon_button(ui, 20.0, "open folder", icons::folder_open).clicked() {
                    self.open_requested = true;
                }
            });
        });

        if let Some(mode) = &self.create_mode {
            ui.separator();
            ui.label(format!(
                "new {} in {}",
                if *mode == CreateMode::Dir {
                    "folder"
                } else {
                    "file"
                },
                self.create_parent.display()
            ));
            ui.horizontal(|ui| {
                let resp = ui.text_edit_singleline(&mut self.create_name);
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    self.do_create();
                }
                if ui.small_button("create").clicked() {
                    self.do_create();
                }
                if ui.small_button("cancel").clicked() {
                    self.create_mode = None;
                }
            });
        }

        if self.rename_target.is_some() {
            ui.separator();
            ui.label("rename to:");
            ui.horizontal(|ui| {
                let resp = ui.text_edit_singleline(&mut self.rename_buf);
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    self.do_rename();
                }
                if ui.small_button("apply").clicked() {
                    self.do_rename();
                }
                if ui.small_button("cancel").clicked() {
                    self.rename_target = None;
                }
            });
        }

        if let Some(err) = &self.error {
            ui.colored_label(theme::danger(), err);
        }

        ui.separator();
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
                if nodes.is_empty() {
                    ui.label(
                        egui::RichText::new("empty folder — right-click Explorer for options")
                            .color(theme::dim_text()),
                    );
                } else {
                    self.render_nodes(ui, nodes, 0);
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
                chevron_left(d) + CHEVRON_COL <= glyph_left(d),
                "level {d}: chevron overlaps its own glyph"
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
