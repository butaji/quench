//! The specializer's Cranelift stage: `crate::staged` operations emitted as IR.

use super::*;
use crate::staged::{Domain, NumericStage, Stage, Table};
use crate::vm::operations::{NumberArithmetic, NumberRelation};

/// The specializer's stage (`crate::staged`): each operation emits its residual as Cranelift
/// IR into `builder`, where a guard's failure jumps to its miss block.
pub(in crate::vm::compiled) struct Emit<'a, 'b> {
    pub(in crate::vm::compiled) builder: &'a mut FunctionBuilder<'b>,
    /// The property arena's value base (`Table::PropertyValues`), where the operation
    /// addresses a run; a read through a proved layout already holds its address.
    pub(in crate::vm::compiled) property_values: Option<ir::Value>,
}

impl Stage for Emit<'_, '_> {
    type Word = ir::Value;
    /// Any integer: nonzero holds.
    type Cond = ir::Value;
    type Miss = ir::Block;

    fn constant(&mut self, bits: u64) -> ir::Value { self.builder.ins().iconst(types::I64, bits as i64) }
    fn load_u32(&mut self, base: ir::Value, offset: i32) -> ir::Value {
        self.builder.ins().uload32(MemFlagsData::trusted(), base, offset)
    }
    fn load_u64(&mut self, base: ir::Value, offset: i32) -> ir::Value {
        self.builder.ins().load(types::I64, MemFlagsData::trusted(), base, offset)
    }
    fn load_table(&mut self, address: ir::Value) -> ir::Value {
        self.builder.ins().load(types::I64, MemFlagsData::trusted().with_readonly().with_can_move(), address, 0)
    }
    fn add(&mut self, a: ir::Value, b: ir::Value) -> ir::Value { self.builder.ins().iadd(a, b) }
    fn or(&mut self, a: ir::Value, b: ir::Value) -> ir::Value { self.builder.ins().bor(a, b) }
    fn add_imm(&mut self, a: ir::Value, imm: u64) -> ir::Value { self.builder.ins().iadd_imm_u(a, imm as i64) }
    fn mul_imm(&mut self, a: ir::Value, imm: u64) -> ir::Value { self.builder.ins().imul_imm_u(a, imm as i64) }
    fn and_imm(&mut self, a: ir::Value, imm: u64) -> ir::Value { self.builder.ins().band_imm_u(a, imm as i64) }
    fn or_imm(&mut self, a: ir::Value, imm: u64) -> ir::Value { self.builder.ins().bor_imm_u(a, imm as i64) }
    fn shr_imm(&mut self, a: ir::Value, shift: u32) -> ir::Value { self.builder.ins().ushr_imm_u(a, i64::from(shift)) }
    fn sar32_imm(&mut self, a: ir::Value, shift: u32) -> ir::Value {
        let low = self.builder.ins().ireduce(types::I32, a);
        let shifted = self.builder.ins().sshr_imm_u(low, i64::from(shift));
        self.builder.ins().uextend(types::I64, shifted)
    }
    fn eq_imm(&mut self, a: ir::Value, imm: u64) -> ir::Value { self.builder.ins().icmp_imm_u(IntCC::Equal, a, imm as i64) }
    fn ne_imm(&mut self, a: ir::Value, imm: u64) -> ir::Value { self.builder.ins().icmp_imm_u(IntCC::NotEqual, a, imm as i64) }
    fn ugt_imm(&mut self, a: ir::Value, imm: u64) -> ir::Value {
        self.builder.ins().icmp_imm_u(IntCC::UnsignedGreaterThan, a, imm as i64)
    }
    fn not(&mut self, cond: ir::Value) -> ir::Value { self.builder.ins().icmp_imm_u(IntCC::Equal, cond, 0) }
    fn choose(&mut self, cond: ir::Value, then: impl FnOnce(&mut Self) -> ir::Value,
        otherwise: impl FnOnce(&mut Self) -> ir::Value) -> ir::Value {
        let (then_block, else_block, join) = (self.builder.create_block(), self.builder.create_block(), self.builder.create_block());
        let value = self.builder.append_block_param(join, types::I64);
        self.builder.ins().brif(cond, then_block, &[], else_block, &[]);
        self.builder.switch_to_block(then_block);
        self.builder.seal_block(then_block);
        let taken = then(self);
        self.builder.ins().jump(join, &[ir::BlockArg::Value(taken)]);
        self.builder.switch_to_block(else_block);
        self.builder.seal_block(else_block);
        let other = otherwise(self);
        self.builder.ins().jump(join, &[ir::BlockArg::Value(other)]);
        self.builder.switch_to_block(join);
        self.builder.seal_block(join);
        value
    }
    fn guard(&mut self, cond: ir::Value, miss: ir::Block) -> Option<()> {
        emit_js_guard(self.builder, cond, miss);
        Some(())
    }
    fn known(&self, _cond: ir::Value) -> Option<bool> { None }
    fn table(&mut self, table: Table) -> ir::Value {
        match table { Table::PropertyValues => self.property_values.expect("an operation addressing the arena has its base") }
    }
}

impl NumericStage for Emit<'_, '_> {
    fn relation(&mut self, relation: NumberRelation, domain: Domain,
        left: ir::Value, right: ir::Value) -> ir::Value {
        emit_number_relation(self.builder, relation, left, right, domain)
    }
    fn float_arithmetic(&mut self, operation: NumberArithmetic,
        left: ir::Value, right: ir::Value) -> ir::Value {
        emit_number_arithmetic_operation(self.builder, operation, left, right)
    }
    fn float_negate(&mut self, value: ir::Value) -> ir::Value {
        self.builder.ins().fneg(value)
    }
    fn int_arithmetic(&mut self, operation: NumberArithmetic,
        left: ir::Value, right: ir::Value) -> (ir::Value, ir::Value) {
        emit_checked_int32_arithmetic(self.builder, operation, left, right)
    }
    fn i32_constant(&mut self, value: i32) -> ir::Value { self.builder.ins().iconst(types::I32, i64::from(value)) }
    fn i32_to_f64(&mut self, a: ir::Value) -> ir::Value { self.builder.ins().fcvt_from_sint(types::F64, a) }
    fn i32_is_zero(&mut self, a: ir::Value) -> ir::Value { self.builder.ins().icmp_imm_u(IntCC::Equal, a, 0) }
    fn i32_is_negative(&mut self, a: ir::Value) -> ir::Value { self.builder.ins().icmp_imm_s(IntCC::SignedLessThan, a, 0) }
    fn both(&mut self, a: ir::Value, b: ir::Value) -> ir::Value { self.builder.ins().band(a, b) }
    fn either(&mut self, a: ir::Value, b: ir::Value) -> ir::Value { self.builder.ins().bor(a, b) }
    fn cond_word(&mut self, cond: ir::Value) -> ir::Value { cond }
    fn guard_false(&mut self, cond: ir::Value, miss: ir::Block) -> Option<()> {
        let passed = self.builder.create_block();
        self.builder.ins().brif(cond, miss, &[], passed, &[]);
        self.builder.switch_to_block(passed);
        Some(())
    }
}

#[cfg(all(test, feature = "jit"))]
mod tests {
    use super::*;
    use crate::heap::{Cell, Heap, Object, PlainPropertyStorage};
    use crate::staged::{Form, PackedAccess};

    /// Objects of every storage form, with their values: narrow and wide, inline and spilled,
    /// plain and marked as prototypes.
    fn objects_of_every_form(heap: &mut Heap) -> Vec<(Value, Vec<Value>)> {
        let wide = crate::value_vec::INLINE_PROPERTY_COUNT + 2;
        for shape in 1..=wide {
            heap.register_property_shape(shape as u32, shape, None);
        }
        let reference = heap.alloc(Cell::Error("reference".into()));
        let mut objects = Vec::new();
        for prototype in [false, true] {
            for values in [vec![Value::number(7.0), reference], vec![Value::number(0.5), Value::NULL],
                (0..wide).map(|index| Value::number(index as f64)).collect()] {
                let object = heap.alloc(Cell::Object(Object::new(Value::NULL)));
                for (slot, value) in values.iter().enumerate() {
                    heap.property_add(object, *value, slot as u32 + 1);
                }
                if prototype { heap.object_record_mut(object).unwrap().mark_as_prototype(); }
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
        let mut stage = Emit { builder: &mut builder, property_values: Some(base) };
        let inline = crate::staged::object::inline_storage(&mut stage, record);
        let layout = crate::staged::own_layout(&mut stage, record, inline, form,
            usize::from(slot) + 1, PackedAccess::Reload, miss).unwrap();
        let value = crate::staged::read_slot(&mut stage, layout, slot);
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
        let isa = super::super::super::super::jit_backend::target_isa().unwrap();
        let mut heap = Heap::new();
        let objects = objects_of_every_form(&mut heap);
        for form in [Form::Packed, Form::Arena, Form::Either] {
            for slot in 0..2_u16 {
                let memory = compiled_own_read(&*isa, form, slot);
                // SAFETY: the function reads the record and the arena run it names, both live.
                let run = unsafe { std::mem::transmute::<*mut u8, unsafe extern "C" fn(u64, u64) -> u64>(memory.pointer()) };
                for (object, values) in &objects {
                    let data = heap.object(*object).unwrap();
                    let Some(expected) = values.get(usize::from(slot)) else { continue };
                    let read = Value::from_bits(unsafe { run(data as *const Object as u64, heap.property_values_base() as u64) });
                    let storage = data.plain_property_storage();
                    let expected = if handles(form, storage) { *expected } else { Value::DELETED };
                    assert_eq!(read, expected, "{form:?} slot {slot} {storage:?}");
                }
            }
        }
    }
}
