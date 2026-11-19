//! Planning a `Call` for the b-jit: from the instruction and its site's call target record, one
//! chain of stencil parts (`quench-bjit/stencils/call_rows.rs` holds the rows and their register
//! and stack contract; `quench_bjit::call` names them).
//!
//! A call whose callee is small and monomorphic is not a chain at all: the stitch splices in the
//! callee's rows (`inline::InlineSite`), so no part is planned and the pinned registers stay where
//! they are. A call that returns its result (`return f(x)`) is followed by the row that completes
//! the activation with it.
//!
//! The chain, in layout order:
//!
//! 1. one `Spill` row per pinned register live at the call, writing it to its frame slot as a
//!    canonical tagged word (the callee and the helpers read the frame); a leading argument's spill
//!    that only the refusal path reads (its pinned value dead after the call, `refusal_only`) waits
//!    for the direct rows' refusal, the direct row reading the pin itself (`CallRecord::argument_places`);
//! 2. when the target's entry cell is stable (`Choice::Direct`), the direct row for the call's
//!    argument count and receiver binding (checked, or unchecked where a Stable fact holds the callee),
//!    whose record (`CallRecord`) the chain owns;
//! 3. the refusal spills, the helper row (`lane_body_call`), the direct row's refusal path and the whole call
//!    otherwise;
//! 4. one `reload_*` row per pinned register live after the call (every pinned register is dead
//!    across a call row).
//!
//! `plan_step` is the same chain for any instruction the helper runs in place (`steps`): spills,
//! the helper row, reloads. The calls of the other forms (`CallForm`) are this chain with their own
//! helper row and no direct row; a returning call has no reloads, the completion following it.
//!
//! Successors are chain-local: a part's `taken` is the index of the part it branches to, and
//! `parts.len()` means the part after the chain (the next instruction). A part's `exits`, by
//! exit kind, say which leave the linker patches in (`Exit`: `At(pc)` leaves at `pc` without
//! spilling, the spills already ran; `Status` is `quench_bjit::call::LEAVE_STATUS`, the (status,
//! word) in the first two pinned ints being the leave's result). The chain's record
//! lives in the chain: the code that uses the chain must not outlive it.
//!
//! Direct admission reads the cell's facts word once at plan time (`CellFacts`: its registers,
//! room, params and entry kind must describe the callee this site recorded) and the cell's
//! validity word becomes the row's expected word, so a republish or withdrawal that bumps the
//! validity sends the call to the helper row, which resolves the target anew.

pub(super) mod inline;
use super::{Exit, SLOT};
use super::super::direct_callee::DirectCallee;
use super::super::{FRAME_BYTES, LaneHelper, LaneHelpers};
use crate::bytecode::{Class, ImmediateLayout, Register, WideInstruction};
use crate::staged::call::{CellFacts, ENTRY_ACTIVATES, ENTRY_CELL};
use quench_bjit::call::{self, CallRow, ThisRow};
use quench_bjit::call_record::{CallRecord, MAX_DIRECT_ARGC, PLACED_ARGUMENTS};
use quench_bjit::numeric::{self, Kind, Place};
use quench_bjit::{HOLES, Stencil, padded_holes};

/// How a body makes a call, by the instruction's operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CallForm {
    /// `Call`, through its callee register and argument window: direct through the callee's entry
    /// cell where its site recorded a stable callee, else the call helper (`lane_body_call`).
    Plain,
    /// `Construct` over registers: the construct helper (`lane_body_construct`), which plans the
    /// receiver's fields or activates the constructor as the lane's own construction does.
    Construct,
    /// Every other form (method, this-method, array, spread and forwarding calls; a `super`
    /// or array-argument construction): the interpreter's own step made in place by the step helper,
    /// the call it makes running the callee as any call from a helper does. These sites record no
    /// callee to call by its entry cell, and a method call's callee is the method cache's lookup,
    /// which no register holds for a call row to read.
    Stepped,
}

impl CallForm {
    /// The form of the call `instruction` is, if a body's call chain makes it: a call form with no
    /// result flag left but the `this` binding (the return flag is `cover`'s). `CallKnown` is the Wasm
    /// compiler's call, which no JavaScript function holds, so it has no form. A call that binds the caller's
    /// `this` (`SET_THIS_REGISTER`, a `super()` call) or has an `eval` flag (which declares bindings in the
    /// caller's scope) is the step helper's own step made in place, a helper call row whatever its form: the
    /// helper binds the frame's `this` and publishes the receiver, or runs the eval in the frame.
    pub(super) fn of(instruction: WideInstruction) -> Option<Self> {
        let bound_this = if instruction.writes_current_this() { crate::bytecode::SET_THIS_REGISTER } else { 0 };
        let op = instruction.op();
        let calls = op.makes_script_call() && op.class() != Class::CallKnown && instruction.result_flags() & !bound_this == 0;
        calls.then(|| Self::shaped(instruction))
    }

    /// The form of a script call `instruction` (`of`).
    fn shaped(instruction: WideInstruction) -> Self {
        let in_place = instruction.writes_current_this() || instruction.resolves_dynamic_scope();
        match instruction.op().class() {
            Class::Call if !in_place => Self::Plain,
            Class::Construct if !in_place && !instruction.is_super_construct() && !ImmediateLayout::construct_array_arguments(instruction.imm()) => Self::Construct,
            _ => Self::Stepped,
        }
    }

    /// The helper whose row makes the call.
    pub(super) fn helper(self) -> LaneHelper {
        match self {
            Self::Plain => LaneHelper::Call,
            Self::Construct => LaneHelper::ConstructCall,
            Self::Stepped => LaneHelper::Step,
        }
    }
}

/// What a call instruction names, decoded once: the record the helper takes (`call_pc`), the
/// register that receives the result and the form. A plain call also names the callee and receiver
/// registers, the first argument register and the argument count (zero for the other forms, whose
/// helpers read their own operands from the record).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CallShape {
    pub(super) call_pc: u32,
    pub(super) callee: Register,
    pub(super) this: Register,
    pub(super) window: Register,
    pub(super) argc: u16,
    pub(super) result: Register,
    /// The call completes its caller's activation with its result (`return f(x)`): the row that
    /// follows it does (`steps::completing`), and nothing is live after it.
    pub(super) returning: bool,
    pub(super) form: CallForm,
}

impl CallShape {
    pub(super) fn of(call_pc: u32, instruction: WideInstruction, form: CallForm) -> Self {
        let imm = instruction.imm();
        let (result, returning) = (instruction.result_register(), instruction.returns_from_frame());
        match form {
            CallForm::Plain => Self {
                call_pc,
                callee: instruction.register_b(),
                this: instruction.register_c(),
                window: ImmediateLayout::call_window_base(imm),
                argc: ImmediateLayout::argument_count(imm),
                result,
                returning,
                form,
            },
            CallForm::Construct | CallForm::Stepped => Self { call_pc, callee: 0, this: 0, window: 0, argc: 0, result, returning, form },
        }
    }
}

/// What the call site's target record says of its callee, read once (`DirectFacts::of`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct DirectFacts {
    pub(super) callee: u64,
    pub(super) cell: usize,
    pub(super) validity: u64,
    pub(super) facts: CellFacts,
    pub(super) descriptor: usize,
    pub(super) captures: u64,
    /// The global object a sloppy callee binds an `undefined` or `null` receiver to.
    pub(super) global_this: Option<u64>,
    /// An arrow, which binds no receiver (`DirectCallee::lexical`).
    pub(super) lexical: bool,
    pub(super) registers: u16,
    /// A construction site's forwarding record: not a plain call.
    pub(super) construction: bool,
    pub(super) frame_bytes: u64,
    /// The site's callee register holds this callee by a Stable fact (`Callees::stable`): the body
    /// subscribes to the fact, so its change retires the body, and the row need not check the register.
    pub(super) held: bool,
}

impl DirectFacts {
    /// The facts of `callee`, whose cell's validity and facts words are read now; `held` is whether the site's
    /// callee register is a Stable fact's (`DirectFacts::held`).
    ///
    /// # Safety
    /// `callee.cell` is the address of the callee function's live entry cell.
    pub(super) unsafe fn of(callee: &DirectCallee, held: bool) -> Self {
        // SAFETY: the caller's contract; the words are at the designed cell's offsets.
        let word = |offset: i32| unsafe { ((callee.cell + offset as usize) as *const u64).read() };
        Self {
            callee: callee.callee.to_bits(),
            cell: callee.cell,
            validity: word(ENTRY_CELL.validity_offset),
            facts: CellFacts::of_word(word(ENTRY_CELL.facts_offset)),
            descriptor: std::ptr::from_ref(callee).addr(),
            captures: callee.capture_word(),
            global_this: callee.sloppy.map(|global| global.to_bits()),
            lexical: callee.lexical,
            registers: callee.registers,
            construction: callee.forwarder.is_some(),
            frame_bytes: FRAME_BYTES as u64,
            held,
        }
    }

    /// Whether the cell's facts describe this callee as a plain activation that takes `argc`
    /// arguments, so the stencil's admission (validity and room) is all that is left to check.
    fn describes(&self, argc: u16) -> bool {
        let facts = self.facts;
        facts.registers == self.registers && facts.room == self.registers && facts.params >= argc && u64::from(facts.entry) == ENTRY_ACTIVATES
    }
}

/// Why a call takes the helper row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Declined {
    /// The site recorded no stable target.
    NoTarget,
    /// More arguments than the direct rows pass (`MAX_DIRECT_ARGC`).
    Arguments,
    /// A construction, which the helper row's `lane_body_call` does not make either: the
    /// planner for `Construct` chooses.
    Construction,
    /// The call is not a plain `Call`: no direct row makes it (`CallForm`).
    Form,
    /// A callee with no registers has no window the room check can refuse.
    Window,
    /// The cell's facts do not describe the callee (not yet published, or another function's).
    Facts,
    /// A returning call: its callee's activation under a host that makes proper tail calls must not
    /// grow the stack, which only the helper row's own admission (`lane_returning_call_admitted`) knows.
    Returning,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Choice {
    Direct,
    Helper(Declined),
}

/// The row class of a call: direct when its target's cell is stable and describes the callee,
/// else the helper row, with the first reason it cannot be direct.
pub(super) fn choose(shape: &CallShape, direct: Option<&DirectFacts>) -> Choice {
    if shape.form != CallForm::Plain {
        return Choice::Helper(Declined::Form);
    }
    let Some(facts) = direct else { return Choice::Helper(Declined::NoTarget) };
    let declined = [
        (usize::from(shape.argc) > MAX_DIRECT_ARGC, Declined::Arguments),
        (facts.construction, Declined::Construction),
        (facts.registers == 0, Declined::Window),
        (!facts.describes(shape.argc), Declined::Facts),
        (shape.returning, Declined::Returning),
    ];
    declined.into_iter().find(|(declined, _)| *declined).map_or(Choice::Direct, |(_, why)| Choice::Helper(why))
}

/// One stencil of a chain with its hole values, chain-local `taken` successor and exits by kind.
pub(super) struct CallPart {
    pub(super) stencil: &'static Stencil,
    pub(super) holes: [u64; HOLES],
    pub(super) taken: Option<usize>,
    pub(super) exits: Vec<Exit>,
}

/// A planned call: its parts, the records its direct rows read (one per callee the site reached), and
/// the choice that made them.
pub(super) struct CallChain {
    pub(super) parts: Vec<CallPart>,
    pub(super) records: Vec<Box<CallRecord>>,
    pub(super) choice: Choice,
}

/// Everything a call is planned from: the pinned registers live at the call (spilled before it)
/// and live after it (reloaded after it), each with the register whose frame slot it uses.
pub(super) struct CallPlan<'a> {
    pub(super) shape: CallShape,
    /// Each callee the site reached, latest first (`Layout::callees`).
    pub(super) direct: &'a [DirectFacts],
    /// The address of `lane_body_call` (`helper_address`).
    pub(super) helper: usize,
    pub(super) spills: &'a [(Place, Register)],
    pub(super) reloads: &'a [(Place, Register)],
}

/// The address of `helper` in a process helper table; none for a linked or symbolic one.
pub(super) fn helper_address(helpers: LaneHelpers, helper: LaneHelper) -> Option<usize> {
    match helpers {
        LaneHelpers::Process(table) => Some(table[helper as usize].addr()),
        _ => None,
    }
}

fn slot(register: Register) -> u64 {
    u64::from(register) * SLOT
}

fn part(stencil: &'static Stencil, holes: [u64; HOLES], exits: Vec<Exit>) -> CallPart {
    CallPart { stencil, holes, taken: None, exits }
}

pub(super) fn spill_part(place: Place, register: Register) -> Result<CallPart, String> {
    let row = numeric::Row { kind: Kind::Spill, dst: Place::Mem, a: place, b: Place::Mem, speculate: false };
    Ok(part(numeric::stencil(row)?, padded_holes([slot(register)]), Vec::new()))
}

pub(super) fn reload_part(place: Place, register: Register) -> Result<CallPart, String> {
    Ok(part(call::stencil(CallRow::Reload(place))?, padded_holes([slot(register)]), Vec::new()))
}

fn helper_part(call_pc: u32, helper: usize) -> Result<CallPart, String> {
    if helper == 0 {
        return Err("call: no helper address".to_string());
    }
    Ok(part(call::stencil(CallRow::Helper)?, padded_holes([u64::from(call_pc), helper as u64]), vec![Exit::At(call_pc), Exit::Status]))
}


/// The pinned register holding argument `k` of the call, where the call passes it by place (the leading
/// `PLACED_ARGUMENTS` only) and the register is spilled: its place and its window register.
fn pinned_argument(plan: &CallPlan, k: usize) -> Option<(Place, Register)> {
    if k >= usize::from(plan.shape.argc).min(PLACED_ARGUMENTS) {
        return None;
    }
    let register = plan.shape.window + k as Register;
    let (place, _) = plan.spills.iter().copied().find(|&(_, spilled)| spilled == register)?;
    Some((place, register))
}

/// The place each leading argument is passed from (`CallRecord::argument_places`): its pinned register, else the window slot.
fn argument_places(plan: &CallPlan) -> [u8; PLACED_ARGUMENTS] {
    std::array::from_fn(|k| pinned_argument(plan, k).map_or(Place::Mem.code(), |(place, _)| place.code()))
}

/// Whether the spill of `register` serves only the refusal path: a pinned argument a direct row reads from its
/// pin, not live after the call (its window slot is not reloaded). A live one keeps its home before the call.
fn refusal_only(plan: &CallPlan, register: Register) -> bool {
    (0..PLACED_ARGUMENTS).any(|k| pinned_argument(plan, k).is_some_and(|(_, passed)| passed == register))
        && !plan.reloads.iter().any(|&(_, reloaded)| reloaded == register)
}

/// The direct row and the record it reads, whose address is its hole 0. `arguments` is the place each leading
/// argument is passed from (`argument_places`).
fn direct_part(shape: &CallShape, facts: &DirectFacts, arguments: [u8; PLACED_ARGUMENTS]) -> Result<(CallPart, Box<CallRecord>), String> {
    let record = Box::new(CallRecord {
        callee: facts.callee,
        cell: facts.cell as u64,
        expected: facts.validity,
        descriptor: facts.descriptor as u64,
        captures: facts.captures,
        global_this: facts.global_this.unwrap_or_default(),
        frame_bytes: facts.frame_bytes,
        callee_register: shape.callee,
        this_register: shape.this,
        args_register: shape.window,
        result_register: shape.result,
        argument_places: arguments,
    });
    let argc = u8::try_from(shape.argc).map_err(|e| e.to_string())?;
    let this = if facts.lexical { ThisRow::Lexical } else if facts.global_this.is_some() { ThisRow::Sloppy } else { ThisRow::Strict };
    let stencil = call::stencil(CallRow::Direct { argc, this, checked: !facts.held })?;
    Ok((part(stencil, quench_bjit::padded_holes([std::ptr::from_ref(&*record).addr() as u64, u64::from(shape.call_pc)]), vec![Exit::Status]), record))
}

/// The chain of a helper step at `pc` (any instruction whose operation the helper runs in place):
/// the spills, the helper row, the reloads.
pub(super) fn plan_step(pc: u32, helper: usize, spills: &[(Place, Register)], reloads: &[(Place, Register)]) -> Result<Vec<CallPart>, String> {
    let mut parts = spills.iter().map(|&(place, register)| spill_part(place, register)).collect::<Result<Vec<_>, _>>()?;
    parts.push(helper_part(pc, helper)?);
    for &(place, register) in reloads {
        parts.push(reload_part(place, register)?);
    }
    Ok(parts)
}

/// The chain of a call (see the module doc for its layout and successors). A polymorphic site's chain
/// holds one direct row per callee it reached, latest first: each refuses another callee to the next
/// (NEXT), the last to the helper row, so a site alternating between known callees calls each by its
/// entry, as the interpreter's own link does, and only an unknown one takes the helper.
pub(super) fn plan_call(plan: &CallPlan) -> Result<CallChain, String> {
    let choices: Vec<(Choice, &DirectFacts)> = plan.direct.iter().map(|facts| (choose(&plan.shape, Some(facts)), facts)).collect();
    let choice = choices.iter().map(|(choice, _)| *choice).find(|choice| *choice == Choice::Direct)
        .unwrap_or_else(|| choices.first().map_or_else(|| choose(&plan.shape, None), |(choice, _)| *choice));
    let arguments = argument_places(plan);
    let mut directs = Vec::new();
    for (_, facts) in choices.into_iter().filter(|(choice, _)| *choice == Choice::Direct) {
        directs.push(direct_part(&plan.shape, facts, arguments)?);
    }
    // The spills before the direct rows; the spills of pinned arguments that only the refusal path reads
    // (`refusal_only`) follow them, where the rows' NEXT falls to the helper row. Without a direct row, all come first.
    let (mut parts, mut refusal) = (Vec::new(), Vec::new());
    for &(place, register) in plan.spills {
        let spill = spill_part(place, register)?;
        if directs.is_empty() || !refusal_only(plan, register) { parts.push(spill) } else { refusal.push(spill) }
    }
    // Past every direct row, the refusal spills and the helper row that follows them: the first reload, or the next instruction.
    let past = parts.len() + directs.len() + refusal.len() + 1;
    let mut records = Vec::new();
    for (mut row, owned) in directs {
        row.taken = Some(past);
        parts.push(row);
        records.push(owned);
    }
    parts.extend(refusal);
    parts.push(helper_part(plan.shape.call_pc, plan.helper)?);
    for &(place, register) in plan.reloads {
        parts.push(reload_part(place, register)?);
    }
    Ok(CallChain { parts, records, choice })
}

#[cfg(test)]
mod tests {
    use super::*;
    use quench_bjit::call_record as protocol;

    const SHAPE: CallShape = CallShape { call_pc: 9, callee: 1, this: 2, window: 3, argc: 2, result: 6, returning: false, form: CallForm::Plain };

    fn facts(registers: u16, params: u16) -> DirectFacts {
        let cell = CellFacts { registers, room: registers, params, fill: crate::staged::call::FILL_ALL, entry: ENTRY_ACTIVATES as u8 };
        DirectFacts { callee: 7, cell: 0x1000, validity: 5, facts: cell, descriptor: 0x2000, captures: 0, global_this: None, lexical: false, registers, construction: false, frame_bytes: 32, held: false }
    }

    /// Each call opcode has its form, and an instruction that is no call, or carries a result flag, has none.
    #[test]
    fn each_call_opcode_has_its_form() {
        use crate::bytecode::{Op, RETURN_REGISTER};
        let form = |op| CallForm::of(WideInstruction::new(op, 1, 2, 3, 0));
        assert_eq!((form(Op::Call), form(Op::Construct)), (Some(CallForm::Plain), Some(CallForm::Construct)));
        for op in [Op::CallMethod, Op::CallThisMethod, Op::CallArray, Op::CallSpread, Op::CallForwardingArguments] {
            assert_eq!(form(op), Some(CallForm::Stepped), "{op:?}");
        }
        assert_eq!(form(Op::CallKnown), None, "the Wasm compiler's call is no JavaScript call form");
        assert_eq!(form(Op::Move), None);
        assert_eq!(CallForm::of(WideInstruction::new(Op::Call, 1 | RETURN_REGISTER, 2, 3, 0)), None);
        let binds_this = WideInstruction::new(Op::Call, 1 | crate::bytecode::SET_THIS_REGISTER, 2, 3, 0);
        assert_eq!(CallForm::of(binds_this), binds_this.writes_current_this().then_some(CallForm::Stepped), "a call that binds `this` is the helper step");
        let [plain, construct, stepped] = [CallForm::Plain, CallForm::Construct, CallForm::Stepped].map(|form| form.helper() as usize);
        assert!(plain != construct && construct != stepped && plain != stepped, "each form has its own helper");
    }

    #[test]
    fn a_stable_cell_that_describes_the_callee_is_called_directly() {
        assert_eq!(choose(&SHAPE, Some(&facts(4, 2))), Choice::Direct);
        assert_eq!(choose(&SHAPE, Some(&facts(4, 5))), Choice::Direct, "fewer arguments than params");
    }

    #[test]
    fn every_other_call_takes_the_helper_with_its_reason() {
        let helper = |why| Choice::Helper(why);
        assert_eq!(choose(&SHAPE, None), helper(Declined::NoTarget));
        let many = CallShape { argc: MAX_DIRECT_ARGC as u16 + 1, ..SHAPE };
        assert_eq!(choose(&many, Some(&facts(4, 9))), helper(Declined::Arguments));
        assert_eq!(choose(&SHAPE, Some(&DirectFacts { construction: true, ..facts(4, 2) })), helper(Declined::Construction));
        assert_eq!(choose(&SHAPE, Some(&facts(0, 2))), helper(Declined::Window));
        assert_eq!(choose(&SHAPE, Some(&facts(4, 1))), helper(Declined::Facts), "more arguments than params");
        let returning = CallShape { returning: true, ..SHAPE };
        assert_eq!(choose(&returning, Some(&facts(4, 2))), helper(Declined::Returning));
        for form in [CallForm::Construct, CallForm::Stepped] {
            assert_eq!(choose(&CallShape { form, ..SHAPE }, Some(&facts(4, 2))), helper(Declined::Form));
        }
        let unpublished = DirectFacts { facts: CellFacts { room: 9, ..facts(4, 2).facts }, ..facts(4, 2) };
        assert_eq!(choose(&SHAPE, Some(&unpublished)), helper(Declined::Facts), "the cell describes another window");
        let other_kind = DirectFacts { facts: CellFacts { entry: 1, ..facts(4, 2).facts }, ..facts(4, 2) };
        assert_eq!(choose(&SHAPE, Some(&other_kind)), helper(Declined::Facts), "not a plain activation");
    }

    fn plan<'a>(direct: Option<&'a DirectFacts>, spills: &'a [(Place, Register)], reloads: &'a [(Place, Register)]) -> CallPlan<'a> {
        let direct = direct.map_or(&[][..], std::slice::from_ref);
        CallPlan { shape: SHAPE, direct, helper: 0x4000, spills, reloads }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn a_direct_chain_is_spills_direct_helper_reloads_with_the_direct_row_skipping_the_helper() {
        let (spills, reloads) = ([(Place::Pin(0), 1), (Place::PinF(2), 5)], [(Place::Pin(0), 1)]);
        let direct = facts(4, 2);
        let chain = plan_call(&plan(Some(&direct), &spills, &reloads)).expect("a plan");
        assert_eq!(chain.choice, Choice::Direct);
        assert_eq!(chain.parts.len(), spills.len() + 2 + reloads.len());
        let (direct_at, helper_at) = (spills.len(), spills.len() + 1);
        assert_eq!(chain.parts[direct_at].taken, Some(helper_at + 1), "completion skips the helper row");
        assert_eq!(chain.parts[direct_at].exits, [Exit::Status]);
        assert_eq!(chain.parts[helper_at].exits, [Exit::At(SHAPE.call_pc), Exit::Status]);
        assert_eq!(chain.parts[helper_at].holes[..2], [u64::from(SHAPE.call_pc), 0x4000]);
        let record = chain.records.into_iter().next().expect("the direct row's record");
        assert_eq!(chain.parts[direct_at].holes[0], std::ptr::from_ref(&*record).addr() as u64);
        assert_eq!((record.callee, record.cell, record.expected, record.result_register), (7, 0x1000, 5, 6));
        assert_eq!(chain.parts[spills.len() + 2].holes[0], SLOT);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn a_helper_chain_has_no_record_and_its_helper_row_is_last_before_the_reloads() {
        let chain = plan_call(&plan(None, &[], &[(Place::PinF(0), 3)])).expect("a plan");
        assert_eq!((chain.choice, chain.records.is_empty(), chain.parts.len()), (Choice::Helper(Declined::NoTarget), true, 2));
        assert!(chain.parts.iter().all(|part| part.taken.is_none()));
        let nowhere = CallPlan { helper: 0, ..plan(None, &[], &[]) };
        assert!(plan_call(&nowhere).is_err(), "a helper row needs its address");
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn a_step_chain_is_spills_helper_reloads() {
        let (spills, reloads) = ([(Place::Pin(0), 1)], [(Place::PinF(1), 5), (Place::Pin(0), 1)]);
        let parts = plan_step(9, 0x4000, &spills, &reloads).expect("a chain");
        assert_eq!(parts.len(), spills.len() + 1 + reloads.len());
        assert_eq!(parts[spills.len()].holes[..2], [9, 0x4000]);
        assert_eq!(parts[spills.len()].exits, [Exit::At(9), Exit::Status]);
        assert!(plan_step(9, 0, &[], &[]).is_err(), "a helper row needs its address");
    }

    #[test]
    fn a_sloppy_callee_takes_the_sloppy_row_with_its_global() {
        let sloppy = DirectFacts { global_this: Some(0xabc), ..facts(4, 2) };
        let chain = plan_call(&plan(Some(&sloppy), &[], &[])).expect("a plan");
        assert_eq!(chain.records.into_iter().next().expect("a record").global_this, 0xabc);
        let name = CallRow::Direct { argc: 2, this: ThisRow::Sloppy, checked: true }.name().expect("a row");
        assert!(std::ptr::eq(chain.parts[0].stencil, call::stencil(CallRow::Direct { argc: 2, this: ThisRow::Sloppy, checked: true }).expect(&name)));
    }

    /// A pinned argument dead after the call is read from its pin by the direct row (its place in the record), and its
    /// spill moves behind the direct rows, to the refusal path; a live one keeps its spill before the call.
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn a_dead_pinned_argument_is_passed_by_its_pin_and_spilled_on_refusal_only() {
        let spills = [(Place::Pin(1), SHAPE.window), (Place::PinF(0), 9)];
        let reloads: [(Place, Register); 0] = [];
        let direct = facts(4, 2);
        let chain = plan_call(&plan(Some(&direct), &spills, &reloads)).expect("a plan");
        assert_eq!(chain.parts.len(), 4, "the kept spill, the direct row, the refusal spill, the helper row");
        assert_eq!(chain.parts[1].taken, Some(4), "completion skips the refusal spill and the helper row");
        assert_eq!(chain.parts[2].holes[0], slot(SHAPE.window), "the refusal spill is the window argument's");
        let record = chain.records.into_iter().next().expect("the direct row's record");
        assert_eq!(record.argument_places, [Place::Pin(1).code(), Place::Mem.code()]);
    }

    /// The call protocol the stencils are compiled with (`quench_bjit::call_record`,
    /// `stencils/layout.rs`) is the runtime's own.
    #[test]
    fn call_protocol_matches_the_runtime() {
        use super::super::super as compiled;
        use compiled::entry_cell::{EntryCell, LANE_BODY_NOT_ENTERED};
        #[allow(dead_code)]
        mod stencil_layout {
            include!("../../../../../quench-bjit/stencils/layout.rs");
        }
        use stencil_layout as mirror;
        assert_eq!(
            [compiled::LANE_CALL_COMPLETED, compiled::LANE_CALL_MISSED, compiled::LANE_CALL_LEFT, compiled::LANE_CALL_TERMINATED, compiled::LANE_CALL_BRANCHED, compiled::LaneExit::Callee.word()],
            [protocol::LANE_CALL_COMPLETED, protocol::LANE_CALL_MISSED, protocol::LANE_CALL_LEFT, protocol::LANE_CALL_TERMINATED, protocol::LANE_CALL_BRANCHED, protocol::CALLEE_EXIT]
        );
        assert_eq!(
            [compiled::RETURN_STATUS_BASE, compiled::LANE_BODY_RETURNED, LANE_BODY_NOT_ENTERED, compiled::NATIVE_JS_STOPPED, compiled::TERMINATED_STATUS as u32],
            [protocol::GENERAL_RETURN_STATUS_BASE, protocol::LANE_BODY_RETURNED, protocol::LANE_BODY_NOT_ENTERED, protocol::NATIVE_JS_STOPPED, protocol::TERMINATED_STATUS]
        );
        assert_eq!(protocol::REGISTER_BYTES, std::mem::size_of::<crate::value::Value>());
        assert_eq!(mirror::FRAME_CONTEXT_WORDS, crate::vm::register_file::FRAME_CONTEXT);
        use crate::vm::dispatch_fast::LaneRecoverySpace;
        assert_eq!(
            [std::mem::offset_of!(EntryCell, entry), std::mem::offset_of!(EntryCell, recovery), std::mem::offset_of!(EntryCell, body), std::mem::offset_of!(LaneRecoverySpace, values), std::mem::offset_of!(LaneRecoverySpace, activations)],
            [mirror::CELL_ENTRY_AT, mirror::CELL_RECOVERY_AT, mirror::CELL_BODY_AT, mirror::RECOVERY_VALUES_AT, mirror::RECOVERY_ACTIVATIONS_AT]
        );
    }
}
