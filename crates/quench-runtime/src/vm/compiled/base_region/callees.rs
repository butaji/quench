//! The callees a whole function's body calls by their entries, and which of its call sites the
//! stitcher inlines (`call_plan::inline`) instead of chaining a call.
//!
//! A call whose callee is small and monomorphic is planned as the callee's rows over registers of
//! the caller and its window (`InlineSite`), which the stitch splices in place of the call
//! (`stitch::stitch`) behind a row that proves the callee register holds the function the site
//! recorded. The callee's body is not named by the code, but its identity is: the site's
//! descriptor stays among the method's direct callees, so a collection that frees the callee
//! retires the body (`ProgramStore::retire_dead_native_calls`), and the identity a word names is
//! never a reused one.

use super::*;
use super::call_plan::inline::{Callee, Context, InlineSite};
use super::call_plan::{CallForm, CallShape, DirectFacts};
use super::super::{DirectCallee, DirectCandidates};
use crate::bytecode::Function;
use crate::compile::liveness::RegisterMasks;
use crate::heap::GlobalFact;
use std::rc::Rc;

/// A function a site may inline: its instructions, registers, the register each parameter is
/// passed to and the registers live before each instruction.
pub(in crate::vm::compiled) struct InlineBody {
    code: Vec<WideInstruction>,
    registers: u16,
    homes: Vec<(u16, Register)>,
    live: Rc<RegisterMasks>,
    /// The Number constants the callee's instructions name, by pool index: its rows fold them as the
    /// caller's rows fold the caller's (the caller's region holds only its own).
    numbers: std::collections::BTreeMap<usize, f64>,
}

impl InlineBody {
    /// The body of `function`, whose liveness is `live` and whose constant pool is `constants`; none
    /// where liveness does not cover it.
    pub(in crate::vm::compiled) fn of(function: &Function, live: Option<&Rc<RegisterMasks>>, constants: &[crate::value::Value]) -> Option<Self> {
        let home = |argument: u16| function.parameter_registers.map(|base| base + argument).or_else(|| function.promoted_register(argument));
        let homes = (0..function.params).filter_map(|argument| home(argument).map(|home| (argument, home))).collect();
        let code: Vec<WideInstruction> = function.instructions().collect();
        let numbers = code.iter().flat_map(|instruction| named_constants(*instruction))
            .filter_map(|index| Some((index, constants.get(index)?.as_number()?)))
            .collect();
        Some(Self { code, registers: function.registers, homes, live: live?.clone(), numbers })
    }
}

/// The constant-pool indexes `instruction` may name: a loaded constant and its constant operands.
fn named_constants(instruction: WideInstruction) -> impl Iterator<Item = usize> {
    use crate::bytecode::{Class, OperandKind};
    let loaded = (instruction.op().class() == Class::LoadConst).then(|| instruction.constant_index());
    let operand = |operand: crate::bytecode::Operand| (operand.kind() == Some(OperandKind::Constant)).then(|| usize::from(operand.payload()));
    let binary = matches!(instruction.op().class(), Class::Binary | Class::NumericAdd | Class::NumericMultiply | Class::JumpBinaryFalse);
    let operands = binary.then(|| [operand(instruction.operand_b()), operand(instruction.operand_c())]).into_iter().flatten().flatten();
    loaded.into_iter().chain(operands)
}

/// The callees a body calls by their entries: the descriptors its code names, which its method
/// owns for as long as the code runs, by PC which of them a call site calls, and the function
/// each runs where a site may inline it. The default has no call site: every call stays a call.
#[derive(Default)]
pub(in crate::vm::compiled) struct Callees {
    pub(in crate::vm::compiled) owned: Box<[DirectCallee]>,
    at: Vec<Option<usize>>,
    bodies: Vec<Option<InlineBody>>,
    /// By PC, the Stable fact holding the callee its call site recorded: the global cell its
    /// callee register is read from (`Vm::stable_callees`).
    stable: Vec<Option<GlobalFact>>,
}

/// Whether the call at `instruction` is a plain call a body's call chain makes: a call whose result
/// is its register's, or one that returns its result (`return f(x)`), which the row after it
/// completes with. The other forms record no callee (`call_plan::CallForm`).
pub(super) fn chains(instruction: WideInstruction) -> bool {
    CallForm::of(instruction.split_return().0) == Some(CallForm::Plain)
}

/// The callee the call site `instruction` makes calls, if its body's call chain makes it: the
/// latest its site reached.
fn called<'a>(instruction: WideInstruction, site: &'a DirectCandidates) -> Option<&'a DirectCallee> {
    site.first().filter(|_| chains(instruction))
}

impl Callees {
    /// The function each call site of `code` was recorded calling (`candidates`, by PC).
    pub(in crate::vm::compiled) fn recorded(code: &[WideInstruction], candidates: &[DirectCandidates]) -> Vec<Option<crate::value::Value>> {
        code.iter().zip(candidates).map(|(instruction, site)| called(*instruction, site).map(|callee| callee.callee)).collect()
    }

    /// The callee each call site of `code` calls (`candidates`, by PC), `body_of` the function it
    /// runs, and `stable` the Stable fact holding it by PC.
    pub(in crate::vm::compiled) fn of(code: &[WideInstruction], candidates: &[DirectCandidates], body_of: &dyn Fn(&DirectCallee) -> Option<InlineBody>, stable: Vec<Option<GlobalFact>>) -> Self {
        let mut owned = Vec::new();
        let at = code.iter().zip(candidates).map(|(instruction, site)| {
            owned.push(called(*instruction, site)?.clone());
            Some(owned.len() - 1)
        }).collect();
        let bodies = owned.iter().map(body_of).collect();
        Self { owned: owned.into_boxed_slice(), at, bodies, stable }
    }

    /// Whether every call of `code` is a direct one (`LaneMethod::closed`).
    pub(in crate::vm::compiled) fn closed(&self, code: &[WideInstruction]) -> bool {
        code.iter().zip(&self.at).all(|(instruction, callee)| !instruction.op().is_call() || callee.is_some())
    }

    /// What each call site recorded of its callee, by PC.
    ///
    /// # Safety
    /// Each owned callee's `cell` is the address of its function's live entry cell.
    pub(super) unsafe fn facts(&self) -> Vec<Option<DirectFacts>> {
        // SAFETY: the caller's contract.
        self.at.iter().map(|callee| callee.map(|at| unsafe { DirectFacts::of(&self.owned[at]) })).collect()
    }

    /// The call `shape` planned as its callee's rows (`facts`: what each site recorded), in
    /// `context`, which it spends from; none where the callee stays a call.
    pub(super) fn inlined_call(&self, facts: &[Option<DirectFacts>], region: &LoopRegion, context: &mut Context, shape: &CallShape) -> Option<InlineSite> {
        let at = (*self.at.get(shape.call_pc as usize)?)?;
        let (callee, body) = (&self.owned[at], self.bodies[at].as_ref()?);
        let numbers = |index: usize| body.numbers.get(&index).copied().or_else(|| region.number_constant(index));
        let inlinable = Callee { function: callee.target.function, identity: callee.callee, code: &body.code, registers: body.registers, homes: &body.homes, numbers: &numbers, live: &body.live, stable: self.stable[shape.call_pc as usize] };
        let mut trial = context.clone();
        let site = InlineSite::plan(shape, facts[shape.call_pc as usize].as_ref()?, &inlinable, &mut trial).ok()?;
        *context = trial;
        note_inlined();
        Some(site)
    }
}
