use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct GitFile {
    pub path: String,
    pub code: String,
}

pub struct GitPanel {
    pub branch: String,
    pub files: Vec<GitFile>,
    pub selected: Option<String>,
    pub diff: String,
    pub error: Option<String>,
    pub opened: Option<PathBuf>,
}

impl GitPanel {
    pub fn new() -> Self {
        Self {
            branch: String::from("-"),
            files: Vec::new(),
            selected: None,
            diff: String::new(),
            error: None,
            opened: None,
        }
    }

    pub fn refresh(&mut self, root: &PathBuf) {
        self.files.clear();
        self.error = None;
        let repo = match git2::Repository::open(root) {
            Ok(r) => r,
            Err(e) => {
                self.branch = String::from("not a repo");
                self.error = Some(e.to_string());
                return;
            }
        };
        self.branch = repo
            .head()
            .ok()
            .and_then(|h| h.shorthand().ok().map(|s| s.to_string()))
            .unwrap_or_else(|| "detached".to_string());
        let mut opts = git2::StatusOptions::new();
        opts.include_untracked(true).recurse_untracked_dirs(true);
        match repo.statuses(Some(&mut opts)) {
            Ok(statuses) => {
                for entry in statuses.iter() {
                    let path = entry.path().unwrap_or("?").to_string();
                    let st = entry.status();
                    let code = if st.contains(git2::Status::WT_NEW)
                        || st.contains(git2::Status::INDEX_NEW)
                    {
                        "A"
                    } else if st.contains(git2::Status::WT_MODIFIED)
                        || st.contains(git2::Status::INDEX_MODIFIED)
                    {
                        "M"
                    } else if st.contains(git2::Status::WT_DELETED)
                        || st.contains(git2::Status::INDEX_DELETED)
                    {
                        "D"
                    } else if st.contains(git2::Status::WT_RENAMED) {
                        "R"
                    } else {
                        "?"
                    };
                    self.files.push(GitFile {
                        path,
                        code: code.to_string(),
                    });
                }
                self.files.sort_by(|a, b| a.path.cmp(&b.path));
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        if let Some(sel) = self.selected.clone() {
            self.load_diff(root, &sel);
        } else {
            self.diff.clear();
        }
    }

    fn load_diff(&mut self, root: &Path, rel: &str) {
        let out = std::process::Command::new("git")
            .args(["-C", &root.display().to_string(), "diff", "--", rel])
            .output();
        match out {
            Ok(o) => {
                let mut text = String::from_utf8_lossy(&o.stdout).to_string();
                if text.trim().is_empty() {
                    let untracked = std::process::Command::new("git")
                        .args([
                            "-C",
                            &root.display().to_string(),
                            "status",
                            "--porcelain",
                            "--",
                            rel,
                        ])
                        .output();
                    if let Ok(uo) = untracked {
                        let st = String::from_utf8_lossy(&uo.stdout).to_string();
                        if st.starts_with("??") {
                            text = String::from("(untracked — not in diff)");
                        }
                    }
                }
                if text.len() > 100_000 {
                    text.truncate(100_000);
                }
                self.diff = text;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    pub fn ui(&mut self, ui: &mut eframe::egui::Ui, root: &PathBuf) {
        ui.horizontal(|ui| {
            ui.heading("Git");
            ui.label(
                eframe::egui::RichText::new(&self.branch)
                    .small()
                    .color(crate::theme::dim_text()),
            );
            ui.with_layout(
                eframe::egui::Layout::right_to_left(eframe::egui::Align::Center),
                |ui| {
                    if ui.small_button("refresh").clicked() {
                        self.refresh(root);
                    }
                },
            );
        });
        if let Some(err) = &self.error {
            ui.label(
                eframe::egui::RichText::new(err)
                    .small()
                    .color(crate::theme::dim_text()),
            );
        }
        ui.separator();
        eframe::egui::ScrollArea::vertical()
            .id_salt("snor_git_files")
            .max_height(160.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.files.is_empty() {
                    ui.label(eframe::egui::RichText::new("clean").color(crate::theme::dim_text()));
                }
                let files = self.files.clone();
                for f in &files {
                    let selected = self.selected.as_ref() == Some(&f.path);
                    if ui
                        .selectable_label(selected, format!("[{}] {}", f.code, f.path))
                        .clicked()
                    {
                        self.selected = Some(f.path.clone());
                        self.load_diff(root, &f.path);
                        let full = root.join(&f.path);
                        if full.is_file() {
                            self.opened = Some(full);
                        }
                    }
                }
            });
        ui.separator();
        ui.label(
            eframe::egui::RichText::new("diff")
                .small()
                .color(crate::theme::dim_text()),
        );
        eframe::egui::ScrollArea::both()
            .id_salt("snor_git_diff")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.diff.is_empty() {
                    ui.label(
                        eframe::egui::RichText::new("select a file")
                            .color(crate::theme::dim_text()),
                    );
                } else {
                    ui.monospace(&self.diff);
                }
            });
    }
}
