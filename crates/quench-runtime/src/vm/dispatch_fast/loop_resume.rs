//! A lane body that leaves inside a loop whose BaseJIT region is published resumes in that
//! region's code at the leave's PC (`CompiledOsrRegion::can_run_from`) rather than in the
//! interpreter: the leave's frame slots are canonical, so the region's resume stub loads the
//! pinned registers live there, and its guards decline to the interpreter at the same PC.
use super::*;

/// The innermost loop site of `own` holding `pc` whose published code can re-enter at it, with
/// its region.
fn resumable_site<'v>(own: &'v LaneView, pc: u32) -> Option<(&'v LoopSite, &'v super::super::compiled::LoopRegion)> {
    let mut held: Vec<&LoopSite> = own.loops.iter().filter(|site| (site.header..=site.backedge).contains(&pc)).collect();
    held.sort_by_key(|site| site.backedge - site.header);
    held.into_iter().find_map(|site| {
        let region = site.active_region().filter(|region| region.inlining.is_none())?;
        site.tier.borrow().resumable(region, pc).then_some((site, region))
    })
}

/// Runs the loop's published code from `pc` on the activation's registers `r`, and where the
/// lane continues after it; `None` where no code re-enters there or its entry record cannot be
/// built, and the caller resumes the interpreter at `pc`.
pub(super) fn lane_loop_resume<H: Host>(cx: *mut LaneContext<H>, r: Registers, own: &LaneView, pc: usize) -> Option<LaneBodyLeft> {
    let (site, region) = resumable_site(own, pc as u32)?;
    let mut entry = super::super::compiled::LoopEntry::new();
    let views = lane_loop_entry(cx, r, region, &mut entry)?;
    let continuation = lane_loop_run(cx, r, site, region, &mut entry, views, pc as u32);
    count_continuation(&continuation);
    site.declines.set(if matches!(continuation, LoopContinuation::Jump) { site.declines.get() + 1 } else { 0 });
    // SAFETY: the running activation's records start at the context's base, and the region's PCs are the function's.
    let at = |pc: u32| unsafe { (*cx).base.add(pc as usize) };
    let script = |next| LaneBodyLeft::Resume(r, next, Carried::script(cx));
    Some(match continuation {
        LoopContinuation::Jump | LoopContinuation::Compiling => script(at(pc as u32)),
        LoopContinuation::Resume(next) => script(at(next)),
        LoopContinuation::Exit(next) => {
            count_arithmetic_leave(cx, site, next, region.header..=region.backedge);
            script(at(next))
        }
        // The backward jump's own record polls the interrupt.
        LoopContinuation::Safepoint => script(at(region.backedge)),
        LoopContinuation::Completed(next, value) => LaneBodyLeft::Completed(at(next), value),
        LoopContinuation::Callee => {
            let mut carried = Carried::script(cx);
            // SAFETY: `lane_body_call` set both before the region left.
            let (registers, resume) = unsafe { ((*cx).registers, (*cx).callee_resume) };
            carried.acc = Slot::of(unsafe { (*cx).callee_acc });
            LaneBodyLeft::Resume(registers, resume, carried)
        }
        LoopContinuation::Inlined { .. } => unreachable!("a region without inlining makes no inlined leave"),
    })
}
