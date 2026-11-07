//! How to read: budgets for untrusted input, and cancellation.
use crate::fbx::Error;

/// Size limits checked while reading. Every array is sized from its header
/// and checked against these before anything is allocated for it, so they
/// bound the reader's memory whatever a file claims.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Bytes of one decoded array.
    pub array_bytes: usize,
    /// Bytes of all decoded arrays together.
    pub total_bytes: usize,
    /// Nesting depth of the node tree.
    pub depth: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            array_bytes: 4 << 30,
            total_bytes: 32 << 30,
            depth: 64,
        }
    }
}

/// How to read a file.
#[derive(Clone, Copy)]
pub struct Options<'a> {
    /// The budgets.
    pub limits: Limits,
    /// Asked between long steps; once it answers `true`, the read stops
    /// with [`Error::Cancelled`].
    pub cancelled: &'a (dyn Fn() -> bool + Sync),
}

impl Default for Options<'_> {
    fn default() -> Self {
        Self {
            limits: Limits::default(),
            cancelled: &|| false,
        }
    }
}

impl Options<'_> {
    /// `Err(Cancelled)` once the read is cancelled.
    pub(crate) fn check(&self) -> Result<(), Error> {
        if (self.cancelled)() {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
}

impl std::fmt::Debug for Options<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Options")
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}
