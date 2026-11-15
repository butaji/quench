use super::*;
use crate::vm::wtf16::{JsString, Parts};

#[test]
#[cfg(not(feature = "profile-aggregate"))]
fn memory_census_accounts_for_direct_and_legacy_vectors_across_collection() {
    let mut heap = Heap::new();
    let before = heap.memory_composition();
    let direct = heap.alloc_v128([0x5a; crate::wasm::V128_BYTES]);
    assert!(direct.direct_v128_index().is_some());
    let legacy = heap.alloc_in_slot(Cell::WasmV128([0x66; crate::wasm::V128_BYTES]), false);
    assert!(legacy.direct_v128_index().is_none());
    let census = heap.memory_composition();
    assert_eq!(census.cell_counts[CellKind::WasmV128 as usize], 2);
    assert_eq!(census.direct_records, before.direct_records + 1);
    assert_eq!(census.occupied_slots, before.occupied_slots + 2);
    assert_eq!(census.v128_record_used_bytes, crate::wasm::V128_BYTES);
    assert!(census.v128_record_reserved_bytes >= census.v128_record_used_bytes);
    assert_eq!(census.used_bytes() - before.used_bytes(), crate::wasm::V128_BYTES + census.slot_size_bytes);
    assert_eq!(census.cell_counts.iter().sum::<usize>(), census.occupied_slots);
    let variant = census.cell_variant_names.iter().position(|name| *name == "wasm_v128").unwrap();
    assert_eq!(census.cell_variant_counts[variant], 2);
    heap.collect([direct]);
    let rooted = heap.memory_composition();
    assert_eq!(rooted.cell_counts[CellKind::WasmV128 as usize], 1);
    assert_eq!(rooted.v128_record_used_bytes, crate::wasm::V128_BYTES);
    assert_eq!(heap.v128(direct), Some([0x5a; crate::wasm::V128_BYTES]));
    heap.collect([]);
    let empty = heap.memory_composition();
    assert_eq!(empty.cell_counts[CellKind::WasmV128 as usize], 0);
    assert_eq!(empty.v128_record_used_bytes, 0);
}

#[test]
fn slot_type_admission_reads_live_storage_and_rebuilds_after_remapping() {
    use crate::typed_ir::Type;
    let mut heap = Heap::new();
    let layouts = [(0, None), (1, None), (2, Some((1, 1)))];
    heap.register_shape_layouts(&layouts);
    let link = heap.alloc_object(plain_object());
    let packed = heap.alloc_object_with_properties(Value::NULL, 1, &[Value::NULL]);
    let arena = heap.alloc_object_with_properties(Value::NULL, 2, &[link, Value::number(0.5)]);
    let extras = heap.alloc_object_with_properties(Value::NULL, 1, &[Value::UNDEFINED]);
    heap.object_record_mut(extras).unwrap().set_extensible(false);
    let deleted = heap.alloc_object_with_properties(Value::NULL, 1, &[Value::DELETED]);
    assert!(heap.object(packed).unwrap().inline_properties().is_some());
    assert!(heap.object(arena).unwrap().inline_properties().is_none());
    heap.track_slot_representations(&layouts);
    let nullable = Type::DIRECT_OBJECT_FAMILY.join(Type::NULLISH);
    assert_eq!(heap.slot_type(1, 0), nullable);
    assert_eq!(heap.slot_type(2, 0), nullable);
    assert_eq!(heap.slot_type(2, 1), Type::NUMBER);
    assert_eq!(heap.unrepresented_slots(), 0);

    // A fresh admission reflects live contents, not stale observation history.
    for object in [packed, arena, extras, deleted] { heap.property_set(object, 0, link); }
    heap.track_slot_representations(&layouts);
    assert_eq!(heap.slot_type(1, 0), Type::DIRECT_OBJECT_FAMILY);
    // Reassign shape identities and re-establish the parent-owned slot projection.
    let remapped = [(0, None), (0, None), (1, None), (2, Some((2, 1)))];
    heap.remap_live_object_shapes(&[0, 2, 3], &remapped);
    assert_eq!(heap.slot_type(2, 0), Type::DIRECT_OBJECT_FAMILY);
    assert_eq!(heap.slot_type(3, 0), Type::DIRECT_OBJECT_FAMILY);
    assert_eq!(heap.slot_type(3, 1), Type::NUMBER);
    assert_eq!(heap.unrepresented_slots(), 0);
}

#[test]
fn raw_slot_admission_records_write_domain_before_exposing_storage() {
    use crate::typed_ir::Type;
    let mut heap = Heap::new();
    let layouts = [(0, None), (1, None), (3, None)];
    heap.register_shape_layouts(&layouts);
    let packed = heap.alloc_object_with_properties(Value::NULL, 1, &[Value::integer(1)]);
    let object = heap.alloc_object_with_properties(Value::NULL, 2,
        &[Value::NULL, Value::integer(1), Value::integer(2)]);
    let child = heap.alloc_object(plain_object());
    heap.track_slot_representations(&layouts);
    // Declined storage does not admit a write capability.
    assert!(unsafe { heap.flat_property_mut_ptr_unchecked(packed, 0, Type::ANY) }.is_none());
    assert_eq!(heap.slot_type(1, 0), Type::INT32);
    let pointer = unsafe { heap.flat_property_mut_ptr_unchecked(object, 0, Type::ANY) }.unwrap();
    assert_eq!(heap.slot_type(2, 0), Type::ANY);
    // A reference-capable region logs its owner before its no-GC raw writes.
    heap.object_store_mut(object, object).unwrap();
    unsafe { pointer.write(child); }
    assert_eq!(heap.unrepresented_slots(), 0);
}

#[test]
fn object_publication_admits_preinitialized_slots_after_census() {
    use crate::typed_ir::Type;
    let publishers: [(&str, fn(&mut Heap, Object) -> Value); 2] = [
        ("object", Heap::alloc_object),
        ("cell", |heap, object| heap.alloc(Cell::Object(object))),
    ];
    for (publisher, publish) in publishers {
        for storage in ["inline", "arena", "extras"] {
            let mut heap = Heap::new();
            heap.track_slot_representations(&[(0, None), (1, None)]);
            assert_eq!(heap.slot_type(1, 0), Type::NEVER);
            let value = if storage == "arena" { Value::number(0.5) } else { Value::NULL };
            let (properties, inline) = heap.initial_object_storage(1, &[value], storage != "arena");
            let mut object = Object::with_property_storage(Value::NULL, properties, inline);
            if storage == "extras" { object.set_extensible(false); }
            let published = publish(&mut heap, object);
            assert!(published.direct_object_index().is_some());
            assert_eq!(heap.slot_type(1, 0), Type::immediate(value), "{publisher}/{storage}");
            assert_eq!(heap.unrepresented_slots(), 0, "{publisher}/{storage}");
            assert_eq!(heap.property_get(heap.object(published).unwrap(), 0), Some(value));
        }
    }
}

#[test]
fn unfilled_object_records_only_the_values_that_fill_it() {
    use crate::typed_ir::Type;
    let mut heap = Heap::new();
    heap.track_slot_representations(&[(0, None), (1, None)]);
    let header = crate::heap::InlineObjectHeader::of(Value::NULL, 1);
    let object = heap.alloc_unfilled_object_under_flagged_prototype(header);
    assert_eq!(heap.slot_type(1, 0), Type::NEVER);
    heap.fill_new_object_slot(object, 0, Value::NULL);
    assert_eq!(heap.slot_type(1, 0), Type::immediate(Value::NULL));
    assert_eq!(heap.unrepresented_slots(), 0);
    assert_eq!(heap.property_get(heap.object(object).unwrap(), 0), Some(Value::NULL));
}

#[test]
fn shape_departures_retire_readers_through_each_storage_path() {
    use crate::{code_dependencies::{DependentCode, SlotDependency}, typed_ir::Type};
    struct Reader(std::cell::Cell<bool>);
    impl DependentCode for Reader {
        fn is_published(&self) -> bool { self.0.get() }
        fn retire(&self) { self.0.set(false); }
    }
    #[derive(Clone, Copy, Debug)]
    enum Change { Shape, Replace, Copy, Append, Remap }
    for change in [Change::Shape, Change::Replace, Change::Copy, Change::Append, Change::Remap] {
        for inline in [false, true] {
            let mut heap = Heap::new();
            heap.track_slot_representations(&[(0, None), (1, None), (1, None), (2, Some((1, 1)))]);
            let (storage, values) = heap.initial_object_storage(1, &[Value::integer(1)], inline);
            let object = heap.alloc_object(Object::with_property_storage(Value::NULL, storage, values));
            let reader: Rc<dyn DependentCode> = Rc::new(Reader(std::cell::Cell::new(true)));
            assert!(heap.subscribe_slot(SlotDependency { shape: 1, slot: 0, ty: Type::INT32 }, &reader));
            let unrelated: Rc<dyn DependentCode> = Rc::new(Reader(std::cell::Cell::new(true)));
            assert!(heap.properties.subscribe_shape(2, &unrelated));
            // No departure: a repeated shape and storage descriptor preserve the
            // shape assumption (physical storage needs its own proof).
            heap.set_object_shape(object, 1);
            let (data, arena) = heap.object_and_arena_mut(object).unwrap();
            data.set_properties(data.properties(), arena);
            assert!(reader.is_published());
            match change {
                Change::Shape => heap.set_object_shape(object, 2),
                Change::Replace => heap.initialize_object_properties(object, 2, &[Value::integer(3)]),
                Change::Copy => {
                    let source = heap.alloc_object_with_properties(Value::NULL, 2, &[Value::integer(3)]);
                    let source = heap.object(source).unwrap().clone();
                    assert!(heap.copy_property_storage(&source, object));
                }
                Change::Append => heap.property_add(object, Value::integer(3), 3),
                Change::Remap => heap.remap_live_object_shapes(&[0, 2, 1, 3],
                    &[(0, None), (1, None), (1, None), (2, Some((2, 1)))]),
            }
            assert!(!reader.is_published(), "{change:?}, inline={inline}");
            assert_eq!(unrelated.is_published(), !matches!(change, Change::Remap), "{change:?}");
            assert_eq!(heap.object(object).unwrap().shape(), if matches!(change, Change::Append) { 3 } else { 2 });
        }
    }
}

#[test]
fn named_property_publication_retires_readers_for_every_record_family() {
    use crate::{code_dependencies::{DependentCode, SlotDependency}, typed_ir::Type};
    struct Reader(std::cell::Cell<bool>);
    impl DependentCode for Reader {
        fn is_published(&self) -> bool { self.0.get() }
        fn retire(&self) { self.0.set(false); }
    }
    #[derive(Clone, Copy, Debug)]
    enum Family { Array, Function, Promise, Iterator, Date }
    for family in [Family::Array, Family::Function, Family::Promise, Family::Iterator, Family::Date] {
        for generic in [false, true] {
            let mut heap = Heap::new();
            heap.track_slot_representations(&[(0, None), (1, None)]);
            heap.alloc_object_with_properties(Value::NULL, 1, &[Value::integer(1)]);
            let reader: Rc<dyn DependentCode> = Rc::new(Reader(std::cell::Cell::new(true)));
            assert!(heap.subscribe_slot(SlotDependency { shape: 1, slot: 0, ty: Type::INT32 }, &reader));
            // Construct storage without publishing it: the publication boundary
            // must admit its contents, regardless of the allocator's fast path.
            let value = Value::number(0.5);
            let (properties, inline) = heap.initial_object_storage(1, &[value], false);
            let object = Object::with_property_storage(Value::NULL, properties, inline);
            let cell = match family {
                Family::Array => Cell::Array { object: ArrayObject::new(object, Vec::<Value>::new()) },
                Family::Function => Cell::Function(Box::new(FunctionRecord {
                    object, realm: Value::NULL, env: Value::NULL,
                    kind: FunctionKind::User(crate::vm::ProgramId::MAIN, 0),
                })),
                Family::Promise => Cell::Promise(Box::new(PromiseCell { object, record: PromiseRecord::pending(None.into()) })),
                Family::Iterator => Cell::Iterator(Box::new(IteratorRecord {
                    object, source: Value::NULL, kind: IteratorKind::Array, index: 0, done: false,
                    ext: IteratorExt::default(),
                })),
                Family::Date => Cell::Date { object: Box::new(object), milliseconds: 0.0 },
            };
            let published = if generic { heap.alloc_cell(cell) } else {
                match cell {
                    Cell::Array { object } => heap.alloc_array(object),
                    Cell::Function(function) => heap.alloc_function(*function),
                    Cell::Promise(promise) => heap.alloc_promise(*promise),
                    Cell::Iterator(iterator) => heap.alloc_iterator(*iterator),
                    cell => heap.alloc(cell),
                }
            };
            assert!(!reader.is_published(), "{family:?}, generic={generic}");
            assert_eq!(heap.slot_type(1, 0), Type::NUMBER);
            assert_eq!(heap.property_get(heap.object(published).unwrap(), 0), Some(value));
            assert_eq!(heap.unrepresented_slots(), 0);
            heap.track_slot_representations(&[(0, None), (1, None)]);
            assert_eq!(heap.slot_type(1, 0), Type::NUMBER, "census includes {family:?}");
        }
    }
}

#[test]
fn memory_census_distinguishes_sparse_environment_storage_from_logical_slots() {
    let mut heap = Heap::new();
    let width = super::cell::SPARSE_CLONE_MIN_SLOTS;
    let slots = EnvironmentSlots::from_values(std::iter::repeat_n(Value::UNDEFINED, width));
    let (owner, _) = heap.alloc_environment(Value::UNDEFINED, 0, slots, EnvironmentScopeRef::of_program(0));
    let Some(Cell::Environment { slots, .. }) = heap.get(owner) else { unreachable!() };
    let sparse = slots.iteration_copy(owner, &[], crate::bytecode::Resets::listed(&[]), |_, _| None).unwrap();
    assert!(sparse.dense().is_none());
    let sparse_bytes = sparse.0.allocated_bytes();
    let (copy, _) = heap.alloc_environment(owner, 0, sparse, EnvironmentScopeRef::of_program(0));
    let census = heap.memory_composition();
    assert_eq!(census.environment_logical_slots, 2 * width);
    assert_eq!(census.environment_sparse_count, 1);
    assert_eq!(census.environment_storage_bytes,
        (1 + super::cell::ENVIRONMENT_PREFIX_WORDS) * size_of::<usize>() + width * size_of::<EnvironmentSlot>() + sparse_bytes);
    assert!(sparse_bytes < width * size_of::<EnvironmentSlot>());
    heap.collect([copy]);
    assert_eq!(heap.memory_composition().environment_storage_bytes, census.environment_storage_bytes);
    heap.collect([]);
    assert_eq!(heap.memory_composition().environment_storage_bytes, 0);
}

/// Allocates the strings of the sweep test, a mix of live and dead ones, into `kept` and `dead`.
fn allocate_sweep_strings(heap: &mut Heap, shared: &JsString, wide: &JsString, kept: &mut Vec<Value>, dead: &mut Vec<Value>) {
    for i in 0..300_usize {
        let mut rope = JsString::from_str(&"r".repeat(1024));
        rope.push_js_string(shared);
        let strings = [
            JsString::from_str("ab"),
            JsString::from_str(&"x".repeat(40 + i % 7)),
            shared.clone(),
            shared.substring(3..150),
            wide.clone(),
            rope,
        ];
        for (position, string) in strings.into_iter().enumerate() {
            let value = heap.alloc_string(string);
            if (i + position) % 2 == 0 { kept.push(value) } else { dead.push(value) }
        }
        // Parts are ASCII strings (`Vm::string_of_parts`).
        let mut parts = Parts::EMPTY;
        let (first, second) = (heap.alloc_string(JsString::from_str("part one")), heap.alloc_string(JsString::from_str(&"p".repeat(64))));
        kept.extend([first, second]);
        let Some(HeapReference::StringRecord(first)) = first.heap_reference() else { unreachable!() };
        let Some(HeapReference::StringRecord(second)) = second.heap_reference() else { unreachable!() };
        assert!(parts.push_text(first) && parts.push_text(second) && parts.push_number(i as f64));
        let value = heap.alloc_parts_string(parts);
        if i % 3 == 0 { kept.push(value) } else { dead.push(value) }
    }
}

/// The swept heap keeps exactly the kept strings, with their atoms and texts.
fn assert_swept(heap: &Heap, kept: &[Value], dead: &[Value], texts: &[String]) {
    assert!(kept.iter().all(|&value| heap.is_live(value)));
    assert!(dead.iter().all(|&value| !heap.is_live(value)));
    assert!(dead.iter().all(|&value| heap.string_atom(value).is_none()), "a dead string's atom is forgotten");
    assert!(kept.iter().all(|&value| heap.string_atom(value).is_some()));
    for (&value, text) in kept.iter().zip(texts) {
        assert_eq!(&heap.string(value).map_or_else(String::new, JsString::to_host_owned), text);
    }
}

#[test]
fn a_full_collection_sweeping_strings_on_a_worker_matches_the_serial_sweep() {
    let run = |on_worker: bool| {
        let mut heap = Heap::new();
        if on_worker {
            heap.force_parallel_mark_for_test();
        }
        // Text shared with this frame: every dead heap copy and slice drops a clone of its `Rc`.
        let shared = JsString::from_str(&"s".repeat(200));
        let wide = JsString::from_units(&[0x3b1, 0x3b2, 0x3b3, 0x3b4]);
        let (mut kept, mut dead) = (Vec::new(), Vec::new());
        allocate_sweep_strings(&mut heap, &shared, &wide, &mut kept, &mut dead);
        for (atom, &value) in dead.iter().chain(&kept).enumerate() {
            heap.record_string_atom(value, atom as u32);
        }
        let texts: Vec<String> = kept
            .iter()
            .map(|&value| heap.string(value).map_or_else(String::new, JsString::to_host_owned))
            .collect();
        heap.collect_with_object_roots(kept.iter().copied(), |_, _, _, _, _| {}, None, None, true);
        assert_swept(&heap, &kept, &dead, &texts);
        assert_eq!(shared.to_host_owned(), "s".repeat(200));
        (heap.string_records.live_count(), heap.payload_bytes)
    };
    assert_eq!(run(true), run(false));
}
