//! The running Rust-side native calls (`NativeActivation`): the one owner of their stack. Every
//! push, pop and reader goes through here, so the store can become the plain frame header in the
//! frame sequence (one-frame-one-call.md F5, TOD-1919) without touching its callers.
use super::activation::{NativeActivation, NativeCallBoundary};
use super::*;

impl Vm {
    /// Enters the native call of `callable`, which keeps `receiver` alive, above `frame_depth` frames.
    #[inline(always)]
    pub(super) fn push_host_frame(&mut self, callable: Value, receiver: Value, frame_depth: usize, boundary: NativeCallBoundary) {
        self.realm.promise.active_native.push(NativeActivation { callable, receiver, frame_depth, boundary });
    }

    /// Leaves the innermost native call: what it kept alive, if one was running.
    #[inline(always)]
    pub(super) fn pop_host_frame(&mut self) -> Option<Value> {
        self.realm.promise.active_native.pop().map(|activation| activation.receiver)
    }

    /// The count of native calls running.
    #[inline(always)]
    pub(super) fn host_frame_count(&self) -> usize {
        self.realm.promise.active_native.len()
    }

    /// Leaves every native call above the first `count`.
    pub(super) fn truncate_host_frames(&mut self, count: usize) {
        self.realm.promise.active_native.truncate(count);
    }

    /// What the running natives keep alive: each callable and receiver.
    pub(super) fn host_frame_roots(&self, roots: &mut Vec<Value>) {
        roots.extend(self.realm.promise.active_native.iter().flat_map(|activation| [activation.callable, activation.receiver]));
    }

    /// The innermost running native's callable.
    pub(super) fn active_native_callable(&self) -> Option<Value> {
        self.realm.promise.active_native.last().map(|activation| activation.callable)
    }

    /// Whether an opaque native runs directly above frame `index`, which hides its callers
    /// (`Vm::function_caller`).
    pub(super) fn host_frame_hides_caller(&self, index: usize) -> bool {
        self.realm.promise.active_native.iter().any(|activation| {
            activation.frame_depth == index && activation.boundary == NativeCallBoundary::Opaque
        })
    }

    /// Running natives, innermost of the first `natives`, that recorded `depth` frames beside
    /// activations now `added` frames above it: their caller is the innermost of those.
    pub(super) fn shift_beside_natives(&mut self, natives: usize, depth: usize, added: usize) {
        for native in self.realm.promise.active_native[..natives].iter_mut().rev() {
            if native.frame_depth != depth {
                break;
            }
            native.frame_depth += added;
        }
    }
}
