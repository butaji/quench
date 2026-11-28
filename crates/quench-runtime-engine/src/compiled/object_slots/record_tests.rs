//! Field-load validity tests of `object_slots` (the record proofs' tests went with the object record table, S7f).

use super::*;

#[test]
fn direct_field_plans_reject_dictionary_and_missing_receiver_shapes() {
    let field = NativeFieldLoad { shape: 0, holder: None, slot: 0, storage_len: 1, storage: None, ty: quench_runtime::typed_ir::Type::ANY, inherited: None, function_shape: false, formed: None, holder_cell: None, class: None };
    assert!(field.valid());
    for shape in [quench_runtime::vm::NO_FIELD_RECEIVER,
        quench_runtime::value_vec::DICTIONARY_STORAGE, quench_runtime::value_vec::DICTIONARY_STORAGE | 1] {
        assert!(!NativeFieldLoad { shape, ..field }.valid());
    }
}

