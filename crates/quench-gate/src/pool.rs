//! Running independent checks on `jobs` threads.

use std::{
    sync::atomic::{AtomicUsize, Ordering},
    thread,
};

/// Applies `check` to every item on `jobs` threads; the failures it reports, one per failing item.
pub fn failures<T: Sync>(jobs: usize, items: &[T], check: impl Fn(&T) -> Option<String> + Sync) -> Vec<String> {
    let next = AtomicUsize::new(0);
    thread::scope(|scope| {
        let workers: Vec<_> = (0..jobs)
            .map(|_| {
                scope.spawn(|| {
                    let mut found = Vec::new();
                    while let Some(item) = items.get(next.fetch_add(1, Ordering::Relaxed)) {
                        found.extend(check(item));
                    }
                    found
                })
            })
            .collect();
        workers.into_iter().flat_map(|worker| worker.join().unwrap_or_default()).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_item_is_checked_once() {
        let items: Vec<u32> = (0..100).collect();
        let mut found = failures(4, &items, |item| (item % 10 == 0).then(|| item.to_string()));
        found.sort();
        let mut expected: Vec<String> = (0..10).map(|tens| (tens * 10).to_string()).collect();
        expected.sort();
        assert_eq!(found, expected);
    }
}
