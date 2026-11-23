//! Geometry core for AItelier.
//!
//! The data model follows the Houdini idea: geometry is a list of **points**,
//! **primitives** (polygons and polylines) that reference points through
//! **vertices**, and named **attributes** stored on points, vertices,
//! primitives or the whole geometry (detail).

pub mod attrib;
pub mod geometry;
pub mod obj;

pub use attrib::{AttribArray, AttribKind, AttribValue};
pub use geometry::{Geometry, PrimKind, Primitive, TopologyReport};
pub use glam::{Mat3, Mat4, Quat, Vec2, Vec3};
