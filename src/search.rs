use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub path: PathBuf,
    pub line: usize,
    pub preview: String,
}

pub struct Search {
    pub query: String,
    pub results: Vec<SearchHit>,
    pub error: Option<String>,
    pub searched: bool,
    pub opened: Option<PathBuf>,
}

impl Search {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            results: Vec::new(),
            error: None,
            searched: false,
            opened: None,
        }
    }

    pub fn run(&mut self, root: &PathBuf) {
        let q = self.query.trim().to_string();
        self.results.clear();
        self.error = None;
        self.searched = true;
        if q.len() < 2 {
            self.error = Some("type 2+ chars".to_string());
            return;
        }
        let q_lower = q.to_lowercase();
        let walker = ignore::WalkBuilder::new(root)
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                name != "target" && name != ".git" && name != "node_modules"
            })
            .build();
        for entry in walker.filter_map(|e| e.ok()) {
            if self.results.len() >= 2000 {
                self.error = Some("capped at 2000 hits".to_string());
                break;
            }
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if path.metadata().map(|m| m.len() > 1_000_000).unwrap_or(true) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(path) else {
                continue;
            };
            for (idx, line) in text.lines().enumerate() {
                if line.to_lowercase().contains(&q_lower) {
                    self.results.push(SearchHit {
                        path: path.to_path_buf(),
                        line: idx + 1,
                        preview: line.trim().chars().take(120).collect(),
                    });
                    if self.results.len() >= 2000 {
                        break;
                    }
                }
                if idx > 20_000 {
                    break;
                }
            }
        }
        if self.results.is_empty() && self.error.is_none() {
            self.error = Some("no matches".to_string());
        }
    }

    pub fn ui(&mut self, ui: &mut eframe::egui::Ui, root: &PathBuf) {
        ui.heading("Search");
        ui.horizontal(|ui| {
            let avail = ui.available_width();
            let resp = ui.add_sized(
                [(avail - 44.0).max(60.0), 22.0],
                eframe::egui::TextEdit::singleline(&mut self.query)
                    .hint_text("find text (2+ chars)"),
            );
            if resp.lost_focus() && ui.input(|i| i.key_pressed(eframe::egui::Key::Enter)) {
                self.run(root);
            }
            if ui.small_button("go").clicked() {
                self.run(root);
            }
        });
        if let Some(err) = &self.error {
            ui.label(
                eframe::egui::RichText::new(err)
                    .small()
                    .color(crate::theme::dim_text()),
            );
        } else if self.searched {
            ui.label(
                eframe::egui::RichText::new(format!("{} hits", self.results.len()))
                    .small()
                    .color(crate::theme::dim_text()),
            );
        }
        ui.separator();
        eframe::egui::ScrollArea::vertical()
            .id_salt("snor_search_results")
            .max_height(220.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for hit in &self.results {
                    let name = hit
                        .path
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if ui
                        .selectable_label(false, format!("{name}:{}  {}", hit.line, hit.preview))
                        .clicked()
                    {
                        self.opened = Some(hit.path.clone());
                    }
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_text_across_files() {
        let dir = std::env::temp_dir().join("snor_search_test");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("a.rs"), "fn alpha() {}\n").unwrap();
        std::fs::write(dir.join("b.txt"), "nothing here\n").unwrap();
        let mut s = Search::new();
        s.query = "alpha".to_string();
        s.run(&dir);
        assert_eq!(s.results.len(), 1);
        assert_eq!(s.results[0].line, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
