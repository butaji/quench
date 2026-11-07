//! A lane view's lane-body counters, kept under `QUENCH_TRACE=js-jit`.

/// A view's lane-body counters, which only a diagnostics run keeps
/// (`ProgramStore::js_jit_diagnostics_enabled`): its function, its compiles and the frames the
/// last inlined, and its runs by how they ended (`LaneExit`), in that order.
#[derive(Default)]
pub(in crate::vm) struct LaneBodyStats {
    function: std::cell::Cell<u32>,
    compiles: std::cell::Cell<u32>,
    frames: std::cell::Cell<u32>,
    /// Completed, left at a caller PC, inside an inlined callee, in a native call's callee,
    /// or irreversibly terminated.
    ends: [std::cell::Cell<u32>; 5],
    /// The PCs it left at, and inside inlined frames as `frame:pc`, with their counts.
    leaves: std::cell::RefCell<Vec<(String, u32)>>,
    /// Entries of the specialized body that ran in the lane instead (`lane_body_admits`).
    pub(in crate::vm) refused: std::cell::Cell<u32>,
}

impl LaneBodyStats {
    pub(in crate::vm) fn compiled(&self, function: u32, frames: usize) {
        self.function.set(function);
        self.compiles.set(self.compiles.get() + 1);
        self.frames.set(frames as u32);
    }

    /// `frames`, the body's inlined calls, name the function a leave inside one was in.
    pub(in crate::vm) fn ended(&self, exit: super::super::compiled::LaneExit, frames: &[super::super::compiled::InlinedCall], line: u64) {
        let index = match exit {
            super::super::compiled::LaneExit::Returned { .. } => 0,
            super::super::compiled::LaneExit::At(_) => 1,
            super::super::compiled::LaneExit::Inlined { .. } => 2,
            super::super::compiled::LaneExit::Callee => 3,
            super::super::compiled::LaneExit::Terminated => 4,
        };
        self.ends[index].set(self.ends[index].get() + 1);
        // Prefixed by the compile that left there, the first being `c1`.
        let compile = self.compiles.get();
        // Each with the function it left in and its PC there, which the report names the
        // instruction of: `#function@pc`.
        // A guard's leave names the lowering line that made it (`JsStepDispatch::exit_line`).
        let compile = if line == 0 { format!("c{compile}") } else { format!("c{compile} L{line}") };
        let at = match exit {
            super::super::compiled::LaneExit::At(pc) => format!("{compile} {pc}#{}@{pc}", self.function.get()),
            super::super::compiled::LaneExit::Inlined { frame, pc } => match frames.get(frame) {
                Some(call) => format!("{compile} {frame}:{pc}#{}@{pc}", call.target.function),
                None => format!("{compile} {frame}:{pc}"),
            },
            _ => return,
        };
        let mut leaves = self.leaves.borrow_mut();
        match leaves.iter_mut().find(|(leave, _)| *leave == at) {
            Some((_, count)) => *count += 1,
            None => leaves.push((at, 1)),
        }
    }

    /// The report line of a view that compiled a body, `QUENCH_LANE_BODY {...}`, with the
    /// state its body ended the run in.
    pub(in crate::vm) fn report(&self, program: usize, state: &str, op_at: &dyn Fn(u32, u32) -> Option<String>) -> Option<String> {
        (self.compiles.get() != 0).then(|| {
            let function = self.function.get();
            let [completed, at, inlined, callee, terminated] = self.ends.each_ref().map(std::cell::Cell::get);
            // A leave names the instruction it left at, in the function it left in. Places are
            // generated compile, line and PC numbers and opcode names, never guest text.
            let leaves = self.leaves.borrow().iter().map(|(at, count)| {
                let place = match at.split_once('#') {
                    Some((place, site)) => {
                        let op = site.split_once('@').and_then(|(function, pc)| op_at(function.parse().ok()?, pc.parse().ok()?));
                        format!("{place} f{site} {}", op.unwrap_or_default())
                    }
                    None => at.clone(),
                };
                format!("[\"{place}\",{count}]")
            }).collect::<Vec<_>>().join(",");
            format!(
                "QUENCH_LANE_BODY {{\"program\":{program},\"function\":{function},\"compiles\":{},\"frames\":{},\"state\":\"{state}\",\"refused\":{},\"completed\":{completed},\"left_at\":{at},\"left_inlined\":{inlined},\"left_in_callee\":{callee},\"terminated\":{terminated},\"leaves\":[{leaves}]}}",
                self.compiles.get(), self.frames.get(), self.refused.get(),
            )
        })
    }
}
