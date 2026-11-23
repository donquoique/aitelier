//! Primitive shapes: box, grid, sphere, cylinder, circle.

use std::f32::consts::TAU;
use std::sync::Arc;

use aitelier_geo::{Geometry, Vec2, Vec3};

use super::node_header;
use crate::registry::{NodeType, ParamDef, Params};
use crate::GraphError;

// ---------------------------------------------------------------- box

pub struct BoxNode {
    params: Vec<ParamDef>,
}
impl BoxNode {
    pub fn new() -> Self {
        Self {
            params: vec![
                ParamDef::vec3("size", [1.0, 1.0, 1.0], "Size along X, Y, Z"),
                ParamDef::vec3("center", [0.0, 0.0, 0.0], "Center position"),
            ],
        }
    }
}
impl NodeType for BoxNode {
    node_header!(
        "box",
        "Box with 6 quad faces, per-face UVs and hard-edged normals.",
        0,
        Some(0)
    );
    fn cook(&self, _: &[Arc<Geometry>], p: &Params) -> Result<Geometry, GraphError> {
        let h = p.vec3("size")?.abs() * 0.5;
        let c = p.vec3("center")?;
        let mut g = Geometry::new();
        for i in 0..8u32 {
            let s = |bit: u32, v: f32| if i & bit != 0 { v } else { -v };
            g.add_point(c + Vec3::new(s(1, h.x), s(2, h.y), s(4, h.z)));
        }
        // Outward, counter-clockwise faces: +X, -X, +Y, -Y, +Z, -Z.
        for f in [
            [1, 3, 7, 5],
            [0, 4, 6, 2],
            [2, 6, 7, 3],
            [0, 1, 5, 4],
            [4, 5, 7, 6],
            [0, 2, 3, 1],
        ] {
            g.add_polygon(f.to_vec());
        }
        let quad_uv = [
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(0.0, 1.0),
        ];
        let uv = g.vertex_vec2_mut("uv");
        for (k, t) in uv.iter_mut().enumerate() {
            *t = quad_uv[k % 4];
        }
        g.compute_vertex_normals(30.0);
        Ok(g)
    }
}

// ---------------------------------------------------------------- grid

pub struct GridNode {
    params: Vec<ParamDef>,
}
impl GridNode {
    pub fn new() -> Self {
        Self {
            params: vec![
                ParamDef::vec2("size", [10.0, 10.0], "Size along X and Z"),
                ParamDef::int("columns", 10, "Divisions along X"),
                ParamDef::int("rows", 10, "Divisions along Z"),
                ParamDef::vec3("center", [0.0, 0.0, 0.0], "Center position"),
            ],
        }
    }
}
impl NodeType for GridNode {
    node_header!(
        "grid",
        "Flat grid of quads on the XZ plane, facing +Y (a ground plane).",
        0,
        Some(0)
    );
    fn cook(&self, _: &[Arc<Geometry>], p: &Params) -> Result<Geometry, GraphError> {
        let size = p.vec2("size")?;
        let (nx, nz) = (p.int_min("columns", 1)?, p.int_min("rows", 1)?);
        let c = p.vec3("center")?;
        let mut g = Geometry::new();
        let id = |i: usize, j: usize| (j * (nx + 1) + i) as u32;
        for j in 0..=nz {
            for i in 0..=nx {
                let (u, v) = (i as f32 / nx as f32, j as f32 / nz as f32);
                g.add_point(c + Vec3::new((u - 0.5) * size.x, 0.0, (v - 0.5) * size.y));
            }
        }
        for j in 0..nz {
            for i in 0..nx {
                g.add_polygon(vec![id(i, j), id(i, j + 1), id(i + 1, j + 1), id(i + 1, j)]);
            }
        }
        // Per-vertex UVs from the point positions.
        let uvs: Vec<Vec2> = g
            .prims
            .iter()
            .flat_map(|pr| {
                pr.points
                    .iter()
                    .map(|&pt| ((pt as usize % (nx + 1)), (pt as usize / (nx + 1))))
            })
            .map(|(i, j)| Vec2::new(i as f32 / nx as f32, j as f32 / nz as f32))
            .collect();
        *g.vertex_vec2_mut("uv") = uvs;
        g.compute_vertex_normals(30.0);
        Ok(g)
    }
}

// ---------------------------------------------------------------- sphere

pub struct SphereNode {
    params: Vec<ParamDef>,
}
impl SphereNode {
    pub fn new() -> Self {
        Self {
            params: vec![
                ParamDef::float("radius", 1.0, "Radius"),
                ParamDef::int("rows", 16, "Latitude bands (pole to pole)"),
                ParamDef::int("columns", 32, "Longitude divisions"),
                ParamDef::vec3("center", [0.0, 0.0, 0.0], "Center position"),
            ],
        }
    }
}
impl NodeType for SphereNode {
    node_header!(
        "sphere",
        "UV sphere: closed and watertight, with UVs and smooth normals.",
        0,
        Some(0)
    );
    fn cook(&self, _: &[Arc<Geometry>], p: &Params) -> Result<Geometry, GraphError> {
        let r = p.f32("radius")?;
        let rows = p.int_min("rows", 2)?;
        let cols = p.int_min("columns", 3)?;
        let c = p.vec3("center")?;
        // A sphere is a half circle revolved around Y.
        let profile: Vec<Vec3> = (0..=rows)
            .map(|k| {
                let a = std::f32::consts::PI * k as f32 / rows as f32;
                Vec3::new(r * a.sin(), -r * a.cos(), 0.0)
            })
            .collect();
        let mut g = crate::nodes::revolve::revolve_points(
            &profile,
            false,
            Vec3::ZERO,
            Vec3::Y,
            0.0,
            360.0,
            cols,
            60.0,
        );
        g.transform(aitelier_geo::Mat4::from_translation(c));
        Ok(g)
    }
}

// ---------------------------------------------------------------- cylinder

pub struct CylinderNode {
    params: Vec<ParamDef>,
}
impl CylinderNode {
    pub fn new() -> Self {
        Self {
            params: vec![
                ParamDef::float("radius", 1.0, "Radius"),
                ParamDef::float("height", 2.0, "Height along Y"),
                ParamDef::int("columns", 32, "Divisions around"),
                ParamDef::int("rows", 1, "Divisions along the height"),
                ParamDef::boolean("caps", true, "Close the top and bottom"),
                ParamDef::vec3("center", [0.0, 0.0, 0.0], "Center position"),
            ],
        }
    }
}
impl NodeType for CylinderNode {
    node_header!(
        "cylinder",
        "Cylinder along Y; watertight when capped.",
        0,
        Some(0)
    );
    fn cook(&self, _: &[Arc<Geometry>], p: &Params) -> Result<Geometry, GraphError> {
        let r = p.f32("radius")?;
        let h = p.f32("height")?;
        let cols = p.int_min("columns", 3)?;
        let rows = p.int_min("rows", 1)?;
        let caps = p.boolean("caps")?;
        let c = p.vec3("center")?;
        // Profile: (axis bottom) -> side bottom -> side top -> (axis top), revolved around Y.
        let mut profile = Vec::new();
        if caps {
            profile.push(Vec3::new(0.0, -h / 2.0, 0.0));
        }
        for k in 0..=rows {
            profile.push(Vec3::new(r, -h / 2.0 + h * k as f32 / rows as f32, 0.0));
        }
        if caps {
            profile.push(Vec3::new(0.0, h / 2.0, 0.0));
        }
        let mut g = crate::nodes::revolve::revolve_points(
            &profile,
            false,
            Vec3::ZERO,
            Vec3::Y,
            0.0,
            360.0,
            cols,
            45.0,
        );
        g.transform(aitelier_geo::Mat4::from_translation(c));
        Ok(g)
    }
}

// ---------------------------------------------------------------- circle

pub struct CircleNode {
    params: Vec<ParamDef>,
}
impl CircleNode {
    pub fn new() -> Self {
        Self {
            params: vec![
                ParamDef::float("radius", 1.0, "Radius"),
                ParamDef::int("segments", 32, "Number of points"),
                ParamDef::menu("plane", &["xz", "xy", "yz"], "Plane the circle lies in"),
                ParamDef::menu(
                    "output",
                    &["polygon", "polyline"],
                    "Filled face, or closed outline curve",
                ),
                ParamDef::vec3("center", [0.0, 0.0, 0.0], "Center position"),
            ],
        }
    }
}
impl NodeType for CircleNode {
    node_header!(
        "circle",
        "Circle as a filled polygon or a closed outline.",
        0,
        Some(0)
    );
    fn cook(&self, _: &[Arc<Geometry>], p: &Params) -> Result<Geometry, GraphError> {
        let r = p.f32("radius")?;
        let n = p.int_min("segments", 3)?;
        let plane = p.menu("plane")?;
        let outline = p.menu("output")? == "polyline";
        let c = p.vec3("center")?;
        let (normal, map): (Vec3, fn(f32, f32) -> Vec3) = match plane.as_str() {
            "xy" => (Vec3::Z, |a, b| Vec3::new(a, b, 0.0)),
            "yz" => (Vec3::X, |a, b| Vec3::new(0.0, a, b)),
            _ => (Vec3::Y, |a, b| Vec3::new(a, 0.0, b)),
        };
        let mut g = Geometry::new();
        let mut ids: Vec<u32> = (0..n)
            .map(|k| {
                let a = TAU * k as f32 / n as f32;
                g.add_point(c + map(r * a.cos(), r * a.sin()))
            })
            .collect();
        if outline {
            ids.push(ids[0]);
            g.add_polyline(ids);
        } else {
            g.add_polygon(ids);
            if g.prim_normal(0).dot(normal) < 0.0 {
                g.reverse_prim(0);
            }
            g.compute_vertex_normals(30.0);
        }
        Ok(g)
    }
}
