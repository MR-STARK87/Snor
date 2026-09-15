use eframe::egui;
use std::path::PathBuf;

pub struct SnorApp {
    root: PathBuf,
    status: String,
    files: Vec<String>,
}

impl SnorApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::theme::apply_dark(&cc.egui_ctx);
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut app = Self {
            root,
            status: String::from("ready"),
            files: Vec::new(),
        };
        app.refresh_files();
        app
    }

    fn refresh_files(&mut self) {
        self.files.clear();
        if let Ok(entries) = std::fs::read_dir(&self.root) {
            let mut names: Vec<String> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n != "target")
                .collect();
            names.sort();
            self.files = names;
        }
        self.status = format!("{} entries", self.files.len());
    }
}

impl eframe::App for SnorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("snor_top").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading(
                    egui::RichText::new("Snor")
                        .color(crate::theme::accent())
                        .strong(),
                );
                ui.label(
                    egui::RichText::new(self.root.display().to_string())
                        .color(crate::theme::dim_text())
                        .small(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new("main")
                            .small()
                            .color(crate::theme::dim_text()),
                    );
                    if ui.small_button("refresh").clicked() {
                        self.refresh_files();
                    }
                });
            });
        });

        egui::Panel::left("snor_tree")
            .default_size(240.0)
            .min_size(180.0)
            .show(ui, |ui| {
                ui.heading("Explorer");
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for name in &self.files {
                        let _ = ui.selectable_label(false, name);
                    }
                    if self.files.is_empty() {
                        ui.label(
                            egui::RichText::new("empty folder").color(crate::theme::dim_text()),
                        );
                    }
                });
            });

        egui::Panel::bottom("snor_terminal")
            .default_size(200.0)
            .min_size(120.0)
            .resizable(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Terminal");
                    ui.label(
                        egui::RichText::new("ConPTY powershell lands here (next)")
                            .small()
                            .color(crate::theme::dim_text()),
                    );
                });
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.monospace("$ powershell.exe");
                    ui.monospace("# run: opencode  (wired in terminal milestone)");
                });
            });

        egui::Panel::bottom("snor_status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&self.status).small());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new("UTF-8  Ln 1, Col 1")
                            .small()
                            .color(crate::theme::dim_text()),
                    );
                });
            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                let _ = ui.selectable_label(true, "welcome.rs");
                let _ = ui.selectable_label(false, "+");
            });
            ui.separator();
            egui::ScrollArea::both().show(ui, |ui| {
                ui.monospace("// Snor shell up. Editor + highlight lands next milestone.");
                ui.monospace("// V1 target: <250MB idle vs Zed 980MB.");
                ui.add_space(12.0);
                ui.label(format!("root: {}", self.root.display()));
            });
        });

        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(500));
    }
}
