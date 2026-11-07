//! Why a read failed.
use std::{fmt, io};

/// Why a file could not be read.
#[derive(Debug)]
pub enum Error {
    /// The file could not be read from disk.
    Io(io::Error),
    /// The file breaks the format: where reading stopped, and why.
    Malformed {
        /// A byte offset (binary) or a 1-based line (ASCII).
        at: At,
        /// What was wrong there.
        message: String,
    },
    /// The file is FBX this reader does not read (FBX 6, or newer than 7.x).
    Unsupported(String),
    /// The file asks more than the read's budget allows.
    TooLarge(String),
    /// The read was cancelled.
    Cancelled,
}

/// Where in a file reading stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum At {
    /// A byte offset in a binary file.
    Byte(u64),
    /// A 1-based line of an ASCII file.
    Line(u64),
    /// Not one place: the object graph as a whole.
    Graph,
}

impl Error {
    pub(crate) fn malformed(at: At, message: impl Into<String>) -> Self {
        Error::Malformed {
            at,
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(error) => error.fmt(f),
            Error::Malformed { at, message } => match at {
                At::Byte(offset) => write!(f, "byte {offset}: {message}"),
                At::Line(line) => write!(f, "line {line}: {message}"),
                At::Graph => f.write_str(message),
            },
            Error::Unsupported(message) | Error::TooLarge(message) => f.write_str(message),
            Error::Cancelled => f.write_str("the read was cancelled"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(error) => Some(error),
            _ => None,
        }
    }
}

/// The read's budget refusing: cancelled, or over a limit.
impl From<kinogaki_studio_api::Error> for Error {
    fn from(error: kinogaki_studio_api::Error) -> Self {
        match error.is_cancelled() {
            true => Error::Cancelled,
            false => Error::TooLarge(error.message().to_owned()),
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Error::Io(error)
    }
}
