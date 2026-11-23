use std::collections::{BTreeMap, HashMap};

use glam::{Mat3, Mat4, Vec2, Vec3};

use crate::attrib::{AttribArray, AttribKind, AttribValue};

/// Kind of primitive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimKind {
    /// Closed polygon face.
    Polygon,
    /// Open polyline (curve).
    Polyline,
}

/// A primitive: an ordered list of point references (its vertices).
#[derive(Clone, Debug, PartialEq)]
pub struct Primitive {
    pub kind: PrimKind,
    /// Point index of each vertex.
    pub points: Vec<u32>,
    /// Index of this primitive's first vertex in the vertex attribute arrays.
    pub vertex_start: usize,
}

impl Primitive {
    pub fn vertex_range(&self) -> std::ops::Range<usize> {
        self.vertex_start..self.vertex_start + self.points.len()
    }
}

/// Result of a topology check.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TopologyReport {
    /// Edges used by exactly one polygon (holes / open borders).
    pub boundary_edges: usize,
    /// Edges used by more than two polygons.
    pub nonmanifold_edges: usize,
    /// Edges used twice in the same direction (inconsistent winding).
    pub flipped_edges: usize,
}

impl TopologyReport {
    /// Closed, manifold and consistently oriented.
    pub fn is_watertight(&self) -> bool {
        self.boundary_edges == 0 && self.nonmanifold_edges == 0 && self.flipped_edges == 0
    }
}

/// Points + primitives + attributes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Geometry {
    /// Point positions (the `P` attribute).
    pub points: Vec<Vec3>,
    pub point_attribs: BTreeMap<String, AttribArray>,
    pub prims: Vec<Primitive>,
    pub prim_attribs: BTreeMap<String, AttribArray>,
    /// Indexed by global vertex number (see [`Primitive::vertex_start`]).
    pub vertex_attribs: BTreeMap<String, AttribArray>,
    pub detail_attribs: BTreeMap<String, AttribValue>,
    /// Named sets of point indices.
    pub point_groups: BTreeMap<String, Vec<u32>>,
    /// Named sets of primitive indices.
    pub prim_groups: BTreeMap<String, Vec<u32>>,
    num_vertices: usize,
}

fn ensure<'a>(
    map: &'a mut BTreeMap<String, AttribArray>,
    name: &str,
    kind: AttribKind,
    n: usize,
) -> &'a mut AttribArray {
    let replace = match map.get(name) {
        Some(a) => a.kind() != kind,
        None => true,
    };
    if replace {
        map.insert(name.to_string(), AttribArray::zeros(kind, n));
    }
    map.get_mut(name).expect("just inserted")
}

macro_rules! typed_access {
    ($fn_name:ident, $field:ident, $count:ident, $variant:ident, $ty:ty) => {
        /// Get (creating it if needed) a typed attribute array.
        pub fn $fn_name(&mut self, name: &str) -> &mut Vec<$ty> {
            let n = self.$count();
            match ensure(&mut self.$field, name, AttribKind::$variant, n) {
                AttribArray::$variant(v) => v,
                _ => unreachable!(),
            }
        }
    };
}

impl Geometry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn num_points(&self) -> usize {
        self.points.len()
    }
    pub fn num_prims(&self) -> usize {
        self.prims.len()
    }
    pub fn num_vertices(&self) -> usize {
        self.num_vertices
    }

    /// Add a point; all point attributes get a default value for it.
    pub fn add_point(&mut self, p: Vec3) -> u32 {
        self.points.push(p);
        let n = self.points.len();
        for a in self.point_attribs.values_mut() {
            a.resize(n);
        }
        (n - 1) as u32
    }

    /// Add a primitive referencing existing points.
    pub fn add_prim(&mut self, kind: PrimKind, points: Vec<u32>) -> u32 {
        debug_assert!(points.iter().all(|&p| (p as usize) < self.points.len()));
        let start = self.num_vertices;
        self.num_vertices += points.len();
        self.prims.push(Primitive {
            kind,
            points,
            vertex_start: start,
        });
        for a in self.vertex_attribs.values_mut() {
            a.resize(self.num_vertices);
        }
        let n = self.prims.len();
        for a in self.prim_attribs.values_mut() {
            a.resize(n);
        }
        (n - 1) as u32
    }

    pub fn add_polygon(&mut self, points: Vec<u32>) -> u32 {
        self.add_prim(PrimKind::Polygon, points)
    }

    pub fn add_polyline(&mut self, points: Vec<u32>) -> u32 {
        self.add_prim(PrimKind::Polyline, points)
    }

    typed_access!(point_float_mut, point_attribs, num_points, Float, f32);
    typed_access!(point_vec2_mut, point_attribs, num_points, Vec2, Vec2);
    typed_access!(point_vec3_mut, point_attribs, num_points, Vec3, Vec3);
    typed_access!(vertex_float_mut, vertex_attribs, num_vertices, Float, f32);
    typed_access!(vertex_vec2_mut, vertex_attribs, num_vertices, Vec2, Vec2);
    typed_access!(vertex_vec3_mut, vertex_attribs, num_vertices, Vec3, Vec3);
    typed_access!(prim_float_mut, prim_attribs, num_prims, Float, f32);
    typed_access!(prim_int_mut, prim_attribs, num_prims, Int, i64);
    typed_access!(prim_str_mut, prim_attribs, num_prims, Str, String);

    /// Append another geometry into this one (points, prims, attributes, groups).
    pub fn merge(&mut self, other: &Geometry) {
        let p_off = self.points.len() as u32;
        let pr_off = self.prims.len() as u32;
        let v_off = self.num_vertices;

        merge_attribs(
            &mut self.point_attribs,
            &other.point_attribs,
            self.points.len(),
            other.points.len(),
        );
        merge_attribs(
            &mut self.vertex_attribs,
            &other.vertex_attribs,
            self.num_vertices,
            other.num_vertices,
        );
        merge_attribs(
            &mut self.prim_attribs,
            &other.prim_attribs,
            self.prims.len(),
            other.prims.len(),
        );

        self.points.extend_from_slice(&other.points);
        for pr in &other.prims {
            self.prims.push(Primitive {
                kind: pr.kind,
                points: pr.points.iter().map(|p| p + p_off).collect(),
                vertex_start: pr.vertex_start + v_off,
            });
        }
        self.num_vertices += other.num_vertices;

        for (k, v) in &other.detail_attribs {
            self.detail_attribs
                .entry(k.clone())
                .or_insert_with(|| v.clone());
        }
        for (k, ids) in &other.point_groups {
            self.point_groups
                .entry(k.clone())
                .or_default()
                .extend(ids.iter().map(|i| i + p_off));
        }
        for (k, ids) in &other.prim_groups {
            self.prim_groups
                .entry(k.clone())
                .or_default()
                .extend(ids.iter().map(|i| i + pr_off));
        }
    }

    /// Apply an affine transform to positions and to `N` attributes.
    pub fn transform(&mut self, m: Mat4) {
        for p in &mut self.points {
            *p = m.transform_point3(*p);
        }
        let nm = Mat3::from_mat4(m).inverse().transpose();
        for map in [&mut self.point_attribs, &mut self.vertex_attribs] {
            if let Some(AttribArray::Vec3(ns)) = map.get_mut("N") {
                for n in ns.iter_mut() {
                    *n = (nm * *n).normalize_or_zero();
                }
            }
        }
        // A mirroring transform turns faces inside-out; keep them facing outward.
        if m.determinant() < 0.0 {
            self.reverse_all();
        }
    }

    /// Reverse the winding of one primitive (and its vertex attributes).
    pub fn reverse_prim(&mut self, i: usize) {
        let range = self.prims[i].vertex_range();
        self.prims[i].points.reverse();
        for a in self.vertex_attribs.values_mut() {
            a.reverse_range(range.clone());
        }
    }

    pub fn reverse_all(&mut self) {
        for i in 0..self.prims.len() {
            if self.prims[i].kind == PrimKind::Polygon {
                self.reverse_prim(i);
            }
        }
    }

    /// Axis-aligned bounds (min, max). Returns zeros for empty geometry.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        if self.points.is_empty() {
            return (Vec3::ZERO, Vec3::ZERO);
        }
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        for p in &self.points {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        (lo, hi)
    }

    /// Newell normal of a primitive, unnormalised: its length is twice the polygon area.
    pub fn prim_area_normal(&self, i: usize) -> Vec3 {
        let pts = &self.prims[i].points;
        let mut n = Vec3::ZERO;
        for k in 0..pts.len() {
            let a = self.points[pts[k] as usize];
            let b = self.points[pts[(k + 1) % pts.len()] as usize];
            n += Vec3::new(
                (a.y - b.y) * (a.z + b.z),
                (a.z - b.z) * (a.x + b.x),
                (a.x - b.x) * (a.y + b.y),
            );
        }
        n
    }

    pub fn prim_normal(&self, i: usize) -> Vec3 {
        self.prim_area_normal(i).normalize_or_zero()
    }

    pub fn prim_centroid(&self, i: usize) -> Vec3 {
        let pts = &self.prims[i].points;
        pts.iter().map(|&p| self.points[p as usize]).sum::<Vec3>() / pts.len().max(1) as f32
    }

    /// Compute vertex normals (`N` on vertices). Faces meeting at an angle larger than
    /// `cusp_degrees` keep a hard edge; smaller angles are smoothed.
    pub fn compute_vertex_normals(&mut self, cusp_degrees: f32) {
        let face_area_n: Vec<Vec3> = (0..self.prims.len())
            .map(|i| {
                if self.prims[i].kind == PrimKind::Polygon {
                    self.prim_area_normal(i)
                } else {
                    Vec3::ZERO
                }
            })
            .collect();
        let face_n: Vec<Vec3> = face_area_n.iter().map(|n| n.normalize_or_zero()).collect();
        let mut adjacent: Vec<Vec<u32>> = vec![Vec::new(); self.points.len()];
        for (pi, pr) in self.prims.iter().enumerate() {
            if pr.kind != PrimKind::Polygon {
                continue;
            }
            for &pt in &pr.points {
                let list = &mut adjacent[pt as usize];
                if list.last() != Some(&(pi as u32)) {
                    list.push(pi as u32);
                }
            }
        }
        let cos_cusp = cusp_degrees.to_radians().cos();
        let mut out = vec![Vec3::ZERO; self.num_vertices];
        for (pi, pr) in self.prims.iter().enumerate() {
            if pr.kind != PrimKind::Polygon {
                continue;
            }
            let own = face_n[pi];
            for (k, &pt) in pr.points.iter().enumerate() {
                let mut n = Vec3::ZERO;
                for &q in &adjacent[pt as usize] {
                    if face_n[q as usize].dot(own) >= cos_cusp {
                        n += face_area_n[q as usize];
                    }
                }
                out[pr.vertex_start + k] = if n.length_squared() > 0.0 {
                    n.normalize()
                } else {
                    own
                };
            }
        }
        self.vertex_attribs
            .insert("N".into(), AttribArray::Vec3(out));
    }

    /// Check edges: open borders, non-manifold edges and inconsistent winding.
    pub fn topology(&self) -> TopologyReport {
        // key: (min, max) -> (count, count in min->max direction)
        let mut edges: HashMap<(u32, u32), (u32, u32)> = HashMap::new();
        for pr in &self.prims {
            if pr.kind != PrimKind::Polygon {
                continue;
            }
            let n = pr.points.len();
            for k in 0..n {
                let a = pr.points[k];
                let b = pr.points[(k + 1) % n];
                if a == b {
                    continue;
                }
                let e = edges.entry((a.min(b), a.max(b))).or_insert((0, 0));
                e.0 += 1;
                if a < b {
                    e.1 += 1;
                }
            }
        }
        let mut r = TopologyReport::default();
        for (count, forward) in edges.values() {
            match count {
                1 => r.boundary_edges += 1,
                2 => {
                    if *forward != 1 {
                        r.flipped_edges += 1
                    }
                }
                _ => r.nonmanifold_edges += 1,
            }
        }
        r
    }

    /// Signed volume enclosed by the polygons (positive when faces point outward).
    /// Only meaningful for closed meshes.
    pub fn signed_volume(&self) -> f32 {
        let mut v = 0.0;
        for pr in &self.prims {
            if pr.kind != PrimKind::Polygon || pr.points.len() < 3 {
                continue;
            }
            let p0 = self.points[pr.points[0] as usize];
            for k in 1..pr.points.len() - 1 {
                let p1 = self.points[pr.points[k] as usize];
                let p2 = self.points[pr.points[k + 1] as usize];
                v += p0.dot(p1.cross(p2));
            }
        }
        v / 6.0
    }

    /// For a closed mesh: flip every face if the mesh is inside-out.
    pub fn orient_outward(&mut self) {
        if self.signed_volume() < 0.0 {
            self.reverse_all();
        }
    }

    /// Read a Vec2 vertex attribute value if present.
    pub fn vertex_vec2(&self, name: &str, v: usize) -> Option<Vec2> {
        match self.vertex_attribs.get(name) {
            Some(AttribArray::Vec2(a)) => a.get(v).copied(),
            _ => None,
        }
    }
}

fn merge_attribs(
    dst: &mut BTreeMap<String, AttribArray>,
    src: &BTreeMap<String, AttribArray>,
    dst_len: usize,
    src_len: usize,
) {
    for (name, s) in src {
        let d = dst
            .entry(name.clone())
            .or_insert_with(|| AttribArray::zeros(s.kind(), dst_len));
        if !d.append(s) {
            // Type mismatch: keep the existing attribute, pad with defaults.
            d.resize(dst_len + src_len);
        }
    }
    for d in dst.values_mut() {
        d.resize(dst_len + src_len);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tetra() -> Geometry {
        let mut g = Geometry::new();
        let a = g.add_point(Vec3::new(0.0, 0.0, 0.0));
        let b = g.add_point(Vec3::new(1.0, 0.0, 0.0));
        let c = g.add_point(Vec3::new(0.0, 1.0, 0.0));
        let d = g.add_point(Vec3::new(0.0, 0.0, 1.0));
        g.add_polygon(vec![a, c, b]);
        g.add_polygon(vec![a, b, d]);
        g.add_polygon(vec![a, d, c]);
        g.add_polygon(vec![b, c, d]);
        g
    }

    #[test]
    fn tetra_is_watertight_and_outward() {
        let g = tetra();
        assert!(g.topology().is_watertight(), "{:?}", g.topology());
        assert!((g.signed_volume() - 1.0 / 6.0).abs() < 1e-6);
    }

    #[test]
    fn merge_offsets_and_attribs() {
        let mut a = tetra();
        a.point_float_mut("w")[0] = 2.0;
        let mut b = tetra();
        b.vertex_vec2_mut("uv")[0] = Vec2::ONE;
        a.merge(&b);
        assert_eq!(a.num_points(), 8);
        assert_eq!(a.num_prims(), 8);
        assert_eq!(a.num_vertices(), 24);
        assert_eq!(a.point_attribs["w"].len(), 8);
        assert_eq!(a.vertex_attribs["uv"].len(), 24);
        assert_eq!(a.vertex_vec2("uv", 12), Some(Vec2::ONE));
        assert!(a.topology().is_watertight());
    }

    #[test]
    fn mirror_keeps_outward() {
        let mut g = tetra();
        g.transform(Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0)));
        assert!(g.signed_volume() > 0.0);
        assert!(g.topology().is_watertight());
    }

    #[test]
    fn detects_hole() {
        let mut g = tetra();
        g.prims.pop();
        assert_eq!(g.topology().boundary_edges, 3);
    }
}
