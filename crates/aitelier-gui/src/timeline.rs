//! Timeline: playback controls, frame range, and a track showing keyframes.

use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke};

use crate::app::App;
use crate::edit;

impl App {
    pub fn timeline_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(3.0);
        ui.horizontal(|ui| {
            let tl = self.graph.timeline;
            if ui.button("⏮").on_hover_text("Go to start (Home)").clicked() {
                self.set_frame(tl.start as f64);
            }
            if ui.button("◀").on_hover_text("Previous frame (←)").clicked() {
                self.set_frame(self.frame - 1.0);
            }
            if ui
                .button(if self.playing { "⏸" } else { "▶" })
                .on_hover_text("Play / pause (Space)")
                .clicked()
            {
                self.playing = !self.playing;
            }
            if ui.button("▶|").on_hover_text("Next frame (→)").clicked() {
                self.set_frame(self.frame + 1.0);
            }
            if ui.button("⏭").on_hover_text("Go to end").clicked() {
                self.set_frame(tl.end as f64);
            }
            ui.separator();
            ui.label("Frame");
            let mut f = self.frame;
            if ui
                .add(egui::DragValue::new(&mut f).speed(0.2).max_decimals(0))
                .changed()
            {
                self.set_frame(f);
            }
            ui.separator();
            let t = &mut self.graph.timeline;
            ui.label("Start");
            ui.add(
                egui::DragValue::new(&mut t.start)
                    .speed(0.2)
                    .max_decimals(0),
            );
            ui.label("End");
            ui.add(egui::DragValue::new(&mut t.end).speed(0.2).max_decimals(0));
            ui.label("FPS");
            ui.add(
                egui::DragValue::new(&mut t.fps)
                    .speed(0.2)
                    .range(1.0..=240.0)
                    .max_decimals(0),
            );
            t.start = t.start.round();
            t.end = t.end.round().max(t.start);
        });

        // --- track
        let tl = self.graph.timeline;
        let rect = ui.available_rect_before_wrap().shrink2(vec2(8.0, 4.0));
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let painter = ui.painter_at(rect.expand(2.0));
        painter.rect_filled(rect, 3.0, Color32::from_gray(24));
        let (s, e) = (tl.start, tl.end.max(tl.start + 1.0));
        let x_of = |f: f32| rect.left() + (f - s) / (e - s) * rect.width();

        // ticks
        let span = e - s;
        let step = [1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0]
            .into_iter()
            .find(|st| rect.width() / (span / st) >= 40.0)
            .unwrap_or(1000.0);
        let mut f = (s / step).ceil() * step;
        while f <= e {
            let x = x_of(f);
            painter.line_segment(
                [pos2(x, rect.top()), pos2(x, rect.top() + 6.0)],
                Stroke::new(1.0, Color32::from_gray(90)),
            );
            painter.text(
                pos2(x + 2.0, rect.top() + 4.0),
                Align2::LEFT_TOP,
                format!("{f}"),
                FontId::proportional(10.0),
                Color32::from_gray(120),
            );
            f += step;
        }

        // keyframes of the graph parameters and the selected node
        let y = rect.center().y + 5.0;
        for k in edit::key_frames(&self.graph, self.selected.as_deref()) {
            if k < s || k > e {
                continue;
            }
            let c = pos2(x_of(k), y);
            let on = (k - self.frame as f32).abs() < 1e-3;
            let col = if on {
                Color32::from_rgb(235, 190, 60)
            } else {
                Color32::from_rgb(200, 160, 60)
            };
            painter.add(egui::Shape::convex_polygon(
                vec![
                    c + vec2(0.0, -5.0),
                    c + vec2(5.0, 0.0),
                    c + vec2(0.0, 5.0),
                    c + vec2(-5.0, 0.0),
                ],
                col,
                Stroke::new(1.0, Color32::BLACK),
            ));
        }

        // playhead
        let px = x_of(self.frame as f32);
        painter.line_segment(
            [pos2(px, rect.top()), pos2(px, rect.bottom())],
            Stroke::new(2.0, Color32::from_rgb(90, 150, 240)),
        );
        let tag = Rect::from_center_size(pos2(px, rect.bottom() - 7.0), vec2(30.0, 14.0));
        painter.rect_filled(tag, 3.0, Color32::from_rgb(70, 120, 210));
        painter.text(
            tag.center(),
            Align2::CENTER_CENTER,
            format!("{}", self.frame),
            FontId::proportional(10.5),
            Color32::WHITE,
        );

        if resp.clicked() || resp.dragged() {
            if let Some(p) = resp.interact_pointer_pos() {
                let f = s + (p.x - rect.left()) / rect.width() * (e - s);
                self.set_frame(f as f64);
            }
        }
    }
}
