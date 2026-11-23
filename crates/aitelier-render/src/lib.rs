//! CPU preview renderer.
//!
//! A small z-buffer rasterizer used by the editor viewport and the `aitelier render`
//! command. It produces a shaded colour image plus a linear depth buffer. It is meant
//! for previews; the GPU renderer for datasets comes later.

use std::path::Path;

use aitelier_geo::{AttribArray, Geometry, PrimKind};
use glam::{Mat4, Vec3, Vec4};

/// Orbit camera around a target point. Y is up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub target: Vec3,
    /// Rotation around Y, degrees.
    pub yaw: f32,
    /// Elevation, degrees (positive looks down on the scene).
    pub pitch: f32,
    pub distance: f32,
    /// Vertical field of view, degrees.
    pub fov_y: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            target: Vec3::ZERO,
            yaw: 35.0,
            pitch: 20.0,
            distance: 10.0,
            fov_y: 40.0,
        }
    }
}

impl Camera {
    pub fn eye(&self) -> Vec3 {
        let (y, p) = (self.yaw.to_radians(), self.pitch.to_radians());
        self.target + self.distance * Vec3::new(p.cos() * y.sin(), p.sin(), p.cos() * y.cos())
    }

    pub fn view(&self) -> Mat4 {
        Mat4::look_at_rh(self.eye(), self.target, Vec3::Y)
    }

    pub fn near_far(&self) -> (f32, f32) {
        let near = (self.distance * 0.01).max(1e-4);
        (near, self.distance * 100.0 + 100.0)
    }

    pub fn projection(&self, aspect: f32) -> Mat4 {
        let (n, f) = self.near_far();
        Mat4::perspective_rh(self.fov_y.to_radians(), aspect, n, f)
    }

    /// Aim at a bounding box so it fills the view.
    pub fn frame_bounds(&mut self, lo: Vec3, hi: Vec3) {
        self.target = (lo + hi) * 0.5;
        let radius = ((hi - lo).length() * 0.5).max(1e-3);
        self.distance = radius / (self.fov_y.to_radians() * 0.5).sin() * 1.1;
    }

    /// Orbit by a screen drag (pixels).
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * 0.4;
        self.pitch = (self.pitch + dy * 0.4).clamp(-89.0, 89.0);
    }

    /// Pan by a screen drag (pixels), for a viewport `height` pixels tall.
    pub fn pan(&mut self, dx: f32, dy: f32, height: f32) {
        let world_per_px =
            2.0 * self.distance * (self.fov_y.to_radians() * 0.5).tan() / height.max(1.0);
        let fwd = (self.target - self.eye()).normalize();
        let right = fwd.cross(Vec3::Y).normalize_or_zero();
        let up = right.cross(fwd);
        self.target += (-dx * right + dy * up) * world_per_px;
    }

    /// Zoom; positive `steps` moves closer.
    pub fn zoom(&mut self, steps: f32) {
        self.distance = (self.distance * 0.9f32.powf(steps)).max(1e-3);
    }
}

#[derive(Clone, Debug)]
pub struct RenderSettings {
    pub width: u32,
    pub height: u32,
    /// Supersampling factor for anti-aliasing (1 = off, 2 = 4 samples per pixel).
    pub supersample: u32,
    pub base_color: Vec3,
    pub background_top: Vec3,
    pub background_bottom: Vec3,
    pub grid: bool,
    pub wireframe: bool,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            width: 960,
            height: 640,
            supersample: 1,
            base_color: Vec3::new(0.60, 0.62, 0.60),
            background_top: Vec3::new(0.055, 0.058, 0.066),
            background_bottom: Vec3::new(0.018, 0.018, 0.021),
            grid: true,
            wireframe: false,
        }
    }
}

/// Rendered image: 8-bit RGBA, plus camera-space depth per pixel (infinity = background).
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub depth: Vec<f32>,
}

impl Image {
    pub fn write_png(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let file = std::fs::File::create(path)?;
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), self.width, self.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().map_err(std::io::Error::other)?;
        w.write_image_data(&self.rgba)
            .map_err(std::io::Error::other)?;
        Ok(())
    }
}

/// A projected vertex: screen position, 1/w, and world-space data.
#[derive(Clone, Copy)]
struct SVert {
    x: f32,
    y: f32,
    inv_w: f32,
    n: Vec3,
    p: Vec3,
}

struct Target {
    w: usize,
    h: usize,
    color: Vec<Vec3>,
    depth: Vec<f32>,
}

/// Render geometry with the given camera.
pub fn render(geo: &Geometry, cam: &Camera, s: &RenderSettings) -> Image {
    let ss = s.supersample.clamp(1, 4) as usize;
    let (w, h) = (
        (s.width.max(1) as usize) * ss,
        (s.height.max(1) as usize) * ss,
    );
    let mut t = Target {
        w,
        h,
        color: Vec::with_capacity(w * h),
        depth: vec![f32::INFINITY; w * h],
    };
    for y in 0..h {
        let k = y as f32 / (h - 1).max(1) as f32;
        let c = s.background_top.lerp(s.background_bottom, k);
        t.color.extend(std::iter::repeat_n(c, w));
    }

    let view = cam.view();
    let vp = cam.projection(w as f32 / h as f32) * view;
    let eye = cam.eye();
    let (near, _) = cam.near_far();

    // Lights follow the camera so the model is always readable.
    let fwd = (cam.target - eye).normalize();
    let right = fwd.cross(Vec3::Y).normalize_or_zero();
    let up = right.cross(fwd);
    let key = (-fwd * 0.6 + up * 0.7 - right * 0.4).normalize();
    let fill = (-fwd * 0.5 - up * 0.2 + right * 0.8).normalize();

    let vn = match geo.vertex_attribs.get("N") {
        Some(AttribArray::Vec3(n)) => Some(n),
        _ => None,
    };
    let pn = match geo.point_attribs.get("N") {
        Some(AttribArray::Vec3(n)) => Some(n),
        _ => None,
    };

    // --- shaded polygons
    for (pi, pr) in geo.prims.iter().enumerate() {
        if pr.kind != PrimKind::Polygon || pr.points.len() < 3 {
            continue;
        }
        let face_n = geo.prim_normal(pi);
        let corner = |k: usize| {
            let p = geo.points[pr.points[k] as usize];
            let n = vn
                .map(|n| n[pr.vertex_start + k])
                .or_else(|| pn.map(|n| n[pr.points[k] as usize]))
                .filter(|n| n.length_squared() > 0.0)
                .unwrap_or(face_n);
            (p, n)
        };
        let tris: Vec<[usize; 3]> = if pr.points.len() == 3 {
            vec![[0, 1, 2]]
        } else {
            let pts: Vec<Vec3> = pr.points.iter().map(|&p| geo.points[p as usize]).collect();
            aitelier_geo::mesh_ops::triangulate(&pts)
        };
        for [a, b, c] in tris {
            let tri = [corner(a), corner(b), corner(c)];
            for poly in clip_near(&tri, &vp, near) {
                raster_triangle(&mut t, &poly, eye, key, fill, s.base_color);
            }
        }
    }

    // --- grid on the ground plane
    if s.grid {
        let spacing = grid_spacing(cam.distance);
        let n = 20;
        let ext = spacing * n as f32;
        for i in -n..=n {
            let o = i as f32 * spacing;
            let (cx, cz) = if i == 0 {
                (Vec3::new(0.40, 0.06, 0.06), Vec3::new(0.06, 0.13, 0.42))
            } else {
                (Vec3::splat(0.075), Vec3::splat(0.075))
            };
            let major = i % 5 == 0;
            let (cx, cz) = if major || i == 0 {
                (cx, cz)
            } else {
                (Vec3::splat(0.04), Vec3::splat(0.04))
            };
            draw_line(
                &mut t,
                &vp,
                Vec3::new(-ext, 0.0, o),
                Vec3::new(ext, 0.0, o),
                cx,
                near,
                1e-3,
            );
            draw_line(
                &mut t,
                &vp,
                Vec3::new(o, 0.0, -ext),
                Vec3::new(o, 0.0, ext),
                cz,
                near,
                1e-3,
            );
        }
    }

    // --- wireframe and curves
    for pr in &geo.prims {
        let n = pr.points.len();
        let (color, segs) = match pr.kind {
            PrimKind::Polygon if s.wireframe => (Vec3::new(0.08, 0.08, 0.09), n),
            PrimKind::Polygon => continue,
            PrimKind::Polyline => (Vec3::new(0.98, 0.80, 0.25), n.saturating_sub(1)),
        };
        for k in 0..segs {
            let a = geo.points[pr.points[k] as usize];
            let b = geo.points[pr.points[(k + 1) % n] as usize];
            draw_line(&mut t, &vp, a, b, color, near, 2e-3);
        }
    }

    // --- resolve: downsample, gamma
    let (ow, oh) = (w / ss, h / ss);
    let mut rgba = Vec::with_capacity(ow * oh * 4);
    let mut depth = Vec::with_capacity(ow * oh);
    for y in 0..oh {
        for x in 0..ow {
            let mut c = Vec3::ZERO;
            let mut d = f32::INFINITY;
            for sy in 0..ss {
                for sx in 0..ss {
                    let i = (y * ss + sy) * w + x * ss + sx;
                    c += t.color[i];
                    d = d.min(t.depth[i]);
                }
            }
            c /= (ss * ss) as f32;
            for ch in [c.x, c.y, c.z] {
                rgba.push((ch.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0 + 0.5) as u8);
            }
            rgba.push(255);
            depth.push(d);
        }
    }
    Image {
        width: ow as u32,
        height: oh as u32,
        rgba,
        depth,
    }
}

/// Grid line spacing that suits the camera distance (powers of 10 times 1, 2 or 5).
pub fn grid_spacing(distance: f32) -> f32 {
    let raw = (distance / 12.0).max(1e-4);
    let base = 10f32.powf(raw.log10().floor());
    [1.0, 2.0, 5.0, 10.0]
        .iter()
        .map(|m| m * base)
        .find(|v| *v >= raw)
        .unwrap_or(base * 10.0)
}

/// Project a triangle and clip it against the near plane (w > near). Returns 0..2 triangles.
fn clip_near(tri: &[(Vec3, Vec3); 3], vp: &Mat4, near: f32) -> Vec<[(Vec4, Vec3, Vec3); 3]> {
    let pts: Vec<(Vec4, Vec3, Vec3)> = tri
        .iter()
        .map(|(p, n)| (*vp * p.extend(1.0), *n, *p))
        .collect();
    let inside = |v: &(Vec4, Vec3, Vec3)| v.0.w > near;
    if pts.iter().all(inside) {
        return vec![[pts[0], pts[1], pts[2]]];
    }
    let mut poly = Vec::new();
    for i in 0..3 {
        let (a, b) = (pts[i], pts[(i + 1) % 3]);
        if inside(&a) {
            poly.push(a);
        }
        if inside(&a) != inside(&b) {
            let t = (near - a.0.w) / (b.0.w - a.0.w);
            poly.push((a.0.lerp(b.0, t), a.1.lerp(b.1, t), a.2.lerp(b.2, t)));
        }
    }
    (1..poly.len().saturating_sub(1))
        .map(|k| [poly[0], poly[k], poly[k + 1]])
        .collect()
}

fn to_screen(t: &Target, clip: Vec4) -> (f32, f32, f32) {
    let inv_w = 1.0 / clip.w;
    let x = (clip.x * inv_w * 0.5 + 0.5) * t.w as f32;
    let y = (0.5 - clip.y * inv_w * 0.5) * t.h as f32;
    (x, y, inv_w)
}

fn raster_triangle(
    t: &mut Target,
    tri: &[(Vec4, Vec3, Vec3); 3],
    eye: Vec3,
    key: Vec3,
    fill: Vec3,
    base: Vec3,
) {
    let v: Vec<SVert> = tri
        .iter()
        .map(|(c, n, p)| {
            let (x, y, inv_w) = to_screen(t, *c);
            SVert {
                x,
                y,
                inv_w,
                n: *n,
                p: *p,
            }
        })
        .collect();
    let area = (v[1].x - v[0].x) * (v[2].y - v[0].y) - (v[2].x - v[0].x) * (v[1].y - v[0].y);
    if area.abs() < 1e-12 {
        return;
    }
    let min_x = v
        .iter()
        .map(|q| q.x)
        .fold(f32::INFINITY, f32::min)
        .floor()
        .max(0.0) as usize;
    let max_x = v
        .iter()
        .map(|q| q.x)
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .min(t.w as f32 - 1.0);
    let min_y = v
        .iter()
        .map(|q| q.y)
        .fold(f32::INFINITY, f32::min)
        .floor()
        .max(0.0) as usize;
    let max_y = v
        .iter()
        .map(|q| q.y)
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .min(t.h as f32 - 1.0);
    if max_x < 0.0 || max_y < 0.0 {
        return;
    }
    let (max_x, max_y) = (max_x as usize, max_y as usize);
    let inv_area = 1.0 / area;
    let edge = |a: &SVert, b: &SVert, px: f32, py: f32| {
        (b.x - a.x) * (py - a.y) - (b.y - a.y) * (px - a.x)
    };

    for py in min_y..=max_y {
        for px in min_x..=max_x {
            let (fx, fy) = (px as f32 + 0.5, py as f32 + 0.5);
            let w0 = edge(&v[1], &v[2], fx, fy) * inv_area;
            let w1 = edge(&v[2], &v[0], fx, fy) * inv_area;
            let w2 = edge(&v[0], &v[1], fx, fy) * inv_area;
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                continue;
            }
            let iw = w0 * v[0].inv_w + w1 * v[1].inv_w + w2 * v[2].inv_w;
            let z = 1.0 / iw;
            let i = py * t.w + px;
            if z >= t.depth[i] {
                continue;
            }
            // Perspective-correct interpolation.
            let (a, b, c) = (
                w0 * v[0].inv_w * z,
                w1 * v[1].inv_w * z,
                w2 * v[2].inv_w * z,
            );
            let mut n = (v[0].n * a + v[1].n * b + v[2].n * c).normalize_or_zero();
            let p = v[0].p * a + v[1].p * b + v[2].p * c;
            let to_eye = (eye - p).normalize_or_zero();
            let mut albedo = base;
            if n.dot(to_eye) < 0.0 {
                n = -n; // back face: show it, slightly darker
                albedo *= 0.55;
            }
            let diffuse = 0.62 * n.dot(key).max(0.0) + 0.22 * n.dot(fill).max(0.0);
            let sky = 0.18 + 0.08 * n.y;
            let half = (key + to_eye).normalize();
            let spec = 0.18 * n.dot(half).max(0.0).powf(40.0);
            let rim = 0.10 * (1.0 - n.dot(to_eye).max(0.0)).powf(3.0);
            t.color[i] = albedo * (sky + diffuse) + Vec3::splat(spec + rim);
            t.depth[i] = z;
        }
    }
}

/// Depth-tested line. `bias` is a fraction of the depth to let lines win over the surface they lie on.
fn draw_line(t: &mut Target, vp: &Mat4, a: Vec3, b: Vec3, color: Vec3, near: f32, bias: f32) {
    let (mut ca, mut cb) = (*vp * a.extend(1.0), *vp * b.extend(1.0));
    if ca.w <= near && cb.w <= near {
        return;
    }
    if ca.w <= near {
        ca = ca.lerp(cb, (near - ca.w) / (cb.w - ca.w));
    } else if cb.w <= near {
        cb = cb.lerp(ca, (near - cb.w) / (ca.w - cb.w));
    }
    let (x0, y0, iw0) = to_screen(t, ca);
    let (x1, y1, iw1) = to_screen(t, cb);
    let steps = (x1 - x0).abs().max((y1 - y0).abs()).ceil().min(20_000.0) as usize;
    for s in 0..=steps {
        let k = if steps == 0 {
            0.0
        } else {
            s as f32 / steps as f32
        };
        let (x, y) = (x0 + (x1 - x0) * k, y0 + (y1 - y0) * k);
        if x < 0.0 || y < 0.0 || x >= t.w as f32 || y >= t.h as f32 {
            continue;
        }
        let z = 1.0 / (iw0 + (iw1 - iw0) * k);
        let i = y as usize * t.w + x as usize;
        if z * (1.0 - bias) < t.depth[i] {
            t.color[i] = color;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad() -> Geometry {
        let mut g = Geometry::new();
        let ids: Vec<u32> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .iter()
            .map(|(x, y)| g.add_point(Vec3::new(*x, *y, 0.0)))
            .collect();
        g.add_polygon(ids);
        g
    }

    #[test]
    fn renders_object_in_center() {
        let cam = Camera {
            target: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            distance: 5.0,
            fov_y: 40.0,
        };
        let s = RenderSettings {
            width: 64,
            height: 48,
            grid: false,
            ..Default::default()
        };
        let img = render(&quad(), &cam, &s);
        let center = (24 * 64 + 32) as usize;
        assert!(
            (img.depth[center] - 5.0).abs() < 1e-3,
            "depth {}",
            img.depth[center]
        );
        assert!(img.depth[0].is_infinite());
        assert_eq!(img.rgba.len(), 64 * 48 * 4);
    }

    #[test]
    fn near_plane_clipping_keeps_visible_part() {
        // Camera inside a large quad's plane span: the quad crosses the near plane.
        let mut g = Geometry::new();
        let ids: Vec<u32> = [
            (-50.0, -1.0, -50.0),
            (50.0, -1.0, -50.0),
            (50.0, -1.0, 50.0),
            (-50.0, -1.0, 50.0),
        ]
        .iter()
        .map(|(x, y, z)| g.add_point(Vec3::new(*x, *y, *z)))
        .collect();
        g.add_polygon(ids);
        let cam = Camera {
            target: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            distance: 3.0,
            fov_y: 60.0,
        };
        let img = render(
            &g,
            &cam,
            &RenderSettings {
                width: 40,
                height: 40,
                grid: false,
                ..Default::default()
            },
        );
        let covered = img.depth.iter().filter(|d| d.is_finite()).count();
        assert!(covered > 300 && covered < 1600, "covered {covered}");
    }

    #[test]
    fn spacing_steps() {
        assert_eq!(grid_spacing(12.0), 1.0);
        assert_eq!(grid_spacing(30.0), 5.0);
        assert_eq!(grid_spacing(100.0), 10.0);
    }
}
