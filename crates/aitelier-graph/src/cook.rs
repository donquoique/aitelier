//! Cooking: evaluating the node graph at a frame, with caching.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use aitelier_geo::Geometry;
use indexmap::IndexMap;

use crate::graph::Graph;
use crate::registry::{Params, Registry};
use crate::value::{EvalCtx, Value};

use crate::GraphError;

/// Evaluates a validated graph. Results are cached: nodes that do not change over
/// time are cooked once; animated nodes are cached per frame.
/// Evaluates parameter values (numbers, expressions, keyframes) at a frame.
/// Works on any graph, even one that does not validate.
pub struct Evaluator {
    engine: rhai::Engine,
}

impl Default for Evaluator {
    fn default() -> Self {
        Self::new()
    }
}

impl Evaluator {
    pub fn new() -> Self {
        let mut engine = rhai::Engine::new();
        engine.set_max_expr_depths(64, 64);
        Self { engine }
    }

    /// Graph-level parameters at `frame`, in declaration order (later ones may use earlier ones).
    pub fn parameters_at(
        &self,
        graph: &Graph,
        frame: f64,
    ) -> Result<IndexMap<String, f64>, GraphError> {
        let mut vars: IndexMap<String, f64> = IndexMap::new();
        for (name, v) in &graph.parameters {
            let x =
                self.ctx(graph, frame, &vars)
                    .num(v)
                    .map_err(|msg| GraphError::ParameterValue {
                        name: name.clone(),
                        msg,
                    })?;
            vars.insert(name.clone(), x);
        }
        Ok(vars)
    }

    /// An evaluation context for `frame` with already-evaluated parameters `vars`.
    pub fn ctx<'a>(
        &'a self,
        graph: &Graph,
        frame: f64,
        vars: &'a IndexMap<String, f64>,
    ) -> EvalCtx<'a> {
        let tl = &graph.timeline;
        EvalCtx {
            frame,
            time: tl.time_at(frame as f32) as f64,
            fps: tl.fps as f64,
            vars,
            engine: &self.engine,
        }
    }
}

pub struct Cooker {
    graph: Arc<Graph>,
    registry: Arc<Registry>,
    eval: Evaluator,
    animated_params: Vec<String>,
    time_dependent: HashMap<String, bool>,
    cache: HashMap<(String, Option<i64>), Arc<Geometry>>,
}

fn frame_key(frame: f64) -> i64 {
    (frame * 1000.0).round() as i64
}

impl Cooker {
    /// Validate the graph and prepare a cooker.
    pub fn new(graph: Arc<Graph>, registry: Arc<Registry>) -> Result<Self, GraphError> {
        validate(&graph, &registry)?;

        // Which graph parameters vary over time (directly, or through earlier parameters)?
        let mut animated_params: Vec<String> = Vec::new();
        for (name, v) in &graph.parameters {
            if v.is_time_dependent(&animated_params) {
                animated_params.push(name.clone());
            }
        }

        // Which nodes vary over time? Visit in dependency order.
        let mut time_dependent = HashMap::new();
        for name in topo_order(&graph)? {
            let spec = &graph.nodes[&name];
            let own = spec
                .params
                .values()
                .any(|v| v.is_time_dependent(&animated_params));
            let upstream = spec.inputs.iter().any(|i| time_dependent[i]);
            time_dependent.insert(name, own || upstream);
        }

        Ok(Self {
            graph,
            registry,
            eval: Evaluator::new(),
            animated_params,
            time_dependent,
            cache: HashMap::new(),
        })
    }

    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    pub fn is_time_dependent(&self, node: &str) -> bool {
        self.time_dependent.get(node).copied().unwrap_or(false)
    }

    pub fn animated_parameters(&self) -> &[String] {
        &self.animated_params
    }

    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Evaluate the graph-level parameters at `frame` (in declaration order, so a
    /// parameter's expression may use the parameters declared before it).
    pub fn parameters_at(&self, frame: f64) -> Result<IndexMap<String, f64>, GraphError> {
        self.eval.parameters_at(&self.graph, frame)
    }

    /// Cook the graph's output node at `frame`.
    pub fn cook_output(&mut self, frame: f64) -> Result<Arc<Geometry>, GraphError> {
        let out = self.graph.output.clone();
        self.cook(&out, frame)
    }

    /// Cook any node at `frame`.
    pub fn cook(&mut self, node: &str, frame: f64) -> Result<Arc<Geometry>, GraphError> {
        if !self.graph.nodes.contains_key(node) {
            return Err(GraphError::UnknownNode(node.to_string()));
        }
        let vars = self.parameters_at(frame)?;
        self.cook_node(node, frame, &vars)
    }

    fn cook_node(
        &mut self,
        name: &str,
        frame: f64,
        vars: &IndexMap<String, f64>,
    ) -> Result<Arc<Geometry>, GraphError> {
        let key = (
            name.to_string(),
            if self.time_dependent[name] {
                Some(frame_key(frame))
            } else {
                None
            },
        );
        if let Some(g) = self.cache.get(&key) {
            return Ok(g.clone());
        }
        let graph = self.graph.clone();
        let registry = self.registry.clone();
        let spec = &graph.nodes[name];

        check_input_count(name, spec, registry.get(&spec.kind).expect("validated"))?;
        let mut inputs = Vec::with_capacity(spec.inputs.len());
        for i in &spec.inputs {
            inputs.push(self.cook_node(i, frame, vars)?);
        }

        let node_type = registry.get(&spec.kind).expect("validated");
        let geo = {
            let ctx = self.eval.ctx(&graph, frame, vars);
            let params = Params {
                node: name,
                values: &spec.params,
                defs: node_type.params(),
                ctx: &ctx,
            };
            node_type.cook(&inputs, &params)?
        };
        let geo = Arc::new(geo);
        self.cache.insert(key, geo.clone());
        Ok(geo)
    }
}

/// Input counts are checked only when a node is cooked, so an unfinished node
/// elsewhere in the graph does not stop the rest from working.
fn check_input_count(
    name: &str,
    spec: &crate::graph::NodeSpec,
    t: &dyn crate::registry::NodeType,
) -> Result<(), GraphError> {
    let (min, max) = t.input_count();
    let n = spec.inputs.len();
    if n < min || max.is_some_and(|m| n > m) {
        let expected = match max {
            Some(m) if m == min => format!("{min}"),
            Some(m) => format!("{min} to {m}"),
            None => format!("at least {min}"),
        };
        return Err(GraphError::InputCount {
            node: name.to_string(),
            kind: spec.kind.clone(),
            expected,
            got: n,
        });
    }
    Ok(())
}

/// Check node types, input names, parameter names and cycles.
pub fn validate(graph: &Graph, registry: &Registry) -> Result<(), GraphError> {
    for (name, spec) in &graph.nodes {
        let t = registry
            .get(&spec.kind)
            .ok_or_else(|| GraphError::UnknownType {
                node: name.clone(),
                kind: spec.kind.clone(),
            })?;
        for i in &spec.inputs {
            if !graph.nodes.contains_key(i) {
                return Err(GraphError::MissingInput {
                    node: name.clone(),
                    input: i.clone(),
                });
            }
        }
        for p in spec.params.keys() {
            if !t.params().iter().any(|d| d.name == p) {
                let valid = t
                    .params()
                    .iter()
                    .map(|d| d.name)
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(GraphError::UnknownParam {
                    node: name.clone(),
                    kind: spec.kind.clone(),
                    param: p.clone(),
                    valid,
                });
            }
        }
    }
    for (name, v) in &graph.parameters {
        if matches!(v, Value::List(_)) {
            return Err(GraphError::ParameterValue {
                name: name.clone(),
                msg: "graph parameters must be single numbers".into(),
            });
        }
    }
    topo_order(graph).map(|_| ())
}

/// Node names ordered so that every node comes after its inputs.
pub fn topo_order(graph: &Graph) -> Result<Vec<String>, GraphError> {
    fn visit(
        g: &Graph,
        n: &str,
        done: &mut HashSet<String>,
        active: &mut HashSet<String>,
        out: &mut Vec<String>,
    ) -> Result<(), GraphError> {
        if done.contains(n) {
            return Ok(());
        }
        if !active.insert(n.to_string()) {
            return Err(GraphError::Cycle(n.to_string()));
        }
        for i in &g.nodes[n].inputs {
            visit(g, i, done, active, out)?;
        }
        active.remove(n);
        done.insert(n.to_string());
        out.push(n.to_string());
        Ok(())
    }
    let mut done = HashSet::new();
    let mut active = HashSet::new();
    let mut out = Vec::new();
    for n in graph.nodes.keys() {
        visit(graph, n, &mut done, &mut active, &mut out)?;
    }
    Ok(out)
}
