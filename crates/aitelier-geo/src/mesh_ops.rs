//! Mesh utilities: polygon triangulation, welding duplicate points, and repairing
//! T-junctions (cracks where one face's corner lies on another face's edge).

use std::collections::HashMap;

use glam::{Vec2, Vec3};

use crate::geometry::{Geometry, PrimKind};

/// Triangulate a planar polygon (convex or concave, no holes) by ear clipping.
/// Returns index triples into `pts`, wound the same way as the polygon.
pub fn triangulate(pts: &[Vec3]) -> Vec<[usize; 3]> {
    let n = pts.len();
    if n < 3 {
        return vec![];
    }
    if n == 3 {
        return vec![[0, 1, 2]];
    }
    // Project onto the plane of the polygon's dominant normal axis.
    let mut normal = Vec3::ZERO;
    for k in 0..n {
        let (a, b) = (pts[k], pts[(k + 1) % n]);
        normal += Vec3::new(
            (a.y - b.y) * (a.z + b.z),
            (a.z - b.z) * (a.x + b.x),
            (a.x - b.x) * (a.y + b.y),
        );
    }
    let ax = normal.abs();
    let flat: Vec<Vec2> = if ax.x >= ax.y && ax.x >= ax.z {
        pts.iter()
            .map(|p| Vec2::new(p.y, p.z * normal.x.signum()))
            .collect()
    } else if ax.y >= ax.z {
        pts.iter()
            .map(|p| Vec2::new(p.z, p.x * normal.y.signum()))
            .collect()
    } else {
        pts.iter()
            .map(|p| Vec2::new(p.x, p.y * normal.z.signum()))
            .collect()
    };
    // Viewed from the normal's side these axes are right-handed, so the polygon is counter-clockwise.
    let cross = |o: Vec2, a: Vec2, b: Vec2| (a - o).perp_dot(b - o);

    // Fast path: convex polygons use a fan.
    if (0..n).all(|i| cross(flat[i], flat[(i + 1) % n], flat[(i + 2) % n]) >= -1e-12) {
        return (1..n - 1).map(|k| [0, k, k + 1]).collect();
    }

    let mut idx: Vec<usize> = (0..n).collect();
    let mut tris = Vec::with_capacity(n - 2);
    let mut guard = 0;
    while idx.len() > 3 && guard < n * n {
        guard += 1;
        let m = idx.len();
        let mut clipped = false;
        for i in 0..m {
            let (ia, ib, ic) = (idx[(i + m - 1) % m], idx[i], idx[(i + 1) % m]);
            let (a, b, c) = (flat[ia], flat[ib], flat[ic]);
            if cross(a, b, c) <= 1e-12 {
                continue; // reflex or degenerate corner
            }
            let inside = idx.iter().any(|&j| {
                if j == ia || j == ib || j == ic {
                    return false;
                }
                let p = flat[j];
                cross(a, b, p) >= 0.0 && cross(b, c, p) >= 0.0 && cross(c, a, p) >= 0.0
            });
            if !inside {
                tris.push([ia, ib, ic]);
                idx.remove(i);
                clipped = true;
                break;
            }
        }
        if !clipped {
            break; // self-intersecting input: fall back to a fan for the rest
        }
    }
    if idx.len() >= 3 {
        for k in 1..idx.len() - 1 {
            tris.push([idx[0], idx[k], idx[k + 1]]);
        }
    }
    tris
}

/// Merge points closer than `eps` into one, update primitives, and drop polygons that
/// collapse to fewer than 3 distinct points. Point attributes keep the first point's value.
/// Vertex attributes are dropped (welding is used on fresh boolean results).
pub fn weld(geo: &Geometry, eps: f32) -> Geometry {
    let key = |p: Vec3| {
        (
            (p.x / eps).round() as i64,
            (p.y / eps).round() as i64,
            (p.z / eps).round() as i64,
        )
    };
    let mut map: HashMap<(i64, i64, i64), u32> = HashMap::new();
    let mut out = Geometry::new();
    let mut remap = vec![0u32; geo.points.len()];
    for (i, p) in geo.points.iter().enumerate() {
        let k = key(*p);
        // also check neighbouring cells so points straddling a cell border still merge
        let mut found = None;
        'search: for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(&j) = map.get(&(k.0 + dx, k.1 + dy, k.2 + dz)) {
                        if out.points[j as usize].distance(*p) <= eps {
                            found = Some(j);
                            break 'search;
                        }
                    }
                }
            }
        }
        remap[i] = match found {
            Some(j) => j,
            None => {
                let j = out.add_point(*p);
                map.insert(k, j);
                j
            }
        };
    }
    for pr in &geo.prims {
        let mut pts: Vec<u32> = pr.points.iter().map(|&p| remap[p as usize]).collect();
        pts.dedup();
        if pr.kind == PrimKind::Polygon {
            while pts.len() > 1 && pts.first() == pts.last() {
                pts.pop();
            }
            if pts.len() < 3 {
                continue;
            }
        }
        out.add_prim(pr.kind, pts);
    }
    out
}

/// Remove sliver polygons: faces whose corners all lie within `eps` of one line
/// (zero or near-zero width). Their corners are then stitched into the neighbouring
/// faces by [`fix_t_junctions`] with the same `eps`, which keeps the surface closed.
pub fn remove_degenerate(geo: &Geometry, eps: f32) -> Geometry {
    let mut out = Geometry::new();
    out.points = geo.points.clone();
    out.point_attribs = geo.point_attribs.clone();
    for pr in &geo.prims {
        if pr.kind == PrimKind::Polygon && is_sliver(&geo.points, &pr.points, eps) {
            continue;
        }
        out.add_prim(pr.kind, pr.points.clone());
    }
    out
}

fn is_sliver(points: &[Vec3], ids: &[u32], eps: f32) -> bool {
    let n = ids.len();
    if n < 3 {
        return true;
    }
    // longest edge
    let (mut a, mut b, mut best) = (Vec3::ZERO, Vec3::ZERO, -1.0f32);
    for k in 0..n {
        let (p, q) = (points[ids[k] as usize], points[ids[(k + 1) % n] as usize]);
        let d = p.distance_squared(q);
        if d > best {
            best = d;
            a = p;
            b = q;
        }
    }
    if best <= eps * eps {
        return true;
    }
    let dir = (b - a).normalize();
    ids.iter().all(|&i| {
        let v = points[i as usize] - a;
        (v - dir * v.dot(dir)).length() <= eps
    })
}

/// Insert points that lie on a polygon's edges into that polygon, closing T-junction cracks.
pub fn fix_t_junctions(geo: &mut Geometry, eps: f32) {
    if geo.points.is_empty() {
        return;
    }
    // Spatial hash of points, cell size from the average edge length.
    let mut total = 0.0;
    let mut count = 0usize;
    for pr in &geo.prims {
        let n = pr.points.len();
        for k in 0..n {
            total += geo.points[pr.points[k] as usize]
                .distance(geo.points[pr.points[(k + 1) % n] as usize]);
            count += 1;
        }
    }
    let cell = (total / count.max(1) as f32).max(eps * 10.0);
    let ckey = |p: Vec3| {
        (
            (p.x / cell).floor() as i64,
            (p.y / cell).floor() as i64,
            (p.z / cell).floor() as i64,
        )
    };
    let mut grid: HashMap<(i64, i64, i64), Vec<u32>> = HashMap::new();
    for (i, p) in geo.points.iter().enumerate() {
        grid.entry(ckey(*p)).or_default().push(i as u32);
    }

    let points = geo.points.clone();
    let mut new_prims: Vec<Vec<u32>> = Vec::with_capacity(geo.prims.len());
    for pr in &geo.prims {
        if pr.kind != PrimKind::Polygon {
            new_prims.push(pr.points.clone());
            continue;
        }
        let n = pr.points.len();
        let mut out = Vec::with_capacity(n);
        let mut used: Vec<u32> = pr.points.clone(); // a point goes into a face at most once
        for k in 0..n {
            let (ia, ib) = (pr.points[k], pr.points[(k + 1) % n]);
            out.push(ia);
            let (a, b) = (points[ia as usize], points[ib as usize]);
            let ab = b - a;
            let len2 = ab.length_squared();
            if len2 <= eps * eps {
                continue;
            }
            let (lo, hi) = (
                ckey(a.min(b) - Vec3::splat(eps)),
                ckey(a.max(b) + Vec3::splat(eps)),
            );
            let cells = (hi.0 - lo.0 + 1) * (hi.1 - lo.1 + 1) * (hi.2 - lo.2 + 1);
            if cells > 4096 {
                continue; // very long edge across a huge area: skip (rare)
            }
            let mut on_edge: Vec<(f32, u32)> = Vec::new();
            for x in lo.0..=hi.0 {
                for y in lo.1..=hi.1 {
                    for z in lo.2..=hi.2 {
                        if let Some(list) = grid.get(&(x, y, z)) {
                            for &j in list {
                                if used.contains(&j) {
                                    continue; // never add a corner the face already has
                                }
                                let p = points[j as usize];
                                let t = (p - a).dot(ab) / len2;
                                if t <= 0.0
                                    || t >= 1.0
                                    || p.distance(a) <= eps
                                    || p.distance(b) <= eps
                                {
                                    continue;
                                }
                                if (a + ab * t).distance(p) <= eps {
                                    on_edge.push((t, j));
                                }
                            }
                        }
                    }
                }
            }
            on_edge.sort_by(|x, y| x.0.total_cmp(&y.0));
            for (_, j) in on_edge {
                if !used.contains(&j) {
                    used.push(j);
                    out.push(j);
                }
            }
        }
        new_prims.push(out);
    }
    let kinds: Vec<PrimKind> = geo.prims.iter().map(|p| p.kind).collect();
    let mut rebuilt = Geometry::new();
    rebuilt.points = std::mem::take(&mut geo.points);
    rebuilt.point_attribs = std::mem::take(&mut geo.point_attribs);
    rebuilt.detail_attribs = std::mem::take(&mut geo.detail_attribs);
    rebuilt.point_groups = std::mem::take(&mut geo.point_groups);
    for (kind, pts) in kinds.into_iter().zip(new_prims) {
        rebuilt.add_prim(kind, pts);
    }
    *geo = rebuilt;
}

/// Average normal per point (from vertex `N` if present, else face normals).
pub fn point_normals(geo: &Geometry) -> Vec<Vec3> {
    let mut acc = vec![Vec3::ZERO; geo.points.len()];
    let vn = match geo.vertex_attribs.get("N") {
        Some(crate::AttribArray::Vec3(v)) => Some(v),
        _ => None,
    };
    for (i, pr) in geo.prims.iter().enumerate() {
        if pr.kind != PrimKind::Polygon {
            continue;
        }
        let face = geo.prim_area_normal(i);
        for (k, &p) in pr.points.iter().enumerate() {
            acc[p as usize] += vn.map(|v| v[pr.vertex_start + k]).unwrap_or(face);
        }
    }
    acc.into_iter().map(|n| n.normalize_or_zero()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triangulates_l_shape() {
        let pts: Vec<Vec3> = [
            (0.0, 0.0),
            (2.0, 0.0),
            (2.0, 1.0),
            (1.0, 1.0),
            (1.0, 2.0),
            (0.0, 2.0),
        ]
        .iter()
        .map(|(x, z)| Vec3::new(*x, 0.0, *z))
        .collect();
        let tris = triangulate(&pts);
        assert_eq!(tris.len(), 4);
        let area: f32 = tris
            .iter()
            .map(|t| {
                (pts[t[1]] - pts[t[0]])
                    .cross(pts[t[2]] - pts[t[0]])
                    .length()
                    * 0.5
            })
            .sum();
        assert!((area - 3.0).abs() < 1e-5, "area {area}");
    }

    #[test]
    fn weld_and_fix_crack() {
        // Two quads side by side where the right one is split in two: a T-junction at (1, 0.5).
        let mut g = Geometry::new();
        let p = |g: &mut Geometry, x: f32, y: f32| g.add_point(Vec3::new(x, y, 0.0));
        let (a, b, c, d) = (
            p(&mut g, 0.0, 0.0),
            p(&mut g, 1.0, 0.0),
            p(&mut g, 1.0, 1.0),
            p(&mut g, 0.0, 1.0),
        );
        g.add_polygon(vec![a, b, c, d]);
        let (e, f, h, m) = (
            p(&mut g, 1.0, 0.0),
            p(&mut g, 2.0, 0.0),
            p(&mut g, 2.0, 0.5),
            p(&mut g, 1.0, 0.5),
        );
        g.add_polygon(vec![e, f, h, m]);
        let (q, r, s) = (
            p(&mut g, 2.0, 1.0),
            p(&mut g, 1.0, 1.0),
            p(&mut g, 1.0, 0.5),
        );
        let h2 = p(&mut g, 2.0, 0.5);
        g.add_polygon(vec![s, h2, q, r]);
        let mut w = weld(&g, 1e-4);
        assert_eq!(w.num_points(), 8);
        fix_t_junctions(&mut w, 1e-4);
        assert_eq!(w.prims[0].points.len(), 5, "left quad gains the T point");
        assert_eq!(w.topology().boundary_edges, 7);
    }
}
