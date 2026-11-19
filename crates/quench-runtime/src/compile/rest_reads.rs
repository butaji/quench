//! Rest parameters read only by index and `length` keep no array: such a body's reads of the
//! rest register become `RestGet` and `RestLength`, which read the values its activation kept
//! past the fixed parameters (`Function::rest_view`). An entry that keeps those values leaves
//! the register `undefined` and makes no array; any other entry still makes the array, and
//! the reads then read it, so every entry stays exact. A spread of the whole rest parameter
//! (`f(...rest)`, a `Move` of it into the window of the `CallSpread` right after) becomes
//! `CallRest`, which spreads the kept values where the register holds `undefined`.
use super::liveness::{self, MethodArguments, register_bit};
use crate::bytecode::{FieldLookup, FieldSite, Function, Instr, Op, Superinstruction, Traits, the_op};

const REST_INDEX_READ_OP: Op = the_op(Traits::REST_INDEX_READ);
const REST_LENGTH_READ_OP: Op = the_op(Traits::REST_LENGTH_READ);
const REST_SPREAD_CALL_OP: Op = the_op(Traits::REST_SPREAD_CALL);

/// Rewrites `function`'s reads of its rest parameter where every use is `rest[key]` or
/// `rest.length` and no instruction writes it.
pub(super) fn rewrite(
    function: &mut Function,
    methods: &dyn MethodArguments,
    fields: &[FieldSite],
    supers: &[Superinstruction],
    is_length: &dyn Fn(u32) -> bool,
) {
    if !function.rest || !function.wide.is_empty() || function.is_async || function.is_generator
        || function.has_dynamic_scope_access() || function.arguments_slot.is_some()
        || !function.has_restricted_legacy_caller_access() || function.is_class_constructor || function.derived_constructor
    { return; }
    let fixed = function.params - 1;
    let Some(rest) = function.promoted_register(fixed) else { return };
    let Some(transfers) = liveness::transfers(function, methods, fields, supers) else { return };
    let bit = register_bit(rest);
    let code: Vec<_> = function.instructions().collect();
    let mut entered = vec![false; code.len() + 1];
    for (pc, instruction) in code.iter().enumerate() {
        crate::bytecode::for_each_instruction_successor(pc, *instruction, |successor| {
            if successor != pc + 1 && successor < entered.len() {
                entered[successor] = true;
            }
        });
    }
    let mut replacements = Vec::new();
    for (pc, instruction) in code.iter().copied().enumerate() {
        let (uses, definitions) = transfers[pc];
        if definitions & bit != 0 { return; }
        if uses & bit == 0 { continue; }
        let replacement = match instruction.op() {
            op if op.traits().has(Traits::ELEMENT_LOAD) && instruction.result_flags() == 0 && instruction.b() == rest && instruction.c() != rest =>
                Instr::try_new(REST_INDEX_READ_OP, instruction.result_register(), instruction.c(), rest, u32::from(fixed)),
            op if op.traits().has(Traits::FIELD_LEAF) && instruction.result_flags() == 0 => match instruction.field_lookup() {
                Some(FieldLookup::Atom { atom, base, .. }) if is_length(atom) && base.register_index() == Some(rest) =>
                    Instr::try_new(REST_LENGTH_READ_OP, instruction.result_register(), rest, 0, u32::from(fixed)),
                _ => None,
            },
            // `Move window, rest` straight into `CallSpread` of that one-value window: the
            // move stays, the call spreads what it moved (`CallRest`).
            op if op.traits().has(Traits::REGISTER_COPY) && instruction.result_flags() == 0 && instruction.b() == rest => {
                let window = instruction.result_register();
                // The call is the next instruction that is not a no-op, and no jump enters the
                // span between them.
                let Some(next) = (pc + 1..code.len()).find(|&at| !code[at].op().traits().has(Traits::NO_OP)) else { return };
                let Some(call) = code.get(next).copied().filter(|call| {
                    call.op().traits().has(Traits::SPREAD_CALL) && !entered[pc + 1..=next].iter().any(|&entry| entry)
                        && !call.direct_eval() && !call.parameter_eval()
                        && call.call_window().base == window && call.call_window().count == 1
                        && window != rest && call.b() != rest && call.c() != rest
                }) else { return };
                let Some(spread) = Instr::try_new(REST_SPREAD_CALL_OP, call.a(), call.b(), call.c(), call.imm()) else { return };
                replacements.push((next, spread));
                continue;
            }
            _ => None,
        };
        let Some(replacement) = replacement else { return };
        replacements.push((pc, replacement));
    }
    if replacements.is_empty() { return; }
    for (pc, replacement) in replacements {
        function.code[pc] = replacement;
    }
}
