//! The callees a whole function's body calls by their entries, and which of its call sites the
//! stitcher inlines (`call_plan::inline`) instead of chaining a call.
//!
//! A call whose callee is one row over the caller's own registers (`InlineSite::collapsed`) is
//! decoded as that row, which proves the callee register holds the function the site recorded
//! (`Decoded::identity`) and otherwise leaves at the call, where the interpreter makes the whole
//! call. The callee's body is not named by the code, but its identity is: the site's descriptor
//! stays among the method's direct callees, so a collection that frees the callee retires the
//! body (`ProgramStore::retire_dead_native_calls`), and the identity a word names is never a
//! reused one.

use super::*;
use super::call_plan::inline::{Callee, Context, InlineSite};
use super::call_plan::{CallShape, DirectFacts};
use super::super::{DirectCallee, DirectCandidates};
use crate::bytecode::Function;

/// A function a site may inline: its instructions, registers and the register each parameter
/// is passed to.
pub(in crate::vm::compiled) struct InlineBody {
    code: Vec<WideInstruction>,
    registers: u16,
    homes: Vec<(u16, Register)>,
}

impl InlineBody {
    pub(in crate::vm::compiled) fn of(function: &Function) -> Self {
        let home = |argument: u16| function.parameter_registers.map(|base| base + argument).or_else(|| function.promoted_register(argument));
        let homes = (0..function.params).filter_map(|argument| home(argument).map(|home| (argument, home))).collect();
        Self { code: function.instructions().collect(), registers: function.registers, homes }
    }
}

/// The callees a body calls by their entries: the descriptors its code names, which its method
/// owns for as long as the code runs, by PC which of them a call site calls, and the function
/// each runs where a site may inline it.
pub(in crate::vm::compiled) struct Callees {
    pub(in crate::vm::compiled) owned: Box<[DirectCallee]>,
    at: Vec<Option<usize>>,
    bodies: Vec<Option<InlineBody>>,
}

/// Whether the call at `instruction` is one a body's call chain makes: a call whose result is its
/// register's, or one that returns its result (`return f(x)`), which the row after it completes with.
pub(super) fn chains(instruction: WideInstruction) -> bool {
    instruction.op().class() == Class::Call && (instruction.result_flags() == 0 || instruction.only_returns_from_frame())
        && !instruction.direct_eval() && !instruction.parameter_eval()
}

impl Callees {
    /// The callee each call site of `code` calls: the latest its site reached (`candidates`, by
    /// PC), and `body_of` the function it runs.
    pub(in crate::vm::compiled) fn of(code: &[WideInstruction], candidates: &[DirectCandidates], body_of: &dyn Fn(u32) -> Option<InlineBody>) -> Self {
        let mut owned = Vec::new();
        let at = code.iter().zip(candidates).map(|(instruction, site)| {
            let callee = site.first().filter(|_| chains(*instruction))?;
            owned.push(callee.clone());
            Some(owned.len() - 1)
        }).collect();
        let bodies = owned.iter().map(|callee: &DirectCallee| body_of(callee.target.function)).collect();
        Self { owned: owned.into_boxed_slice(), at, bodies }
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

    /// The row the call `shape` decodes as where its callee is one row over the caller's
    /// `registers` (`facts`: what each site recorded), planned in `context`, which it spends from.
    pub(super) fn inlined_call(&self, facts: &[Option<DirectFacts>], region: &LoopRegion, context: &mut Context, shape: &CallShape, registers: u16) -> Option<Decoded> {
        let at = (*self.at.get(shape.call_pc as usize)?)?;
        let (callee, body) = (&self.owned[at], self.bodies[at].as_ref()?);
        let numbers = |index: usize| region.number_constant(index);
        let inlinable = Callee { function: callee.target.function, identity: callee.callee, code: &body.code, registers: body.registers, homes: &body.homes, numbers: &numbers };
        let mut trial = context.clone();
        let row = InlineSite::plan(shape, facts[shape.call_pc as usize].as_ref()?, &inlinable, &mut trial).ok()?.collapsed(shape, registers)?;
        *context = trial;
        note_inlined();
        Some(Decoded { identity: Some((shape.callee, callee.callee.to_bits())), ..row })
    }
}
