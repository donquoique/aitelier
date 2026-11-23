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
