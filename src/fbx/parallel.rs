//! Independent tasks run side by side, the one place the reader runs work
//! in parallel (inflating and parsing arrays, reading meshes): on the
//! process's workers, the SDK's, within the host's one budget (on the
//! calling thread alone where no host installed any). Results come back in
//! task order, so they never depend on how many threads ran them.
use kinogaki_studio_api::geometry::parallel;
use std::convert::Infallible;

/// `f(i)` for every `i` in `0..count`, results in order.
#[track_caller]
pub(crate) fn map<T: Send>(count: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    match parallel::map_blocks(count, 1, |range| Ok::<_, Infallible>(f(range.start))) {
        Ok(results) => results,
        Err(never) => match never {},
    }
}
