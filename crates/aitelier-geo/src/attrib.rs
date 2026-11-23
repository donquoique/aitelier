use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

/// The type of an attribute.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AttribKind {
    Float,
    Int,
    Vec2,
    Vec3,
    Str,
}

/// A single attribute value (used for detail attributes).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AttribValue {
    Float(f32),
    Int(i64),
    Vec2(Vec2),
    Vec3(Vec3),
    Str(String),
}

/// One attribute stored for every element of a class (points, vertices or primitives).
#[derive(Clone, Debug, PartialEq)]
pub enum AttribArray {
    Float(Vec<f32>),
    Int(Vec<i64>),
    Vec2(Vec<Vec2>),
    Vec3(Vec<Vec3>),
    Str(Vec<String>),
}

impl AttribArray {
    /// An array of `n` default (zero / empty) values.
    pub fn zeros(kind: AttribKind, n: usize) -> Self {
        match kind {
            AttribKind::Float => Self::Float(vec![0.0; n]),
            AttribKind::Int => Self::Int(vec![0; n]),
            AttribKind::Vec2 => Self::Vec2(vec![Vec2::ZERO; n]),
            AttribKind::Vec3 => Self::Vec3(vec![Vec3::ZERO; n]),
            AttribKind::Str => Self::Str(vec![String::new(); n]),
        }
    }

    pub fn kind(&self) -> AttribKind {
        match self {
            Self::Float(_) => AttribKind::Float,
            Self::Int(_) => AttribKind::Int,
            Self::Vec2(_) => AttribKind::Vec2,
            Self::Vec3(_) => AttribKind::Vec3,
            Self::Str(_) => AttribKind::Str,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Self::Float(v) => v.len(),
            Self::Int(v) => v.len(),
            Self::Vec2(v) => v.len(),
            Self::Vec3(v) => v.len(),
            Self::Str(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Grow or shrink to `n` elements, filling with defaults.
    pub fn resize(&mut self, n: usize) {
        match self {
            Self::Float(v) => v.resize(n, 0.0),
            Self::Int(v) => v.resize(n, 0),
            Self::Vec2(v) => v.resize(n, Vec2::ZERO),
            Self::Vec3(v) => v.resize(n, Vec3::ZERO),
            Self::Str(v) => v.resize(n, String::new()),
        }
    }

    /// Append another array of the same kind. Returns false (and does nothing) on a type mismatch.
    pub fn append(&mut self, other: &AttribArray) -> bool {
        match (self, other) {
            (Self::Float(a), Self::Float(b)) => a.extend_from_slice(b),
            (Self::Int(a), Self::Int(b)) => a.extend_from_slice(b),
            (Self::Vec2(a), Self::Vec2(b)) => a.extend_from_slice(b),
            (Self::Vec3(a), Self::Vec3(b)) => a.extend_from_slice(b),
            (Self::Str(a), Self::Str(b)) => a.extend_from_slice(b),
            _ => return false,
        }
        true
    }

    /// Reverse the order of the elements in `range`.
    pub fn reverse_range(&mut self, range: std::ops::Range<usize>) {
        match self {
            Self::Float(v) => v[range].reverse(),
            Self::Int(v) => v[range].reverse(),
            Self::Vec2(v) => v[range].reverse(),
            Self::Vec3(v) => v[range].reverse(),
            Self::Str(v) => v[range].reverse(),
        }
    }
}
