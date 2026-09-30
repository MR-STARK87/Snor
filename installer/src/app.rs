//! The wizard: screens, chrome and the worker hand-off.
//!
//! The shape is the app's shell on purpose: the window is undecorated and the
//! title bar, its controls and the window's own edge are drawn by us, off the
//! same constants and the same shared palette (see `lib.rs`). Two things are
//! deliberately absent relative to the app: maximise and the resize bands, and
//! the F11 path — this is a fixed dialog, and there is no workspace behind it.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use eframe::egui;

use crate::install::{self, Plan};
use crate::payload::Bundle;
use crate::ui;
use crate::{icons, theme};

// --- Chrome constants --------------------------------------------------
//
// Values copied from the app's `app.rs`: layout, not palette — the palette
// itself is shared source. A change to the bar's proportions belongs in both
// files together or in neither.
const TITLE_PAD: f32 = 9.0;
const TITLE_PAD_BELOW: f32 = 5.0;
const TITLE_LEFT_PAD: f32 = 13.6;
const TITLE_DOT: f32 = 4.0;
const TITLE_DOT_LEAD: f32 = 8.0;
const TITLE_RIGHT_GAP: f32 = 44.8;
const CTRL_BOX: f32 = 24.0;
const CTRL_STEP: f32 = 43.2;
const CTRL_EDGE: f32 = 20.4;
const WINDOW_EDGE_W: f32 = 1.6;
/// The same reserve the app's placement assumes: egui has no work-area field,
/// and the Windows taskbar is 48pt at the default scaling.
const TASKBAR_RESERVE: f32 = 48.0;
const MIN_USABLE_H: f32 = 200.0;
/// The body's side margin.
const BODY_PAD: f32 = 30.0;

/// What this process was started to do. Decided before the window exists,
/// because it decides the title — and it is the whole of the "is this the
/// setup or the uninstaller" question, because the uninstaller is a copy of
/// the setup binary with no payload.
pub enum Launch {
    Install(Bundle),
    Uninstall(PathBuf),
    MissingPayload(String),
}

impl Launch {
    pub fn detect() -> Self {
        let exe = std::env::current_exe().unwrap_or_default();
        let dir = exe.parent().map(PathBuf::from).unwrap_or_default();
        if std::env::args().any(|arg| arg == "--uninstall") {
            return Launch::Uninstall(dir);
        }
        match crate::payload::parse_file(&exe) {
            Ok(Some(bundle)) => Launch::Install(bundle),
            Err(message) => Launch::MissingPayload(message),
            // No payload in this file. Priority matters here: an installed
            // directory contains an `snor.exe` too, so the manifest — not the
            // sidecar — is what identifies the uninstaller, and the sidecar
            // is only read when neither a payload nor a manifest is present.
            Ok(None) => {
                if dir.join(install::MANIFEST).is_file() {
                    return Launch::Uninstall(dir);
                }
                match crate::payload::sidecar(&exe) {
                    Some(sidecar) => match std::fs::read(&sidecar) {
                        Ok(app) => Launch::Install(Bundle {
                            exe: app,
                            prefix_len: std::fs::metadata(&exe)
                                .map(|meta| meta.len())
                                .unwrap_or(0),
                        }),
                        Err(e) => {
                            Launch::MissingPayload(format!("could not read {}: {e}", sidecar.display()))
                        }
                    },
                    None => Launch::MissingPayload(
                        "This copy of Snor Setup does not contain Snor. \
                         Download the full setup file and run that."
                            .into(),
                    ),
                }
            }
        }
    }

    pub fn window_title(&self) -> &'static str {
        match self {
            Launch::Uninstall(_) => "Snor Uninstall",
            Launch::Install(_) | Launch::MissingPayload(_) => "Snor Setup",
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Install,
    Uninstall,
}

/// The wizard's pages. Progress and Done are reached by working; `Failed` is
/// reachable from anywhere a step can refuse.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Screen {
    Welcome,
    Options,
    Progress,
    Done,
    Failed,
}

/// What the worker thread reports back.
enum Msg {
    /// Stage `i` is starting.
    Stage(usize),
    /// The run is over. The screen switches on this — see [`SetupApp::poll`].
    Done(Result<(), String>),
}

pub struct SetupApp {
    mode: Mode,
    screen: Screen,
    bundle: Option<Bundle>,
    uninstall_dir: PathBuf,
    dir: String,
    start_menu: bool,
    desktop: bool,
    launch_after: bool,
    stages: Vec<&'static str>,
    stage_now: usize,
    run: Option<Receiver<Msg>>,
    failure: Option<String>,
    installed_exe: Option<PathBuf>,
    cleanup_scheduled: bool,
    window_fitted: bool,
}

impl SetupApp {
    pub fn new(cc: &eframe::CreationContext<'_>, launch: Launch) -> Self {
        // The app's own entry-point pair: the same faces, the same palette.
        crate::fonts::install(&cc.egui_ctx);
        crate::theme::apply_dark(&cc.egui_ctx);

        let (mode, bundle, uninstall_dir, failure, screen) = match launch {
            Launch::Install(bundle) => (
                Mode::Install,
                Some(bundle),
                PathBuf::new(),
                None,
                Screen::Welcome,
            ),
            Launch::Uninstall(dir) => (Mode::Uninstall, None, dir, None, Screen::Welcome),
            Launch::MissingPayload(message) => {
                (Mode::Install, None, PathBuf::new(), Some(message), Screen::Failed)
            }
        };
        Self {
            mode,
            screen,
            bundle,
            uninstall_dir,
            dir: install::default_dir().display().to_string(),
            start_menu: true,
            desktop: true,
            launch_after: true,
            stages: Vec::new(),
            stage_now: 0,
            run: None,
            failure,
            installed_exe: None,
            cleanup_scheduled: false,
            window_fitted: false,
        }
    }

    /// Where the window should sit so it is fully visible: centred on the
    /// monitor, minus the taskbar reserve. The app's `fit_to_monitor` logic
    /// without the resizing case — this window is fixed-size, so only the
    /// corner is computed. Pure, so it is testable without a window.
    fn centred_on_monitor(monitor: egui::Vec2, want: egui::Vec2) -> (egui::Vec2, egui::Pos2) {
        let usable_h = (monitor.y - TASKBAR_RESERVE).max(MIN_USABLE_H);
        let size = egui::vec2(want.x.min(monitor.x.max(1.0)), want.y.min(usable_h));
        let pos = egui::pos2((monitor.x - size.x) * 0.5, (usable_h - size.y) * 0.5);
        (size, pos)
    }

    /// One-shot, first frame: `main.rs` sets no position, so Windows cascades
    /// the window wherever it likes. Nothing happens until egui has reported a
    /// monitor size, and the flag is only set once the move is issued — a
    /// `None` on the first frame must retry, not give up permanently.
    fn fit_on_first_frame(&mut self, ctx: &egui::Context) {
        if self.window_fitted {
            return;
        }
        let (monitor, size, maximized, fullscreen) = ctx.input(|i| {
            let viewport = i.viewport();
            (
                viewport.monitor_size,
                viewport.inner_rect.or(viewport.outer_rect).map(|r| r.size()),
                viewport.maximized.unwrap_or(false),
                viewport.fullscreen.unwrap_or(false),
            )
        });
        let (Some(monitor), Some(size)) = (monitor, size) else {
            return;
        };
        // A monitor of 1x1 (or less) is not a real measurement; retry.
        if monitor.x <= 1.0 || monitor.y <= 1.0 {
            return;
        }
        self.window_fitted = true;
        if maximized || fullscreen {
            return;
        }
        let (fitted, pos) = Self::centred_on_monitor(monitor, size);
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(fitted));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
    }
}
impl eframe::App for SetupApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.fit_on_first_frame(ui.ctx());
        self.poll();
        // Before the panels: the uninstaller's folder can only be deleted once
        // this process is gone, so the moment a close is *requested* is the
        // moment to hand that job off — whichever way the close arrives.
        if ui.ctx().input(|i| i.viewport().close_requested()) {
            self.hand_off_cleanup_if_done();
        }

        egui::Panel::top("setup_title")
            .frame(egui::Frame::side_top_panel(ui.style()).fill(theme::surface_title()))
            .show(ui, |ui| self.title_bar(ui));
        egui::Panel::bottom("setup_actions")
            .frame(
                egui::Frame::side_top_panel(ui.style())
                    .fill(theme::surface_recessed())
                    .inner_margin(egui::Margin::symmetric(20, 12)),
            )
            .show(ui, |ui| self.action_bar(ui));
        egui::CentralPanel::default().show(ui, |ui| self.body(ui));

        // The window's own frame, last and on the foreground layer, exactly as
        // the app draws it: with the OS decorations off, this is the boundary.
        ui.ctx()
            .layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("setup_window_edge"),
            ))
            .rect_stroke(
                ui.ctx().viewport_rect(),
                0.0,
                egui::Stroke::new(WINDOW_EDGE_W, theme::window_edge()),
                egui::StrokeKind::Inside,
            );

        // Faster while a worker is running, so the stage list keeps up; an
        // idle screen only needs enough wake-ups to stay responsive to the OS.
        let wait = if self.run.is_some() { 60 } else { 250 };
        ui.ctx().request_repaint_after(Duration::from_millis(wait));
    }
}
impl SetupApp {
    fn title_bar(&mut self, ui: &mut egui::Ui) {
        // Registered *first* so the controls added below win the hit test
        // where they overlap the drag band — the app learned this the hard
        // way: a drag band registered last swallows every click on close.
        let drag = ui.interact(
            ui.max_rect(),
            egui::Id::new("setup_title_drag"),
            egui::Sense::click_and_drag(),
        );
        if drag.dragged() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        ui.add_space(TITLE_PAD);
        ui.horizontal(|ui| {
            ui.add_space(TITLE_LEFT_PAD);
            ui.label(
                egui::RichText::new("Snor")
                    .size(18.8)
                    .family(theme::medium())
                    .color(theme::accent()),
            );
            ui.add_space(TITLE_DOT_LEAD);
            let (dot, _) =
                ui.allocate_exact_size(egui::vec2(TITLE_DOT, TITLE_DOT), egui::Sense::hover());
            if ui.is_rect_visible(dot) {
                ui.painter()
                    .circle_filled(dot.center(), TITLE_DOT * 0.5, theme::faint());
            }
            ui.label(
                egui::RichText::new(match self.mode {
                    Mode::Install => "Setup",
                    Mode::Uninstall => "Uninstall",
                })
                .size(11.5)
                .color(theme::tagline()),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.window_controls(ui);
                ui.add_space(TITLE_RIGHT_GAP);
                // The app's own bar mark, so the two title bars read as one
                // product: the label first, so the leaf ends up on its left.
                ui.label(
                    egui::RichText::new("Stay consistent.")
                        .size(12.5)
                        .color(theme::dim_text()),
                );
                let (slot, _) =
                    ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                if ui.is_rect_visible(slot) {
                    icons::leaf(&ui.painter_at(slot), slot, theme::accent());
                }
            });
        });
        ui.add_space(TITLE_PAD_BELOW);
    }

    /// Close and minimise, in the theme. There is no maximise: a fixed dialog
    /// has nothing to maximise. Both carry the real command — with the OS
    /// decorations off, they are the only window controls there are.
    fn window_controls(&mut self, ui: &mut egui::Ui) {
        let ink = theme::window_control();
        let step = ui.spacing().item_spacing.x;
        ui.spacing_mut().item_spacing.x = CTRL_STEP - CTRL_BOX;
        ui.add_space(CTRL_EDGE - CTRL_BOX * 0.5);

        if icons::icon_button(ui, CTRL_BOX, "close", |p, r, _| {
            icons::close_x(
                p,
                egui::Rect::from_center_size(r.center(), egui::vec2(8.3 / 0.4, 8.3 / 0.4)),
                ink,
            );
        })
        .clicked()
        {
            self.close(ui.ctx());
        }
        if icons::icon_button(ui, CTRL_BOX, "minimize", |p, r, _| {
            icons::minus(
                p,
                egui::Rect::from_center_size(r.center(), egui::vec2(9.9 / 0.4, 9.9 / 0.4)),
                ink,
            );
        })
        .clicked()
        {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }

        ui.spacing_mut().item_spacing.x = step;
    }
}
impl SetupApp {
    fn close(&mut self, ctx: &egui::Context) {
        self.hand_off_cleanup_if_done();
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    /// A finished uninstall cannot delete the folder it is running from, so
    /// the remainder is handed to `install::schedule_cleanup` — once, and only
    /// when there is a finished uninstall to clean up after. Every close path
    /// funnels through here: our cross, Escape, Alt+F4, the taskbar menu.
    fn hand_off_cleanup_if_done(&mut self) {
        if self.cleanup_scheduled || self.mode != Mode::Uninstall || self.screen != Screen::Done {
            return;
        }
        self.cleanup_scheduled = true;
        install::schedule_cleanup(&self.uninstall_dir);
    }

    fn action_bar(&mut self, ui: &mut egui::Ui) {
        // Enter activates the primary action on the screens that have one,
        // unless a text field owns the keyboard. Escape closes from anywhere
        // except a running install — there is nothing to cancel into.
        let editing = ui.memory(|m| m.focused().is_some());
        let enter =
            !editing && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        let escape = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if escape && self.screen != Screen::Progress {
            self.close(ui.ctx());
            return;
        }

        ui.horizontal(|ui| {
            if self.screen == Screen::Options && ui::ghost_button(ui, "Back").clicked() {
                self.screen = Screen::Welcome;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                match self.screen {
                    Screen::Welcome => {
                        let label = if self.mode == Mode::Install {
                            "Continue"
                        } else {
                            "Uninstall"
                        };
                        if ui::primary_button(ui, label).clicked() || enter {
                            self.begin();
                        }
                    }
                    Screen::Options => {
                        if ui::primary_button(ui, "Install").clicked() || enter {
                            self.begin();
                        }
                    }
                    Screen::Done => {
                        let label = if self.mode == Mode::Install {
                            "Finish"
                        } else {
                            "Close"
                        };
                        if ui::primary_button(ui, label).clicked() || enter {
                            self.finish(ui.ctx());
                        }
                    }
                    Screen::Failed => {
                        if ui::primary_button(ui, "Close").clicked() || enter {
                            self.close(ui.ctx());
                        }
                    }
                    Screen::Progress => {
                        ui::caption(
                            ui,
                            "This only takes a moment — leave this window alone until it finishes.",
                        );
                    }
                }
            });
        });
    }
}
impl SetupApp {
    fn body(&mut self, ui: &mut egui::Ui) {
        ui.add_space(26.0);
        ui.horizontal(|ui| {
            ui.add_space(BODY_PAD);
            ui.vertical(|ui| {
                ui.set_max_width((ui.available_width() - BODY_PAD).max(200.0));
                match self.screen {
                    Screen::Welcome => self.welcome(ui),
                    Screen::Options => self.options(ui),
                    Screen::Progress => self.progress(ui),
                    Screen::Done => self.done(ui),
                    Screen::Failed => self.failed(ui),
                }
            });
        });
    }

    fn welcome(&mut self, ui: &mut egui::Ui) {
        match self.mode {
            Mode::Install => {
                ui::heading(ui, "Welcome to Snor.");
                ui.add_space(6.0);
                ui::subtitle(ui, "A lightweight workspace for directing coding agents — a file tree, a highlighted editor and as many real shells as you need, in one calm window.");
                ui.add_space(16.0);
                ui::card(ui, |ui| {
                    ui::bullet(ui, "Installs for you alone — no administrator password is asked for.");
                    ui::bullet(ui, "Adds a Start Menu shortcut, and a desktop one if you want it.");
                    ui::bullet(ui, "Uninstalls from Windows Settings, or from its own uninstaller.");
                });
            }
            Mode::Uninstall => {
                ui::heading(ui, "Remove Snor?");
                ui.add_space(6.0);
                ui::subtitle(ui, "Snor, its shortcuts and its entry in Windows Settings will be removed.");
                ui.add_space(16.0);
                ui::card(ui, |ui| {
                    ui::bullet(ui, "Your settings in %APPDATA%\\Snor are left alone.");
                    ui::bullet(ui, "Your projects are never touched.");
                });
            }
        }
        ui.add_space(18.0);
        ui::caption(
            ui,
            &format!(
                "Version {} — Windows {}",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::ARCH
            ),
        );
    }

    fn options(&mut self, ui: &mut egui::Ui) {
        ui::heading(ui, "Where should Snor live?");
        ui.add_space(6.0);
        ui::subtitle(ui, "Snor installs for you alone, so Windows never asks for a password.");
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            let spacing = ui.spacing().item_spacing.x;
            let browse_w = 96.0;
            let field_w = (ui.available_width() - browse_w - spacing).max(160.0);
            ui.add_sized(
                [field_w, 26.0],
                egui::TextEdit::singleline(&mut self.dir).hint_text("install folder"),
            );
            if ui::ghost_button(ui, "Browse…").clicked() {
                self.browse_for_folder();
            }
        });
        ui.add_space(12.0);
        ui.checkbox(&mut self.start_menu, "Add a Start Menu shortcut");
        ui.checkbox(&mut self.desktop, "Add a desktop shortcut");
        ui.add_space(16.0);
        ui::caption(ui, "Snor keeps its settings in %APPDATA%\\Snor; uninstalling never touches them, or your projects.");
    }

    fn browse_for_folder(&mut self) {
        let current = clean_dir(&self.dir);
        let mut dialog = rfd::FileDialog::new();
        if current.is_dir() {
            dialog = dialog.set_directory(current);
        }
        if let Some(chosen) = dialog.pick_folder() {
            self.dir = chosen.display().to_string();
        }
    }
}
impl SetupApp {
    fn progress(&mut self, ui: &mut egui::Ui) {
        ui::heading(
            ui,
            match self.mode {
                Mode::Install => "Installing Snor.",
                Mode::Uninstall => "Removing Snor.",
            },
        );
        ui.add_space(18.0);
        for (index, stage) in self.stages.iter().enumerate() {
            let state = match index.cmp(&self.stage_now) {
                std::cmp::Ordering::Less => ui::StepState::Done,
                std::cmp::Ordering::Equal => ui::StepState::Active,
                std::cmp::Ordering::Greater => ui::StepState::Pending,
            };
            ui::step_row(ui, stage, state);
            ui.add_space(2.0);
        }
        ui.add_space(20.0);
        let fraction = self.stage_now as f32 / self.stages.len().max(1) as f32;
        ui::progress_bar(ui, fraction);
    }

    fn done(&mut self, ui: &mut egui::Ui) {
        match self.mode {
            Mode::Install => {
                ui::heading(ui, "Snor is installed.");
                ui.add_space(8.0);
                if let Some(exe) = &self.installed_exe {
                    ui::subtitle(ui, &exe.display().to_string());
                }
                ui.add_space(16.0);
                ui.checkbox(&mut self.launch_after, "Open Snor");
                ui.add_space(12.0);
                ui::caption(ui, "Close this window whenever you like — nothing is waiting on it.");
            }
            Mode::Uninstall => {
                ui::heading(ui, "Snor has been removed.");
                ui.add_space(8.0);
                ui::subtitle(
                    ui,
                    "Its shortcuts and its Windows Settings entry are gone; your settings file and your projects were left alone.",
                );
                ui.add_space(16.0);
                ui::caption(ui, "The uninstaller finishes tidying its own folder as this window closes.");
            }
        }
    }

    fn failed(&mut self, ui: &mut egui::Ui) {
        ui::heading(
            ui,
            match self.mode {
                Mode::Install => "Setup could not finish.",
                Mode::Uninstall => "The uninstall could not finish.",
            },
        );
        ui.add_space(10.0);
        if let Some(message) = &self.failure {
            ui.label(egui::RichText::new(message).size(13.0).color(theme::danger()));
        }
        ui.add_space(14.0);
        ui::caption(
            ui,
            "Fix the cause above and run this again — setup writes over whatever it left behind.",
        );
    }
}
impl SetupApp {
    fn begin(&mut self) {
        match self.mode {
            Mode::Install => {
                if self.screen == Screen::Welcome {
                    self.screen = Screen::Options;
                } else {
                    self.start_install();
                }
            }
            Mode::Uninstall => self.start_uninstall(),
        }
    }

    fn start_install(&mut self) {
        let Some(bundle) = self.bundle.take() else {
            self.failure =
                Some("the Snor payload went missing between screens — start setup again".into());
            self.screen = Screen::Failed;
            return;
        };
        let plan = Plan {
            dir: clean_dir(&self.dir),
            start_menu: self.start_menu,
            desktop: self.desktop,
        };
        let own = std::env::current_exe().unwrap_or_default();
        self.stages = install::INSTALL_STAGES.to_vec();
        self.run_worker(move |report| install::install(&plan, &bundle, &own, report));
    }

    fn start_uninstall(&mut self) {
        let dir = self.uninstall_dir.clone();
        self.stages = install::UNINSTALL_STAGES.to_vec();
        self.run_worker(move |report| install::uninstall(&dir, report));
    }

    /// Run `work` on a thread and watch it from the UI. The channel is the
    /// only thing the two share, so the worker never touches egui state and
    /// the window keeps drawing while a slow step — PowerShell, mostly —
    /// runs behind it.
    fn run_worker(
        &mut self,
        work: impl FnOnce(&mut dyn FnMut(usize)) -> Result<(), String> + Send + 'static,
    ) {
        let (sender, receiver) = mpsc::channel();
        self.run = Some(receiver);
        self.stage_now = 0;
        self.failure = None;
        self.screen = Screen::Progress;
        std::thread::spawn(move || {
            let mut report = |stage: usize| {
                let _ = sender.send(Msg::Stage(stage));
            };
            let result = work(&mut report);
            let _ = sender.send(Msg::Done(result));
        });
    }
}
impl SetupApp {
    /// Drain the worker's channel. The screen switches on `Msg::Done`, never
    /// on the channel closing: a worker that panicked would otherwise leave
    /// the progress screen up forever, and a silent hang is the one failure a
    /// progress screen cannot express.
    fn poll(&mut self) {
        let Some(receiver) = &self.run else {
            return;
        };
        loop {
            match receiver.try_recv() {
                Ok(Msg::Stage(stage)) => self.stage_now = stage,
                Ok(Msg::Done(result)) => {
                    self.run = None;
                    match result {
                        Ok(()) => {
                            self.stage_now = self.stages.len();
                            if self.mode == Mode::Install {
                                self.installed_exe = Some(clean_dir(&self.dir).join("snor.exe"));
                            }
                            self.screen = Screen::Done;
                        }
                        Err(message) => {
                            self.failure = Some(message);
                            self.screen = Screen::Failed;
                        }
                    }
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.run = None;
                    self.failure = Some(
                        "setup stopped unexpectedly — its work may be incomplete; run it again"
                            .into(),
                    );
                    self.screen = Screen::Failed;
                    break;
                }
            }
        }
    }

    fn finish(&mut self, ctx: &egui::Context) {
        if self.mode == Mode::Install
            && self.launch_after
            && let Some(exe) = &self.installed_exe
        {
            let _ = std::process::Command::new(exe)
                .current_dir(exe.parent().unwrap_or(std::path::Path::new(".")))
                .spawn();
        }
        self.close(ctx);
    }
}

/// A path as the user gave it: trimmed, and with the pair of quotes Explorer's
/// "Copy as path" adds stripped — pasting is how this box gets filled.
fn clean_dir(raw: &str) -> PathBuf {
    PathBuf::from(raw.trim().trim_matches('"'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centred_window_is_fully_on_screen() {
        let monitor = egui::vec2(1920.0, 1080.0);
        let want = egui::vec2(620.0, 460.0);
        let (size, pos) = SetupApp::centred_on_monitor(monitor, want);
        assert_eq!(size, want);
        assert!(pos.x >= 0.0 && pos.y >= 0.0, "off the top-left: {pos:?}");
        assert!(pos.x + size.x <= monitor.x, "off the right edge: {pos:?}");
        // The bottom band of a 1080p screen is the taskbar's; the window must
        // not be tucked under it.
        assert!(
            pos.y + size.y <= monitor.y - TASKBAR_RESERVE,
            "under the taskbar: {pos:?}"
        );
    }

    #[test]
    fn a_window_taller_than_the_work_area_is_shrunk_to_fit() {
        let monitor = egui::vec2(1280.0, 720.0);
        let want = egui::vec2(620.0, 2000.0);
        let (size, pos) = SetupApp::centred_on_monitor(monitor, want);
        assert!(size.y <= monitor.y - TASKBAR_RESERVE);
        assert!(pos.y >= 0.0, "off the top of the screen: {pos:?}");
    }

    #[test]
    fn a_misreported_monitor_cannot_produce_a_negative_size() {
        let (size, pos) =
            SetupApp::centred_on_monitor(egui::vec2(0.0, 0.0), egui::vec2(620.0, 460.0));
        assert!(size.x >= 1.0 && size.y >= 1.0, "degenerate size: {size:?}");
        assert!(pos.x.is_finite() && pos.y.is_finite(), "bad position: {pos:?}");
    }

    /// Explorer's "Copy as path" wraps the path in quotes, and pasting that is
    /// exactly how this box gets filled.
    #[test]
    fn a_pasted_path_loses_its_quotes_and_its_whitespace() {
        assert_eq!(
            clean_dir("  \"C:\\Program Files\\Snor\" "),
            PathBuf::from("C:\\Program Files\\Snor")
        );
        assert_eq!(clean_dir("C:\\Snor"), PathBuf::from("C:\\Snor"));
    }
}