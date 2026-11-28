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
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

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
    /// The facts of the words this step read from the frame (R2, facts on values): each register's
    /// type at this point, which the frame noted when it read the word (`EmitFrame::tagged`).
    /// A row's class test the fact decides builds no code (`Stage::fact`, `value::decided`).
    pub(crate) facts: smallvec::SmallVec<[(ir::Value, quench_runtime::typed_ir::Type); 4]>,
    /// Whether the holder this step stores into is fresh (allocated in this unit, with no safepoint since, `LocalProofs::fresh`):
    /// its store barrier's log is elided, and only the observation gate remains (`store_gate`, `held_store_gate`).
    pub(crate) fresh: bool,
}

impl Emit<'_, '_> {
    /// Notes `fact` of `word` for the rest of this step (`facts`); a later note of the same word keeps both.
    #[inline(always)]
    pub(crate) fn note_fact(&mut self, word: ir::Value, fact: quench_runtime::typed_ir::Type) {
        if fact != quench_runtime::typed_ir::Type::ANY { self.facts.push((word, fact)); }
    }
}

/// An immediate of the Cranelift stage: a constant, or a value the caller holds as one (a stencil's
/// patch hole, which is an operand until the patcher fills it).
#[derive(Clone, Copy)]
pub(crate) enum EmitImm {
    Const(u64),
    /// Shape recipe `key` of an ahead image (`ic::SYMBOLIC_SHAPE`): the symbol its loader resolves
    /// to the shape's id in the VM it links to, or to an id no record has where it builds none.
    Shape(u32),
    /// A word an ahead image's loader resolves for a holder guard key (`staged::ImageWord`).
    Image(quench_runtime::staged::ImageWord, u32),
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
        match imm {
            EmitImm::Const(bits) => self.constant(bits),
            EmitImm::Shape(key) => crate::symbol_words::symbol_address(self.builder, crate::compiled::lane_region::relocs::RelocKind::GuardShape as u32, key),
            EmitImm::Image(word, key) => crate::symbol_words::symbol_address(self.builder, crate::compiled::lane_region::relocs::RelocKind::of_image_word(word) as u32, key),
        }
    }
    fn compare(&mut self, relation: Relation, a: ir::Value, b: ir::Value) -> ir::Value { self.raw_compare(relation, a, b) }
    /// `a` against an immediate at `a`'s own width (`icmp_imm`): a record's 32-bit shape word
    /// against a shape id is a 32-bit test, where an `i64` constant would not verify.
    fn compare_imm(&mut self, relation: Relation, a: ir::Value, imm: EmitImm) -> ir::Value {
        match imm {
            EmitImm::Const(bits) => self.raw_compare_imm(relation, a, bits as i64),
            EmitImm::Shape(_) | EmitImm::Image(..) => {
                let symbol = self.word(imm);
                let b = if self.builder.func.dfg.value_type(a) == ir::types::I32 { self.narrow(symbol) } else { symbol };
                self.raw_compare(relation, a, b)
            }
        }
    }
    fn widen(&mut self, a: ir::Value) -> ir::Value { self.raw_unary(Unary::ZeroExtend(Lane::I64), a) }
}

quench_runtime::projection!(Emit<'_, '_> => Emit);

impl Stage for Emit<'_, '_> {
    const SLOT_TABLES: bool = false;
    type Proof = super::super::EmitProof;
    type Word = ir::Value;
    /// Any integer: nonzero holds.
    type Cond = ir::Value;
    type Miss = ir::Block;
    type Imm = EmitImm;

    fn site(&mut self, v: u64) -> EmitImm { EmitImm::Const(v) }
    fn shape_site(&mut self, shape: u32) -> EmitImm {
        let tag = quench_runtime::staged::ic::SYMBOLIC_SHAPE;
        if shape & tag != 0 { EmitImm::Shape(shape & !tag) } else { EmitImm::Const(u64::from(shape)) }
    }
    fn image_word(&mut self, word: quench_runtime::staged::ImageWord, key: u32) -> EmitImm { EmitImm::Image(word, key) }
    fn load_site(&mut self, at: Access, base: ir::Value, offset: EmitImm) -> ir::Value {
        match offset {
            EmitImm::Const(bits) => self.load_u64(at, base, bits as i32),
            EmitImm::Shape(_) | EmitImm::Image(..) => unreachable!("a shape or a holder's word is never an offset"),
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
    /// The immediate forms (`iadd_imm_s`, `imul_imm_s`, ...): a constant an operand of the
    /// instruction, not a register holding it. An element's address is then an index scaled in
    /// the load itself (`[values, index, lsl #3]`) where a `mov` of 8 and a `mul` left two
    /// instructions the C a programmer writes for the same access never executes.
    fn add_imm(&mut self, a: ir::Value, imm: impl Into<EmitImm>) -> ir::Value {
        match imm.into() {
            EmitImm::Const(bits) => self.builder.ins().iadd_imm_s(a, bits as i64),
            imm => {
                let v = self.word(imm);
                self.raw_binary(Binary::Add, a, v)
            }
        }
    }
    fn imm_word(&mut self, imm: EmitImm) -> ir::Value { self.word(imm) }
    fn mul_imm(&mut self, a: ir::Value, imm: impl Into<EmitImm>) -> ir::Value {
        match (imm.into(), self.raw_lane(a)) {
            // A power of two is a shift, which the backend folds into the addressing mode of
            // the access that follows (`[values, index, lsl #3]`): an element's index costs no
            // instruction of its own, where `mul` by a register holding 8 costs two.
            (EmitImm::Const(0), _) => self.constant(0),
            (EmitImm::Const(bits), Lane::I64) if bits.is_power_of_two() => {
                self.builder.ins().ishl_imm_s(a, bits.trailing_zeros() as i64)
            }
            (EmitImm::Const(bits), Lane::I64) => self.builder.ins().imul_imm_s(a, bits as i64),
            (imm, _) => {
                let v = self.word(imm);
                self.raw_binary(Binary::Mul, a, v)
            }
        }
    }
    fn and_imm(&mut self, a: ir::Value, imm: impl Into<EmitImm>) -> ir::Value {
        match imm.into() {
            EmitImm::Const(bits) => self.builder.ins().band_imm_s(a, bits as i64),
            imm => {
                let v = self.word(imm);
                self.raw_binary(Binary::And, a, v)
            }
        }
    }
    fn or_imm(&mut self, a: ir::Value, imm: impl Into<EmitImm>) -> ir::Value {
        match imm.into() {
            EmitImm::Const(bits) => self.builder.ins().bor_imm_s(a, bits as i64),
            imm => {
                let v = self.word(imm);
                self.raw_binary(Binary::Or, a, v)
            }
        }
    }
    fn shr_imm(&mut self, a: ir::Value, shift: impl Into<EmitImm>) -> ir::Value {
        match shift.into() {
            EmitImm::Const(bits) => self.builder.ins().ushr_imm_s(a, bits as i64),
            shift => {
                let v = self.word(shift);
                self.raw_binary(Binary::ShiftRightUnsigned, a, v)
            }
        }
    }
    fn sar32_imm(&mut self, a: ir::Value, shift: u32) -> ir::Value {
        let low = self.narrow(a);
        let shifted = self.builder.ins().sshr_imm_s(low, i64::from(shift));
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
        if self.fresh {
            // A fresh holder logs nothing (4.7 (b)): only the observation gate is asked.
            return self.gate_word(|stage, payload, watch, logged| quench_runtime::staged::barrier::store_untracked(stage, payload, watch, logged));
        }
        self.gate_word(|stage, payload, watch, logged| match space {
            quench_runtime::staged::StagedRecordSpace::Class => quench_runtime::staged::barrier::class_store_unlogged(stage, payload, watch, super::super::class_mark_table(), record_id, value, logged),
            _ => quench_runtime::staged::barrier::store_unlogged(stage, payload, watch, super::super::store_mark_list(space), record_id, value, logged),
        })
    }
    /// Whether a store of a value the slot's type holds needs no barrier log
    /// (`Heap::held_store_unlogged`): slot observation sees nothing of it, so the tracking bit of
    /// the gate word is not asked; old records keep no counts (`counts_clear`), and a heap value's
    /// owner is unmarked (`barrier::value_unlogged`). A stage with no payload answers no, and the
    /// store leaves at its miss.
    fn held_store_gate(&mut self, space: quench_runtime::staged::StagedRecordSpace, record_id: ir::Value, value: ir::Value) -> ir::Value {
        if self.fresh {
            // A fresh holder's barrier log is elided (4.7 (b)), and the held store observes nothing: it is open.
            let zero = self.constant(0);
            return self.eq(zero, zero);
        }
        let Some(payload) = self.gate else { return self.ult(record_id, record_id) };
        let (logged, join) = (self.builder.create_block(), self.builder.create_block());
        self.builder.set_cold_block(logged);
        let open = self.builder.append_block_param(join, types::I8);
        let clear = self.counts_clear(payload);
        emit_js_guard(self.builder, clear, logged);
        match space {
            quench_runtime::staged::StagedRecordSpace::Class => quench_runtime::staged::barrier::class_value_unlogged(self, payload, super::super::class_mark_table(), record_id, value, logged),
            _ => quench_runtime::staged::barrier::value_unlogged(self, payload, super::super::store_mark_list(space), record_id, value, logged),
        }
        .expect("an Emit guard always continues");
        let yes = self.raw_const(Lane::I8, 1);
        self.goto(join, &[yes]);
        self.builder.switch_to_block(logged);
        let no = self.raw_const(Lane::I8, 0);
        self.goto(join, &[no]);
        self.builder.switch_to_block(join);
        open
    }
    /// Whether an immediate store needs no barrier log: the dispatch payload's counts bit is clear, read at run
    /// time from the run's own payload (the gate's, else the step's). A stage with no payload cannot read the run's
    /// counts, so it is closed and the store leaves at its miss (no process-wide flag is baked into compiled code).
    fn immediate_store_open(&mut self) -> ir::Value {
        let Some(payload) = self.gate.or(self.payload) else {
            let zero = self.constant(0);
            return self.ult(zero, zero);
        };
        self.counts_clear(payload)
    }
    fn assumption(&mut self, _watch: EmitImm) -> ir::Value {
        // A watched op never reaches emitted code: the specializer holds the watch's subject as a
        // code dependency instead (`StableHold::hold_holder`, `HeldShapes`), the invalidation hook
        // that replaces the test. An op that reached here anyway leaves at its miss.
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
    fn box_number(&mut self, number: quench_runtime::staged::Float64<ir::Value>) -> ir::Value {
        super::super::emit_js_box_number(self.builder, number)
    }
    /// A stored double's own bits (decision 4.1): a plain move, no read-side canonicalization.
    fn box_raw_number(&mut self, number: quench_runtime::staged::Float64<ir::Value>) -> ir::Value {
        self.raw_unary(Unary::Bitcast(Lane::I64), number.word())
    }
    /// A float bit entry: a NaN the canonical one (decision 4.1), an unordered self-compare's select.
    fn canonical_float(&mut self, bits: ir::Value) -> ir::Value {
        let unordered = self.raw_compare(Relation::FloatUnordered, bits, bits);
        let nan = self.raw_const(Lane::F64, f64::NAN.to_bits() as i64);
        self.raw_select(unordered, nan, bits)
    }
    /// An int32 word converts to its double's bits; any other Number word is a double already.
    fn raw_number(&mut self, bits: ir::Value) -> ir::Value {
        let high = self.shr_imm(bits, u64::from(u32::BITS));
        let int = self.eq_imm(high, quench_runtime::staged::value_encoding::INT_TAG_HIGH);
        let double = self.i32_to_f64(bits);
        let converted = self.raw_unary(Unary::Bitcast(Lane::I64), double);
        self.select(int, converted, bits)
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
    /// An ahead image's guessed shapes (`ic::SYMBOLIC_SHAPE`) are each compared with their
    /// symbol; the others form the set (`shape_in_set`).
    fn shape_in(&mut self, shape: ir::Value, shapes: &[u32]) -> ir::Value {
        let word = if self.raw_lane(shape) == Lane::I32 { shape } else { self.narrow(shape) };
        let tag = quench_runtime::staged::ic::SYMBOLIC_SHAPE;
        let (guessed, known): (Vec<u32>, Vec<u32>) = shapes.iter().partition(|shape| *shape & tag != 0);
        let mut member = (!known.is_empty()).then(|| super::shape_in_set(self.builder, word, &known));
        for shape in guessed {
            let symbol = self.word(EmitImm::Shape(shape & !tag));
            let symbol = self.narrow(symbol);
            let same = self.raw_compare(Relation::Equal, word, symbol);
            member = Some(match member { Some(member) => self.or(member, same), None => same });
        }
        member.expect("a shape set is non-empty")
    }
    fn number_truthy(&mut self, bits: ir::Value) -> ir::Value {
        let number = self.raw_unary(Unary::Bitcast(Lane::F64), bits);
        numbers::number_truthy(self, number)
    }
    fn choose(&mut self, cond: ir::Value, then: impl FnOnce(&mut Self) -> ir::Value,
        otherwise: impl FnOnce(&mut Self) -> ir::Value) -> ir::Value {
        self.choose_arms(cond, then, otherwise, false)
    }
    fn effect_if(&mut self, cond: ir::Value, then: impl FnOnce(&mut Self), cold: impl FnOnce(&mut Self)) { self.effect_arms(cond, None::<fn(&mut Self) -> ir::Value>, then, cold) }
    fn effect_if_both(&mut self, first: ir::Value, second: impl FnOnce(&mut Self) -> ir::Value, then: impl FnOnce(&mut Self), cold: impl FnOnce(&mut Self)) { self.effect_arms(first, Some(second), then, cold) }
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
    fn known(&self, cond: ir::Value) -> Option<bool> {
        crate::compiled::emit_machine::scalar::known_integer(self.builder, cond).map(|(bits, _)| bits != 0)
    }
    /// The fact this step noted of `word` (`facts`); the most precise of two notes of one word is the
    /// later one a refinement made, and both hold, so the last is taken.
    fn fact(&self, word: ir::Value) -> quench_runtime::typed_ir::Type {
        let word = self.builder.func.dfg.resolve_aliases(word);
        self.facts.iter().rev().find(|(noted, _)| *noted == word).map_or(quench_runtime::typed_ir::Type::ANY, |(_, fact)| *fact)
    }
    fn table(&mut self, table: Table) -> ir::Value {
        match table {
            Table::PropertyValues => match (self.property_values, self.payload) {
                (Some(base), _) => base,
                (None, Some(payload)) => super::super::emit_js_property_values(self.builder, payload).0,
                (None, None) => panic!("an operation addressing the arena has its base"),
            },
            // The table is a static that never moves; in-process code embeds its address.
            Table::Layouts => self.constant(quench_runtime::tier_surface::layouts::table_base() as u64),
            // The class space's base is per heap, so it is not an iconst: compiled code is shared by the
            // heaps that run it. The dispatch head holds the base of the heap the running VM owns.
            Table::ClassSpace => match self.payload {
                Some(payload) => super::super::emit_js_payload_word(self.builder, payload, std::mem::offset_of!(super::super::JsStepDispatch, head.class_base)),
                None => panic!("an operation addressing the class space has its dispatch payload"),
            },
            // A stage without the slot tables (`SLOT_TABLES`) never addresses them.
            Table::SlotChains | Table::ChainOwners | Table::OwnerTypes => unreachable!("a Cranelift row stores through the VM"),
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
    fn call_entry(&mut self, signature: ir::SigRef, entry: quench_runtime::staged::machine::CallTarget<ir::Value>, args: &[ir::Value]) -> (ir::Value, ir::Value) {
        let results = self.call_signature(signature, entry, args);
        (results[0], results[1])
    }
    fn ret(&mut self, values: &[ir::Value]) { self.leave(values); }
}

/// Whether `divisor` is a power of two of at least one: a finite binary64 with a significand of one. Its quotient
/// `x / divisor` never overflows, and its product with the truncated quotient is exact.
fn is_power_of_two_at_least_one(divisor: f64) -> bool {
    divisor >= 1.0 && divisor.is_finite() && divisor.to_bits() & ((1 << 52) - 1) == 0
}

impl Emit<'_, '_> {
    /// The binary64 value of `value` when the lowering folded it to a constant: an `f64const`, or an int32 or
    /// 64-bit integer constant converted or bit-copied into one.
    fn known_double(&self, value: ir::Value) -> Option<f64> {
        let dfg = &self.builder.func.dfg;
        let value = dfg.resolve_aliases(value);
        match dfg.insts[dfg.value_def(value).inst()?] {
            ir::InstructionData::UnaryIeee64 { opcode: ir::Opcode::F64const, imm } => Some(f64::from_bits(imm.bits())),
            ir::InstructionData::Unary { opcode: ir::Opcode::FcvtFromSint, arg } => {
                crate::compiled::emit_machine::scalar::known_integer(self.builder, arg).map(|(_, signed)| signed as f64)
            }
            ir::InstructionData::Unary { opcode: ir::Opcode::Bitcast, arg } => {
                crate::compiled::emit_machine::scalar::known_integer(self.builder, arg).map(|(bits, _)| f64::from_bits(bits))
            }
            _ => None,
        }
    }

    /// The remainder by a constant divisor the exact sequence covers (`power_of_two_remainder`), else `None`.
    fn constant_remainder(&mut self, left: ir::Value, right: ir::Value) -> Option<ir::Value> {
        let divisor = self.known_double(right).filter(|&divisor| is_power_of_two_at_least_one(divisor))?;
        Some(self.power_of_two_remainder(left, divisor))
    }

    /// `left % divisor` for a power-of-two divisor of at least one: `x - trunc(x / c) * c`, which is fmod exactly
    /// here (the quotient cannot overflow, the product is a shift, the difference is representable), with the
    /// dividend's sign copied on so a zero remainder of a negative dividend is `-0`.
    fn power_of_two_remainder(&mut self, left: ir::Value, divisor: f64) -> ir::Value {
        let dividend = if self.raw_lane(left) == Lane::F64 { left } else { self.f64_from_bits(left) };
        let divisor = self.f64_constant(divisor);
        let quotient = self.float_arithmetic(NumberArithmetic::Divide, dividend, divisor);
        let whole = self.raw_unary(Unary::Truncate, quotient);
        let product = self.float_arithmetic(NumberArithmetic::Multiply, whole, divisor);
        let remainder = self.float_arithmetic(NumberArithmetic::Subtract, dividend, product);
        self.raw_binary(Binary::FloatCopySign, remainder, dividend)
    }

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

impl NumericStage for Emit<'_, '_> {
    const ORDERED_FIRST: bool = true;
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
    /// The remainder of two binary64 words: where the integral remainder admits them (`number_machine::
    /// integral_remainder_operands`, the interpreter's bound and rule), its exact i64 form; else the shared leaf
    /// (`numbers::float_remainder`).
    fn float_remainder(&mut self, left: ir::Value, right: ir::Value) -> ir::Value {
        if let Some(value) = self.constant_remainder(left, right) {
            return value;
        }
        let (dividend, divisor, integral) = quench_runtime::staged::number_machine::integral_remainder_operands(self, left, right);
        self.choose_arms(integral,
            |stage| quench_runtime::staged::number_machine::integral_remainder_value(stage, (dividend, divisor), left),
            |stage| numbers::float_remainder(stage, left, right), false)
    }
    fn integral_remainder(&mut self, left: ir::Value, right: ir::Value) -> (ir::Value, ir::Value) {
        // The u-jit makes the float remainder for every operand (a leaf call it can hold), so its miss is never taken.
        let value = match self.constant_remainder(left, right) {
            Some(value) => value,
            None => numbers::float_remainder(self, left, right),
        };
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
    /// `base ** exponent`: the product a constant exponent `SmallPower` names, else a call of the power leaf
    /// (`EngineSlowPath::NumberPower`), which touches no VM state.
    fn float_power(&mut self, base: ir::Value, exponent: ir::Value) -> ir::Value {
        match self.known_double(exponent).and_then(quench_runtime::staged::power::SmallPower::of) {
            Some(small) => quench_runtime::staged::power::power_by(self, small, base),
            None => self.call_leaf_lanes(crate::compiled::slow_paths::EngineSlowPath::NumberPower, (&[Lane::F64, Lane::F64], Lane::F64), &[base, exponent]),
        }
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
mod effects;
mod gate;
#[cfg(all(test, feature = "jit"))] mod tier_homomorphism_tests;
#[cfg(all(test, feature = "jit", target_arch = "x86_64"))] mod ir_predict_tests;

