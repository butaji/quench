//! Stitch-inlining: at a direct-cell call whose callee is monomorphic and small
//! (`tier_policy::Stitch`), the stitcher splices the callee's rows into the caller in place of the
//! call chain (`call_plan::plan_call`, `Choice::Inline`).
//!
//! * The callee's registers are a fresh window `base ..` above the registers of the activation
//!   that makes the call (`Context`). An argument reaches its parameter by renaming: a parameter
//!   the callee never writes is read from the caller's argument register itself (an alias, no
//!   row); one it writes is first copied (`Move`).
//! * A callee `Return` is a `Move` of the value to the call's result register and a jump to the
//!   continuation, the end of the site's rows; a final `Return` falls through to it.
//! * Each row names the exit a guard of it takes: `LaneExit::Inlined` with the site's frame and
//!   the callee pc, so the lane materializes the callee activation and the interpreter resumes in
//!   it (`Vm::lane_body_inlined_activation`). The activation takes its parameters from the
//!   window and its other registers from the unit's: an alias is copied into its parameter's
//!   register first (`InlineSite::copies`, which every such leave runs).
//! * The callee's identity is folded: the site holds the Stable fact the callee register's global
//!   read took (`InlineSite::stable`), which the publication subscribes
//!   (`StableFacts::globals`), so a redefinition of the callee retires the code and the next
//!   plan names the new identity, with no check per call.
//!
//! Jump targets of the rows are local to the site (`0 ..= rows.len()`, where `rows.len()` is the
//! continuation); the stitcher adds the pc its first row is placed at.

use super::{choose, CallShape, Choice, DirectFacts};
use crate::bytecode::{Class, Register, WideInstruction, REGISTER_MASK};
use crate::heap::GlobalFact;
use crate::value::Value;
use super::super::decode::{numeric_row, Decoded, Numbers, Source};
use super::super::super::lane_unit::{CallRecovery, CallTarget, CalleeBinding, Frame};
use super::super::super::osr_loop::{inlined_returning, renamed};
use super::super::super::tier_policy::TIERS;
use super::super::super::LaneExit;
use quench_bjit::numeric::Kind;

/// A callee the stitcher may inline, as its function and the call site's records show it.
pub(in super::super) struct Callee<'f> {
    pub(in super::super) function: u32,
    pub(in super::super) identity: Value,
    /// The Stable fact the call's callee register took: the global cell holding `identity`.
    pub(in super::super) stable: GlobalFact,
    pub(in super::super) code: &'f [WideInstruction],
    pub(in super::super) registers: u16,
    /// Each parameter an argument passes to, as (argument index, home register)
    /// (`Frame::passed_registers`).
    pub(in super::super) homes: &'f [(u16, Register)],
    /// The callee's constant table's Numbers.
    pub(in super::super) numbers: Numbers<'f>,
}

/// The activations around a call being planned, and what is left of the stitch's budget.
pub(in super::super) struct Context {
    /// The activations making the call, outermost first: each the function it runs, the frame
    /// that holds it (`None` for the caller), and the first register above its registers.
    chain: Vec<(u32, Option<usize>, u32)>,
    frames: usize,
    budget: usize,
}

impl Context {
    /// The context of `caller`, which has `registers` registers.
    pub(in super::super) fn root(caller: u32, registers: u16) -> Self {
        Self { chain: vec![(caller, None, u32::from(registers))], frames: 0, budget: TIERS.stitch.max_total_instructions }
    }

    fn top(&self) -> (u32, Option<usize>, u32) {
        *self.chain.last().expect("a context holds the caller's activation")
    }

    /// The calls `site` makes are planned within its activation, until `leave`.
    pub(in super::super) fn enter(&mut self, site: &InlineSite) {
        self.chain.push((site.frame.target.function, Some(site.index), u32::from(site.frame.base) + u32::from(site.frame.registers)));
    }

    pub(in super::super) fn leave(&mut self) {
        self.chain.pop();
    }

    fn reserve(&mut self, bytecodes: usize) {
        self.frames += 1;
        self.budget -= bytecodes;
    }
}

/// Why a call keeps its call chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) enum Refused {
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
    /// The registers of the window do not fit an instruction.
    Registers,
    /// A parameter takes an argument the call does not pass: it would read `undefined`.
    Arguments,
    /// An opcode class outside the stitch's set.
    Opcode(Class),
    /// The instruction at this callee pc takes no row.
    Row(u32),
}

/// What a site's row does: a row of the stencil library, or a call the stitcher plans in turn.
pub(in super::super) enum Step {
    Run(Decoded),
    Call(CallShape),
}

/// A row of a site and the exit its guard takes.
pub(in super::super) struct Row {
    pub(in super::super) step: Step,
    pub(in super::super) exit: LaneExit,
}

/// A call planned as its callee's rows.
pub(in super::super) struct InlineSite {
    /// The site's frame: its index in the method's inlined frames, and the record at it.
    pub(in super::super) index: usize,
    pub(in super::super) frame: Frame,
    pub(in super::super) rows: Vec<Row>,
    /// The (parameter register, argument register) copies a leave inside the site runs first.
    pub(in super::super) copies: Vec<(Register, Register)>,
    pub(in super::super) stable: GlobalFact,
    bytecodes: usize,
}

/// The Stable facts the sites fold, which the publication subscribes.
pub(in super::super) fn dependencies(sites: &[InlineSite]) -> Box<[GlobalFact]> {
    let mut facts: Vec<GlobalFact> = sites.iter().map(|site| site.stable).collect();
    facts.sort_unstable();
    facts.dedup();
    facts.into()
}

/// The first reason `callee` does not inline at `shape`.
fn admit(shape: &CallShape, facts: &DirectFacts, callee: &Callee, context: &Context) -> Result<(), Refused> {
    let policy = &TIERS.stitch;
    let window = context.top().2 + u32::from(callee.registers);
    let refusals = [
        (choose(shape, Some(facts)) != Choice::Direct, Refused::NotDirect),
        (facts.callee != callee.identity.to_bits(), Refused::Identity),
        (context.chain.iter().any(|(function, ..)| *function == callee.function), Refused::Recursion),
        (context.chain.len() > policy.max_depth, Refused::Depth),
        (callee.code.len() > policy.max_instructions, Refused::Size),
        (callee.code.len() > context.budget, Refused::Budget),
        (window > u32::from(REGISTER_MASK) + 1, Refused::Registers),
        (callee.homes.iter().any(|(argument, _)| *argument >= shape.argc), Refused::Arguments),
    ];
    if let Some((_, why)) = refusals.into_iter().find(|(refused, _)| *refused) {
        return Err(why);
    }
    match callee.code.iter().map(|instruction| instruction.op().class()).find(|class| !policy.classes.contains(class)) {
        Some(class) => Err(Refused::Opcode(class)),
        None => Ok(()),
    }
}

/// The callee instructions, renamed into the unit's registers and laid out as a site's rows.
struct Emitter<'a> {
    shape: &'a CallShape,
    callee: &'a Callee<'a>,
    base: Register,
    /// Each parameter register the callee never writes, with the argument register it reads.
    aliases: Vec<(Register, Register)>,
    /// The local row index each callee pc starts at, and the end.
    starts: Vec<u32>,
    frame: usize,
    rows: Vec<Row>,
}

impl Emitter<'_> {
    fn register(&self, register: Register) -> Register {
        self.aliases.iter().find(|(home, _)| *home == register).map_or(self.base + register, |(_, argument)| *argument)
    }

    fn continuation(&self) -> u32 {
        *self.starts.last().expect("starts hold the end")
    }

    fn push(&mut self, pc: u32, step: Step) {
        self.rows.push(Row { step, exit: LaneExit::Inlined { frame: self.frame, pc } });
    }

    fn mov(&mut self, pc: u32, dst: Register, src: Register) {
        self.push(pc, Step::Run(Decoded::number(Kind::Move, Some(dst), Some(Source::Register(src)), None, None)));
    }

    /// The call completes with `value`: moved to the result, then past the site unless this is
    /// the callee's last instruction.
    fn complete(&mut self, pc: u32, value: Register) {
        if value != self.shape.result {
            self.mov(pc, self.shape.result, value);
        }
        if pc as usize + 1 < self.callee.code.len() {
            let end = self.continuation();
            self.push(pc, Step::Run(Decoded::number(Kind::Jump, None, None, None, Some(end))));
        }
    }

    fn renamed(&self, pc: u32, instruction: WideInstruction) -> Result<WideInstruction, Refused> {
        let target = |target: u32| self.starts.get(target as usize).copied();
        renamed(instruction, &|register| self.register(register), 0, &target).ok_or(Refused::Row(pc))
    }

    fn instruction(&mut self, pc: u32, instruction: WideInstruction) -> Result<(), Refused> {
        let class = instruction.op().class();
        if class == Class::Return {
            if instruction.result_flags() != 0 {
                return Err(Refused::Row(pc));
            }
            self.complete(pc, self.register(instruction.register_a()));
            return Ok(());
        }
        let returning = class != Class::Call && inlined_returning(instruction);
        let plain = if returning {
            WideInstruction::new(instruction.op(), instruction.result_register(), instruction.b(), instruction.c(), instruction.imm())
        } else {
            instruction
        };
        let renamed = self.renamed(pc, plain)?;
        let step = if class == Class::Call {
            Step::Call(CallShape::of(pc, renamed))
        } else {
            Step::Run(numeric_row(self.callee.numbers, renamed).flatten().ok_or(Refused::Row(pc))?)
        };
        self.push(pc, step);
        if returning {
            self.complete(pc, self.register(instruction.result_register()));
        }
        Ok(())
    }
}

/// The local row index each callee pc starts at, and the end: a `Return` is a `Move` unless it
/// returns the result register already, and a jump unless last; a returning instruction adds the
/// same.
fn starts(shape: &CallShape, callee: &Callee, register: &dyn Fn(Register) -> Register, leading: u32) -> Vec<u32> {
    let mut at = leading;
    let mut starts = Vec::with_capacity(callee.code.len() + 1);
    for (pc, instruction) in callee.code.iter().enumerate() {
        starts.push(at);
        let completion = |value: Register| u32::from(value != shape.result) + u32::from(pc + 1 < callee.code.len());
        at += if instruction.op().class() == Class::Return {
            completion(register(instruction.register_a()))
        } else if instruction.op().class() != Class::Call && inlined_returning(*instruction) {
            1 + completion(register(instruction.result_register()))
        } else {
            1
        };
    }
    starts.push(at);
    starts
}

impl InlineSite {
    /// `callee` spliced in at `shape` in `context`, reserving its frame, registers and budget;
    /// the first reason it does not (`Refused`) leaves the context as it was.
    pub(in super::super) fn plan(shape: &CallShape, facts: &DirectFacts, callee: &Callee, context: &mut Context) -> Result<Self, Refused> {
        admit(shape, facts, callee, context)?;
        let base = Register::try_from(context.top().2).map_err(|_| Refused::Registers)?;
        let written: Vec<Register> = callee.code.iter().flat_map(|instruction| instruction.written_registers()).collect();
        let (copied, aliased): (Vec<_>, Vec<_>) = callee.homes.iter().partition(|(_, home)| written.contains(home));
        let aliases: Vec<(Register, Register)> = aliased.iter().map(|(argument, home)| (*home, shape.window + argument)).collect();
        let mut emitter = Emitter { shape, callee, base, aliases, starts: Vec::new(), frame: context.frames, rows: Vec::new() };
        emitter.starts = starts(shape, callee, &|register| emitter.register(register), copied.len() as u32);
        for (argument, home) in copied {
            emitter.mov(0, base + home, shape.window + argument);
        }
        for (pc, instruction) in callee.code.iter().enumerate() {
            emitter.instruction(pc as u32, *instruction)?;
        }
        let copies = emitter.aliases.iter().map(|(home, argument)| (base + home, *argument)).collect();
        let frame = Frame {
            recovery: CallRecovery::Materialize,
            parent: context.top().1,
            call: shape.call_pc,
            target: CallTarget { callee: CalleeBinding::Identity(callee.identity), function: callee.function },
            callee_register: shape.callee,
            callee_entry_word: None,
            receiver: shape.this,
            window: shape.window,
            arguments: shape.argc,
            result: Some(shape.result),
            base,
            registers: callee.registers,
            alternative: None,
            function_call: None,
        };
        let site = Self { index: context.frames, frame, rows: emitter.rows, copies, stable: callee.stable, bytecodes: callee.code.len() };
        context.reserve(site.bytecodes);
        Ok(site)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::{Op, Operand, RETURN_REGISTER};
    use crate::code_dependencies::{DependentCode, PublishedCode};
    use crate::heap::{global_value_fact, note_global_write, subscribe_global_value, GlobalCell};
    use crate::staged::call::{CellFacts, ENTRY_ACTIVATES};
    use oxc_ast::ast::BinaryOperator;
    use std::rc::Rc;

    const SHAPE: CallShape = CallShape { call_pc: 9, callee: 1, this: 2, window: 3, argc: 2, result: 6 };
    /// The caller's registers: the callee's window starts here.
    const BASE: Register = 8;
    const IDENTITY: u64 = 7;
    const CALLER: u32 = 100;
    const CALLEE: u32 = 101;

    fn facts() -> DirectFacts {
        let cell = CellFacts { registers: 4, room: 4, params: 2, entry: ENTRY_ACTIVATES as u8 };
        DirectFacts { callee: IDENTITY, cell: 0x1000, validity: 5, facts: cell, descriptor: 0x2000, captures: 0, global_this: None, registers: 4, construction: false, frame_bytes: 32 }
    }

    fn fact(table: u32, cell: u32) -> GlobalFact {
        global_value_fact(table, GlobalCell::Slot(cell)).expect("a cell nothing wrote is folded")
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

    fn callee<'f>(code: &'f [WideInstruction], homes: &'f [(u16, Register)], stable: GlobalFact) -> Callee<'f> {
        Callee { function: CALLEE, identity: Value::from_bits(IDENTITY), stable, code, registers: 4, homes, numbers: &no_numbers }
    }

    fn stable() -> GlobalFact {
        fact(crate::heap::new_table(), 0)
    }

    const HOMES: [(u16, Register); 2] = [(0, 0), (1, 1)];

    #[test]
    fn a_small_arithmetic_callee_is_inlined_with_its_parameters_renamed_and_no_call_row() {
        let (code, mut context) = (sum(), Context::root(CALLER, BASE));
        let before = super::super::super::inlined_sites();
        let site = InlineSite::plan(&SHAPE, &facts(), &callee(&code, &HOMES, stable()), &mut context).expect("inlined");
        assert!(site.copies.len() == 2 && site.copies.iter().all(|(home, argument)| *home == BASE + (argument - SHAPE.window)));
        let [Row { step: Step::Run(add), .. }, Row { step: Step::Run(moved), .. }] = site.rows.as_slice() else { panic!("an add and a move") };
        assert_eq!((add.dst, moved.dst), (Some(BASE + 2), Some(SHAPE.result)));
        assert!(matches!((add.a, add.b), (Some(Source::Register(3)), Some(Source::Register(4)))), "parameters read the caller's argument registers");
        assert_eq!(site.rows.len(), 2, "no jump after the last instruction");
        let plan = super::super::CallPlan { shape: SHAPE, direct: Some(&facts()), helper: 0x4000, spills: &[], reloads: &[], inline: Some(&site) };
        let chain = super::super::plan_call(&plan).expect("a plan");
        assert!(chain.parts.is_empty() && chain.record.is_none() && chain.choice == Choice::Inline, "no call row");
        assert!(super::super::super::inlined_sites() > before, "the site is counted");
    }

    #[test]
    fn a_written_parameter_is_copied_and_a_return_jumps_past_the_rest() {
        let code = vec![
            WideInstruction::new(Op::Return, 0, 0, 0, 0),
            WideInstruction::new(Op::Move, 0, 1, 0, 0),
            WideInstruction::new(Op::Return, 0, 0, 0, 0),
        ];
        let mut context = Context::root(CALLER, BASE);
        let site = InlineSite::plan(&SHAPE, &facts(), &callee(&code, &HOMES, stable()), &mut context).expect("inlined");
        assert_eq!(site.copies, [(BASE + 1, SHAPE.window + 1)], "only the parameter the callee does not write is an alias");
        let kinds: Vec<_> = site.rows.iter().map(|row| match &row.step { Step::Run(row) => (row.kind, row.target), Step::Call(_) => unreachable!() }).collect();
        assert!(kinds[0].0 == Kind::Move, "the written parameter's copy");
        assert!(kinds[2] == (Kind::Jump, Some(site.rows.len() as u32)), "a return jumps to the continuation");
    }

    #[test]
    fn every_row_leaves_inside_the_callee_at_its_pc() {
        let code = vec![
            WideInstruction::new(Op::Binary, 2, operand(0), operand(1), BinaryOperator::Addition as u32),
            WideInstruction::new(Op::Return, 2, 0, 0, 0),
        ];
        let mut context = Context::root(CALLER, BASE);
        context.frames = 4;
        let site = InlineSite::plan(&SHAPE, &facts(), &callee(&code, &HOMES, stable()), &mut context).expect("inlined");
        assert_eq!((site.index, context.frames), (4, 5));
        assert_eq!(site.rows[0].exit, LaneExit::Inlined { frame: 4, pc: 0 });
        assert_eq!(site.rows[1].exit, LaneExit::Inlined { frame: 4, pc: 1 });
        assert_eq!((site.frame.parent, site.frame.call, site.frame.base, site.frame.result), (None, 9, BASE, Some(6)));
        assert_eq!(site.frame.target.identity(), Some(Value::from_bits(IDENTITY)));
    }

    #[test]
    fn redefining_the_callee_retires_the_code_and_the_next_plan_names_the_new_identity() {
        let table = crate::heap::new_table();
        let first = fact(table, 0);
        let (code, mut context) = (sum(), Context::root(CALLER, BASE));
        let site = InlineSite::plan(&SHAPE, &facts(), &callee(&code, &HOMES, first), &mut context).expect("inlined");
        let published = Rc::new(PublishedCode::new(()));
        let reader: Rc<dyn DependentCode> = published.clone();
        assert!(dependencies(std::slice::from_ref(&site)).iter().all(|fact| subscribe_global_value(*fact, &reader)));
        note_global_write(table, GlobalCell::Slot(0));
        assert!(!published.is_published(), "the redefinition retires the region");
        assert!(!subscribe_global_value(first, &reader), "the old identity cannot be subscribed again");
        let again = fact(table, 0);
        let mut context = Context::root(CALLER, BASE);
        let moved = Callee { identity: Value::from_bits(IDENTITY + 1), ..callee(&code, &HOMES, again) };
        assert_eq!(InlineSite::plan(&SHAPE, &facts(), &moved, &mut context).err(), Some(Refused::Identity), "the call record names the old callee until it is re-recorded");
        let record = DirectFacts { callee: IDENTITY + 1, ..facts() };
        assert!(InlineSite::plan(&SHAPE, &record, &moved, &mut context).is_ok(), "re-planned under the new identity");
    }

    #[test]
    fn size_budget_opcode_recursion_and_depth_are_bounded_by_the_policy() {
        let policy = &TIERS.stitch;
        let long = vec![WideInstruction::new(Op::Move, 2, 0, 0, 0); policy.max_instructions + 1];
        let plan = |code: &[WideInstruction], context: &mut Context| InlineSite::plan(&SHAPE, &facts(), &callee(code, &HOMES, stable()), context).err();
        assert_eq!(plan(&long, &mut Context::root(CALLER, BASE)), Some(Refused::Size));
        let mut spent = Context::root(CALLER, BASE);
        spent.budget = 0;
        assert_eq!(plan(&sum(), &mut spent), Some(Refused::Budget));
        let throws = vec![WideInstruction::new(Op::Throw, 0, 0, 0, 0)];
        assert_eq!(plan(&throws, &mut Context::root(CALLER, BASE)), Some(Refused::Opcode(Class::Throw)));
        assert_eq!(plan(&sum(), &mut Context::root(CALLEE, BASE)), Some(Refused::Recursion));
        let mut deep = Context::root(CALLER, BASE);
        for function in 0..policy.max_depth {
            deep.chain.push((function as u32, Some(function), u32::from(BASE)));
        }
        assert_eq!(plan(&sum(), &mut deep), Some(Refused::Depth));
        let (few, code) = ([HOMES[0], (2, 2)], sum());
        let missing = Callee { homes: &few, ..callee(&code, &HOMES, stable()) };
        assert_eq!(InlineSite::plan(&SHAPE, &facts(), &missing, &mut Context::root(CALLER, BASE)).err(), Some(Refused::Arguments));
    }

    #[test]
    fn a_refused_site_reserves_nothing_and_an_accepted_one_takes_a_frame_and_budget() {
        let mut context = Context::root(CALLER, BASE);
        let budget = context.budget;
        assert!(InlineSite::plan(&SHAPE, &facts(), &callee(&long_return(), &HOMES, stable()), &mut context).is_err());
        assert_eq!((context.frames, context.budget), (0, budget));
        let code = sum();
        let site = InlineSite::plan(&SHAPE, &facts(), &callee(&code, &HOMES, stable()), &mut context).expect("inlined");
        assert_eq!((context.frames, context.budget), (1, budget - code.len()));
        context.enter(&site);
        assert_eq!(context.top(), (CALLEE, Some(site.index), u32::from(BASE) + 4), "calls inside the site plan above its window");
        context.leave();
        assert_eq!(context.top().0, CALLER);
    }

    fn long_return() -> Vec<WideInstruction> {
        vec![WideInstruction::new(Op::Throw, 0, 0, 0, 0)]
    }
}
