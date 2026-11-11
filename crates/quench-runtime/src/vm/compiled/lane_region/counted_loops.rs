//! Counted loops: a loop whose header test bounds one induction register against a value the
//! loop never changes, run once per iteration through straight-line code. Every index register
//! that steps a fixed count per iteration then has a closed form in the iteration number, so
//! its accesses' range follows from the loop's entry values (`CountedLoop::uses`).
//!
//! With `i0` the driver's value on entry and `bound` the test's, the test passes on iterations
//! `0..trips` where `trips = bound - i0 + 1` (`<=`) or `bound - i0` (`<`), floored at zero. A
//! use of a register `r` that holds `r0` on entry and steps `steps` times per iteration reads
//! `r0 + steps * k + before` on iteration `k`.

use super::*;
use crate::vm::operations::NumberRelation;

/// One element access whose index register has a closed form in the iteration number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct IndexUse {
    pub(super) pc: usize,
    /// The array register the access indexes.
    pub(super) array: Register,
    /// The register that steps: the index itself, or the one the index copies.
    pub(super) register: Register,
    /// Steps of `register` per iteration.
    pub(super) steps: u32,
    /// Steps of `register` in the iteration before this access reads it.
    pub(super) before: u32,
    pub(super) write: bool,
}

/// A loop's induction driver, its bound and the accesses with closed-form indices.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct CountedLoop {
    pub(super) header: usize,
    pub(super) test: usize,
    pub(super) driver: Register,
    pub(super) bound: Operand,
    /// The pc of the bound register's capture read inside the loop, when it has one.
    pub(super) bound_read: Option<usize>,
    pub(super) inclusive: bool,
    pub(super) uses: Vec<IndexUse>,
}

/// The pcs (offsets in `body`) of `register`'s increments when every write of it in the loop
/// is a `++register`, or `None` when some write is not.
fn increments(body: &[WideInstruction], register: Register) -> Option<Vec<usize>> {
    let mut steps = Vec::new();
    for (pc, instruction) in body.iter().enumerate() {
        if instruction.op().class().eq(Class::IncDec) && instruction.result_register() == register
            && instruction.register_b() == register && instruction.boolean_flag() == Some(false) { steps.push(pc); }
        else if writes(*instruction).contains(&register) { return None; }
    }
    Some(steps)
}

/// How many of `steps` run before offset `pc`.
fn before(steps: &[usize], pc: usize) -> u32 { steps.partition_point(|step| *step < pc) as u32 }

/// The register an index operand steps through at `pc`, with its steps before `pc` and per
/// iteration: the register itself, or the sole `Move` source it copies (the copy then reads
/// the source as of the move).
fn closed_form(body: &[WideInstruction], at: usize, index: Register) -> Option<(Register, u32, u32)> {
    let writers: Vec<usize> = (0..body.len()).filter(|pc| writes(body[*pc]).contains(&index)).collect();
    let (register, read_at) = match writers.as_slice() {
        [mover] if body[*mover].op().class().eq(Class::Move) && *mover < at => (body[*mover].register_b(), *mover),
        _ => (index, at),
    };
    let steps = increments(body, register)?;
    (!steps.is_empty()).then(|| (register, steps.len() as u32, before(&steps, read_at)))
}

/// The capture read supplying a test's `bound` (`Some(pc)`), or `None` where it is an int32
/// constant or a register the loop never writes; `None` in the result where it is neither.
fn bound_source(body: &[WideInstruction], constants: &[crate::bytecode::Constant], header: usize, test: usize, bound: Operand) -> Option<Option<usize>> {
    match bound.kind()? {
        OperandKind::Constant => {
            constants.get(usize::from(bound.payload())).and_then(super::super::facts::immediate).filter(|value| super::super::facts::is_int32(*value))?;
            Some(None)
        }
        OperandKind::Register => match (0..body.len()).filter(|pc| writes(body[*pc]).contains(&bound.payload())).collect::<Vec<_>>().as_slice() {
            [] => Some(None),
            [read] if *read < test && body[*read].op().class().eq(Class::LoadCapture) => Some(Some(header + read)),
            _ => None,
        },
        _ => None,
    }
}

/// The test, driver and bound of the loop `body` (from `header`): the one exit test, an
/// `i < bound` or `i <= bound` over a driver stepping once per iteration, after the test, against
/// a constant or a register the loop writes only by one capture read before the test.
fn driver_test(body: &[WideInstruction], constants: &[crate::bytecode::Constant], header: usize, back: usize)
    -> Option<(usize, Register, Operand, Option<usize>, bool)> {
    let latch = body.last()?;
    if !latch.op().class().eq(Class::Jump) || latch.jump_target() as usize != header { return None; }
    let jumps: Vec<usize> = (0..body.len() - 1).filter(|pc| body[*pc].op().control_flow_layout().jumps()).collect();
    let [test] = jumps.as_slice() else { return None };
    let instruction = body[*test];
    if !instruction.op().class().eq(Class::JumpBinaryFalse) || instruction.jump_target() as usize != back + 1 { return None; }
    let inclusive = match NumberRelation::from_operator(instruction.binary_operator_field())? {
        NumberRelation::LessThan => false,
        NumberRelation::LessEqual => true,
        _ => return None,
    };
    let driver = instruction.operand_b().register_index()?;
    let steps = increments(body, driver)?;
    if steps.len() != 1 || before(&steps, *test) != 0 { return None; }
    let bound = instruction.operand_c();
    let bound_read = bound_source(body, constants, header, *test, bound)?;
    Some((header + *test, driver, bound, bound_read, inclusive))
}

/// The element accesses of `body` whose index register has a closed form (`closed_form`), on
/// an array the loop does not write.
fn index_uses(body: &[WideInstruction], header: usize) -> Vec<IndexUse> {
    body.iter().enumerate().filter_map(|(pc, instruction)| {
        let (array, index, write) = match instruction.op().class() {
            Class::GetIndex => (instruction.operand_b().register_index()?, instruction.operand_c().register_index()?, false),
            Class::SetIndex => (instruction.register_b(), instruction.register_c(), true),
            _ => return None,
        };
        let (register, steps, before) = closed_form(body, pc, index)?;
        (!writes_any(body, array)).then_some(IndexUse { pc: header + pc, array, register, steps, before, write })
    }).collect()
}

/// The loop `header..=back` as a counted loop, if it is one.
pub(super) fn counted_loop(instructions: &[WideInstruction], constants: &[crate::bytecode::Constant], header: usize, back: usize) -> Option<CountedLoop> {
    let body = instructions.get(header..=back)?;
    let (test, driver, bound, bound_read, inclusive) = driver_test(body, constants, header, back)?;
    Some(CountedLoop { header, test, driver, bound, bound_read, inclusive, uses: index_uses(body, header) })
}

/// Whether the loop writes `register`.
fn writes_any(body: &[WideInstruction], register: Register) -> bool {
    body.iter().any(|instruction| writes(*instruction).contains(&register))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loops(source: &str, name: &str) -> Vec<CountedLoop> {
        let program = crate::Engine::specialize(source, "counted.js").unwrap();
        let id = program.functions.iter().position(|body| body.header().name
            .is_some_and(|atom| &program.atoms[atom as usize] == name)).unwrap();
        let unit = super::super::super::lane_unit::unit(&program, id as u32, program.function(id), &|_, _| Vec::new(),
            &|callee| program.function_get(callee as usize)).unwrap();
        let instructions = &unit.instructions;
        instructions.iter().enumerate().filter_map(|(back, jump)| {
            let header = jump.op().control_flow_layout().jumps().then(|| jump.jump_target() as usize).filter(|header| *header <= back)?;
            counted_loop(instructions, &program.constants, header, back)
        }).collect()
    }

    const LIN_SOLVE: &str = "var width=8,rowSize=10;function lin_solve(x,x0,a){var lastRow=0,currentRow=10,nextRow=20,lastX=x[currentRow];++currentRow;for(var i=1;i<=width;i++)lastX=x[currentRow]=(x0[currentRow]+a*(lastX+x[++currentRow]+x[++lastRow]+x[++nextRow]));}lin_solve([],[],1);";

    #[test]
    fn lin_solve_inner_loop_is_counted_with_closed_form_indices() {
        let found = loops(LIN_SOLVE, "lin_solve");
        let [counted] = found.as_slice() else { panic!("{found:?}") };
        assert!(counted.inclusive);
        assert_eq!(counted.uses.len(), 5, "{:?}", counted.uses);
        assert!(counted.uses.iter().all(|used| used.steps == 1));
        assert_eq!(counted.uses.iter().filter(|used| used.write).count(), 1);
        let befores: Vec<u32> = counted.uses.iter().map(|used| used.before).collect();
        assert_eq!(befores, [0, 1, 1, 1, 0], "x0[cur], x[++cur], x[++last], x[++next], x[cur]=");
    }

    #[test]
    fn branches_and_changing_bounds_are_not_counted() {
        let branching = "function f(x,n){var s=0;for(var i=0;i<n;i++){if(x[i]>0)s++;}return s;}f([],1);";
        assert!(loops(branching, "f").is_empty());
        let moving = "function f(x,n){var s=0;for(var i=0;i<n;i++){n=n-1;s=s+x[i];}return s;}f([],4);";
        assert!(loops(moving, "f").is_empty());
        let decrement = "function f(x,n){var s=0;for(var i=n;i>0;i--){s=s+x[i];}return s;}f([],4);";
        assert!(loops(decrement, "f").is_empty());
    }
}
