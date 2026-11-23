//! Timeline, keyframes and animation channels.
//!
//! A [`Channel`] holds keyframes for one number. Any node parameter (or any
//! component of a vector parameter) can be driven by a channel.

use serde::{Deserialize, Serialize};

/// How the value moves from a key to the next one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Interp {
    /// Hold the value until the next key (step).
    Constant,
    /// Straight line.
    Linear,
    /// Smooth curve with automatic, clamped tangents (no overshoot). Blender's default.
    #[default]
    Bezier,
    /// Smooth start and stop at every key (zero tangents).
    Ease,
}

/// A keyframe. `interp` applies to the segment that starts at this key.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Key {
    pub frame: f32,
    pub value: f32,
    #[serde(default)]
    pub interp: Interp,
}

/// Animation curve for one value.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Channel {
    keys: Vec<Key>,
}

impl Channel {
    pub fn new(mut keys: Vec<Key>) -> Self {
        keys.sort_by(|a, b| a.frame.total_cmp(&b.frame));
        keys.dedup_by(|b, a| a.frame == b.frame); // keep the first key on a duplicate frame
        Self { keys }
    }

    pub fn keys(&self) -> &[Key] {
        &self.keys
    }

    /// Insert or replace the key at `key.frame`.
    pub fn set_key(&mut self, key: Key) {
        match self
            .keys
            .binary_search_by(|k| k.frame.total_cmp(&key.frame))
        {
            Ok(i) => self.keys[i] = key,
            Err(i) => self.keys.insert(i, key),
        }
    }

    /// Remove the key at exactly `frame`. Returns true if one was removed.
    pub fn remove_key(&mut self, frame: f32) -> bool {
        let before = self.keys.len();
        self.keys.retain(|k| k.frame != frame);
        before != self.keys.len()
    }

    /// Slope used by the automatic Bezier tangent at key `i` (value per frame).
    fn auto_slope(&self, i: usize) -> f32 {
        let k = &self.keys;
        if i == 0 || i + 1 >= k.len() {
            return 0.0;
        }
        let (prev, cur, next) = (k[i - 1], k[i], k[i + 1]);
        // Clamp: flat tangent at local extremes so the curve never overshoots them.
        if (cur.value - prev.value) * (next.value - cur.value) <= 0.0 {
            return 0.0;
        }
        (next.value - prev.value) / (next.frame - prev.frame)
    }

    /// Value at `frame`. Before the first key / after the last key the value is held.
    pub fn eval(&self, frame: f32) -> f32 {
        let k = &self.keys;
        match k.len() {
            0 => return 0.0,
            1 => return k[0].value,
            _ => {}
        }
        if frame <= k[0].frame {
            return k[0].value;
        }
        if frame >= k[k.len() - 1].frame {
            return k[k.len() - 1].value;
        }
        let i = match k.binary_search_by(|key| key.frame.total_cmp(&frame)) {
            Ok(i) => return k[i].value,
            Err(i) => i - 1,
        };
        let (a, b) = (k[i], k[i + 1]);
        let span = b.frame - a.frame;
        let t = (frame - a.frame) / span;
        match a.interp {
            Interp::Constant => a.value,
            Interp::Linear => a.value + (b.value - a.value) * t,
            Interp::Ease | Interp::Bezier => {
                let (m0, m1) = if a.interp == Interp::Ease {
                    (0.0, 0.0)
                } else {
                    (self.auto_slope(i), self.auto_slope(i + 1))
                };
                let t2 = t * t;
                let t3 = t2 * t;
                let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
                let h10 = t3 - 2.0 * t2 + t;
                let h01 = -2.0 * t3 + 3.0 * t2;
                let h11 = t3 - t2;
                let v = h00 * a.value + h10 * span * m0 + h01 * b.value + h11 * span * m1;
                v.clamp(a.value.min(b.value), a.value.max(b.value))
            }
        }
    }
}

/// Frame range and frame rate.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Timeline {
    #[serde(default = "one")]
    pub start: f32,
    #[serde(default = "one")]
    pub end: f32,
    #[serde(default = "default_fps")]
    pub fps: f32,
}

fn one() -> f32 {
    1.0
}
fn default_fps() -> f32 {
    24.0
}

impl Default for Timeline {
    fn default() -> Self {
        Self {
            start: 1.0,
            end: 1.0,
            fps: 24.0,
        }
    }
}

impl Timeline {
    /// Seconds since the start of the timeline.
    pub fn time_at(&self, frame: f32) -> f32 {
        (frame - self.start) / self.fps
    }

    /// Whole frames from start to end (inclusive).
    pub fn frames(&self) -> impl Iterator<Item = f32> {
        let (s, e) = (self.start.round() as i64, self.end.round() as i64);
        (s..=e).map(|f| f as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(frame: f32, value: f32, interp: Interp) -> Key {
        Key {
            frame,
            value,
            interp,
        }
    }

    #[test]
    fn hold_outside_range() {
        let c = Channel::new(vec![
            key(10.0, 1.0, Interp::Linear),
            key(20.0, 3.0, Interp::Linear),
        ]);
        assert_eq!(c.eval(0.0), 1.0);
        assert_eq!(c.eval(99.0), 3.0);
    }

    #[test]
    fn linear_and_constant() {
        let c = Channel::new(vec![
            key(0.0, 0.0, Interp::Linear),
            key(10.0, 10.0, Interp::Constant),
            key(20.0, 0.0, Interp::Linear),
        ]);
        assert!((c.eval(2.5) - 2.5).abs() < 1e-6);
        assert_eq!(c.eval(15.0), 10.0);
        assert_eq!(c.eval(20.0), 0.0);
    }

    #[test]
    fn bezier_is_smooth_and_does_not_overshoot() {
        let c = Channel::new(vec![
            key(0.0, 0.0, Interp::Bezier),
            key(10.0, 10.0, Interp::Bezier),
            key(20.0, 0.0, Interp::Bezier),
        ]);
        let mut prev = c.eval(0.0);
        for f in 1..=10 {
            let v = c.eval(f as f32);
            assert!(v >= prev - 1e-5 && v <= 10.0 + 1e-5);
            prev = v;
        }
        assert!((c.eval(5.0) - 5.0).abs() < 1e-5); // symmetric segment with flat ends
    }

    #[test]
    fn ease_has_flat_ends() {
        let c = Channel::new(vec![
            key(0.0, 0.0, Interp::Ease),
            key(10.0, 1.0, Interp::Linear),
        ]);
        assert!(c.eval(1.0) < 0.1);
        assert!(c.eval(9.0) > 0.9);
    }

    #[test]
    fn set_and_remove_keys() {
        let mut c = Channel::default();
        c.set_key(key(5.0, 2.0, Interp::Linear));
        c.set_key(key(1.0, 0.0, Interp::Linear));
        c.set_key(key(5.0, 4.0, Interp::Linear));
        assert_eq!(c.keys().len(), 2);
        assert_eq!(c.eval(5.0), 4.0);
        assert!(c.remove_key(1.0));
        assert_eq!(c.eval(0.0), 4.0);
    }

    #[test]
    fn timeline_time() {
        let t = Timeline {
            start: 1.0,
            end: 48.0,
            fps: 24.0,
        };
        assert_eq!(t.time_at(25.0), 1.0);
        assert_eq!(t.frames().count(), 48);
    }
}
