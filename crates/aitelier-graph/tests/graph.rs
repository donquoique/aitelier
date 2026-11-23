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
