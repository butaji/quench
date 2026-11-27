//! How `build.rs` shards the stencil compilation: the split of the generated row instances into
//! independent rustc invocations and the merge of their stencil tables. Pure data operations, so
//! the unit tests (compiled into `quench-bjit`'s test build) can check them without rustc.

/// The compile weight one shard holds at most, in row units. rustc holds every monomorphized
/// row of its crate in memory at once, and a row's cost is its kind's (`NUMBER_WEIGHT` and after): a shard of
/// 4096 number rows peaked near 4.6 GB, 219 typed element rows near 4.3 GB, 256 property, site and IC rows
/// near 3.1 GB, and 626 such rows (weighed at 2 each) with 88 element rows near 9.6 GB, as did 360
/// supernodes of an index row (weighed at 2). At this budget a shard stays near 2 to 2.5 GB beside the fixed
/// cost of the shared row definitions, so `MAX_WORKERS` of them fit together.
pub const SHARD_WEIGHT: usize = 1536;

/// The weights of a row's kind, in units of a number row's cost: a heap row inlines its staged
/// step (7 to 11 units measured for the property, site, IC and environment rows; the build weighs a
/// supernode as its rows together), and an element row (an index read or store, plain or typed) the whole
/// element family's.
pub const NUMBER_WEIGHT: usize = 1;
pub const HEAP_WEIGHT: usize = 8;
pub const ELEMENT_WEIGHT: usize = 14;
/// A plain index read or store row (`INDEX_ROW`, `STORE_ROW`): 476 store rows took 14 s and peaked at 613 MB in
/// one shard (Nov 24), about 1.3 number rows each, so it weighs 2 units (it weighed 44 when it inlined every
/// backing's element read and write, and its 34-row shards each paid rustc's fixed 2.5 s).
pub const INDEX_WEIGHT: usize = 2;

/// A direct call row (`direct_row!`): it inlines the whole staged call path, 0.74 GB for four of them against 0.30 GB
/// for none (Nov 27), ~110 MB a row, so 16 of them fill a shard. A reload row weighs a number row.
pub const DIRECT_CALL_WEIGHT: usize = 96;

/// The weight of one call row invocation (`build/call_rows.rs`).
pub fn call_weight(invocation: &str) -> usize {
    if invocation.starts_with("direct_row!") { DIRECT_CALL_WEIGHT } else { NUMBER_WEIGHT }
}

/// Most shard compilations running at once: four shards of `SHARD_WEIGHT` (near 10 GB together) fit a
/// 16 GB, 4-core machine beside the linker and the other crates cargo builds in parallel. LLVM is about 90%
/// of a shard's time (`-Z time-passes`), so the shards' codegen is what runs in parallel.
pub const MAX_WORKERS: usize = 4;

/// `items` in contiguous slices, each as long as its weights (`weight`) fit `SHARD_WEIGHT`, and
/// at least one item a slice; at least one slice, so the fixed rows always have a home.
pub fn split<T>(items: &[T], weight: impl Fn(&T) -> usize) -> Vec<&[T]> {
    let mut parts = Vec::new();
    let (mut start, mut held) = (0, 0);
    for (at, item) in items.iter().enumerate() {
        let cost = weight(item);
        if at > start && held + cost > SHARD_WEIGHT {
            parts.push(&items[start..at]);
            (start, held) = (at, 0);
        }
        held += cost;
    }
    if start < items.len() || parts.is_empty() {
        parts.push(&items[start..]);
    }
    parts
}

/// The rustc processes to run at once over `shards` shards on `parallelism` cores.
pub fn workers(shards: usize, parallelism: usize) -> usize {
    shards.min(parallelism).min(MAX_WORKERS).max(1)
}

/// The rows (name, table entry) of every shard as one list sorted by name; a name two shards
/// both define is a build error.
pub fn merge<T: Ord>(shards: impl IntoIterator<Item = Vec<(String, T)>>) -> Vec<(String, T)> {
    let mut rows: Vec<(String, T)> = shards.into_iter().flatten().collect();
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
        assert_eq!(split::<u8>(&[], |_| NUMBER_WEIGHT).len(), 1);
        assert_eq!(split(&[0u8; SHARD_WEIGHT], |_| NUMBER_WEIGHT).len(), 1);
        assert_eq!(split(&[0u8; SHARD_WEIGHT + 1], |_| NUMBER_WEIGHT).len(), 2);
    }

    #[test]
    fn split_is_contiguous_within_weight_and_complete() {
        let items: Vec<usize> = (0..SHARD_WEIGHT * 3 + 5).collect();
        let weight = |item: &usize| if item % 7 == 0 { ELEMENT_WEIGHT } else { NUMBER_WEIGHT };
        let parts = split(&items, weight);
        assert_eq!(parts.concat(), items);
        assert!(parts.iter().all(|part| part.iter().map(weight).sum::<usize>() <= SHARD_WEIGHT));
    }

    #[test]
    fn workers_are_bounded() {
        assert_eq!(workers(2, 64), 2);
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
