//! Futamura's projections over Quench's one interpreter.
//!
//! The interpreter is the shared semantic operations: one per opcode, reached
//! through `Vm::resolve_step_operation`. A residual program is the static
//! input. A [`Specializer`] is the first projection: it residualizes the
//! interpreter for one program, folding exactly the inputs its [`Division`]
//! names as known (Ershov's mixed computation). A generating extension is the
//! second projection's unit: from one instruction's static inputs it produces
//! that instruction's residual code, so applying it across a program is a
//! compiler derived from the interpreter. Rust monomorphization produces the
//! per-opcode semantic entries at build time, and copy-and-patch stencils
//! compile the residual shapes once per process (`native_stencil`).
//!
//! The contract, the division table and the plan live in
//! `docs/v2/specialization.md`.

use crate::bytecode::{BindingTime, ControlFlowLayout, Function, Op, ResidualProgram};

/// Which binding times a specializer treats as known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Division {
    /// Build-time facts only: static inputs and storage locations. AOT images
    /// and eager publication need no training run, so they never fold feedback.
    Offline,
    /// Also the contents of feedback sites, each behind a guard whose failure
    /// resumes the interpreter at the exact PC: the JIT's regions.
    #[cfg_attr(not(test), expect(dead_code, reason = "the guarded-region specializer adopts it (TOD-1237)"))]
    Online,
}

impl Division {
    pub(crate) const fn folds(self, time: BindingTime) -> bool {
        match time {
            BindingTime::Static | BindingTime::Location => true,
            BindingTime::Site => matches!(self, Self::Online),
        }
    }

    /// Whether this division knows every input of `op`, so its generating
    /// extension may residualize the instruction's meaning beyond a call to
    /// the semantic entry.
    pub(crate) const fn folds_instruction(self, op: Op) -> bool {
        let times = op.input_binding_times();
        let mut index = 0;
        while index < times.len() {
            if !self.folds(times[index]) {
                return false;
            }
            index += 1;
        }
        true
    }
}

/// Futamura's first projection: the interpreter specialized to one program.
/// Implementations differ in their division and in how the residual code is
/// published (linked object, Cranelift in live memory, copied stencils).
pub(crate) trait Specializer {
    type Residual;
    const DIVISION: Division;

        fn specialize(self, program: &ResidualProgram) -> Result<Self::Residual, String>;
}

/// Where control continues after an instruction's semantic entry returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Continuation {
    /// At the next PC when the entry reports it, so the residual may place
    /// that instruction directly after this one; any other PC dispatches.
    Fallthrough,
    /// Wherever the entry reports, through exact-PC dispatch.
    Dispatch,
    /// The specializer computed the successor: every input is static and the
    /// instruction's only meaning is this transfer, so no semantic entry runs.
    Static(usize),
}

/// The static computation of the call-threaded division for one instruction:
/// everything its residual folds, computed once per instruction. Residual
/// code addresses `operands` directly, so a site stays where it was built.
pub(crate) struct InstructionSite {
    pub(crate) pc: usize,
    pub(crate) op: Op,
    pub(crate) operands: super::NativeJsOperands,
    pub(crate) continuation: Continuation,
}

impl InstructionSite {
    /// Whether the residual calls the opcode's semantic entry here.
    pub(crate) fn calls_semantic_entry(&self) -> bool {
        !matches!(self.continuation, Continuation::Static(_))
    }
}

/// The successor of an instruction whose meaning the division computes at
/// specialization time. A forward `Jump` is pure control transfer; a backward
/// one keeps its semantic entry, which polls interrupts and collection on every
/// loop iteration. A PC where the VM may ask native code to stop (the end of
/// parameter initialization) keeps its entry, which performs that stop.
fn static_successor(
    division: Division,
    function: &Function,
    instruction: crate::bytecode::WideInstruction,
    pc: usize,
) -> Option<usize> {
    let op = instruction.op();
    if op != Op::Jump || !division.folds_instruction(op) {
        return None;
    }
    let stops_here = function.parameter_end_pc != 0 && function.parameter_end_pc as usize == pc;
    let target = instruction.jump_target() as usize;
    (!stops_here && target > pc && target < function.code.len()).then_some(target)
}

pub(crate) fn instruction_sites(division: Division, function: &Function) -> Vec<InstructionSite> {
    let count = function.code.len();
    function
        .code
        .iter()
        .copied()
        .enumerate()
        .map(|(pc, packed)| {
            let instruction = super::dispatch_fast::decoded_instruction(function, packed);
            let op = instruction.op();
            let continuation = if let Some(target) = static_successor(division, function, instruction, pc) {
                Continuation::Static(target)
            } else if op.control_flow_layout() == ControlFlowLayout::Fallthrough && pc + 1 < count {
                Continuation::Fallthrough
            } else {
                Continuation::Dispatch
            };
            InstructionSite {
                pc,
                op,
                operands: super::NativeJsOperands::new(
                    instruction.a(),
                    instruction.b(),
                    instruction.c(),
                    instruction.imm(),
                ),
                continuation,
            }
        })
        .collect()
}

/// The call-threaded division's generating extension: residual code for each
/// function's instruction sites, then the published program. Each publication
/// (object, Cranelift live code, stencils) is one implementation.
pub(crate) trait CallThreadedExtension {
    type Residual;

    fn function(&mut self, function_id: u32, sites: Vec<InstructionSite>) -> Result<(), String>;
    fn finish(self) -> Result<Self::Residual, String>;
}

/// The call-threaded specializer: dispatch, PCs, operand records and
/// successors are folded; each instruction's meaning is its opcode's
/// monomorphized semantic entry.
pub(crate) struct CallThreaded<E>(pub(crate) E);

impl<E: CallThreadedExtension> Specializer for CallThreaded<E> {
    type Residual = E::Residual;
    const DIVISION: Division = Division::Offline;

    fn specialize(self, program: &ResidualProgram) -> Result<Self::Residual, String> {
        program.validate_unless_proven()?;
        let mut extension = self.0;
        for id in 0..program.function_count() {
            let function_id = u32::try_from(id).map_err(|_| "native function count exceeds u32")?;
            extension.function(function_id, instruction_sites(Self::DIVISION, program.function(id)))?;
        }
        extension.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn division_is_total_and_offline_never_folds_sites() {
        for index in 0..Op::COUNT {
            let op = Op::from_index(index).unwrap();
            let times = op.input_binding_times();
            assert!(Division::Online.folds_instruction(op), "{op:?}: online folds every input");
            let reads_site = times.contains(&BindingTime::Site);
            assert_eq!(
                Division::Offline.folds_instruction(op),
                !reads_site,
                "{op:?}: offline folds exactly the site-free instructions"
            );
            for (field, time) in crate::bytecode::InstructionField::ALL.iter().zip(times) {
                if op.field_layout(*field).is_register_field() {
                    assert_eq!(time, BindingTime::Location, "{op:?}: register fields are locations");
                }
            }
        }
    }

    #[test]
    fn feedback_sites_and_static_facts_are_classified_from_the_opcode_table() {
        assert!(!Division::Offline.folds_instruction(Op::GetField), "field lookups carry an inline-cache site");
        assert!(!Division::Offline.folds_instruction(Op::CallMethod), "method calls carry a method site");
        for op in [Op::Jump, Op::Move, Op::LoadConst, Op::Return, Op::Binary] {
            assert!(Division::Offline.folds_instruction(op), "{op:?} is decided by the program alone");
        }
        assert_eq!(Op::LoadConst.input_binding_times()[3], BindingTime::Static);
        assert_eq!(Op::LoadLocal.input_binding_times()[3], BindingTime::Location);
    }

        #[test]
    fn instruction_sites_fold_operands_and_continuations_once() {
        let program = crate::Engine::specialize_unspecialized(
            "function f(x) { if (x) return 1; return x + 2; } f(3);",
            "sites.js",
        )
        .unwrap();
        for id in 0..program.function_count() {
            let function = program.function(id);
            let sites = instruction_sites(Division::Offline, function);
            assert_eq!(sites.len(), function.code.len());
            for site in &sites {
                let falls = site.op.control_flow_layout() == ControlFlowLayout::Fallthrough
                    && site.pc + 1 < sites.len();
                assert_eq!(site.continuation == Continuation::Fallthrough, falls);
                if let Continuation::Static(target) = site.continuation {
                    assert_eq!(site.op, Op::Jump);
                    assert!(target > site.pc, "only forward jumps are computed statically");
                }
            }
            assert!(sites.last().is_none_or(|site| site.continuation != Continuation::Fallthrough));
        }
    }

    #[test]
    fn forward_jumps_are_computed_and_backward_jumps_keep_their_entry() {
        let program = crate::Engine::specialize_unspecialized(
            "let s = 0; for (let i = 0; i < 4; i++) { s += i ? 1 : 2; } s;",
            "jumps.js",
        )
        .unwrap();
        let sites = instruction_sites(Division::Offline, program.function(0));
        let jumps: Vec<_> = sites.iter().filter(|site| site.op == Op::Jump).collect();
        assert!(jumps.iter().any(|site| !site.calls_semantic_entry()), "the conditional's join is a forward jump");
        assert!(jumps.iter().any(|site| site.calls_semantic_entry()), "the loop's backedge keeps its entry");
    }
}
