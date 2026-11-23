//! Asset library: built-in examples plus the user's own saved models, with thumbnails.
//!
//! - Built-in assets are the `examples` folder shipped with AItelier.
//! - The user's library is `Documents/AItelier/Library` (outside the program folder,
//!   so updates never touch it). Every asset is a graph `.json` file.
//! - Thumbnails are rendered on demand and cached in `Library/.thumbnails`.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use aitelier_graph::{Cooker, Graph, Registry};
use aitelier_render::{Camera, RenderSettings};
use egui::{Color32, RichText, Sense, Vec2};

use crate::app::App;
use crate::edit;

const THUMB: u32 = 192;

#[derive(Clone)]
pub struct Asset {
    pub name: String,
    pub path: PathBuf,
    pub builtin: bool,
    modified: SystemTime,
}

enum Thumb {
    Ready(egui::TextureHandle),
    Failed,
}

pub struct Library {
    pub assets: Vec<Asset>,
    pub user_dir: PathBuf,
    builtin_dir: Option<PathBuf>,
    thumbs: HashMap<PathBuf, Thumb>,
    pub search: String,
    pub selected: Option<PathBuf>,
    pub save_name: String,
    last_scan: f64,
    confirm_delete: Option<PathBuf>,
    pub confirm_open: Option<PathBuf>,
}

/// `Documents/AItelier/Library` in the user's home folder.
fn user_library_dir() -> PathBuf {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join("Documents").join("AItelier").join("Library")
}

/// The `examples` folder: next to the program (searched upwards), or where it was built.
fn builtin_dir() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        for dir in exe.ancestors().skip(1).take(5) {
            let candidate = dir.join("examples");
            // must hold graph files (cargo also makes an unrelated target/*/examples folder)
            if candidate.join("vase.json").is_file() {
                return Some(candidate);
            }
        }
    }
    let built = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples");
    built.join("vase.json").is_file().then_some(built)
}

fn scan_dir(dir: &Path, builtin: bool) -> Vec<Asset> {
    let mut out: Vec<Asset> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| x.eq_ignore_ascii_case("json"))
                != Some(true)
            {
                return None;
            }
            let name = path.file_stem()?.to_string_lossy().to_string();
            let modified = e
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            Some(Asset {
                name,
                path,
                builtin,
                modified,
            })
        })
        .collect();
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

/// Make a name safe to use as a file name.
pub fn file_name_for(name: &str) -> String {
    let s: String = name
        .trim()
        .chars()
        .map(|c| {
            if "<>:\"/\\|?*".contains(c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    if s.is_empty() {
        "untitled".into()
    } else {
        s
    }
}

impl Library {
    pub fn new() -> Self {
        let user_dir = user_library_dir();
        let _ = std::fs::create_dir_all(user_dir.join(".thumbnails"));
        let mut lib = Self {
            assets: vec![],
            user_dir,
            builtin_dir: builtin_dir(),
            thumbs: HashMap::new(),
            search: String::new(),
            selected: None,
            save_name: String::new(),
            last_scan: f64::NEG_INFINITY,
            confirm_delete: None,
            confirm_open: None,
        };
        lib.rescan();
        lib
    }

    pub fn rescan(&mut self) {
        let mut assets = Vec::new();
        if let Some(dir) = &self.builtin_dir {
            assets.extend(scan_dir(dir, true));
        }
        assets.extend(scan_dir(&self.user_dir, false));
        // forget thumbnails of changed or removed files
        self.thumbs
            .retain(|p, _| assets.iter().any(|a| &a.path == p));
        self.assets = assets;
    }

    fn thumb_cache_path(&self, a: &Asset) -> PathBuf {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        a.path.hash(&mut h);
        a.modified.hash(&mut h);
        self.user_dir
            .join(".thumbnails")
            .join(format!("{:016x}.png", h.finish()))
    }

    /// Create one missing thumbnail (one per frame keeps the editor responsive).
    fn make_next_thumbnail(&mut self, ctx: &egui::Context, registry: &Arc<Registry>) {
        let Some(asset) = self
            .assets
            .iter()
            .find(|a| !self.thumbs.contains_key(&a.path))
            .cloned()
        else {
            return;
        };
        let cache = self.thumb_cache_path(&asset);
        let rgba = read_png(&cache).or_else(|| {
            let (rgba, w, h) = render_thumbnail(&asset.path, registry)?;
            let _ = write_png(&cache, &rgba, w, h);
            Some((rgba, w, h))
        });
        let thumb = match rgba {
            Some((rgba, w, h)) => Thumb::Ready(ctx.load_texture(
                asset.path.to_string_lossy(),
                egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba),
                egui::TextureOptions::LINEAR,
            )),
            None => Thumb::Failed,
        };
        self.thumbs.insert(asset.path.clone(), thumb);
        ctx.request_repaint();
    }

    fn forget_thumbnail(&mut self, path: &Path) {
        self.thumbs.remove(path);
    }
}

/// Render a graph file's output at its first frame.
fn render_thumbnail(path: &Path, registry: &Arc<Registry>) -> Option<(Vec<u8>, u32, u32)> {
    let graph = Graph::from_json(&std::fs::read_to_string(path).ok()?).ok()?;
    render_graph_thumbnail(&graph, registry)
}

pub fn render_graph_thumbnail(
    graph: &Graph,
    registry: &Arc<Registry>,
) -> Option<(Vec<u8>, u32, u32)> {
    let start = graph.timeline.start as f64;
    let mut cooker = Cooker::new(Arc::new(graph.clone()), registry.clone()).ok()?;
    let geo = cooker.cook_output(start).ok()?;
    let mut cam = Camera {
        yaw: 35.0,
        pitch: 22.0,
        ..Default::default()
    };
    let (lo, hi) = geo.bounds();
    cam.frame_bounds(lo, hi);
    let settings = RenderSettings {
        width: THUMB,
        height: THUMB,
        supersample: 2,
        grid: false,
        ..Default::default()
    };
    let img = aitelier_render::render(&geo, &cam, &settings);
    Some((img.rgba, img.width, img.height))
}

fn read_png(path: &Path) -> Option<(Vec<u8>, u32, u32)> {
    let file = std::fs::File::open(path).ok()?;
    let mut reader = png::Decoder::new(std::io::BufReader::new(file))
        .read_info()
        .ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    (info.color_type == png::ColorType::Rgba).then(|| {
        buf.truncate(info.buffer_size());
        (buf, info.width, info.height)
    })
}

pub fn write_png(path: &Path, rgba: &[u8], w: u32, h: u32) -> std::io::Result<()> {
    let file = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().map_err(std::io::Error::other)?;
    wr.write_image_data(rgba).map_err(std::io::Error::other)
}

impl App {
    fn library_open(&mut self, asset: &Asset) {
        let text = match std::fs::read_to_string(&asset.path) {
            Ok(t) => t,
            Err(e) => {
                self.status = format!("Could not read {}: {e}", asset.path.display());
                return;
            }
        };
        match Graph::from_json(&text) {
            Ok(g) => {
                // Built-in examples open as untitled copies so the originals stay intact.
                let path = if asset.builtin {
                    None
                } else {
                    Some(asset.path.clone())
                };
                self.set_document(g, path);
                self.status = if asset.builtin {
                    format!(
                        "Opened example \"{}\" (Save As to keep your own copy)",
                        asset.name
                    )
                } else {
                    format!("Opened \"{}\" from your library", asset.name)
                };
            }
            Err(e) => self.status = format!("\"{}\" is not a valid graph: {e}", asset.name),
        }
    }

    fn library_insert(&mut self, asset: &Asset) {
        let graph = match std::fs::read_to_string(&asset.path)
            .map_err(|e| e.to_string())
            .and_then(|t| Graph::from_json(&t).map_err(|e| e.to_string()))
        {
            Ok(g) => g,
            Err(e) => {
                self.status = format!("Could not insert \"{}\": {e}", asset.name);
                return;
            }
        };
        let previous_output = self
            .graph
            .nodes
            .contains_key(&self.graph.output)
            .then(|| self.graph.output.clone());
        let scene_bounds = self
            .result
            .as_ref()
            .filter(|g| g.num_points() > 0)
            .map(|g| g.bounds());
        let asset_bounds = Cooker::new(Arc::new(graph.clone()), self.registry.clone())
            .ok()
            .and_then(|mut c| c.cook_output(graph.timeline.start as f64).ok())
            .filter(|g| g.num_points() > 0)
            .map(|g| g.bounds());
        match edit::insert_graph(&mut self.graph, &graph, &asset.name, &self.registry) {
            Some(raw_out) => {
                // A transform after the inserted model, so it can be placed; start it beside the scene.
                let pos = self.graph.nodes[&raw_out]
                    .ui
                    .map(|u| [u.pos[0], u.pos[1] + 100.0])
                    .unwrap_or([0.0, 0.0]);
                let out = edit::add_node(&mut self.graph, "transform", pos);
                let out = if edit::rename_node(
                    &mut self.graph,
                    &out,
                    &format!("{}_place", edit::slug(&asset.name)),
                ) {
                    format!("{}_place", edit::slug(&asset.name))
                } else {
                    out
                };
                let offset = match (scene_bounds, asset_bounds) {
                    (Some((slo, shi)), Some((alo, ahi))) => {
                        let gap = ((shi - slo).max_element()).max(1.0) * 0.1;
                        [
                            shi.x + gap - alo.x,
                            0.0,
                            (slo.z + shi.z) * 0.5 - (alo.z + ahi.z) * 0.5,
                        ]
                    }
                    _ => [0.0, 0.0, 0.0],
                };
                if let Some(t) = self.graph.nodes.get_mut(&out) {
                    t.inputs = vec![raw_out.clone()];
                    let round = |x: f32| ((x * 100.0).round() / 100.0) as f64;
                    t.params.insert(
                        "translate".into(),
                        aitelier_graph::Value::List(
                            offset
                                .iter()
                                .map(|x| aitelier_graph::Value::Num(round(*x)))
                                .collect(),
                        ),
                    );
                }
                // Combine with what was shown before, so the inserted model appears in the scene.
                match previous_output {
                    Some(prev) if self.graph.nodes[&prev].kind == "merge" => {
                        self.graph
                            .nodes
                            .get_mut(&prev)
                            .expect("exists")
                            .inputs
                            .push(out.clone());
                        self.status = format!("Inserted \"{}\" and added it to {prev}", asset.name);
                    }
                    Some(prev) => {
                        let pos = self.graph.nodes[&prev]
                            .ui
                            .map(|u| [u.pos[0], u.pos[1] + 120.0])
                            .unwrap_or([0.0, 0.0]);
                        let merge = edit::add_node(&mut self.graph, "merge", pos);
                        if let Some(m) = self.graph.nodes.get_mut(&merge) {
                            m.inputs = vec![prev.clone(), out.clone()];
                        }
                        self.graph.output = merge.clone();
                        self.status = format!(
                            "Inserted \"{}\": merged with {prev} in the new node {merge}",
                            asset.name
                        );
                    }
                    None => {
                        self.graph.output = out.clone();
                        self.status = format!("Inserted \"{}\"", asset.name);
                    }
                }
                self.status
                    .push_str(&format!(". Move it with the translate parameter of {out}."));
                self.selected = Some(out);
                self.editor.frame_on_next_draw = true;
                self.frame_after_cook = true;
            }
            None => self.status = format!("\"{}\" has no output node", asset.name),
        }
    }

    fn library_save_current(&mut self) {
        let name = self.library.save_name.trim().to_string();
        if name.is_empty() {
            return;
        }
        let path = self
            .library
            .user_dir
            .join(format!("{}.json", file_name_for(&name)));
        match std::fs::write(&path, self.graph.to_json()) {
            Ok(()) => {
                self.status = format!("Saved \"{name}\" to your library");
                self.library.save_name.clear();
                self.library.forget_thumbnail(&path);
                self.library.rescan();
                self.library.selected = Some(path);
            }
            Err(e) => self.status = format!("Could not save to the library: {e}"),
        }
    }

    pub fn library_ui(&mut self, ui: &mut egui::Ui) {
        let now = ui.input(|i| i.time);
        if now - self.library.last_scan > 3.0 {
            self.library.rescan();
            self.library.last_scan = now;
        }
        let ctx = ui.ctx().clone();
        let registry = self.registry.clone();
        self.library.make_next_thumbnail(&ctx, &registry);

        // ---- save current graph
        ui.add_space(4.0);
        ui.label(RichText::new("Save the current graph to My library").strong());
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.library.save_name)
                    .hint_text("name, e.g. dining chair")
                    .desired_width(180.0),
            );
            let exists = !self.library.save_name.trim().is_empty()
                && self
                    .library
                    .user_dir
                    .join(format!("{}.json", file_name_for(&self.library.save_name)))
                    .exists();
            let label = if exists { "Overwrite" } else { "Save" };
            if ui
                .add_enabled(
                    !self.library.save_name.trim().is_empty(),
                    egui::Button::new(label),
                )
                .clicked()
            {
                self.library_save_current();
            }
        });
        ui.separator();

        // ---- search and folder
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.library.search)
                    .hint_text("search")
                    .desired_width(150.0),
            );
            if ui
                .button("Open folder")
                .on_hover_text(self.library.user_dir.display().to_string())
                .clicked()
            {
                open_in_file_manager(&self.library.user_dir);
            }
        });
        ui.add_space(4.0);

        // ---- selected asset actions
        let selected = self
            .library
            .selected
            .clone()
            .and_then(|p| self.library.assets.iter().find(|a| a.path == p).cloned());
        if let Some(a) = &selected {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label(RichText::new(&a.name).strong());
                ui.label(RichText::new(if a.builtin { "Built-in example" } else { "My library" }).small().color(Color32::GRAY));
                ui.horizontal(|ui| {
                    let confirm = self.library.confirm_open.as_ref() == Some(&a.path);
                    let open_label = if self.is_dirty() && !confirm { "Open" } else if confirm { "Open (discard changes)" } else { "Open" };
                    if ui.button(open_label).on_hover_text("Replace the current graph with this one").clicked() {
                        if self.is_dirty() && !confirm {
                            self.library.confirm_open = Some(a.path.clone());
                            self.status = "The current graph has unsaved changes. Click Open again to discard them, or save first.".into();
                        } else {
                            self.library.confirm_open = None;
                            self.library_open(a);
                        }
                    }
                    if ui.button("Insert into scene").on_hover_text("Add this model's nodes to the current graph").clicked() {
                        self.library_insert(a);
                    }
                    if !a.builtin {
                        let armed = self.library.confirm_delete.as_ref() == Some(&a.path);
                        let del = egui::Button::new(RichText::new(if armed { "Really delete?" } else { "Delete" }).color(Color32::from_rgb(235, 110, 100)));
                        if ui.add(del).clicked() {
                            if armed {
                                let _ = std::fs::remove_file(&a.path);
                                self.status = format!("Deleted \"{}\" from your library", a.name);
                                self.library.selected = None;
                                self.library.confirm_delete = None;
                                self.library.rescan();
                            } else {
                                self.library.confirm_delete = Some(a.path.clone());
                            }
                        }
                    }
                });
            });
            ui.add_space(6.0);
        }

        // ---- cards
        let search = self.library.search.to_lowercase();
        let mut clicked: Option<PathBuf> = None;
        let mut opened: Option<Asset> = None;
        for (title, builtin) in [("My library", false), ("Built-in examples", true)] {
            let list: Vec<Asset> = self
                .library
                .assets
                .iter()
                .filter(|a| a.builtin == builtin && a.name.to_lowercase().contains(&search))
                .cloned()
                .collect();
            ui.label(RichText::new(format!("{title} ({})", list.len())).strong());
            if list.is_empty() {
                let hint = if builtin {
                    "No examples found."
                } else {
                    "Empty. Save a graph above to add it here."
                };
                ui.label(RichText::new(hint).small().color(Color32::GRAY));
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                for a in &list {
                    let sel = self.library.selected.as_ref() == Some(&a.path);
                    let (rect, resp) =
                        ui.allocate_exact_size(Vec2::new(104.0, 124.0), Sense::click());
                    let painter = ui.painter_at(rect);
                    let bg = if sel {
                        Color32::from_rgb(70, 95, 140)
                    } else if resp.hovered() {
                        Color32::from_gray(55)
                    } else {
                        Color32::from_gray(40)
                    };
                    painter.rect_filled(rect, 5.0, bg);
                    let img_rect = egui::Rect::from_min_size(
                        rect.min + Vec2::new(4.0, 4.0),
                        Vec2::splat(96.0),
                    );
                    match self.library.thumbs.get(&a.path) {
                        Some(Thumb::Ready(t)) => {
                            painter.image(
                                t.id(),
                                img_rect,
                                egui::Rect::from_min_max(
                                    egui::pos2(0.0, 0.0),
                                    egui::pos2(1.0, 1.0),
                                ),
                                Color32::WHITE,
                            );
                        }
                        Some(Thumb::Failed) => {
                            painter.text(
                                img_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "⚠",
                                egui::FontId::proportional(24.0),
                                Color32::from_rgb(235, 110, 100),
                            );
                        }
                        None => {
                            painter.text(
                                img_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "…",
                                egui::FontId::proportional(20.0),
                                Color32::GRAY,
                            );
                        }
                    }
                    let mut name = a.name.clone();
                    if name.chars().count() > 15 {
                        name = name.chars().take(14).collect::<String>() + "…";
                    }
                    painter.text(
                        egui::pos2(rect.center().x, rect.bottom() - 12.0),
                        egui::Align2::CENTER_CENTER,
                        name,
                        egui::FontId::proportional(12.0),
                        Color32::from_gray(225),
                    );
                    let resp =
                        resp.on_hover_text(format!("{}\nDouble-click to open", a.path.display()));
                    if resp.clicked() {
                        clicked = Some(a.path.clone());
                    }
                    if resp.double_clicked() {
                        opened = Some(a.clone());
                    }
                }
            });
            ui.add_space(10.0);
        }
        if let Some(p) = clicked {
            if self.library.selected.as_ref() != Some(&p) {
                self.library.confirm_delete = None;
                self.library.confirm_open = None;
            }
            self.library.selected = Some(p);
        }
        if let Some(a) = opened {
            if self.is_dirty() {
                self.library.selected = Some(a.path.clone());
                self.library.confirm_open = Some(a.path.clone());
                self.status = "The current graph has unsaved changes. Click Open again to discard them, or save first.".into();
            } else {
                self.library_open(&a);
            }
        }
    }
}

fn open_in_file_manager(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer").arg(dir).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(dir).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_file_names() {
        assert_eq!(file_name_for("dining chair"), "dining chair");
        assert_eq!(file_name_for("a/b:c?"), "a_b_c_");
        assert_eq!(file_name_for("   "), "untitled");
    }

    #[test]
    fn builtin_examples_found_and_render() {
        let dir = builtin_dir().expect("examples folder");
        let assets = scan_dir(&dir, true);
        assert!(assets.iter().any(|a| a.name == "vase"));
        let reg = Arc::new(Registry::with_builtins());
        let (rgba, w, h) = render_thumbnail(&dir.join("vase.json"), &reg).expect("thumbnail");
        assert_eq!((w, h), (THUMB, THUMB));
        assert_eq!(rgba.len(), (THUMB * THUMB * 4) as usize);
    }
}
