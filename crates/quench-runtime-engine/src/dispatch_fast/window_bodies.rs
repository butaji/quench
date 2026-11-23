//! The locals a lane body's rows read, by where its function runs: the VM frame's, or the environment window
//! of the lane activation (`activate_environment_in_lane`), whose locals follow its environment entries and whose
//! environment-owned ones live in the storage its slots word names.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

/// What a body entered on a lane context reads of its function's locals.
#[derive(Clone, Copy)]
pub(crate) struct BodyLocals {
    /// The locals' address and count (`JsStepDispatch::native_locals`).
    pub(crate) locals: *mut Value,
    pub(crate) count: usize,
    /// The word of the storage the environment-owned ones live in (`JsStepDispatch::environment_storage`).
    pub(crate) environment: u64,
}

/// Which caller's rows `body_locals` resolves the locals for. The two callers answer alike for an activation with
/// a window and for a frame whose storage covers its function; they differ in the shapes named on each variant. The
/// difference is kept here, as each caller's rows were compiled against it, rather than merged unchecked.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocalsOf {
    /// A lane body's rows (`Vm::bind_body_entry`). The top activation's window where its function has a window layout
    /// and the context's environment is made (`activation_window`); else the VM frame's storage, counted by its length,
    /// with no environment storage. A pending run (`CALLEE_ACTIVATION_PENDING`, whose activation is not pushed yet) is
    /// resolved the same way, from the activations pushed so far, as it always was. The OSR answer for such a run is
    /// nothing, which differs; no OSR caller binds a pending run, so the two never meet.
    Body,
    /// An OSR loop region's rows (`osr_lane::bind_frame`). The top activation's window where it is an environment
    /// activation (`script.environment`), made or not; nothing where the top activation is not one. Without an
    /// activation, the VM frame's own storage where it covers the function, else, where the function addresses a
    /// local, the window past the register file the run binds, and nothing where it addresses none. A loop region
    /// has no environment rows, so it takes no environment storage.
    Osr,
}

/// The locals the body entered on lane context `cx` reads, as `caller` names: the window where its function's
/// activation runs one, else the VM frame's, with storage that holds no slot (`heap::absent_environment_storage`),
/// which every environment row misses at. None of either where there is no context (a test's entry).
///
/// `registers` is the register file of the run the caller binds. `LocalsOf::Osr` finds its window past it; `LocalsOf::Body`
/// finds its window past the context's own registers, as it always has.
///
/// # Safety
/// `cx` is null or a live `LaneContext`; `registers` is the live register file of the run.
pub(crate) unsafe fn body_locals(cx: *mut (), registers: *mut Value, caller: LocalsOf) -> BodyLocals {
    let cx = cx.cast::<LaneContext>();
    match caller {
        LocalsOf::Body => {
            // SAFETY: the caller's contract.
            if let Some(window) = unsafe { activation_window(cx) } {
                return window;
            }
            // SAFETY: as above; `frame_locals` takes the context as it is.
            let (locals, count) = unsafe { frame_locals(cx.cast()) };
            BodyLocals { locals, count, environment: quench_runtime::heap::absent_environment_storage() }
        }
        // SAFETY: the caller's contract.
        LocalsOf::Osr => unsafe { osr_locals(cx, registers) },
    }
}

/// `body_locals` of an OSR loop region (`LocalsOf::Osr`).
///
/// # Safety
/// As `body_locals`.
unsafe fn osr_locals(cx: *mut LaneContext, registers: *mut Value) -> BodyLocals {
    let absent = quench_runtime::heap::absent_environment_storage();
    let none = BodyLocals { locals: std::ptr::null_mut(), count: 0, environment: absent };
    if cx.is_null() {
        return none;
    }
    // SAFETY: the caller's contract; the context's program outlives the lane run.
    let (vm, code) = unsafe { (vm(cx), &*(*cx).code) };
    // The window past the register file of `function`, which holds its locals only where it has any.
    let window = |function: u32| code.function_get(function as usize)
        .filter(|body| body.locals != 0)
        // SAFETY: the activation's window follows its register file.
        .map_or(none, |body| BodyLocals {
            locals: unsafe { registers.add(usize::from(body.registers) + usize::from(WINDOW_LOCALS)) },
            count: usize::from(body.locals),
            environment: absent,
        });
    if let Some(activation) = vm.lane_stack.top_frame() {
        return match activation.script() {
            Some(script) if script.environment => window(activation.function()),
            _ => none,
        };
    }
    // SAFETY: its frame is the running one.
    let Some(frame) = vm.frames.get_mut(unsafe { (*cx).frame() }) else { return none };
    let function = frame.function;
    match code.function_get(function as usize) {
        Some(body) if frame.locals.len() >= usize::from(body.locals) => BodyLocals {
            locals: frame.locals.as_mut_ptr(),
            count: frame.locals.len(),
            environment: absent,
        },
        // No record of the function addresses a local (`context_locals_in_window_in_lane`).
        Some(body) if !body.call_plan(code).addresses_locals => none,
        _ => window(function),
    }
}

/// The locals of an environment activation (`activation_window`), as a lane body reads them.
///
/// # Safety
/// As `body_locals`.
unsafe fn activation_window(cx: *mut LaneContext) -> Option<BodyLocals> {
    let cx = std::ptr::NonNull::new(cx)?.as_ptr();
    // SAFETY: the caller's contract; the program's views outlive the lane run.
    unsafe {
        if !vm(cx).lane_stack.has_frames() || (*cx).registers.env().is_deleted() {
            return None;
        }
        let function = running_function_in_lane(cx);
        let layout = (&*(*cx).views).get(function as usize).and_then(quench_runtime::vm::program_store::derived_lane_view)?.environment_window?;
        let registers = (*(*cx).code).function(function as usize).registers;
        let entries = (*cx).registers.0.add(usize::from(registers));
        Some(BodyLocals {
            locals: entries.add(usize::from(WINDOW_LOCALS)),
            count: usize::from(layout.locals),
            environment: std::mem::transmute::<Value, u64>(*entries.add(usize::from(WINDOW_SLOTS))),
        })
    }
}
