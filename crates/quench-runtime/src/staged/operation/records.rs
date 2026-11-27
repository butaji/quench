//! Runtime glue of `operation`: the instruction queries the tiers ask (`Operation::of`, the `is_*`
//! predicates, `field_cache_sites`) and the row constant `RETURN_ROW`. The stencils decode no instruction.
#![allow(unused_imports)]
use super::*;
use crate::staged::OperationOperationExt;

/// The instruction queries the tiers ask (runtime only, as the stencils decode no instruction), and the
/// queries the runtime's profile and VM ask of the rows.
pub trait OperationOperationRecordsExt: Sized {
    fn of(instruction: WideInstruction) -> Option<Self>;

    fn decode_lane(instruction: WideInstruction) -> Option<Self>;

    fn is_set_field(instruction: WideInstruction) -> bool;

    fn is_nop(instruction: WideInstruction) -> bool;

    fn is_number_binary(instruction: WideInstruction) -> bool;

    fn is_number_unary(instruction: WideInstruction) -> bool;

    fn is_plain_call(instruction: WideInstruction) -> bool;

    fn is_ordinary_call(instruction: WideInstruction) -> bool;

    fn is_construction(instruction: WideInstruction) -> bool;

    fn is_throw(instruction: WideInstruction) -> bool;

    fn is_yield(instruction: WideInstruction) -> bool;

    fn is_await(instruction: WideInstruction) -> bool;

    fn is_field_read(instruction: WideInstruction) -> bool;

    fn is_field_store(instruction: WideInstruction) -> bool;

    fn field_cache_sites(instruction: WideInstruction, field_sites: &[crate::bytecode::FieldSite]) -> Vec<(u16, bool)>;

    fn is_unit_rewritten(instruction: WideInstruction) -> bool;

    const RETURN_ROW: u16;
}

impl OperationOperationRecordsExt for Operation<u64, ()> {
    /// The operation `instruction` denotes, decoded with no tables (`DecodeContext::EMPTY`): what a
    /// tier asks an instruction's meaning by, where the row's operands need no table.
    fn of(instruction: WideInstruction) -> Option<Self> {
        Self::decode(instruction, DecodeContext::EMPTY)
    }

    /// The lane's own rows (`Decoder::Lane`): a string-case index over the constants the record names, and an
    /// `await` the lane's frame suspends.
    #[inline(always)]
    fn decode_lane(instruction: WideInstruction) -> Option<Self> {
        Some(match instruction.op() {
            Op::StringCaseIndex => Self::StringCaseIndex { destination: instruction.result_register(),
                source: instruction.register_c(), start: instruction.imm(), count: instruction.element_count() },
            Op::TierPrologue => Self::Prologue { function: instruction.imm(), parameters: instruction.a() },
            Op::BodyEntry => Self::BodyEntry { function: instruction.imm(), binds_this: instruction.b() != 0 },
            _ => return None,
        })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    /// Whether `instruction` is the named store `SetField` itself (not a store to `this`).
    fn is_set_field(instruction: WideInstruction) -> bool {
        instruction.op() == Op::SetField
    }

    /// Whether `instruction` does nothing: a marker that defines and writes nothing.
    fn is_nop(instruction: WideInstruction) -> bool {
        instruction.op() == Op::Nop
    }

    /// Whether `instruction` is a Number binary operation (`NumericBinary`): its operator is the
    /// Number arithmetic, relation or bitwise operation the tier reads from its operands.
    fn is_number_binary(instruction: WideInstruction) -> bool {
        instruction.op().js_denotation() == crate::bytecode::JsDenotation::NumericBinary
    }

    /// Whether `instruction` is a Number unary operation (`NumericUnary`), whose operator the tier
    /// reads from the instruction's unary operator field.
    fn is_number_unary(instruction: WideInstruction) -> bool {
        instruction.op().js_denotation() == crate::bytecode::JsDenotation::NumericUnary
    }

    /// Whether `instruction` is a call through its callee register and a window of arguments (`Call`, and
    /// `CallForwardingArguments`, whose window the same record names), which an inlined unit plans and admits as a call.
    fn is_plain_call(instruction: WideInstruction) -> bool {
        matches!(instruction.op(), Op::Call | Op::CallForwardingArguments)
    }

    /// Whether `instruction` is an ordinary call through its callee register, with a window or a
    /// spread array of arguments (`Call`, `CallArray`).
    fn is_ordinary_call(instruction: WideInstruction) -> bool {
        instruction.op().is_ordinary_call()
    }

    /// Whether `instruction` constructs (`Construct`), with its callee and arguments in registers.
    fn is_construction(instruction: WideInstruction) -> bool {
        instruction.op() == Op::Construct
    }

    /// Whether `instruction` throws: its decoded operation is `Throw`, which leaves at the miss edge.
    fn is_throw(instruction: WideInstruction) -> bool {
        matches!(Self::of(instruction), Some(Self::Throw { .. }))
    }

    /// Whether `instruction` yields a value from a generator (`Yield`, a lane row). The interpreter
    /// frame answers no suspension (`Frame::suspend_yield`), so its yield is the general loop's step.
    fn is_yield(instruction: WideInstruction) -> bool {
        matches!(Self::decode_lane(instruction), Some(Self::Suspend { kind: crate::staged::Suspension::Yield { .. }, .. }))
    }

    /// Whether `instruction` awaits a value (`Await`, a lane row). The interpreter frame answers no
    /// suspension (`Frame::suspend_await`), so an async frame's await is the general loop's step.
    fn is_await(instruction: WideInstruction) -> bool {
        matches!(Self::decode_lane(instruction), Some(Self::Suspend { kind: crate::staged::Suspension::Await, .. }))
    }

    /// Whether `instruction` is a named field read (`GetField`), whatever its result flags.
    fn is_field_read(instruction: WideInstruction) -> bool {
        instruction.op() == Op::GetField
    }

    /// Whether `instruction` is a named field store (`SetField`, `SetThisField`, or a store of a
    /// field of `this`), whatever its operand.
    fn is_field_store(instruction: WideInstruction) -> bool {
        instruction.op().is_field_store()
    }

    /// The field cache sites `instruction` reads (`true`) or stores through (`false`), as the
    /// program's field sites name them: a named read's lookup (its site, or the sites of a
    /// nested lookup), and a field store's cache site.
    fn field_cache_sites(instruction: WideInstruction, field_sites: &[crate::bytecode::FieldSite]) -> Vec<(u16, bool)> {
        if Self::is_field_read(instruction) {
            return match instruction.field_lookup() {
                Some(FieldLookup::Atom { cache_site, .. }) => vec![(cache_site, true)],
                Some(FieldLookup::Site(index)) => field_sites.get(index).map_or_else(Vec::new, |site| {
                    let reads = [Some(site.first), site.second].into_iter().flatten().map(|(_, cache)| (cache, true));
                    reads.chain(site.sink.map(|(_, cache)| (cache, false))).collect()
                }),
                None => Vec::new(),
            };
        }
        if Self::is_field_store(instruction) { vec![(instruction.cache_site_index(), false)] } else { Vec::new() }
    }

    /// Whether a unit holds `instruction` by its own rewrite or an exit rather than by renaming
    /// (the catalog's `UNIT_REWRITTEN` trait).
    fn is_unit_rewritten(instruction: WideInstruction) -> bool {
        instruction.op().is_unit_rewritten()
    }

    /// The row that completes the activation (`Return`). `decode_flow` decodes it as its fallback arm
    /// (`Self::Return`, `invoke::ret`); the profile's tests name a second physical row by it.
    const RETURN_ROW: u16 = Op::Return as u16;
}


/// Whether an opcode index is the plain call `encode_call` writes: the stop the inliner takes
/// over once the call site is hot.
#[inline(always)]
pub const fn operation_is_call_index(opcode: u16) -> bool {
    opcode == Op::Call as u16
}
