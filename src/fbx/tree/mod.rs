//! The node tree every FBX file is: named nodes holding values and
//! children. Binary and ASCII files read into the same tree, so everything
//! above it never asks which a file was.
//!
//! Object names read the same from both: binary's `Name\0\x01Class` is
//! written `Class::Name`, as ASCII spells it. Arrays are decoded after the
//! tree is read, all of them side by side (`arrays`).
mod arrays;
mod ascii;
mod binary;

use crate::fbx::{Error, Options};
use std::borrow::Cow;

/// A file's node tree.
#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    /// The FBX version, as `7400` for 7.4; `0` when an ASCII file says none.
    pub version: u32,
    /// How the file was written.
    pub format: Format,
    /// The top-level nodes (`FBXHeaderExtension`, `GlobalSettings`,
    /// `Definitions`, `Objects`, `Connections`…).
    pub nodes: Vec<Node>,
}

/// How a file was written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// Kaydara's binary format.
    Binary,
    /// The ASCII format.
    Ascii,
}

/// One node: a name, values, and nodes under it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Node {
    /// The node's name (`Model`, `Vertices`, `P`…).
    pub name: String,
    /// Its values, in order.
    pub values: Vec<Value>,
    /// The nodes under it, in order.
    pub children: Vec<Node>,
}

/// One value of a node.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// A binary `C` value.
    Bool(bool),
    /// An integer of any width.
    Int(i64),
    /// A floating-point number of either width.
    Float(f64),
    /// A string, or an ASCII file's bare word (`Shading: Y`).
    String(String),
    /// Raw bytes (binary `R`; ASCII's base64 is decoded where it is used).
    Bytes(Vec<u8>),
    /// An array.
    Array(Array),
}

/// An array value, as the file typed it. ASCII files say no element type:
/// their arrays are `I64` when every element is an integer, else `F64`.
#[derive(Clone, Debug, PartialEq)]
pub enum Array {
    /// Booleans (binary `b`).
    Bool(Vec<bool>),
    /// 32-bit integers (binary `i`).
    I32(Vec<i32>),
    /// 64-bit integers (binary `l`).
    I64(Vec<i64>),
    /// 32-bit floats (binary `f`).
    F32(Vec<f32>),
    /// 64-bit floats (binary `d`).
    F64(Vec<f64>),
}

impl Default for Array {
    /// No elements.
    fn default() -> Self {
        Array::Bool(Vec::new())
    }
}

/// Reads a binary or ASCII file's tree.
pub fn parse(bytes: &[u8]) -> Result<Document, Error> {
    parse_with(bytes, &Options::default())
}

/// How a file starting with `head` was written, when it is FBX: binary by
/// Kaydara's signature, ASCII by its first comment or header node.
pub fn detect(head: &[u8]) -> Option<Format> {
    if head.starts_with(binary::MAGIC) {
        Some(Format::Binary)
    } else if ascii::looks_like(head) {
        Some(Format::Ascii)
    } else {
        None
    }
}

/// [`parse`] with budgets and cancellation.
pub fn parse_with(bytes: &[u8], options: &Options) -> Result<Document, Error> {
    if bytes.starts_with(binary::MAGIC) {
        binary::parse(bytes, options)
    } else if ascii::looks_like(bytes) {
        ascii::parse(bytes, options)
    } else {
        Err(Error::Unsupported("not an FBX file".into()))
    }
}

impl Document {
    /// The first top-level node named `name`.
    pub fn node(&self, name: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.name == name)
    }
}

impl Node {
    /// The first child named `name`.
    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|n| n.name == name)
    }

    /// Every child named `name`, in order.
    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |n| n.name == name)
    }

    /// Value `i` as an integer.
    pub fn int(&self, i: usize) -> Option<i64> {
        self.values.get(i)?.as_int()
    }

    /// Value `i` as a number.
    pub fn float(&self, i: usize) -> Option<f64> {
        self.values.get(i)?.as_float()
    }

    /// Value `i` as a string.
    pub fn str(&self, i: usize) -> Option<&str> {
        self.values.get(i)?.as_str()
    }

    /// The first value of the child named `name`, as a string.
    pub fn child_str(&self, name: &str) -> Option<&str> {
        self.child(name)?.str(0)
    }

    /// The first value of the child named `name`, as an integer.
    pub fn child_int(&self, name: &str) -> Option<i64> {
        self.child(name)?.int(0)
    }

    /// Takes the first array value of the child named `name`, leaving an
    /// empty one: the model moves arrays out of the tree instead of copying.
    pub fn take_array(&mut self, name: &str) -> Option<Array> {
        let child = self.children.iter_mut().find(|n| n.name == name)?;
        child.values.iter_mut().find_map(|value| match value {
            Value::Array(array) => Some(std::mem::take(array)),
            _ => None,
        })
    }
}

impl Value {
    /// An integer, a boolean as 0 or 1, or a float that is a whole number.
    pub fn as_int(&self) -> Option<i64> {
        match *self {
            Value::Int(v) => Some(v),
            Value::Bool(v) => Some(i64::from(v)),
            Value::Float(v) if v.fract() == 0.0 && v.abs() < 9.2e18 => Some(v as i64),
            _ => None,
        }
    }

    /// A number of any kind.
    pub fn as_float(&self) -> Option<f64> {
        match *self {
            Value::Float(v) => Some(v),
            Value::Int(v) => Some(v as f64),
            Value::Bool(v) => Some(f64::from(u8::from(v))),
            _ => None,
        }
    }

    /// A string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
}

impl Array {
    /// The number of elements.
    pub fn len(&self) -> usize {
        match self {
            Array::Bool(v) => v.len(),
            Array::I32(v) => v.len(),
            Array::I64(v) => v.len(),
            Array::F32(v) => v.len(),
            Array::F64(v) => v.len(),
        }
    }

    /// Whether it holds no elements.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The elements as 64-bit floats: moved when they are, else converted.
    pub fn into_f64s(self) -> Vec<f64> {
        match self {
            Array::F64(v) => v,
            Array::F32(v) => v.into_iter().map(f64::from).collect(),
            Array::I32(v) => v.into_iter().map(f64::from).collect(),
            Array::I64(v) => v.into_iter().map(|x| x as f64).collect(),
            Array::Bool(v) => v.into_iter().map(|x| f64::from(u8::from(x))).collect(),
        }
    }

    /// The elements as 32-bit integers: moved when they are, else
    /// converted; `None` when one is not a 32-bit integer.
    pub fn into_i32s(self) -> Option<Vec<i32>> {
        match self {
            Array::I32(v) => Some(v),
            Array::I64(v) => v.into_iter().map(|x| i32::try_from(x).ok()).collect(),
            Array::Bool(v) => Some(v.into_iter().map(i32::from).collect()),
            Array::F32(v) => v.into_iter().map(|x| whole(f64::from(x))).collect(),
            Array::F64(v) => v.into_iter().map(whole).collect(),
        }
    }

    /// The elements as 64-bit floats, borrowed when they are.
    pub fn to_f64s(&self) -> Cow<'_, [f64]> {
        match self {
            Array::F64(v) => Cow::Borrowed(v),
            other => Cow::Owned(other.clone().into_f64s()),
        }
    }
}

/// `x` as an `i32` when it is a whole number in range.
fn whole(x: f64) -> Option<i32> {
    (x.fract() == 0.0 && x >= f64::from(i32::MIN) && x <= f64::from(i32::MAX)).then_some(x as i32)
}
