use std::{
    collections::HashMap,
    ops::Range,
    path::Path,
    sync::{
        atomic::{AtomicI32, Ordering},
        Arc, Mutex,
    },
    thread,
};

use eframe::egui;

use egui::{menu, CentralPanel, TopBottomPanel};
use fst::{
    fst::{Fst, ScopeId, VarId},
    valvec::ValAndTimeVec,
};

use hierarchy::{show_scopes_panel, show_vars_panel};

mod decoder;
mod hierarchy;
mod waves;

use anyhow::Result;
use waves::show_waves_widget;

fn main() {
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "wavery",
        native_options,
        Box::new(|cc| Box::new(MainApp::new(cc))),
    );
}

#[derive(Default)]
enum FileState {
    #[default]
    None,
    Loaded(Fst),
    Error(anyhow::Error),
    Loading(FstLoader),
}

struct FstLoader {
    // When the thread has finished loading it will put it here. If it's
    // still loading it will be None. If it is finished and there was an error
    // it will be Some(Err()).
    loaded_file: Arc<Mutex<Option<Result<Fst>>>>,

    // Progress amount.
    progress: Arc<AtomicI32>,

    // Set to true to cancel loading.
    cancelled: Arc<Mutex<bool>>,
}

impl FstLoader {
    fn new(filename: &Path, mut update_callback: Box<dyn FnMut() -> () + Send>) -> Self {
        let loaded_file = Arc::new(Mutex::new(None));
        let loaded_file_thread = loaded_file.clone();

        let cancelled = Arc::new(Mutex::new(false));
        let cancelled_thread = cancelled.clone();

        let progress = Arc::new(AtomicI32::new(0));
        let progress_thread = progress.clone();

        let filename = filename.to_owned();

        // Start a new thread.
        thread::spawn(move || {
            let mut cancel_progress_callback = |p: i32| {
                progress_thread.store(p, Ordering::SeqCst);
                update_callback();
                *cancelled_thread.lock().unwrap()
            };
            let fst = Fst::load(&filename);
            *loaded_file_thread.lock().unwrap() = Some(fst);
            cancel_progress_callback(100);
        });

        Self {
            loaded_file,
            cancelled,
            progress,
        }
    }

    fn progress(&self) -> i32 {
        self.progress.load(Ordering::SeqCst)
    }

    fn cancel(&mut self) {
        *self.cancelled.lock().unwrap() = true;
    }

    /// Return None if the file hasn't finished being loaded, otherwise return
    /// the result of loading the file. I.e. Some(Err()) if it failed, Some(Ok())
    /// if it succeeded, and None if it hasn't finished.
    fn take(&mut self) -> Option<Result<Fst>> {
        self.loaded_file.lock().unwrap().take()
    }
}

#[derive(Default)]
struct MainApp {
    // The file (or in-progress loading of said file).
    file: FileState,
    // Waves that we have loaded.
    cached_waves: HashMap<VarId, ValAndTimeVec>,
    // backend_panel: BackendPanel,
    selected_scope: Option<ScopeId>,
    /// The filter for the vars panel.
    vars_filter: String,
    // Bit of a hack, but if this is Some(foo) then foo was passed on the
    // command line and we should load that.
    pending_file_load: Option<String>,
    // Currently shown time span in the waves view.
    timespan: Range<f64>,
}

impl MainApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // Customize egui here with cc.egui_ctx.set_fonts and cc.egui_ctx.set_visuals.
        // Restore app state using cc.storage (requires the "persistence" feature).
        // Use the cc.gl (a glow::Context) to create graphics shaders and buffers that you can use
        // for e.g. egui::PaintCallback.
        let mut app = Self::default();
        // Load files from command line.
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args.len() == 1 {
            app.pending_file_load = Some(args[0].clone());
        }
        app
    }

    fn load_file(&mut self, path: &Path, ctx: &egui::Context) {
        let ctx2 = ctx.clone();
        let update = Box::new(move || {
            ctx2.request_repaint();
        });

        self.file = FileState::Loading(FstLoader::new(path, update));
    }
}

impl eframe::App for MainApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Handle pending command line arguments.
        let pending_file_load = self.pending_file_load.take();
        if let Some(pending_file_load) = pending_file_load {
            self.load_file(Path::new(&pending_file_load), ctx);
            frame.set_window_title(&format!("Wavery - {}", pending_file_load));
        }

        // Check if loading has completed.
        let new_file = match &mut self.file {
            FileState::Loading(loader) => {
                if loader.progress() >= 100 {
                    Some(match loader.take() {
                        Some(Ok(fst)) => FileState::Loaded(fst),
                        Some(Err(e)) => FileState::Error(e),
                        None => FileState::None,
                    })
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(new_file) = new_file {
            self.file = new_file;
            if let FileState::Loaded(fst) = &self.file {
                self.timespan = fst.header.start_time as f64..fst.header.end_time as f64;
            }
        }

        TopBottomPanel::top("menu").show(ctx, |ui| {
            menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open...").clicked() {
                        ui.close_menu();
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("FST", &["fst"])
                            .pick_file()
                        {
                            self.load_file(&path, ctx);
                            frame.set_window_title(&format!("Wavery - {}", path.display()));
                        }
                    }
                });
            });
        });
        match &mut self.file {
            FileState::None => {
                CentralPanel::default().show(ctx, |ui| {
                    ui.heading("No file loaded");
                });
            }
            FileState::Loaded(e) => {
                show_scopes_panel(ctx, e, &mut self.selected_scope);
                show_vars_panel(
                    ctx,
                    e,
                    &self.selected_scope,
                    &mut self.vars_filter,
                    &mut self.cached_waves,
                );
                CentralPanel::default().show(ctx, |ui| {
                    show_waves_widget(ui, e, &self.cached_waves, self.timespan.clone());
                });
            }
            FileState::Error(e) => {
                CentralPanel::default().show(ctx, |ui| {
                    ui.label(format!("Error loading file: {:?}", e));
                });
            }
            FileState::Loading(loader) => {
                CentralPanel::default().show(ctx, |ui| {
                    ui.label("Loading...");
                    ui.spinner();
                });
            }
        }
    }
}
