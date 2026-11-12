//! How `build.rs` shards the stencil compilation: the split of the generated row instances into
//! independent rustc invocations and the merge of their stencil tables. Pure data operations, so
//! the unit tests (compiled into `quench-bjit`'s test build) can check them without rustc.

/// Target instances per shard. rustc holds every monomorphized row of its crate in memory at
/// once: one invocation over the ~26k rows peaked near 6 GB (about 0.23 MB per row), so 4096 rows
/// keep a shard near 1 GB plus the fixed cost of the shared row definitions.
pub const ROWS_PER_SHARD: usize = 4096;

/// Most shard compilations running at once: eight shards of about 1 GB fit a 16 GB machine
/// beside the linker and the other crates cargo builds in parallel.
pub const MAX_WORKERS: usize = 8;

/// The shard count for `rows` instances: at least one, so the fixed rows always have a home.
pub fn shard_count(rows: usize) -> usize {
    rows.div_ceil(ROWS_PER_SHARD).max(1)
}

/// `items` in `shard_count` contiguous slices whose lengths differ by at most one.
pub fn split<T>(items: &[T]) -> Vec<&[T]> {
    let count = shard_count(items.len());
    let (base, extra) = (items.len() / count, items.len() % count);
    let mut start = 0;
    (0..count)
        .map(|shard| {
            let end = start + base + usize::from(shard < extra);
            let part = &items[start..end];
            start = end;
            part
        })
        .collect()
}

/// Concurrent compilations for `shards` shards on a machine with `parallelism` threads.
pub fn workers(shards: usize, parallelism: usize) -> usize {
    shards.min(parallelism).min(MAX_WORKERS).max(1)
}

/// The rows (name, table entry) of every shard as one list sorted by name; a name two shards
/// both define is a build error.
pub fn merge(shards: impl IntoIterator<Item = Vec<(String, String)>>) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = shards.into_iter().flatten().collect();
    rows.sort();
    if let Some(pair) = rows.windows(2).find(|pair| pair[0].0 == pair[1].0) {
        panic!("stencil {} is defined by two shards", pair[0].0);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_lists_make_one_shard() {
        assert_eq!(shard_count(0), 1);
        assert_eq!(split::<u8>(&[]).len(), 1);
        assert_eq!(shard_count(ROWS_PER_SHARD), 1);
        assert_eq!(shard_count(ROWS_PER_SHARD + 1), 2);
    }

    #[test]
    fn split_is_contiguous_balanced_and_complete() {
        let items: Vec<usize> = (0..ROWS_PER_SHARD * 3 + 5).collect();
        let parts = split(&items);
        assert_eq!(parts.len(), 4);
        assert_eq!(parts.concat(), items);
        let (min, max) = (parts.iter().map(|p| p.len()).min().unwrap(), parts.iter().map(|p| p.len()).max().unwrap());
        assert!(max - min <= 1 && max <= ROWS_PER_SHARD);
    }

    #[test]
    fn workers_are_bounded() {
        assert_eq!(workers(3, 64), 3);
        assert_eq!(workers(100, 64), MAX_WORKERS);
        assert_eq!(workers(100, 2), 2);
        assert_eq!(workers(1, 0), 1);
    }

    #[test]
    fn merge_sorts_across_shards() {
        let row = |name: &str| (name.to_string(), String::new());
        let merged = merge([vec![row("b"), row("d")], vec![row("a"), row("c")]]);
        assert_eq!(merged.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["a", "b", "c", "d"]);
    }

    #[test]
    #[should_panic(expected = "defined by two shards")]
    fn merge_rejects_duplicate_names() {
        let row = |name: &str| (name.to_string(), String::new());
        merge([vec![row("a")], vec![row("a")]]);
    }
}
