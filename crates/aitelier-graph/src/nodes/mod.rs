//! Built-in nodes.

mod curve;
mod ops;
mod prims;
mod revolve;

use crate::registry::Registry;

pub fn register_builtins(r: &mut Registry) {
    r.register(Box::new(prims::BoxNode::new()));
    r.register(Box::new(prims::GridNode::new()));
    r.register(Box::new(prims::SphereNode::new()));
    r.register(Box::new(prims::CylinderNode::new()));
    r.register(Box::new(prims::CircleNode::new()));
    r.register(Box::new(curve::CurveNode::new()));
    r.register(Box::new(revolve::RevolveNode::new()));
    r.register(Box::new(ops::TransformNode::new()));
    r.register(Box::new(ops::MergeNode::new()));
    r.register(Box::new(ops::NormalNode::new()));
}

/// Implements the boilerplate parts of [`crate::NodeType`].
macro_rules! node_header {
    ($name:literal, $desc:literal, $min:expr, $max:expr) => {
        fn type_name(&self) -> &'static str {
            $name
        }
        fn description(&self) -> &'static str {
            $desc
        }
        fn input_count(&self) -> (usize, Option<usize>) {
            ($min, $max)
        }
        fn params(&self) -> &[crate::registry::ParamDef] {
            &self.params
        }
    };
}
pub(crate) use node_header;
