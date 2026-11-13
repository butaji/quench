//! The specializer's Cranelift stage: `crate::staged` operations emitted as IR.

use super::*;
use crate::staged::region::Mutability;
use crate::staged::{Access, Domain, NumericStage, Stage, Table};
use crate::staged::flow::FlowStage;
use crate::vm::compiled::regions;
use crate::vm::operations::{NumberArithmetic, NumberBitwise, NumberRelation};

/// One Cranelift instruction at the stage's builder: `ins!(self, iadd(a, b))` is
/// `self.builder.ins().iadd(a, b)`.
macro_rules! ins {
    ($stage:expr, $instruction:ident($($argument:expr),* $(,)?)) => { $stage.builder.ins().$instruction($($argument),*) };
}

/// The specializer's stage (`crate::staged`): each operation emits its residual as Cranelift
/// IR into `builder`, where a guard's failure jumps to its miss block.
pub(in crate::vm::compiled) struct Emit<'a, 'b> {
    pub(in crate::vm::compiled) builder: &'a mut FunctionBuilder<'b>,
    /// The property arena's value base (`Table::PropertyValues`), where the operation
    /// addresses a run; a read through a proved layout already holds its address.
    pub(in crate::vm::compiled) property_values: Option<ir::Value>,
    /// The dispatch payload that holds the object marks the store barrier's test reads, where
    /// the operation stores (`Stage::store_gate`).
    pub(in crate::vm::compiled) gate: Option<ir::Value>,
    /// The dispatch payload that holds the slot-observation flag and the object marks the store
    /// barrier's test reads, where the operation stores (`Stage::store_gate`).
    pub(in crate::vm::compiled) gate: Option<ir::Value>,
}

/// An immediate of the Cranelift stage: a constant, or a value the caller holds as one (a stencil's
/// patch hole, which is an operand until the patcher fills it).
#[derive(Clone, Copy)]
pub(in crate::vm::compiled) enum EmitImm {
    Const(u64),
}

impl From<u64> for EmitImm { fn from(bits: u64) -> Self { Self::Const(bits) } }
impl From<u32> for EmitImm { fn from(bits: u32) -> Self { Self::Const(u64::from(bits)) } }

impl Emit<'_, '_> {
    /// `imm` as an operand: Cranelift folds a constant operand back into the instruction's immediate.
    fn word(&mut self, imm: EmitImm) -> ir::Value {
        match imm { EmitImm::Const(bits) => self.constant(bits) }
    }
    fn compare(&mut self, condition: IntCC, a: ir::Value, b: ir::Value) -> ir::Value { self.builder.ins().icmp(condition, a, b) }
    fn widen(&mut self, a: ir::Value) -> ir::Value { self.builder.ins().uextend(types::I64, a) }
}

impl Stage for Emit<'_, '_> {
    type Proof = super::super::EmitProof;
    type Word = ir::Value;
    /// Any integer: nonzero holds.
    type Cond = ir::Value;
    type Miss = ir::Block;
    type Imm = EmitImm;

    fn site(&mut self, v: u64) -> EmitImm { EmitImm::Const(v) }
    fn load_site(&mut self, at: Access, base: ir::Value, offset: EmitImm) -> ir::Value {
        match offset {
            EmitImm::Const(bits) => self.load_u64(at, base, bits as i32),
        }
    }
    fn shape_in_site(&mut self, shape: ir::Value, shapes: &[u32]) -> ir::Value { self.shape_in(shape, shapes) }

    fn constant(&mut self, bits: u64) -> ir::Value { self.builder.ins().iconst(types::I64, bits as i64) }
    /// A read-only word is a plain `load` and a `uextend`: the egraph merges and hoists only
    /// plain loads, and the extend folds into the target's zero-extending load.
    fn load_u32(&mut self, at: Access, base: ir::Value, offset: i32) -> ir::Value {
        match at.mutability {
            Mutability::Readonly => {
                let word = self.builder.ins().load(types::I32, regions::flags(at), base, offset);
                self.builder.ins().uextend(types::I64, word)
            }
            Mutability::Mutable => self.builder.ins().uload32(regions::flags(at), base, offset),
        }
    }
    fn load_u64(&mut self, at: Access, base: ir::Value, offset: i32) -> ir::Value {
        self.builder.ins().load(types::I64, regions::flags(at), base, offset)
    }
    fn load_u8(&mut self, at: Access, base: ir::Value, offset: i32) -> ir::Value {
        self.builder.ins().uload8(types::I64, regions::flags(at), base, offset)
    }
    fn load_u16(&mut self, at: Access, base: ir::Value, offset: i32) -> ir::Value {
        self.builder.ins().uload16(types::I64, regions::flags(at), base, offset)
    }
    fn load_table(&mut self, at: Access, address: ir::Value) -> ir::Value {
        self.load_u64(at, address, 0)
    }
    fn add(&mut self, a: ir::Value, b: ir::Value) -> ir::Value { ins!(self, iadd(a, b)) }
    fn or(&mut self, a: ir::Value, b: ir::Value) -> ir::Value { ins!(self, bor(a, b)) }
    fn add_imm(&mut self, a: ir::Value, imm: impl Into<EmitImm>) -> ir::Value {
        let v = self.word(imm.into());
        self.add(a, v)
    }
    fn mul_imm(&mut self, a: ir::Value, imm: impl Into<EmitImm>) -> ir::Value {
        let v = self.word(imm.into());
        ins!(self, imul(a, v))
    }
    fn and_imm(&mut self, a: ir::Value, imm: impl Into<EmitImm>) -> ir::Value {
        let v = self.word(imm.into());
        ins!(self, band(a, v))
    }
    fn or_imm(&mut self, a: ir::Value, imm: impl Into<EmitImm>) -> ir::Value {
        let v = self.word(imm.into());
        self.or(a, v)
    }
    fn shr_imm(&mut self, a: ir::Value, shift: impl Into<EmitImm>) -> ir::Value {
        let v = self.word(shift.into());
        ins!(self, ushr(a, v))
    }
    fn sar32_imm(&mut self, a: ir::Value, shift: u32) -> ir::Value {
        let low = self.narrow(a);
        let shifted = self.builder.ins().sshr_imm_u(low, i64::from(shift));
        self.widen(shifted)
    }
    /// Compared at `a`'s width: a record's 32-bit shape word against a shape id is a 32-bit test.
    fn eq_imm(&mut self, a: ir::Value, imm: impl Into<EmitImm>) -> ir::Value {
        let v = self.word(imm.into());
        self.compare(IntCC::Equal, a, v)
    }
    fn ne_imm(&mut self, a: ir::Value, imm: impl Into<EmitImm>) -> ir::Value {
        let v = self.word(imm.into());
        self.compare(IntCC::NotEqual, a, v)
    }
    fn sub(&mut self, a: ir::Value, b: ir::Value) -> ir::Value { ins!(self, isub(a, b)) }
    fn select(&mut self, cond: ir::Value, then: ir::Value, otherwise: ir::Value) -> ir::Value {
        ins!(self, select(cond, then, otherwise))
    }
    fn store_u32(&mut self, at: Access, base: ir::Value, offset: i32, value: ir::Value) {
        self.builder.ins().istore32(regions::flags(at), value, base, offset);
    }
    fn store_u64(&mut self, at: Access, base: ir::Value, offset: i32, value: ir::Value) {
        self.builder.ins().store(regions::flags(at), value, base, offset);
    }
    /// Whether the store needs no barrier log: the shared tag and mark test. A value the frame
    /// proved immediate never asks (`staged::store`); a stage with no payload answers no, and
    /// the store leaves at its miss.
    fn store_gate(&mut self, record_id: ir::Value, value: ir::Value) -> ir::Value {
        let Some(payload) = self.gate else { return self.ult(record_id, record_id) };
        let (logged, join) = (self.builder.create_block(), self.builder.create_block());
        self.builder.set_cold_block(logged);
        let open = self.builder.append_block_param(join, types::I8);
        super::super::emit_unlogged_property_store_guard(self.builder, payload, record_id, value, logged);
        let yes = ins!(self, iconst(types::I8, 1));
        ins!(self, jump(join, &[ir::BlockArg::Value(yes)]));
        self.builder.switch_to_block(logged);
        let no = ins!(self, iconst(types::I8, 0));
        ins!(self, jump(join, &[ir::BlockArg::Value(no)]));
        self.builder.switch_to_block(join);
        open
    }
    fn number_operator(&mut self, _operator: EmitImm, _passed: ir::Value, _constant: ir::Value) -> (ir::Value, ir::Value) {
        // Native arithmetic for computed initializer fields is the construct lowering's (thread 13): until it lands the field leaves.
        let zero = self.constant(0);
        let never = self.ne_imm(zero, 0u32);
        (zero, never)
    }
    fn assumption(&mut self, _watch: EmitImm) -> ir::Value {
        // No assumption registry reaches emitted code yet: the read leaves at its miss.
        let zero = self.constant(0);
        self.ne_imm(zero, 0u32)
    }
    fn ult(&mut self, a: ir::Value, b: ir::Value) -> ir::Value {
        self.compare(IntCC::UnsignedLessThan, a, b)
    }
    fn uextend32(&mut self, a: ir::Value) -> ir::Value {
        if self.builder.func.dfg.value_type(a) == types::I32 { self.widen(a) } else { a }
    }
    fn ugt_imm(&mut self, a: ir::Value, imm: impl Into<EmitImm>) -> ir::Value {
        let v = self.word(imm.into());
        self.compare(IntCC::UnsignedGreaterThan, a, v)
    }
    fn load_i8(&mut self, at: Access, base: ir::Value, offset: i32) -> ir::Value {
        self.builder.ins().sload8(types::I32, regions::flags(at), base, offset)
    }
    fn load_i16(&mut self, at: Access, base: ir::Value, offset: i32) -> ir::Value {
        self.builder.ins().sload16(types::I32, regions::flags(at), base, offset)
    }
    fn load_i32(&mut self, at: Access, base: ir::Value, offset: i32) -> ir::Value {
        self.builder.ins().load(types::I32, regions::flags(at), base, offset)
    }
    fn load_f32(&mut self, at: Access, base: ir::Value, offset: i32) -> ir::Value {
        self.builder.ins().load(types::F32, regions::flags(at), base, offset)
    }
    fn load_f64(&mut self, at: Access, base: ir::Value, offset: i32) -> ir::Value {
        self.builder.ins().load(types::F64, regions::flags(at), base, offset)
    }
    fn store_u8(&mut self, at: Access, base: ir::Value, offset: i32, value: ir::Value) {
        self.builder.ins().istore8(regions::flags(at), value, base, offset);
    }
    fn store_u16(&mut self, at: Access, base: ir::Value, offset: i32, value: ir::Value) {
        self.builder.ins().istore16(regions::flags(at), value, base, offset);
    }
    fn store_f32(&mut self, at: Access, base: ir::Value, offset: i32, value: ir::Value) {
        self.builder.ins().store(regions::flags(at), value, base, offset);
    }
    fn store_f64(&mut self, at: Access, base: ir::Value, offset: i32, value: ir::Value) {
        self.builder.ins().store(regions::flags(at), value, base, offset);
    }
    fn f32_to_f64(&mut self, a: ir::Value) -> ir::Value { self.builder.ins().fpromote(types::F64, a) }
    fn f64_to_f32(&mut self, a: ir::Value) -> ir::Value { self.builder.ins().fdemote(types::F32, a) }
    /// NaN is zero, the rest clamps and rounds half to even (`nearest`) before the saturating conversion.
    fn clamp_u8(&mut self, a: ir::Value) -> ir::Value {
        let (zero, max) = (self.builder.ins().f64const(f64::from(u8::MIN)), self.builder.ins().f64const(f64::from(u8::MAX)));
        let unordered = self.builder.ins().fcmp(FloatCC::Unordered, a, a);
        let above = self.builder.ins().fmax(a, zero);
        let clamped = self.builder.ins().fmin(above, max);
        let rounded = self.builder.ins().nearest(clamped);
        let number = self.builder.ins().select(unordered, zero, rounded);
        self.builder.ins().fcvt_to_uint_sat(types::I32, number)
    }
    fn not(&mut self, cond: ir::Value) -> ir::Value { self.i32_is_zero(cond) }
    fn any(&mut self, a: ir::Value, b: ir::Value) -> ir::Value { self.or(a, b) }
    fn pick(&mut self, cond: ir::Value, then: ir::Value, otherwise: ir::Value) -> ir::Value {
        self.select(cond, then, otherwise)
    }
    fn shape_in(&mut self, shape: ir::Value, shapes: &[u32]) -> ir::Value {
        let word = if self.builder.func.dfg.value_type(shape) == types::I32 { shape } else { self.narrow(shape) };
        super::shape_in_set(self.builder, word, shapes)
    }
    fn number_truthy(&mut self, bits: ir::Value) -> ir::Value {
        let number = ins!(self, bitcast(types::F64, MemFlagsData::new(), bits));
        super::super::emit_number_truthiness(self.builder, number)
    }
    fn choose(&mut self, cond: ir::Value, then: impl FnOnce(&mut Self) -> ir::Value,
        otherwise: impl FnOnce(&mut Self) -> ir::Value) -> ir::Value {
        self.choose_arms(cond, then, otherwise, false)
    }
    fn guard(&mut self, cond: ir::Value, miss: ir::Block) -> Option<()> {
        emit_js_guard(self.builder, cond, miss);
        Some(())
    }
    fn known(&self, _cond: ir::Value) -> Option<bool> { None }
    fn table(&mut self, table: Table) -> ir::Value {
        match table { Table::PropertyValues => self.property_values.expect("an operation addressing the arena has its base") }
    }
    /// The walk unrolled up to the tier's bound; a longer chain is undecided, for the caller's
    /// residual.
    fn walk_prototypes(&mut self, records: ir::Value, object: ir::Value, target: ir::Value, max_depth: u32)
        -> (ir::Value, ir::Value) {
        let links = max_depth.min(crate::vm::compiled::tier_policy::PROTOTYPE_UNROLL_LINKS);
        crate::staged::object::walk_prototypes(self, records, object, target, links)
    }
}

impl FlowStage for Emit<'_, '_> {
    type Block = ir::Block;
    fn jump(&mut self, block: ir::Block, args: &[ir::Value]) {
        let args: Vec<ir::BlockArg> = args.iter().map(|value| ir::BlockArg::Value(*value)).collect();
        self.builder.ins().jump(block, &args);
    }
    #[cfg(feature = "aot")]
    fn branch(&mut self, cond: ir::Value, then: (ir::Block, &[ir::Value]), otherwise: (ir::Block, &[ir::Value])) {
        let arguments = |values: &[ir::Value]| values.iter().map(|value| ir::BlockArg::Value(*value)).collect::<Vec<_>>();
        self.builder.ins().brif(cond, then.0, &arguments(then.1), otherwise.0, &arguments(otherwise.1));
    }
    #[cfg(feature = "aot")]
    fn ret(&mut self, values: &[ir::Value]) { self.builder.ins().return_(values); }
}

impl Emit<'_, '_> {
    /// `choose` whose `then` arm is out of line: the path that rarely runs (a call to the VM).
    pub(in crate::vm::compiled) fn choose_cold(&mut self, cond: ir::Value, then: impl FnOnce(&mut Self) -> ir::Value,
        otherwise: impl FnOnce(&mut Self) -> ir::Value) -> ir::Value {
        self.choose_arms(cond, then, otherwise, true)
    }

    fn choose_arms(&mut self, cond: ir::Value, then: impl FnOnce(&mut Self) -> ir::Value,
        otherwise: impl FnOnce(&mut Self) -> ir::Value, cold_then: bool) -> ir::Value {
        let (then_block, else_block, join) = (self.builder.create_block(), self.builder.create_block(), self.builder.create_block());
        if cold_then { self.builder.set_cold_block(then_block); }
        self.builder.ins().brif(cond, then_block, &[], else_block, &[]);
        self.builder.switch_to_block(then_block);
        self.builder.seal_block(then_block);
        let taken = then(self);
        // The arms' words share one type: an address or a tagged word, or a Number's `f64`.
        let kind = self.builder.func.dfg.value_type(taken);
        let value = self.builder.append_block_param(join, kind);
        ins!(self, jump(join, &[ir::BlockArg::Value(taken)]));
        self.builder.switch_to_block(else_block);
        self.builder.seal_block(else_block);
        let other = otherwise(self);
        ins!(self, jump(join, &[ir::BlockArg::Value(other)]));
        self.builder.switch_to_block(join);
        self.builder.seal_block(join);
        value
    }
    /// The walk unrolled up to the tier's bound; a longer chain is undecided, for the caller's
    /// residual.
    fn walk_prototypes(&mut self, records: ir::Value, object: ir::Value, target: ir::Value, max_depth: u32)
        -> (ir::Value, ir::Value) {
        let links = max_depth.min(crate::vm::compiled::tier_policy::PROTOTYPE_UNROLL_LINKS);
        crate::staged::object::walk_prototypes(self, records, object, target, links)
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
        ins!(self, fneg(value))
    }
    fn int_arithmetic(&mut self, operation: NumberArithmetic,
        left: ir::Value, right: ir::Value) -> (ir::Value, ir::Value) {
        emit_checked_int32_arithmetic(self.builder, operation, left, right)
    }
    fn float_remainder(&mut self, left: ir::Value, right: ir::Value) -> ir::Value {
        emit_number_remainder(self.builder, types::I64, left, right)
    }
    fn int_remainder(&mut self, left: ir::Value, right: ir::Value) -> ir::Value {
        self.builder.ins().srem(left, right)
    }
    fn float_unary(&mut self, operation: crate::staged::math::FloatUnary, value: ir::Value) -> ir::Value {
        crate::vm::compiled::lane_region::emit_float_unary(self.builder, operation, value)
    }
    fn int_multiply(&mut self, left: ir::Value, right: ir::Value) -> ir::Value {
        crate::vm::compiled::lane_region::emit_int_multiply(self.builder, left, right)
    }
    fn float_extreme(&mut self, maximum: bool, left: ir::Value, right: ir::Value) -> ir::Value {
        crate::vm::compiled::lane_region::emit_float_extreme(self.builder, maximum, left, right)
    }
    fn i32_constant(&mut self, value: i32) -> ir::Value { self.builder.ins().iconst(types::I32, i64::from(value)) }
    fn f64_constant(&mut self, value: f64) -> ir::Value { self.builder.ins().f64const(value) }
    fn narrow(&mut self, a: ir::Value) -> ir::Value { self.builder.ins().ireduce(types::I32, a) }
    /// An int32 word, whether a 32-bit value or a zero-extended 64-bit one.
    fn i32_to_f64(&mut self, a: ir::Value) -> ir::Value {
        let a = if self.builder.func.dfg.value_type(a) == types::I64 { self.narrow(a) } else { a };
        self.builder.ins().fcvt_from_sint(types::F64, a)
    }
    fn u32_to_f64(&mut self, a: ir::Value) -> ir::Value { self.builder.ins().fcvt_from_uint(types::F64, a) }
    fn to_int32(&mut self, number: ir::Value, miss: ir::Block) -> Option<ir::Value> {
        Some(emit_number_to_int32(self.builder, number, miss))
    }
    fn int_bitwise(&mut self, operation: NumberBitwise, left: ir::Value, right: ir::Value) -> ir::Value {
        emit_number_bitwise_bits(self.builder, operation, left, right)
    }
    fn i32_is_zero(&mut self, a: ir::Value) -> ir::Value { self.builder.ins().icmp_imm_u(IntCC::Equal, a, 0) }
    fn i32_is_negative(&mut self, a: ir::Value) -> ir::Value { self.builder.ins().icmp_imm_s(IntCC::SignedLessThan, a, 0) }
    fn both(&mut self, a: ir::Value, b: ir::Value) -> ir::Value { self.builder.ins().band(a, b) }
    fn either(&mut self, a: ir::Value, b: ir::Value) -> ir::Value { self.or(a, b) }
    fn cond_word(&mut self, cond: ir::Value) -> ir::Value { cond }
    fn guard_false(&mut self, cond: ir::Value, miss: ir::Block) -> Option<()> {
        let passed = self.builder.create_block();
        // The forced-deopt switch counts this guard too: both successors leave.
        #[cfg(test)]
        let passed_or_miss = if crate::vm::compiled::forced_miss::next() { miss } else { passed };
        #[cfg(not(test))]
        let passed_or_miss = passed;
        ins!(self, brif(cond, miss, &[], passed_or_miss, &[]));
        self.builder.switch_to_block(passed);
        Some(())
    }
}

#[cfg(all(test, feature = "jit"))]
mod tests {
    use super::*;
    use crate::heap::{Cell, Heap, Object, PlainPropertyStorage};
    use crate::staged::object::{own_layout, read_slot, Form, PackedAccess};
    use crate::staged::Region;

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
        let mut stage = Emit { builder: &mut builder, property_values: Some(base), gate: None };
        let inline = crate::staged::object::inline_storage(&mut stage, record);
        let layout = own_layout(&mut stage, record, inline, form,
            usize::from(slot) + 1, PackedAccess::Reload, miss).unwrap();
        let value = read_slot(&mut stage, layout, slot);
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
        let mut stage = Emit { builder: &mut builder, property_values: None, gate: None };
        let flat = crate::staged::strings::flat_string(&mut stage, records, string, miss).unwrap();
        let code = crate::staged::strings::string_char_code_at(&mut stage, flat, index, miss).unwrap();
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
        use crate::staged::{Concrete, strings};
        use crate::vm::wtf16::{JsString, SHORT_ASCII_CAPACITY};
        let isa = super::super::super::super::jit_backend::target_isa().unwrap();
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
                let mut stage = Concrete::new(std::ptr::null());
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
        let word = Emit { builder: &mut builder, property_values: None, gate: None }.load_u32(at, base, 8);
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
        let isa = super::super::super::super::jit_backend::target_isa().unwrap();
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
        let isa = super::super::super::super::jit_backend::target_isa().unwrap();
        let function = word_summed_in_a_loop(&*isa, Region::ObjectSlots.mutable());
        let [(opcode, flags)] = accesses(&function)[..] else { panic!("one access") };
        assert_eq!(opcode, ir::Opcode::Uload32);
        assert!(!flags.readonly() && !flags.can_move() && flags.notrap());
        assert_eq!(flags.alias_region(), regions::flags(Region::ObjectSlots.mutable()).alias_region());
    }

    /// The barrier-log path of a store's gate is a guard failure: the only cold block the
    /// gate adds.
    #[test]
    fn the_barrier_log_path_of_a_store_gate_is_cold() {
        let isa = super::super::super::super::jit_backend::target_isa().unwrap();
        let mut signature = Signature::new(isa.default_call_conv());
        signature.params.extend([AbiParam::new(types::I64); 3]);
        signature.returns.push(AbiParam::new(types::I64));
        let mut function = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
        regions::declare_regions(&mut function);
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut function, &mut context);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        let [payload, record, value] = builder.block_params(entry).try_into().unwrap();
        let open = Emit { builder: &mut builder, property_values: None, gate: Some(payload) }.store_gate(record, value);
        let open = builder.ins().uextend(types::I64, open);
        builder.ins().return_(&[open]);
        builder.seal_all_blocks();
        builder.finalize(isa.frontend_config());
        cranelift_codegen::verify_function(&function, &*isa).unwrap();
        assert_eq!(function.layout.blocks().filter(|block| function.layout.is_cold(*block)).count(), 1);
    }
    /// A loop that reads the object record base in its body, with or without a helper call
    /// beside the read, optimized by Cranelift; the base loads it keeps, in the loop and out.
    fn record_base_loads_after_optimizing(isa: &dyn cranelift_codegen::isa::TargetIsa, with_call: bool) -> (usize, usize) {
        let mut signature = Signature::new(isa.default_call_conv());
        signature.params.extend([AbiParam::new(types::I64); 2]);
        signature.returns.push(AbiParam::new(types::I64));
        let mut function = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature.clone());
        regions::declare_regions(&mut function);
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut function, &mut context);
        let helper = builder.import_signature(signature);
        let (entry, header, body, exit) =
            (builder.create_block(), builder.create_block(), builder.create_block(), builder.create_block());
        builder.append_block_params_for_function_params(entry);
        let (sum, index) = (builder.append_block_param(header, types::I64), builder.append_block_param(header, types::I64));
        let total = builder.append_block_param(exit, types::I64);
        builder.switch_to_block(entry);
        let [payload, count] = builder.block_params(entry).try_into().unwrap();
        let zero = builder.ins().iconst(types::I64, 0);
        builder.ins().jump(header, &[zero.into(), zero.into()]);
        builder.switch_to_block(header);
        let more = builder.ins().icmp(IntCC::UnsignedLessThan, index, count);
        builder.ins().brif(more, body, &[], exit, &[sum.into()]);
        builder.switch_to_block(body);
        let base = super::super::emit_js_record_base(&mut builder, payload, std::mem::offset_of!(super::super::JsStepDispatch, object_record_base));
        let record = builder.ins().iadd(base, index);
        let field = Emit { builder: &mut builder, property_values: None, gate: None }.load_u64(Region::ObjectSlots.mutable(), record, 8);
        let sum_next = builder.ins().iadd(sum, field);
        if with_call {
            let target = builder.ins().iconst(types::I64, 8);
            builder.ins().call_indirect(helper, target, &[payload, index]);
        }
        let index_next = builder.ins().iadd_imm_u(index, 1);
        builder.ins().jump(header, &[sum_next.into(), index_next.into()]);
        builder.switch_to_block(exit);
        builder.ins().return_(&[total]);
        builder.seal_all_blocks();
        builder.finalize(isa.frontend_config());
        let mut compiling = cranelift_codegen::Context::for_function(function);
        compiling.optimize(isa, &mut cranelift_codegen::control::ControlPlane::default()).unwrap();
        compiling.flowgraph();
        compiling.compute_loop_analysis();
        let record_base = regions::record_base().alias_region();
        let (mut inside, mut outside) = (0, 0);
        for block in compiling.func.layout.blocks() {
            for inst in compiling.func.layout.block_insts(block) {
                let flags = compiling.func.dfg.insts[inst].memflags_data(&compiling.func.dfg);
                if flags.is_some_and(|flags| flags.alias_region() == record_base) {
                    if compiling.loop_analysis.innermost_loop(block).is_some() { inside += 1 } else { outside += 1 }
                }
            }
        }
        (inside, outside)
    }

    /// The base of a record reservation is read once, before the loop, whether or not the
    /// loop calls a helper: a reservation does not move while a run executes, so there is
    /// no reload after the call.
    #[test]
    fn a_record_base_is_read_once_outside_a_loop_with_or_without_a_call() {
        let isa = super::super::super::super::jit_backend::target_isa().unwrap();
        for with_call in [false, true] {
            assert_eq!(record_base_loads_after_optimizing(&*isa, with_call), (0, 1), "call: {with_call}");
        }
    }
}
