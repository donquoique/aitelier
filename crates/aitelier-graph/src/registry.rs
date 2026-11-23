//! Node types, their parameter definitions, and typed parameter access.

use std::collections::BTreeMap;
use std::sync::Arc;

use aitelier_geo::{Geometry, Vec2, Vec3};
use indexmap::IndexMap;
use serde::Serialize;

use crate::value::{EvalCtx, Value};
use crate::GraphError;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamKind {
    Float,
    Int,
    Bool,
    Vec2,
    Vec3,
    Vec3List,
    IntList,
    /// A string chosen from a fixed set of options.
    Menu,
    Str,
}

#[derive(Clone, Debug, Serialize)]
pub struct ParamDef {
    pub name: &'static str,
    pub kind: ParamKind,
    pub default: Value,
    pub doc: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<&'static str>,
}

fn num(x: f64) -> Value {
    Value::Num(x)
}

impl ParamDef {
    pub fn float(name: &'static str, default: f64, doc: &'static str) -> Self {
        Self {
            name,
            kind: ParamKind::Float,
            default: num(default),
            doc,
            options: vec![],
        }
    }
    pub fn int(name: &'static str, default: i64, doc: &'static str) -> Self {
        Self {
            name,
            kind: ParamKind::Int,
            default: num(default as f64),
            doc,
            options: vec![],
        }
    }
    pub fn boolean(name: &'static str, default: bool, doc: &'static str) -> Self {
        Self {
            name,
            kind: ParamKind::Bool,
            default: Value::Bool(default),
            doc,
            options: vec![],
        }
    }
    pub fn vec2(name: &'static str, d: [f64; 2], doc: &'static str) -> Self {
        Self {
            name,
            kind: ParamKind::Vec2,
            default: Value::List(d.iter().map(|x| num(*x)).collect()),
            doc,
            options: vec![],
        }
    }
    pub fn vec3(name: &'static str, d: [f64; 3], doc: &'static str) -> Self {
        Self {
            name,
            kind: ParamKind::Vec3,
            default: Value::List(d.iter().map(|x| num(*x)).collect()),
            doc,
            options: vec![],
        }
    }
    pub fn vec3_list(name: &'static str, d: &[[f64; 3]], doc: &'static str) -> Self {
        let default = Value::List(
            d.iter()
                .map(|p| Value::List(p.iter().map(|x| num(*x)).collect()))
                .collect(),
        );
        Self {
            name,
            kind: ParamKind::Vec3List,
            default,
            doc,
            options: vec![],
        }
    }
    pub fn int_list(name: &'static str, doc: &'static str) -> Self {
        Self {
            name,
            kind: ParamKind::IntList,
            default: Value::List(vec![]),
            doc,
            options: vec![],
        }
    }
    pub fn menu(name: &'static str, options: &[&'static str], doc: &'static str) -> Self {
        Self {
            name,
            kind: ParamKind::Menu,
            default: Value::Str(options[0].to_string()),
            doc,
            options: options.to_vec(),
        }
    }
}

/// A kind of node (box, revolve, transform ...).
pub trait NodeType: Send + Sync {
    fn type_name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    /// (minimum, maximum) number of inputs; `None` = unlimited.
    fn input_count(&self) -> (usize, Option<usize>);
    fn params(&self) -> &[ParamDef];
    fn cook(&self, inputs: &[Arc<Geometry>], p: &Params) -> Result<Geometry, GraphError>;
}

/// Parameter values of one node at one frame, with typed getters.
pub struct Params<'a> {
    pub node: &'a str,
    pub values: &'a IndexMap<String, Value>,
    pub defs: &'a [ParamDef],
    pub ctx: &'a EvalCtx<'a>,
}

impl Params<'_> {
    fn raw(&self, name: &str) -> &Value {
        if let Some(v) = self.values.get(name) {
            return v;
        }
        &self
            .defs
            .iter()
            .find(|d| d.name == name)
            .unwrap_or_else(|| {
                panic!("node type has no parameter `{name}` (bug in node definition)")
            })
            .default
    }

    fn wrap<T>(&self, name: &str, r: Result<T, String>) -> Result<T, GraphError> {
        r.map_err(|msg| GraphError::Param {
            node: self.node.to_string(),
            param: name.to_string(),
            msg,
        })
    }

    /// A cook error for this node.
    pub fn fail(&self, msg: impl Into<String>) -> GraphError {
        GraphError::Cook {
            node: self.node.to_string(),
            msg: msg.into(),
        }
    }

    pub fn f32(&self, name: &str) -> Result<f32, GraphError> {
        self.wrap(name, self.ctx.num(self.raw(name)).map(|x| x as f32))
    }
    pub fn int(&self, name: &str) -> Result<i64, GraphError> {
        self.wrap(name, self.ctx.num(self.raw(name)).map(|x| x.round() as i64))
    }
    /// Integer clamped to at least `min`.
    pub fn int_min(&self, name: &str, min: i64) -> Result<usize, GraphError> {
        Ok(self.int(name)?.max(min) as usize)
    }
    pub fn boolean(&self, name: &str) -> Result<bool, GraphError> {
        self.wrap(name, self.ctx.boolean(self.raw(name)))
    }
    pub fn vec2(&self, name: &str) -> Result<Vec2, GraphError> {
        self.wrap(name, self.ctx.vec2(self.raw(name)))
    }
    pub fn vec3(&self, name: &str) -> Result<Vec3, GraphError> {
        self.wrap(name, self.ctx.vec3(self.raw(name)))
    }
    pub fn vec3_list(&self, name: &str) -> Result<Vec<Vec3>, GraphError> {
        self.wrap(name, self.ctx.vec3_list(self.raw(name)))
    }
    pub fn int_list(&self, name: &str) -> Result<Vec<i64>, GraphError> {
        self.wrap(
            name,
            self.ctx
                .nums(self.raw(name))
                .map(|v| v.iter().map(|x| x.round() as i64).collect()),
        )
    }
    pub fn string(&self, name: &str) -> Result<String, GraphError> {
        self.wrap(name, self.ctx.string(self.raw(name)))
    }
    /// A menu choice, validated against the parameter's options.
    pub fn menu(&self, name: &str) -> Result<String, GraphError> {
        let s = self.string(name)?;
        let def = self
            .defs
            .iter()
            .find(|d| d.name == name)
            .expect("param exists");
        if def.options.iter().any(|o| *o == s) {
            Ok(s)
        } else {
            self.wrap(name, Err(format!("`{s}` is not one of {:?}", def.options)))
        }
    }
}

/// All known node types.
pub struct Registry {
    types: BTreeMap<&'static str, Box<dyn NodeType>>,
}

impl Registry {
    pub fn empty() -> Self {
        Self {
            types: BTreeMap::new(),
        }
    }

    /// Registry with every built-in node.
    pub fn with_builtins() -> Self {
        let mut r = Self::empty();
        crate::nodes::register_builtins(&mut r);
        r
    }

    pub fn register(&mut self, t: Box<dyn NodeType>) {
        self.types.insert(t.type_name(), t);
    }

    pub fn get(&self, name: &str) -> Option<&dyn NodeType> {
        self.types.get(name).map(|b| b.as_ref())
    }

    pub fn iter(&self) -> impl Iterator<Item = &dyn NodeType> {
        self.types.values().map(|b| b.as_ref())
    }
}
