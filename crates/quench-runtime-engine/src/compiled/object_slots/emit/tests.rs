//! Tests of the Cranelift emission of the staged operations.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::{library_surface::Heap, tier_surface::{Cell, Object, PlainPropertyStorage}};
use quench_runtime::staged::ic;
use quench_runtime::staged::object::{own_layout, read_slot, Form, PackedAccess};
use quench_runtime::staged::Region;
use quench_runtime::typed_ir::Representation;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// Objects of every storage form, with their values: narrow and wide, inline and spilled,
/// plain and marked as prototypes.
fn objects_of_every_form(heap: &mut Heap) -> Vec<(Value, Vec<Value>)> {
    let wide = quench_runtime::value_vec::INLINE_PROPERTY_COUNT + 2;
    for shape in 1..=wide {
        heap.register_property_shape(shape as u32, shape, None);
    }
    let reference = heap.alloc(Cell::Error("reference".into()));
    let mut objects = Vec::new();
    for prototype in [false, true] {
        for values in [vec![Value::number(7.0), reference], vec![Value::number(0.5), Value::NULL],
            (0..wide).map(|index| Value::number(index as f64)).collect()] {
            let object = heap.alloc_object(Object::new(Value::NULL));
            for (slot, value) in values.iter().enumerate() {
                heap.property_add(object, *value, slot as u32 + 1);
            }
            if prototype { heap.mark_prototype(object); }
            objects.push((object, values));
        }
    }
    objects
}

/// Native code reading `slot` of a record proved to be in `form`, or `DELETED` where it leaves.
fn compiled_own_read(isa: &dyn cranelift_codegen::isa::TargetIsa, form: Form, slot: u16) -> CodeMemory {
    let mut signature = Signature::new(isa.default_call_conv());
    signature.params.extend([AbiParam::new(types::I64); 2]);
    signature.returns.push(AbiParam::new(types::I64));
    let mut native = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut native, &mut context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let [record, base] = builder.block_params(entry).try_into().unwrap();
    let miss = builder.create_block();
    let mut stage = Emit { builder: &mut builder, property_values: Some(base), gate: None , payload: None, facts: Default::default(), fresh: false };
    let inline = quench_runtime::staged::object::inline_storage(&mut stage, record);
    let layout = own_layout(&mut stage, record, inline, form,
        usize::from(slot) + 1, PackedAccess::Reload, miss).unwrap();
    let value = read_slot(&mut stage, layout, slot, Representation::Tagged);
    builder.ins().return_(&[value]);
    builder.switch_to_block(miss);
    let deleted = builder.ins().iconst(types::I64, Value::DELETED.to_bits() as i64);
    builder.ins().return_(&[deleted]);
    builder.seal_all_blocks();
    builder.finalize(isa.frontend_config());
    compile_native_function(native, isa, usize::MAX, CodeMemory::new).unwrap()
}

/// Whether a site proved `form` reads an object stored as `storage` (`None`: values extras).
fn handles(form: Form, storage: Option<PlainPropertyStorage>) -> bool {
    match (form, storage) {
        (_, None) => false,
        (Form::Packed, Some(storage)) => storage == PlainPropertyStorage::PackedInline,
        (Form::Arena, Some(storage)) => storage == PlainPropertyStorage::Arena,
        (Form::Either, Some(_)) => true,
    }
}

/// The residual of `staged::own_layout` and `read_slot` reads what the interpreter's
/// reading of the same definition reads, for every form a site may prove, and leaves
/// for the forms it does not handle (values extras displaced, or a form the site did not
/// prove).
#[test]
fn emitted_own_reads_agree_with_the_interpreter_or_leave() {
    let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
    let mut heap = Heap::new();
    let objects = objects_of_every_form(&mut heap);
    for form in [Form::Packed, Form::Arena, Form::Either] {
        for slot in 0..2_u16 {
            let memory = compiled_own_read(&*isa, form, slot);
            // SAFETY: the function reads the record and the arena run it names, both live.
            let run = unsafe { std::mem::transmute::<*mut u8, unsafe extern "C" fn(u64, u64) -> u64>(memory.pointer()) };
            for (object, values) in &objects {
                let data = heap.object(*object).unwrap().record_only().unwrap();
                let Some(expected) = values.get(usize::from(slot)) else { continue };
                let read = Value::from_bits(unsafe { run(data as *const Object as u64, heap.property_values_base() as u64) });
                let storage = data.plain_property_storage();
                let expected = if handles(form, storage) { *expected } else { Value::DELETED };
                assert_eq!(read, expected, "{form:?} slot {slot} {storage:?}");
            }
        }
    }
}
/// Native code running `string_char_code_at` of the string value over the string records,
/// or `DELETED` where a row leaves.
fn compiled_char_code_at(isa: &dyn cranelift_codegen::isa::TargetIsa) -> CodeMemory {
    let mut signature = Signature::new(isa.default_call_conv());
    signature.params.extend([AbiParam::new(types::I64); 3]);
    signature.returns.push(AbiParam::new(types::I64));
    let mut native = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut native, &mut context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let [records, string, index] = builder.block_params(entry).try_into().unwrap();
    let miss = builder.create_block();
    let mut stage = Emit { builder: &mut builder, property_values: None, gate: None , payload: None, facts: Default::default(), fresh: false };
    let flat = quench_runtime::staged::strings::flat_string(&mut stage, records, string, miss).unwrap();
    let code = quench_runtime::staged::strings::string_char_code_at(&mut stage, flat, index, miss).unwrap();
    builder.ins().return_(&[code]);
    builder.switch_to_block(miss);
    let deleted = builder.ins().iconst(types::I64, Value::DELETED.to_bits() as i64);
    builder.ins().return_(&[deleted]);
    builder.seal_all_blocks();
    builder.finalize(isa.frontend_config());
    compile_native_function(native, isa, usize::MAX, CodeMemory::new).unwrap()
}

/// The residual of `strings::flat_string` and `string_char_code_at` reads what the
/// interpreter's reading of the same rows reads, for each form of string record, and leaves
/// where the rows leave (a slice, a double index, a value that is no string).
#[test]
fn emitted_string_reads_agree_with_the_interpreter_or_leave() {
    use quench_runtime::staged::{Concrete, strings};
    use quench_runtime::vm::wtf16::{JsString, SHORT_ASCII_CAPACITY};
    let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
    let memory = compiled_char_code_at(&*isa);
    // SAFETY: the function reads the string record and the units its form names, all live.
    let run = unsafe { std::mem::transmute::<*mut u8, unsafe extern "C" fn(u64, u64, u64) -> u64>(memory.pointer()) };
    let mut heap = Heap::new();
    let long = JsString::from_str(&"q".repeat(SHORT_ASCII_CAPACITY + 4));
    let strings_of_every_form = [
        heap.alloc_string(JsString::from_str("")), heap.alloc_string(JsString::from_str("abc")),
        heap.alloc_string(JsString::from_units(&[0x61, 0x20ac])), heap.alloc_string(long.substring(1..20)),
        heap.alloc_string(long), Value::integer(1),
    ];
    let records = heap.string_record_base().map_or(0, |base| base as u64);
    for string in strings_of_every_form {
        for index in [Value::integer(0), Value::integer(1), Value::integer(2), Value::integer(-1), Value::number(0.5), Value::UNDEFINED] {
            let mut stage = Concrete::pure();
            let expected = strings::flat_string(&mut stage, records, string.to_bits(), ())
                .and_then(|flat| strings::string_char_code_at(&mut stage, flat, index.to_bits(), ()))
                .map_or(Value::DELETED, Value::from_bits);
            let read = Value::from_bits(unsafe { run(records, string.to_bits(), index.to_bits()) });
            assert_eq!(read, expected, "{:#x} at {:#x}", string.to_bits(), index.to_bits());
        }
    }
}

/// A function summing the 32-bit word at offset 8 of its first parameter, read through
/// `Emit` in the body of a counted loop over its second.
fn word_summed_in_a_loop(isa: &dyn cranelift_codegen::isa::TargetIsa, at: Access) -> ir::Function {
    let mut signature = Signature::new(isa.default_call_conv());
    signature.params.extend([AbiParam::new(types::I64); 2]);
    signature.returns.push(AbiParam::new(types::I64));
    let mut function = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
    regions::declare_regions(&mut function);
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut function, &mut context);
    let (entry, header, body, exit) =
        (builder.create_block(), builder.create_block(), builder.create_block(), builder.create_block());
    builder.append_block_params_for_function_params(entry);
    let (sum, index) = (builder.append_block_param(header, types::I64), builder.append_block_param(header, types::I64));
    let total = builder.append_block_param(exit, types::I64);
    builder.switch_to_block(entry);
    let [base, count] = builder.block_params(entry).try_into().unwrap();
    let zero = builder.ins().iconst(types::I64, 0);
    builder.ins().jump(header, &[zero.into(), zero.into()]);
    builder.switch_to_block(header);
    let more = builder.ins().icmp(IntCC::UnsignedLessThan, index, count);
    builder.ins().brif(more, body, &[], exit, &[sum.into()]);
    builder.switch_to_block(body);
    let word = Emit { builder: &mut builder, property_values: None, gate: None , payload: None, facts: Default::default(), fresh: false }.load_u32(at, base, 8);
    let next_sum = builder.ins().iadd(sum, word);
    let next_index = builder.ins().iadd_imm_u(index, 1);
    builder.ins().jump(header, &[next_sum.into(), next_index.into()]);
    builder.switch_to_block(exit);
    builder.ins().return_(&[total]);
    builder.seal_all_blocks();
    builder.finalize(isa.frontend_config());
    function
}

/// The opcode and flags of every memory access of `function`.
fn accesses(function: &ir::Function) -> Vec<(ir::Opcode, MemFlagsData)> {
    function.layout.blocks().flat_map(|block| function.layout.block_insts(block))
        .filter_map(|inst| function.dfg.insts[inst].memflags_data(&function.dfg)
            .map(|flags| (function.dfg.insts[inst].opcode(), flags)))
        .collect()
}

/// A read-only word in a loop is a plain, trusted `load` tagged with its region and
/// movable, the one kind of load the egraph hoists and merges.
#[test]
fn a_readonly_word_read_in_a_loop_is_a_movable_plain_load() {
    let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
    let function = word_summed_in_a_loop(&*isa, Region::LoopEntry.readonly());
    cranelift_codegen::verify_function(&function, &*isa).unwrap();
    let [(opcode, flags)] = accesses(&function)[..] else { panic!("one access") };
    assert_eq!(opcode, ir::Opcode::Load);
    assert!(flags.readonly() && flags.can_move() && flags.notrap() && flags.aligned());
    assert_eq!(flags.alias_region(), regions::flags(Region::LoopEntry.readonly()).alias_region());
}

/// A word a store can change keeps its place: not movable, still tagged with its region.
#[test]
fn a_mutable_word_read_in_a_loop_stays_in_the_loop() {
    let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
    let function = word_summed_in_a_loop(&*isa, Region::ObjectSlots.mutable());
    let [(opcode, flags)] = accesses(&function)[..] else { panic!("one access") };
    assert_eq!(opcode, ir::Opcode::Uload32);
    assert!(!flags.readonly() && !flags.can_move() && flags.notrap());
    assert_eq!(flags.alias_region(), regions::flags(Region::ObjectSlots.mutable()).alias_region());
}

/// How Cranelift writes an immediate in its CLIF text: decimal below ten thousand, else hex in
/// groups of four digits from the right (`0x2000_0000`).
fn clif_immediate(value: u64) -> String {
    if value < 10_000 {
        return value.to_string();
    }
    let digits = format!("{value:x}");
    let mut groups = Vec::new();
    let mut rest = digits.as_str();
    while rest.len() > 4 {
        let (head, tail) = rest.split_at(rest.len() - 4);
        groups.push(tail);
        rest = head;
    }
    groups.push(rest);
    groups.reverse();
    format!("0x{}", groups.join("_"))
}

/// The CLIF of a formed own read of `slot` through layout id `id`, as the lane lowers a
/// single-shape field read with a hint: `IcOp::GuardLayout` of the id, then the slot read in
/// the form the id names (`ic::form_of_layout`).
fn formed_own_read_clif(isa: &dyn cranelift_codegen::isa::TargetIsa, id: u32, slot: u16) -> String {
    let mut signature = Signature::new(isa.default_call_conv());
    signature.params.extend([AbiParam::new(types::I64); 2]);
    signature.returns.push(AbiParam::new(types::I64));
    let mut native = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut native, &mut context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let [record, base] = builder.block_params(entry).try_into().unwrap();
    let miss = builder.create_block();
    let mut stage = Emit { builder: &mut builder, property_values: Some(base), gate: None , payload: None, facts: Default::default(), fresh: false };
    let ops = [ic::IcOp::GuardLayout(id), ic::IcOp::LoadSlot { slot, form: ic::form_of_layout(id),
        storage_len: usize::from(slot) + 1, rep: Representation::Tagged }];
    let value = ic::run(&mut stage, &ops, ic::Reached::bare(record), None, miss).expect("a load ends the read");
    builder.ins().return_(&[value]);
    builder.switch_to_block(miss);
    let deleted = builder.ins().iconst(types::I64, Value::DELETED.to_bits() as i64);
    builder.ins().return_(&[deleted]);
    builder.seal_all_blocks();
    builder.finalize(isa.frontend_config());
    native.display().to_string()
}

/// A formed own read of a record whose layout id names the arena form emits no inline-flag
/// test: the id's one compare proves the form, so the run is the payload's, with no test of the
/// inline bit or of the extras tag. The read is the form the id names, not the one a site saw.
#[test]
fn a_formed_arena_read_emits_no_inline_flag_test() {
    let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
    let mut heap = Heap::new();
    let objects = objects_of_every_form(&mut heap);
    let arena_object = objects.iter().map(|(object, _)| *object)
        .find(|object| heap.object(*object).unwrap().record_only().unwrap().plain_property_storage() == Some(PlainPropertyStorage::Arena))
        .expect("a wide object keeps its values in the arena");
    let id = heap.object(arena_object).unwrap().hint_shape();
    assert_eq!(ic::form_of_layout(id), Form::Arena);
    let clif = formed_own_read_clif(&*isa, id, 0);
    let inline_form = quench_runtime::tier_surface::NATIVE_OBJECT_LAYOUT.inline_form as u64;
    assert!(!clif.contains(&clif_immediate(inline_form)), "the arena form tests no inline flag: {clif}");
}
