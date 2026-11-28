//! Rewrites a load leaves to the first compile of the functions it releases.
use super::replay::{Rewrite, RewriteStart, Settled};
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
        self.rewrite_now(function)
    }

    /// Runs the rewrites of `function` at the load, and says where the sites they add start.
    fn rewrite_now(&mut self, function: &mut BcFunction) -> replay::Rewrite {
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
        self.replayable_source && self.may_defer_rewrites()
    }

    /// Whether any record's rewrites may wait for its first read: nothing encodes or compares
    /// the load's records, and the rewrites run at all. A released record's rewrites stay in
    /// the load while the load encodes it (`load_encoding`), because that encoding is made
    /// from the lowered record as the load finishes it (`LoadEncoding::add`).
    pub(super) fn may_defer_rewrites(&self) -> bool {
        self.load_encoding.is_none() && self.may_defer_kept_rewrites()
    }

    /// Whether the rewrites of a kept function may wait for its first read. A kept record is not
    /// released, so the load's encoding of released records does not bind it: its record is
    /// encoded when the program is, unlowered and unrewritten (`binary::encode_program`), and its
    /// first read settles the rewrites.
    fn may_defer_kept_rewrites(&self) -> bool {
        crate::DYNAMIC_EXECUTION_ENABLED
            && !lowers_at_load()
            && !cfg!(feature = "profile-aggregate")
    }

    /// Whether some record of the program awaits its rewrites, which its first read runs: a
    /// kept one's deferred (`defers_kept_rewrites`), or a stub's chunk, appended after the load.
    pub(super) fn rewrites_pend(&self, room: &first_call::ChunkRoom) -> bool {
        !self.kept_rewrites.is_empty() || (room.reserves() && self.may_defer_rewrites())
    }

    /// Whether function `id`, which the program keeps, leaves its rewrites to its first read.
    /// A program that keeps most of its functions without running them pays for the rewrites
    /// of only the ones that run: an eval of a bundle, and a host script's kept form, which
    /// the load encodes whole (`load_encoding`) and whose records a first read decodes. The
    /// root runs at once, so its rewrites stay in the load.
    fn defers_kept_rewrites(&mut self, id: usize, function: &BcFunction) -> bool {
        let kept_form = self.eval_context.is_some() || self.load_encoding.is_some();
        let defers = kept_form && id != crate::vm::ROOT_FUNCTION_ID as usize && self.may_defer_kept_rewrites();
        if defers {
            self.deferred_rewrite_code += function.code.len();
            self.kept_rewrites.push(id as u32);
        }
        defers
    }

    /// Starts the rewrites of the load's function in `slot`, a replayable one's (`replay`) as
    /// `start_rewrite` settles them; the function, released from `slot`, when they are deferred.
    pub(super) fn rewrite_replayable(&mut self, id: usize, slot: &mut Option<BcFunction>, replay: Option<&mut replay::FunctionReplay>) -> Option<Box<replay::ReleasedFunction>> {
        let function = slot.as_mut()?;
        let Some(replay) = replay else {
            if !self.defers_kept_rewrites(id, function) {
                self.rewrite_function(function);
            }
            return None;
        };
        if self.load_encoding.is_some() {
            // The load encodes what it releases, so only a function it keeps reaches here: its
            // rewrites wait as those of a function with no replay do, and it is never released,
            // as `start_rewrite` would never defer it under the encoding.
            replay.rewrite = if self.defers_kept_rewrites(id, function) { Rewrite::Deferred } else { self.rewrite_now(function) };
            return None;
        }
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
            let solved = self.lower_function(function, undefined_constant);
            function.register_roots = liveness::register_roots_from(function, self.method_arguments(), self.field_sites.as_slice(), self.superinstructions.as_slice(), solved);
        } else {
            self.settle_dispatch_only(function);
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

/// The rewrites of kept function `id`, which its load deferred (`Compiler::defers_kept_rewrites`),
/// run as its record is first read: they append their sites to the program's tables, as a
/// replay's do, or stay unrun when the room the load reserved is short.
///
/// The load settled the record's dispatch class before the rewrites ran (`settle_dispatch`, on
/// the code it had), so the class is taken again here from the code the rewrites leave, as the
/// load's `settle_dispatch` would have taken it after them; the rewrites that fuse instructions
/// can change the instruction count the class reads. A record whose rewrites stay unrun keeps
/// the class the load settled, which is the one a load that skips them takes.
pub(crate) fn settle_deferred_rewrite(program: &crate::bytecode::ResidualProgram, id: u32, function: &mut BcFunction) {
    if !program.specialized {
        return;
    }
    let (fields, supers) = (&program.field_sites, &program.superinstructions);
    match rewrite_plan(program, id, Rewrite::Deferred, function.code.len()) {
        RewritePlan::Skip => {}
        RewritePlan::Replay(start) => rewrite::apply(
            function,
            program,
            &mut SiteTable::replay(fields, start.field_sites as usize),
            &mut SiteTable::replay(supers, start.superinstructions as usize),
            &mut CompileScratch::default(),
        ),
        RewritePlan::Append => rewrite::apply(
            function,
            program,
            &mut SiteTable::grow(fields),
            &mut SiteTable::grow(supers),
            &mut CompileScratch::default(),
        ),
    }
    // The rewrites leave their `Nop`s in place; this settles the record without lowering it.
    rewrite::remove_nops(function, &mut None, &mut CompileScratch::default());
    function.dispatch = Compiler::dispatch_class(&function.code, &function.wide);
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
        self.rewrite_ranges.settled.borrow().get(id as usize).copied().unwrap_or(Settled::UNSETTLED).get()
    }

    /// Notes where the sites of function `id`'s deferred rewrites start, or `None` if they stayed unrun.
    fn record_rewrite(&self, id: u32, start: Option<RewriteStart>) {
        let mut ranges = self.rewrite_ranges.settled.borrow_mut();
        let len = ranges.len().max(id as usize + 1);
        ranges.resize(len, Settled::UNSETTLED);
        ranges[id as usize] = start.map_or(Settled::UNRUN, Settled::placed);
    }
}
