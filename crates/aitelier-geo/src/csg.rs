//! Boolean operations (union, subtract, intersect) on closed meshes using BSP trees.
//!
//! This follows the classic csg.js algorithm. Trees are stored in a flat arena and
//! walked with explicit stacks (no recursion), so large meshes cannot overflow the
//! stack. Results are welded and their T-junctions repaired, so solid inputs give
//! watertight outputs.

use glam::{DVec3, Vec3};

use crate::geometry::{Geometry, PrimKind};
use crate::mesh_ops::{fix_t_junctions, remove_degenerate, triangulate, weld};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoolOp {
    Union,
    Subtract,
    Intersect,
}

const EPS: f64 = 1e-6;

#[derive(Clone, Copy, Debug)]
struct Plane {
    n: DVec3,
    w: f64,
}

impl Plane {
    fn from_points(a: DVec3, b: DVec3, c: DVec3) -> Option<Plane> {
        let n = (b - a).cross(c - a);
        let len = n.length();
        if len < 1e-14 {
            return None;
        }
        let n = n / len;
        Some(Plane { n, w: n.dot(a) })
    }
    fn flip(&mut self) {
        self.n = -self.n;
        self.w = -self.w;
    }
}

#[derive(Clone, Debug)]
struct Poly {
    verts: Vec<DVec3>,
    plane: Plane,
}

impl Poly {
    fn flip(&mut self) {
        self.verts.reverse();
        self.plane.flip();
    }
}

const COPLANAR: u8 = 0;
const FRONT: u8 = 1;
const BACK: u8 = 2;
const SPANNING: u8 = 3;

/// Split `poly` by `plane` into the four output lists.
fn split(
    plane: &Plane,
    poly: Poly,
    eps: f64,
    cf: &mut Vec<Poly>,
    cb: &mut Vec<Poly>,
    front: &mut Vec<Poly>,
    back: &mut Vec<Poly>,
) {
    let mut kind = 0u8;
    let types: Vec<u8> = poly
        .verts
        .iter()
        .map(|v| {
            let t = plane.n.dot(*v) - plane.w;
            let ty = if t < -eps {
                BACK
            } else if t > eps {
                FRONT
            } else {
                COPLANAR
            };
            kind |= ty;
            ty
        })
        .collect();
    match kind {
        COPLANAR => {
            if plane.n.dot(poly.plane.n) > 0.0 {
                cf.push(poly)
            } else {
                cb.push(poly)
            }
        }
        FRONT => front.push(poly),
        BACK => back.push(poly),
        _ => {
            let n = poly.verts.len();
            let (mut f, mut b) = (Vec::new(), Vec::new());
            for i in 0..n {
                let j = (i + 1) % n;
                let (ti, tj) = (types[i], types[j]);
                let (vi, vj) = (poly.verts[i], poly.verts[j]);
                if ti != BACK {
                    f.push(vi);
                }
                if ti != FRONT {
                    b.push(vi);
                }
                if (ti | tj) == SPANNING {
                    let t = (plane.w - plane.n.dot(vi)) / plane.n.dot(vj - vi);
                    let v = vi.lerp(vj, t);
                    f.push(v);
                    b.push(v);
                }
            }
            if f.len() >= 3 {
                front.push(Poly {
                    verts: f,
                    plane: poly.plane,
                });
            }
            if b.len() >= 3 {
                back.push(Poly {
                    verts: b,
                    plane: poly.plane,
                });
            }
        }
    }
}

#[derive(Default, Clone)]
struct Node {
    plane: Option<Plane>,
    front: Option<usize>,
    back: Option<usize>,
    polys: Vec<Poly>,
}

/// BSP tree in an arena; node 0 is the root.
#[derive(Clone)]
struct Bsp {
    nodes: Vec<Node>,
    eps: f64,
}

impl Bsp {
    fn new(polys: Vec<Poly>, eps: f64) -> Self {
        let mut t = Bsp {
            nodes: vec![Node::default()],
            eps,
        };
        t.build(0, polys);
        t
    }

    fn build(&mut self, root: usize, polys: Vec<Poly>) {
        let mut stack = vec![(root, polys)];
        while let Some((idx, polys)) = stack.pop() {
            if polys.is_empty() {
                continue;
            }
            if self.nodes[idx].plane.is_none() {
                self.nodes[idx].plane = Some(polys[0].plane);
            }
            let plane = self.nodes[idx].plane.expect("set");
            let (mut cf, mut cb, mut f, mut b) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
            for p in polys {
                split(&plane, p, self.eps, &mut cf, &mut cb, &mut f, &mut b);
            }
            self.nodes[idx].polys.extend(cf);
            self.nodes[idx].polys.extend(cb);
            if !f.is_empty() {
                let child = self.child(idx, true);
                stack.push((child, f));
            }
            if !b.is_empty() {
                let child = self.child(idx, false);
                stack.push((child, b));
            }
        }
    }

    fn child(&mut self, idx: usize, front: bool) -> usize {
        let existing = if front {
            self.nodes[idx].front
        } else {
            self.nodes[idx].back
        };
        if let Some(c) = existing {
            return c;
        }
        self.nodes.push(Node::default());
        let c = self.nodes.len() - 1;
        if front {
            self.nodes[idx].front = Some(c);
        } else {
            self.nodes[idx].back = Some(c);
        }
        c
    }

    fn invert(&mut self) {
        for n in &mut self.nodes {
            for p in &mut n.polys {
                p.flip();
            }
            if let Some(pl) = n.plane.as_mut() {
                pl.flip();
            }
            std::mem::swap(&mut n.front, &mut n.back);
        }
    }

    /// Remove the parts of `polys` that are inside this solid.
    fn clip_polygons(&self, polys: Vec<Poly>) -> Vec<Poly> {
        let mut out = Vec::new();
        let mut stack = vec![(0usize, polys)];
        while let Some((idx, polys)) = stack.pop() {
            let node = &self.nodes[idx];
            let Some(plane) = node.plane else {
                out.extend(polys);
                continue;
            };
            let (mut cf, mut cb, mut f, mut b) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
            for p in polys {
                split(&plane, p, self.eps, &mut cf, &mut cb, &mut f, &mut b);
            }
            f.extend(cf);
            b.extend(cb);
            match node.front {
                Some(c) => stack.push((c, f)),
                None => out.extend(f),
            }
            if let Some(c) = node.back {
                stack.push((c, b));
            } // else: behind a leaf = inside the solid, discarded
        }
        out
    }
}

/// Polygons of a geometry (triangulated, so they are convex) in double precision.
fn to_polys(geo: &Geometry) -> Vec<Poly> {
    let mut out = Vec::new();
    for pr in &geo.prims {
        if pr.kind != PrimKind::Polygon || pr.points.len() < 3 {
            continue;
        }
        let pts: Vec<Vec3> = pr.points.iter().map(|&p| geo.points[p as usize]).collect();
        for [a, b, c] in triangulate(&pts) {
            let (a, b, c) = (pts[a].as_dvec3(), pts[b].as_dvec3(), pts[c].as_dvec3());
            if let Some(plane) = Plane::from_points(a, b, c) {
                out.push(Poly {
                    verts: vec![a, b, c],
                    plane,
                });
            }
        }
    }
    out
}

/// Boolean of two closed meshes. The result is welded and watertight when both inputs are.
pub fn boolean(a: &Geometry, b: &Geometry, op: BoolOp) -> Geometry {
    let (raw, scale) = bsp_boolean(a, b, op);
    let weld_eps = (scale * 1e-5) as f32;
    let welded = weld(&raw, weld_eps);
    let mut out = remove_degenerate(&welded, weld_eps);
    fix_t_junctions(&mut out, weld_eps);
    out
}

/// Topology after each cleanup stage (for debugging).
#[doc(hidden)]
pub fn boolean_stages(
    a: &Geometry,
    b: &Geometry,
    op: BoolOp,
) -> Vec<(&'static str, usize, crate::geometry::TopologyReport)> {
    let (raw, scale) = bsp_boolean(a, b, op);
    let weld_eps = (scale * 1e-5) as f32;
    let welded = weld(&raw, weld_eps);
    let clean = remove_degenerate(&welded, weld_eps);
    let mut fixed = clean.clone();
    fix_t_junctions(&mut fixed, weld_eps);
    vec![
        ("raw", raw.num_prims(), raw.topology()),
        ("welded", welded.num_prims(), welded.topology()),
        ("no slivers", clean.num_prims(), clean.topology()),
        ("cracks fixed", fixed.num_prims(), fixed.topology()),
    ]
}

fn flip_all(mut polys: Vec<Poly>) -> Vec<Poly> {
    for p in &mut polys {
        p.flip();
    }
    polys
}

/// Bounding box of a solid's polygons, grown by `pad`.
fn bbox(polys: &[Poly], pad: f64) -> (DVec3, DVec3) {
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    for p in polys {
        for v in &p.verts {
            lo = lo.min(*v);
            hi = hi.max(*v);
        }
    }
    (lo - DVec3::splat(pad), hi + DVec3::splat(pad))
}

fn touches(p: &Poly, (lo, hi): (DVec3, DVec3)) -> bool {
    let mut plo = DVec3::splat(f64::INFINITY);
    let mut phi = DVec3::splat(f64::NEG_INFINITY);
    for v in &p.verts {
        plo = plo.min(*v);
        phi = phi.max(*v);
    }
    plo.cmple(hi).all() && phi.cmpge(lo).all()
}

/// Clip `polys` with `tree`. Polygons that cannot touch the tree's solid (outside its
/// bounding box) skip the tree: they are kept when clipping with a normal tree (they are
/// outside the solid) and dropped with an inverted tree (they are not inside it).
/// Leaving faces away from the intersection untouched avoids needless slivers.
fn clip(tree: &Bsp, inverted: bool, bounds: (DVec3, DVec3), polys: Vec<Poly>) -> Vec<Poly> {
    let (near, far): (Vec<Poly>, Vec<Poly>) = polys.into_iter().partition(|p| touches(p, bounds));
    let mut out = tree.clip_polygons(near);
    if !inverted {
        out.extend(far);
    }
    out
}

/// The BSP boolean itself: unwelded polygons, plus the scene scale.
fn bsp_boolean(a: &Geometry, b: &Geometry, op: BoolOp) -> (Geometry, f64) {
    let (lo_a, hi_a) = a.bounds();
    let (lo_b, hi_b) = b.bounds();
    let scale = (hi_a - lo_a).max(hi_b - lo_b).max_element().max(1e-3) as f64;
    let eps = EPS * scale.max(1.0);

    let (pa, pb) = (to_polys(a), to_polys(b));
    let (bba, bbb) = (bbox(&pa, eps * 10.0), bbox(&pb, eps * 10.0));
    // The trees only classify points as inside/outside; output is built from the original faces.
    let ta = Bsp::new(pa.clone(), eps);
    let tb = Bsp::new(pb.clone(), eps);
    let mut tai = ta.clone();
    tai.invert();
    let mut tbi = tb.clone();
    tbi.invert();

    // Same steps as csg.js, including its trick for faces two solids share.
    let result: Vec<Poly> = match op {
        BoolOp::Union => {
            let a1 = clip(&tb, false, bbb, pa);
            let b1 = clip(&ta, false, bba, pb);
            let b2 = flip_all(clip(&ta, false, bba, flip_all(b1)));
            a1.into_iter().chain(b2).collect()
        }
        BoolOp::Subtract => {
            let a1 = clip(&tb, false, bbb, flip_all(pa));
            let b1 = clip(&tai, true, bba, pb);
            let b2 = clip(&tai, true, bba, flip_all(b1));
            flip_all(a1).into_iter().chain(b2).collect()
        }
        BoolOp::Intersect => {
            let b1 = clip(&tai, true, bba, pb);
            let a1 = clip(&tbi, true, bbb, flip_all(pa));
            let b2 = clip(&tai, true, bba, flip_all(b1));
            flip_all(a1).into_iter().chain(flip_all(b2)).collect()
        }
    };

    let mut raw = Geometry::new();
    for p in result {
        let ids: Vec<u32> = p.verts.iter().map(|v| raw.add_point(v.as_vec3())).collect();
        raw.add_polygon(ids);
    }
    (raw, scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube(center: Vec3, size: f32) -> Geometry {
        let h = size / 2.0;
        let mut g = Geometry::new();
        for i in 0..8u32 {
            let s = |bit: u32| if i & bit != 0 { h } else { -h };
            g.add_point(center + Vec3::new(s(1), s(2), s(4)));
        }
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
        g
    }

    fn check(op: BoolOp, expected: f32) {
        let a = cube(Vec3::ZERO, 2.0);
        let b = cube(Vec3::new(1.0, 1.0, 1.0), 2.0); // overlaps a in a 1x1x1 corner
        let r = boolean(&a, &b, op);
        let v = r.signed_volume();
        assert!(
            (v - expected).abs() < 1e-3,
            "{op:?}: volume {v}, expected {expected}"
        );
        assert!(r.topology().is_watertight(), "{op:?}: {:?}", r.topology());
    }

    #[test]
    fn union_volume() {
        check(BoolOp::Union, 15.0);
    }

    #[test]
    fn subtract_volume() {
        check(BoolOp::Subtract, 7.0);
    }

    #[test]
    fn intersect_volume() {
        check(BoolOp::Intersect, 1.0);
    }

    #[test]
    fn hole_through_plate() {
        let plate = cube(Vec3::ZERO, 4.0);
        let mut pin = cube(Vec3::ZERO, 1.0);
        pin.transform(glam::Mat4::from_scale(Vec3::new(1.0, 6.0, 1.0)));
        let r = boolean(&plate, &pin, BoolOp::Subtract);
        assert!(
            (r.signed_volume() - (64.0 - 4.0)).abs() < 1e-3,
            "{}",
            r.signed_volume()
        );
        assert!(r.topology().is_watertight(), "{:?}", r.topology());
    }
}
