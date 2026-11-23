//! Operators: transform, merge, normal.

use std::sync::Arc;

use aitelier_geo::{Geometry, Mat4};

use super::node_header;
use crate::registry::{NodeType, ParamDef, Params};
use crate::GraphError;

pub struct TransformNode {
    params: Vec<ParamDef>,
}
impl TransformNode {
    pub fn new() -> Self {
        Self {
            params: vec![
                ParamDef::vec3("translate", [0.0, 0.0, 0.0], "Move"),
                ParamDef::vec3(
                    "rotate",
                    [0.0, 0.0, 0.0],
                    "Rotation in degrees, applied X then Y then Z",
                ),
                ParamDef::vec3("scale", [1.0, 1.0, 1.0], "Scale per axis"),
                ParamDef::float("uniform_scale", 1.0, "Extra scale on all axes"),
                ParamDef::vec3(
                    "pivot",
                    [0.0, 0.0, 0.0],
                    "Rotate and scale around this point",
                ),
            ],
        }
    }
}
impl NodeType for TransformNode {
    node_header!(
        "transform",
        "Move, rotate and scale the input (normals follow).",
        1,
        Some(1)
    );
    fn cook(&self, inputs: &[Arc<Geometry>], p: &Params) -> Result<Geometry, GraphError> {
        let t = p.vec3("translate")?;
        let r = p.vec3("rotate")? * (std::f32::consts::PI / 180.0);
        let s = p.vec3("scale")? * p.f32("uniform_scale")?;
        let pivot = p.vec3("pivot")?;
        let rot =
            Mat4::from_rotation_z(r.z) * Mat4::from_rotation_y(r.y) * Mat4::from_rotation_x(r.x);
        let m = Mat4::from_translation(t + pivot)
            * rot
            * Mat4::from_scale(s)
            * Mat4::from_translation(-pivot);
        let mut g = (*inputs[0]).clone();
        g.transform(m);
        Ok(g)
    }
}

pub struct MergeNode {
    params: Vec<ParamDef>,
}
impl MergeNode {
    pub fn new() -> Self {
        Self { params: vec![] }
    }
}
impl NodeType for MergeNode {
    node_header!("merge", "Combine all inputs into one geometry.", 1, None);
    fn cook(&self, inputs: &[Arc<Geometry>], _: &Params) -> Result<Geometry, GraphError> {
        let mut g = (*inputs[0]).clone();
        for other in &inputs[1..] {
            g.merge(other);
        }
        Ok(g)
    }
}

pub struct NormalNode {
    params: Vec<ParamDef>,
}
impl NormalNode {
    pub fn new() -> Self {
        Self {
            params: vec![ParamDef::float(
                "cusp_angle",
                50.0,
                "Edges sharper than this angle (degrees) stay hard; 180 = fully smooth",
            )],
        }
    }
}
impl NodeType for NormalNode {
    node_header!("normal", "Recompute vertex normals (N).", 1, Some(1));
    fn cook(&self, inputs: &[Arc<Geometry>], p: &Params) -> Result<Geometry, GraphError> {
        let mut g = (*inputs[0]).clone();
        g.compute_vertex_normals(p.f32("cusp_angle")?.clamp(0.0, 180.0));
        Ok(g)
    }
}
