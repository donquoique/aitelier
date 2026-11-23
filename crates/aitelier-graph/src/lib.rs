//! AItelier procedural node graph.
//!
//! - [`Graph`]: the JSON graph file (timeline, parameters, nodes, output).
//! - [`Registry`]: the available node types ([`Registry::with_builtins`]).
//! - [`Cooker`]: evaluates a graph at any frame, with caching.

pub mod cook;
pub mod graph;
pub mod nodes;
pub mod registry;
pub mod value;

pub use cook::{Cooker, Evaluator};
pub use graph::{Graph, NodeSpec, NodeUi};
pub use registry::{NodeType, ParamDef, ParamKind, Params, Registry};
pub use value::Value;

#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    #[error("graph file is not valid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("there is no node named `{0}`")]
    UnknownNode(String),
    #[error("node `{node}`: unknown node type `{kind}` (run `aitelier nodes` for the list)")]
    UnknownType { node: String, kind: String },
    #[error("node `{node}`: input `{input}` does not exist")]
    MissingInput { node: String, input: String },
    #[error("node `{node}` ({kind}) needs {expected} input(s), has {got}")]
    InputCount {
        node: String,
        kind: String,
        expected: String,
        got: usize,
    },
    #[error("node `{node}`: `{kind}` has no parameter `{param}` (valid: {valid})")]
    UnknownParam {
        node: String,
        kind: String,
        param: String,
        valid: String,
    },
    #[error("graph has a cycle through node `{0}`")]
    Cycle(String),
    #[error("there is no graph parameter named `{0}`")]
    UnknownParameter(String),
    #[error("graph parameter `{name}`: {msg}")]
    ParameterValue { name: String, msg: String },
    #[error("node `{node}`, parameter `{param}`: {msg}")]
    Param {
        node: String,
        param: String,
        msg: String,
    },
    #[error("node `{node}`: {msg}")]
    Cook { node: String, msg: String },
}

impl GraphError {
    /// The node this error is about, if any (used to highlight it in the editor).
    pub fn node(&self) -> Option<&str> {
        match self {
            GraphError::UnknownType { node, .. }
            | GraphError::MissingInput { node, .. }
            | GraphError::InputCount { node, .. }
            | GraphError::UnknownParam { node, .. }
            | GraphError::Param { node, .. }
            | GraphError::Cook { node, .. } => Some(node),
            GraphError::Cycle(node) => Some(node),
            _ => None,
        }
    }
}
