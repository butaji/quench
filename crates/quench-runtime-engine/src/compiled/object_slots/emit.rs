//! The specializer's Cranelift stage: `quench_runtime::staged` operations emitted as IR.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::staged::region::Mutability;
use quench_runtime::staged::{Access, Domain, NumericStage, Stage, Table};
use quench_runtime::staged::flow::FlowStage;
use quench_runtime::staged::math::FloatUnary;
use quench_runtime::staged::number_machine as numbers;
use quench_runtime::staged::machine::{Binary, Extend, Lane, MachineAccess, MachineFlow, MachineScalar, Relation, Unary};
use crate::compiled::regions;
use quench_runtime::vm::operations::{NumberArithmetic, NumberBitwise, NumberRelation};

/// The specializer's stage (`quench_runtime::staged`): each operation emits its residual as Cranelift
/// IR into `builder`, where a guard's failure jumps to its miss block.
pub(crate) struct Emit<'a, 'b> {
    pub(crate) builder: &'a mut FunctionBuilder<'b>,
    /// The property arena's value base (`Table::PropertyValues`), where the operation
    /// addresses a run; a read through a proved layout already holds its address.
    pub(crate) property_values: Option<ir::Value>,
    /// The dispatch payload that holds the slot-observation flag and the object marks the store
    /// barrier's test reads, where the operation stores (`Stage::store_gate`).
    pub(crate) gate: Option<ir::Value>,
    /// The dispatch payload, where the operation may address the arena with no base given
    /// (`property_values` is `None`): `Table::PropertyValues` then reads the payload's property
    /// buffer where it is used, after any call the operation made before it.
    pub(crate) payload: Option<ir::Value>,
}

/// An immediate of the Cranelift stage: a constant, or a value the caller holds as one (a stencil's
/// patch hole, which is an operand until the patcher fills it).
#[derive(Clone, Copy)]
pub(crate) enum EmitImm {
    Const(u64),
}

impl From<u64> for EmitImm { fn from(bits: u64) -> Self { Self::Const(bits) } }
impl From<u32> for EmitImm { fn from(bits: u32) -> Self { Self::Const(u64::from(bits)) } }

impl Emit<'_, '_> {
    /// Whether the dispatch `payload`'s gate word has its counts bit clear (`STORE_WATCH_COUNTS`):
    /// old records keep no reference counts, so an immediate store needs no barrier log.
    pub(crate) fn counts_clear(&mut self, payload: ir::Value) -> ir::Value {
        let watch = super::super::emit_js_payload_word(self.builder, payload, std::mem::offset_of!(super::super::JsStepDispatch, head.store_watch));
        let counts = self.and_imm(watch, quench_runtime::staged::layout_offsets::STORE_WATCH_COUNTS);
        self.raw_compare_imm(Relation::Equal, counts, 0)
    }

    /// `imm` as an operand: Cranelift folds a constant operand back into the instruction's immediate.
    fn word(&mut self, imm: EmitImm) -> ir::Value {
        match imm { EmitImm::Const(bits) => self.constant(bits) }
    }
    fn compare(&mut self, relation: Relation, a: ir::Value, b: ir::Value) -> ir::Value { self.raw_compare(relation, a, b) }
    /// `a` against an immediate at `a`'s own width (`icmp_imm`): a record's 32-bit shape word
    /// against a shape id is a 32-bit test, where an `i64` constant would not verify.
    fn compare_imm(&mut self, relation: Relation, a: ir::Value, imm: EmitImm) -> ir::Value {
        match imm { EmitImm::Const(bits) => self.raw_compare_imm(relation, a, bits as i64) }
    }
    fn widen(&mut self, a: ir::Value) -> ir::Value { self.raw_unary(Unary::ZeroExtend(Lane::I64), a) }
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

    fn constant(&mut self, bits: u64) -> ir::Value { self.raw_const(Lane::I64, bits as i64) }
    /// A read-only word is a plain `load` and a `uextend`: the egraph merges and hoists only
    /// plain loads, and the extend folds into the target's zero-extending load.
    fn load_u32(&mut self, at: Access, base: ir::Value, offset: i32) -> ir::Value {
        match at.mutability {
            Mutability::Readonly => {
                let word = self.raw_load(Lane::I32, regions::flags(at), base, offset);
                self.widen(word)
            }
            Mutability::Mutable => self.raw_load_extended(Lane::I32, Extend::Zero, Lane::I64, regions::flags(at), base, offset),
        }
    }
    fn load_u64(&mut self, at: Access, base: ir::Value, offset: i32) -> ir::Value {
        self.raw_load(Lane::I64, regions::flags(at), base, offset)
    }
    fn load_u8(&mut self, at: Access, base: ir::Value, offset: i32) -> ir::Value {
        self.raw_load_extended(Lane::I8, Extend::Zero, Lane::I64, regions::flags(at), base, offset)
    }
    fn load_u16(&mut self, at: Access, base: ir::Value, offset: i32) -> ir::Value {
        self.raw_load_extended(Lane::I16, Extend::Zero, Lane::I64, regions::flags(at), base, offset)
    }
    fn load_table(&mut self, at: Access, address: ir::Value) -> ir::Value {
        self.load_u64(at, address, 0)
    }
    fn add(&mut self, a: ir::Value, b: ir::Value) -> ir::Value { self.raw_binary(Binary::Add, a, b) }
    fn or(&mut self, a: ir::Value, b: ir::Value) -> ir::Value { self.raw_binary(Binary::Or, a, b) }
    fn and(&mut self, a: ir::Value, b: ir::Value) -> ir::Value { self.raw_binary(Binary::And, a, b) }
    fn shr(&mut self, a: ir::Value, b: ir::Value) -> ir::Value { self.raw_binary(Binary::ShiftRightUnsigned, a, b) }
    fn add_imm(&mut self, a: ir::Value, imm: impl Into<EmitImm>) -> ir::Value {
        let v = self.word(imm.into());
        self.add(a, v)
    }
    fn mul_imm(&mut self, a: ir::Value, imm: impl Into<EmitImm>) -> ir::Value {
        let v = self.word(imm.into());
        self.raw_binary(Binary::Mul, a, v)
    }
    fn and_imm(&mut self, a: ir::Value, imm: impl Into<EmitImm>) -> ir::Value {
        let v = self.word(imm.into());
        self.raw_binary(Binary::And, a, v)
    }
    fn or_imm(&mut self, a: ir::Value, imm: impl Into<EmitImm>) -> ir::Value {
        let v = self.word(imm.into());
        self.or(a, v)
    }
    fn shr_imm(&mut self, a: ir::Value, shift: impl Into<EmitImm>) -> ir::Value {
        let v = self.word(shift.into());
        self.raw_binary(Binary::ShiftRightUnsigned, a, v)
    }
    fn sar32_imm(&mut self, a: ir::Value, shift: u32) -> ir::Value {
        let low = self.narrow(a);
        let by = self.raw_const(Lane::I32, i64::from(shift));
        let shifted = self.raw_binary(Binary::ShiftRightSigned, low, by);
        self.widen(shifted)
    }
    /// Compared at `a`'s width: a record's 32-bit shape word against a shape id is a 32-bit test.
    fn eq_imm(&mut self, a: ir::Value, imm: impl Into<EmitImm>) -> ir::Value {
        self.compare_imm(Relation::Equal, a, imm.into())
    }
    fn ne_imm(&mut self, a: ir::Value, imm: impl Into<EmitImm>) -> ir::Value {
        self.compare_imm(Relation::NotEqual, a, imm.into())
    }
    fn sub(&mut self, a: ir::Value, b: ir::Value) -> ir::Value { self.raw_binary(Binary::Sub, a, b) }
    /// A choice between a double and a word of an integer lane reads the word as its bits, as the
    /// interpreter's untyped words are (`int_case`: an int's double or the double a tagged word is).
    fn select(&mut self, cond: ir::Value, then: ir::Value, otherwise: ir::Value) -> ir::Value {
        let (then, otherwise) = match (self.raw_lane(then), self.raw_lane(otherwise)) {
            (Lane::F64, Lane::I64) => (then, self.f64_from_bits(otherwise)),
            (Lane::I64, Lane::F64) => (self.f64_from_bits(then), otherwise),
            _ => (then, otherwise),
        };
        self.raw_select(cond, then, otherwise)
    }
    fn store_u32(&mut self, at: Access, base: ir::Value, offset: i32, value: ir::Value) {
        self.raw_store(Lane::I32, regions::flags(at), (value, false), base, offset);
    }
    fn store_u64(&mut self, at: Access, base: ir::Value, offset: i32, value: ir::Value) {
        self.raw_store(Lane::I64, regions::flags(at), (value, false), base, offset);
    }
    fn store_private_u32(&mut self, at: Access, base: ir::Value, offset: i32, value: ir::Value) {
        self.raw_store(Lane::I32, regions::flags(at), (value, true), base, offset);
    }
    fn store_private_u64(&mut self, at: Access, base: ir::Value, offset: i32, value: ir::Value) {
        self.raw_store(Lane::I64, regions::flags(at), (value, true), base, offset);
    }
    /// Whether the store needs no barrier log: the shared tag and mark test. A value the frame
    /// proved immediate never asks (`staged::store`); a stage with no payload answers no, and
    /// the store leaves at its miss.
    fn store_gate(&mut self, space: quench_runtime::staged::StagedRecordSpace, record_id: ir::Value, value: ir::Value) -> ir::Value {
        let Some(payload) = self.gate else { return self.ult(record_id, record_id) };
        let (logged, join) = (self.builder.create_block(), self.builder.create_block());
        self.builder.set_cold_block(logged);
        let open = self.builder.append_block_param(join, types::I8);
        let tracked = super::super::emit_js_payload_word(self.builder, payload, std::mem::offset_of!(super::super::JsStepDispatch, head.store_watch));
        let untracked = self.raw_compare_imm(Relation::Equal, tracked, 0);
        super::super::emit_js_guard(self.builder, untracked, logged);
        super::super::emit_unlogged_property_store_guard(self, super::super::store_mark_list(space), payload, record_id, value, logged);
        let yes = self.raw_const(Lane::I8, 1);
        self.goto(join, &[yes]);
        self.builder.switch_to_block(logged);
        let no = self.raw_const(Lane::I8, 0);
        self.goto(join, &[no]);
        self.builder.switch_to_block(join);
        open
    }
    /// Whether an immediate store needs no barrier log: the gate word's counts bit is clear. A stage
    /// with no payload compiles under whether any heap counts (`heap::rc::COUNTS_SEEN`): open
    /// without counts, else closed, and the store leaves at its miss.
    fn immediate_store_open(&mut self) -> ir::Value {
        let Some(payload) = self.gate else {
            let seen = quench_runtime::heap::rc::COUNTS_SEEN.load(core::sync::atomic::Ordering::Relaxed);
            let counted = self.raw_const(Lane::I8, i64::from(seen));
            return self.raw_compare_imm(Relation::Equal, counted, 0);
        };
        self.counts_clear(payload)
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
        self.compare(Relation::UnsignedLess, a, b)
    }
    fn eq(&mut self, a: ir::Value, b: ir::Value) -> ir::Value {
        self.compare(Relation::Equal, a, b)
    }
    /// A do-while in a header block whose parameters are the carried words. The body is emitted into the
    /// header: its `go_on` branches back to the header with the next words, or on to the exit block, whose
    /// parameters are the final words. The header is sealed after the back edge.
    fn loop_words<const N: usize>(&mut self, init: [ir::Value; N],
        mut body: impl FnMut(&mut Self, [ir::Value; N]) -> (ir::Value, [ir::Value; N])) -> [ir::Value; N] {
        let kinds = init.map(|word| self.builder.func.dfg.value_type(word));
        let (header, exit) = (self.builder.create_block(), self.builder.create_block());
        let carried: [ir::Value; N] = std::array::from_fn(|index| self.builder.append_block_param(header, kinds[index]));
        let finals: [ir::Value; N] = std::array::from_fn(|index| self.builder.append_block_param(exit, kinds[index]));
        self.goto(header, &init[..]);
        self.builder.switch_to_block(header);
        let (go_on, next) = body(self, carried);
        self.branch_if(go_on, (header, &next[..]), (exit, &next[..]));
        self.builder.seal_block(header);
        self.builder.switch_to_block(exit);
        self.builder.seal_block(exit);
        finals
    }
    fn uextend32(&mut self, a: ir::Value) -> ir::Value {
        if self.raw_lane(a) == Lane::I32 { self.widen(a) } else { a }
    }
    fn ugt_imm(&mut self, a: ir::Value, imm: impl Into<EmitImm>) -> ir::Value {
        self.compare_imm(Relation::UnsignedGreater, a, imm.into())
    }
    fn load_i8(&mut self, at: Access, base: ir::Value, offset: i32) -> ir::Value {
        self.raw_load_extended(Lane::I8, Extend::Sign, Lane::I32, regions::flags(at), base, offset)
    }
    fn load_i16(&mut self, at: Access, base: ir::Value, offset: i32) -> ir::Value {
        self.raw_load_extended(Lane::I16, Extend::Sign, Lane::I32, regions::flags(at), base, offset)
    }
    fn load_i32(&mut self, at: Access, base: ir::Value, offset: i32) -> ir::Value {
        self.raw_load(Lane::I32, regions::flags(at), base, offset)
    }
    fn load_f32(&mut self, at: Access, base: ir::Value, offset: i32) -> ir::Value {
        self.raw_load(Lane::F32, regions::flags(at), base, offset)
    }
    fn load_f64(&mut self, at: Access, base: ir::Value, offset: i32) -> ir::Value {
        self.raw_load(Lane::F64, regions::flags(at), base, offset)
    }
    fn box_number(&mut self, number: ir::Value) -> ir::Value {
        super::super::emit_js_box_number(self.builder, number)
    }
    fn store_u8(&mut self, at: Access, base: ir::Value, offset: i32, value: ir::Value) {
        self.raw_store(Lane::I8, regions::flags(at), (value, false), base, offset);
    }
    fn store_u16(&mut self, at: Access, base: ir::Value, offset: i32, value: ir::Value) {
        self.raw_store(Lane::I16, regions::flags(at), (value, false), base, offset);
    }
    fn store_f32(&mut self, at: Access, base: ir::Value, offset: i32, value: ir::Value) {
        self.raw_store(Lane::F32, regions::flags(at), (value, false), base, offset);
    }
    fn store_f64(&mut self, at: Access, base: ir::Value, offset: i32, value: ir::Value) {
        self.raw_store(Lane::F64, regions::flags(at), (value, false), base, offset);
    }
    fn f32_to_f64(&mut self, a: ir::Value) -> ir::Value { self.raw_unary(Unary::Promote, a) }
    fn f64_to_f32(&mut self, a: ir::Value) -> ir::Value { self.raw_unary(Unary::Demote, a) }
    /// NaN is zero, the rest clamps and rounds half to even (`nearest`) before the saturating conversion.
    fn clamp_u8(&mut self, a: ir::Value) -> ir::Value {
        let (zero, max) = (self.raw_const(Lane::F64, f64::from(u8::MIN).to_bits() as i64), self.raw_const(Lane::F64, f64::from(u8::MAX).to_bits() as i64));
        let unordered = self.raw_compare(Relation::FloatUnordered, a, a);
        let above = self.raw_binary(Binary::FloatMax, a, zero);
        let clamped = self.raw_binary(Binary::FloatMin, above, max);
        let rounded = self.raw_unary(Unary::Nearest, clamped);
        let number = self.raw_select(unordered, zero, rounded);
        self.raw_unary(Unary::ToUnsignedSaturating(Lane::I32), number)
    }
    fn not(&mut self, cond: ir::Value) -> ir::Value { self.i32_is_zero(cond) }
    fn any(&mut self, a: ir::Value, b: ir::Value) -> ir::Value { self.or(a, b) }
    fn all(&mut self, a: ir::Value, b: ir::Value) -> ir::Value { self.raw_binary(Binary::And, a, b) }
    fn pick(&mut self, cond: ir::Value, then: ir::Value, otherwise: ir::Value) -> ir::Value {
        self.raw_select(cond, then, otherwise)
    }
    fn shape_in(&mut self, shape: ir::Value, shapes: &[u32]) -> ir::Value {
        let word = if self.raw_lane(shape) == Lane::I32 { shape } else { self.narrow(shape) };
        super::shape_in_set(self.builder, word, shapes)
    }
    fn number_truthy(&mut self, bits: ir::Value) -> ir::Value {
        let number = self.raw_unary(Unary::Bitcast(Lane::F64), bits);
        numbers::number_truthy(self, number)
    }
    fn choose(&mut self, cond: ir::Value, then: impl FnOnce(&mut Self) -> ir::Value,
        otherwise: impl FnOnce(&mut Self) -> ir::Value) -> ir::Value {
        self.choose_arms(cond, then, otherwise, false)
    }
    fn guard_status(&mut self, cond: ir::Value, miss: ir::Block) -> Option<()> {
        let passed = self.builder.create_block();
        self.branch_if(cond, (passed, &[]), (miss, &[]));
        self.builder.switch_to_block(passed);
        self.builder.seal_block(passed);
        Some(())
    }
    fn guard(&mut self, cond: ir::Value, miss: ir::Block) -> Option<()> {
        emit_js_guard(self.builder, cond, miss);
        Some(())
    }
    fn known(&self, _cond: ir::Value) -> Option<bool> { None }
    fn table(&mut self, table: Table) -> ir::Value {
        match table {
            Table::PropertyValues => match (self.property_values, self.payload) {
                (Some(base), _) => base,
                (None, Some(payload)) => super::super::emit_js_property_values(self.builder, payload).0,
                (None, None) => panic!("an operation addressing the arena has its base"),
            },
            // The table is a static that never moves; in-process code embeds its address.
            Table::Layouts => self.builder.ins().iconst(types::I64, quench_runtime::heap::layouts::table_base() as i64),
        }
    }
    /// The walk unrolled up to the tier's bound; a longer chain is undecided, for the caller's
    /// residual.
    fn walk_prototypes(&mut self, records: ir::Value, object: ir::Value, target: ir::Value, max_depth: u32)
        -> (ir::Value, ir::Value) {
        let links = max_depth.min(crate::compiled::tier_policy::PROTOTYPE_UNROLL_LINKS);
        quench_runtime::staged::object::walk_prototypes(self, records, object, target, links)
    }
}

impl FlowStage for Emit<'_, '_> {
    type Block = ir::Block;
    type Signature = ir::SigRef;
    fn jump(&mut self, block: ir::Block, args: &[ir::Value]) { self.goto(block, args); }
    fn branch(&mut self, cond: ir::Value, then: (ir::Block, &[ir::Value]), otherwise: (ir::Block, &[ir::Value])) {
        self.branch_if(cond, then, otherwise);
    }
    fn call_entry(&mut self, signature: ir::SigRef, entry: ir::Value, args: &[ir::Value]) -> (ir::Value, ir::Value) {
        let results = self.call_signature(signature, entry, args);
        (results[0], results[1])
    }
    fn ret(&mut self, values: &[ir::Value]) { self.leave(values); }
}

impl Emit<'_, '_> {
    /// `choose` whose `then` arm is out of line: the path that rarely runs (a call to the VM).
    pub(crate) fn choose_cold(&mut self, cond: ir::Value, then: impl FnOnce(&mut Self) -> ir::Value,
        otherwise: impl FnOnce(&mut Self) -> ir::Value) -> ir::Value {
        self.choose_arms(cond, then, otherwise, true)
    }

    fn choose_arms(&mut self, cond: ir::Value, then: impl FnOnce(&mut Self) -> ir::Value,
        otherwise: impl FnOnce(&mut Self) -> ir::Value, cold_then: bool) -> ir::Value {
        let (then_block, else_block, join) = (self.builder.create_block(), self.builder.create_block(), self.builder.create_block());
        if cold_then { self.builder.set_cold_block(then_block); }
        self.branch_if(cond, (then_block, &[]), (else_block, &[]));
        self.builder.switch_to_block(then_block);
        self.builder.seal_block(then_block);
        let taken = then(self);
        // The arms' words share one type: an address or a tagged word, or a Number's `f64`.
        let kind = self.builder.func.dfg.value_type(taken);
        let value = self.builder.append_block_param(join, kind);
        self.goto(join, &[taken]);
        self.builder.switch_to_block(else_block);
        self.builder.seal_block(else_block);
        let other = otherwise(self);
        self.goto(join, &[other]);
        self.builder.switch_to_block(join);
        self.builder.seal_block(join);
        value
    }
}

impl Emit<'_, '_> {
    /// The i64 word of the binary64 `value` and whether it is an integer of magnitude at most 2^53 (`value` converted
    /// and converted back is itself, and the saturated conversion stays inside that range; NaN fails both).
    fn integral_below_2_53(&mut self, value: ir::Value) -> (ir::Value, ir::Value) {
        let integer = self.raw_unary(Unary::ToSignedSaturating(Lane::I64), value);
        let back = self.raw_unary(Unary::FromSigned(Lane::F64), integer);
        let exact = self.raw_compare(Relation::FloatEqual, back, value);
        let bias = self.raw_const(Lane::I64, 1 << 53);
        let shifted = self.raw_binary(Binary::Add, integer, bias);
        let width = self.raw_const(Lane::I64, 1 << 54);
        let small = self.raw_compare(Relation::UnsignedLessEqual, shifted, width);
        (integer, self.raw_binary(Binary::And, exact, small))
    }
}

impl NumericStage for Emit<'_, '_> {
    fn relation(&mut self, relation: NumberRelation, domain: Domain,
        left: ir::Value, right: ir::Value) -> ir::Value {
        numbers::number_relation(self, relation, left, right, domain)
    }
    fn float_arithmetic(&mut self, operation: NumberArithmetic,
        left: ir::Value, right: ir::Value) -> ir::Value {
        numbers::float_arithmetic(self, operation, left, right)
    }
    fn float_negate(&mut self, value: ir::Value) -> ir::Value {
        self.raw_unary(Unary::FloatNegate, value)
    }
    /// IEEE equality of two binary64 words: a word of an integer lane is read as its bits.
    fn float_eq(&mut self, left: ir::Value, right: ir::Value) -> ir::Value {
        let left = if self.raw_lane(left) == Lane::F64 { left } else { self.f64_from_bits(left) };
        let right = if self.raw_lane(right) == Lane::F64 { right } else { self.f64_from_bits(right) };
        self.raw_compare(Relation::FloatEqual, left, right)
    }
    fn int_arithmetic(&mut self, operation: NumberArithmetic,
        left: ir::Value, right: ir::Value) -> (ir::Value, ir::Value) {
        numbers::checked_int32_arithmetic(self, operation, left, right)
    }
    /// The remainder of two binary64 words: where both are integers below 2^53 and the divisor is nonzero, the i64
    /// remainder (exact, and the dividend's sign), copied onto the dividend's sign for a zero result (`-0`); else
    /// the shared leaf (`numbers::float_remainder`).
    fn float_remainder(&mut self, left: ir::Value, right: ir::Value) -> ir::Value {
        let (dividend, dividend_ok) = self.integral_below_2_53(left);
        let (divisor, divisor_ok) = self.integral_below_2_53(right);
        let nonzero = self.raw_compare_imm(Relation::NotEqual, divisor, 0);
        let both = self.raw_binary(Binary::And, dividend_ok, divisor_ok);
        let integral = self.raw_binary(Binary::And, both, nonzero);
        self.choose_arms(integral, |stage| {
            let remainder = stage.raw_binary(Binary::RemainderSigned, dividend, divisor);
            let magnitude = stage.raw_unary(Unary::FromSigned(Lane::F64), remainder);
            stage.raw_binary(Binary::FloatCopySign, magnitude, left)
        }, |stage| numbers::float_remainder(stage, left, right), false)
    }
    fn integral_remainder(&mut self, left: ir::Value, right: ir::Value) -> (ir::Value, ir::Value) {
        // The u-jit makes the float remainder for every operand (a leaf call it can hold), so its miss is never taken.
        let value = numbers::float_remainder(self, left, right);
        (value, self.i32_constant(0))
    }
    fn int_remainder(&mut self, left: ir::Value, right: ir::Value) -> ir::Value {
        self.raw_binary(Binary::RemainderSigned, left, right)
    }
    fn float_unary(&mut self, operation: FloatUnary, value: ir::Value) -> ir::Value {
        match operation {
            FloatUnary::Floor => self.raw_unary(Unary::Floor, value),
            FloatUnary::Ceil => self.raw_unary(Unary::Ceiling, value),
            FloatUnary::Trunc => self.raw_unary(Unary::Truncate, value),
            FloatUnary::Abs => self.raw_unary(Unary::FloatAbsolute, value),
            FloatUnary::Sqrt => self.raw_unary(Unary::SquareRoot, value),
            FloatUnary::Fround => {
                let single = self.raw_unary(Unary::Demote, value);
                self.raw_unary(Unary::Promote, single)
            }
            FloatUnary::Round => quench_runtime::staged::math::round(self, value),
            FloatUnary::Sign => quench_runtime::staged::math::sign(self, value),
        }
    }
    fn int_multiply(&mut self, left: ir::Value, right: ir::Value) -> ir::Value {
        self.raw_binary(Binary::Mul, left, right)
    }
    /// A 64-bit word's low 32 bits are the int32; the count is over those alone.
    fn int_leading_zeros(&mut self, a: ir::Value) -> ir::Value {
        let a = if self.raw_lane(a) == Lane::I64 { self.narrow(a) } else { a };
        self.raw_unary(Unary::LeadingZeros, a)
    }
    fn float_extreme(&mut self, maximum: bool, left: ir::Value, right: ir::Value) -> ir::Value {
        self.raw_binary(if maximum { Binary::FloatMax } else { Binary::FloatMin }, left, right)
    }
    fn i32_constant(&mut self, value: i32) -> ir::Value { self.raw_const(Lane::I32, i64::from(value)) }
    fn f64_constant(&mut self, value: f64) -> ir::Value { self.raw_const(Lane::F64, value.to_bits() as i64) }
    fn f64_from_bits(&mut self, bits: ir::Value) -> ir::Value { self.raw_unary(Unary::Bitcast(Lane::F64), bits) }
    fn narrow(&mut self, a: ir::Value) -> ir::Value { self.raw_unary(Unary::Narrow(Lane::I32), a) }
    /// An int32 word, whether a 32-bit value or a zero-extended 64-bit one.
    fn i32_to_f64(&mut self, a: ir::Value) -> ir::Value {
        let a = if self.raw_lane(a) == Lane::I64 { self.narrow(a) } else { a };
        self.raw_unary(Unary::FromSigned(Lane::F64), a)
    }
    fn u32_to_f64(&mut self, a: ir::Value) -> ir::Value { self.raw_unary(Unary::FromUnsigned(Lane::F64), a) }
    fn to_int32(&mut self, number: ir::Value, miss: ir::Block) -> Option<ir::Value> {
        Some(emit_number_to_int32(self.builder, number, miss))
    }
    fn int_bitwise(&mut self, operation: NumberBitwise, left: ir::Value, right: ir::Value) -> ir::Value {
        numbers::int32_bitwise(self, operation, left, right)
    }
    fn i32_is_zero(&mut self, a: ir::Value) -> ir::Value { self.raw_compare_imm(Relation::Equal, a, 0) }
    fn i32_is_negative(&mut self, a: ir::Value) -> ir::Value { self.raw_compare_imm(Relation::SignedLess, a, 0) }
    fn both(&mut self, a: ir::Value, b: ir::Value) -> ir::Value { self.raw_binary(Binary::And, a, b) }
    fn either(&mut self, a: ir::Value, b: ir::Value) -> ir::Value { self.or(a, b) }
    fn cond_word(&mut self, cond: ir::Value) -> ir::Value { cond }
    fn guard_false(&mut self, cond: ir::Value, miss: ir::Block) -> Option<()> {
        let passed = self.builder.create_block();
        // The forced-deopt switch counts this guard too: both successors leave.
        #[cfg(test)]
        let passed_or_miss = if crate::compiled::forced_miss::next() { miss } else { passed };
        #[cfg(not(test))]
        let passed_or_miss = passed;
        self.branch_if(cond, (miss, &[]), (passed_or_miss, &[]));
        self.builder.switch_to_block(passed);
        Some(())
    }
}

#[cfg(all(test, feature = "jit"))]
pub(crate) mod tests;
