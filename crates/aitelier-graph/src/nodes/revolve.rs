//! Revolve (lathe): sweep a profile curve around an axis.
//!
//! Profile points that lie on the axis become a single shared point, so a profile that
//! starts and ends on the axis produces a closed, watertight solid. To model a vessel
//! with wall thickness, draw the outer wall upward and the inner wall back down.

use std::sync::Arc;

use aitelier_geo::{Geometry, PrimKind, Quat, Vec2, Vec3};

use super::node_header;
use crate::registry::{NodeType, ParamDef, Params};
use crate::GraphError;

pub struct RevolveNode {
    params: Vec<ParamDef>,
}
impl RevolveNode {
    pub fn new() -> Self {
        Self {
            params: vec![
                ParamDef::vec3("axis", [0.0, 1.0, 0.0], "Axis direction"),
                ParamDef::vec3("origin", [0.0, 0.0, 0.0], "A point on the axis"),
                ParamDef::int("divisions", 48, "Steps around the axis"),
                ParamDef::float("start_angle", 0.0, "Start angle in degrees"),
                ParamDef::float("end_angle", 360.0, "End angle in degrees (360 = full turn)"),
                ParamDef::float(
                    "cusp_angle",
                    50.0,
                    "Edges sharper than this angle (degrees) stay hard",
                ),
            ],
        }
    }
}

impl NodeType for RevolveNode {
    node_header!("revolve", "Sweep the input profile curve around an axis (lathe). Profiles that start and end on the axis give watertight solids.", 1, Some(1));
    fn cook(&self, inputs: &[Arc<Geometry>], p: &Params) -> Result<Geometry, GraphError> {
        let input = &inputs[0];
        let prim = input
            .prims
            .first()
            .ok_or_else(|| p.fail("input has no curve to revolve"))?;
        let mut pts: Vec<Vec3> = prim
            .points
            .iter()
            .map(|&i| input.points[i as usize])
            .collect();
        let mut closed = prim.kind == PrimKind::Polygon;
        if pts.len() > 2 && prim.points.first() == prim.points.last() {
            pts.pop();
            closed = true;
        }
        if pts.len() < 2 {
            return Err(p.fail("profile needs at least 2 points"));
        }
        let axis = p.vec3("axis")?;
        if axis.length_squared() < 1e-12 {
            return Err(p.fail("axis has zero length"));
        }
        Ok(revolve_points(
            &pts,
            closed,
            p.vec3("origin")?,
            axis.normalize(),
            p.f32("start_angle")?,
            p.f32("end_angle")?,
            p.int_min("divisions", 3)?,
            p.f32("cusp_angle")?,
        ))
    }
}

/// Revolve `profile` around the axis through `origin` with unit direction `axis`.
#[allow(clippy::too_many_arguments)]
pub fn revolve_points(
    profile: &[Vec3],
    closed: bool,
    origin: Vec3,
    axis: Vec3,
    start_deg: f32,
    end_deg: f32,
    divisions: usize,
    cusp: f32,
) -> Geometry {
    let span = end_deg - start_deg;
    let full = span.abs() >= 360.0 - 1e-3;
    let steps = if full { divisions } else { divisions + 1 }; // distinct angles
    let scale = profile
        .iter()
        .map(|q| (*q - origin).length())
        .fold(0.0f32, f32::max)
        .max(1e-6);
    let eps = scale * 1e-5;

    let rel: Vec<Vec3> = profile.iter().map(|q| *q - origin).collect();
    let on_axis: Vec<bool> = rel
        .iter()
        .map(|d| (*d - axis * d.dot(axis)).length() < eps)
        .collect();

    let mut g = Geometry::new();
    // idx[i][j]: point of profile point i at angle step j.
    let mut idx: Vec<Vec<u32>> = Vec::with_capacity(profile.len());
    for (i, d) in rel.iter().enumerate() {
        if on_axis[i] {
            let p = g.add_point(origin + axis * d.dot(axis));
            idx.push(vec![p; steps]);
        } else {
            let row = (0..steps)
                .map(|j| {
                    let a = (start_deg + span * j as f32 / divisions as f32).to_radians();
                    g.add_point(origin + Quat::from_axis_angle(axis, a) * *d)
                })
                .collect();
            idx.push(row);
        }
    }

    // v coordinate: arc length along the profile.
    let mut arc = vec![0.0f32; profile.len() + 1];
    for i in 0..profile.len() {
        let next = if i + 1 < profile.len() {
            profile[i + 1]
        } else {
            profile[0]
        };
        arc[i + 1] = arc[i] + profile[i].distance(next);
    }
    let segs = if closed {
        profile.len()
    } else {
        profile.len() - 1
    };
    let total = arc[segs].max(1e-6);

    let mut uvs: Vec<Vec2> = Vec::new();
    let mut seg_first_face: Vec<Option<usize>> = vec![None; segs];
    for i in 0..segs {
        let i1 = (i + 1) % profile.len();
        if on_axis[i] && on_axis[i1] {
            continue; // a segment along the axis sweeps no area
        }
        let (v0, v1) = (arc[i] / total, arc[i + 1] / total);
        seg_first_face[i] = Some(g.prims.len());
        for j in 0..divisions {
            let j1 = if full { (j + 1) % steps } else { j + 1 };
            let (u0, u1) = (
                j as f32 / divisions as f32,
                (j + 1) as f32 / divisions as f32,
            );
            let um = (u0 + u1) * 0.5;
            let (a, b, c, d) = (idx[i][j], idx[i1][j], idx[i1][j1], idx[i][j1]);
            if on_axis[i] {
                g.add_polygon(vec![a, b, c]);
                uvs.extend([Vec2::new(um, v0), Vec2::new(u0, v1), Vec2::new(u1, v1)]);
            } else if on_axis[i1] {
                g.add_polygon(vec![a, b, d]);
                uvs.extend([Vec2::new(u0, v0), Vec2::new(um, v1), Vec2::new(u1, v0)]);
            } else {
                g.add_polygon(vec![a, b, c, d]);
                uvs.extend([
                    Vec2::new(u0, v0),
                    Vec2::new(u0, v1),
                    Vec2::new(u1, v1),
                    Vec2::new(u1, v0),
                ]);
            }
        }
    }
    *g.vertex_vec2_mut("uv") = uvs;

    if g.topology().is_watertight() {
        g.orient_outward();
    } else {
        // Open surface: faces point away from the axis where the profile runs along +axis.
        let seg = (0..segs).find(|&i| {
            let d = (profile[(i + 1) % profile.len()] - profile[i]).dot(axis);
            seg_first_face[i].is_some() && d.abs() > eps
        });
        if let Some(i) = seg {
            let up = (profile[(i + 1) % profile.len()] - profile[i]).dot(axis) > 0.0;
            let f = seg_first_face[i].expect("checked");
            let c = g.prim_centroid(f) - origin;
            let radial = c - axis * c.dot(axis);
            let outward = g.prim_normal(f).dot(radial) > 0.0;
            if outward != up {
                g.reverse_all();
            }
        }
    }
    g.compute_vertex_normals(cusp);
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_profile_on_axis_is_watertight() {
        let prof = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(4.0, 0.0, 0.0),
            Vec3::new(4.0, 10.0, 0.0),
            Vec3::new(3.5, 10.0, 0.0),
            Vec3::new(3.5, 1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        ];
        let g = revolve_points(&prof, false, Vec3::ZERO, Vec3::Y, 0.0, 360.0, 64, 50.0);
        assert!(g.topology().is_watertight(), "{:?}", g.topology());
        // Volume of a cup: outer cylinder minus inner cylinder (polygonal approximation).
        let expected = std::f32::consts::PI * (16.0 * 10.0 - 3.5 * 3.5 * 9.0);
        let v = g.signed_volume();
        assert!(
            v > 0.0 && (v - expected).abs() / expected < 0.01,
            "volume {v} vs {expected}"
        );
    }

    #[test]
    fn open_tube_faces_outward() {
        let prof = [Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 2.0, 0.0)];
        let g = revolve_points(&prof, false, Vec3::ZERO, Vec3::Y, 0.0, 360.0, 16, 50.0);
        assert_eq!(g.topology().boundary_edges, 32);
        let c = g.prim_centroid(0);
        assert!(g.prim_normal(0).dot(Vec3::new(c.x, 0.0, c.z)) > 0.0);
    }

    #[test]
    fn torus_from_closed_profile() {
        let prof: Vec<Vec3> = (0..12)
            .map(|k| {
                let a = std::f32::consts::TAU * k as f32 / 12.0;
                Vec3::new(3.0 + a.cos(), a.sin(), 0.0)
            })
            .collect();
        let g = revolve_points(&prof, true, Vec3::ZERO, Vec3::Y, 0.0, 360.0, 24, 50.0);
        assert!(g.topology().is_watertight());
        assert!(g.signed_volume() > 0.0);
    }
}
