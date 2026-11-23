//! Node editor canvas: nodes flow top to bottom (inputs on top, output at the bottom), like Houdini.

use egui::epaint::CubicBezierShape;
use egui::{
    pos2, vec2, Align2, Color32, FontId, Key, PointerButton, Pos2, Rect, Sense, Stroke, StrokeKind,
    Vec2,
};

use crate::app::App;
use crate::edit;

const NODE_W: f32 = 150.0;
const NODE_H: f32 = 42.0;
const PORT_R: f32 = 5.5;

pub struct EditorView {
    pub pan: Vec2,
    pub zoom: f32,
    pub frame_on_next_draw: bool,
    /// A connection being dragged from this node's output.
    wire_from: Option<String>,
    /// Where the "add node" menu was opened (graph coordinates).
    menu_pos: [f32; 2],
}

impl Default for EditorView {
    fn default() -> Self {
        Self {
            pan: vec2(60.0, 60.0),
            zoom: 1.0,
            frame_on_next_draw: true,
            wire_from: None,
            menu_pos: [0.0, 0.0],
        }
    }
}

struct Layout {
    name: String,
    rect: Rect,
    inputs: Vec<Pos2>,
    output: Pos2,
}

enum Action {
    Select(String),
    Move(String, Vec2),
    SetOutput(String),
    Delete(String),
    Duplicate(String),
    StartWire(String),
    Disconnect {
        node: String,
        slot: usize,
        source: String,
    },
}

impl App {
    pub fn node_editor_ui(&mut self, ui: &mut egui::Ui) {
        let rect = ui.available_rect_before_wrap();
        let bg = ui.allocate_rect(rect, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_gray(23));

        if self.editor.frame_on_next_draw
            || (bg.hovered()
                && !ui.egui_wants_keyboard_input()
                && ui.input(|i| i.key_pressed(Key::H)))
        {
            self.frame_nodes(rect);
            self.editor.frame_on_next_draw = false;
        }

        // --- pan and zoom
        if bg.dragged_by(PointerButton::Middle) || bg.dragged_by(PointerButton::Primary) {
            self.editor.pan += bg.drag_delta();
        }
        if bg.hovered() || ui.rect_contains_pointer(rect) {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                if let Some(cursor) = ui.input(|i| i.pointer.hover_pos()) {
                    let old = self.editor.zoom;
                    let new = (old * 1.1f32.powf(scroll / 40.0)).clamp(0.3, 2.5);
                    let g = (cursor - rect.min - self.editor.pan) / old;
                    self.editor.pan = cursor - rect.min - g * new;
                    self.editor.zoom = new;
                }
            }
        }
        let (pan, zoom) = (self.editor.pan, self.editor.zoom);
        let to_screen = |p: [f32; 2]| rect.min + pan + vec2(p[0], p[1]) * zoom;
        let to_graph = |p: Pos2| {
            let g = (p - rect.min - pan) / zoom;
            [g.x, g.y]
        };

        // dotted grid
        let step = 24.0 * zoom;
        if step > 6.0 {
            let start = rect.min + vec2(pan.x.rem_euclid(step), pan.y.rem_euclid(step));
            let mut y = start.y;
            while y < rect.max.y {
                let mut x = start.x;
                while x < rect.max.x {
                    painter.circle_filled(pos2(x, y), 0.9, Color32::from_gray(48));
                    x += step;
                }
                y += step;
            }
        }

        // --- layout
        let layouts: Vec<Layout> = self
            .graph
            .nodes
            .iter()
            .map(|(name, spec)| {
                let p = spec.ui.map(|u| u.pos).unwrap_or([0.0, 0.0]);
                let r = Rect::from_min_size(to_screen(p), vec2(NODE_W, NODE_H) * zoom);
                let n = edit::input_slots(&self.registry, spec);
                let inputs = (0..n)
                    .map(|i| {
                        pos2(
                            r.left() + r.width() * (i + 1) as f32 / (n + 1) as f32,
                            r.top(),
                        )
                    })
                    .collect();
                Layout {
                    name: name.clone(),
                    rect: r,
                    inputs,
                    output: pos2(r.center().x, r.bottom()),
                }
            })
            .collect();
        let find = |n: &str| layouts.iter().find(|l| l.name == n);

        // --- wires
        let wire = |from: Pos2, to: Pos2, color: Color32| {
            let k = ((to.y - from.y).abs() * 0.5).max(30.0 * zoom);
            CubicBezierShape::from_points_stroke(
                [from, from + vec2(0.0, k), to - vec2(0.0, k), to],
                false,
                Color32::TRANSPARENT,
                Stroke::new(2.0 * zoom.sqrt(), color),
            )
        };
        for l in &layouts {
            let spec = &self.graph.nodes[&l.name];
            for (slot, src) in spec.inputs.iter().enumerate() {
                if let (Some(s), Some(to)) = (find(src), l.inputs.get(slot)) {
                    let hot = self.selected.as_deref() == Some(l.name.as_str())
                        || self.selected.as_deref() == Some(src.as_str());
                    painter.add(wire(
                        s.output,
                        *to,
                        if hot {
                            Color32::from_rgb(230, 170, 80)
                        } else {
                            Color32::from_gray(140)
                        },
                    ));
                }
            }
        }

        // --- nodes
        let mut actions: Vec<Action> = Vec::new();
        let error_node = self.error.as_ref().and_then(|e| e.node.clone());
        let animated = |n: &str| self.cooker.as_ref().is_some_and(|c| c.is_time_dependent(n));
        for l in &layouts {
            let spec = &self.graph.nodes[&l.name];
            let resp = ui.interact(
                l.rect,
                ui.scope_id().with(("node", &l.name)),
                Sense::click_and_drag(),
            );
            if resp.clicked() || resp.drag_started() {
                actions.push(Action::Select(l.name.clone()));
            }
            if resp.dragged_by(PointerButton::Primary) {
                actions.push(Action::Move(l.name.clone(), resp.drag_delta() / zoom));
            }
            if resp.double_clicked() {
                actions.push(Action::SetOutput(l.name.clone()));
            }
            resp.context_menu(|ui| {
                if ui.button("Set as output").clicked() {
                    actions.push(Action::SetOutput(l.name.clone()));
                    ui.close();
                }
                if ui.button("Duplicate").clicked() {
                    actions.push(Action::Duplicate(l.name.clone()));
                    ui.close();
                }
                if ui.button("Delete").clicked() {
                    actions.push(Action::Delete(l.name.clone()));
                    ui.close();
                }
            });

            let selected = self.selected.as_deref() == Some(l.name.as_str());
            let is_output = self.graph.output == l.name;
            let cr = 5.0 * zoom;
            painter.rect_filled(
                l.rect,
                cr,
                if resp.hovered() {
                    Color32::from_gray(62)
                } else {
                    Color32::from_gray(52)
                },
            );
            if is_output {
                // display flag: blue strip on the right
                let strip =
                    Rect::from_min_max(pos2(l.rect.right() - 9.0 * zoom, l.rect.top()), l.rect.max);
                painter.rect_filled(
                    strip,
                    egui::CornerRadius {
                        nw: 0,
                        sw: 0,
                        ne: cr as u8,
                        se: cr as u8,
                    },
                    Color32::from_rgb(70, 130, 220),
                );
            }
            let border = if error_node.as_deref() == Some(l.name.as_str()) {
                Stroke::new(2.0, Color32::from_rgb(235, 90, 80))
            } else if selected {
                Stroke::new(2.0, Color32::from_rgb(240, 175, 60))
            } else {
                Stroke::new(1.0, Color32::from_gray(80))
            };
            painter.rect_stroke(l.rect, cr, border, StrokeKind::Inside);
            if zoom > 0.45 {
                painter.text(
                    l.rect.left_top() + vec2(9.0, 7.0) * zoom,
                    Align2::LEFT_TOP,
                    &l.name,
                    FontId::proportional(13.5 * zoom),
                    Color32::from_gray(235),
                );
                let mut sub = spec.kind.clone();
                if animated(&l.name) {
                    sub.push_str("  ◆");
                }
                painter.text(
                    l.rect.left_top() + vec2(9.0, 24.0) * zoom,
                    Align2::LEFT_TOP,
                    sub,
                    FontId::proportional(11.0 * zoom),
                    Color32::from_gray(150),
                );
            }

            // ports
            let pr = PORT_R * zoom.max(0.6);
            for (slot, p) in l.inputs.iter().enumerate() {
                let connected = spec.inputs.get(slot).cloned();
                let presp = ui.interact(
                    Rect::from_center_size(*p, vec2(pr, pr) * 2.6),
                    ui.scope_id().with(("in", &l.name, slot)),
                    Sense::drag(),
                );
                if presp.drag_started() {
                    if let Some(src) = connected.clone() {
                        actions.push(Action::Disconnect {
                            node: l.name.clone(),
                            slot,
                            source: src,
                        });
                    }
                }
                let fill = if connected.is_some() {
                    Color32::from_gray(190)
                } else {
                    Color32::from_gray(80)
                };
                painter.circle(*p, pr, fill, Stroke::new(1.0, Color32::from_gray(25)));
            }
            let oresp = ui.interact(
                Rect::from_center_size(l.output, vec2(pr, pr) * 2.6),
                ui.scope_id().with(("out", &l.name)),
                Sense::drag(),
            );
            if oresp.drag_started() {
                actions.push(Action::StartWire(l.name.clone()));
            }
            painter.circle(
                l.output,
                pr,
                Color32::from_rgb(230, 200, 120),
                Stroke::new(1.0, Color32::from_gray(25)),
            );
        }

        // --- wire being dragged
        let pointer = ui.input(|i| i.pointer.hover_pos());
        if let (Some(from), Some(ptr)) = (self.editor.wire_from.clone(), pointer) {
            if let Some(s) = find(&from) {
                painter.add(wire(s.output, ptr, Color32::from_rgb(230, 200, 120)));
            }
            if ui.input(|i| i.pointer.any_released()) {
                // drop on the nearest input port
                let target = layouts
                    .iter()
                    .flat_map(|l| {
                        l.inputs
                            .iter()
                            .enumerate()
                            .map(move |(slot, p)| (l.name.clone(), slot, *p))
                    })
                    .filter(|(n, _, _)| *n != from)
                    .map(|(n, slot, p)| (n, slot, p.distance(ptr)))
                    .filter(|(_, _, d)| *d < 18.0 * zoom.max(0.7))
                    .min_by(|a, b| a.2.total_cmp(&b.2));
                if let Some((dst, slot, _)) = target {
                    edit::connect(&mut self.graph, &from, &dst, slot);
                    self.status = format!("Connected {from} → {dst}");
                }
                self.editor.wire_from = None;
            }
        }

        // --- background: add-node menu
        if bg.secondary_clicked() {
            if let Some(p) = bg.interact_pointer_pos() {
                self.editor.menu_pos = to_graph(p);
            }
        }
        let menu_pos = self.editor.menu_pos;
        let mut add: Option<&'static str> = None;
        bg.context_menu(|ui| {
            ui.label(egui::RichText::new("Add node").strong());
            ui.separator();
            for t in self.registry.iter() {
                if ui
                    .button(t.type_name())
                    .on_hover_text(t.description())
                    .clicked()
                {
                    add = Some(t.type_name());
                    ui.close();
                }
            }
        });
        if let Some(kind) = add {
            let name = edit::add_node(
                &mut self.graph,
                kind,
                [menu_pos[0] - NODE_W / 2.0, menu_pos[1] - NODE_H / 2.0],
            );
            self.status = format!("Added {name}");
            self.selected = Some(name);
        }
        if bg.clicked() {
            self.selected = None;
        }

        // --- apply node actions
        for a in actions {
            match a {
                Action::Select(n) => {
                    self.selected = Some(n);
                    self.right_tab = crate::app::RightTab::Parameters;
                }
                Action::Move(n, d) => {
                    if let Some(ui) = self.graph.nodes.get_mut(&n).and_then(|s| s.ui.as_mut()) {
                        ui.pos[0] += d.x;
                        ui.pos[1] += d.y;
                    }
                }
                Action::SetOutput(n) => {
                    self.status = format!("{n} is now the output");
                    self.graph.output = n;
                }
                Action::Delete(n) => {
                    self.selected = Some(n);
                    self.delete_selected();
                }
                Action::Duplicate(n) => self.selected = edit::duplicate_node(&mut self.graph, &n),
                Action::StartWire(n) => self.editor.wire_from = Some(n),
                Action::Disconnect { node, slot, source } => {
                    if let Some(s) = self.graph.nodes.get_mut(&node) {
                        if slot < s.inputs.len() {
                            s.inputs.remove(slot);
                        }
                    }
                    self.editor.wire_from = Some(source);
                }
            }
        }

        // --- keys
        if ui.rect_contains_pointer(rect)
            && !ui.egui_wants_keyboard_input()
            && ui.input(|i| i.key_pressed(Key::Delete) || i.key_pressed(Key::X))
        {
            self.delete_selected();
        }

        if self.graph.nodes.is_empty() {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "Right-click to add a node",
                FontId::proportional(14.0),
                Color32::from_gray(120),
            );
        }
    }

    /// Fit all nodes in view.
    fn frame_nodes(&mut self, rect: Rect) {
        let pts: Vec<[f32; 2]> = self
            .graph
            .nodes
            .values()
            .filter_map(|s| s.ui.map(|u| u.pos))
            .collect();
        if pts.is_empty() {
            return;
        }
        let (mut lo, mut hi) = (vec2(f32::MAX, f32::MAX), vec2(f32::MIN, f32::MIN));
        for p in pts {
            lo = lo.min(vec2(p[0], p[1]));
            hi = hi.max(vec2(p[0] + NODE_W, p[1] + NODE_H));
        }
        let size = (hi - lo).max(vec2(1.0, 1.0));
        let zoom = ((rect.width() - 80.0) / size.x)
            .min((rect.height() - 80.0) / size.y)
            .clamp(0.5, 1.2);
        self.editor.zoom = zoom;
        self.editor.pan = rect.size() * 0.5 - (lo + size * 0.5) * zoom;
    }
}
