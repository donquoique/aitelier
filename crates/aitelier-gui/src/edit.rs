//! Editing helpers for graph values: keyframes, mode changes, node bookkeeping.

use aitelier_anim::{Interp, Key};
use aitelier_graph::value::{AnimSpec, KeySpec, Value};
use aitelier_graph::{Graph, NodeSpec, NodeUi, Registry};

/// Keys of an animated value, sorted by frame.
pub fn keys_of(v: &Value) -> Option<Vec<Key>> {
    match v {
        Value::Anim(a) => {
            let mut k: Vec<Key> = a.keys.iter().map(KeySpec::to_key).collect();
            k.sort_by(|a, b| a.frame.total_cmp(&b.frame));
            Some(k)
        }
        _ => None,
    }
}

/// Build an animated value from keys, writing them compactly.
pub fn anim_from_keys(mut keys: Vec<Key>) -> Value {
    keys.sort_by(|a, b| a.frame.total_cmp(&b.frame));
    Value::Anim(AnimSpec {
        keys: keys
            .into_iter()
            .map(|k| {
                if k.interp == Interp::Bezier {
                    KeySpec::Plain(k.frame, k.value)
                } else {
                    KeySpec::WithInterp(k.frame, k.value, k.interp)
                }
            })
            .collect(),
    })
}

pub fn is_animated(v: &Value) -> bool {
    matches!(v, Value::Anim(_))
}

pub fn is_expression(v: &Value) -> bool {
    matches!(v, Value::Str(_) | Value::Expr(_))
}

pub fn has_key_at(v: &Value, frame: f32) -> bool {
    keys_of(v).is_some_and(|k| k.iter().any(|k| (k.frame - frame).abs() < 1e-3))
}

/// Set a key at `frame`. A plain number becomes animated.
pub fn set_key(v: &mut Value, frame: f32, value: f32) {
    let mut keys = keys_of(v).unwrap_or_default();
    match keys.iter_mut().find(|k| (k.frame - frame).abs() < 1e-3) {
        Some(k) => k.value = value,
        None => keys.push(Key {
            frame,
            value,
            interp: Interp::default(),
        }),
    }
    *v = anim_from_keys(keys);
}

/// Remove the key at `frame`. When no key is left, the value becomes a plain number.
pub fn remove_key(v: &mut Value, frame: f32, value_now: f32) {
    if let Some(mut keys) = keys_of(v) {
        keys.retain(|k| (k.frame - frame).abs() >= 1e-3);
        *v = if keys.is_empty() {
            Value::Num(value_now as f64)
        } else {
            anim_from_keys(keys)
        };
    }
}

/// Every key frame inside a value (including inside lists).
pub fn collect_key_frames(v: &Value, out: &mut Vec<f32>) {
    match v {
        Value::Anim(_) => out.extend(keys_of(v).unwrap_or_default().iter().map(|k| k.frame)),
        Value::List(items) => items.iter().for_each(|i| collect_key_frames(i, out)),
        _ => {}
    }
}

/// Every key frame used anywhere in the graph's parameters (and optionally one node).
pub fn key_frames(graph: &Graph, node: Option<&str>) -> Vec<f32> {
    let mut out = Vec::new();
    for v in graph.parameters.values() {
        collect_key_frames(v, &mut out);
    }
    if let Some(spec) = node.and_then(|n| graph.nodes.get(n)) {
        for v in spec.params.values() {
            collect_key_frames(v, &mut out);
        }
    }
    out.sort_by(f32::total_cmp);
    out.dedup();
    out
}

/// A node name based on `base` that is not used yet.
pub fn unique_name(graph: &Graph, base: &str) -> String {
    if !graph.nodes.contains_key(base) {
        return base.to_string();
    }
    (1..)
        .map(|i| format!("{base}{i}"))
        .find(|n| !graph.nodes.contains_key(n))
        .expect("unbounded")
}

/// Add a node of `kind` at editor position `pos`; returns its name.
pub fn add_node(graph: &mut Graph, kind: &str, pos: [f32; 2]) -> String {
    let name = unique_name(graph, kind);
    graph.nodes.insert(
        name.clone(),
        NodeSpec {
            kind: kind.to_string(),
            inputs: vec![],
            params: Default::default(),
            ui: Some(NodeUi { pos }),
        },
    );
    if !graph.nodes.contains_key(&graph.output) {
        graph.output = name.clone();
    }
    name
}

/// Delete a node and every connection to it.
pub fn delete_node(graph: &mut Graph, name: &str) {
    graph.nodes.shift_remove(name);
    for spec in graph.nodes.values_mut() {
        spec.inputs.retain(|i| i != name);
    }
    if graph.output == name {
        graph.output = graph.nodes.keys().last().cloned().unwrap_or_default();
    }
}

/// Rename a node, updating connections and the output. Returns false if the name is taken or empty.
pub fn rename_node(graph: &mut Graph, old: &str, new: &str) -> bool {
    let new = new.trim();
    if new.is_empty()
        || new == old
        || graph.nodes.contains_key(new)
        || !graph.nodes.contains_key(old)
    {
        return false;
    }
    let idx = graph.nodes.get_index_of(old).expect("exists");
    let spec = graph.nodes.shift_remove(old).expect("exists");
    graph.nodes.insert(new.to_string(), spec);
    let last = graph.nodes.len() - 1;
    graph.nodes.move_index(last, idx);
    for s in graph.nodes.values_mut() {
        for i in s.inputs.iter_mut() {
            if i == old {
                *i = new.to_string();
            }
        }
    }
    if graph.output == old {
        graph.output = new.to_string();
    }
    true
}

/// Duplicate a node (without its inputs' connections changing). Returns the new name.
pub fn duplicate_node(graph: &mut Graph, name: &str) -> Option<String> {
    let mut spec = graph.nodes.get(name)?.clone();
    let new = unique_name(graph, name);
    if let Some(ui) = spec.ui.as_mut() {
        ui.pos[0] += 30.0;
        ui.pos[1] += 30.0;
    }
    graph.nodes.insert(new.clone(), spec);
    Some(new)
}

/// Connect `src`'s output into input slot `slot` of `dst`.
pub fn connect(graph: &mut Graph, src: &str, dst: &str, slot: usize) {
    if src == dst || creates_cycle(graph, src, dst) {
        return;
    }
    if let Some(spec) = graph.nodes.get_mut(dst) {
        if slot < spec.inputs.len() {
            spec.inputs[slot] = src.to_string();
        } else {
            spec.inputs.push(src.to_string());
        }
    }
}

/// Would connecting `src` -> `dst` create a loop? (true if `dst` is upstream of `src`)
fn creates_cycle(graph: &Graph, src: &str, dst: &str) -> bool {
    let mut stack = vec![src.to_string()];
    let mut seen = std::collections::HashSet::new();
    while let Some(n) = stack.pop() {
        if n == dst {
            return true;
        }
        if seen.insert(n.clone()) {
            if let Some(s) = graph.nodes.get(&n) {
                stack.extend(s.inputs.iter().cloned());
            }
        }
    }
    false
}

/// Give positions to nodes that have none: columns by depth, top to bottom.
pub fn auto_layout(graph: &mut Graph) {
    if graph.nodes.values().all(|s| s.ui.is_some()) {
        return;
    }
    let order = aitelier_graph::cook::topo_order(graph)
        .unwrap_or_else(|_| graph.nodes.keys().cloned().collect());
    let mut depth: std::collections::HashMap<String, usize> = Default::default();
    for n in &order {
        let d = graph.nodes[n]
            .inputs
            .iter()
            .filter_map(|i| depth.get(i))
            .map(|d| d + 1)
            .max()
            .unwrap_or(0);
        depth.insert(n.clone(), d);
    }
    let mut per_row: std::collections::HashMap<usize, usize> = Default::default();
    for n in &order {
        let d = depth[n];
        let col = per_row.entry(d).or_insert(0);
        let spec = graph.nodes.get_mut(n).expect("exists");
        if spec.ui.is_none() {
            spec.ui = Some(NodeUi {
                pos: [*col as f32 * 190.0, d as f32 * 100.0],
            });
        }
        *col += 1;
    }
}

/// Number of input slots to draw for a node.
pub fn input_slots(registry: &Registry, spec: &NodeSpec) -> usize {
    match registry.get(&spec.kind).map(|t| t.input_count()) {
        Some((_, Some(max))) => max,
        Some((_, None)) => spec.inputs.len() + 1,
        None => spec.inputs.len(),
    }
}

/// The graph without editor-only data, for "did anything that affects cooking change?".
pub fn cook_key(graph: &Graph) -> Graph {
    let mut g = graph.clone();
    for s in g.nodes.values_mut() {
        s.ui = None;
    }
    g
}

/// Rename identifiers in an expression (e.g. `height * 0.5` -> `vase_height * 0.5`).
pub fn rename_identifiers(src: &str, map: &std::collections::HashMap<String, String>) -> String {
    let mut out = String::with_capacity(src.len());
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if !word.is_empty() {
            match map.get(word.as_str()) {
                Some(new) if !word.starts_with(|c: char| c.is_ascii_digit()) => out.push_str(new),
                _ => out.push_str(word),
            }
            word.clear();
        }
    };
    for c in src.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

fn rename_in_value(v: &mut Value, map: &std::collections::HashMap<String, String>) {
    match v {
        Value::Str(s) => *s = rename_identifiers(s, map),
        Value::Expr(e) => e.expr = rename_identifiers(&e.expr, map),
        Value::List(items) => items.iter_mut().for_each(|i| rename_in_value(i, map)),
        _ => {}
    }
}

/// Turn any text into a safe name part: lowercase letters, digits and underscores.
pub fn slug(name: &str) -> String {
    let s: String = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let s = s.trim_matches('_').to_string();
    if s.is_empty() || s.starts_with(|c: char| c.is_ascii_digit()) {
        format!("asset_{s}")
    } else {
        s
    }
}

/// Copy another graph's nodes and parameters into `target`, prefixing their names so
/// nothing clashes. Expressions are updated to the new parameter names. The copied
/// nodes are placed to the right of the existing ones. Returns the copied output node.
pub fn insert_graph(
    target: &mut Graph,
    asset: &Graph,
    prefix: &str,
    registry: &Registry,
) -> Option<String> {
    use std::collections::HashMap;
    let prefix = slug(prefix);

    let mut param_map: HashMap<String, String> = HashMap::new();
    for name in asset.parameters.keys() {
        let mut new = format!("{prefix}_{name}");
        let mut i = 1;
        while target.parameters.contains_key(&new) || param_map.values().any(|v| *v == new) {
            new = format!("{prefix}_{name}{i}");
            i += 1;
        }
        param_map.insert(name.clone(), new);
    }
    for (name, value) in &asset.parameters {
        let mut v = value.clone();
        rename_in_value(&mut v, &param_map);
        target.parameters.insert(param_map[name].clone(), v);
    }

    let mut node_map: HashMap<String, String> = HashMap::new();
    for name in asset.nodes.keys() {
        let mut new = unique_name(target, &format!("{prefix}_{name}"));
        let mut i = 1;
        while node_map.values().any(|v| *v == new) {
            new = unique_name(target, &format!("{prefix}_{name}{i}"));
            i += 1;
        }
        node_map.insert(name.clone(), new);
    }

    // place to the right of what is already there
    let mut asset_sorted = asset.clone();
    auto_layout(&mut asset_sorted);
    let right = target
        .nodes
        .values()
        .filter_map(|s| s.ui.map(|u| u.pos[0]))
        .fold(f32::MIN, f32::max);
    let top = target
        .nodes
        .values()
        .filter_map(|s| s.ui.map(|u| u.pos[1]))
        .fold(f32::MAX, f32::min);
    let a_left = asset_sorted
        .nodes
        .values()
        .filter_map(|s| s.ui.map(|u| u.pos[0]))
        .fold(f32::MAX, f32::min);
    let a_top = asset_sorted
        .nodes
        .values()
        .filter_map(|s| s.ui.map(|u| u.pos[1]))
        .fold(f32::MAX, f32::min);
    let (dx, dy) = if target.nodes.is_empty() {
        (-a_left, -a_top)
    } else {
        (right + 240.0 - a_left, top - a_top)
    };

    for (name, spec) in &asset_sorted.nodes {
        let mut s = spec.clone();
        s.inputs = s
            .inputs
            .iter()
            .map(|i| node_map.get(i).cloned().unwrap_or_else(|| i.clone()))
            .collect();
        let defs = registry
            .get(&s.kind)
            .map(|t| t.params().to_vec())
            .unwrap_or_default();
        for (pname, v) in s.params.iter_mut() {
            let literal = defs.iter().find(|d| d.name == pname).is_some_and(|d| {
                matches!(
                    d.kind,
                    aitelier_graph::ParamKind::Menu | aitelier_graph::ParamKind::Str
                )
            });
            if !literal {
                rename_in_value(v, &param_map);
            }
        }
        if let Some(ui) = s.ui.as_mut() {
            ui.pos = [ui.pos[0] + dx, ui.pos[1] + dy];
        }
        target.nodes.insert(node_map[name].clone(), s);
    }
    let out = node_map.get(&asset.output).cloned();
    if !target.nodes.contains_key(&target.output) {
        if let Some(o) = &out {
            target.output = o.clone();
        }
    }
    out
}

/// A new graph with a single box.
pub fn starter_graph() -> Graph {
    let mut g = Graph::from_json(
        r#"{"timeline": {"start": 1, "end": 48, "fps": 24}, "nodes": {}, "output": ""}"#,
    )
    .expect("valid");
    add_node(&mut g, "box", [0.0, 0.0]);
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_round_trip() {
        let mut v = Value::Num(2.0);
        set_key(&mut v, 1.0, 2.0);
        set_key(&mut v, 10.0, 5.0);
        assert!(has_key_at(&v, 10.0));
        set_key(&mut v, 10.0, 6.0);
        assert_eq!(keys_of(&v).unwrap().len(), 2);
        remove_key(&mut v, 1.0, 6.0);
        remove_key(&mut v, 10.0, 6.0);
        assert_eq!(v, Value::Num(6.0));
    }

    #[test]
    fn insert_renames_nodes_and_parameters() {
        let reg = Registry::with_builtins();
        let asset = Graph::from_json(
            r#"{"parameters": {"h": 2, "w": "h * 2"},
            "nodes": {"b": {"type": "box", "params": {"size": ["w", "h", 1]}},
                      "c": {"type": "circle", "params": {"plane": "xy", "radius": "h"}},
                      "m": {"type": "merge", "inputs": ["b", "c"]}}, "output": "m"}"#,
        )
        .unwrap();
        let mut g = starter_graph();
        let out = insert_graph(&mut g, &asset, "My Chair", &reg).unwrap();
        assert_eq!(out, "my_chair_m");
        assert_eq!(
            g.parameters["my_chair_w"],
            Value::Str("my_chair_h * 2".into())
        );
        assert_eq!(
            g.nodes["my_chair_m"].inputs,
            vec!["my_chair_b".to_string(), "my_chair_c".to_string()]
        );
        assert_eq!(
            g.nodes["my_chair_c"].params["plane"],
            Value::Str("xy".into()),
            "menu values are not renamed"
        );
        assert_eq!(
            g.nodes["my_chair_c"].params["radius"],
            Value::Str("my_chair_h".into())
        );
        assert_eq!(g.output, "box", "existing output kept");
        // inserting twice does not clash
        let out2 = insert_graph(&mut g, &asset, "My Chair", &reg).unwrap();
        assert_ne!(out, out2);
        assert!(aitelier_graph::cook::validate(&g, &reg).is_ok());
    }

    #[test]
    fn rename_and_delete() {
        let mut g = starter_graph();
        let t = add_node(&mut g, "transform", [0.0, 100.0]);
        connect(&mut g, "box", &t, 0);
        g.output = t.clone();
        assert!(rename_node(&mut g, "box", "cube"));
        assert_eq!(g.nodes[&t].inputs, vec!["cube".to_string()]);
        assert_eq!(g.nodes.get_index_of("cube"), Some(0));
        connect(&mut g, &t, "cube", 0); // would be a cycle: ignored
        assert!(g.nodes["cube"].inputs.is_empty());
        delete_node(&mut g, "cube");
        assert!(g.nodes[&t].inputs.is_empty());
    }
}
