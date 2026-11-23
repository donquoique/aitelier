//! Modeling nodes: rectangle, extrude, boolean, sweep.

use std::sync::Arc;

use aitelier_geo::csg::{boolean, BoolOp};
use aitelier_geo::{Geometry, PrimKind, Quat, Vec2, Vec3};

use super::node_header;
use crate::registry::{NodeType, ParamDef, Params};
use crate::GraphError;

/// Point loops of a geometry's curves/faces: (points, closed).
fn loops(g: &Geometry) -> Vec<(Vec<Vec3>, bool)> {
    g.prims
        .iter()
        .filter(|p| p.points.len() >= 2)
        .map(|p| {
            let mut pts: Vec<Vec3> = p.points.iter().map(|&i| g.points[i as usize]).collect();
            let mut closed = p.kind == PrimKind::Polygon;
            if p.points.len() > 2 && p.points.first() == p.points.last() {
                pts.pop();
                closed = true;
            }
            (pts, closed)
        })
        .collect()
}

fn newell(pts: &[Vec3]) -> Vec3 {
    let n = pts.len();
    let mut v = Vec3::ZERO;
    for k in 0..n {
        let (a, b) = (pts[k], pts[(k + 1) % n]);
        v += Vec3::new(
            (a.y - b.y) * (a.z + b.z),
            (a.z - b.z) * (a.x + b.x),
            (a.x - b.x) * (a.y + b.y),
        );
    }
    v
}

fn plane_map(plane: &str) -> (Vec3, fn(f32, f32) -> Vec3) {
    match plane {
        "xy" => (Vec3::Z, |a, b| Vec3::new(a, b, 0.0)),
        "yz" => (Vec3::X, |a, b| Vec3::new(0.0, a, b)),
        _ => (Vec3::Y, |a, b| Vec3::new(a, 0.0, -b)),
    }
}

// ---------------------------------------------------------------- rectangle

pub struct RectangleNode {
    params: Vec<ParamDef>,
}
impl RectangleNode {
    pub fn new() -> Self {
        Self {
            params: vec![
                ParamDef::vec2("size", [2.0, 1.0], "Width and length"),
                ParamDef::float("corner_radius", 0.0, "Round the corners (0 = sharp)"),
                ParamDef::int("corner_segments", 6, "Points per rounded corner"),
                ParamDef::menu("plane", &["xz", "xy", "yz"], "Plane the rectangle lies in"),
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
impl NodeType for RectangleNode {
    node_header!(
        "rectangle",
        "Rectangle with optional rounded corners; extrude it for panels and table tops.",
        0,
        Some(0)
    );
    fn cook(&self, _: &[Arc<Geometry>], p: &Params) -> Result<Geometry, GraphError> {
        let size = p.vec2("size")?.abs();
        let (hx, hy) = (size.x / 2.0, size.y / 2.0);
        let r = p.f32("corner_radius")?.clamp(0.0, hx.min(hy));
        let seg = p.int_min("corner_segments", 1)?;
        let (normal, map) = plane_map(&p.menu("plane")?);
        let c = p.vec3("center")?;

        let mut flat: Vec<Vec2> = Vec::new();
        if r <= 1e-6 {
            flat.extend([
                Vec2::new(-hx, -hy),
                Vec2::new(hx, -hy),
                Vec2::new(hx, hy),
                Vec2::new(-hx, hy),
            ]);
        } else {
            // corner centres, counter-clockwise, each with its start angle
            let corners = [
                (Vec2::new(hx - r, -hy + r), -90.0f32),
                (Vec2::new(hx - r, hy - r), 0.0),
                (Vec2::new(-hx + r, hy - r), 90.0),
                (Vec2::new(-hx + r, -hy + r), 180.0),
            ];
            for (cc, a0) in corners {
                for k in 0..=seg {
                    let a = (a0 + 90.0 * k as f32 / seg as f32).to_radians();
                    flat.push(cc + Vec2::new(a.cos(), a.sin()) * r);
                }
            }
            flat.dedup_by(|a, b| a.distance(*b) < 1e-6);
        }
        let mut g = Geometry::new();
        let mut ids: Vec<u32> = flat
            .iter()
            .map(|q| g.add_point(c + map(q.x, q.y)))
            .collect();
        if p.menu("output")? == "polyline" {
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

// ---------------------------------------------------------------- extrude

pub struct ExtrudeNode {
    params: Vec<ParamDef>,
}
impl ExtrudeNode {
    pub fn new() -> Self {
        Self {
            params: vec![
                ParamDef::float("depth", 1.0, "How far to extrude"),
                ParamDef::menu(
                    "direction",
                    &["normal", "custom"],
                    "normal = along the face normal; custom = along the vector below",
                ),
                ParamDef::vec3(
                    "custom_direction",
                    [0.0, 1.0, 0.0],
                    "Direction when 'custom' is chosen (also used for open curves)",
                ),
                ParamDef::float(
                    "taper",
                    1.0,
                    "Scale of the far end (1 = straight, 0.5 = half size)",
                ),
                ParamDef::int("divisions", 1, "Segments along the extrusion"),
                ParamDef::boolean("caps", true, "Close both ends (closed shapes only)"),
            ],
        }
    }
}
impl NodeType for ExtrudeNode {
    node_header!("extrude", "Extrude closed shapes (rectangle, circle, closed curve) into solids, or open curves into walls.", 1, Some(1));
    fn cook(&self, inputs: &[Arc<Geometry>], p: &Params) -> Result<Geometry, GraphError> {
        let depth = p.f32("depth")?;
        let custom = p.menu("direction")? == "custom";
        let cdir = p.vec3("custom_direction")?.normalize_or_zero();
        let taper = p.f32("taper")?;
        let div = p.int_min("divisions", 1)?;
        let caps = p.boolean("caps")?;
        let shapes = loops(&inputs[0]);
        if shapes.is_empty() {
            return Err(p.fail("input has no shape to extrude (use rectangle, circle or curve)"));
        }
        let mut out = Geometry::new();
        for (pts, closed) in shapes {
            if pts.len() < 2 || (closed && pts.len() < 3) {
                continue;
            }
            let n = pts.len();
            let face_n = newell(&pts).normalize_or_zero();
            let dir = if custom || !closed || face_n == Vec3::ZERO {
                cdir
            } else {
                face_n
            };
            if dir == Vec3::ZERO {
                return Err(p.fail("extrude direction is zero"));
            }
            let centroid = pts.iter().copied().sum::<Vec3>() / n as f32;

            let mut g = Geometry::new();
            let mut rings: Vec<Vec<u32>> = Vec::with_capacity(div + 1);
            for k in 0..=div {
                let t = k as f32 / div as f32;
                let s = 1.0 + (taper - 1.0) * t;
                rings.push(
                    pts.iter()
                        .map(|q| g.add_point(centroid + (*q - centroid) * s + dir * depth * t))
                        .collect(),
                );
            }
            let mut arc = vec![0.0f32; n + 1];
            for i in 0..n {
                arc[i + 1] = arc[i] + pts[i].distance(pts[(i + 1) % n]);
            }
            let edges = if closed { n } else { n - 1 };
            let total = arc[edges].max(1e-6);
            let mut uvs = Vec::new();
            for k in 0..div {
                let (v0, v1) = (k as f32 / div as f32, (k + 1) as f32 / div as f32);
                for i in 0..edges {
                    let j = (i + 1) % n;
                    g.add_polygon(vec![
                        rings[k][i],
                        rings[k][j],
                        rings[k + 1][j],
                        rings[k + 1][i],
                    ]);
                    let (u0, u1) = (arc[i] / total, arc[i + 1] / total);
                    uvs.extend([
                        Vec2::new(u0, v0),
                        Vec2::new(u1, v0),
                        Vec2::new(u1, v1),
                        Vec2::new(u0, v1),
                    ]);
                }
            }
            if closed && caps {
                let mut bottom = rings[0].clone();
                bottom.reverse();
                g.add_polygon(bottom);
                g.add_polygon(rings[div].clone());
                uvs.resize(g.num_vertices(), Vec2::ZERO);
            }
            *g.vertex_vec2_mut("uv") = uvs;
            if g.topology().is_watertight() {
                g.orient_outward();
            } else if depth < 0.0 {
                g.reverse_all();
            }
            out.merge(&g);
        }
        out.compute_vertex_normals(40.0);
        Ok(out)
    }
}

// ---------------------------------------------------------------- boolean

pub struct BooleanNode {
    params: Vec<ParamDef>,
}
impl BooleanNode {
    pub fn new() -> Self {
        Self {
            params: vec![
                ParamDef::menu("operation", &["union", "subtract", "intersect"], "union = join; subtract = cut the second input out of the first; intersect = keep only the overlap"),
                ParamDef::float("cusp_angle", 30.0, "Edges sharper than this angle (degrees) stay hard"),
            ],
        }
    }
}
impl NodeType for BooleanNode {
    node_header!("boolean", "Union, subtract or intersect two closed solids. Input 1 is A, input 2 is B (subtract = A minus B).", 2, Some(2));
    fn cook(&self, inputs: &[Arc<Geometry>], p: &Params) -> Result<Geometry, GraphError> {
        let op = match p.menu("operation")?.as_str() {
            "subtract" => BoolOp::Subtract,
            "intersect" => BoolOp::Intersect,
            _ => BoolOp::Union,
        };
        for (i, g) in inputs.iter().enumerate() {
            if !g.prims.iter().any(|pr| pr.kind == PrimKind::Polygon) {
                return Err(p.fail(format!(
                    "input {} has no faces; booleans need solids",
                    i + 1
                )));
            }
        }
        let mut g = boolean(&inputs[0], &inputs[1], op);
        g.compute_vertex_normals(p.f32("cusp_angle")?);
        Ok(g)
    }
}

// ---------------------------------------------------------------- sweep

pub struct SweepNode {
    params: Vec<ParamDef>,
}
impl SweepNode {
    pub fn new() -> Self {
        Self {
            params: vec![
                ParamDef::float("scale", 1.0, "Scale of the profile"),
                ParamDef::float("twist", 0.0, "Total twist along the path, degrees"),
                ParamDef::boolean(
                    "caps",
                    true,
                    "Close the ends (closed profile on an open path)",
                ),
            ],
        }
    }
}
impl NodeType for SweepNode {
    node_header!("sweep", "Sweep a profile (input 1, drawn in the XY plane around the origin) along a path curve (input 2): pipes, handles, rails.", 2, Some(2));
    fn cook(&self, inputs: &[Arc<Geometry>], p: &Params) -> Result<Geometry, GraphError> {
        let (profile, pclosed) = loops(&inputs[0])
            .into_iter()
            .next()
            .ok_or_else(|| p.fail("input 1 has no profile curve"))?;
        let (path, path_closed) = loops(&inputs[1])
            .into_iter()
            .next()
            .ok_or_else(|| p.fail("input 2 has no path curve"))?;
        if path.len() < 2 || profile.len() < 2 {
            return Err(p.fail("profile and path need at least 2 points"));
        }
        let scale = p.f32("scale")?;
        let twist = p.f32("twist")?.to_radians();
        let frames = sweep_frames(&path, path_closed);
        let m = path.len();
        let np = profile.len();

        let mut g = Geometry::new();
        let rings: Vec<Vec<u32>> = (0..m)
            .map(|i| {
                let (t, r) = frames[i];
                let angle = if m > 1 {
                    twist * i as f32 / (m - 1) as f32
                } else {
                    0.0
                };
                let r = Quat::from_axis_angle(t, angle) * r;
                let s = t.cross(r);
                profile
                    .iter()
                    .map(|q| g.add_point(path[i] + (r * q.x + s * q.y) * scale))
                    .collect()
            })
            .collect();

        let mut parc = vec![0.0f32; np + 1];
        for k in 0..np {
            parc[k + 1] = parc[k] + profile[k].distance(profile[(k + 1) % np]);
        }
        let pedges = if pclosed { np } else { np - 1 };
        let mut larc = vec![0.0f32; m + 1];
        for i in 0..m {
            larc[i + 1] = larc[i] + path[i].distance(path[(i + 1) % m]);
        }
        let segs = if path_closed { m } else { m - 1 };
        let (pt, lt) = (parc[pedges].max(1e-6), larc[segs].max(1e-6));
        let mut uvs = Vec::new();
        for i in 0..segs {
            let i1 = (i + 1) % m;
            for k in 0..pedges {
                let k1 = (k + 1) % np;
                g.add_polygon(vec![rings[i][k], rings[i][k1], rings[i1][k1], rings[i1][k]]);
                let (u0, u1, v0, v1) = (
                    parc[k] / pt,
                    parc[k + 1] / pt,
                    larc[i] / lt,
                    larc[i + 1] / lt,
                );
                uvs.extend([
                    Vec2::new(u0, v0),
                    Vec2::new(u1, v0),
                    Vec2::new(u1, v1),
                    Vec2::new(u0, v1),
                ]);
            }
        }
        if pclosed && !path_closed && p.boolean("caps")? {
            let mut start = rings[0].clone();
            start.reverse();
            g.add_polygon(start);
            g.add_polygon(rings[m - 1].clone());
            uvs.resize(g.num_vertices(), Vec2::ZERO);
        }
        *g.vertex_vec2_mut("uv") = uvs;
        if g.topology().is_watertight() {
            g.orient_outward();
        }
        g.compute_vertex_normals(50.0);
        Ok(g)
    }
}

/// Rotation-minimising frames (tangent, normal) along a path (double reflection method).
fn sweep_frames(path: &[Vec3], closed: bool) -> Vec<(Vec3, Vec3)> {
    let m = path.len();
    let tangent = |i: usize| -> Vec3 {
        let (a, b) = if closed {
            (path[(i + m - 1) % m], path[(i + 1) % m])
        } else if i == 0 {
            (path[0], path[1])
        } else if i == m - 1 {
            (path[m - 2], path[m - 1])
        } else {
            (path[i - 1], path[i + 1])
        };
        (b - a).normalize_or(Vec3::Z)
    };
    let t0 = tangent(0);
    let reference = if t0.dot(Vec3::Y).abs() < 0.9 {
        Vec3::Y
    } else {
        Vec3::X
    };
    let r0 = (reference - t0 * t0.dot(reference)).normalize();
    let mut frames = vec![(t0, r0)];
    let steps = if closed { m } else { m - 1 };
    for i in 0..steps {
        let (ti, ri) = frames[i];
        let j = (i + 1) % m;
        let v1 = path[j] - path[i];
        let c1 = v1.dot(v1);
        let (rl, tl) = if c1 < 1e-12 {
            (ri, ti)
        } else {
            (
                ri - v1 * (2.0 / c1) * v1.dot(ri),
                ti - v1 * (2.0 / c1) * v1.dot(ti),
            )
        };
        let tj = tangent(j);
        let v2 = tj - tl;
        let c2 = v2.dot(v2);
        let rj = if c2 < 1e-12 {
            rl
        } else {
            rl - v2 * (2.0 / c2) * v2.dot(rl)
        };
        frames.push((tj, (rj - tj * tj.dot(rj)).normalize_or(r0)));
    }
    if closed {
        // The frame carried all the way round may not line up with the start: spread the difference.
        let (_, r_end) = frames[m];
        let angle = r0.cross(r_end).dot(t0).atan2(r0.dot(r_end));
        frames.truncate(m);
        for (i, f) in frames.iter_mut().enumerate() {
            f.1 = Quat::from_axis_angle(f.0, -angle * i as f32 / m as f32) * f.1;
        }
    }
    frames
}
