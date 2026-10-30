//! Bounded independent work, with results consumed in input order.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// Prepares independent items on scoped workers. A missing result means the coordinator
/// must retry that item serially: a worker failed, panicked, or could not be started.
/// Preparations must leave shared caller state unchanged.
pub fn prepare_ordered<T: Sync, R: Send, E, F>(
    items: &[T],
    workers: usize,
    prepare: F,
) -> Vec<Option<R>>
where
    F: Fn(&T) -> Result<R, E> + Sync,
{
    let next = AtomicUsize::new(0);
    let results: Vec<Mutex<Option<R>>> = items.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for _ in 0..workers.min(items.len()) {
            let work = || {
                while let Some((item, slot)) = {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    items.get(index).zip(results.get(index))
                } {
                    let result =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| prepare(item)))
                            .ok()
                            .and_then(Result::ok);
                    if let Ok(mut slot) = slot.lock() {
                        *slot = result;
                    }
                }
            };
            if let Ok(handle) = std::thread::Builder::new().spawn_scoped(scope, work) {
                handles.push(handle);
            }
        }
        for handle in handles {
            let _ = handle.join();
        }
    });
    results
        .into_iter()
        .map(|slot| slot.into_inner().ok().flatten())
        .collect()
}

/// Limits workers by CPU availability, item count, and current free memory. The estimate
/// covers one worker's temporary allocations, including child processes. Unknown memory
/// keeps the CPU limit; even a budget smaller than one item allows one serial worker.
pub fn workers(requested: usize, parts: usize, bytes_per_part: u64) -> usize {
    let cpus = std::thread::available_parallelism().map_or(1, usize::from);
    worker_limit(
        requested,
        parts,
        bytes_per_part,
        cpus,
        crate::available_memory(),
    )
}

fn worker_limit(
    requested: usize,
    parts: usize,
    bytes_per_part: u64,
    cpus: usize,
    available: Option<u64>,
) -> usize {
    let requested = if requested == 0 { cpus } else { requested };
    let memory = available
        .map(|bytes| usize::try_from(bytes / bytes_per_part.max(1)).unwrap_or(usize::MAX))
        .unwrap_or(requested);
    requested.min(parts).min(memory).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_leave_only_their_own_slot_for_serial_retry() {
        let items = [0, 1, 2, 3, 4, 5];
        let results = prepare_ordered(&items, 4, |&item| {
            if item == 1 {
                panic!("forced preparation panic");
            }
            if item == 3 {
                return Err("forced preparation error");
            }
            Ok(item * 2)
        });
        assert_eq!(results, [Some(0), None, Some(4), None, Some(8), Some(10)]);
    }

    #[test]
    fn serial_limits_allow_at_least_one_worker() {
        assert_eq!(worker_limit(1, 100, 1, 8, Some(100)), 1);
        assert_eq!(worker_limit(0, 100, 1, 8, Some(100)), 8);
        assert_eq!(worker_limit(4, 100, 1, 8, None), 4);
        assert_eq!(worker_limit(4, 100, 16, 8, Some(32)), 2);
        assert_eq!(worker_limit(4, 0, 1, 8, Some(100)), 1);
        assert_eq!(worker_limit(4, 3, u64::MAX, 8, Some(100)), 1);
        assert_eq!(worker_limit(0, 2, 1, 8, None), 2);
    }
}
