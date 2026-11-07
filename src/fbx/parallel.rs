//! Independent tasks run side by side: the one place the reader runs work
//! in parallel (inflating and parsing arrays).
//!
//! Until the plugin joins the SDK this splits over scoped threads, up to the
//! available parallelism. In Studio a package never starts threads: this
//! body becomes the SDK's `geometry::parallel::map_blocks`, so reading runs
//! within the host's one budget. Results come back in task order, so they
//! never depend on how many threads ran them.
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// `f(i)` for every `i` in `0..count`, results in order.
pub(crate) fn map<T: Send>(count: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    if threads <= 1 || count <= 1 {
        return (0..count).map(f).collect();
    }
    let results: Vec<Mutex<Option<T>>> = (0..count).map(|_| Mutex::new(None)).collect();
    let next = AtomicUsize::new(0);
    let work = || {
        loop {
            let at = next.fetch_add(1, Ordering::Relaxed);
            if at >= count {
                break;
            }
            let result = f(at);
            *results[at].lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
        }
    };
    std::thread::scope(|scope| {
        for _ in 1..threads.min(count) {
            scope.spawn(work);
        }
        work();
    });
    results
        .into_iter()
        .map(|slot| {
            slot.into_inner()
                .unwrap_or_else(|e| e.into_inner())
                .expect("every task ran")
        })
        .collect()
}
