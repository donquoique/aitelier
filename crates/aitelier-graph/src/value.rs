//! Parameter values as written in graph files, and their evaluation at a frame.
//!
//! In a graph file a numeric parameter can be:
//! - a number: `2.5`
//! - an expression string: `"height * 0.4"` or `{"expr": "sin(frame * 0.1)"}`
//! - keyframes: `{"keys": [[1, 0], [48, 10, "linear"]]}`
//!
//! Vectors and lists are JSON arrays whose elements can each be any of the above.

use aitelier_anim::{Channel, Interp, Key};
use glam::{Vec2, Vec3};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Bool(bool),
    Num(f64),
    /// A literal string for string parameters; an expression in numeric places.
    Str(String),
    List(Vec<Value>),
    Anim(AnimSpec),
    Expr(ExprSpec),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimSpec {
    pub keys: Vec<KeySpec>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum KeySpec {
    /// `[frame, value, "interp"]`
    WithInterp(f32, f32, Interp),
    /// `[frame, value]` (bezier)
    Plain(f32, f32),
    /// `{"frame": .., "value": .., "interp": ..}`
    Full(Key),
}

impl KeySpec {
    pub fn to_key(&self) -> Key {
        match *self {
            KeySpec::WithInterp(frame, value, interp) => Key {
                frame,
                value,
                interp,
            },
            KeySpec::Plain(frame, value) => Key {
                frame,
                value,
                interp: Interp::default(),
            },
            KeySpec::Full(k) => k,
        }
    }
}

impl AnimSpec {
    pub fn channel(&self) -> Channel {
        Channel::new(self.keys.iter().map(KeySpec::to_key).collect())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExprSpec {
    pub expr: String,
}

impl Value {
    /// Does this value change over time, given the names of time-varying variables?
    pub fn is_time_dependent(&self, animated_vars: &[String]) -> bool {
        match self {
            Value::Bool(_) | Value::Num(_) => false,
            Value::Anim(_) => true,
            Value::Str(s) | Value::Expr(ExprSpec { expr: s }) => identifiers(s)
                .any(|id| id == "frame" || id == "time" || animated_vars.iter().any(|v| v == id)),
            Value::List(items) => items.iter().any(|v| v.is_time_dependent(animated_vars)),
        }
    }
}

/// Identifier tokens in an expression.
pub fn identifiers(src: &str) -> impl Iterator<Item = &str> {
    src.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|t| {
            t.chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        })
}

/// Everything needed to turn a [`Value`] into a concrete number at a given frame.
pub struct EvalCtx<'a> {
    pub frame: f64,
    pub time: f64,
    pub fps: f64,
    /// Graph-level parameters, already evaluated at this frame.
    pub vars: &'a IndexMap<String, f64>,
    pub engine: &'a rhai::Engine,
}

impl EvalCtx<'_> {
    pub fn expr(&self, src: &str) -> Result<f64, String> {
        let mut scope = rhai::Scope::new();
        scope.push_constant("frame", self.frame);
        scope.push_constant("time", self.time);
        scope.push_constant("fps", self.fps);
        for (k, v) in self.vars {
            scope.push_constant(k.as_str(), *v);
        }
        let r = self
            .engine
            .eval_expression_with_scope::<rhai::Dynamic>(&mut scope, src)
            .map_err(|e| format!("expression `{src}`: {e}"))?;
        if let Ok(f) = r.as_float() {
            Ok(f)
        } else if let Ok(i) = r.as_int() {
            Ok(i as f64)
        } else if let Ok(b) = r.as_bool() {
            Ok(if b { 1.0 } else { 0.0 })
        } else {
            Err(format!("expression `{src}` did not return a number"))
        }
    }

    pub fn num(&self, v: &Value) -> Result<f64, String> {
        match v {
            Value::Num(n) => Ok(*n),
            Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
            Value::Str(s) | Value::Expr(ExprSpec { expr: s }) => self.expr(s),
            Value::Anim(a) => Ok(a.channel().eval(self.frame as f32) as f64),
            Value::List(_) => Err("expected a number, got a list".into()),
        }
    }

    pub fn list<'v>(&self, v: &'v Value) -> Result<&'v [Value], String> {
        match v {
            Value::List(items) => Ok(items),
            _ => Err("expected a list".into()),
        }
    }

    pub fn nums(&self, v: &Value) -> Result<Vec<f64>, String> {
        self.list(v)?.iter().map(|x| self.num(x)).collect()
    }

    pub fn vec2(&self, v: &Value) -> Result<Vec2, String> {
        match v {
            Value::List(_) => match self.nums(v)?.as_slice() {
                [x, y] => Ok(Vec2::new(*x as f32, *y as f32)),
                other => Err(format!("expected 2 numbers, got {}", other.len())),
            },
            _ => Ok(Vec2::splat(self.num(v)? as f32)),
        }
    }

    pub fn vec3(&self, v: &Value) -> Result<Vec3, String> {
        match v {
            Value::List(_) => match self.nums(v)?.as_slice() {
                [x, y, z] => Ok(Vec3::new(*x as f32, *y as f32, *z as f32)),
                [x, y] => Ok(Vec3::new(*x as f32, *y as f32, 0.0)),
                other => Err(format!("expected 3 numbers, got {}", other.len())),
            },
            _ => Ok(Vec3::splat(self.num(v)? as f32)),
        }
    }

    pub fn vec3_list(&self, v: &Value) -> Result<Vec<Vec3>, String> {
        self.list(v)?
            .iter()
            .enumerate()
            .map(|(i, p)| self.vec3(p).map_err(|e| format!("item {i}: {e}")))
            .collect()
    }

    pub fn boolean(&self, v: &Value) -> Result<bool, String> {
        match v {
            Value::Bool(b) => Ok(*b),
            _ => Ok(self.num(v)? != 0.0),
        }
    }

    pub fn string(&self, v: &Value) -> Result<String, String> {
        match v {
            Value::Str(s) => Ok(s.clone()),
            _ => Err("expected a string".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_eval(json: &str, frame: f64) -> f64 {
        let v: Value = serde_json::from_str(json).unwrap();
        let vars: IndexMap<String, f64> = [("height".to_string(), 20.0)].into_iter().collect();
        let engine = rhai::Engine::new();
        let ctx = EvalCtx {
            frame,
            time: 0.0,
            fps: 24.0,
            vars: &vars,
            engine: &engine,
        };
        ctx.num(&v).unwrap()
    }

    #[test]
    fn parses_all_forms() {
        assert_eq!(ctx_eval("3", 1.0), 3.0);
        assert_eq!(ctx_eval("\"height * 0.5\"", 1.0), 10.0);
        assert_eq!(ctx_eval("{\"expr\": \"frame * 2\"}", 5.0), 10.0);
        assert_eq!(
            ctx_eval("{\"keys\": [[1, 0, \"linear\"], [11, 10]]}", 6.0),
            5.0
        );
        assert_eq!(
            ctx_eval("{\"keys\": [{\"frame\": 1, \"value\": 4}]}", 6.0),
            4.0
        );
        assert!((ctx_eval("\"sin(PI() / 2.0)\"", 1.0) - 1.0).abs() < 1e-9);
        assert_eq!(ctx_eval("\"frame * 3\"", 2.0), 6.0);
    }

    #[test]
    fn time_dependence() {
        let anim = vec!["belly".to_string()];
        let check = |s: &str| {
            serde_json::from_str::<Value>(s)
                .unwrap()
                .is_time_dependent(&anim)
        };
        assert!(check("\"frame*2\""));
        assert!(check("[0, \"belly\", 0]"));
        assert!(!check("\"height\""));
        assert!(!check("\"catmull\""));
        assert!(!check("5"));
    }
}
