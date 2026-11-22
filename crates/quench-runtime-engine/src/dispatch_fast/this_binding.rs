//! How a lane call target binds `this`.

pub(crate) use quench_runtime::vm::engine_types::ThisBinding;

#[quench_macros::ext(ThisBindingOps1)]
impl ThisBinding {
    /// Whether the callee binds `this` to the call's receiver.
    #[inline(always)]
    pub(crate) fn binds_receiver(self) -> bool {
        matches!(self, ThisBinding::Sloppy | ThisBinding::Strict)
    }
}
