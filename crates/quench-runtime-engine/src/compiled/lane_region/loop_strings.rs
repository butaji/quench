//! The flat strings a loop reads through a register it never redefines (`LoopString`): their
//! facts the loop's entering edge proves once, which the loop's `length` reads, `charCodeAt`
//! calls and strict equalities then hold, as C holds a string's length and its pointer across a loop. A string's length
//! never changes, and a flat string keeps its form for as long as the register holds it, so the
//! held record and length stay true across the loop. Its units are read from the record where a
//! call uses them (`strings::record_units`): a record's suballocations may change when the VM runs.

#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::bytecode::{Register, WideInstruction};
use quench_runtime::staged::{string_length, strings, Comparison, Operation};
use super::*;
use super::builtin_calls::continues;

/// A loop-invariant register whose flat facts the loop holds: one a `charCodeAt` of the loop reads
/// (`reads_units`), or one a strict equality of the loop compares (`held_strings_equal`).
#[derive(Clone, Copy)]
pub(crate) struct LoopString {
    pub(crate) header: usize,
    pub(crate) back: usize,
    pub(crate) register: Register,
    pub(crate) reads_units: bool,
    /// Whether the loop's entering edge last hoisted it (`hoist_strings`): its variables are defined
    /// only then.
    pub(crate) hoisted: bool,
}

/// The variables a held string's facts live in across its loop's iterations: its record's address
/// (fixed while the value is rooted in its register), its form and width as words of 0 or 1, and
/// its length.
#[derive(Clone, Copy)]
pub(crate) struct StringVariables {
    record: Variable,
    short: Variable,
    wide: Variable,
    length: Variable,
}

/// The registers a strict equality at `instruction` compares: both operands of a strict `===` or `!==`
/// jump or value row, where they are registers (`equality`).
fn strict_compared(instruction: &WideInstruction) -> Vec<Register> {
    match crate::compiled::performed(instruction) {
        Some(Operation::CompareJump { comparison: Comparison::Equality { strict: true, .. }, left, right, .. }) =>
            [left, right].iter().filter_map(|operand| operand.register_index()).collect(),
        Some(Operation::Equality { left, right, strict: true, .. }) =>
            [left, right].iter().filter_map(|operand| operand.register_index()).collect(),
        _ => Vec::new(),
    }
}

/// The held strings of `loops`: per loop, each register a `charCodeAt` in the loop reads, and each
/// register a strict equality in the loop compares, that no instruction of the loop writes. A register
/// that is no string leaves the loop at its header (`hoist_strings`) only when a `charCodeAt` reads it;
/// a register only compared is held where its entering edge proves a string. A `length` read of any
/// other register is not held.
pub(crate) fn loop_strings(unit: &super::super::lane_unit::Unit, loops: &[(usize, usize)],
    intrinsics: &super::native_calls::Intrinsics, lowered: impl Fn(usize) -> bool) -> Vec<LoopString> {
    let mut held: Vec<LoopString> = Vec::new();
    for &(header, back) in loops.iter().filter(|(header, back)| lowered(*header) && lowered(*back)) {
        let body = &unit.instructions[header..=back];
        let written: BTreeSet<Register> = body.iter().flat_map(|candidate| writes(*candidate)).collect();
        let receivers = (header..).zip(body)
            .filter(|(pc, _)| super::native_calls::reads_string(intrinsics, *pc))
            .map(|(_, instruction)| (instruction.register_c(), true));
        let compared = (header..).zip(body)
            .flat_map(|(_, instruction)| strict_compared(instruction))
            .map(|register| (register, false));
        for (register, reads_units) in receivers.chain(compared) {
            if written.contains(&register) { continue; }
            if let Some(string) = held.iter_mut().find(|string| string.header == header && string.register == register) {
                string.reads_units |= reads_units;
                continue;
            }
            held.push(LoopString { header, back, register, reads_units, hoisted: false });
        }
    }
    // A loop inside one that holds the same register reads the facts the outer entry proved.
    let outer = held.clone();
    held.into_iter().filter(|inner| !outer.iter().any(|encloser| encloser.register == inner.register && encloser.header < inner.header
        && inner.back <= encloser.back)).collect()
}

impl Lowering<'_, '_> {
    /// The held string `register` names at `pc`, of the innermost loop around `pc` that holds it.
    pub(crate) fn loop_string(&self, pc: usize, register: Option<Register>) -> Option<LoopString> {
        let register = register?;
        self.loop_strings.iter().copied()
            .filter(|string| string.hoisted && string.register == register && (string.header..=string.back).contains(&pc))
            .min_by_key(|string| string.back - string.header)
    }

    /// The variables `register`'s held string lives in, declared once.
    fn string_variables(&mut self, register: Register) -> StringVariables {
        let builder = &mut self.builder;
        *self.string_vars.entry(register).or_insert_with(|| StringVariables {
            record: builder.declare_var(types::I64),
            short: builder.declare_var(types::I64),
            wide: builder.declare_var(types::I64),
            length: builder.declare_var(types::I64),
        })
    }

    /// The held strings of the loop at `header` that its entering edge `state` proves, marking each
    /// (`hoisted`). A register only compared is held where `state` proves a string, else it stays unheld
    /// and the loop compares it exactly; a register a `charCodeAt` reads leaves at the header, as before.
    /// The last hoist of the header decides: the entering edge that reaches the loop body.
    fn strings_to_hoist(&mut self, state: &State, header: usize) -> Vec<LoopString> {
        for string in self.loop_strings.iter_mut().filter(|string| string.header == header) {
            string.hoisted = string.reads_units || state.values[usize::from(string.register)].value.ty
                .is_subtype(Type::language(quench_runtime::ValueType::String));
        }
        self.loop_strings.iter().copied().filter(|string| string.header == header && string.hoisted).collect()
    }

    /// Prove the flat strings the loop at `header` holds on its entering edge (`loop_strings`). A
    /// register holding a rope takes its flat form from the VM (`LaneHelper::FlattenString`): the
    /// same string in a record that holds its units, which the register then holds. A register that
    /// is no string, whose text has no record form, or a length past int32 leaves at the header
    /// before the loop runs.
    pub(crate) fn hoist_strings(&mut self, state: &mut State, header: usize) {
        let held = self.strings_to_hoist(state, header);
        if held.is_empty() { return; }
        let exit = self.guard_exit(state, header as u32);
        for string in held {
            // The solver's type of the register before any rope is flattened into it: its length range proves the fit.
            let length_fits = super::tag_proofs::length_fits_int32(state.values[usize::from(string.register)].value.ty);
            let bits = self.tagged(state, string.register);
            let ready = self.builder.create_block();
            let parts = [(); 3].map(|()| self.builder.append_block_param(ready, types::I64));
            let rope = cold_block(&mut self.builder);
            let records = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, head.string_record_base));
            let flat = self.flat_record_words(records, bits, rope);
            self.goto(ready, &flat);
            // A rope: the VM's flat copy of it, a new record the register holds from here on. The
            // helper allocates and so may move the bases the lowering holds (`allocated`); a
            // record it cannot make flat leaves at the header.
            self.builder.switch_to_block(rope);
            self.builder.seal_block(rope);
            let flattened = self.helper_call(LaneHelper::FlattenString, &[self.payload, bits]);
            let made = self.raw_compare_imm(Relation::NotEqual, flattened, Value::DELETED.to_bits() as i64);
            emit_js_guard(&mut self.builder, made, exit);
            self.allocated(state);
            self.define(state, string.register, flattened, RegisterFact::definition(Type::language(quench_runtime::ValueType::String), Representation::Tagged));
            let records = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, head.string_record_base));
            let flat = self.flat_record_words(records, flattened, exit);
            self.goto(ready, &flat);
            self.builder.switch_to_block(ready);
            self.builder.seal_block(ready);
            let [record, short, wide] = parts;
            let length = {
                let mut stage = super::super::object_slots::emit(&mut self.builder);
                let short = stage.ne_imm(short, 0u32);
                strings::record_length(&mut stage, record, short)
            };
            if !length_fits {
                let fits = self.raw_compare_imm(Relation::UnsignedLessEqual, length, i64::from(i32::MAX));
                emit_js_guard(&mut self.builder, fits, exit);
            }
            let variables = self.string_variables(string.register);
            self.builder.def_var(variables.record, record);
            self.builder.def_var(variables.short, short);
            self.builder.def_var(variables.wide, wide);
            self.builder.def_var(variables.length, length);
        }
    }

    /// The record, form and width words of the flat string `value` names among the string records at
    /// `records` (`string_length::flat_record`), the form and width as words of 0 or 1; `value` that
    /// is no flat string leaves at `miss`.
    fn flat_record_words(&mut self, records: ir::Value, value: ir::Value, miss: ir::Block) -> [ir::Value; 3] {
        let mut stage = super::super::object_slots::emit(&mut self.builder);
        let (record, short, wide) = continues(string_length::flat_record(&mut stage, records, value, miss));
        let (one, zero) = (stage.constant(1), stage.constant(0));
        [record, stage.select(short, one, zero), stage.select(wide, one, zero)]
    }

    /// The held length of `string` as an int32: a flat string's, proved on the loop's entry.
    pub(crate) fn held_length(&mut self, string: LoopString) -> ir::Value {
        let variables = self.string_variables(string.register);
        let length = self.builder.use_var(variables.length);
        self.raw_unary(Unary::Narrow(Lane::I32), length)
    }

    /// `charCodeAt` at the index in `index` of the held string `string`, as its tagged row: the units
    /// are read from the held record at the call, and the row's miss leaves at `exit`. `within`: the
    /// index is proved below the length, so the row takes no bounds test.
    pub(crate) fn held_char_code_at(&mut self, state: &mut State, string: LoopString, index: Register, within: bool,
        exit: ir::Block) -> ir::Value {
        let variables = self.string_variables(string.register);
        let record = self.builder.use_var(variables.record);
        let short = self.builder.use_var(variables.short);
        let wide = self.builder.use_var(variables.wide);
        let length = self.builder.use_var(variables.length);
        let index = self.tagged(state, index);
        let mut stage = super::super::object_slots::emit(&mut self.builder);
        let short = stage.ne_imm(short, 0u32);
        let units = strings::record_units(&mut stage, record, short);
        let flat = strings::FlatString { units, length, wide };
        continues(if within {
            strings::string_char_code_at_within(&mut stage, flat, index, exit)
        } else {
            strings::string_char_code_at(&mut stage, flat, index, exit)
        })
    }

    /// The held pair of a strict equality of two registers both held at `pc` (`equality`).
    pub(crate) fn held_pair(&self, pc: usize, left: Operand, right: Operand, strict: bool) -> Option<[LoopString; 2]> {
        match (strict, self.loop_string(pc, left.register_index()), self.loop_string(pc, right.register_index())) {
            (true, Some(left_string), Some(right_string)) => Some([left_string, right_string]),
            _ => None,
        }
    }

    /// `left === right` of a held pair, whose tagged words are `bits`, as C compares two strings: the
    /// identity test first, then the held lengths; equal lengths go to `held_units_equal`.
    pub(crate) fn held_strings_equal(&mut self, pair: [LoopString; 2], bits: [ir::Value; 2]) -> ir::Value {
        let [identical, distinct, sized, join] = std::array::from_fn(|_| self.builder.create_block());
        let equal = self.builder.append_block_param(join, types::I8);
        let same = self.raw_compare(Relation::Equal, bits[0], bits[1]);
        self.branch_if(same, (identical, &[]), (distinct, &[]));
        // A string is never NaN, so identical words are equal.
        self.builder.switch_to_block(identical);
        let holds = self.raw_const(Lane::I8, 1);
        self.goto(join, &[holds]);
        self.builder.switch_to_block(distinct);
        let (left, right) = (self.string_variables(pair[0].register), self.string_variables(pair[1].register));
        let (left_length, right_length) = (self.builder.use_var(left.length), self.builder.use_var(right.length));
        let same_length = self.raw_compare(Relation::Equal, left_length, right_length);
        let unequal = self.raw_const(Lane::I8, 0);
        self.branch_if(same_length, (sized, &[]), (join, &[unequal]));
        self.builder.switch_to_block(sized);
        self.held_units_equal([left, right], bits, join);
        self.builder.switch_to_block(join);
        equal
    }

    /// The equal-length case of `held_strings_equal`, in the block it is entered in: two short records
    /// compare their words (the form holds its units, zero-padded); any other pair is the VM's compare
    /// (`LaneHelper::StrictEqual`). Its result goes to `join`.
    fn held_units_equal(&mut self, [left, right]: [StringVariables; 2], bits: [ir::Value; 2], join: ir::Block) {
        let (left_short, right_short) = (self.builder.use_var(left.short), self.builder.use_var(right.short));
        let (left_record, right_record) = (self.builder.use_var(left.record), self.builder.use_var(right.record));
        let meta = regions::flags(quench_runtime::staged::region::Region::HeapMeta.mutable());
        let [words, compare] = std::array::from_fn(|_| self.builder.create_block());
        let both_short_word = self.raw_binary(Binary::And, left_short, right_short);
        let both_short = self.raw_compare_imm(Relation::NotEqual, both_short_word, 0);
        self.branch_if(both_short, (words, &[]), (compare, &[]));
        self.builder.switch_to_block(words);
        let mut differ = self.raw_const(Lane::I64, 0);
        for word in 0..quench_runtime::staged::string_form::RECORD_SIZE / size_of::<u64>() {
            let offset = (word * size_of::<u64>()) as i32;
            let (a, b) = (self.raw_load(Lane::I64, meta, left_record, offset), self.raw_load(Lane::I64, meta, right_record, offset));
            let mismatch = self.raw_binary(Binary::Xor, a, b);
            differ = self.raw_binary(Binary::Or, differ, mismatch);
        }
        let same_text = self.raw_compare_imm(Relation::Equal, differ, 0);
        self.goto(join, &[same_text]);
        self.builder.switch_to_block(compare);
        let result = self.helper_call(LaneHelper::StrictEqual, &bits);
        let cells_equal = self.raw_unary(Unary::Narrow(Lane::I8), result);
        self.goto(join, &[cells_equal]);
    }
}
