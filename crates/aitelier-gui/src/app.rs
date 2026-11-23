//! Application state, menus, file handling, undo, cooking and layout.

use std::path::PathBuf;
use std::sync::Arc;

use aitelier_geo::Geometry;
use aitelier_graph::{Cooker, Evaluator, Graph, Registry};
use egui::{Key, KeyboardShortcut, Modifiers};
use indexmap::IndexMap;

use crate::edit;
use crate::node_editor::EditorView;
use crate::viewport::ViewportState;

const UNDO_LIMIT: usize = 200;

/// The last cook problem, and the node it belongs to.
pub struct CookError {
    pub node: Option<String>,
    pub message: String,
}

pub struct App {
    pub registry: Arc<Registry>,
    pub eval: Evaluator,

    // document
    pub graph: Graph,
    pub path: Option<PathBuf>,
    saved: Graph,
    committed: Graph,
    undo: Vec<Graph>,
    redo: Vec<Graph>,

    // selection and display
    pub selected: Option<String>,
    /// Show the selected node in the viewport instead of the graph output.
    pub display_selected: bool,

    // time
    pub frame: f64,
    pub playing: bool,
    play_clock: f64,

    // cooking
    pub(crate) cooker: Option<Cooker>,
    cooker_key: Option<Graph>,
    pub result: Option<Arc<Geometry>>,
    result_key: Option<(String, i64)>,
    pub error: Option<CookError>,
    /// Graph parameters evaluated at the current frame (for showing live values).
    pub vars: IndexMap<String, f64>,

    // panels
    pub editor: EditorView,
    pub viewport: ViewportState,
    pub status: String,
    title: String,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = egui::Color32::from_gray(30);
        cc.egui_ctx.set_theme(egui::Theme::Dark); // ignore the Windows light/dark setting
        cc.egui_ctx.set_visuals_of(egui::Theme::Dark, visuals);

        let graph = edit::starter_graph();
        let mut app = Self {
            registry: Arc::new(Registry::with_builtins()),
            eval: Evaluator::new(),
            saved: graph.clone(),
            committed: graph.clone(),
            graph,
            path: None,
            undo: vec![],
            redo: vec![],
            selected: None,
            display_selected: false,
            frame: 1.0,
            playing: false,
            play_clock: 0.0,
            cooker: None,
            cooker_key: None,
            result: None,
            result_key: None,
            error: None,
            vars: IndexMap::new(),
            editor: EditorView::default(),
            viewport: ViewportState::default(),
            status: "Right-click in the node editor to add nodes.".into(),
            title: String::new(),
        };
        if let Some(p) = path {
            app.open_path(p);
        }
        app
    }

    // ------------------------------------------------------------------ files

    fn set_document(&mut self, graph: Graph, path: Option<PathBuf>) {
        let mut graph = graph;
        edit::auto_layout(&mut graph);
        self.saved = graph.clone();
        self.committed = graph.clone();
        self.graph = graph;
        self.path = path;
        self.undo.clear();
        self.redo.clear();
        self.selected = None;
        self.frame = self.graph.timeline.start as f64;
        self.playing = false;
        self.cooker = None;
        self.cooker_key = None;
        self.result_key = None;
        self.viewport.frame_on_next_result = true;
        self.editor.frame_on_next_draw = true;
    }

    pub fn open_path(&mut self, path: PathBuf) {
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| Graph::from_json(&t).map_err(|e| e.to_string()))
        {
            Ok(g) => {
                self.status = format!("Opened {}", path.display());
                self.set_document(g, Some(path));
            }
            Err(e) => self.status = format!("Could not open {}: {e}", path.display()),
        }
    }

    fn open_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("AItelier graph", &["json"])
            .pick_file()
        {
            self.open_path(p);
        }
    }

    fn save(&mut self, save_as: bool) {
        let path = match (&self.path, save_as) {
            (Some(p), false) => Some(p.clone()),
            _ => rfd::FileDialog::new()
                .add_filter("AItelier graph", &["json"])
                .set_file_name("graph.json")
                .save_file(),
        };
        let Some(path) = path else { return };
        match std::fs::write(&path, self.graph.to_json()) {
            Ok(()) => {
                self.saved = self.graph.clone();
                self.status = format!("Saved {}", path.display());
                self.path = Some(path);
            }
            Err(e) => self.status = format!("Could not save: {e}"),
        }
    }

    fn export_obj(&mut self) {
        let Some(geo) = self.result.clone() else {
            self.status = "Nothing to export: the graph has no result.".into();
            return;
        };
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Wavefront OBJ", &["obj"])
            .set_file_name("model.obj")
            .save_file()
        {
            self.status = match std::fs::write(&p, aitelier_geo::obj::to_obj_string(&geo)) {
                Ok(()) => format!("Exported frame {} to {}", self.frame, p.display()),
                Err(e) => format!("Export failed: {e}"),
            };
        }
    }

    fn render_png(&mut self) {
        let Some(geo) = self.result.clone() else {
            self.status = "Nothing to render: the graph has no result.".into();
            return;
        };
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("PNG image", &["png"])
            .set_file_name("render.png")
            .save_file()
        {
            let settings = aitelier_render::RenderSettings {
                width: 1920,
                height: 1080,
                supersample: 2,
                ..self.viewport.settings()
            };
            let img = aitelier_render::render(&geo, &self.viewport.camera, &settings);
            self.status = match img.write_png(&p) {
                Ok(()) => format!("Rendered {}", p.display()),
                Err(e) => format!("Render failed: {e}"),
            };
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.graph != self.saved
    }

    // ------------------------------------------------------------------ undo

    fn commit_if_changed(&mut self, ctx: &egui::Context) {
        let busy = ctx.input(|i| i.pointer.any_down()) || ctx.egui_wants_keyboard_input();
        if !busy && self.graph != self.committed {
            self.undo
                .push(std::mem::replace(&mut self.committed, self.graph.clone()));
            if self.undo.len() > UNDO_LIMIT {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }

    fn undo(&mut self) {
        if self.graph != self.committed {
            self.undo.push(self.committed.clone());
        }
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.graph, prev));
            self.committed = self.graph.clone();
            self.fix_selection();
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.graph, next));
            self.committed = self.graph.clone();
            self.fix_selection();
        }
    }

    fn fix_selection(&mut self) {
        if self
            .selected
            .as_ref()
            .is_some_and(|s| !self.graph.nodes.contains_key(s))
        {
            self.selected = None;
        }
    }

    pub fn delete_selected(&mut self) {
        if let Some(n) = self.selected.take() {
            edit::delete_node(&mut self.graph, &n);
            self.status = format!("Deleted {n}");
        }
    }

    // ------------------------------------------------------------------ cooking

    /// Node currently shown in the viewport.
    pub fn display_node(&self) -> String {
        match (&self.selected, self.display_selected) {
            (Some(s), true) => s.clone(),
            _ => self.graph.output.clone(),
        }
    }

    fn update_cook(&mut self) {
        self.vars = self
            .eval
            .parameters_at(&self.graph, self.frame)
            .unwrap_or_default();

        let key = edit::cook_key(&self.graph);
        if self.cooker_key.as_ref() != Some(&key) {
            self.cooker_key = Some(key);
            self.result_key = None;
            match Cooker::new(Arc::new(self.graph.clone()), self.registry.clone()) {
                Ok(c) => {
                    self.cooker = Some(c);
                    self.error = None;
                }
                Err(e) => {
                    self.cooker = None;
                    self.result = None;
                    self.error = Some(CookError {
                        node: e.node().map(str::to_string),
                        message: e.to_string(),
                    });
                    self.viewport.dirty = true;
                }
            }
        }
        let target = self.display_node();
        let Some(cooker) = self.cooker.as_mut() else {
            return;
        };
        let rkey = (target.clone(), (self.frame * 1000.0).round() as i64);
        if self.result_key.as_ref() == Some(&rkey) {
            return;
        }
        self.result_key = Some(rkey);
        if !self.graph.nodes.contains_key(&target) {
            self.result = None;
            self.error = None;
            self.viewport.dirty = true;
            return;
        }
        match cooker.cook(&target, self.frame) {
            Ok(g) => {
                self.result = Some(g);
                self.error = None;
            }
            Err(e) => {
                self.result = None;
                self.error = Some(CookError {
                    node: e.node().map(str::to_string),
                    message: e.to_string(),
                });
            }
        }
        self.viewport.dirty = true;
    }

    // ------------------------------------------------------------------ time

    pub fn set_frame(&mut self, f: f64) {
        let tl = self.graph.timeline;
        self.frame = f
            .round()
            .clamp(tl.start as f64, tl.end.max(tl.start) as f64);
    }

    fn advance_playback(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if !self.playing {
            self.play_clock = now;
            return;
        }
        let tl = self.graph.timeline;
        let step = 1.0 / tl.fps.max(1.0) as f64;
        while now - self.play_clock >= step {
            self.play_clock += step;
            let next = self.frame + 1.0;
            self.frame = if next > tl.end as f64 {
                tl.start as f64
            } else {
                next
            };
        }
        ctx.request_repaint();
    }

    // ------------------------------------------------------------------ shortcuts and menus

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let cmd = Modifiers::COMMAND;
        let sc = |m: Modifiers, k: Key| KeyboardShortcut::new(m, k);
        if ctx.input_mut(|i| i.consume_shortcut(&sc(cmd | Modifiers::SHIFT, Key::Z)))
            || ctx.input_mut(|i| i.consume_shortcut(&sc(cmd, Key::Y)))
        {
            self.redo();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&sc(cmd, Key::Z))) {
            self.undo();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&sc(cmd | Modifiers::SHIFT, Key::S))) {
            self.save(true);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&sc(cmd, Key::S))) {
            self.save(false);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&sc(cmd, Key::O))) {
            self.open_dialog();
        }
        if ctx.egui_wants_keyboard_input() {
            return; // typing in a field
        }
        if ctx.input(|i| i.key_pressed(Key::Space)) {
            self.playing = !self.playing;
        }
        if ctx.input(|i| i.key_pressed(Key::ArrowRight)) {
            self.set_frame(self.frame + 1.0);
        }
        if ctx.input(|i| i.key_pressed(Key::ArrowLeft)) {
            self.set_frame(self.frame - 1.0);
        }
        if ctx.input(|i| i.key_pressed(Key::Home)) {
            self.set_frame(self.graph.timeline.start as f64);
        }
        if ctx.input(|i| i.key_pressed(Key::F)) {
            self.viewport.frame_all(self.result.as_deref());
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New").clicked() {
                    self.set_document(edit::starter_graph(), None);
                    self.status = "New graph".into();
                }
                if ui.button("Open…    Ctrl+O").clicked() {
                    self.open_dialog();
                }
                if ui.button("Save    Ctrl+S").clicked() {
                    self.save(false);
                }
                if ui.button("Save As…    Ctrl+Shift+S").clicked() {
                    self.save(true);
                }
                ui.separator();
                if ui.button("Export OBJ (current frame)…").clicked() {
                    self.export_obj();
                }
                if ui.button("Render PNG (current view)…").clicked() {
                    self.render_png();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui
                    .add_enabled(
                        !self.undo.is_empty() || self.graph != self.committed,
                        egui::Button::new("Undo    Ctrl+Z"),
                    )
                    .clicked()
                {
                    self.undo();
                }
                if ui
                    .add_enabled(!self.redo.is_empty(), egui::Button::new("Redo    Ctrl+Y"))
                    .clicked()
                {
                    self.redo();
                }
                ui.separator();
                if ui
                    .add_enabled(
                        self.selected.is_some(),
                        egui::Button::new("Delete node    Del"),
                    )
                    .clicked()
                {
                    self.delete_selected();
                }
            });
            ui.menu_button("View", |ui| {
                if ui.button("Frame model    F").clicked() {
                    self.viewport.frame_all(self.result.as_deref());
                }
                if ui.button("Frame nodes    H (over node editor)").clicked() {
                    self.editor.frame_on_next_draw = true;
                }
                ui.checkbox(&mut self.viewport.wireframe, "Wireframe");
                ui.checkbox(&mut self.viewport.grid, "Ground grid");
                ui.checkbox(
                    &mut self.display_selected,
                    "Show selected node (instead of output)",
                );
            });
            ui.menu_button("Help", |ui| {
                ui.label(HELP);
            });
        });
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "untitled".into());
        let title = format!(
            "{}{} - AItelier",
            if self.is_dirty() { "*" } else { "" },
            name
        );
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }
}

const HELP: &str = "\
Node editor
  Right-click empty space: add a node
  Drag a node: move it.  Click: select.  Double-click: make it the output
  Drag from a node's bottom port to another node's top port: connect
  Drag a connected top port: disconnect / reconnect
  Middle-drag or drag empty space: pan.  Scroll: zoom.  H: frame all nodes
  Delete or X: delete the selected node

Parameters
  Drag numbers to change them; double-click to type
  Diamond button: set or remove a keyframe at the current frame
  f button: switch between a number and an expression (e.g. height * 0.4)
  Right-click a number for more options

Viewport
  Left-drag: orbit.  Right/middle-drag or Shift+drag: pan.  Scroll: zoom.  F: frame

Timeline
  Space: play/pause.  Left/Right arrows: step.  Home: go to start";

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.shortcuts(&ctx);
        self.advance_playback(&ctx);
        self.update_cook();

        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::bottom("status")
            .exact_size(24.0)
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    if let Some(e) = &self.error {
                        ui.colored_label(egui::Color32::from_rgb(240, 110, 100), &e.message);
                    } else {
                        ui.label(&self.status);
                    }
                });
            });
        egui::Panel::bottom("timeline")
            .exact_size(74.0)
            .show(ui, |ui| self.timeline_ui(ui));
        egui::Panel::right("params")
            .resizable(true)
            .default_size(360.0)
            .min_size(260.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.params_ui(ui));
            });
        egui::Panel::left("nodes")
            .resizable(true)
            .default_size(520.0)
            .min_size(240.0)
            .show(ui, |ui| self.node_editor_ui(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| self.viewport_ui(ui));

        self.commit_if_changed(&ctx);
        self.update_title(&ctx);
    }
}
