//! Compiled evals kept by text: a global eval by its text, a direct eval also by the caller
//! facts its compilation read.
use super::*;
use crate::vm::eval::eval_source::{DirectEvalKey, EvalSource};

impl ProgramStore {
    pub(in crate::vm) fn cached_global_eval(&self, source: &EvalSource) -> Option<ProgramId> {
        let program = *self.global_eval_programs.get(source)?;
        self.entry(program).map(|_| program)
    }

    pub(in crate::vm) fn cache_global_eval(&mut self, source: EvalSource, program: ProgramId) {
        self.global_eval_programs.insert(source, program);
    }

    pub(in crate::vm) fn cached_direct_eval(&self, key: &DirectEvalKey) -> Option<ProgramId> {
        let program = *self.direct_eval_programs.get(key)?;
        self.entry(program).map(|_| program)
    }

    pub(in crate::vm) fn cache_direct_eval(&mut self, key: DirectEvalKey, program: ProgramId) {
        self.direct_eval_programs.insert(key, program);
    }

    /// Forgets the eval programs and owned scripts whose program was reclaimed.
    pub(super) fn retain_live_eval_programs(&mut self) {
        let programs = &self.programs;
        let live = |id: &ProgramId| programs.get(id.index()).is_some_and(Option::is_some);
        self.global_eval_programs.retain(|_, id| live(id));
        self.direct_eval_programs.retain(|_, id| live(id));
        self.owned_scripts.retain(|source, _| live(source));
    }
}
