use super::*;

struct SilentHost;
impl Host for SilentHost {
    fn write_line(&mut self, _: &str) {}
    fn clock_millis(&mut self) -> f64 {
        0.0
    }
}

#[test]
fn third_receiver_promotes_field_site_to_megamorphic() {
    let mut vm = Vm::new(SilentHost);
    vm.field_caches.push(EMPTY_CACHE);
    vm.megamorphic_field_indices.push(NO_MEGAMORPHIC_FIELD);
    for receiver in 1..=4 {
        vm.record_field_cache(
            0,
            FieldCache {
                receiver,
                owner: Value::number(f64::from(receiver)),
                owner_shape: receiver,
                slot: 0,
                depth: 0,
            },
        );
    }
    let table = &vm.megamorphic_fields[0];
    assert_eq!(table.len(), 4);
    assert!(table.get(1).is_some() && table.get(4).is_some());
    assert_eq!(vm.megamorphic_field_indices, [0]);
}

#[test]
fn shape_slot_index_is_derived_from_immutable_shape_keys() {
    let mut vm = Vm::new(SilentHost);
    let first = vm.intern_atom("first");
    let second = vm.intern_atom("second");
    let first_shape = vm.transition_shape(0, first);
    let shape = vm.transition_shape(first_shape, second);
    assert_eq!(vm.shape_slot(shape, first), Some(0));
    assert_eq!(vm.shape_slot(shape, second), Some(1));
    let missing = vm.intern_atom("missing");
    assert_eq!(vm.shape_slot(shape, missing), None);
}
