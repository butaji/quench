//! The flat strings a loop reads through a register it never redefines (`LoopString`): their
//! facts the loop's entering edge proves once, which the loop's `length` reads and `charCodeAt`
//! calls then hold, as C holds a string's length and its pointer across a loop. A string's length
//! never changes, and a flat string keeps its form for as long as the register holds it, so the
//! held record and length stay true across the loop. Its units are read from the record where a
//! call uses them (`strings::record_units`): a record's suballocations may change when the VM runs.

use crate::bytecode::Register;
use crate::staged::{string_length, strings};
use super::*;
use super::builtin_calls::continues;

/// A loop-invariant register whose loop reads a `charCodeAt` of it, and so holds its flat facts.
#[derive(Clone, Copy)]
pub(super) struct LoopString {
    pub(super) header: usize,
    pub(super) back: usize,
    pub(super) register: Register,
}

/// The variables a held string's facts live in across its loop's iterations: its record's address
/// (fixed while the value is rooted in its register), its form and width as words of 0 or 1, and
/// its length.
#[derive(Clone, Copy)]
pub(super) struct StringVariables {
    record: Variable,
    short: Variable,
    wide: Variable,
    length: Variable,
}

/// The held strings of `loops`: per loop, each register a `charCodeAt` in the loop reads that no
/// instruction of the loop writes. A register that is no string leaves the loop at its header
/// (`hoist_strings`), so only the receivers of a `charCodeAt` qualify, not every `length` read.
pub(super) fn loop_strings(unit: &super::super::lane_unit::Unit, loops: &[(usize, usize)],
    intrinsics: &super::native_calls::Intrinsics, lowered: impl Fn(usize) -> bool) -> Vec<LoopString> {
    let mut held: Vec<LoopString> = Vec::new();
    for &(header, back) in loops.iter().filter(|(header, back)| lowered(*header) && lowered(*back)) {
        let body = &unit.instructions[header..=back];
        let written: BTreeSet<Register> = body.iter().flat_map(|candidate| writes(*candidate)).collect();
        let receivers = (header..).zip(body)
            .filter(|(pc, _)| super::native_calls::reads_string(intrinsics, *pc))
            .map(|(_, instruction)| instruction.register_c());
        for register in receivers {
            if written.contains(&register) || held.iter().any(|string| string.header == header && string.register == register) { continue; }
            held.push(LoopString { header, back, register });
        }
    }
    // A loop inside one that holds the same register reads the facts the outer entry proved.
    let outer = held.clone();
    held.into_iter().filter(|inner| !outer.iter().any(|encloser| encloser.register == inner.register && encloser.header < inner.header
        && inner.back <= encloser.back)).collect()
}

impl Lowering<'_, '_> {
    /// The held string `register` names at `pc`, of the innermost loop around `pc` that holds it.
    pub(super) fn loop_string(&self, pc: usize, register: Option<Register>) -> Option<LoopString> {
        let register = register?;
        self.loop_strings.iter().copied()
            .filter(|string| string.register == register && (string.header..=string.back).contains(&pc))
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

    /// Prove the flat strings the loop at `header` holds on its entering edge (`loop_strings`): a
    /// register that is no flat string, or a length past int32, leaves at the header before the loop runs.
    pub(super) fn hoist_strings(&mut self, state: &mut State, header: usize) {
        let held: Vec<LoopString> = self.loop_strings.iter().copied().filter(|string| string.header == header).collect();
        if held.is_empty() { return; }
        let exit = self.guard_exit(state, header as u32);
        for string in held {
            let bits = self.tagged(state, string.register);
            let records = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, head.string_record_base));
            let (record, short, wide, length) = {
                let mut stage = super::super::object_slots::emit(&mut self.builder);
                let (record, short, wide) = continues(string_length::flat_record(&mut stage, records, bits, exit));
                let length = strings::record_length(&mut stage, record, short);
                let (one, zero) = (stage.constant(1), stage.constant(0));
                (record, stage.select(short, one, zero), stage.select(wide, one, zero), length)
            };
            let fits = self.raw_compare_imm(Relation::UnsignedLessEqual, length, i64::from(i32::MAX));
            emit_js_guard(&mut self.builder, fits, exit);
            let variables = self.string_variables(string.register);
            self.builder.def_var(variables.record, record);
            self.builder.def_var(variables.short, short);
            self.builder.def_var(variables.wide, wide);
            self.builder.def_var(variables.length, length);
        }
    }

    /// The held length of `string` as an int32: a flat string's, proved on the loop's entry.
    pub(super) fn held_length(&mut self, string: LoopString) -> ir::Value {
        let variables = self.string_variables(string.register);
        let length = self.builder.use_var(variables.length);
        self.raw_unary(Unary::Narrow(Lane::I32), length)
    }

    /// `charCodeAt` at the index in `index` of the held string `string`, as its tagged row: the units
    /// are read from the held record at the call, and the row's miss leaves at `exit`. `within`: the
    /// index is proved below the length, so the row takes no bounds test.
    pub(super) fn held_char_code_at(&mut self, state: &mut State, string: LoopString, index: Register, within: bool,
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
}
