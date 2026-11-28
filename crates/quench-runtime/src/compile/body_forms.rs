//! The body forms of script functions, classified once from the function record and its bytecode: the entry a
//! call of a body takes (`body_entry`), the read a body returns (`returning_read`), and whether a
//! call of a body forwards its arguments (`forwarding_form`, `forwards_args`). The VM classifies a target with
//! these at link time (`quench-runtime-engine`'s `call_body_forms` and `dispatch_fast::closure_targets`), and the
//! AOT facts read the same functions (`AotFacts::body_entry` and its siblings), so both name one classification.

use crate::bytecode::{CallPlan, Constant, FieldBase, FieldLookup, Function, ImmediateRole, OperandKind, RETURN_REGISTER, Traits, WideInstruction};
use crate::vm::engine_types::{CallEntry, Framed, Frameless};

/// The operator `!==`, as the oxc operator's index in a binary record's immediate.
pub const STRICT_INEQUALITY_OPERATOR: u32 = oxc_ast::ast::BinaryOperator::StrictInequality as u32;

/// The base of a returned field read: `this`, or a parameter by its register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadBase {
    This,
    Parameter(u16),
}

/// A body that is `return this.f` or `return p.f` (`returning_read`): the field's atom and its base.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReturningRead {
    pub atom: u32,
    pub base: ReadBase,
}

/// Whether `instruction` returns a field read from the frame (`Traits::RETURNING_FIELD`): the one test the VM's
/// `returning_kind` and the body entry share.
pub fn returning_field(instruction: WideInstruction) -> bool {
    instruction.returns_from_frame() && instruction.op().traits().has(Traits::RETURNING_FIELD)
}

/// The entry a call of `body` takes in its call form (`frameless`, from the callee's composed signature,
/// `call_plan::entry_form`), before what its call needs (a frame, an environment, kept arguments) decides further.
/// The form is the signature's, never the body's: a framed call form takes `Framed::Activates`, the framed
/// shape that runs any body, so the body's frameless answers are recognized only for a frameless form. A
/// frameless form takes the first answer its body has of those a call answers without activating it; a body
/// with none takes `Framed::Activates` here, and the VM's `leaf_body` makes it `CallEntry::leaf` where it proves
/// a leaf; where it proves none, the call keeps a frame although the signature is frameless, since no frameless
/// shape runs such a body. A class constructor's calls throw on the general path, so they take a frame; only its
/// constructions activate frameless (`script_construct_miss`). A frameless form whose body answers none of those
/// takes `Frameless::Native` where `native` (the body is published with native code that calls only natively),
/// before `Framed::Activates`.
pub fn body_entry(body: &Function, constants: &[Constant], frameless: bool, native: bool) -> CallEntry {
    if body.is_class_constructor {
        CallEntry::framed(Framed::Frame)
    } else if !frameless {
        CallEntry::framed(Framed::Activates)
    } else if body.code.first().is_some_and(|packed| returning_field(body.widened(*packed))) {
        CallEntry::frameless(Frameless::Reads)
    } else if capture_read_body(body) {
        CallEntry::frameless(Frameless::ReadsCapture)
    } else if this_read_body(body) {
        CallEntry::frameless(Frameless::ReadsThis)
    } else if appends_capture(body, constants) {
        CallEntry::frameless(Frameless::AppendsCapture)
    } else if native {
        CallEntry::frameless(Frameless::Native)
    } else {
        CallEntry::framed(Framed::Activates)
    }
}

/// The body `return this.f` or `return p.f` (`ReturningRead`): its first instruction is a returning field read
/// (`returning_field`) of `this` or of a parameter. A returning first instruction leaves no path to the rest of the
/// code, so the body is that read alone, unless a handler can reach the rest (then None).
pub fn returning_read(body: &Function) -> Option<ReturningRead> {
    if !body.handlers.is_empty() {
        return None;
    }
    let first = body.widened(*body.code.first()?);
    if first.op().immediate_role() != ImmediateRole::FieldLookup || !returning_field(first) {
        return None;
    }
    let FieldLookup::Atom { atom, base, .. } = first.field_lookup()? else {
        return None;
    };
    let base = if base.0 == FieldBase::THIS.0 {
        ReadBase::This
    } else {
        ReadBase::Parameter(base.register_index().filter(|&register| register < body.params)?)
    };
    Some(ReturningRead { atom, base })
}

/// The framed form a call of `body` with `plan` takes when the call forwards its arguments: `Gathers` for a rest
/// body (its rest parameter gathers the values past the others), `Keeps` for a body whose plan keeps the
/// arguments for a forwarded `arguments` (`CallPlan::keeps_arguments`), else None. `ScriptCallTarget::forwards` is
/// true for these; the VM's `target_entry` takes this form after the settling, class constructor and environment forms.
pub fn forwarding_form(body: &Function, plan: &CallPlan) -> Option<Framed> {
    if body.rest {
        Some(Framed::Gathers)
    } else if plan.keeps_arguments {
        Some(Framed::Keeps)
    } else {
        None
    }
}

/// Whether a call of `body` forwards its arguments in every run (`ScriptCallTarget::forwards`), from the function
/// and its plan alone: `forwarding_form` holds, and no other form of the call takes its place. A class constructor
/// takes its own form. A settling body (`settles_in_loop`), an environment activation (`environment_activation`,
/// `suspended_environment`) and a body that is not registers-only take a settling, environment or frame form,
/// so they answer false. A numeric caller's frame (`numeric`) is decided by the caller, not here.
pub fn forwards_args(body: &Function, plan: &CallPlan) -> bool {
    forwarding_form(body, plan).is_some()
        && !body.is_class_constructor
        && plan.registers_only
        && !plan.settles_in_loop
        && !plan.environment_activation
        && !plan.suspended_environment
}

/// The first instruction of `body` and the one after it, when it has two.
fn leading_pair(body: &Function) -> Option<[WideInstruction; 2]> {
    let [first, second, ..] = body.code.as_slice() else {
        return None;
    };
    Some([first, second].map(|packed| body.widened(*packed)))
}

/// Whether `body` is `LoadCapture` and `Return` of it (`CallEntry::frameless(Frameless::ReadsCapture)`), as
/// `() => binding` compiles.
fn capture_read_body(body: &Function) -> bool {
    leading_pair(body).is_some_and(|[load, done]| {
        crate::staged::reads_capture(load.op()) && !load.returns_from_frame() && crate::staged::returns(done.op()) && done.a() == load.a()
    })
}

/// Whether `body` is `LoadThis` and `Return` of it (`CallEntry::frameless(Frameless::ReadsThis)`): a function's own
/// `this`, which neither an arrow's lexical one nor a constructor's binding complicates.
fn this_read_body(body: &Function) -> bool {
    !body.is_arrow
        && !body.is_class_constructor
        && leading_pair(body).is_some_and(|[load, done]| {
            crate::staged::loads_this(load.op()) && !load.returns_from_frame() && crate::staged::returns(done.op()) && done.a() == load.a()
        })
}

/// Positions in an appending body (`appends_capture`).
const RESULT_PC: usize = 5;
const RETURN_PC: usize = RESULT_PC + 1;

/// The parameter an appending body appends: its register is the parameter's own.
const CHUNK_PARAMETER: u16 = 0;

/// Whether `body` is `null !== chunk && (binding += chunk); return true;` over its first
/// parameter and one capture of its own environment, as each instruction's role says
/// (`CallEntry::frameless(Frameless::AppendsCapture)`). The VM's `capture_append::run` completes such a call.
pub fn appends_capture(body: &Function, constants: &[Constant]) -> bool {
    if body.params == 0 || !body.handlers.is_empty() {
        return false;
    }
    let Some(&[test, branch, load, add, store, result, done]) = body.code.get(..=RETURN_PC) else {
        return false;
    };
    let [test, branch, load, add, store, result, done] =
        [test, branch, load, add, store, result, done].map(|packed| body.widened(packed));
    tests_chunk_is_not_null(test, constants)
        && skips_append_unless(branch, test)
        && loads_own_capture(load)
        && appends_chunk(add, load)
        && stores_back(store, add, load)
        && returns_true(result, done, constants)
}

/// A write that leaves the chunk parameter's register for the append to read.
fn spares_chunk(instruction: WideInstruction) -> bool {
    instruction.result_flags() == 0 && instruction.result_register() != CHUNK_PARAMETER
}

/// `null !== chunk`, into a register other than the chunk's.
fn tests_chunk_is_not_null(test: WideInstruction, constants: &[Constant]) -> bool {
    crate::staged::binary(test.op())
        && spares_chunk(test)
        && test.binary_operator() == STRICT_INEQUALITY_OPERATOR
        && test.operand_b().register_index() == Some(CHUNK_PARAMETER)
        && test.operand_c().kind() == Some(OperandKind::Constant)
        && matches!(constants.get(usize::from(test.operand_c().payload())), Some(Constant::Null))
}

/// A false test jumps over the append to the result.
fn skips_append_unless(branch: WideInstruction, test: WideInstruction) -> bool {
    crate::staged::jumps_when_false(branch.op())
        && branch.register_a() == test.result_register()
        && branch.jump_target() as usize == RESULT_PC
}

/// A capture of the body's own environment, into a register other than the chunk's.
fn loads_own_capture(load: WideInstruction) -> bool {
    crate::staged::reads_capture(load.op()) && spares_chunk(load) && load.capture_depth() == 0
}

/// `binding + chunk`, in that order.
fn appends_chunk(add: WideInstruction, load: WideInstruction) -> bool {
    crate::staged::binary(add.op())
        && add.result_flags() == 0
        && add.binary_operator() == oxc_ast::ast::BinaryOperator::Addition as u32
        && add.operand_b().register_index() == Some(load.result_register())
        && add.operand_c().register_index() == Some(CHUNK_PARAMETER)
}

/// The sum, stored to the capture the load read.
fn stores_back(store: WideInstruction, add: WideInstruction, load: WideInstruction) -> bool {
    crate::staged::writes_capture(store.op()) && store.register_a() == add.result_register() && store.imm() == load.imm()
}

/// `return true`, whether or not the append ran.
fn returns_true(result: WideInstruction, done: WideInstruction, constants: &[Constant]) -> bool {
    crate::staged::loads_constant(result.op())
        && result.result_flags() & !RETURN_REGISTER == 0
        && matches!(constants.get(result.constant_index()), Some(Constant::Boolean(true)))
        && crate::staged::returns(done.op())
        && done.register_a() == result.result_register()
}
