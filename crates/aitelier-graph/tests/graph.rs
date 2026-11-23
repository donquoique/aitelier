use std::sync::Arc;

use aitelier_graph::{Cooker, Graph, GraphError, Registry, Value};

fn cooker(g: &Graph, reg: &Arc<Registry>) -> Result<Cooker, GraphError> {
    Cooker::new(Arc::new(g.clone()), reg.clone())
}

fn graph(json: &str) -> Graph {
    Graph::from_json(json).expect("valid json")
}

#[test]
fn closed_primitives_are_watertight_and_outward() {
    let reg = Arc::new(Registry::with_builtins());
    for (kind, params) in [
        ("box", "{}"),
        ("sphere", "{}"),
        ("cylinder", "{}"),
        ("sphere", r#"{"rows": 3, "columns": 5}"#),
    ] {
        let g = graph(&format!(
            r#"{{"nodes": {{"a": {{"type": "{kind}", "params": {params}}}}}, "output": "a"}}"#
        ));
        let mut c = cooker(&g, &reg).unwrap();
        let geo = c.cook_output(1.0).unwrap();
        assert!(
            geo.topology().is_watertight(),
            "{kind}: {:?}",
            geo.topology()
        );
        assert!(geo.signed_volume() > 0.0, "{kind} inside-out");
    }
}

#[test]
fn box_volume_and_transform() {
    let reg = Arc::new(Registry::with_builtins());
    let g = graph(
        r#"{
        "nodes": {
            "b": {"type": "box", "params": {"size": [2, 3, 4]}},
            "t": {"type": "transform", "inputs": ["b"], "params": {"scale": [-1, 1, 1], "translate": [10, 0, 0]}}
        },
        "output": "t"}"#,
    );
    let mut c = cooker(&g, &reg).unwrap();
    let geo = c.cook_output(1.0).unwrap();
    assert!((geo.signed_volume() - 24.0).abs() < 1e-3);
    let (lo, hi) = geo.bounds();
    assert!((lo.x - 9.0).abs() < 1e-5 && (hi.x - 11.0).abs() < 1e-5);
}

#[test]
fn animation_and_caching() {
    let reg = Arc::new(Registry::with_builtins());
    let g = graph(
        r#"{
        "timeline": {"start": 1, "end": 11, "fps": 24},
        "parameters": {"r": {"keys": [[1, 1, "linear"], [11, 2]]}, "h": 5, "double_r": "r * 2"},
        "nodes": {
            "static": {"type": "box", "params": {"size": ["h", 1, 1]}},
            "anim": {"type": "sphere", "params": {"radius": "double_r"}},
            "spin": {"type": "transform", "inputs": ["static"], "params": {"rotate": [0, "frame * 9", 0]}},
            "all": {"type": "merge", "inputs": ["anim", "spin"]}
        },
        "output": "all"}"#,
    );
    let mut c = cooker(&g, &reg).unwrap();
    assert!(!c.is_time_dependent("static"));
    assert!(c.is_time_dependent("anim"));
    assert!(c.is_time_dependent("spin"));
    assert_eq!(
        c.animated_parameters(),
        ["r".to_string(), "double_r".to_string()]
    );
    let s1 = c.cook("static", 1.0).unwrap();
    let s2 = c.cook("static", 7.0).unwrap();
    assert!(
        std::sync::Arc::ptr_eq(&s1, &s2),
        "static node should be cooked once"
    );
    let a = c.cook("anim", 6.0).unwrap();
    let (_, hi) = a.bounds();
    assert!(
        (hi.y - 3.0).abs() < 1e-4,
        "radius at frame 6 should be 2*1.5, got {}",
        hi.y
    );
}

#[test]
fn helpful_errors() {
    let reg = Arc::new(Registry::with_builtins());
    let bad_param =
        graph(r#"{"nodes": {"a": {"type": "box", "params": {"sise": 1}}}, "output": "a"}"#);
    let e = cooker(&bad_param, &reg).err().unwrap();
    assert!(matches!(e, GraphError::UnknownParam { .. }), "{e}");
    assert!(e.to_string().contains("size"));

    let cycle = graph(
        r#"{"nodes": {"a": {"type": "transform", "inputs": ["b"]}, "b": {"type": "transform", "inputs": ["a"]}}, "output": "a"}"#,
    );
    assert!(matches!(
        cooker(&cycle, &reg).err().unwrap(),
        GraphError::Cycle(_)
    ));

    let bad_expr = graph(
        r#"{"nodes": {"a": {"type": "sphere", "params": {"radius": "nope + 1"}}}, "output": "a"}"#,
    );
    let mut c = cooker(&bad_expr, &reg).unwrap();
    let e = c.cook_output(1.0).err().unwrap();
    assert!(e.to_string().contains("radius"), "{e}");
}

#[test]
fn set_parameter_and_roundtrip() {
    let mut g = graph(
        r#"{"parameters": {"r": 1}, "nodes": {"a": {"type": "sphere", "params": {"radius": "r"}}}, "output": "a"}"#,
    );
    g.set_parameter("r", Value::Num(3.0)).unwrap();
    assert!(g.set_parameter("zzz", Value::Num(3.0)).is_err());
    let again = Graph::from_json(&g.to_json()).unwrap();
    assert_eq!(again, g);
}

#[test]
fn unfinished_node_does_not_break_output() {
    let reg = Arc::new(Registry::with_builtins());
    let g = graph(
        r#"{"nodes": {"a": {"type": "box"}, "lonely": {"type": "transform"}}, "output": "a"}"#,
    );
    let mut c = cooker(&g, &reg).unwrap();
    assert!(c.cook_output(1.0).is_ok());
    let e = c.cook("lonely", 1.0).err().unwrap();
    assert!(matches!(e, GraphError::InputCount { .. }), "{e}");
}

fn cook_json(json: &str) -> std::sync::Arc<aitelier_geo::Geometry> {
    let reg = Arc::new(Registry::with_builtins());
    let mut c = cooker(&graph(json), &reg).unwrap();
    c.cook_output(1.0).unwrap()
}

#[test]
fn extrude_rectangle_and_taper() {
    let g = cook_json(
        r#"{"nodes": {"r": {"type": "rectangle", "params": {"size": [2, 1]}}, "e": {"type": "extrude", "inputs": ["r"], "params": {"depth": 3}}}, "output": "e"}"#,
    );
    assert!(g.topology().is_watertight(), "{:?}", g.topology());
    assert!(
        (g.signed_volume() - 6.0).abs() < 1e-4,
        "{}",
        g.signed_volume()
    );
    let (lo, hi) = g.bounds();
    assert!(
        (lo.y - 0.0).abs() < 1e-5 && (hi.y - 3.0).abs() < 1e-5,
        "extrudes along +Y"
    );
    let t = cook_json(
        r#"{"nodes": {"r": {"type": "rectangle", "params": {"size": [2, 2]}}, "e": {"type": "extrude", "inputs": ["r"], "params": {"depth": 3, "taper": 0}}}, "output": "e"}"#,
    );
    assert!(
        (t.signed_volume() - 4.0).abs() < 1e-3,
        "pyramid volume {}",
        t.signed_volume()
    );
}

#[test]
fn extrude_concave_curve() {
    let g = cook_json(
        r#"{"nodes": {"c": {"type": "curve", "params": {"points": [[0,0,0],[2,0,0],[2,1,0],[1,1,0],[1,2,0],[0,2,0]], "closed": true}},
        "e": {"type": "extrude", "inputs": ["c"], "params": {"depth": 1}}}, "output": "e"}"#,
    );
    assert!(g.topology().is_watertight());
    assert!(
        (g.signed_volume() - 3.0).abs() < 1e-4,
        "{}",
        g.signed_volume()
    );
}

#[test]
fn rounded_rectangle_area() {
    let g = cook_json(
        r#"{"nodes": {"r": {"type": "rectangle", "params": {"size": [4, 2], "corner_radius": 0.5, "corner_segments": 16}},
        "e": {"type": "extrude", "inputs": ["r"], "params": {"depth": 1}}}, "output": "e"}"#,
    );
    let expected = 8.0 - (4.0 - std::f32::consts::PI) * 0.25;
    assert!(
        (g.signed_volume() - expected).abs() < 0.01,
        "{} vs {expected}",
        g.signed_volume()
    );
}

#[test]
fn boolean_node_drills_a_hole() {
    let g = cook_json(
        r#"{"nodes": {"plate": {"type": "box", "params": {"size": [4, 1, 4]}},
        "pin": {"type": "cylinder", "params": {"radius": 0.5, "height": 3, "columns": 32}},
        "cut": {"type": "boolean", "inputs": ["plate", "pin"], "params": {"operation": "subtract"}}}, "output": "cut"}"#,
    );
    assert!(g.topology().is_watertight(), "{:?}", g.topology());
    let hole = 32.0 * 0.5 * 0.25 * (std::f32::consts::TAU / 32.0).sin(); // polygon area * height 1
    assert!(
        (g.signed_volume() - (16.0 - hole)).abs() < 1e-3,
        "{}",
        g.signed_volume()
    );
}

#[test]
fn sweep_pipe_and_ring() {
    let pipe = cook_json(
        r#"{"nodes": {"prof": {"type": "circle", "params": {"radius": 0.5, "segments": 24, "plane": "xy", "output": "polyline"}},
        "path": {"type": "curve", "params": {"points": [[0,0,0],[0,1,0],[0,2,0],[0,4,0]]}},
        "s": {"type": "sweep", "inputs": ["prof", "path"]}}, "output": "s"}"#,
    );
    assert!(pipe.topology().is_watertight(), "{:?}", pipe.topology());
    let area = 24.0 * 0.5 * 0.25 * (std::f32::consts::TAU / 24.0).sin();
    assert!(
        (pipe.signed_volume() - area * 4.0).abs() < 1e-3,
        "{}",
        pipe.signed_volume()
    );
    let ring = cook_json(
        r#"{"nodes": {"prof": {"type": "circle", "params": {"radius": 0.3, "segments": 12, "plane": "xy", "output": "polyline"}},
        "path": {"type": "circle", "params": {"radius": 3, "segments": 48, "output": "polyline"}},
        "s": {"type": "sweep", "inputs": ["prof", "path"], "params": {"twist": 90}}}, "output": "s"}"#,
    );
    assert!(ring.topology().is_watertight(), "{:?}", ring.topology());
    assert!(ring.signed_volume() > 0.0);
}

#[test]
fn copies_array_mirror() {
    let legs = cook_json(
        r#"{"nodes": {"leg": {"type": "box", "params": {"size": [0.2, 1, 0.2]}},
        "corners": {"type": "rectangle", "params": {"size": [2, 1]}},
        "c": {"type": "copy_to_points", "inputs": ["leg", "corners"]}}, "output": "c"}"#,
    );
    assert_eq!(legs.num_prims(), 24);
    assert!((legs.signed_volume() - 4.0 * 0.04).abs() < 1e-4);
    let row = cook_json(
        r#"{"nodes": {"b": {"type": "box"}, "a": {"type": "array", "inputs": ["b"], "params": {"count": 5, "offset": [2, 0, 0]}}}, "output": "a"}"#,
    );
    let (lo, hi) = row.bounds();
    assert!((hi.x - lo.x - 9.0).abs() < 1e-5);
    let radial = cook_json(
        r#"{"nodes": {"b": {"type": "box", "params": {"center": [3, 0, 0]}}, "a": {"type": "array", "inputs": ["b"], "params": {"mode": "radial", "count": 6}}}, "output": "a"}"#,
    );
    assert_eq!(radial.num_prims(), 36);
    let m = cook_json(
        r#"{"nodes": {"b": {"type": "box", "params": {"center": [2, 0, 0]}}, "m": {"type": "mirror", "inputs": ["b"]}}, "output": "m"}"#,
    );
    assert!(
        (m.signed_volume() - 2.0).abs() < 1e-5,
        "mirrored copy faces outward"
    );
    assert!(m.topology().is_watertight());
}
