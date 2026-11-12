//! What a lane run binds in the thread's lane frame: a VM's constants once, the rest per run.

use super::*;

impl JsStepDispatch {
    /// What a VM may change between native entries: the call targets its eval state admits.
    #[inline(always)]
    pub(super) fn bind_run_storage<H: Host>(&mut self, vm: &Vm<H>) {
        self.call_targets = if vm.direct_eval || vm.parameter_eval { NO_CALL_TARGETS.as_ptr() }
            else { vm.call_targets.as_ptr() } as usize;
        self.proper_tail_calls = u64::from(vm.host.proper_tail_calls());
    }

    /// The storage a lane run reads: the thread's lane frame binds a VM's constants when it
    /// first meets the VM, and each run binds only what the VM may have changed since.
    #[inline(always)]
    pub(super) fn bind_lane_storage<H: Host>(&mut self, vm: *mut Vm<H>) {
        if self.vm == vm.addr() {
            // SAFETY: the VM outlives the run that binds it.
            self.bind_run_storage(unsafe { &*vm });
        } else {
            self.vm = vm.addr();
            self.bind_native_storage(vm);
        }
    }

    /// A lane entry's refresh: the bases stand while this VM's record layout and atom words are
    /// as the last entry read them (a helper's refresh in between only made them fresher).
    #[inline(always)]
    pub(super) fn refresh_object_bases_at_entry<H: Host>(&mut self, vm: &Vm<H>) {
        let layout = vm.heap.native_layout_epoch();
        let atoms = vm.method_cache_atoms.words();
        if self.vm == (vm as *const Vm<H>).addr() && self.layout_seen == layout
            && self.method_cache_atoms == atoms.as_ptr() as usize && self.method_cache_atom_words == atoms.len() {
            return;
        }
        self.refresh_object_bases(vm);
        self.layout_seen = layout;
    }
}
