//! A first-call chunk appended to its stub's program (`Engine::append_first_call_chunk`),
//! when the program has room for it, so a call between the chunk's functions and the
//! program's stays within one program. The tables the program's residual and its store entry
//! keep are laid out for their reserved capacity (`ProgramStore::accepts_chunks`), so what
//! the chunk adds is a few entries and the values of its constants.
use super::*;

impl<H: Host> Vm<H> {
    /// The function of stub `function` of program `program`, whose residual is `p`, compiled
    /// into the program, or `None` where the chunk must be a program of its own.
    pub(super) fn append_first_chunk(&mut self, p: &ResidualProgram, (program, function): (ProgramId, u32)) -> Option<FunctionKind> {
        if !self.programs.accepts_chunks(program, p) {
            return None;
        }
        let delta = crate::Engine::append_first_call_chunk(p, function, &*self)?;
        if !p.has_room_for(&delta) {
            return None;
        }
        // The names the chunk adds take the ids it compiled them under.
        for index in delta.first_atom..delta.atoms.len() {
            let atom = self.intern_atom(&delta.atoms[index]);
            debug_assert_eq!(atom as usize, index, "a chunk's names follow the VM's");
        }
        let first_constant = delta.base.constants;
        let target = p.append_chunk(delta);
        let constants: Vec<Value> = p.constants[first_constant..].iter().map(|constant| self.materialize_constant(constant)).collect();
        self.programs.push_constants(program, constants.into_iter());
        if program == self.active_program {
            self.set_active_program(program);
        }
        Some(FunctionKind::User(program, target))
    }
}
