//! How `build.rs` shards the stencil compilation: the split of the generated row instances into
//! independent rustc invocations and the merge of their stencil tables. Pure data operations, so
//! the unit tests (compiled into `quench-bjit`'s test build) can check them without rustc.

/// Target instances per shard. rustc holds every monomorphized row of its crate in memory at
/// once: one invocation over the ~26k rows peaked near 6 GB (about 0.23 MB per row), so 4096 rows
/// kept a shard near 1 GB plus the fixed cost of the shared row definitions. The staged rows grew that cost:
/// one 4096-row shard reached 14 GB on 2026-11-19, so a shard holds 1536.
pub const ROWS_PER_SHARD: usize = 1024;

/// Most shard compilations running at once: eight shards of about 1 GB fit a 16 GB machine
/// beside the linker and the other crates cargo builds in parallel.
pub const MAX_WORKERS: usize = 8;

/// `items` in contiguous slices of at most `ROWS_PER_SHARD` weight each (an item heavier than that alone), at least
/// one slice. A heap row inlines a staged heap operation, several times a Number row's code and its compile memory,
/// so the build weighs each row (`HEAP_ROW_WEIGHT`) and no shard's rustc outgrows the box.
pub fn split_weighted<T>(items: &[T], weight: impl Fn(&T) -> usize) -> Vec<&[T]> {
    let mut parts = Vec::new();
    let (mut start, mut load) = (0, 0);
    for (index, item) in items.iter().enumerate() {
        let w = weight(item);
        if load + w > ROWS_PER_SHARD && index > start {
            parts.push(&items[start..index]);
            (start, load) = (index, 0);
        }
        load += w;
    }
    parts.push(&items[start..]);
    parts
}

/// The weight of one heap row in `split_weighted`, in Number rows: a 1490-row shard of mostly heap rows reached 8.5 GB
/// where a Number shard of the same length stays near 2 GB.
pub const HEAP_ROW_WEIGHT: usize = 8;

/// Concurrent compilations for `shards` shards on a machine with `parallelism` threads.
pub fn workers(shards: usize, parallelism: usize) -> usize {
    shards.min(parallelism).min(MAX_WORKERS).max(1)
}

/// The rows (name, table entry) of every shard as one list sorted by name; a name two shards
/// both define is a build error.
pub fn merge(shards: impl IntoIterator<Item = Vec<(String, String)>>) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = shards.into_iter().flatten().collect();
    rows.sort();
    if let Some((row, _)) = rows.iter().zip(rows.iter().skip(1)).find(|(row, next)| row.0 == next.0) {
        panic!("stencil {} is defined by two shards", row.0);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_lists_make_one_shard() {
        assert_eq!(split_weighted::<u8>(&[], |_| 1).len(), 1);
        assert_eq!(split_weighted(&[0u8; ROWS_PER_SHARD], |_| 1).len(), 1);
        assert_eq!(split_weighted(&[0u8; ROWS_PER_SHARD + 1], |_| 1).len(), 2);
    }

    #[test]
    fn weighted_split_is_contiguous_bounded_and_complete() {
        let items: Vec<usize> = (0..ROWS_PER_SHARD * 3 + 5).collect();
        let weight = |item: &usize| if item % 3 == 0 { HEAP_ROW_WEIGHT } else { 1 };
        let parts = split_weighted(&items, weight);
        assert_eq!(parts.concat(), items);
        assert!(parts.iter().all(|part| part.iter().map(weight).sum::<usize>() <= ROWS_PER_SHARD));
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
