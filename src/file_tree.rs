use notify::{RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

const SKIP_DIRS: &[&str] = &["target", ".git", "node_modules", ".idea"];

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
            error: None,
            opened_file: None,
        }
    }
}

impl FileTree {
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

    fn parent_for_new(&self) -> PathBuf {
        if let Some(sel) = &self.selected {
            if sel.is_dir() {
                return sel.clone();
            }
            if let Some(p) = sel.parent() {
                return p.to_path_buf();
            }
        }
        self.root.clone()
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

    fn render_nodes(&mut self, ui: &mut eframe::egui::Ui, nodes: Vec<FileNode>) {
        for node in nodes {
            if node.is_dir {
                let is_open = self.expanded.contains(&node.path);
                let header = eframe::egui::CollapsingHeader::new(&node.name)
                    .id_salt(node.path.display().to_string())
                    .open(Some(is_open));
                let mut toggle: Option<bool> = None;
                let mut clicked_select = false;
                let resp = header.show(ui, |ui| {
                    let kids = node.children.clone();
                    self.render_nodes(ui, kids);
                });
                if resp.header_response.clicked() {
                    clicked_select = true;
                }
                // Detect open-state change by comparing after show
                if resp.openness > 0.5 && !is_open {
                    toggle = Some(true);
                } else if resp.openness < 0.5 && is_open {
                    toggle = Some(false);
                }
                if let Some(open) = toggle {
                    if open {
                        self.expanded.insert(node.path.clone());
                    } else {
                        self.expanded.remove(&node.path);
                    }
                }
                if clicked_select {
                    self.selected = Some(node.path.clone());
                }
            } else {
                let selected = self.selected.as_ref() == Some(&node.path);
                let resp = ui.selectable_label(selected, &node.name);
                if resp.clicked() {
                    self.selected = Some(node.path.clone());
                }
                if resp.double_clicked() {
                    self.selected = Some(node.path.clone());
                    self.opened_file = Some(node.path.clone());
                }
            }
        }
    }

    pub fn ui(&mut self, ui: &mut eframe::egui::Ui) {
        self.poll_watcher();

        ui.horizontal(|ui| {
            ui.heading("Explorer");
            ui.with_layout(
                eframe::egui::Layout::right_to_left(eframe::egui::Align::Center),
                |ui| {
                    if ui.small_button("refresh").clicked() {
                        self.refresh();
                    }
                },
            );
        });
        ui.horizontal(|ui| {
            if ui.small_button("+ file").clicked() {
                self.create_parent = self.parent_for_new();
                self.create_mode = Some(CreateMode::File);
                self.create_name.clear();
                self.error = None;
            }
            if ui.small_button("+ dir").clicked() {
                self.create_parent = self.parent_for_new();
                self.create_mode = Some(CreateMode::Dir);
                self.create_name.clear();
                self.error = None;
            }
            let can_edit = self.selected.is_some();
            if ui
                .add_enabled(can_edit, eframe::egui::Button::new("rename").small())
                .clicked()
                && let Some(sel) = self.selected.clone()
            {
                self.rename_target = Some(sel.clone());
                self.rename_buf = sel
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                self.error = None;
            }
            if ui
                .add_enabled(can_edit, eframe::egui::Button::new("delete").small())
                .clicked()
            {
                self.delete_target = self.selected.clone();
                self.error = None;
            }
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
                if resp.lost_focus() && ui.input(|i| i.key_pressed(eframe::egui::Key::Enter)) {
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
                if resp.lost_focus() && ui.input(|i| i.key_pressed(eframe::egui::Key::Enter)) {
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
            ui.colored_label(eframe::egui::Color32::from_rgb(0xE0, 0x6C, 0x75), err);
        }

        ui.separator();
        eframe::egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let nodes = self.nodes.clone();
                if nodes.is_empty() {
                    ui.label(
                        eframe::egui::RichText::new("empty folder").color(crate::theme::dim_text()),
                    );
                } else {
                    self.render_nodes(ui, nodes);
                }
            });

        // Delete confirm modal
        if let Some(target) = self.delete_target.clone() {
            eframe::egui::Window::new("confirm delete")
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
