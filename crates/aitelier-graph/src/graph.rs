//! The graph file format (JSON).
//!
//! ```json
//! {
//!   "timeline":   { "start": 1, "end": 48, "fps": 24 },
//!   "parameters": { "height": 20, "belly": { "keys": [[1, 6], [48, 10]] } },
//!   "nodes": {
//!     "profile": { "type": "curve",   "params": { "points": [[0,0,0], ["belly","height*0.4",0]] } },
//!     "vase":    { "type": "revolve", "inputs": ["profile"] }
//!   },
//!   "output": "vase"
//! }
//! ```

use aitelier_anim::Timeline;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::value::Value;
use crate::GraphError;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Graph {
    #[serde(default)]
    pub timeline: Timeline,
    /// Graph-level parameters: the generator's exposed controls. Expressions anywhere
    /// in the graph can use them by name. They can be numbers, expressions or keyframes.
    #[serde(default)]
    pub parameters: IndexMap<String, Value>,
    pub nodes: IndexMap<String, NodeSpec>,
    /// Name of the node whose result is the graph's result.
    pub output: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeSpec {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<String>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub params: IndexMap<String, Value>,
    /// Editor-only data (node position). Ignored when cooking.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<NodeUi>,
}

/// Where a node sits in the node editor.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeUi {
    pub pos: [f32; 2],
}

impl Graph {
    pub fn from_json(text: &str) -> Result<Self, GraphError> {
        Ok(serde_json::from_str(text)?)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("graph serialises")
    }

    /// Replace a graph-level parameter's value (e.g. from the command line or a sampler).
    pub fn set_parameter(&mut self, name: &str, value: Value) -> Result<(), GraphError> {
        match self.parameters.get_mut(name) {
            Some(v) => {
                *v = value;
                Ok(())
            }
            None => Err(GraphError::UnknownParameter(name.to_string())),
        }
    }
}
