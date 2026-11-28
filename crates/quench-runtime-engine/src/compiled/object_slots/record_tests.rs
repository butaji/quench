//! Record-proof and checked-table tests: the projections `object_slots` emits, run as native code.

use super::*;
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

#[cfg(feature = "jit")]
#[test]
fn checked_table_admits_once_without_admitting_receiver_handles() {
    let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
    let mut signature = Signature::new(isa.default_call_conv());
    signature.params.extend([AbiParam::new(types::I64); 4]);
    signature.returns.push(AbiParam::new(types::I64));
    let mut native = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut native, &mut context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let parameters = builder.block_params(entry).to_vec();
    let miss = builder.create_block();
    let table = CheckedObjectTable::prove(&mut builder, parameters[0], miss);
    // The second receiver retains its own family and range checks, even
    // after the first receiver has proved both against this same table.
    ObjectRecord::prove(&mut builder, parameters[1], table, parameters[3], None, miss);
    let record = ObjectRecord::prove(&mut builder, parameters[2], table, parameters[3], None, miss);
    let address = record.address(&mut builder);
    builder.ins().return_(&[address]);
    builder.switch_to_block(miss);
    let deleted = builder.ins().iconst(types::I64, Value::DELETED.to_bits() as i64);
    builder.ins().return_(&[deleted]);
    builder.seal_all_blocks();
    builder.finalize(isa.frontend_config());
    let guards = native.layout.blocks().flat_map(|block| native.layout.block_insts(block))
        .filter(|inst| native.dfg.insts[*inst].opcode() == ir::Opcode::Brif).count();
    // A receiver's family alone: a direct object's id names a record of the reservation.
    const TABLE_GUARDS: usize = 1;
    const RECEIVER_GUARDS: usize = 1;
    assert_eq!(guards, TABLE_GUARDS + 2 * RECEIVER_GUARDS, "{native}");
    let memory = compile_native_function(native, &*isa, usize::MAX, CodeMemory::new).unwrap();
    // SAFETY: the compiled function only checks bits and computes an
    // address; it dereferences neither table nor supplied receiver.
    let run = unsafe { std::mem::transmute::<*mut u8,
        unsafe extern "C" fn(u64, u64, u64, u64) -> u64>(memory.pointer()) };
    let layout = quench_runtime::tier_surface::NATIVE_OBJECT_LAYOUT;
    let records = vec![0_u8; layout.record_size * 2];
    let base = records.as_ptr() as u64;
    // Granule 1's class cell names record 1 in its header's high half.
    let mut classes = vec![0_u64; 2];
    classes[1] = 1 << u32::BITS;
    let class_base = classes.as_ptr() as u64;
    let object = Value::direct_object(1).to_bits();
    assert_eq!(unsafe { run(base, object, object, class_base) }, base + layout.record_size as u64);
    for (table, first, second) in [
        (0, object, object), (base, Value::NULL.to_bits(), object), (base, object, Value::NULL.to_bits()),
    ] {
        assert_eq!(unsafe { run(table, first, second, class_base) }, Value::DELETED.to_bits());
    }
}

#[cfg(feature = "jit")]
#[test]
fn direct_family_admission_tests_the_family_alone() {
    use quench_runtime::typed_ir::{BindingTime, Fact, Projection, Proof, Representation, Type};
    let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
    let layout = quench_runtime::tier_surface::NATIVE_OBJECT_LAYOUT;
    // A direct object's id names a record of the reservation, whose base every native
    // entry reads: only the family needs a test, and none where a fact proves it.
    for (ty, expected_guards) in [
        (None, 1),
        (Some(Type::language(quench_runtime::ValueType::Object)), 1),
        (Some(Type::DIRECT_OBJECT_FAMILY), 0),
        (Some(Type::direct_object(0)), 0),
        (Some(Type::direct_object(0).join(Type::language(quench_runtime::ValueType::Null))), 1),
        (Some(Type::direct_object(0).join(Type::language(quench_runtime::ValueType::Null).join(Type::language(quench_runtime::ValueType::Undefined)))), 1),
    ] {
        let mut signature = Signature::new(isa.default_call_conv());
        signature.params.extend([AbiParam::new(types::I64); 3]);
        signature.returns.push(AbiParam::new(types::I64));
        let mut native = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut native, &mut context);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        let parameters = builder.block_params(entry).to_vec();
        let miss = builder.create_block();
        let known = ty.map(|ty| Fact { ty, representation: Representation::Tagged,
            binding: BindingTime::Dynamic, proof: Proof::Definition, projection: Projection::Exact }
            .verified().unwrap());
        let record = ObjectRecord::prove(&mut builder, parameters[1], parameters[0], parameters[2], known, miss);
        let address = record.address(&mut builder);
        builder.ins().return_(&[address]);
        builder.switch_to_block(miss);
        let deleted = builder.ins().iconst(types::I64, Value::DELETED.to_bits() as i64);
        builder.ins().return_(&[deleted]);
        builder.seal_all_blocks();
        builder.finalize(isa.frontend_config());
        let guards = native.layout.blocks().flat_map(|block| native.layout.block_insts(block))
            .filter(|inst| native.dfg.insts[*inst].opcode() == ir::Opcode::Brif).count();
        assert_eq!(guards, expected_guards, "{ty:?}: {native}");
        if ty.is_some_and(|ty| ty.is_direct_object_family()) { continue; }
        let memory = compile_native_function(native, &*isa, usize::MAX, CodeMemory::new).unwrap();
        // SAFETY: the entry takes two u64 words and performs guarded address
        // arithmetic only; it never dereferences the supplied table address.
        let run = unsafe { std::mem::transmute::<*mut u8,
            unsafe extern "C" fn(u64, u64, u64) -> u64>(memory.pointer()) };
        let records = vec![0_u8; layout.record_size * 2];
        let base = records.as_ptr() as u64;
        let mut classes = vec![0_u64; 2];
        classes[1] = 1 << u32::BITS;
        let class_base = classes.as_ptr() as u64;
        let bits = Value::direct_object(1).to_bits();
        assert_eq!(unsafe { run(base, bits, class_base) }, base + layout.record_size as u64);
        assert_eq!(unsafe { run(base, Value::NULL.to_bits(), class_base) }, Value::DELETED.to_bits(), "{ty:?}");
        assert_eq!(unsafe { run(base, Value::UNDEFINED.to_bits(), class_base) }, Value::DELETED.to_bits(), "{ty:?}");
    }
}
