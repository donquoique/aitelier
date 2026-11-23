//! Copying nodes: copy_to_points, array, mirror.

use std::sync::Arc;

use aitelier_geo::mesh_ops::point_normals;
use aitelier_geo::{AttribArray, Geometry, Mat4, Quat, Vec3};

use super::node_header;
use crate::registry::{NodeType, ParamDef, Params};
use crate::GraphError;

fn euler(deg: Vec3) -> Mat4 {
    let r = deg * (std::f32::consts::PI / 180.0);
    Mat4::from_rotation_z(r.z) * Mat4::from_rotation_y(r.y) * Mat4::from_rotation_x(r.x)
}

fn copies(src: &Geometry, mats: impl IntoIterator<Item = Mat4>) -> Geometry {
    let mut out = Geometry::new();
    for m in mats {
        let mut g = src.clone();
        g.transform(m);
        out.merge(&g);
    }
    out
}

// ---------------------------------------------------------------- copy to points

pub struct CopyToPointsNode {
    params: Vec<ParamDef>,
}
impl CopyToPointsNode {
    pub fn new() -> Self {
        Self {
            params: vec![
                ParamDef::menu(
                    "align",
                    &["none", "normal"],
                    "normal = turn each copy's up axis (+Y) to the point's normal",
                ),
                ParamDef::float(
                    "scale",
                    1.0,
                    "Scale of every copy (multiplied by a point 'pscale' attribute if present)",
                ),
                ParamDef::vec3(
                    "rotate",
                    [0.0, 0.0, 0.0],
                    "Extra rotation of every copy, degrees",
                ),
            ],
        }
    }
}
impl NodeType for CopyToPointsNode {
    node_header!(
        "copy_to_points",
        "Place a copy of input 1 on every point of input 2 (e.g. legs on a rectangle's corners).",
        2,
        Some(2)
    );
    fn cook(&self, inputs: &[Arc<Geometry>], p: &Params) -> Result<Geometry, GraphError> {
        let (src, targets) = (&inputs[0], &inputs[1]);
        if targets.points.is_empty() {
            return Ok(Geometry::new());
        }
        let align = p.menu("align")? == "normal";
        let scale = p.f32("scale")?;
        let rot = euler(p.vec3("rotate")?);
        let normals = if align {
            point_normals(targets)
        } else {
            vec![]
        };
        let pscale = match targets.point_attribs.get("pscale") {
            Some(AttribArray::Float(v)) => Some(v.clone()),
            _ => None,
        };
        let mats = targets.points.iter().enumerate().map(|(i, pt)| {
            let s = scale * pscale.as_ref().map(|v| v[i]).unwrap_or(1.0);
            let orient = if align && normals[i] != Vec3::ZERO {
                Mat4::from_quat(Quat::from_rotation_arc(Vec3::Y, normals[i]))
            } else {
                Mat4::IDENTITY
            };
            Mat4::from_translation(*pt) * orient * rot * Mat4::from_scale(Vec3::splat(s))
        });
        Ok(copies(src, mats))
    }
}

// ---------------------------------------------------------------- array

pub struct ArrayNode {
    params: Vec<ParamDef>,
}
impl ArrayNode {
    pub fn new() -> Self {
        Self {
            params: vec![
                ParamDef::menu(
                    "mode",
                    &["linear", "radial"],
                    "linear = in a row; radial = around an axis",
                ),
                ParamDef::int("count", 4, "Number of copies (including the original)"),
                ParamDef::vec3("offset", [1.0, 0.0, 0.0], "Linear: step between copies"),
                ParamDef::vec3("axis", [0.0, 1.0, 0.0], "Radial: rotation axis"),
                ParamDef::vec3("center", [0.0, 0.0, 0.0], "Radial: a point on the axis"),
                ParamDef::float(
                    "angle",
                    360.0,
                    "Radial: total angle covered, degrees (360 = full circle)",
                ),
            ],
        }
    }
}
impl NodeType for ArrayNode {
    node_header!(
        "array",
        "Repeat the input in a row or around an axis (slats, spokes, chair legs).",
        1,
        Some(1)
    );
    fn cook(&self, inputs: &[Arc<Geometry>], p: &Params) -> Result<Geometry, GraphError> {
        let count = p.int_min("count", 1)?;
        let mats: Vec<Mat4> = if p.menu("mode")? == "radial" {
            let axis = p.vec3("axis")?.normalize_or(Vec3::Y);
            let c = p.vec3("center")?;
            let total = p.f32("angle")?;
            let step = if (total.abs() - 360.0).abs() < 1e-3 {
                total / count as f32
            } else {
                total / (count.max(2) - 1) as f32
            };
            (0..count)
                .map(|k| {
                    Mat4::from_translation(c)
                        * Mat4::from_axis_angle(axis, (step * k as f32).to_radians())
                        * Mat4::from_translation(-c)
                })
                .collect()
        } else {
            let off = p.vec3("offset")?;
            (0..count)
                .map(|k| Mat4::from_translation(off * k as f32))
                .collect()
        };
        Ok(copies(&inputs[0], mats))
    }
}

// ---------------------------------------------------------------- mirror

pub struct MirrorNode {
    params: Vec<ParamDef>,
}
impl MirrorNode {
    pub fn new() -> Self {
        Self {
            params: vec![
                ParamDef::menu(
                    "axis",
                    &["x", "y", "z"],
                    "Mirror across the plane perpendicular to this axis",
                ),
                ParamDef::float("offset", 0.0, "Position of the mirror plane along the axis"),
                ParamDef::boolean(
                    "keep_original",
                    true,
                    "Output the original and the mirrored copy together",
                ),
            ],
        }
    }
}
impl NodeType for MirrorNode {
    node_header!(
        "mirror",
        "Mirror the input across a plane (model one half, get both).",
        1,
        Some(1)
    );
    fn cook(&self, inputs: &[Arc<Geometry>], p: &Params) -> Result<Geometry, GraphError> {
        let axis = match p.menu("axis")?.as_str() {
            "y" => Vec3::Y,
            "z" => Vec3::Z,
            _ => Vec3::X,
        };
        let off = axis * p.f32("offset")?;
        let flip = Vec3::ONE - axis * 2.0;
        let m = Mat4::from_translation(off) * Mat4::from_scale(flip) * Mat4::from_translation(-off);
        let mut mirrored = (*inputs[0]).clone();
        mirrored.transform(m);
        if p.boolean("keep_original")? {
            let mut g = (*inputs[0]).clone();
            g.merge(&mirrored);
            Ok(g)
        } else {
            Ok(mirrored)
        }
    }
}
