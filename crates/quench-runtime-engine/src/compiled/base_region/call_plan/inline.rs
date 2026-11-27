//! Stitch-inlining: at a direct-cell call whose callee is monomorphic and small
//! (`tier_policy::Stitch`), the stitcher splices the callee's rows into the caller in place of the
//! call chain (`function::cover`, `stitch::stitch`).
//!
//! * The callee's registers are a fresh window `base ..` above the registers of the activation
//!   that makes the call (`Context`), in the frame slots past the caller's, which the body's
//!   recovery room covers (`LaneRecoverySpace`). An argument reaches its parameter by renaming: a
//!   parameter the callee never writes is read from the caller's argument register itself (an
//!   alias, no row); one it writes is first copied (`Move`).
//! * The callee's own jumps stay rows of the site, its registers renamed; a `Return` is a `Move`
//!   of the value to the call's result register and a jump to the continuation, the end of the
//!   site's rows; a final `Return` falls through to it. Several returns are several such pairs.
//! * The callee's identity is a Stable fact where the callee register's by-name read is a global
//!   cell (`Proof::Held`): the publication subscribes it (`StableFacts::globals`), so a
//!   redefinition of the callee retires the code and the next plan names the new identity, with no
//!   check per call. Any other callee (one a closure holds) is proved by a row at the call
//!   (`Proof::Guard`).
//! * The callee belongs to the realm the body was compiled in, which the body's entry proves active
//!   (`Heap::Realm`): a callee of another realm is not inlined (`Vm::inlinable_callee`), as its
//!   rows would read the caller's globals.
//! * Each row names the registers of the callee that are live before it (`SiteRow::live`), renamed
//!   into the window, so the stitch derives what is live at every row of the flat program.
//! * The callee's receiver is the call's `this` register (`CallShape::this`), which the caller's
//!   rows hold live across the site. A `this` read is that register (`LoadThis` a move, a field
//!   read through `this` a `Heap::Site` on it), which holds only for a strict callee: an arrow's
//!   `this` is lexical and a sloppy callee binds an `undefined` receiver to the global object, so
//!   a callee of either kind that reads `this` is refused (`Refused::Receiver`).
//!
//! Jump targets of the rows are local to the site (`0 ..= rows.len()`, where `rows.len()` is the
//! continuation); the stitcher adds the row index the first is placed at.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::{choose, CallShape, Choice, DirectFacts};
use quench_runtime::bytecode::{Class, FieldBase, FieldLookup, Register, WideInstruction, REGISTER_MASK};
use quench_runtime::compile::liveness::RegisterMasks;
use quench_runtime::tier_surface::GlobalFact;
use quench_runtime::value::Value;
use super::super::super::lane_unit::{inlined_returning, renamed, CallRecovery, CallTarget, CalleeBinding, InlinedCall};
use super::super::super::tier_policy::Stitch;
use super::super::decode::{numeric_row, register_of, site_read_row, Decoded, Numbers, Source};
use super::super::super::base_rows::Heap;
use quench_bjit::numeric::Kind;

/// A callee the stitcher may inline, as its function and the call site's records show it.
pub(crate) struct Callee<'f> {
    pub(crate) function: u32,
    pub(crate) identity: Value,
    pub(crate) code: &'f [WideInstruction],
    pub(crate) registers: u16,
    /// Each parameter an argument passes to, as (argument index, home register).
    pub(crate) homes: &'f [(u16, Register)],
    /// The callee's constant table's Numbers.
    pub(crate) numbers: Numbers<'f>,
    /// The callee's live registers before each instruction (`compile::liveness`).
    pub(crate) live: &'f RegisterMasks,
    /// The Stable fact the callee register's by-name read took: the global cell holding `identity`.
    pub(crate) stable: Option<GlobalFact>,
    /// The address and length of the closure's own capture slots (its environment's dense run), which
    /// the site's proof fixes with `identity`: a depth-0 capture read is a read of one slot's address.
    pub(crate) captures: (usize, usize),
}

/// The activations around a call being planned, and what is left of the stitch's budget.
#[derive(Clone)]
pub(crate) struct Context {
    /// The functions making the call, outermost first.
    chain: Vec<u32>,
    /// The first register above the registers of the innermost.
    top: u32,
    /// The first register no window may reach: the caller's registers and the policy's window.
    limit: u32,
    budget: usize,
    /// The thresholds this stitch plans under (`Known::stitch`).
    policy: &'static Stitch,
}

impl Context {
    /// The context of `caller`, which has `registers` registers.
    pub(crate) fn root(caller: u32, registers: u16, policy: &'static Stitch) -> Self {
        let limit = (u32::from(registers) + policy.window_registers).min(u32::from(REGISTER_MASK) + 1);
        Self { chain: vec![caller], top: u32::from(registers), limit, budget: policy.max_total_instructions, policy }
    }

    /// Keeps the windows planned from now on above `register`, which the site holds across its callee's rows.
    pub(crate) fn above(&mut self, register: Register) {
        self.top = self.top.max(u32::from(register) + 1);
    }

    fn reserve(&mut self, bytecodes: usize) {
        self.budget -= bytecodes;
    }
}

/// Why a call keeps its call chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refused {
    /// The call is not a direct-cell call (`call_plan::choose`).
    NotDirect,
    /// The callee is not the function the call's record names.
    Identity,
    /// The callee is already being inlined around this call.
    Recursion,
    /// More activations than the inline depth.
    Depth,
    Size,
    Budget,
    /// The registers of the window pass the policy's window or do not fit an instruction.
    Registers,
    /// A parameter takes an argument the call does not pass: it would read `undefined`.
    Arguments,
    /// A register that is no parameter is read before the callee writes it: the window holds
    /// no initial value for it.
    Uninitialized,
    /// The callee reads `this` where the call does not bind it to the call's receiver: an arrow (its
    /// lexical `this`), or a sloppy callee (an `undefined` receiver is the global object).
    Receiver,
    /// An opcode class outside the stitch's set.
    Opcode(Class),
    /// The instruction at this callee pc takes no row.
    Row(u32),
}

/// What proves a call runs the function its site was stitched for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Proof {
    /// The global cell holding it, which a change of retires the code.
    Held(GlobalFact),
    /// The callee register holds exactly this word, tested at the call.
    Guard(Register, u64),
}

/// A row of an inlined site.
pub(crate) struct SiteRow {
    pub(crate) decoded: Decoded,
    /// The callee instruction the row is part of; `None` for the copy of an argument, which
    /// runs before the callee's first instruction.
    pub(crate) pc: Option<u32>,
    /// The registers the interpreter resumed in the callee at this row would read before writing,
    /// renamed into the caller's registers and the window.
    pub(crate) live: Vec<Register>,
}

/// A call planned as its callee's rows, each over registers of the caller or its window.
pub(crate) struct InlineSite {
    pub(crate) shape: CallShape,
    pub(crate) proof: Proof,
    pub(crate) rows: Vec<SiteRow>,
    /// The first register above the callee's window.
    pub(crate) top: u32,
    /// The (parameter register, argument register) copies a leave inside the callee runs first:
    /// each parameter the callee never writes is read from its argument register, where the
    /// interpreter's activation expects it in its own.
    pub(crate) copies: Vec<(Register, Register)>,
    /// The call as the method's inlined frame, which a leave inside the callee recovers
    /// (`Vm::body_inlined_activation_in_lane`).
    pub(crate) frame: InlinedCall,
}

/// The first reason `callee` does not inline at `shape`.
fn admit(shape: &CallShape, facts: &DirectFacts, callee: &Callee, context: &Context) -> Result<(), Refused> {
    let policy = context.policy;
    let window = context.top + u32::from(callee.registers);
    let refusals = [
        // A returning call grows no stack where it inlines, so it is direct as the call that does not return is.
        (choose(&CallShape { returning: false, ..*shape }, Some(facts)) != Choice::Direct, Refused::NotDirect),
        (facts.callee != callee.identity.to_bits(), Refused::Identity),
        (context.chain.contains(&callee.function), Refused::Recursion),
        (context.chain.len() > policy.max_depth, Refused::Depth),
        (callee.code.len() > policy.max_instructions, Refused::Size),
        (callee.code.len() > context.budget, Refused::Budget),
        (window > context.limit, Refused::Registers),
        (callee.homes.iter().any(|(argument, _)| *argument >= shape.argc), Refused::Arguments),
        ((0..callee.registers).any(|register| callee.live.is_live(0, register) && callee.homes.iter().all(|(_, home)| *home != register)), Refused::Uninitialized),
        ((facts.lexical || facts.global_this.is_some()) && callee.code.iter().any(|instruction| reads_receiver(*instruction)), Refused::Receiver),
    ];
    if let Some((_, why)) = refusals.into_iter().find(|(refused, _)| *refused) {
        return Err(why);
    }
    // The opcode a callee body may hold: the numeric rows of the stencil library, its jumps (the
    // site's own rows), and the `Return` that becomes a move and a jump to the continuation.
    // A receiver read is a move of the call's `this` or a site read on it (`Emitter::receiver_row`).
    match callee.code.iter().find(|instruction| !crate::compiled::meaning_compiled::stitchable(**instruction) && !reads_plain_receiver(**instruction)) {
        Some(instruction) => Err(Refused::Opcode(instruction.op().class())),
        None => Ok(()),
    }
}

/// Whether `instruction` reads the callee's `this`: a `LoadThis`, or a field read whose base is `this`.
fn reads_receiver(instruction: WideInstruction) -> bool {
    crate::compiled::meaning_compiled::is_load_this(instruction) || (crate::compiled::meaning_compiled::is_get_field(instruction) && instruction.b() == FieldBase::THIS.0)
}

/// Whether `instruction` is a receiver read the emitter takes as it is: a `LoadThis`, or a field read through
/// `this` of a named atom, neither returning (`Emitter::receiver_row`).
fn reads_plain_receiver(instruction: WideInstruction) -> bool {
    reads_receiver(instruction) && instruction.result_flags() == 0
        && (crate::compiled::meaning_compiled::is_load_this(instruction) || matches!(instruction.field_lookup(), Some(FieldLookup::Atom { .. })))
}

/// Whether `row` reads the register it writes.
fn rereads(row: &Decoded) -> bool {
    [row.a, row.b].into_iter().flatten().filter_map(register_of).any(|register| Some(register) == row.dst)
}

/// The callee instructions, renamed into the caller's registers and laid out as a site's rows.
struct Emitter<'a> {
    shape: &'a CallShape,
    callee: &'a Callee<'a>,
    base: Register,
    /// Each parameter register the callee never writes, with the argument register it reads.
    aliases: Vec<(Register, Register)>,
    /// The local row index each callee pc starts at, and the end.
    starts: Vec<u32>,
    rows: Vec<SiteRow>,
    /// The callee pcs control reaches from its entry: the rest (the implicit `return undefined`
    /// after a last `return x`) emit no rows, as a compiler drops dead code.
    reached: quench_runtime::bitset::BitSet,
}

impl Emitter<'_> {
    fn register(&self, register: Register) -> Register {
        self.aliases.iter().find(|(home, _)| *home == register).map_or(self.base + register, |(_, argument)| *argument)
    }

    fn continuation(&self) -> u32 {
        *self.starts.last().expect("starts hold the end")
    }

    /// The callee's live registers before `pc`, renamed, and `value` where a row reads one the
    /// instruction at `pc` defines.
    fn live_at(&self, pc: u32, value: Option<Register>) -> Vec<Register> {
        let live = (0..self.callee.registers).filter(|register| self.callee.live.is_live(pc as usize, *register));
        live.map(|register| self.register(register)).chain(value).collect()
    }

    fn push(&mut self, pc: u32, decoded: Decoded, value: Option<Register>) {
        let live = self.live_at(pc, value);
        self.rows.push(SiteRow { decoded, pc: Some(pc), live });
    }

    /// The copy of argument register `argument` into the window register `home`, after the
    /// copies `done`: only the parameters copied so far are live.
    fn copy(&mut self, done: &[(Register, Register)], home: Register, argument: Register) {
        let live = done.iter().map(|(window, _)| *window).collect();
        let decoded = Decoded::number(Kind::Move, Some(home), Some(Source::Register(argument)), None, None);
        self.rows.push(SiteRow { decoded, pc: None, live });
    }

    /// The call completes with `value`: moved to the result, then past the site unless this is
    /// the callee's last reached instruction.
    fn complete(&mut self, pc: u32, value: Register) {
        if value != self.shape.result {
            let decoded = Decoded::number(Kind::Move, Some(self.shape.result), Some(Source::Register(value)), None, None);
            self.push(pc, decoded, Some(value));
        }
        if reached_after(&self.reached, pc as usize, self.callee.code.len()) {
            let end = self.continuation();
            self.push(pc, Decoded::number(Kind::Jump, None, None, None, Some(end)), None);
        }
    }

    /// A depth-0 capture read of the closure's own environment: the slot's address, which the closure's
    /// identity fixes (`Heap::CalleeCapture`). A slot past the run is the interpreter's.
    fn capture_row(&self, instruction: WideInstruction) -> Option<Decoded> {
        let (values, len) = self.callee.captures;
        let slot = usize::from(instruction.capture_slot());
        let capture = matches!(crate::compiled::decoded_completed(instruction), Some(quench_runtime::staged::Operation::LoadCapture { .. }));
        (capture && instruction.capture_depth() == 0 && values != 0 && slot < len).then(|| Decoded {
            hole: (values + slot * std::mem::size_of::<Value>()) as u64,
            ..Decoded::heap(Heap::CalleeCapture, Some(instruction.result_register()), None, None)
        })
    }

    fn renamed(&self, pc: u32, instruction: WideInstruction) -> Result<WideInstruction, Refused> {
        let target = |target: u32| self.starts.get(target as usize).copied();
        renamed(instruction, &|register| self.register(register), self.shape.this, &target).ok_or(Refused::Row(pc))
    }

    /// The row of a receiver read or of a plain field read: `LoadThis` is a move of the call's receiver
    /// (`shape.this`, already in the caller's frame, so not renamed), and a `GetField` reads its object
    /// (the renamed base, which a `this` base made the receiver) through its cache site with no facts.
    /// A nested field lookup names the program's sites, which an inlined callee does not carry.
    fn receiver_row(&self, plain: WideInstruction, renamed: WideInstruction) -> Option<Decoded> {
        if plain.result_flags() != 0 {
            return None;
        }
        match () {
            _ if crate::compiled::meaning_compiled::is_load_this(renamed) => Some(Decoded::number(Kind::Move, Some(renamed.result_register()), Some(Source::Register(self.shape.this)), None, None)),
            _ if crate::compiled::meaning_compiled::is_get_field(renamed) => match renamed.field_lookup()? {
                FieldLookup::Atom { base, cache_site, .. } => Some(site_read_row(base.register_index()?, cache_site, renamed.result_register())),
                FieldLookup::Site(_) => None,
            },
            _ => None,
        }
    }

    fn instruction(&mut self, pc: u32, instruction: WideInstruction) -> Result<(), Refused> {
        if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Return { .. })) {
            if instruction.result_flags() != 0 {
                return Err(Refused::Row(pc));
            }
            self.complete(pc, self.register(instruction.register_a()));
            return Ok(());
        }
        let returning = inlined_returning(instruction);
        let plain = if returning {
            WideInstruction::new(instruction.op(), instruction.result_register(), instruction.b(), instruction.c(), instruction.imm())
        } else {
            instruction
        };
        let renamed = self.renamed(pc, plain)?;
        let row = numeric_row(self.callee.numbers, renamed).flatten().or_else(|| self.capture_row(renamed)).or_else(|| self.receiver_row(plain, renamed)).ok_or(Refused::Row(pc))?;
        // A row behind which a step helper runs names its record by the running function's PC
        // (`Layout::record`), which an inlined callee's instruction has none of: the helper would make
        // whatever instruction of the caller sits at the row's index (a `p === 0` callee reset the
        // caller's counter, or spun its loop). Such a callee keeps its call.
        if row.step.is_some_and(|step| step.runs_helper()) {
            return Err(Refused::Row(pc));
        }
        // A leave between the computation and the return resumes at the instruction, which a row
        // reading what it wrote would apply twice.
        if returning && rereads(&row) {
            return Err(Refused::Row(pc));
        }
        self.push(pc, row, None);
        if returning {
            self.complete(pc, self.register(instruction.result_register()));
        }
        Ok(())
    }
}

/// The local row index each callee pc starts at, and the end: a `Return` is a `Move` unless it
/// returns the result register already, and a jump unless last; a returning instruction adds the
/// same.
fn starts(shape: &CallShape, callee: &Callee, register: &dyn Fn(Register) -> Register, (leading, reached): (u32, &quench_runtime::bitset::BitSet)) -> Vec<u32> {
    let mut at = leading;
    let mut starts = Vec::with_capacity(callee.code.len() + 1);
    for (pc, instruction) in callee.code.iter().enumerate() {
        starts.push(at);
        let completion = |value: Register| u32::from(value != shape.result) + u32::from(reached_after(reached, pc, callee.code.len()));
        at += if !reached.contains(pc) {
            0
        } else if matches!(crate::compiled::decoded(*instruction), Some(quench_runtime::staged::Operation::Return { .. })) {
            completion(register(instruction.register_a()))
        } else if inlined_returning(*instruction) {
            1 + completion(register(instruction.result_register()))
        } else {
            1
        };
    }
    starts.push(at);
    starts
}

/// Whether control reaches a callee instruction after `pc`, so that a completion at `pc` jumps past them.
fn reached_after(reached: &quench_runtime::bitset::BitSet, pc: usize, len: usize) -> bool {
    (pc + 1..len).any(|later| reached.contains(later))
}

/// The call of `callee` at `shape`, its window at `base`, as the inlined frame the interpreter
/// recovers by materializing the callee's activation.
fn frame(shape: &CallShape, callee: &Callee, base: Register) -> InlinedCall {
    InlinedCall {
        recovery: CallRecovery::Materialize,
        parent: None,
        call: shape.call_pc,
        target: CallTarget { callee: CalleeBinding::Identity(callee.identity), function: callee.function },
        callee_register: shape.callee,
        callee_entry_word: None,
        receiver: shape.this,
        window: shape.window,
        arguments: shape.argc,
        result: (!shape.returning).then_some(shape.result),
        base,
        registers: callee.registers,
        alternative: None,
        function_call: None,
    }
}

impl InlineSite {
    /// `callee` spliced in at `shape` in `context`, reserving its registers and budget; the first
    /// reason it does not (`Refused`) leaves the context as it was.
    pub(crate) fn plan(shape: &CallShape, facts: &DirectFacts, callee: &Callee, context: &mut Context) -> Result<Self, Refused> {
        admit(shape, facts, callee, context)?;
        let base = Register::try_from(context.top).map_err(|_| Refused::Registers)?;
        let reached = quench_runtime::bytecode::reachable_from(callee.code, [0]);
        let live_code = callee.code.iter().enumerate().filter(|(pc, _)| reached.contains(*pc)).map(|(_, instruction)| instruction);
        let written: Vec<Register> = live_code.flat_map(|instruction| instruction.written_registers()).collect();
        let (copied, aliased): (Vec<_>, Vec<_>) = callee.homes.iter().partition(|(_, home)| written.contains(home));
        let aliases: Vec<(Register, Register)> = aliased.iter().map(|(argument, home)| (*home, shape.window + argument)).collect();
        let mut emitter = Emitter { shape, callee, base, aliases, starts: Vec::new(), rows: Vec::new(), reached };
        emitter.starts = starts(shape, callee, &|register| emitter.register(register), (copied.len() as u32, &emitter.reached));
        let mut done = Vec::with_capacity(copied.len());
        for (argument, home) in copied {
            emitter.copy(&done, base + home, shape.window + argument);
            done.push((base + home, shape.window + argument));
        }
        for (pc, instruction) in callee.code.iter().enumerate() {
            if emitter.reached.contains(pc) {
                emitter.instruction(pc as u32, *instruction)?;
            }
        }
        context.reserve(callee.code.len());
        let proof = callee.stable.map_or(Proof::Guard(shape.callee, callee.identity.to_bits()), Proof::Held);
        let copies = emitter.aliases.iter().map(|(home, argument)| (base + *home, *argument)).collect();
        Ok(Self { shape: *shape, proof, rows: emitter.rows, top: context.top + u32::from(callee.registers), copies, frame: frame(shape, callee, base) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quench_runtime::vm::tier_policy::TierRows;
    use quench_runtime::bytecode::{Op, Operand, RETURN_REGISTER};
    use quench_runtime::code_dependencies::{DependentCode, PublishedCode};
    use quench_runtime::tier_surface::{global_value_fact, note_global_write, subscribe_global_value, GlobalCell};
    use quench_runtime::staged::call::{CellFacts, ENTRY_ACTIVATES};
    use oxc_ast::ast::BinaryOperator;
    use std::rc::Rc;

    const SHAPE: CallShape = CallShape { call_pc: 9, callee: 1, this: 2, window: 3, argc: 2, result: 6, returning: false, form: super::super::CallForm::Plain, method: None, op: quench_runtime::bytecode::Op::Call };
    /// The caller's registers: the callee's window starts here.
    const BASE: Register = 8;
    const IDENTITY: u64 = 7;
    const CALLER: u32 = 100;
    const CALLEE: u32 = 101;

    fn facts() -> DirectFacts {
        let cell = CellFacts { registers: 4, room: 4, params: 2, fill: quench_runtime::staged::call::FILL_ALL, entry: ENTRY_ACTIVATES as u8 };
        DirectFacts { callee: IDENTITY, cell: 0x1000, validity: 5, facts: cell, call: 0x3000, caller_base: 0x4000, function: 0, captures: 0, env: 0, capture: 0, global_this: None, lexical: false, registers: 4, construction: false, held: false, closed: false, descriptor: 0 }
    }

    fn operand(register: Register) -> u16 {
        Operand::register(register).0
    }

    /// `function (a, b) { return a + b }` with parameters in registers 0 and 1: the add returns.
    fn sum() -> Vec<WideInstruction> {
        vec![WideInstruction::new(Op::Binary, RETURN_REGISTER | 2, operand(0), operand(1), BinaryOperator::Addition as u32)]
    }

    fn no_numbers(_: usize) -> Option<f64> {
        None
    }

    /// Registers 0 and 1 live before every instruction, the parameters.
    fn parameters_live() -> RegisterMasks {
        RegisterMasks::Narrow(vec![0b11; 16].into())
    }

    fn callee<'f>(code: &'f [WideInstruction], homes: &'f [(u16, Register)], live: &'f RegisterMasks) -> Callee<'f> {
        Callee { function: CALLEE, identity: Value::from_bits(IDENTITY), code, registers: 4, homes, numbers: &no_numbers, live, stable: None, captures: (0, 0) }
    }

    const HOMES: [(u16, Register); 2] = [(0, 0), (1, 1)];

    fn rows(site: &InlineSite) -> Vec<(Kind, Option<u32>)> {
        site.rows.iter().map(|row| (row.decoded.kind, row.decoded.target)).collect()
    }

    #[test]
    fn a_small_arithmetic_callee_is_two_rows_over_the_argument_registers() {
        let (code, live, mut context) = (sum(), parameters_live(), Context::root(CALLER, BASE, &TierRows::Measured.table().stitch));
        let site = InlineSite::plan(&SHAPE, &facts(), &callee(&code, &HOMES, &live), &mut context).expect("inlined");
        assert_eq!(rows(&site), [(Kind::Add, None), (Kind::Move, None)], "the add into a window register, and the move to the result");
        let add = &site.rows[0].decoded;
        assert_eq!(add.dst, Some(BASE + 2));
        assert!(matches!((add.a, add.b), (Some(Source::Register(3)), Some(Source::Register(4)))), "parameters read the caller's argument registers");
        assert_eq!((site.proof, site.top), (Proof::Guard(SHAPE.callee, IDENTITY), u32::from(BASE) + 4));
        assert_eq!(site.rows[0].live, [3, 4], "the parameters are live before the add, renamed to the argument registers");
    }

    #[test]
    fn a_written_parameter_is_copied_and_each_return_moves_and_jumps_past_the_rest() {
        let code = vec![
            WideInstruction::new(Op::JumpTrue, 1, 0, 0, 2),
            WideInstruction::new(Op::Return, 0, 0, 0, 0),
            WideInstruction::new(Op::Move, 0, 1, 0, 0),
            WideInstruction::new(Op::Return, 0, 0, 0, 0),
        ];
        let (live, mut context) = (parameters_live(), Context::root(CALLER, BASE, &TierRows::Measured.table().stitch));
        let site = InlineSite::plan(&SHAPE, &facts(), &callee(&code, &HOMES, &live), &mut context).expect("inlined");
        let end = site.rows.len() as u32;
        assert_eq!(rows(&site), [(Kind::Move, None), (Kind::JumpTrue, Some(4)), (Kind::Move, None), (Kind::Jump, Some(end)), (Kind::Move, None), (Kind::Move, None)]);
        assert_eq!(site.rows[0].pc, None, "the copy of the written parameter runs before the callee");
        assert_eq!(site.rows[0].decoded.dst, Some(BASE), "only the parameter the callee writes is copied");
        assert_eq!(site.rows[0].live, Vec::<Register>::new());
    }

    #[test]
    fn a_callee_with_jumps_and_two_returns_keeps_its_jumps_as_rows_of_the_site() {
        let code = vec![
            WideInstruction::new(Op::Jump, 0, 0, 0, 2),
            WideInstruction::new(Op::Return, 0, 0, 0, 0),
            WideInstruction::new(Op::Return, 1, 0, 0, 0),
        ];
        let (live, mut context) = (parameters_live(), Context::root(CALLER, BASE, &TierRows::Measured.table().stitch));
        let site = InlineSite::plan(&SHAPE, &facts(), &callee(&code, &HOMES, &live), &mut context).expect("inlined");
        assert_eq!(rows(&site), [(Kind::Jump, Some(1)), (Kind::Move, None)], "the return the jump skips is dead and makes no rows");
        assert_eq!(site.rows.iter().map(|row| row.pc).collect::<Vec<_>>(), [Some(0), Some(2)]);
    }

    /// `function (x) { return x + 1 }` compiles to the returning add and an implicit `return undefined`
    /// no path reaches: the site is the add and the move, as for a callee without the tail.
    #[test]
    fn the_unreachable_implicit_return_after_a_returning_instruction_makes_no_rows() {
        let mut code = sum();
        code.push(WideInstruction::new(Op::LoadConst, 2, 0, 0, 0));
        code.push(WideInstruction::new(Op::Return, 2, 0, 0, 0));
        let (live, mut context) = (parameters_live(), Context::root(CALLER, BASE, &TierRows::Measured.table().stitch));
        let site = InlineSite::plan(&SHAPE, &facts(), &callee(&code, &HOMES, &live), &mut context).expect("inlined despite the dead tail");
        assert_eq!(rows(&site), [(Kind::Add, None), (Kind::Move, None)]);
    }

    #[test]
    fn size_budget_opcode_recursion_depth_and_window_are_bounded_by_the_policy() {
        let (policy, live) = (&TierRows::Measured.table().stitch, parameters_live());
        let long = vec![WideInstruction::new(Op::Move, 2, 0, 0, 0); policy.max_instructions + 1];
        let plan = |code: &[WideInstruction], context: &mut Context| InlineSite::plan(&SHAPE, &facts(), &callee(code, &HOMES, &live), context).err();
        assert_eq!(plan(&long, &mut Context::root(CALLER, BASE, &TierRows::Measured.table().stitch)), Some(Refused::Size));
        let mut spent = Context::root(CALLER, BASE, &TierRows::Measured.table().stitch);
        spent.budget = 0;
        assert_eq!(plan(&sum(), &mut spent), Some(Refused::Budget));
        let throws = vec![WideInstruction::new(Op::Throw, 0, 0, 0, 0)];
        assert_eq!(plan(&throws, &mut Context::root(CALLER, BASE, &TierRows::Measured.table().stitch)), Some(Refused::Opcode(Class::Throw)));
        assert_eq!(plan(&sum(), &mut Context::root(CALLEE, BASE, &TierRows::Measured.table().stitch)), Some(Refused::Recursion));
        let mut deep = Context::root(CALLER, BASE, &TierRows::Measured.table().stitch);
        deep.chain.extend((0..policy.max_depth as u32).map(|function| function + CALLEE + 1));
        assert_eq!(plan(&sum(), &mut deep), Some(Refused::Depth));
        let mut high = Context::root(CALLER, BASE, &TierRows::Measured.table().stitch);
        high.top = high.limit;
        assert_eq!(plan(&sum(), &mut high), Some(Refused::Registers));
        let (few, code) = ([HOMES[0], (2, 2)], sum());
        let missing = Callee { homes: &few, ..callee(&code, &HOMES, &live) };
        assert_eq!(InlineSite::plan(&SHAPE, &facts(), &missing, &mut Context::root(CALLER, BASE, &TierRows::Measured.table().stitch)).err(), Some(Refused::Arguments));
        let record = DirectFacts { callee: IDENTITY + 1, ..facts() };
        assert_eq!(InlineSite::plan(&SHAPE, &record, &callee(&code, &HOMES, &live), &mut Context::root(CALLER, BASE, &TierRows::Measured.table().stitch)).err(), Some(Refused::Identity));
    }

    #[test]
    fn a_returning_call_has_no_result_register_to_resume_into() {
        let (code, live, mut context) = (sum(), parameters_live(), Context::root(CALLER, BASE, &TierRows::Measured.table().stitch));
        let returning = CallShape { returning: true, ..SHAPE };
        let site = InlineSite::plan(&returning, &facts(), &callee(&code, &HOMES, &live), &mut context).expect("inlined");
        assert_eq!((site.frame.result, site.frame.call), (None, SHAPE.call_pc));
        let site = InlineSite::plan(&SHAPE, &facts(), &callee(&code, &HOMES, &live), &mut Context::root(CALLER, BASE, &TierRows::Measured.table().stitch)).expect("inlined");
        assert_eq!(site.frame.result, Some(SHAPE.result));
    }

    #[test]
    fn a_global_callee_is_held_by_its_fact_and_a_redefinition_retires_the_code() {
        let table = quench_runtime::heap::new_table();
        let first = global_value_fact(table, GlobalCell::Slot(0)).expect("a cell nothing wrote is folded");
        let (code, live, mut context) = (sum(), parameters_live(), Context::root(CALLER, BASE, &TierRows::Measured.table().stitch));
        let held = Callee { stable: Some(first), ..callee(&code, &HOMES, &live) };
        let site = InlineSite::plan(&SHAPE, &facts(), &held, &mut context).expect("inlined");
        assert_eq!(site.proof, Proof::Held(first));
        let published = Rc::new(PublishedCode::new(()));
        let reader: Rc<dyn DependentCode> = published.clone();
        assert!(subscribe_global_value(first, &reader));
        note_global_write(table, GlobalCell::Slot(0));
        assert!(!published.is_published(), "the redefinition retires the code");
        assert!(!subscribe_global_value(first, &reader), "the old identity cannot be subscribed again");
    }

    #[test]
    fn a_register_read_before_it_is_written_keeps_the_call() {
        let (code, live) = (sum(), RegisterMasks::Narrow(vec![0b111; 16].into()));
        let plan = InlineSite::plan(&SHAPE, &facts(), &callee(&code, &HOMES, &live), &mut Context::root(CALLER, BASE, &TierRows::Measured.table().stitch));
        assert_eq!(plan.err(), Some(Refused::Uninitialized));
    }

    #[test]
    fn a_refused_site_reserves_nothing_and_an_accepted_one_takes_budget() {
        let (live, mut context) = (parameters_live(), Context::root(CALLER, BASE, &TierRows::Measured.table().stitch));
        let budget = context.budget;
        let throws = vec![WideInstruction::new(Op::Throw, 0, 0, 0, 0)];
        assert!(InlineSite::plan(&SHAPE, &facts(), &callee(&throws, &HOMES, &live), &mut context).is_err());
        assert_eq!(context.budget, budget);
        let code = sum();
        InlineSite::plan(&SHAPE, &facts(), &callee(&code, &HOMES, &live), &mut context).expect("inlined");
        assert_eq!(context.budget, budget - code.len());
    }
}
