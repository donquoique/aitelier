//! Curve from a list of points, optionally smoothed (centripetal Catmull-Rom).

use std::sync::Arc;

use aitelier_geo::{Geometry, Vec3};

use super::node_header;
use crate::registry::{NodeType, ParamDef, Params};
use crate::GraphError;

pub struct CurveNode {
    params: Vec<ParamDef>,
}
impl CurveNode {
    pub fn new() -> Self {
        Self {
            params: vec![
                ParamDef::vec3_list(
                    "points",
                    &[[0.0, 0.0, 0.0], [1.0, 1.0, 0.0]],
                    "Control points; each coordinate can be a number, expression or keyframes",
                ),
                ParamDef::boolean("closed", false, "Connect the last point back to the first"),
                ParamDef::menu(
                    "smooth",
                    &["none", "catmull"],
                    "none = straight segments; catmull = smooth curve through the points",
                ),
                ParamDef::int("resolution", 8, "Samples per segment when smooth"),
                ParamDef::int_list(
                    "sharp",
                    "Indices of points that stay sharp corners when smooth",
                ),
            ],
        }
    }
}

impl NodeType for CurveNode {
    node_header!(
        "curve",
        "Polyline through points; use it as a profile for revolve.",
        0,
        Some(0)
    );
    fn cook(&self, _: &[Arc<Geometry>], p: &Params) -> Result<Geometry, GraphError> {
        let pts = p.vec3_list("points")?;
        if pts.len() < 2 {
            return Err(p.fail("a curve needs at least 2 points"));
        }
        let closed = p.boolean("closed")?;
        let smooth = p.menu("smooth")? == "catmull";
        let res = p.int_min("resolution", 1)?;
        let sharp: Vec<usize> = p
            .int_list("sharp")?
            .into_iter()
            .filter(|&i| i >= 0)
            .map(|i| i as usize)
            .collect();

        let samples = if smooth {
            smooth_curve(&pts, closed, res, &sharp)
        } else {
            pts.clone()
        };
        let mut g = Geometry::new();
        let mut ids: Vec<u32> = samples.iter().map(|&q| g.add_point(q)).collect();
        if closed {
            ids.push(ids[0]);
        }
        g.add_polyline(ids);
        Ok(g)
    }
}

/// Sample a smooth curve through `pts`. Points listed in `sharp` (and the ends of an open
/// curve) are corners: the curve is smoothed separately between them.
pub fn smooth_curve(pts: &[Vec3], closed: bool, res: usize, sharp: &[usize]) -> Vec<Vec3> {
    let n = pts.len();
    let mut corners: Vec<usize> = sharp.iter().copied().filter(|&i| i < n).collect();
    corners.sort_unstable();
    corners.dedup();

    if closed && corners.is_empty() {
        // Periodic: every segment uses its true neighbours.
        let mut out = Vec::new();
        for i in 0..n {
            let quad = [
                pts[(i + n - 1) % n],
                pts[i],
                pts[(i + 1) % n],
                pts[(i + 2) % n],
            ];
            out.extend(segment(quad, res));
        }
        return out;
    }

    // Open runs between corners.
    let seq: Vec<Vec3> = if closed {
        // Rotate so the curve starts at the first corner, then close it.
        let s = corners[0];
        let mut v: Vec<Vec3> = (0..=n).map(|k| pts[(s + k) % n]).collect();
        corners = corners.iter().map(|&c| (c + n - s) % n).collect();
        corners.push(n);
        v.dedup();
        v
    } else {
        pts.to_vec()
    };
    let last = seq.len() - 1;
    let mut bounds: Vec<usize> = vec![0];
    bounds.extend(corners.into_iter().filter(|&c| c > 0 && c < last));
    bounds.push(last);
    bounds.dedup();

    let mut out = Vec::new();
    for w in bounds.windows(2) {
        let run = &seq[w[0]..=w[1]];
        out.extend(smooth_run(run, res));
    }
    out.push(seq[last]);
    if closed {
        out.pop(); // the start point is the end point
    }
    out
}

/// Samples of an open run, excluding its final point.
fn smooth_run(run: &[Vec3], res: usize) -> Vec<Vec3> {
    let m = run.len();
    if m == 2 {
        return vec![run[0]];
    }
    let mut out = Vec::new();
    for i in 0..m - 1 {
        let p0 = if i == 0 {
            2.0 * run[0] - run[1]
        } else {
            run[i - 1]
        };
        let p3 = if i + 2 < m {
            run[i + 2]
        } else {
            2.0 * run[m - 1] - run[m - 2]
        };
        out.extend(segment([p0, run[i], run[i + 1], p3], res));
    }
    out
}

/// Centripetal Catmull-Rom between q[1] and q[2]: `res` samples, starting at q[1].
fn segment(q: [Vec3; 4], res: usize) -> Vec<Vec3> {
    let step = |t: f32, a: Vec3, b: Vec3| t + (b - a).length().sqrt().max(1e-4);
    let t0 = 0.0;
    let t1 = step(t0, q[0], q[1]);
    let t2 = step(t1, q[1], q[2]);
    let t3 = step(t2, q[2], q[3]);
    (0..res)
        .map(|s| {
            let t = t1 + (t2 - t1) * s as f32 / res as f32;
            let a1 = q[0] * ((t1 - t) / (t1 - t0)) + q[1] * ((t - t0) / (t1 - t0));
            let a2 = q[1] * ((t2 - t) / (t2 - t1)) + q[2] * ((t - t1) / (t2 - t1));
            let a3 = q[2] * ((t3 - t) / (t3 - t2)) + q[3] * ((t - t2) / (t3 - t2));
            let b1 = a1 * ((t2 - t) / (t2 - t0)) + a2 * ((t - t0) / (t2 - t0));
            let b2 = a2 * ((t3 - t) / (t3 - t1)) + a3 * ((t - t1) / (t3 - t1));
            b1 * ((t2 - t) / (t2 - t1)) + b2 * ((t - t1) / (t2 - t1))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_through_points_and_keeps_corners() {
        let pts = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(4.0, 0.0, 0.0),
            Vec3::new(8.0, 8.0, 0.0),
            Vec3::new(3.0, 16.0, 0.0),
        ];
        let s = smooth_curve(&pts, false, 8, &[1]);
        for p in &pts {
            assert!(s.iter().any(|q| q.distance(*p) < 1e-4), "missing {p}");
        }
        // The segment before the sharp corner stays straight on y = 0.
        assert!(s
            .iter()
            .filter(|q| q.x <= 4.0 && q.y < 1.0)
            .all(|q| q.y.abs() < 1e-5));
    }

    #[test]
    fn closed_periodic() {
        let pts = vec![Vec3::X, Vec3::Y, -Vec3::X, -Vec3::Y];
        let s = smooth_curve(&pts, true, 4, &[]);
        assert_eq!(s.len(), 16);
        assert!(s.iter().all(|q| (q.length() - 1.0).abs() < 0.15));
    }
}
