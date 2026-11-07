//! Kaydara's binary format.
//!
//! A 27-byte header (magic, `0x1A 0x00`, the version), then node records:
//! the offset where the record ends, its value count and their bytes, its
//! name, its values, then its children ended by a record of zeros. From
//! 7.5 on the three header fields are 64-bit. Values are typed by a letter:
//! `C` `Y` `I` `L` `F` `D` scalars, `S` strings and `R` bytes with their
//! lengths, and `b` `i` `l` `f` `d` arrays of a count, an encoding (0 raw,
//! 1 zlib) and their byte length. The footer after the last record holds
//! nothing the reader needs.
use super::arrays::{Deferred, Kind};
use super::{Document, Format, Node, Value};
use crate::fbx::{Error, Options, error::At};

/// The first bytes of every binary file.
pub(super) const MAGIC: &[u8] = b"Kaydara FBX Binary  \0";

/// The header's length, and where the first record starts.
const HEADER: usize = 27;

pub(super) fn parse(bytes: &[u8], options: &Options) -> Result<Document, Error> {
    let version = bytes
        .get(23..HEADER)
        .map(|v| u32::from_le_bytes([v[0], v[1], v[2], v[3]]))
        .ok_or_else(|| Error::malformed(At::Byte(0), "the header ends early"))?;
    if version >= 8000 {
        return Err(Error::Unsupported(format!(
            "FBX version {version} is newer than this reader knows (7.1 to 7.7)"
        )));
    }
    let mut reader = Reader {
        bytes,
        at: HEADER,
        wide: version >= 7500,
        path: Vec::new(),
        arrays: Deferred::new(options.limits),
        options,
    };
    let mut nodes = reader.list(bytes.len(), 0, true)?;
    let Reader { arrays, .. } = reader;
    arrays.decode_into(&mut nodes, options, |at| At::Byte(at as u64))?;
    Ok(Document {
        version,
        format: Format::Binary,
        nodes,
    })
}

struct Reader<'a, 'o> {
    bytes: &'a [u8],
    at: usize,
    /// 64-bit record headers (7.5 and later).
    wide: bool,
    /// Child indices down to the node being read, for its arrays.
    path: Vec<u32>,
    arrays: Deferred<'a>,
    options: &'o Options<'o>,
}

/// A record's header.
struct Header {
    end: usize,
    values: usize,
    values_len: usize,
}

impl<'a> Reader<'a, '_> {
    fn error(&self, at: usize, message: impl Into<String>) -> Error {
        Error::malformed(At::Byte(at as u64), message)
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], Error> {
        let bytes = self
            .at
            .checked_add(len)
            .and_then(|end| self.bytes.get(self.at..end))
            .ok_or_else(|| self.error(self.at, "the file ends early"))?;
        self.at += len;
        Ok(bytes)
    }

    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(
            *self.take(4)?.as_array().expect("four bytes"),
        ))
    }

    fn u64(&mut self) -> Result<u64, Error> {
        Ok(u64::from_le_bytes(
            *self.take(8)?.as_array().expect("eight bytes"),
        ))
    }

    /// A header field: 32 or 64 bits, as an offset or count.
    fn field(&mut self) -> Result<usize, Error> {
        let at = self.at;
        let value = if self.wide {
            self.u64()?
        } else {
            u64::from(self.u32()?)
        };
        usize::try_from(value).map_err(|_| self.error(at, "a record field is out of range"))
    }

    /// Records up to `end` or a record of zeros: the top level must end
    /// with one, so a file cut short never reads as a smaller one.
    fn list(&mut self, end: usize, depth: usize, top: bool) -> Result<Vec<Node>, Error> {
        if depth > self.options.limits.depth {
            return Err(self.error(self.at, "nodes nest deeper than the depth budget"));
        }
        let mut nodes = Vec::new();
        while self.at < end || top {
            if top {
                self.options.check()?;
            }
            let start = self.at;
            let header = Header {
                end: self.field()?,
                values: self.field()?,
                values_len: self.field()?,
            };
            let name_len = usize::from(self.u8()?);
            if header.end == 0 && header.values == 0 && header.values_len == 0 && name_len == 0 {
                break;
            }
            if header.end <= start || header.end > end {
                return Err(self.error(start, "a record ends outside its parent"));
            }
            let name = self.take(name_len)?;
            self.path.push(nodes.len() as u32);
            let node = self.node(text(name), header, depth)?;
            self.path.pop();
            nodes.push(node);
        }
        Ok(nodes)
    }

    fn node(&mut self, name: String, header: Header, depth: usize) -> Result<Node, Error> {
        let values_end = self
            .at
            .checked_add(header.values_len)
            .filter(|&e| e <= header.end)
            .ok_or_else(|| self.error(self.at, "a record's values pass its end"))?;
        // Each value takes at least two bytes: the count can't pass that.
        if header.values.saturating_mul(2) > header.values_len {
            return Err(self.error(self.at, "a record has more values than bytes"));
        }
        let mut values = Vec::with_capacity(header.values);
        for _ in 0..header.values {
            let value = self.value(values.len())?;
            values.push(value);
        }
        if self.at != values_end {
            return Err(self.error(self.at, "a record's values don't fill their length"));
        }
        let children = self.list(header.end, depth + 1, false)?;
        if self.at > header.end {
            return Err(self.error(self.at, "a record's children pass its end"));
        }
        self.at = header.end;
        Ok(Node {
            name,
            values,
            children,
        })
    }

    /// Value `index` of the node being read.
    fn value(&mut self, index: usize) -> Result<Value, Error> {
        let at = self.at;
        let code = self.u8()?;
        Ok(match code {
            b'C' => Value::Bool(self.u8()? != 0),
            b'Y' => Value::Int(i64::from(i16::from_le_bytes(
                *self.take(2)?.as_array().expect("two"),
            ))),
            b'I' => Value::Int(i64::from(self.u32()? as i32)),
            b'L' => Value::Int(self.u64()? as i64),
            b'F' => Value::Float(f64::from(f32::from_bits(self.u32()?))),
            b'D' => Value::Float(f64::from_bits(self.u64()?)),
            b'S' | b'R' => {
                let len = self.u32()? as usize;
                let bytes = self.take(len)?;
                if code == b'S' {
                    Value::String(object_name(bytes))
                } else {
                    Value::Bytes(bytes.to_vec())
                }
            }
            b'b' | b'i' | b'l' | b'f' | b'd' => {
                let kind = match code {
                    b'b' => Kind::Bool,
                    b'i' => Kind::I32,
                    b'l' => Kind::I64,
                    b'f' => Kind::F32,
                    _ => Kind::F64,
                };
                let count = self.u32()? as usize;
                let compressed = match self.u32()? {
                    0 => false,
                    1 => true,
                    other => return Err(self.error(at, format!("an array's encoding is {other}"))),
                };
                let len = self.u32()? as usize;
                let data = self.take(len)?;
                self.arrays
                    .binary(&self.path, index, kind, count, compressed, data, at)
                    .map_err(|message| self.error(at, message))?;
                Value::Array(super::Array::default())
            }
            other => return Err(self.error(at, format!("a value of unknown type {other:#04x}"))),
        })
    }
}

/// Bytes as text, invalid UTF-8 replaced.
fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A string, with binary's `Name\0\x01Class` written `Class::Name`.
fn object_name(bytes: &[u8]) -> String {
    match bytes.windows(2).position(|w| w == b"\0\x01") {
        Some(split) => format!("{}::{}", text(&bytes[split + 2..]), text(&bytes[..split])),
        None => text(bytes),
    }
}
