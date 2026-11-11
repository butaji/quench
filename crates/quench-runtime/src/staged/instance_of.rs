//! `instanceof`'s prototype-chain walk as a staged read: from an object record, up to `depth`
//! links, each compared with the encoded link of the constructor's `prototype` object. The
//! interpreter walks the same links (`Vm::steady_instanceof`); a specializer unrolls them,
//! one load and one compare per link, with the target link a per-site operand.

use super::{object, value, Stage};
use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;

/// 1 where the chain of the object record at `record` holds the link `target` within `depth`
/// links, 0 where it ends (null) first. A link that names no object record (an array, a
/// function, a proxy) or a chain longer than `depth` leaves through `miss`: the caller's
/// full walk decides it.
#[inline(always)]
pub(crate) fn instance_of<S: Stage>(stage: &mut S, records: S::Word, record: S::Word, target: u64, depth: usize,
    miss: S::Miss) -> Option<S::Word> {
    let target = stage.site(target);
    walk(stage, records, record, target, depth, miss)
}

/// What an `instanceof` site recorded: the constructor it saw (a function value's bits), the
/// encoded link of the `prototype` object that constructor named, how many links the chains it
/// saw were deep, and the watch that retires the record when that constructor's `prototype`,
/// its shape, or `Function.prototype`'s `@@hasInstance` change.
#[derive(Clone, Copy, Debug)]
pub(crate) struct InstanceOfSite {
    pub(crate) constructor: u64,
    pub(crate) target: u32,
    pub(crate) depth: usize,
    pub(crate) watch: u64,
}

/// `value instanceof constructor` at a site that saw `site`: 1 or 0 as the chain walk decides,
/// `None` (leaving at `miss`) for another constructor, a retired watch, a value that is no
/// direct object record, or a chain the walk cannot decide. `records` is the object record table.
#[allow(dead_code)] // consumer: the lane instanceof lowering (thread 7)
#[inline(always)]
pub(crate) fn instance_of_site<S: Stage>(stage: &mut S, records: S::Word, value_bits: S::Word, constructor: S::Word,
    site: InstanceOfSite, miss: S::Miss) -> Option<S::Word> {
    let seen = stage.site(site.constructor);
    let same = stage.eq_imm(constructor, seen);
    stage.guard(same, miss)?;
    let watch = stage.site(site.watch);
    let holds = stage.assumption(watch);
    stage.guard(holds, miss)?;
    let direct = value::is_direct_object(stage, value_bits);
    stage.guard(direct, miss)?;
    // The record id is the word's low half (`Value::direct_object_index`).
    let id = stage.and_imm(value_bits, u64::from(u32::MAX));
    let record = object::object_record_at(stage, records, id);
    instance_of(stage, records, record, u64::from(site.target), site.depth, miss)
}

#[inline(always)]
fn walk<S: Stage>(stage: &mut S, records: S::Word, record: S::Word, target: S::Imm, depth: usize, miss: S::Miss) -> Option<S::Word> {
    let link = object::proto_link(stage, record);
    let hit = stage.eq_imm(link, target);
    stage.choose_leaving(hit, |stage| Some(stage.constant(1)), |stage| {
        let null = stage.eq_imm(link, u64::from(crate::heap::ProtoLink::NULL.bits()));
        stage.choose_leaving(null, |stage| Some(stage.constant(0)), |stage| {
            if depth == 0 {
                // The chain is longer than the site saw: no answer here.
                let never = stage.eq_imm(link, link_never());
                stage.guard(never, miss)?;
                return Some(stage.constant(0));
            }
            let id = object::proto_object_record_id(stage, link, miss)?;
            let next = object::object_record_at(stage, records, id);
            walk(stage, records, next, target, depth - 1, miss)
        })
    })
}

/// A link no record holds: the guard that leaves a chain past its depth compares with it.
#[inline(always)]
const fn link_never() -> u64 { u64::MAX }

/// The interpreter's reading of `instance_of` over the record table at `records`.
///
/// # Safety
/// `records` addresses the object record table and `record` a live record in it.
#[allow(dead_code)] // consumer: the lane instanceof lowering (thread 7)
#[inline(always)]
pub(crate) unsafe fn concrete_instance_of(records: *const u8, record: *const u8, target: u32, depth: usize) -> Option<bool> {
    let mut stage = super::Concrete::new(std::ptr::null());
    instance_of(&mut stage, records as u64, record as u64, u64::from(target), depth, ()).map(|word| word != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::ProtoLink;

    /// Records laid out `record_size` apart, each holding its prototype link at the meta word.
    fn table(links: &[u32]) -> Vec<u8> {
        let mut bytes = vec![0u8; links.len() * OBJECT.record_size];
        for (index, link) in links.iter().enumerate() {
            let at = index * OBJECT.record_size + OBJECT.meta_offset;
            bytes[at..at + 4].copy_from_slice(&link.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn a_walk_finds_the_target_link_or_the_end_of_the_chain() {
        // Record 3 -> 2 -> 1 -> null; record 0 is unrelated and has no prototype.
        let object = |id| ProtoLink::object_record(id).bits();
        let records = table(&[ProtoLink::NULL.bits(), ProtoLink::NULL.bits(), object(1), object(2)]);
        let base = records.as_ptr();
        // SAFETY: `records` holds four records laid out as the object table.
        let from = |id: usize, target: u32, depth| unsafe { concrete_instance_of(base, base.add(id * OBJECT.record_size), target, depth) };
        assert_eq!(from(3, object(2), 3), Some(true), "the parent");
        assert_eq!(from(3, object(1), 3), Some(true), "the grandparent");
        assert_eq!(from(0, object(1), 3), Some(false), "a chain that ends first");
        assert_eq!(from(3, object(0), 3), Some(false), "a record the chain never names");
        assert_eq!(from(3, object(0), 1), None, "past the site's depth leaves");
    }

    #[test]
    fn the_target_is_a_hole_not_part_of_the_trace() {
        let trace = |target: u32| {
            let mut stage = crate::staged::trace::Trace::default();
            let (records, record) = (stage.constant(0), stage.constant(0));
            instance_of(&mut stage, records, record, u64::from(target), 2, ()).expect("a trace never misses");
            stage
        };
        let (first, second) = (trace(11), trace(23));
        assert_eq!(first.ops, second.ops);
        assert_eq!((first.holes, second.holes), (vec![11], vec![23]));
    }

    #[test]
    fn a_site_row_decides_a_real_chain_under_its_constructor_and_watch() {
        use crate::heap::{Cell, Heap, Object};
        use crate::code_dependencies::PublishedCode;
        use crate::Value;
        use std::rc::Rc;
        let mut heap = Heap::new();
        // grandparent <- parent <- child; `other` has no relation.
        let grandparent = heap.alloc(Cell::Object(Object::new(Value::NULL)));
        let parent = heap.alloc(Cell::Object(Object::new(grandparent)));
        let child = heap.alloc(Cell::Object(Object::new(parent)));
        let other = heap.alloc(Cell::Object(Object::new(Value::NULL)));
        let watch = Rc::new(PublishedCode::new(()));
        let constructor = Value::integer(77);
        let records = heap.object_record_base().expect("a heap with object records") as *const u8;
        let site = |target: Value, depth| InstanceOfSite { constructor: constructor.to_bits(),
            target: ProtoLink::of(target).bits(), depth, watch: Rc::as_ptr(&watch) as u64 };
        let decide = |value: Value, constructor: Value, site: InstanceOfSite| {
            let mut stage = crate::staged::Concrete::new(std::ptr::null());
            instance_of_site(&mut stage, records as u64, value.to_bits(), constructor.to_bits(), site, ()).map(|word| word != 0)
        };
        let first = decide(child, constructor, site(grandparent, 3));
        assert_eq!(first, Some(true));
        assert_eq!(decide(other, constructor, site(grandparent, 3)), Some(false));
        assert_eq!(decide(child, Value::integer(78), site(grandparent, 3)), None, "another constructor");
        assert_eq!(decide(Value::integer(5), constructor, site(grandparent, 3)), None, "a primitive leaves");
        assert_eq!(decide(child, constructor, site(other, 1)), None, "a chain deeper than the site saw leaves");
        watch.retire();
        assert_eq!(decide(child, constructor, site(grandparent, 3)), None, "a retired watch");
    }
}
