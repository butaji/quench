//! The window activation as the owner of a variable environment (`VarOwner::Window`): what a
//! name lookup, a local or a capture step reads of the activation a lane runs without a VM
//! frame, through the same accessors a frame's owner offers (`var_owner`). The activation's
//! environment window (`WINDOW_ENVIRONMENT`, `WINDOW_LOCALS`) holds its environment and its
//! locals, its registers the operands, its record (`FrameRecord`) the dynamic bindings a direct
//! eval made, and the lane stack its `with` objects (`LaneStack::with_chain`).
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use super::with_scopes::WithChain;
use quench_runtime::vm::activation::FrameRecord;
use quench_runtime::vm::caller_view::CallerView;
use quench_runtime::vm::var_owner::VarOwner;
use quench_runtime::bytecode::Atom;

pub(crate) use quench_runtime::vm::engine_types::WindowScope;





#[quench_macros::ext(DispatchFastWindowOwnerVm)]
impl Vm {
    /// The owner of the variable environment of the activation that runs: the innermost frame,
    /// or, beside the lane's frameless activations (`lane_deferred`), the innermost activation's
    /// window, which a step of its function reads where it stands (a plain activation's, registers only,
    /// has no locals: `scoped_names::plain_scope`).
    pub(crate) fn running_owner(&mut self) -> VarOwner {
        if let Some(DeferredLaneCall { cx, call_pc, .. }) = self.lane_deferred {
            let cx = cx.cast::<LaneContext>();
            // SAFETY: the deferred call's lane runs, so its context lives, and the call's record
            // lies in the view of the innermost activation's function.
            let (registers, ip) = unsafe { ((*cx).registers, (*cx).base.add(call_pc)) };
            self.running_window = super::scoped_names::window_scope(cx, registers, ip, false).or_else(|| super::scoped_names::plain_scope(cx, registers));
            if self.running_window.is_some() {
                return VarOwner::Window;
            }
            self.materialize_deferred_lane();
        }
        VarOwner::Frame(self.frames.len() - 1)
    }
}
