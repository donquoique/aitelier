//! 3D viewport: shows the cooked geometry with the CPU preview renderer.

use aitelier_geo::Geometry;
use aitelier_render::{Camera, RenderSettings};
use egui::{Color32, PointerButton, Sense};

use crate::app::App;

pub struct ViewportState {
    pub camera: Camera,
    pub wireframe: bool,
    pub grid: bool,
    /// Re-render needed (geometry or settings changed).
    pub dirty: bool,
    /// Frame the camera on the next successful result (after opening a file).
    pub frame_on_next_result: bool,
    texture: Option<egui::TextureHandle>,
    last: Option<(Camera, [usize; 2], bool, bool)>,
}

impl Default for ViewportState {
    fn default() -> Self {
        Self {
            camera: Camera::default(),
            wireframe: false,
            grid: true,
            dirty: true,
            frame_on_next_result: true,
            texture: None,
            last: None,
        }
    }
}

impl ViewportState {
    pub fn settings(&self) -> RenderSettings {
        RenderSettings {
            wireframe: self.wireframe,
            grid: self.grid,
            ..Default::default()
        }
    }

    pub fn frame_all(&mut self, geo: Option<&Geometry>) {
        if let Some(g) = geo.filter(|g| g.num_points() > 0) {
            let (lo, hi) = g.bounds();
            self.camera.frame_bounds(lo, hi);
        }
    }
}

impl App {
    pub fn viewport_ui(&mut self, ui: &mut egui::Ui) {
        let rect = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let vp = &mut self.viewport;

        if vp.frame_on_next_result {
            if let Some(g) = self.result.as_deref() {
                vp.frame_all(Some(g));
                vp.frame_on_next_result = false;
            }
        }

        // --- navigation
        let shift = ui.input(|i| i.modifiers.shift);
        if resp.dragged_by(PointerButton::Primary) && !shift {
            let d = resp.drag_delta();
            vp.camera.orbit(d.x, d.y);
        }
        if resp.dragged_by(PointerButton::Secondary)
            || resp.dragged_by(PointerButton::Middle)
            || (resp.dragged_by(PointerButton::Primary) && shift)
        {
            let d = resp.drag_delta();
            vp.camera.pan(d.x, d.y, rect.height());
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                vp.camera.zoom(scroll / 40.0);
            }
        }

        // --- render when something changed
        let ppp = ui.pixels_per_point();
        let size = [
            ((rect.width() * ppp) as usize).max(1),
            ((rect.height() * ppp) as usize).max(1),
        ];
        let state = (vp.camera, size, vp.wireframe, vp.grid);
        if vp.dirty || vp.last != Some(state) || vp.texture.is_none() {
            let settings = RenderSettings {
                width: size[0] as u32,
                height: size[1] as u32,
                ..vp.settings()
            };
            let empty = Geometry::new();
            let img = aitelier_render::render(
                self.result.as_deref().unwrap_or(&empty),
                &vp.camera,
                &settings,
            );
            let color = egui::ColorImage::from_rgba_unmultiplied(size, &img.rgba);
            match vp.texture.as_mut() {
                Some(t) => t.set(color, egui::TextureOptions::LINEAR),
                None => {
                    vp.texture = Some(ui.ctx().load_texture(
                        "viewport",
                        color,
                        egui::TextureOptions::LINEAR,
                    ))
                }
            }
            vp.dirty = false;
            vp.last = Some(state);
        }
        if let Some(t) = &vp.texture {
            ui.painter().image(
                t.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }

        // --- overlay text
        let painter = ui.painter_at(rect);
        let font = egui::FontId::proportional(12.5);
        let mut y = rect.top() + 8.0;
        let mut line = |text: String, color: Color32| {
            painter.text(
                egui::pos2(rect.left() + 10.0, y),
                egui::Align2::LEFT_TOP,
                text,
                font.clone(),
                color,
            );
            y += 17.0;
        };
        let shown = self.display_node();
        line(
            format!(
                "{}  ·  frame {}",
                if shown.is_empty() {
                    "(no output)".into()
                } else {
                    shown
                },
                self.frame
            ),
            Color32::from_gray(210),
        );
        match (&self.result, &self.error) {
            (_, Some(e)) => line(e.message.clone(), Color32::from_rgb(240, 110, 100)),
            (Some(g), None) => {
                let topo = g.topology();
                line(
                    format!("{} points  ·  {} primitives", g.num_points(), g.num_prims()),
                    Color32::from_gray(160),
                );
                if g.prims
                    .iter()
                    .any(|p| p.kind == aitelier_geo::PrimKind::Polygon)
                {
                    if topo.is_watertight() {
                        line(
                            format!("watertight  ·  volume {:.3}", g.signed_volume()),
                            Color32::from_rgb(120, 200, 140),
                        );
                    } else {
                        line(
                            format!("open: {} border edges", topo.boundary_edges),
                            Color32::from_gray(140),
                        );
                    }
                }
            }
            (None, None) => line("nothing to show".into(), Color32::from_gray(140)),
        }
    }
}
