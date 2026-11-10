//! A lane view's lane-body counters, kept under `QUENCH_TRACE=js-jit`.

/// A view's lane-body counters, which only a diagnostics run keeps
/// (`ProgramStore::js_jit_diagnostics_enabled`): its function, its compiles and the frames the
/// last inlined, and its runs by how they ended (`LaneExit`), in that order.
#[derive(Default)]
pub(in crate::vm) struct LaneBodyStats {
    function: std::cell::Cell<u32>,
    compiles: std::cell::Cell<u32>,
    frames: std::cell::Cell<u32>,
    /// The last compile's inlined frames as `index:function<-parent@call-pc`, `+` marking an alternative.
    shape: std::cell::RefCell<String>,
    /// Wall-clock milliseconds since the epoch of each compile's publication.
    compiled_at: std::cell::RefCell<Vec<u64>>,
    /// Completed, left at a caller PC, inside an inlined callee, in a native call's callee,
    /// or irreversibly terminated.
    ends: [std::cell::Cell<u32>; End::COUNT],
    /// The PCs it left at, and inside inlined frames as `frame:pc`, with their counts.
    leaves: std::cell::RefCell<Vec<(String, u32)>>,
    /// Entries of the specialized body that ran in the lane instead (`lane_body_admits`).
    pub(in crate::vm) refused: std::cell::Cell<u32>,
}

impl LaneBodyStats {
    pub(in crate::vm) fn compiled(&self, function: u32, frames: &[super::super::compiled::InlinedCall]) {
        self.function.set(function);
        self.compiles.set(self.compiles.get() + 1);
        self.frames.set(frames.len() as u32);
        self.compiled_at.borrow_mut().push(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_millis() as u64));
        *self.shape.borrow_mut() = frames.iter().enumerate().map(|(index, frame)| {
            let parent = frame.parent.map_or(String::from("root"), |parent| parent.to_string());
            format!("{index}:f{}<-{parent}@{}{}", frame.target.function, frame.call, if frame.alternative.is_some() { "+" } else { "" })
        }).collect::<Vec<_>>().join(" ");
    }

    /// `frames`, the body's inlined calls, name the function a leave inside one was in.
    pub(in crate::vm) fn ended(&self, exit: super::super::compiled::LaneExit, frames: &[super::super::compiled::InlinedCall], line: u64) {
        let index = End::of(exit) as usize;
        self.ends[index].set(self.ends[index].get() + 1);
        // Prefixed by the compile that left there, the first being `c1`.
        let compile = self.compiles.get();
        // Each with the function it left in and its PC there, which the report names the
        // instruction of: `#function@pc`.
        // A guard's leave names the lowering line that made it (`JsStepDispatch::exit_line`).
        let compile = if line == 0 { format!("c{compile}") } else { format!("c{compile} L{line}") };
        let at = match exit {
            super::super::compiled::LaneExit::At(pc) => format!("{compile} {pc}#{}@{pc}", self.function.get()),
            super::super::compiled::LaneExit::Entry => format!("{compile} entry#{}", self.function.get()),
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
                "QUENCH_LANE_BODY {{\"program\":{program},\"function\":{function},\"compiles\":{},\"frames\":{},\"shape\":\"{}\",\"compiled_at\":{:?},\"state\":\"{state}\",\"refused\":{},\"completed\":{completed},\"left_at\":{at},\"left_inlined\":{inlined},\"left_in_callee\":{callee},\"terminated\":{terminated},\"leaves\":[{leaves}]}}",
                self.compiles.get(), self.frames.get(), self.shape.borrow(), self.compiled_at.borrow(), self.refused.get(),
            )
        })
    }
}

/// How a lane body's run ended, each with its `LaneBodyStats::ends` counter: completed, left
/// at its own PC (its entry included), inside an inlined call, into a callee, or terminated.
#[derive(Clone, Copy)]
enum End { Completed, LeftAt, LeftInlined, LeftInCallee, Terminated }

impl End {
    const COUNT: usize = Self::Terminated as usize + 1;

    fn of(exit: super::super::compiled::LaneExit) -> Self {
        use super::super::compiled::LaneExit;
        match exit {
            LaneExit::Returned { .. } => Self::Completed,
            LaneExit::At(_) | LaneExit::Entry => Self::LeftAt,
            LaneExit::Inlined { .. } => Self::LeftInlined,
            LaneExit::Callee => Self::LeftInCallee,
            LaneExit::Terminated => Self::Terminated,
        }
    }
}

/// Why a lane body's call or construction helper answered `LANE_CALL_MISSED`, which leaves the
/// body at the call: one variant per refusal site in `lane_body_call`, `lane_body_construct` and
/// `lane_body_native_call`, named `site:reason` in the diagnostics report (`miss_report`).
macro_rules! miss_reasons {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        #[derive(Clone, Copy)]
        pub(in crate::vm) enum Miss { $($variant),+ }
        const MISS_NAMES: [&str; [$($name),+].len()] = [$($name),+];
    };
}
miss_reasons! {
    CallShadowActive => "call:shadow-active",
    CallNoScriptCalls => "call:no-script-calls",
    CallConstructing => "call:constructing",
    CallWithScope => "call:with-scope",
    CallReturningNotAdmitted => "call:returning-not-admitted",
    CallReceiver => "call:receiver",
    CallEntryNotActivation => "call:entry-not-activation",
    CallNoActivationRoom => "call:no-activation-room",
    CallReadAnswer => "call:read-answer",
    ConstructShadowActive => "construct:shadow-active",
    ConstructNoScriptConstructs => "construct:no-script-constructs",
    ConstructConstructTarget => "construct:construct-target",
    ConstructWithScope => "construct:with-scope",
    ConstructCollectionDue => "construct:collection-due",
    ConstructReturning => "construct:returning",
    ConstructNoTarget => "construct:no-target",
    ConstructNotConstructor => "construct:not-constructor",
    ConstructEntryNotActivation => "construct:entry-not-activation",
    ConstructNoActivationRoom => "construct:no-activation-room",
    ConstructNoReceiver => "construct:no-receiver",
    HelperConstructContext => "helper-construct:context",
    HelperConstructBusy => "helper-construct:target-or-collection",
    HelperConstructNoPlan => "helper-construct:no-plan",
    PlanNoUnit => "plan:no-unit",
    PlanInvalidBindings => "plan:invalid-bindings",
    PlanFrameNotAdmitted => "plan:frame-not-admitted",
    PlanNoEntry => "plan:no-entry-instruction",
    PlanLeavesAtEntry => "plan:leaves-at-entry",
    PlanNotLowered => "plan:not-lowered",
    PlanNothingNative => "plan:nothing-native",
    NativeReturning => "native:returning",
    NativeNotNative => "native:not-native",
    NativeInPlaceDeclined => "native:in-place-declined",
}
static MISSES: [std::sync::atomic::AtomicU64; MISS_NAMES.len()] = [const { std::sync::atomic::AtomicU64::new(0) }; MISS_NAMES.len()];

/// Counts a miss for `reason` and answers the helper's miss.
#[cold]
pub(in crate::vm) fn miss(reason: Miss) -> u64 {
    MISSES[reason as usize].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    super::super::compiled::LANE_CALL_MISSED
}

/// The report line of the misses, `QUENCH_LANE_MISSES {...}`, most frequent first.
pub(in crate::vm) fn miss_report() -> String {
    let mut counts: Vec<_> = MISS_NAMES.iter().zip(&MISSES)
        .map(|(name, count)| (*name, count.load(std::sync::atomic::Ordering::Relaxed))).filter(|(_, count)| *count != 0).collect();
    counts.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    let fields = counts.iter().map(|(name, count)| format!("\"{name}\":{count}")).collect::<Vec<_>>().join(",");
    format!("QUENCH_LANE_MISSES {{{fields}}}")
}

/// Instructions a body compile left to the lane (`QUENCH_LANE_UNLOWERED`), by opcode.
static UNLOWERED: std::sync::Mutex<Vec<(String, u64)>> = std::sync::Mutex::new(Vec::new());

/// Counts one instruction of `op` that a lane body compile hands back to the lane.
pub(in crate::vm) fn unlowered(op: impl std::fmt::Debug) {
    let name = format!("{op:?}");
    let mut counts = UNLOWERED.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    match counts.iter_mut().find(|(known, _)| *known == name) {
        Some((_, count)) => *count += 1,
        None => counts.push((name, 1)),
    }
}

pub(in crate::vm) fn unlowered_report() -> String {
    let mut counts = UNLOWERED.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
    counts.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    let fields = counts.iter().map(|(name, count)| format!("\"{name}\":{count}")).collect::<Vec<_>>().join(",");
    format!("QUENCH_LANE_UNLOWERED {{{fields}}}")
}
