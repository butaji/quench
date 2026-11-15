//! The locals a lane body's rows read, by where its function runs: the VM frame's, or the environment window
//! of the lane activation (`lane_activate_environment`), whose locals follow its environment entries and whose
//! environment-owned ones live in the storage its slots word names.
use super::*;

/// What a body entered on a lane context reads of its function's locals.
#[derive(Clone, Copy)]
pub(in crate::vm) struct BodyLocals {
    /// The locals' address and count (`JsStepDispatch::native_locals`).
    pub(in crate::vm) locals: *mut Value,
    pub(in crate::vm) count: usize,
    /// The word of the storage the environment-owned ones live in (`JsStepDispatch::environment_storage`).
    pub(in crate::vm) environment: u64,
}

/// The storage the environment window at register `registers` of `r` keeps its environment-owned locals in.
#[inline(always)]
pub(super) fn window_storage(r: Registers, registers: u16) -> *mut crate::heap::EnvironmentSlots {
    // SAFETY: a window's slots word holds its storage's address, held in a `Value`'s bits (`lane_activate_environment`).
    unsafe { std::mem::transmute::<Value, u64>(r.get(registers + WINDOW_SLOTS)) as *mut crate::heap::EnvironmentSlots }
}

/// The locals the body entered on lane context `cx` reads: the window's where its function's activation runs one
/// (an activation of a function with a window layout whose context holds the environment's parent, as every window
/// handler decides it), else the VM frame's, with storage that holds no slot (`heap::absent_environment_storage`),
/// which every environment row misses at. None of either where there is no context (a test's entry).
///
/// # Safety
/// `cx` is null or a live `LaneContext<H>`.
pub(in crate::vm) unsafe fn body_locals<H: Host>(cx: *mut ()) -> BodyLocals {
    let cx = cx.cast::<LaneContext<H>>();
    // SAFETY: the caller's contract.
    if let Some(window) = unsafe { activation_window(cx) } {
        return window;
    }
    // SAFETY: as above; `frame_locals` takes the context as it is.
    let (locals, count) = unsafe { frame_locals::<H>(cx.cast()) };
    BodyLocals { locals, count, environment: crate::heap::absent_environment_storage() }
}

/// `body_locals` of an environment activation.
///
/// # Safety
/// As `body_locals`.
unsafe fn activation_window<H: Host>(cx: *mut LaneContext<H>) -> Option<BodyLocals> {
    let cx = std::ptr::NonNull::new(cx)?.as_ptr();
    // SAFETY: the caller's contract; the program's views outlive the lane run.
    unsafe {
        if vm(cx).lane_stack.activations.is_empty() || (*cx).registers.env().is_deleted() {
            return None;
        }
        let function = lane_running_function(cx);
        let layout = (&*(*cx).views).get(function as usize).and_then(super::super::program_store::derived_lane_view)?.environment_window?;
        let registers = (*(*cx).code).function(function as usize).registers;
        let entries = (*cx).registers.0.add(usize::from(registers));
        Some(BodyLocals {
            locals: entries.add(usize::from(WINDOW_LOCALS)),
            count: usize::from(layout.locals),
            environment: std::mem::transmute::<Value, u64>(*entries.add(usize::from(WINDOW_SLOTS))),
        })
    }
}
