//! Wavefront OBJ export (opens in Blender, Houdini, Maya, MeshLab ...).

use std::fmt::Write as _;
use std::io::{self, Write};

use crate::attrib::AttribArray;
use crate::geometry::{Geometry, PrimKind};

/// Write geometry as OBJ text. Uses vertex `uv`/`N` when present, else point `uv`/`N`.
pub fn to_obj_string(geo: &Geometry) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "# AItelier OBJ export: {} points, {} primitives",
        geo.num_points(),
        geo.num_prims()
    );
    for p in &geo.points {
        let _ = writeln!(s, "v {} {} {}", p.x, p.y, p.z);
    }

    // UVs: one per vertex, or one per point.
    let uv_mode = match (geo.vertex_attribs.get("uv"), geo.point_attribs.get("uv")) {
        (Some(AttribArray::Vec2(uv)), _) => {
            for t in uv {
                let _ = writeln!(s, "vt {} {}", t.x, t.y);
            }
            Some(true)
        }
        (_, Some(AttribArray::Vec2(uv))) => {
            for t in uv {
                let _ = writeln!(s, "vt {} {}", t.x, t.y);
            }
            Some(false)
        }
        _ => None,
    };
    let n_mode = match (geo.vertex_attribs.get("N"), geo.point_attribs.get("N")) {
        (Some(AttribArray::Vec3(ns)), _) => {
            for n in ns {
                let _ = writeln!(s, "vn {} {} {}", n.x, n.y, n.z);
            }
            Some(true)
        }
        (_, Some(AttribArray::Vec3(ns))) => {
            for n in ns {
                let _ = writeln!(s, "vn {} {} {}", n.x, n.y, n.z);
            }
            Some(false)
        }
        _ => None,
    };

    for pr in &geo.prims {
        let tag = match pr.kind {
            PrimKind::Polygon => "f",
            PrimKind::Polyline => "l",
        };
        s.push_str(tag);
        for (k, &pt) in pr.points.iter().enumerate() {
            let v = pr.vertex_start + k;
            let pi = pt as usize + 1;
            let pick = |per_vertex: bool| if per_vertex { v + 1 } else { pi };
            match (pr.kind, uv_mode, n_mode) {
                (PrimKind::Polyline, _, _) | (_, None, None) => {
                    let _ = write!(s, " {pi}");
                }
                (_, Some(u), None) => {
                    let _ = write!(s, " {pi}/{}", pick(u));
                }
                (_, None, Some(n)) => {
                    let _ = write!(s, " {pi}//{}", pick(n));
                }
                (_, Some(u), Some(n)) => {
                    let _ = write!(s, " {pi}/{}/{}", pick(u), pick(n));
                }
            }
        }
        s.push('\n');
    }
    s
}

pub fn write_obj<W: Write>(geo: &Geometry, mut w: W) -> io::Result<()> {
    w.write_all(to_obj_string(geo).as_bytes())
}
