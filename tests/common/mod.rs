//! Test support: the committed fixtures, and a small writer of synthetic
//! FBX files (binary and ASCII) from a node tree, compressing arrays as
//! exporters do.
#![allow(dead_code)]

use kinogaki_fbx::fbx::tree::{Array, Node, Value};
use std::path::PathBuf;

/// A committed fixture's bytes, by its path under `tests/fixtures`.
pub fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Every committed fixture's path under `tests/fixtures`.
pub fn fixtures() -> Vec<String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut all = Vec::new();
    for dir in ["blender", "houdini"] {
        for entry in std::fs::read_dir(root.join(dir)).expect("fixtures") {
            let name = entry
                .expect("entry")
                .file_name()
                .into_string()
                .expect("utf-8");
            if name.ends_with(".fbx") {
                all.push(format!("{dir}/{name}"));
            }
        }
    }
    all.sort();
    all
}

/// A node.
pub fn node(name: &str, values: Vec<Value>, children: Vec<Node>) -> Node {
    Node {
        name: name.into(),
        values,
        children,
    }
}

/// A string value.
pub fn s(text: &str) -> Value {
    Value::String(text.into())
}

/// `P: "Name", "Type", "Label", "Flags", values…`.
pub fn p(name: &str, kind: &str, values: Vec<Value>) -> Node {
    let mut all = vec![s(name), s(kind), s(""), s("")];
    all.extend(values);
    node("P", all, vec![])
}

/// A binary file of `nodes` at `version`; arrays compressed or raw.
pub fn binary(nodes: &[Node], version: u32, compress: bool) -> Vec<u8> {
    let mut out = b"Kaydara FBX Binary  \0\x1a\0".to_vec();
    out.extend(version.to_le_bytes());
    let wide = version >= 7500;
    for n in nodes {
        record(&mut out, n, wide, compress);
    }
    out.extend(vec![0u8; if wide { 25 } else { 13 }]);
    // A footer as exporters write one; readers skip it.
    out.extend([
        0xfa, 0xbc, 0xab, 0x09, 0xd0, 0xc8, 0xd4, 0x66, 0xb1, 0x76, 0xfb, 0x83, 0x1c, 0xf7, 0x26,
        0x7e,
    ]);
    out.extend([0u8; 20]);
    out
}

fn record(out: &mut Vec<u8>, n: &Node, wide: bool, compress: bool) {
    let start = out.len();
    let field = if wide { 8 } else { 4 };
    out.extend(vec![0u8; field * 3]);
    out.push(n.name.len() as u8);
    out.extend(n.name.as_bytes());
    let values_start = out.len();
    for v in &n.values {
        value(out, v, compress);
    }
    let values_len = out.len() - values_start;
    if !n.children.is_empty() {
        for c in &n.children {
            record(out, c, wide, compress);
        }
        out.extend(vec![0u8; field * 3 + 1]);
    }
    let header = [out.len(), n.values.len(), values_len];
    for (i, h) in header.into_iter().enumerate() {
        let at = start + i * field;
        if wide {
            out[at..at + 8].copy_from_slice(&(h as u64).to_le_bytes());
        } else {
            out[at..at + 4].copy_from_slice(&(h as u32).to_le_bytes());
        }
    }
}

fn value(out: &mut Vec<u8>, v: &Value, compress: bool) {
    match v {
        Value::Bool(b) => out.extend([b'C', u8::from(*b)]),
        Value::Int(i) => match i32::try_from(*i) {
            Ok(i) => {
                out.push(b'I');
                out.extend(i.to_le_bytes());
            }
            Err(_) => {
                out.push(b'L');
                out.extend(i.to_le_bytes());
            }
        },
        Value::Float(f) => {
            out.push(b'D');
            out.extend(f.to_le_bytes());
        }
        Value::String(text) => {
            // Binary writes `Class::Name` as `Name\0\x01Class`.
            let bytes = match text.split_once("::") {
                Some((class, name)) => [name.as_bytes(), b"\0\x01", class.as_bytes()].concat(),
                None => text.as_bytes().to_vec(),
            };
            out.push(b'S');
            out.extend((bytes.len() as u32).to_le_bytes());
            out.extend(bytes);
        }
        Value::Bytes(bytes) => {
            out.push(b'R');
            out.extend((bytes.len() as u32).to_le_bytes());
            out.extend(bytes);
        }
        Value::Array(array) => {
            let (code, count, data): (u8, usize, Vec<u8>) = match array {
                Array::Bool(v) => (b'b', v.len(), v.iter().map(|&b| u8::from(b)).collect()),
                Array::I32(v) => (
                    b'i',
                    v.len(),
                    v.iter().flat_map(|x| x.to_le_bytes()).collect(),
                ),
                Array::I64(v) => (
                    b'l',
                    v.len(),
                    v.iter().flat_map(|x| x.to_le_bytes()).collect(),
                ),
                Array::F32(v) => (
                    b'f',
                    v.len(),
                    v.iter().flat_map(|x| x.to_le_bytes()).collect(),
                ),
                Array::F64(v) => (
                    b'd',
                    v.len(),
                    v.iter().flat_map(|x| x.to_le_bytes()).collect(),
                ),
            };
            let data = if compress {
                kinogaki_deflate::zlib_compress(&data, kinogaki_deflate::Level::Fast)
            } else {
                data
            };
            out.push(code);
            out.extend((count as u32).to_le_bytes());
            out.extend(u32::from(compress).to_le_bytes());
            out.extend((data.len() as u32).to_le_bytes());
            out.extend(data);
        }
    }
}

/// An ASCII file of `nodes`, its version in the first line's comment.
pub fn ascii(nodes: &[Node], version: u32) -> String {
    let mut out = format!(
        "; FBX {}.{}.0 project file\n; written by the tests\n\n",
        version / 1000,
        version % 1000 / 100
    );
    for n in nodes {
        ascii_node(&mut out, n, 0);
    }
    out
}

fn ascii_node(out: &mut String, n: &Node, depth: usize) {
    let indent = "\t".repeat(depth);
    out.push_str(&format!("{indent}{}: ", n.name));
    let values: Vec<String> = n
        .values
        .iter()
        .map(|v| match v {
            Value::Bool(b) => i32::from(*b).to_string(),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => format!("{f:?}"),
            Value::String(text) => format!("\"{}\"", text.replace('"', "&quot;")),
            // As the FBX SDK writes `Content`: base64 after an empty value.
            Value::Bytes(bytes) => format!(",\n\"{}\"", base64(bytes)),
            Value::Array(a) => {
                let numbers: Vec<String> = match a {
                    Array::Bool(v) => v.iter().map(|&b| i32::from(b).to_string()).collect(),
                    Array::I32(v) => v.iter().map(|x| x.to_string()).collect(),
                    Array::I64(v) => v.iter().map(|x| x.to_string()).collect(),
                    Array::F32(v) => v.iter().map(|x| format!("{x:?}")).collect(),
                    Array::F64(v) => v.iter().map(|x| format!("{x:?}")).collect(),
                };
                // Lines of a few numbers, as the FBX SDK wraps them.
                let lines: Vec<String> = numbers.chunks(7).map(|c| c.join(",")).collect();
                format!(
                    "*{} {{\n{indent}\ta: {}\n{indent}}}",
                    a.len(),
                    lines.join(",\n")
                )
            }
        })
        .collect();
    out.push_str(&values.join(","));
    if n.children.is_empty() {
        out.push('\n');
        return;
    }
    out.push_str(" {\n");
    for c in &n.children {
        ascii_node(out, c, depth + 1);
    }
    out.push_str(&format!("{indent}}}\n"));
}

/// Standard base64 with padding.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            let pad = i > chunk.len();
            out.push(if pad {
                '='
            } else {
                char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize])
            });
        }
    }
    out
}

/// Deterministic pseudo-random numbers (xorshift64*).
pub struct Random(pub u64);

impl Random {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}
