use super::*;

pub(super) fn retain_other_atom(entries: &mut [MethodCache], len: usize, atom: Atom) -> usize {
    let mut retained = 0;
    for index in 0..len {
        let entry = entries[index];
        if entry.atom != atom {
            entries[retained] = entry;
            retained += 1;
        }
    }
    entries[retained..].fill(EMPTY_METHOD_CACHE);
    retained
}

pub(super) fn retain_unshadowed_entries(
    heap: &crate::heap::Heap,
    entries: &mut [MethodCache],
    len: usize,
    object: Value,
    atom: Atom,
) -> usize {
    let mut retained = 0;
    for index in 0..len {
        let entry = entries[index];
        if !method_cache_shadowed_by(heap, entry, object, atom) {
            entries[retained] = entry;
            retained += 1;
        }
    }
    entries[retained..].fill(EMPTY_METHOD_CACHE);
    retained
}

pub(super) fn method_cache_shadowed_by(
    heap: &crate::heap::Heap,
    entry: MethodCache,
    object: Value,
    atom: Atom,
) -> bool {
    if entry.atom != atom || entry.guard.depth <= 1 {
        return false;
    }
    let mut prototype = entry.proto;
    for _ in 1..entry.guard.depth {
        if prototype == object {
            return true;
        }
        let Some(data) = heap.object_record(prototype) else {
            return false;
        };
        prototype = data.proto();
    }
    false
}

pub(super) fn retain_entries(heap: &crate::heap::Heap, entries: &mut [MethodCache], len: usize) -> usize {
    let mut retained = 0;
    for index in 0..len {
        let entry = entries[index];
        if method_cache_live(heap, entry) {
            entries[retained] = entry;
            retained += 1;
        }
    }
    entries[retained..].fill(EMPTY_METHOD_CACHE);
    retained
}

pub(super) fn method_cache_live(heap: &crate::heap::Heap, entry: MethodCache) -> bool {
    let key_live = !entry.proto.is_heap() || heap.is_live(entry.proto);
    let owner_live = !entry.guard.owner.is_heap() || heap.is_live(entry.guard.owner);
    let callee_live = !entry.callee.is_heap() || heap.is_live(entry.callee);
    let env = match entry.target {
        Some(CallTarget::User(_, _, env)) => Some(env),
        _ => None,
    };
    key_live
        && owner_live
        && callee_live
        && env.is_none_or(|value| !value.is_heap() || heap.is_live(value))
}
