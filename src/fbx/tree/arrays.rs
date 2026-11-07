//! Arrays are decoded once the tree is read, all of them side by side: a
//! binary array is a task (inflated when compressed), an ASCII array is cut
//! at commas into tasks of about a megabyte. Each array takes the place its
//! node kept for it. Each array is charged to the read's budget from its
//! header, before anything is allocated: its elements, and the bytes it
//! inflates to besides, which a compressed array's own bytes must be able
//! to hold.
use super::{Array, Node, Value};
use crate::fbx::{Error, error::At};
use kinogaki_studio_api::format::ReadBudget;
use kinogaki_studio_api::geometry::parallel;
use std::ops::Range;
use std::sync::atomic::AtomicBool;

/// What an array's elements are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Bool,
    I32,
    I64,
    F32,
    F64,
}

impl Kind {
    /// Bytes of one element in a binary file (and decoded, for ASCII's).
    pub(super) fn size(self) -> usize {
        match self {
            Kind::Bool => 1,
            Kind::I32 | Kind::F32 => 4,
            Kind::I64 | Kind::F64 => 8,
        }
    }
}

/// One piece of decoding.
enum Task<'a> {
    /// A binary array's little-endian elements, raw or one zlib stream.
    Binary {
        kind: Kind,
        count: usize,
        compressed: bool,
        data: &'a [u8],
    },
    /// A run of an ASCII array's comma-separated numbers.
    Text { data: &'a [u8] },
}

/// Where an array goes, and the tasks that make it.
struct Slot {
    /// Child indices from the top level down to the array's node.
    path: Vec<u32>,
    /// The value it is in that node.
    value: usize,
    tasks: Range<usize>,
    /// The elements its header promises.
    count: usize,
    /// Its offset in the file, to say where it is wrong.
    at: usize,
}

/// Arrays waiting to be decoded.
pub(super) struct Deferred<'a> {
    tasks: Vec<Task<'a>>,
    slots: Vec<Slot>,
    budget: ReadBudget,
    /// Bytes of the arrays' elements, together.
    total: usize,
}

/// Why an array isn't read: its header breaks the format, or the budget
/// refuses what it asks.
pub(super) enum Refused {
    Malformed(String),
    Budget(kinogaki_studio_api::Error),
}

impl Refused {
    /// The refusal as the read's error, at `at`.
    pub(super) fn at(self, at: At) -> Error {
        match self {
            Refused::Malformed(message) => Error::malformed(at, message),
            Refused::Budget(e) => Error::from(e),
        }
    }
}

/// An ASCII array is cut into tasks of at least this many bytes.
const TEXT_TASK: usize = 1 << 20;

/// Arrays decoding to fewer bytes than this decode on the calling thread.
const SMALL: usize = 1 << 20;

impl<'a> Deferred<'a> {
    pub(super) fn new(budget: &ReadBudget) -> Self {
        Self {
            tasks: Vec::new(),
            slots: Vec::new(),
            budget: budget.clone(),
            total: 0,
        }
    }

    /// A binary array of `count` elements at `data`, for value `value` of
    /// the node at `path`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn binary(
        &mut self,
        path: &[u32],
        value: usize,
        kind: Kind,
        count: usize,
        compressed: bool,
        data: &'a [u8],
        at: usize,
    ) -> Result<(), Refused> {
        let bytes = count.saturating_mul(kind.size());
        if !compressed && data.len() != bytes {
            return Err(Refused::Malformed(format!(
                "an array of {count} elements holds {} bytes",
                data.len()
            )));
        }
        if compressed {
            // Inflated beside its elements, from bytes that can hold them.
            // More than DEFLATE can code is a broken file, not a large one.
            let encoded = data.len() as u64;
            (self.budget.expand(encoded, bytes as u64)).map_err(|e| match e.is_cancelled() {
                true => Refused::Budget(e),
                false => Refused::Malformed(format!(
                    "{encoded} compressed bytes can't hold {count} elements"
                )),
            })?;
            self.reserve(count, kind.size())?;
        }
        self.reserve(count, kind.size())?;
        self.push(path, value, count, at, |tasks| {
            tasks.push(Task::Binary {
                kind,
                count,
                compressed,
                data,
            })
        });
        Ok(())
    }

    /// An ASCII array of `count` numbers in `data`, cut at commas.
    pub(super) fn text(
        &mut self,
        path: &[u32],
        value: usize,
        count: usize,
        data: &'a [u8],
        at: usize,
    ) -> Result<(), Refused> {
        self.reserve(count, Kind::F64.size())?;
        self.push(path, value, count, at, |tasks| {
            let mut rest = data;
            while rest.len() > TEXT_TASK {
                let Some(comma) = rest[TEXT_TASK..].iter().position(|&b| b == b',') else {
                    break;
                };
                let (task, next) = rest.split_at(TEXT_TASK + comma + 1);
                tasks.push(Task::Text { data: task });
                rest = next;
            }
            tasks.push(Task::Text { data: rest });
        });
        Ok(())
    }

    /// Charges `count` elements of `size` to the budget.
    fn reserve(&mut self, count: usize, size: usize) -> Result<(), Refused> {
        let bytes = count.saturating_mul(size);
        (self.budget.allocate(bytes as u64)).map_err(Refused::Budget)?;
        self.total = self.total.saturating_add(bytes);
        Ok(())
    }

    fn push(
        &mut self,
        path: &[u32],
        value: usize,
        count: usize,
        at: usize,
        tasks: impl FnOnce(&mut Vec<Task<'a>>),
    ) {
        let start = self.tasks.len();
        tasks(&mut self.tasks);
        self.slots.push(Slot {
            path: path.to_vec(),
            value,
            tasks: start..self.tasks.len(),
            count,
            at,
        });
    }

    /// Decodes every array side by side and puts each in its place;
    /// `locate` turns a byte offset into a place in the file.
    pub(super) fn decode_into(
        self,
        nodes: &mut [Node],
        budget: &ReadBudget,
        locate: impl Fn(usize) -> At,
    ) -> Result<(), Error> {
        let tasks = &self.tasks;
        let stop = budget.cancel().shared_flag();
        let task = |i: usize| {
            budget.check()?;
            Ok(decode(&tasks[i], &stop))
        };
        // A few small arrays decode faster than threads start.
        let decoded = if self.total < SMALL {
            (0..tasks.len()).map(task).collect()
        } else {
            parallel::map(tasks.len(), task)
        };
        let mut decoded = decoded.into_iter().collect::<Result<Vec<_>, Error>>()?;
        budget.check()?;
        for slot in self.slots {
            let pieces = decoded[slot.tasks]
                .iter_mut()
                .map(|piece| std::mem::replace(piece, Ok(Array::default())));
            let array = join(pieces, slot.count)
                .map_err(|message| Error::malformed(locate(slot.at), message))?;
            let node = node_at(nodes, &slot.path);
            node.values[slot.value] = Value::Array(array);
        }
        Ok(())
    }
}

/// A task's elements, or why they are wrong.
type Piece = Result<Array, String>;

/// A task's elements; an inflation stops once `stop` is raised (the read
/// cancelled), which the read then says.
fn decode(task: &Task, stop: &AtomicBool) -> Piece {
    match *task {
        Task::Binary {
            kind,
            count,
            compressed,
            data,
        } => binary(kind, count, compressed, data, stop),
        Task::Text { data } => numbers(data),
    }
}

/// A binary array's elements.
fn binary(kind: Kind, count: usize, compressed: bool, data: &[u8], stop: &AtomicBool) -> Piece {
    let bytes = count * kind.size();
    let inflated;
    let data = if compressed {
        inflated = kinogaki_deflate::zlib_decompress_until(data, bytes, stop)
            .map_err(|e| format!("a compressed array: {e}"))?;
        &inflated[..]
    } else {
        data
    };
    if data.len() != bytes {
        return Err(format!(
            "an array of {count} elements inflates to {} bytes",
            data.len()
        ));
    }
    Ok(match kind {
        Kind::Bool => Array::Bool(data.iter().map(|&b| b != 0).collect()),
        Kind::I32 => Array::I32(
            data.as_chunks()
                .0
                .iter()
                .map(|c| i32::from_le_bytes(*c))
                .collect(),
        ),
        Kind::I64 => Array::I64(
            data.as_chunks()
                .0
                .iter()
                .map(|c| i64::from_le_bytes(*c))
                .collect(),
        ),
        Kind::F32 => Array::F32(
            data.as_chunks()
                .0
                .iter()
                .map(|c| f32::from_le_bytes(*c))
                .collect(),
        ),
        Kind::F64 => Array::F64(
            data.as_chunks()
                .0
                .iter()
                .map(|c| f64::from_le_bytes(*c))
                .collect(),
        ),
    })
}

/// Comma-separated numbers: integers while every one is, else floats.
fn numbers(data: &[u8]) -> Piece {
    let text = std::str::from_utf8(data).map_err(|_| "an array holds bytes that aren't text")?;
    let mut tokens = text.split(',').map(str::trim).filter(|t| !t.is_empty());
    let mut ints = Vec::with_capacity(data.len() / 4);
    for token in tokens.by_ref() {
        match token.parse::<i64>() {
            Ok(v) => ints.push(v),
            Err(_) => {
                let mut floats: Vec<f64> = ints.into_iter().map(|v| v as f64).collect();
                floats.push(float(token)?);
                for token in tokens {
                    floats.push(float(token)?);
                }
                return Ok(Array::F64(floats));
            }
        }
    }
    Ok(Array::I64(ints))
}

/// An ASCII number, with the spellings of Windows' C library for the
/// non-finite ones (`1.#INF`, `-1.#IND`).
pub(super) fn float(token: &str) -> Result<f64, String> {
    if let Ok(v) = token.parse::<f64>() {
        return Ok(v);
    }
    let negative = token.starts_with('-');
    match token
        .split_once('#')
        .map(|(_, kind)| kind.to_ascii_uppercase())
    {
        Some(kind) if kind.starts_with("INF") => Ok(if negative {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        }),
        Some(kind) if kind.starts_with("IND") || kind.contains("NAN") => Ok(f64::NAN),
        _ => Err(format!("`{token}` is not a number")),
    }
}

/// An array from its pieces in order, holding `count` elements.
fn join(pieces: impl Iterator<Item = Piece>, count: usize) -> Result<Array, String> {
    let mut pieces: Vec<Array> = pieces.collect::<Result<_, _>>()?;
    let array = if pieces.len() == 1 {
        pieces.pop().unwrap_or_default()
    } else if pieces.iter().all(|p| matches!(p, Array::I64(_))) {
        Array::I64(pieces.into_iter().flat_map(|p| p.into_i64s()).collect())
    } else {
        Array::F64(pieces.into_iter().flat_map(Array::into_f64s).collect())
    };
    if array.len() != count {
        return Err(format!(
            "an array of {count} elements holds {}",
            array.len()
        ));
    }
    Ok(array)
}

impl Array {
    /// An ASCII piece's integers.
    fn into_i64s(self) -> Vec<i64> {
        match self {
            Array::I64(v) => v,
            _ => Vec::new(),
        }
    }
}

/// The node at `path` (child indices from the top level).
fn node_at<'n>(nodes: &'n mut [Node], path: &[u32]) -> &'n mut Node {
    let (first, rest) = path.split_first().expect("an array's node has a path");
    rest.iter().fold(&mut nodes[*first as usize], |node, &i| {
        &mut node.children[i as usize]
    })
}
