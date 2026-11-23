//! Parameter panel: graph parameters and the selected node's parameters, with keyframes and expressions.

use aitelier_graph::value::{ExprSpec, Value};
use aitelier_graph::{ParamDef, ParamKind};
use egui::{Color32, RichText};

use crate::app::App;
use crate::edit;

const KEY_ON: Color32 = Color32::from_rgb(235, 190, 60); // key on this frame
const ANIMATED: Color32 = Color32::from_rgb(110, 190, 110); // animated, no key here
const EXPR: Color32 = Color32::from_rgb(140, 170, 235);

/// Evaluates a value at the current frame (for display).
type Eval<'a> = dyn Fn(&Value) -> Result<f64, String> + 'a;

/// Show one number. Returns true if the value changed.
fn scalar_ui(
    ui: &mut egui::Ui,
    v: &mut Value,
    frame: f32,
    eval: &Eval,
    int: bool,
    buttons: bool,
    width: f32,
) -> bool {
    let before = v.clone();
    let live = eval(v);
    let speed = |x: f64| if int { 0.05 } else { (x.abs() * 0.01).max(0.1) };

    let resp = match v {
        Value::Num(n) => {
            let sp = speed(*n);
            let r = ui.add_sized(
                [width, 20.0],
                egui::DragValue::new(n)
                    .speed(sp)
                    .max_decimals(if int { 0 } else { 4 }),
            );
            if int {
                *n = n.round();
            }
            r
        }
        Value::Bool(b) => ui.checkbox(b, ""),
        Value::Anim(_) => {
            let mut x = live.clone().unwrap_or(0.0);
            let sp = speed(x);
            let color = if edit::has_key_at(v, frame) {
                KEY_ON
            } else {
                ANIMATED
            };
            let r = egui::Frame::NONE
                .fill(color.gamma_multiply(0.6))
                .corner_radius(3.0)
                .show(ui, |ui| {
                    ui.add_sized(
                        [width, 20.0],
                        egui::DragValue::new(&mut x).speed(sp).max_decimals(if int {
                            0
                        } else {
                            4
                        }),
                    )
                })
                .inner;
            if r.changed() {
                edit::set_key(v, frame, if int { x.round() } else { x } as f32);
            }
            r
        }
        Value::Str(s) | Value::Expr(ExprSpec { expr: s }) => {
            let r = ui.add(
                egui::TextEdit::singleline(s)
                    .desired_width(width - 8.0)
                    .font(egui::TextStyle::Monospace)
                    .text_color(EXPR),
            );
            match &live {
                Ok(x) => r.clone().on_hover_text(format!("= {x}")),
                Err(e) => r.clone().on_hover_text(e.as_str()),
            };
            r
        }
        Value::List(_) => ui.label("(list)"),
    };

    // right-click menu with all options
    let live_val = live.clone().unwrap_or(0.0);
    resp.context_menu(|ui| {
        if ui.button("Set key at this frame").clicked() {
            edit::set_key(v, frame, live_val as f32);
            ui.close();
        }
        if edit::has_key_at(v, frame) && ui.button("Remove key at this frame").clicked() {
            edit::remove_key(v, frame, live_val as f32);
            ui.close();
        }
        if edit::is_animated(v) && ui.button("Clear animation").clicked() {
            *v = Value::Num(live_val);
            ui.close();
        }
        if !edit::is_expression(v) && ui.button("Use an expression").clicked() {
            *v = Value::Str(fmt_num(live_val));
            ui.close();
        }
        if edit::is_expression(v) && ui.button("Replace by its value").clicked() {
            *v = Value::Num(live_val);
            ui.close();
        }
    });

    if buttons {
        let has_key = edit::has_key_at(v, frame);
        let can_key = matches!(v, Value::Num(_) | Value::Anim(_));
        let key_txt = RichText::new(if has_key { "◆" } else { "◇" }).color(if has_key {
            KEY_ON
        } else if edit::is_animated(v) {
            ANIMATED
        } else {
            Color32::GRAY
        });
        if ui
            .add_enabled(can_key, egui::Button::new(key_txt).small().frame(false))
            .on_hover_text("Set / remove keyframe at this frame")
            .clicked()
        {
            if has_key {
                edit::remove_key(v, frame, live_val as f32);
            } else {
                edit::set_key(v, frame, live_val as f32);
            }
        }
        let is_expr = edit::is_expression(v);
        let f_txt = RichText::new("ƒ").color(if is_expr { EXPR } else { Color32::GRAY });
        if ui
            .add(egui::Button::new(f_txt).small().frame(false))
            .on_hover_text("Switch between a number and an expression")
            .clicked()
        {
            *v = if is_expr {
                Value::Num(live_val)
            } else {
                Value::Str(fmt_num(live_val))
            };
        }
        if let (true, Err(e)) = (is_expr, &live) {
            ui.colored_label(Color32::from_rgb(235, 100, 90), "⚠")
                .on_hover_text(e.as_str());
        }
    }
    *v != before
}

fn fmt_num(x: f64) -> String {
    let s = format!("{x:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Make sure a vector value is a list of `n` components.
fn as_components(v: &mut Value, n: usize) {
    match v {
        Value::List(items) => items.resize(n, Value::Num(0.0)),
        other => *other = Value::List(vec![other.clone(); n]),
    }
}

fn vector_ui(ui: &mut egui::Ui, v: &mut Value, n: usize, frame: f32, eval: &Eval) -> bool {
    as_components(v, n);
    let mut changed = false;
    let Value::List(items) = v else { return false };
    for c in items.iter_mut() {
        changed |= scalar_ui(ui, c, frame, eval, false, false, 66.0);
    }
    // one key button for the whole vector
    let all_keyed = items.iter().all(|c| edit::has_key_at(c, frame));
    let any_anim = items.iter().any(edit::is_animated);
    let can_key = items
        .iter()
        .all(|c| matches!(c, Value::Num(_) | Value::Anim(_)));
    let txt = RichText::new(if all_keyed { "◆" } else { "◇" }).color(if all_keyed {
        KEY_ON
    } else if any_anim {
        ANIMATED
    } else {
        Color32::GRAY
    });
    if ui
        .add_enabled(can_key, egui::Button::new(txt).small().frame(false))
        .on_hover_text("Set / remove keyframes on all components")
        .clicked()
    {
        for c in items.iter_mut() {
            let x = eval(c).unwrap_or(0.0) as f32;
            if all_keyed {
                edit::remove_key(c, frame, x);
            } else {
                edit::set_key(c, frame, x);
            }
        }
        changed = true;
    }
    changed
}

/// Widget for one node parameter. Returns the edited value if it changed.
fn param_ui(
    ui: &mut egui::Ui,
    def: &ParamDef,
    value: &Value,
    frame: f32,
    eval: &Eval,
) -> Option<Value> {
    let mut v = value.clone();
    let mut changed = false;
    match def.kind {
        ParamKind::Float | ParamKind::Int => {
            ui.horizontal(|ui| {
                changed = scalar_ui(
                    ui,
                    &mut v,
                    frame,
                    eval,
                    def.kind == ParamKind::Int,
                    true,
                    90.0,
                )
            });
        }
        ParamKind::Bool => {
            ui.horizontal(|ui| {
                if let Value::Bool(b) = &mut v {
                    changed = ui.checkbox(b, "").changed();
                } else {
                    changed = scalar_ui(ui, &mut v, frame, eval, false, true, 90.0);
                }
            });
        }
        ParamKind::Vec2 => {
            ui.horizontal(|ui| changed = vector_ui(ui, &mut v, 2, frame, eval));
        }
        ParamKind::Vec3 => {
            ui.horizontal(|ui| changed = vector_ui(ui, &mut v, 3, frame, eval));
        }
        ParamKind::Menu => {
            let mut s = match &v {
                Value::Str(s) => s.clone(),
                _ => def
                    .options
                    .first()
                    .map(|o| o.to_string())
                    .unwrap_or_default(),
            };
            egui::ComboBox::from_id_salt(def.name)
                .selected_text(s.clone())
                .show_ui(ui, |ui| {
                    for o in &def.options {
                        changed |= ui.selectable_value(&mut s, o.to_string(), *o).changed();
                    }
                });
            v = Value::Str(s);
        }
        ParamKind::Str => {
            if let Value::Str(s) = &mut v {
                changed = ui.text_edit_singleline(s).changed();
            }
        }
        ParamKind::IntList => {
            if !matches!(v, Value::List(_)) {
                v = Value::List(vec![]);
            }
            let Value::List(items) = &mut v else {
                unreachable!()
            };
            ui.horizontal_wrapped(|ui| {
                let mut remove = None;
                for (i, c) in items.iter_mut().enumerate() {
                    changed |= scalar_ui(ui, c, frame, eval, true, false, 34.0);
                    if ui.small_button("×").clicked() {
                        remove = Some(i);
                    }
                }
                if let Some(i) = remove {
                    items.remove(i);
                    changed = true;
                }
                if ui.small_button("+").clicked() {
                    items.push(Value::Num(0.0));
                    changed = true;
                }
            });
        }
        ParamKind::Vec3List => {
            if !matches!(v, Value::List(_)) {
                v = Value::List(vec![]);
            }
            let Value::List(rows) = &mut v else {
                unreachable!()
            };
            let mut insert = None;
            let mut remove = None;
            for (i, row) in rows.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{i:>2}"))
                            .monospace()
                            .color(Color32::GRAY),
                    );
                    changed |= vector_ui(ui, row, 3, frame, eval);
                    if ui
                        .small_button("+")
                        .on_hover_text("Insert a point after this one")
                        .clicked()
                    {
                        insert = Some(i);
                    }
                    if ui
                        .small_button("×")
                        .on_hover_text("Delete this point")
                        .clicked()
                    {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = insert {
                let copy = rows[i].clone();
                rows.insert(i + 1, copy);
                changed = true;
            }
            if let Some(i) = remove {
                rows.remove(i);
                changed = true;
            }
            if ui.small_button("+ Add point").clicked() {
                let last = rows
                    .last()
                    .cloned()
                    .unwrap_or(Value::List(vec![Value::Num(0.0); 3]));
                rows.push(last);
                changed = true;
            }
        }
    }
    changed.then_some(v)
}

impl App {
    pub fn params_ui(&mut self, ui: &mut egui::Ui) {
        let frame = self.frame as f32;
        ui.add_space(4.0);

        // ---------------------------------------------------------------- selected node
        if let Some(name) = self
            .selected
            .clone()
            .filter(|n| self.graph.nodes.contains_key(n))
        {
            let spec = self.graph.nodes[&name].clone();
            let node_type = self.registry.get(&spec.kind);
            ui.horizontal(|ui| {
                let mut buf = name.clone();
                let r = ui.add(
                    egui::TextEdit::singleline(&mut buf)
                        .desired_width(150.0)
                        .font(egui::TextStyle::Heading),
                );
                if r.changed() && edit::rename_node(&mut self.graph, &name, &buf) {
                    self.selected = Some(buf.trim().to_string());
                }
                ui.label(RichText::new(&spec.kind).color(Color32::GRAY));
            });
            let name = self.selected.clone().unwrap_or(name);
            if let Some(t) = node_type {
                ui.label(
                    RichText::new(t.description())
                        .small()
                        .color(Color32::from_gray(150)),
                );
            }
            ui.horizontal(|ui| {
                let is_output = self.graph.output == name;
                if ui
                    .add_enabled(
                        !is_output,
                        egui::Button::new(if is_output {
                            "✔ Output"
                        } else {
                            "Set as output"
                        }),
                    )
                    .clicked()
                {
                    self.graph.output = name.clone();
                }
                ui.checkbox(&mut self.display_selected, "Show in viewport");
            });
            if let Some(e) = self
                .error
                .as_ref()
                .filter(|e| e.node.as_deref() == Some(name.as_str()))
            {
                ui.colored_label(Color32::from_rgb(240, 110, 100), &e.message);
            }
            ui.separator();

            if let Some(t) = node_type {
                let graph = &self.graph;
                let vars = &self.vars;
                let eval_fn = |v: &Value| self.eval.ctx(graph, frame as f64, vars).num(v);
                let mut edits: Vec<(String, Value)> = Vec::new();
                egui::Grid::new("node_params")
                    .num_columns(2)
                    .spacing([10.0, 6.0])
                    .show(ui, |ui| {
                        for def in t.params() {
                            let set = spec.params.get(def.name);
                            let label = RichText::new(def.name).color(if set.is_some() {
                                Color32::from_gray(225)
                            } else {
                                Color32::from_gray(160)
                            });
                            ui.label(label).on_hover_text(def.doc);
                            let current = set.unwrap_or(&def.default);
                            if def.kind == ParamKind::Vec3List {
                                ui.vertical(|ui| {
                                    if let Some(nv) = param_ui(ui, def, current, frame, &eval_fn) {
                                        edits.push((def.name.to_string(), nv));
                                    }
                                });
                            } else if let Some(nv) = param_ui(ui, def, current, frame, &eval_fn) {
                                edits.push((def.name.to_string(), nv));
                            }
                            ui.end_row();
                        }
                    });
                for (k, v) in edits {
                    if let Some(s) = self.graph.nodes.get_mut(&name) {
                        s.params.insert(k, v);
                    }
                }
                if !spec.params.is_empty() && ui.small_button("Reset all to defaults").clicked() {
                    if let Some(s) = self.graph.nodes.get_mut(&name) {
                        s.params.clear();
                    }
                }
            } else {
                ui.colored_label(
                    Color32::from_rgb(240, 110, 100),
                    format!("Unknown node type `{}`", spec.kind),
                );
            }
            ui.add_space(10.0);
        } else {
            ui.label(RichText::new("No node selected").color(Color32::GRAY));
            ui.label(
                RichText::new("Click a node in the node editor to edit it.")
                    .small()
                    .color(Color32::from_gray(130)),
            );
            ui.add_space(10.0);
        }

        // ---------------------------------------------------------------- graph parameters
        egui::CollapsingHeader::new(RichText::new("Graph parameters").strong())
            .default_open(true)
            .show(ui, |ui| {
                ui.label(
                    RichText::new(
                        "Generator controls. Use them by name in any expression, e.g. height * 0.4",
                    )
                    .small()
                    .color(Color32::from_gray(140)),
                );
                let names: Vec<String> = self.graph.parameters.keys().cloned().collect();
                let mut edits: Vec<(String, Value)> = Vec::new();
                let mut remove: Option<String> = None;
                {
                    let graph = &self.graph;
                    let eval = &self.eval;
                    // parameters may only use parameters declared before them
                    egui::Grid::new("graph_params")
                        .num_columns(3)
                        .spacing([10.0, 6.0])
                        .show(ui, |ui| {
                            for (i, name) in names.iter().enumerate() {
                                let vars: indexmap::IndexMap<String, f64> = self
                                    .vars
                                    .iter()
                                    .take(i)
                                    .map(|(k, v)| (k.clone(), *v))
                                    .collect();
                                let eval_fn =
                                    |v: &Value| eval.ctx(graph, frame as f64, &vars).num(v);
                                let mut v = graph.parameters[name].clone();
                                ui.label(name);
                                ui.horizontal(|ui| {
                                    if scalar_ui(ui, &mut v, frame, &eval_fn, false, true, 90.0) {
                                        edits.push((name.clone(), v));
                                    }
                                });
                                if ui
                                    .small_button("×")
                                    .on_hover_text("Remove this parameter")
                                    .clicked()
                                {
                                    remove = Some(name.clone());
                                }
                                ui.end_row();
                            }
                        });
                }
                for (k, v) in edits {
                    self.graph.parameters.insert(k, v);
                }
                if let Some(k) = remove {
                    self.graph.parameters.shift_remove(&k);
                }
                ui.horizontal(|ui| {
                    let id = ui.make_persistent_id("new_param_name");
                    let mut buf: String =
                        ui.data_mut(|d| d.get_temp::<String>(id).unwrap_or_default());
                    ui.add(
                        egui::TextEdit::singleline(&mut buf)
                            .hint_text("new parameter name")
                            .desired_width(150.0),
                    );
                    let valid = !buf.is_empty()
                        && buf
                            .chars()
                            .next()
                            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                        && buf.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                        && !self.graph.parameters.contains_key(&buf)
                        && !["frame", "time", "fps"].contains(&buf.as_str());
                    if ui.add_enabled(valid, egui::Button::new("Add")).clicked() {
                        self.graph.parameters.insert(buf.clone(), Value::Num(1.0));
                        buf.clear();
                    }
                    ui.data_mut(|d| d.insert_temp(id, buf));
                });
            });

        ui.add_space(8.0);
        egui::CollapsingHeader::new(RichText::new("Legend").small())
            .default_open(false)
            .show(ui, |ui| {
                ui.colored_label(KEY_ON, "◆ keyframe on this frame");
                ui.colored_label(ANIMATED, "◇ animated (no key on this frame)");
                ui.colored_label(EXPR, "ƒ expression");
                ui.label(
                    RichText::new("Grey parameter names use their default value.")
                        .small()
                        .color(Color32::GRAY),
                );
            });
    }
}
