//! The ASCII format.
//!
//! `Name: value, value… { children }`, one node a line: values continue
//! onto the next line only after a comma, and a node may open a block of
//! children. Values are numbers, `"strings"` and bare words (`Shading: Y`);
//! an array is `*count { a: n, n, … }` and becomes its node's one value, as
//! in binary. `;` starts a comment. Arrays are cut out whole here and
//! parsed side by side afterwards.
use super::arrays::{Deferred, float};
use super::{Document, Format, Node, Value};
use crate::fbx::{Error, error::At};
use kinogaki_studio_api::format::ReadBudget;

/// Whether `bytes` read as an ASCII FBX file: its comment line, or its
/// header node near the start.
pub(super) fn looks_like(bytes: &[u8]) -> bool {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let head = &bytes[..bytes.len().min(4096)];
    head.trim_ascii_start().starts_with(b"; FBX")
        || head.windows(19).any(|w| w == b"FBXHeaderExtension:")
}

pub(super) fn parse(bytes: &[u8], budget: &ReadBudget) -> Result<Document, Error> {
    let start = if bytes.starts_with(b"\xEF\xBB\xBF") {
        3
    } else {
        0
    };
    let mut parser = Parser {
        bytes,
        at: start,
        path: Vec::new(),
        arrays: Deferred::new(budget),
        budget,
    };
    let mut nodes = parser.nodes(0)?;
    let Parser { arrays, .. } = parser;
    arrays.decode_into(&mut nodes, budget, |at| At::Line(line(bytes, at)))?;
    let version = nodes
        .iter()
        .find(|n| n.name == "FBXHeaderExtension")
        .and_then(|n| n.child_int("FBXVersion"))
        .and_then(|v| u32::try_from(v).ok())
        .or_else(|| comment_version(&bytes[start..]))
        .unwrap_or(0);
    Ok(Document {
        version,
        format: Format::Ascii,
        nodes,
    })
}

/// The version of the first line's `; FBX 7.4.0 project file`, as 7400.
fn comment_version(bytes: &[u8]) -> Option<u32> {
    let first = bytes.split(|&b| b == b'\n').next()?;
    let text = std::str::from_utf8(first).ok()?;
    let digits = text.trim().strip_prefix("; FBX ")?.split(' ').next()?;
    let mut parts = digits.split('.').map(|p| p.parse::<u32>().ok());
    let (major, minor, patch) = (
        parts.next()??,
        parts.next()??,
        parts.next().flatten().unwrap_or(0),
    );
    Some(major * 1000 + minor * 100 + patch * 10)
}

/// The 1-based line of byte `at`.
fn line(bytes: &[u8], at: usize) -> u64 {
    1 + bytes[..at.min(bytes.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count() as u64
}

struct Parser<'a, 'o> {
    bytes: &'a [u8],
    at: usize,
    /// Child indices down to the node being read, for its arrays.
    path: Vec<u32>,
    arrays: Deferred<'a>,
    budget: &'o ReadBudget,
}

impl<'a> Parser<'a, '_> {
    fn error(&self, at: usize, message: impl Into<String>) -> Error {
        Error::malformed(At::Line(line(self.bytes, at)), message)
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    /// Skips spaces and tabs, and with `lines` line breaks and comments.
    fn skip(&mut self, lines: bool) {
        while let Some(b) = self.peek() {
            match b {
                b' ' | b'\t' | b'\r' => self.at += 1,
                b'\n' if lines => self.at += 1,
                b';' if lines => {
                    while self.peek().is_some_and(|b| b != b'\n') {
                        self.at += 1;
                    }
                }
                _ => break,
            }
        }
    }

    /// Nodes up to the end of the file (`depth` 0) or a closing `}`.
    fn nodes(&mut self, depth: usize) -> Result<Vec<Node>, Error> {
        if let Err(e) = self.budget.nest(depth) {
            return Err(match e.is_cancelled() {
                true => Error::Cancelled,
                false => self.error(self.at, e.message()),
            });
        }
        let mut nodes = Vec::new();
        loop {
            self.skip(true);
            match self.peek() {
                None if depth == 0 => break,
                None => return Err(self.error(self.at, "a block isn't closed")),
                Some(b'}') if depth > 0 => {
                    self.at += 1;
                    break;
                }
                Some(b) if b.is_ascii_alphanumeric() || b == b'_' => {
                    if depth == 0 {
                        self.budget.check()?;
                    }
                    self.path.push(nodes.len() as u32);
                    let node = self.node(depth)?;
                    self.path.pop();
                    nodes.push(node);
                }
                Some(_) => return Err(self.error(self.at, "expected a node's name")),
            }
        }
        Ok(nodes)
    }

    fn node(&mut self, depth: usize) -> Result<Node, Error> {
        let start = self.at;
        let name = self.word();
        if self.peek() != Some(b':') {
            return Err(self.error(start, format!("`{name}` isn't followed by `:`")));
        }
        self.at += 1;
        let values = self.values()?;
        let array = matches!(values.last(), Some(Value::Array(_)));
        self.skip(true);
        let children = if !array && self.peek() == Some(b'{') {
            self.at += 1;
            self.nodes(depth + 1)?
        } else {
            Vec::new()
        };
        Ok(Node {
            name,
            values,
            children,
        })
    }

    /// The values after a name, up to the end of its line.
    fn values(&mut self) -> Result<Vec<Value>, Error> {
        let mut values = Vec::new();
        loop {
            self.skip(false);
            match self.peek() {
                // An empty value (`Content: ,`) is no value.
                Some(b',') => {
                    self.at += 1;
                    self.skip(true);
                    continue;
                }
                Some(b'"') => values.push(self.string()?),
                Some(b'*') => {
                    let value = self.array(values.len())?;
                    values.push(value);
                    return Ok(values);
                }
                None | Some(b'\n' | b'{' | b'}' | b';') => return Ok(values),
                Some(_) => values.push(self.token()),
            }
            self.skip(false);
            if self.peek() != Some(b',') {
                return Ok(values);
            }
        }
    }

    /// An identifier: letters, digits and `_`.
    fn word(&mut self) -> String {
        let start = self.at;
        while self
            .peek()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            self.at += 1;
        }
        String::from_utf8_lossy(&self.bytes[start..self.at]).into_owned()
    }

    fn string(&mut self) -> Result<Value, Error> {
        let start = self.at;
        self.at += 1;
        let len = self.bytes[self.at..]
            .iter()
            .position(|&b| b == b'"')
            .ok_or_else(|| self.error(start, "a string isn't closed"))?;
        let text = String::from_utf8_lossy(&self.bytes[self.at..self.at + len]);
        self.at += len + 1;
        // The FBX SDK writes a quote inside a string as `&quot;`.
        Ok(Value::String(if text.contains("&quot;") {
            text.replace("&quot;", "\"")
        } else {
            text.into_owned()
        }))
    }

    /// A number, or else a bare word.
    fn token(&mut self) -> Value {
        let start = self.at;
        while self.peek().is_some_and(|b| {
            !matches!(
                b,
                b',' | b'{' | b'}' | b'"' | b';' | b' ' | b'\t' | b'\r' | b'\n'
            )
        }) {
            self.at += 1;
        }
        let token = String::from_utf8_lossy(&self.bytes[start..self.at]);
        if let Ok(v) = token.parse::<i64>() {
            Value::Int(v)
        } else if token.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '-' | '+' | '.'))
            && let Ok(v) = float(&token)
        {
            Value::Float(v)
        } else {
            Value::String(token.into_owned())
        }
    }

    /// `*count { a: n, n, … }`, its numbers deferred, as value `index`.
    fn array(&mut self, index: usize) -> Result<Value, Error> {
        let start = self.at;
        self.at += 1;
        let digits = self.word();
        let count: usize = digits
            .parse()
            .map_err(|_| self.error(start, format!("`*{digits}` isn't an array's length")))?;
        self.skip(true);
        if self.peek() != Some(b'{') {
            return Err(self.error(self.at, "an array's `{` is missing"));
        }
        self.at += 1;
        self.skip(true);
        let data_start = if self.bytes[self.at..].starts_with(b"a:") {
            self.at + 2
        } else {
            self.at
        };
        let len = self.bytes[data_start..]
            .iter()
            .position(|&b| b == b'}')
            .ok_or_else(|| self.error(start, "an array isn't closed"))?;
        let data = &self.bytes[data_start..data_start + len];
        self.at = data_start + len + 1;
        self.arrays
            .text(&self.path, index, count, data, start)
            .map_err(|why| why.at(At::Line(line(self.bytes, start))))?;
        Ok(Value::Array(super::Array::default()))
    }
}
