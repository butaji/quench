//! The callee an AOT image's call site calls by its entry (`aot_image::bound::BoundCalls::bind`): the function
//! the closed world proves the site calls (`AotFacts::call_target`) fixes which entry cell it is; the closure
//! of that body, which its program's closure cell holds once the body's one creation has run
//! (`ProgramStore::closure_cell`), fixes its identity, captures and descriptor, as a JIT caller's direct
//! callee holds them (`direct_callee_in_lane`).
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

#[quench_macros::ext(DispatchFastProvenCalleesVm)]
impl Vm {
    /// The direct callee of the call at `pc` of `function` that the closed world proves calls body `callee`: the
    /// closure of that body the closure cell holds, where the lane would activate it in place. It pins no body: a
    /// call runs what the callee's entry cell holds then, the helper where that is the stub. The callee's view
    /// learns of its caller and warms toward a body (`bind_callees`). `None` until the body's closure is created.
    pub(crate) fn proven_callee(&mut self, program: &quench_runtime::bytecode::ResidualProgram, (function, pc): (u32, u32), callee: u32) -> Option<super::compiled::DirectCallee> {
        let argc = crate::compiled::direct_callee::direct_argc(program, program.function_get(function as usize)?.instruction_at(pc as usize))?; // a method call's count is its site's (TOD-2350)
        let closure = self.programs.closure_cell(self.active_program, callee)?;
        let target = self.resolve_call_target_in_lane(program, closure)?;
        let views = self.programs.views_in_lane(self.active_program)?;
        let view = views.get(function as usize).and_then(super::program_store::derived_lane_view)?;
        let mut direct = direct_callee_in_lane(self, view, function, pc as usize, argc, target)?;
        (direct.method, direct.body) = (None, None);
        super::compiled::lane_facts::bind_callees(std::slice::from_ref(&direct), view);
        Some(direct)
    }

    /// The creation edge of body `body`'s one closure in `program` (`layer_hooks::closure_created`, once the closure cell is
    /// written): binds the active image's proven calls of `body` as `ahead_callees` binds them at install, where the site's
    /// row admits the closure. A site whose closure is not created stays unbound.
    pub(crate) fn bind_closure_records(&mut self, program: quench_runtime::vm::ProgramId, body: u32) {
        if program != self.active_program { return; }
        let Some(code) = self.programs.get(program) else { return };
        let Some(images) = self.programs.ahead_images(program, &code) else { return };
        let Some(live) = images.linked.as_ref() else { return };
        for (key, site) in images.image.calls.iter().enumerate().filter(|(_, site)| site.callee == body) {
            let Some(direct) = self.proven_callee(&code, (site.function, site.pc), body) else { continue };
            if crate::compiled::ahead_images::admits_proven(&code, body, &direct) { live.calls.bind(key, direct, self); }
        }
    }
}
