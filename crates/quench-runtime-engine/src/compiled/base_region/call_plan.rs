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
//!    runs here too although the direct row reads the pin itself (`CallRecord::argument_places`): the
//!    rows' refusal passes every pinned register dead, so a spill behind them would write junk;
//! 2. when the target's entry cell is stable (`Choice::Direct`), the direct row for the call's
//!    argument count and receiver binding, whose record (`CallRecord`) the chain owns;
//! 3. the helper row (`body_call_helper`), the direct row's refusal path and the whole call
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

#[allow(unused_imports)]
use crate::ext_traits::*;
pub(crate) mod inline;
pub(crate) mod chain;
pub(crate) mod builtin;
use builtin::BuiltinCall;
pub(crate) use chain::ChainRead;
use chain::chained_reads;
use super::{Exit, NO_SYMBOLS, SLOT, Symbolic};
use super::super::direct_callee::DirectCallee;
use super::super::{LaneHelper, LaneHelpers};
use quench_runtime::bytecode::{ImmediateLayout, Register, WideInstruction};
use quench_runtime::staged::call::{CellFacts, ENTRY_ACTIVATES, ENTRY_CELL};
use quench_runtime::staged::frame::{ARGC_SHIFT, NATIVE_FLAG, PLAIN_FLAG};
use quench_bjit::call::{self, CallRow, ThisRow};
use quench_bjit::call_record::{CallRecord, DirectHole, HelperHole, MAX_DIRECT_ARGC, PLACED_ARGUMENTS};
use quench_bjit::numeric::{self, Kind, Place};
use quench_bjit::{HOLES, Stencil, padded_holes};
use crate::aot_image::Symbol;

/// How a body makes a call, by the instruction's operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CallForm {
    /// `Call`, through its callee register and argument window: direct through the callee's entry
    /// cell where its site recorded a stable callee, else the call helper (`body_call_helper`).
    Plain,
    /// `Construct` over registers: the construct helper (`body_construct_helper`), which plans the
    /// receiver's fields or activates the constructor as the lane's own construction does.
    Construct,
    /// Every other form (method, this-method, array, spread and forwarding calls; a `super`
    /// or array-argument construction): the interpreter's own step made in place by the step helper,
    /// the call it makes running the callee as any call from a helper does. These sites record no
    /// callee to call by its entry cell, and a method call's callee is the method cache's lookup,
    /// which no register holds for a call row to read.
    Stepped,
    /// `CallMethod` with a window of consecutive argument registers (`method_shape`): a site read writes the
    /// method's value to a scratch register, which the checked direct row compares with the recorded callee.
    Method,
}

/// A method call's site read (`plan::method_read`): the cache entry that answers the method, and the scratch register
/// the read writes the callee to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Method {
    pub(crate) cache: u16,
    pub(crate) scratch: Register,
}

impl CallForm {
    /// The form of the call `instruction` is, if a body's call chain makes it: a call form with no
    /// result flag left but the `this` binding (the return flag is `cover`'s). `CallKnown` is the Wasm
    /// compiler's call, which no JavaScript function holds, so it has no form. A call that binds the caller's
    /// `this` (`SET_THIS_REGISTER`, a `super()` call) or has an `eval` flag (which declares bindings in the
    /// caller's scope) is the step helper's own step made in place, a helper call row whatever its form: the
    /// helper binds the frame's `this` and publishes the receiver, or runs the eval in the frame.
    pub(crate) fn of(instruction: WideInstruction) -> Option<Self> {
        let bound_this = if instruction.writes_current_this() { quench_runtime::bytecode::SET_THIS_REGISTER } else { 0 };
        let op = instruction.op();
        let calls = op.makes_script_call() && !crate::compiled::meaning_compiled::is_call_known(instruction) && instruction.result_flags() & !bound_this == 0;
        calls.then(|| Self::shaped(instruction))
    }

    /// The form of a script call `instruction` (`of`): the staged `Invocation` of it, a plain call through a callee register
    /// and a window of arguments, or a construction of the same operands. A record that binds the caller's `this` or resolves
    /// a dynamic scope (an eval) is the step helper's own step made in place, whatever its operands; every other form is stepped.
    fn shaped(instruction: WideInstruction) -> Self {
        use quench_runtime::staged::{Arguments, Callee, DecodeContext, Invocation, Receiver};
        let in_place = instruction.writes_current_this() || instruction.resolves_dynamic_scope();
        match Invocation::of(instruction, DecodeContext::EMPTY) {
            Some(Invocation { callee: Callee::Register(_), receiver: Receiver::Register(_), arguments: Arguments::Window { .. }, forwards: false, .. }) if !in_place => Self::Plain,
            Some(Invocation { receiver: Receiver::Allocated, arguments: Arguments::Window { .. }, .. }) if !in_place => Self::Construct,
            _ => Self::Stepped,
        }
    }

    /// The helper whose row makes the call.
    pub(crate) fn helper(self) -> LaneHelper {
        match self {
            Self::Plain => LaneHelper::Call,
            Self::Construct => LaneHelper::ConstructCall,
            Self::Stepped | Self::Method => LaneHelper::Step,
        }
    }
}

/// What a call instruction names, decoded once: the record the helper takes (`call_pc`), the
/// register that receives the result and the form. A plain call also names the callee and receiver
/// registers, the first argument register and the argument count (zero for the other forms, whose
/// helpers read their own operands from the record).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CallShape {
    pub(crate) call_pc: u32,
    pub(crate) callee: Register,
    pub(crate) this: Register,
    pub(crate) window: Register,
    pub(crate) argc: u16,
    pub(crate) result: Register,
    /// The call completes its caller's activation with its result (`return f(x)`): the row that
    /// follows it does (`steps::completing`), and nothing is live after it.
    pub(crate) returning: bool,
    pub(crate) form: CallForm,
    /// The method site read a `Method` call makes before its direct row (`None` for the other forms).
    pub(crate) method: Option<Method>,
    /// The opcode of the call record, which keys the fast-row counter of a planned direct call (`note_fast`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) op: quench_runtime::bytecode::Op,
}

impl CallShape {
    pub(crate) fn of(call_pc: u32, instruction: WideInstruction, form: CallForm) -> Self {
        let imm = instruction.imm();
        let (result, returning, op) = (instruction.result_register(), instruction.returns_from_frame(), instruction.op());
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
                method: None,
                op,
            },
            CallForm::Construct | CallForm::Stepped | CallForm::Method => Self { call_pc, callee: 0, this: 0, window: 0, argc: 0, result, returning, form, method: None, op },
        }
    }
}

/// The shape of the method call `instruction` at `call_pc` (`CallMethod`, a receiver register and a method site) as a
/// `Method` chain: its receiver is the direct row's `this`, its callee the `scratch` register the site read writes,
/// its arguments a window of consecutive registers the direct row reads. `None` for any other call, or one whose
/// arguments are not consecutive (`Arguments::List` is a list of registers, not a window).
pub(crate) fn method_shape(program: &quench_runtime::bytecode::ResidualProgram, instruction: WideInstruction, call_pc: u32, scratch: Register, returning: bool) -> Option<CallShape> {
    if !quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::MethodCall) || instruction.writes_current_this() || instruction.resolves_dynamic_scope()
        || instruction.direct_eval() || instruction.parameter_eval() {
        return None;
    }
    let context = quench_runtime::staged::DecodeContext::of(program);
    let invocation = quench_runtime::staged::Invocation::of(instruction, context)?;
    let quench_runtime::staged::Receiver::Register(receiver) = invocation.receiver else { return None };
    let quench_runtime::staged::Arguments::List(list) = invocation.arguments else { return None };
    let start = list.start as usize;
    let registers = program.method_arguments.get(start..start + usize::from(list.count))?;
    let window = registers.first().copied().unwrap_or(0);
    if !registers.iter().enumerate().all(|(k, &register)| register == window + k as Register) {
        return None;
    }
    let quench_runtime::staged::Callee::Method { site, .. } = invocation.callee else { return None };
    let cache = context.method_site(site as usize)?.cache;
    Some(CallShape {
        call_pc, callee: scratch, this: receiver, window, argc: list.count, result: instruction.result_register(), returning,
        form: CallForm::Method, method: Some(Method { cache, scratch }), op: instruction.op(),
    })
}

/// What the call site's target record says of its callee, read once (`DirectFacts::of`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DirectFacts {
    pub(crate) callee: u64,
    pub(crate) cell: usize,
    pub(crate) validity: u64,
    pub(crate) facts: CellFacts,
    /// The caller's call record and base, and the callee's function: the header words (`CallRecord::call`,
    /// `caller_base`, `packed`).
    pub(crate) call: u64,
    pub(crate) caller_base: u64,
    pub(crate) function: u32,
    pub(crate) captures: u64,
    /// The callee's closure environment and captures pointer (`CallRecord::env`, `capture`).
    pub(crate) env: u64,
    pub(crate) capture: u64,
    /// The global object a sloppy callee binds an `undefined` or `null` receiver to.
    pub(crate) global_this: Option<u64>,
    /// An arrow, which binds no receiver (only an AOT image's proven row names one).
    pub(crate) lexical: bool,
    pub(crate) registers: u16,
    /// A construction site's forwarding record: not a plain call.
    pub(crate) construction: bool,
    /// The site's callee register holds this callee by a Stable fact (`Callees::stable`): the body
    /// subscribes to the fact, so its change retires the body, and the row need not check the register.
    pub(crate) held: bool,
    /// The address of the callee's `DirectCallee`, which the body owns (`Callees::owned`, moved into the
    /// published method or region whole, so the address holds): the native header's descriptor word
    /// (`CallRecord::descriptor`), as the u-jit's `push_native` writes it. Zero where none is known yet
    /// (an AOT record before its binding, `aot_image::bound`), which the row refuses to the helper.
    pub(crate) descriptor: u64,
}

impl DirectFacts {
    /// The facts of `callee`, whose cell's validity and facts words are read now; `held` is whether the site's
    /// callee register is a Stable fact's (`DirectFacts::held`).
    ///
    /// # Safety
    /// `callee.cell` is the address of the callee function's live entry cell.
    pub(crate) unsafe fn of(callee: &DirectCallee, held: bool) -> Self {
        // SAFETY: the caller's contract; the words are at the designed cell's offsets.
        let word = |offset: i32| unsafe { ((callee.cell + offset as usize) as *const u64).read() };
        Self {
            callee: callee.callee.to_bits(),
            cell: callee.cell,
            validity: word(ENTRY_CELL.validity_offset),
            facts: CellFacts::of_word(word(ENTRY_CELL.facts_offset)),
            call: callee.call as u64,
            caller_base: callee.caller_base as u64,
            function: callee.target.function,
            captures: callee.capture_word(),
            env: callee.target.context_words().0,
            capture: callee.target.context_words().1,
            global_this: callee.sloppy.map(|global| global.to_bits()),
            // A live callee is never an arrow: no direct row binds a lexical `this` (only an AOT image's proven row does).
            lexical: false,
            registers: callee.registers,
            construction: callee.forwarder.is_some(),
            held,
            descriptor: std::ptr::from_ref(callee).addr() as u64,
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
pub(crate) enum Declined {
    /// The site recorded no stable target.
    NoTarget,
    /// More arguments than the direct rows pass (`MAX_DIRECT_ARGC`).
    Arguments,
    /// A construction, which the helper row's `body_call_helper` does not make either: the
    /// planner for `Construct` chooses.
    Construction,
    /// The call is not a plain `Call`: no direct row makes it (`CallForm`).
    Form,
    /// A callee with no registers has no window the room check can refuse.
    Window,
    /// The cell's facts do not describe the callee (not yet published, or another function's).
    Facts,
    /// A returning call: its callee's activation under a host that makes proper tail calls must not
    /// grow the stack, which only the helper row's own admission (`returning_call_admitted`) knows.
    Returning,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    Direct,
    Helper(Declined),
}

/// The row class of a call: direct when its target's cell is stable and describes the callee,
/// else the helper row, with the first reason it cannot be direct.
pub(crate) fn choose(shape: &CallShape, direct: Option<&DirectFacts>) -> Choice {
    let method = shape.form == CallForm::Method && shape.method.is_some();
    if shape.form != CallForm::Plain && !method {
        return Choice::Helper(Declined::Form);
    }
    let Some(facts) = direct else { return Choice::Helper(Declined::NoTarget) };
    let declined = [
        (usize::from(shape.argc) > MAX_DIRECT_ARGC, Declined::Arguments),
        (facts.construction, Declined::Construction),
        (facts.registers == 0, Declined::Window),
        (!facts.describes(shape.argc), Declined::Facts),
    ];
    // A returning call is a candidate only where the helper would admit it: `lane_direct_callees` withholds
    // returning sites under a host with proper tail calls and a strict body (`lane_returning_call_admitted`).
    declined.into_iter().find(|(declined, _)| *declined).map_or(Choice::Direct, |(_, why)| Choice::Helper(why))
}

/// One stencil of a chain with its hole values, chain-local `taken` successor and exits by kind.
pub(crate) struct CallPart {
    pub(crate) stencil: &'static Stencil,
    pub(crate) holes: [u64; HOLES],
    pub(crate) taken: Option<usize>,
    pub(crate) exits: Vec<Exit>,
    /// The holes whose run-time addresses are symbols the loader resolves, each with its symbol
    /// (`HelperHole::Symbol`, the generic row's entry and operand record).
    pub(crate) symbols: Symbolic,
}

/// A planned call: its parts, the records its direct rows read (one per callee the site reached), and
/// the choice that made them.
pub(crate) struct CallChain {
    pub(crate) parts: Vec<CallPart>,
    pub(crate) records: Vec<Box<CallRecord>>,
    pub(crate) choice: Choice,
}

/// Everything a call is planned from: the pinned registers live at the call (spilled before it)
/// and live after it (reloaded after it), each with the register whose frame slot it uses.
pub(crate) struct CallPlan<'a> {
    pub(crate) shape: CallShape,
    /// Each callee the site reached, latest first (`Layout::callees`).
    pub(crate) direct: &'a [DirectFacts],
    /// The address of `body_call_helper` (`helper_address`).
    pub(crate) helper: HelperPlace,
    pub(crate) spills: &'a [(Place, Register)],
    pub(crate) reloads: &'a [(Place, Register)],
    /// The arguments the caller proves Numbers (bit `k`: argument `k`, `call_args::proven_arguments`), which the direct row
    /// passes on to the callee's proven-arguments entry where its guards are covered (`CallRecord::proven_args`).
    pub(crate) proven_args: u64,
    /// The site read a `Method` call's direct rows follow (`plan::method_read`): its stencil and holes. Planned only
    /// where a direct row is, after the spills; its miss goes to the helper row behind the direct rows.
    pub(crate) read: Option<(&'static Stencil, [u64; HOLES])>,
    /// The prototype-held entries of a method site (`plan::chain_reads`), each a read of the method at a receiver shape
    /// with the value its holder held when planned. Where one pairs with a callee the site reached, the chain is its reads
    /// and direct rows (`chained_reads`), and `read` is not planned.
    pub(crate) chain: Vec<ChainRead>,
    /// The builtin the callee is, where the site's native callee is one the b-jit runs on a row (`builtin::of`).
    pub(crate) builtin: Option<BuiltinCall>,
}

/// Where a helper row's address goes: the address itself in a process helper table, or the symbol
/// an image's loader resolves to it (`LaneHelpers::Ahead`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HelperPlace {
    Address(usize),
    Symbol(LaneHelper),
}

/// The helper's place for `helper` under `helpers`: none for a linked or a frameless one.
pub(crate) fn helper_address(helpers: LaneHelpers, helper: LaneHelper) -> Option<HelperPlace> {
    match helpers {
        LaneHelpers::Process(table) => Some(HelperPlace::Address(table[helper as usize].addr())),
        LaneHelpers::Ahead => Some(HelperPlace::Symbol(helper)),
        _ => None,
    }
}

fn slot(register: Register) -> u64 {
    u64::from(register) * SLOT
}

fn part(stencil: &'static Stencil, holes: [u64; HOLES], exits: Vec<Exit>) -> CallPart {
    CallPart { stencil, holes, taken: None, exits, symbols: NO_SYMBOLS }
}

pub(crate) fn spill_part(place: Place, register: Register) -> Result<CallPart, String> {
    let row = numeric::Row { kind: Kind::Spill, dst: Place::Mem, a: place, b: Place::Mem, speculate: false };
    Ok(part(numeric::stencil(row)?, padded_holes([slot(register)]), Vec::new()))
}

pub(crate) fn reload_part(place: Place, register: Register) -> Result<CallPart, String> {
    Ok(part(call::stencil(CallRow::Reload(place))?, padded_holes([slot(register)]), Vec::new()))
}

fn helper_part(call_pc: u32, helper: HelperPlace, packed: Option<[u64; 2]>) -> Result<CallPart, String> {
    let (address, symbols) = match helper {
        HelperPlace::Address(0) => return Err("call: no helper address".to_string()),
        HelperPlace::Address(address) => (address as u64, NO_SYMBOLS),
        HelperPlace::Symbol(helper) => (0, [Some((HelperHole::Helper as u8, Symbol::Helper(helper as u32))), None]),
    };
    let exits = vec![Exit::At(call_pc), Exit::Status];
    let mut holes = [0; HOLES];
    holes[HelperHole::CallPc as usize] = u64::from(call_pc);
    holes[HelperHole::Helper as usize] = address;
    let row = match packed {
        None => part(call::stencil(CallRow::Helper)?, holes, exits),
        Some([first, imm]) => {
            holes[HelperHole::PackedFirst as usize] = first;
            holes[HelperHole::PackedImm as usize] = imm;
            part(call::stencil(CallRow::HelperPacked)?, holes, exits)
        }
    };
    Ok(CallPart { symbols, ..row })
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

/// The direct row and the record it reads, whose address is its hole 0. `arguments` is the place each leading
/// argument is passed from (`argument_places`).
fn direct_part(shape: &CallShape, facts: &DirectFacts, arguments: [u8; PLACED_ARGUMENTS], proven_args: u64) -> Result<(CallPart, Box<CallRecord>), String> {
    // The callee's plain header: a native frame (NATIVE, as the u-jit's `push_native` writes it) of `argc`
    // arguments in the callee's function, which `native_frames::link_header` links by its descriptor.
    let packed = u64::from(facts.function) | PLAIN_FLAG | NATIVE_FLAG | ((shape.argc as u64) << ARGC_SHIFT);
    let record = Box::new(CallRecord {
        proven_args,
        callee: facts.callee,
        cell: facts.cell as u64,
        expected: facts.validity,
        call: facts.call,
        caller_base: facts.caller_base,
        packed,
        captures: facts.captures,
        global_this: facts.global_this.unwrap_or_default(),
        callee_register: shape.callee,
        this_register: shape.this,
        args_register: shape.window,
        result_register: shape.result,
        argument_places: arguments,
        descriptor: facts.descriptor,
        env: facts.env,
        capture: facts.capture,
    });
    let argc = u8::try_from(shape.argc).map_err(|e| e.to_string())?;
    let this = if facts.lexical { ThisRow::Lexical } else if facts.global_this.is_some() { ThisRow::Sloppy } else { ThisRow::Strict };
    let pinned = arguments.iter().any(|&place| place != Place::Mem.code());
    let stencil = call::stencil(CallRow::Direct { argc, this, checked: !facts.held, pinned })?;
    let mut holes = [0; HOLES];
    holes[DirectHole::Record as usize] = std::ptr::from_ref(&*record).addr() as u64;
    holes[DirectHole::CallPc as usize] = u64::from(shape.call_pc);
    Ok((part(stencil, holes, vec![Exit::Status]), record))
}

/// A call of the builtin's arity whose callee is that builtin declines the region: the interpreter runs the call in
/// place (`lane_call_in_place`), and with the call inside it the region measured slower than the interpreter
/// (TOD-2211). Any other call is planned as it is.
fn decline_builtin(shape: &CallShape, builtin: Option<BuiltinCall>) -> Result<(), String> {
    let Some(BuiltinCall { row }) = builtin else { return Ok(()) };
    if shape.form != CallForm::Plain || shape.returning || shape.argc != row.arguments() {
        return Ok(());
    }
    Err(format!("the builtin call {row:?} runs in place in the interpreter"))
}

/// The chain of a helper step at `pc` (any instruction whose operation the helper runs in place):
/// the spills, the helper row, the reloads.
pub(crate) fn plan_step(pc: u32, helper: HelperPlace, packed: Option<[u64; 2]>, spills: &[(Place, Register)], reloads: &[(Place, Register)]) -> Result<Vec<CallPart>, String> {
    let mut parts = spills.iter().map(|&(place, register)| spill_part(place, register)).collect::<Result<Vec<_>, _>>()?;
    parts.push(helper_part(pc, helper, packed)?);
    for &(place, register) in reloads {
        parts.push(reload_part(place, register)?);
    }
    Ok(parts)
}

/// The chain of a call (see the module doc for its layout and successors). A polymorphic site's chain
/// holds one direct row per callee it reached, latest first: each refuses another callee to the next
/// (NEXT), the last to the helper row, so a site alternating between known callees calls each by its
/// entry, as the interpreter's own link does, and only an unknown one takes the helper.
pub(crate) fn plan_call(plan: &CallPlan) -> Result<CallChain, String> {
    if let Some(chain) = chained_reads(plan)? {
        return Ok(chain);
    }
    let choices: Vec<(Choice, &DirectFacts)> = plan.direct.iter().map(|facts| (choose(&plan.shape, Some(facts)), facts)).collect();
    if quench_runtime::trace::Channel::AotLanes.on() && !plan.direct.is_empty() && !choices.iter().any(|(choice, _)| *choice == Choice::Direct) {
        eprintln!("aot-lanes: call at {} declines its direct row: {:?} (form {:?}, method {})", plan.shape.call_pc, choices.first().map(|(choice, _)| *choice), plan.shape.form, plan.shape.method.is_some());
    }
    let choice = choices.iter().map(|(choice, _)| *choice).find(|choice| *choice == Choice::Direct)
        .unwrap_or_else(|| choices.first().map_or_else(|| choose(&plan.shape, None), |(choice, _)| *choice));
    let arguments = argument_places(plan);
    let mut directs = Vec::new();
    for (_, facts) in choices.into_iter().filter(|(choice, _)| *choice == Choice::Direct) {
        directs.push(direct_part(&plan.shape, facts, arguments, plan.proven_args)?);
    }
    // A builtin callee where no direct row stands declines the region (`decline_builtin`).
    if directs.is_empty() {
        decline_builtin(&plan.shape, plan.builtin)?;
    }
    // Every spill comes before the direct rows. A spill behind them (a pinned argument only the refusal path
    // reads) wrote what the refused row passed on: every pinned register is dead past a direct row, whose NEXT
    // passes junk (`words::dead`), so the helper took a stale register word as the argument (JetStream ML's
    // "Row index out of range": `check(j)` in a nested loop got a pointer's low bits whenever its callee's cell
    // held the stub). The direct row still reads the argument from its pin (`argument_places`).
    let mut parts = Vec::new();
    let read = plan.read.filter(|_| !directs.is_empty());
    for &(place, register) in plan.spills {
        parts.push(spill_part(place, register)?);
    }
    if let Some((stencil, holes)) = read {
        // A miss takes the helper row behind the direct rows, as a refused direct call does: the spills ran, so the frame is canonical.
        let helper = 1 + directs.len();
        parts.push(CallPart { stencil, holes, taken: None, exits: vec![Exit::Ahead(helper)], symbols: NO_SYMBOLS });
    }
    // Past every direct row and the helper row that follows them: the first reload, or the next instruction.
    let past = parts.len() + directs.len() + 1;
    let mut records = Vec::new();
    for (mut row, owned) in directs {
        row.taken = Some(past);
        parts.push(row);
        records.push(owned);
    }
    parts.push(helper_part(plan.shape.call_pc, plan.helper, None)?);
    for &(place, register) in plan.reloads {
        parts.push(reload_part(place, register)?);
    }
    Ok(CallChain { parts, records, choice })
}


#[cfg(test)]
pub(crate) mod tests;
