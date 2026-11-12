//! Stitch-inlining: at a direct-cell call whose callee is monomorphic and small
//! (`tier_policy::Stitch`), the stitcher decodes the call as the callee's one row in place of the
//! call chain (`function::inlined_call`).
//!
//! * The callee's registers are a fresh window `base ..` above the registers of the activation
//!   that makes the call (`Context`). An argument reaches its parameter by renaming: a parameter
//!   the callee never writes is read from the caller's argument register itself (an alias, no
//!   row); one it writes is first copied (`Move`).
//! * A callee `Return` is a `Move` of the value to the call's result register and a jump to the
//!   continuation, the end of the site's rows; a final `Return` falls through to it.
//!
//! The rows are the plan; what the stitcher keeps of it is a site whose rows touch the caller's
//! registers only (`collapsed`): no window register, so no row leaves anything of the callee to
//! materialize, and a guard that fails inside the callee leaves at the call, where the
//! interpreter runs the whole call: nothing of the site's rows has an effect the caller sees
//! before its last row writes the result.

use super::{choose, CallShape, Choice, DirectFacts};
use crate::bytecode::{Class, Register, WideInstruction, REGISTER_MASK};
use crate::value::Value;
use super::super::super::osr_loop::{inlined_returning, renamed};
use super::super::super::tier_policy::TIERS;
use super::super::decode::{numeric_row, Decoded, Numbers, Source};
use quench_bjit::numeric::Kind;

/// A callee the stitcher may inline, as its function and the call site's records show it.
pub(in super::super) struct Callee<'f> {
    pub(in super::super) function: u32,
    pub(in super::super) identity: Value,
    pub(in super::super) code: &'f [WideInstruction],
    pub(in super::super) registers: u16,
    /// Each parameter an argument passes to, as (argument index, home register).
    pub(in super::super) homes: &'f [(u16, Register)],
    /// The callee's constant table's Numbers.
    pub(in super::super) numbers: Numbers<'f>,
}

/// The activations around a call being planned, and what is left of the stitch's budget.
#[derive(Clone)]
pub(in super::super) struct Context {
    /// The functions making the call, outermost first.
    chain: Vec<u32>,
    /// The first register above the registers of the innermost.
    top: u32,
    budget: usize,
}

impl Context {
    /// The context of `caller`, which has `registers` registers.
    pub(in super::super) fn root(caller: u32, registers: u16) -> Self {
        Self { chain: vec![caller], top: u32::from(registers), budget: TIERS.stitch.max_total_instructions }
    }

    fn reserve(&mut self, bytecodes: usize) {
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

/// A call planned as its callee's rows, each over registers of the caller or its window.
pub(in super::super) struct InlineSite {
    rows: Vec<Decoded>,
}

/// The first reason `callee` does not inline at `shape`.
fn admit(shape: &CallShape, facts: &DirectFacts, callee: &Callee, context: &Context) -> Result<(), Refused> {
    let policy = &TIERS.stitch;
    let window = context.top + u32::from(callee.registers);
    let refusals = [
        // A returning call grows no stack where it inlines, so it is direct as the call that does not return is.
        (choose(&CallShape { returning: false, ..*shape }, Some(facts)) != Choice::Direct, Refused::NotDirect),
        (facts.callee != callee.identity.to_bits(), Refused::Identity),
        (context.chain.contains(&callee.function), Refused::Recursion),
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

/// The callee instructions, renamed into the caller's registers and laid out as a site's rows.
struct Emitter<'a> {
    shape: &'a CallShape,
    callee: &'a Callee<'a>,
    base: Register,
    /// Each parameter register the callee never writes, with the argument register it reads.
    aliases: Vec<(Register, Register)>,
    /// The local row index each callee pc starts at, and the end.
    starts: Vec<u32>,
    rows: Vec<Decoded>,
}

impl Emitter<'_> {
    fn register(&self, register: Register) -> Register {
        self.aliases.iter().find(|(home, _)| *home == register).map_or(self.base + register, |(_, argument)| *argument)
    }

    fn continuation(&self) -> u32 {
        *self.starts.last().expect("starts hold the end")
    }

    fn mov(&mut self, dst: Register, src: Register) {
        self.rows.push(Decoded::number(Kind::Move, Some(dst), Some(Source::Register(src)), None, None));
    }

    /// The call completes with `value`: moved to the result, then past the site unless this is
    /// the callee's last instruction.
    fn complete(&mut self, pc: u32, value: Register) {
        if value != self.shape.result {
            self.mov(self.shape.result, value);
        }
        if pc as usize + 1 < self.callee.code.len() {
            let end = self.continuation();
            self.rows.push(Decoded::number(Kind::Jump, None, None, None, Some(end)));
        }
    }

    fn renamed(&self, pc: u32, instruction: WideInstruction) -> Result<WideInstruction, Refused> {
        let target = |target: u32| self.starts.get(target as usize).copied();
        renamed(instruction, &|register| self.register(register), 0, &target).ok_or(Refused::Row(pc))
    }

    fn instruction(&mut self, pc: u32, instruction: WideInstruction) -> Result<(), Refused> {
        if instruction.op().class() == Class::Return {
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
        self.rows.push(numeric_row(self.callee.numbers, renamed).flatten().ok_or(Refused::Row(pc))?);
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
        } else if inlined_returning(*instruction) {
            1 + completion(register(instruction.result_register()))
        } else {
            1
        };
    }
    starts.push(at);
    starts
}

impl InlineSite {
    /// `callee` spliced in at `shape` in `context`, reserving its budget; the first reason it
    /// does not (`Refused`) leaves the context as it was.
    pub(in super::super) fn plan(shape: &CallShape, facts: &DirectFacts, callee: &Callee, context: &mut Context) -> Result<Self, Refused> {
        admit(shape, facts, callee, context)?;
        let base = Register::try_from(context.top).map_err(|_| Refused::Registers)?;
        let written: Vec<Register> = callee.code.iter().flat_map(|instruction| instruction.written_registers()).collect();
        let (copied, aliased): (Vec<_>, Vec<_>) = callee.homes.iter().partition(|(_, home)| written.contains(home));
        let aliases: Vec<(Register, Register)> = aliased.iter().map(|(argument, home)| (*home, shape.window + argument)).collect();
        let mut emitter = Emitter { shape, callee, base, aliases, starts: Vec::new(), rows: Vec::new() };
        emitter.starts = starts(shape, callee, &|register| emitter.register(register), copied.len() as u32);
        for (argument, home) in copied {
            emitter.mov(base + home, shape.window + argument);
        }
        for (pc, instruction) in callee.code.iter().enumerate() {
            emitter.instruction(pc as u32, *instruction)?;
        }
        context.reserve(callee.code.len());
        Ok(Self { rows: emitter.rows })
    }

    /// The one row the site is where it touches no register of the callee's window (`registers`
    /// are the caller's): the computation and a `Move` of its result to the call's result,
    /// fused, or the one row alone. `None` for any other site, which keeps its call chain.
    pub(in super::super) fn collapsed(self, shape: &CallShape, registers: u16) -> Option<Decoded> {
        match <[Decoded; 1]>::try_from(self.rows) {
            Ok([row]) => alone(row, shape, registers),
            Err(rows) => match <[Decoded; 2]>::try_from(rows) {
                Ok([row, moved]) => fused(row, &moved, shape, registers),
                Err(_) => None,
            },
        }
    }
}

/// Whether `row` is a plain computation over registers of the caller (below `registers`) apart
/// from `window`.
fn computes(row: &Decoded, registers: u16, window: Option<Register>) -> bool {
    row.heap.is_none()
        && row.target.is_none()
        && row.kind != Kind::Jump
        && row.registers().filter(|register| Some(*register) != window).all(|register| register < registers)
}

/// The one row of a site, where it writes the call's result.
fn alone(row: Decoded, shape: &CallShape, registers: u16) -> Option<Decoded> {
    (row.dst == Some(shape.result) && computes(&row, registers, None)).then_some(row)
}

/// `row` computing into a window register that `moved` then moves to the call's result: the row
/// writing the result itself, where nothing else of the site names the window register.
fn fused(row: Decoded, moved: &Decoded, shape: &CallShape, registers: u16) -> Option<Decoded> {
    let window = row.dst.filter(|register| *register >= registers)?;
    let moves_it = moved.kind == Kind::Move && moved.dst == Some(shape.result) && matches!(moved.a, Some(Source::Register(source)) if source == window);
    (moves_it && computes(&row, registers, Some(window))).then(|| Decoded { dst: Some(shape.result), ..row })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::{Op, Operand, RETURN_REGISTER};
    use crate::staged::call::{CellFacts, ENTRY_ACTIVATES};
    use oxc_ast::ast::BinaryOperator;

    const SHAPE: CallShape = CallShape { call_pc: 9, callee: 1, this: 2, window: 3, argc: 2, result: 6, returning: false, form: super::CallForm::Plain };
    /// The caller's registers: the callee's window starts here.
    const BASE: Register = 8;
    const IDENTITY: u64 = 7;
    const CALLER: u32 = 100;
    const CALLEE: u32 = 101;

    fn facts() -> DirectFacts {
        let cell = CellFacts { registers: 4, room: 4, params: 2, entry: ENTRY_ACTIVATES as u8 };
        DirectFacts { callee: IDENTITY, cell: 0x1000, validity: 5, facts: cell, descriptor: 0x2000, captures: 0, global_this: None, registers: 4, construction: false, frame_bytes: 32 }
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

    fn callee<'f>(code: &'f [WideInstruction], homes: &'f [(u16, Register)]) -> Callee<'f> {
        Callee { function: CALLEE, identity: Value::from_bits(IDENTITY), code, registers: 4, homes, numbers: &no_numbers }
    }

    const HOMES: [(u16, Register); 2] = [(0, 0), (1, 1)];

    #[test]
    fn a_small_arithmetic_callee_collapses_to_one_row_over_the_argument_registers() {
        let (code, mut context) = (sum(), Context::root(CALLER, BASE));
        let site = InlineSite::plan(&SHAPE, &facts(), &callee(&code, &HOMES), &mut context).expect("inlined");
        assert_eq!(site.rows.len(), 2, "the add into a window register, and the move to the result");
        let row = site.collapsed(&SHAPE, BASE).expect("one row");
        assert_eq!((row.kind, row.dst), (Kind::Add, Some(SHAPE.result)));
        assert!(matches!((row.a, row.b), (Some(Source::Register(3)), Some(Source::Register(4)))), "parameters read the caller's argument registers");
    }

    #[test]
    fn a_site_with_a_jump_or_a_window_register_keeps_its_call() {
        let code = vec![
            WideInstruction::new(Op::Return, 0, 0, 0, 0),
            WideInstruction::new(Op::Move, 0, 1, 0, 0),
            WideInstruction::new(Op::Return, 0, 0, 0, 0),
        ];
        let mut context = Context::root(CALLER, BASE);
        let site = InlineSite::plan(&SHAPE, &facts(), &callee(&code, &HOMES), &mut context).expect("planned");
        assert!(site.collapsed(&SHAPE, BASE).is_none(), "a copied parameter, a jump and several returns are no one row");
    }

    #[test]
    fn size_budget_opcode_recursion_and_depth_are_bounded_by_the_policy() {
        let policy = &TIERS.stitch;
        let long = vec![WideInstruction::new(Op::Move, 2, 0, 0, 0); policy.max_instructions + 1];
        let plan = |code: &[WideInstruction], context: &mut Context| InlineSite::plan(&SHAPE, &facts(), &callee(code, &HOMES), context).err();
        assert_eq!(plan(&long, &mut Context::root(CALLER, BASE)), Some(Refused::Size));
        let mut spent = Context::root(CALLER, BASE);
        spent.budget = 0;
        assert_eq!(plan(&sum(), &mut spent), Some(Refused::Budget));
        let throws = vec![WideInstruction::new(Op::Throw, 0, 0, 0, 0)];
        assert_eq!(plan(&throws, &mut Context::root(CALLER, BASE)), Some(Refused::Opcode(Class::Throw)));
        assert_eq!(plan(&sum(), &mut Context::root(CALLEE, BASE)), Some(Refused::Recursion));
        let mut deep = Context::root(CALLER, BASE);
        deep.chain.extend((0..policy.max_depth as u32).map(|function| function + CALLER + 1));
        assert_eq!(plan(&sum(), &mut deep), Some(Refused::Depth));
        let (few, code) = ([HOMES[0], (2, 2)], sum());
        let missing = Callee { homes: &few, ..callee(&code, &HOMES) };
        assert_eq!(InlineSite::plan(&SHAPE, &facts(), &missing, &mut Context::root(CALLER, BASE)).err(), Some(Refused::Arguments));
        let record = DirectFacts { callee: IDENTITY + 1, ..facts() };
        assert_eq!(InlineSite::plan(&SHAPE, &record, &callee(&code, &HOMES), &mut Context::root(CALLER, BASE)).err(), Some(Refused::Identity));
    }

    #[test]
    fn a_refused_site_reserves_nothing_and_an_accepted_one_takes_budget() {
        let mut context = Context::root(CALLER, BASE);
        let budget = context.budget;
        let throws = vec![WideInstruction::new(Op::Throw, 0, 0, 0, 0)];
        assert!(InlineSite::plan(&SHAPE, &facts(), &callee(&throws, &HOMES), &mut context).is_err());
        assert_eq!(context.budget, budget);
        let code = sum();
        InlineSite::plan(&SHAPE, &facts(), &callee(&code, &HOMES), &mut context).expect("inlined");
        assert_eq!(context.budget, budget - code.len());
    }
}
