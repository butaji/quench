//! Prototype walks in the comparison jumps: the unrolled and looped walks agree, and `instanceof` of
//! a constructor whose `prototype` is a Stable fact is decided by the words of the value's chain.
use super::*;

/// Objects of a heap: `leaf` inherits `middle`, which inherits `root`, which has no prototype, and
/// `other` stands alone; and the base of the object record table the walk addresses.
struct World {
    heap: crate::heap::Heap,
    objects: [Value; 4],
}

impl World {
    fn new() -> Self {
        use crate::heap::{Heap, Object};
        let mut heap = Heap::new();
        let root = heap.alloc_object(Object::new(Value::NULL));
        let middle = heap.alloc_object(Object::new(root));
        let leaf = heap.alloc_object(Object::new(middle));
        let other = heap.alloc_object(Object::new(Value::NULL));
        Self { heap, objects: [leaf, middle, root, other] }
    }

    fn records(&self) -> u64 { self.heap.object_record_base().expect("object records are reserved") as u64 }

    /// The address of the record of `object`.
    fn record(&self, object: Value) -> u64 { self.heap.object_record(object).expect("an object record") as *const crate::heap::Object as u64 }

    fn link(prototype: Value) -> u64 { u64::from(crate::heap::ProtoLink::of(prototype).bits()) }
}

/// A prototype walk finds the target on the chain of an object record, or the end of the chain,
/// and the unrolled walk a specializer emits (`Stage::walk_prototypes`'s default) answers what the
/// loop of the interpreter and the stencils does, for every start, target and bound.
#[test]
fn the_unrolled_and_looped_prototype_walks_agree() {
    let world = World::new();
    let targets = [World::link(Value::NULL), World::link(world.objects[0]), World::link(world.objects[1]),
        World::link(world.objects[2]), World::link(world.objects[3])];
    for start in world.objects {
        for target in targets {
            for links in 0..6 {
                let mut stage = Concrete::over(&world.heap);
                let looped = stage.walk_prototypes(world.records(), world.record(start), target, links);
                let unrolled = crate::staged::object::walk_prototypes(&mut stage, world.records(), world.record(start), target, links);
                assert_eq!(looped, unrolled, "{start:?} {target} {links}");
            }
        }
    }
    let mut stage = Concrete::over(&world.heap);
    let [leaf, middle, root, other] = world.objects;
    let walk = |stage: &mut Concrete, start, prototype, links| stage.walk_prototypes(world.records(), world.record(start), World::link(prototype), links);
    assert_eq!(walk(&mut stage, leaf, middle, 0), (true, true), "the first link needs no step");
    assert_eq!(walk(&mut stage, leaf, root, 1), (true, true));
    assert_eq!(walk(&mut stage, leaf, root, 0), (false, false), "one link too few leaves it undecided");
    assert_eq!(walk(&mut stage, leaf, other, 3), (true, false), "the chain ends at null without it");
    assert_eq!(walk(&mut stage, root, leaf, 0), (true, false), "a root's chain is null");
}

/// The null link the walk ends at is the heap's own.
#[test]
fn the_walk_ends_at_the_heaps_null_link() {
    assert_eq!(crate::heap::NATIVE_OBJECT_LAYOUT.proto_mask, crate::heap::ProtoLink::NULL.bits());
}

/// With the constructor's prototype a Stable fact, `instanceof` of a direct object is decided by
/// the words of its chain and a primitive is no instance, and the VM is asked only for a cell
/// the walk does not follow, or where the frame holds no fact.
#[test]
fn an_instanceof_with_a_stable_prototype_walks_the_chain_by_words() {
    let world = World::new();
    let [leaf, middle, root, other] = world.objects;
    let cell = Value::from_bits(crate::value::ValueLayout::HEAP_TAG | 1);
    for (value, prototype, holds) in [(leaf, middle, true), (leaf, root, true), (middle, middle, false), (other, root, false),
        (leaf, other, false), (Value::integer(1), root, false), (Value::UNDEFINED, root, false), (Value::TRUE, root, false)] {
        let mut frame = Probe::of(&[value, Value::NULL]);
        frame.records = Some(world.records());
        frame.prototype = Some(World::link(prototype));
        let step = run(&mut frame, Comparison::Instanceof, true);
        assert!(matches!(step, Step::Control(edge) if edge == if holds { Edge::Following } else { Edge::Target }), "{value:?} {prototype:?}");
        assert!(frame.calls.is_empty(), "{value:?} is decided by its words");
    }
    for answer in [true, false] {
        let mut frame = Probe::of(&[cell, Value::NULL]);
        frame.records = Some(world.records());
        frame.prototype = Some(World::link(root));
        frame.answer = Some(answer);
        let step = run(&mut frame, Comparison::Instanceof, true);
        assert!(matches!(step, Step::Control(edge) if edge == if answer { Edge::Following } else { Edge::Target }));
        assert_eq!(frame.calls, [CallOut::Decide { left: 0, right: 1, operator: BinaryOperator::Instanceof as u32 }],
            "a cell the walk does not follow is the VM's");
    }
    let mut frame = Probe::of(&[leaf, Value::NULL]);
    frame.records = Some(world.records());
    frame.answer = Some(false);
    run(&mut frame, Comparison::Instanceof, true);
    assert_eq!(frame.calls.len(), 1, "no Stable fact is the VM's decision");
}
