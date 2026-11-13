//! Rewrites a load leaves to the first compile of the functions it releases.
use super::replay::{Rewrite, RewriteStart};
use super::*;

/// The instructions a rewrite consumes for each site it adds: a field site fuses a read of a
/// field with the store of its result, and no later rewrite fuses what a field site made again.
pub(crate) const FIELD_SITE_WINDOW: usize = 2;
/// The instructions of the window a superinstruction fuses.
pub(crate) const SUPERINSTRUCTION_WINDOW: usize = rewrite::CONST_ARRAY_OBJECT2.len();

impl<'a> Compiler<'a> {
    /// Runs the rewrites of replayable `function`, whose record is released, unless its first
    /// compile can run them: they add sites, which a replay finds in the program's tables
    /// where the load left them, so the load either adds them here or reserves room for the
    /// replay to add them (`deferred_rewrite_code`).
    pub(super) fn start_rewrite(&mut self, function: &mut BcFunction) -> replay::Rewrite {
        if self.defers_rewrites() {
            self.deferred_rewrite_code += function.code.len();
            return replay::Rewrite::Deferred;
        }
        let start = replay::RewriteStart {
            field_sites: self.field_sites.len() as u32,
            superinstructions: self.superinstructions.len() as u32,
        };
        self.rewrite_function(function);
        replay::Rewrite::Loaded(start)
    }

    /// Whether the load leaves rewrites to the first compile of the functions it releases:
    /// when their records are dropped for good, not encoded (`load_encoding`) or compared
    /// with their replays (`lowers_at_load`), and a program records them nowhere else.
    fn defers_rewrites(&self) -> bool {
        crate::DYNAMIC_EXECUTION_ENABLED
            && self.mode == SpecializationMode::Enabled
            && self.replayable_source
            && self.load_encoding.is_none()
            && !lowers_at_load()
            && !cfg!(feature = "profile-aggregate")
    }

    /// Starts the rewrites of the load's function in `slot`, a replayable one's (`replay`) as
    /// `start_rewrite` settles them; the function, released from `slot`, when they are deferred.
    pub(super) fn rewrite_replayable(&mut self, slot: &mut Option<BcFunction>, replay: Option<&mut replay::FunctionReplay>) -> Option<Box<replay::ReleasedFunction>> {
        let function = slot.as_mut()?;
        let Some(replay) = replay else {
            self.rewrite_function(function);
            return None;
        };
        replay.rewrite = self.start_rewrite(function);
        matches!(replay.rewrite, Rewrite::Deferred).then(|| slot.take().map(replay::ReleasedFunction::of))?
    }

    /// Settles `function`, whose rewrites started as `rewrite`. The record is released after
    /// and only its header stays, so a load that drops it settles just the dispatch class the
    /// header keeps. One that encodes it finishes it as a replay would, against tables whose
    /// entries for it are final (`lower_settled`); `verify-replay` keeps the whole record to
    /// compare each replay with.
    pub(super) fn settle_early(&mut self, rewrite: &Rewrite, function: &mut BcFunction) {
        if matches!(rewrite, Rewrite::Deferred) {
            // Its replay settles it after its rewrites.
        } else if cfg!(feature = "verify-replay") || self.load_encoding.is_some() {
            let undefined_constant = self.constant(Constant::Undefined);
            self.lower_function(function, undefined_constant);
            liveness::mark_register_roots(std::iter::once(function), self.method_arguments(), self.field_sites.as_slice(), self.superinstructions.as_slice());
        } else {
            self.settle_dispatch(function);
        }
    }

    /// Applies function `id`'s replayed rewrites as `rewrite_plan` settles them.
    pub(super) fn rewrite_replayed(&mut self, program: &'a crate::bytecode::ResidualProgram, id: u32, rewrite: Rewrite, function: &mut BcFunction) {
        match rewrite_plan(program, id, rewrite, function.code.len()) {
            RewritePlan::Skip => {}
            RewritePlan::Replay(start) => {
                self.field_sites.skip_to(start.field_sites as usize);
                self.superinstructions.skip_to(start.superinstructions as usize);
                self.rewrite_function(function);
            }
            RewritePlan::Append => {
                self.field_sites = SiteTable::grow(&program.field_sites);
                self.superinstructions = SiteTable::grow(&program.superinstructions);
                self.rewrite_function(function);
            }
        }
    }
}

/// How a replay treats the rewrites of one function.
enum RewritePlan {
    /// The load, or an earlier replay, put their sites at these cursors.
    Replay(RewriteStart),
    /// This replay appends their sites to the program's tables.
    Append,
    /// They stay unrun: the program's room for sites is short of what they may add. The
    /// function runs on its unrewritten code, as when specialization is off.
    Skip,
}

fn rewrite_plan(program: &crate::bytecode::ResidualProgram, id: u32, rewrite: Rewrite, code: usize) -> RewritePlan {
    match rewrite {
        Rewrite::Loaded(start) => RewritePlan::Replay(start),
        Rewrite::Deferred => match program.settled_rewrite(id) {
            Some(Some(start)) => RewritePlan::Replay(start),
            Some(None) => RewritePlan::Skip,
            None => {
                let (fields, supers) = (&program.field_sites, &program.superinstructions);
                let start = RewriteStart { field_sites: fields.len() as u32, superinstructions: supers.len() as u32 };
                let fits = fields.spare() >= code / FIELD_SITE_WINDOW && supers.spare() >= code / SUPERINSTRUCTION_WINDOW;
                program.record_rewrite(id, fits.then_some(start));
                if fits { RewritePlan::Append } else { RewritePlan::Skip }
            }
        },
    }
}


impl crate::bytecode::ResidualProgram {
    /// What an earlier replay of function `id` settled: its sites' start, or `None` for their
    /// skip. Read out of the cell before a plan records its own choice.
    fn settled_rewrite(&self, id: u32) -> Option<Option<RewriteStart>> {
        self.rewrite_ranges.borrow().get(id as usize).copied().flatten()
    }

    /// Notes where the sites of function `id`'s deferred rewrites start, or `None` if they stayed unrun.
    fn record_rewrite(&self, id: u32, start: Option<RewriteStart>) {
        let mut ranges = self.rewrite_ranges.borrow_mut();
        let len = ranges.len().max(id as usize + 1);
        ranges.resize(len, None);
        ranges[id as usize] = Some(start);
    }
}
