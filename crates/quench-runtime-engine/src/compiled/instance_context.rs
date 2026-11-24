//! The persistent context of a Wasm instance: one heap-stable record per instance, owned by the
//! instance's environment, that holds what native code reads of the instance in place: its
//! global area, its tables' published words, the code table of the artifact it runs, its linear
//! memory and the diagnostics of that artifact. A call context takes the record's words
//! (`SWITCHED`), so entering an instance, from the VM or from another instance's code, is the
//! same copy; a mirror row names its function's instance by the record's address, which lives as
//! long as the environment the function keeps alive.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::{
    CallContext, CompiledProgram, DiagnosticReferenceCallout, MemoryArguments,
    ProgramId, Vm, diagnostic_reference_callout, displacement_base,
};
use quench_runtime::Value;
use quench_runtime::heap::{Cell, TableWords};
use cranelift_codegen::ir::{self, types};
use std::rc::Rc;

pub(crate) use quench_runtime::vm::engine_types::ABI_NOT_DIRECT;

pub use quench_runtime::vm::engine_types::InstanceContext;

/// The memory an artifact is entered with in an instance: its binding, base, current length and
/// guard reservation.
pub(crate) struct MemoryBinding {
    pub(crate) binding: Value,
    pub(crate) base: u64,
    pub(crate) length: u64,
    pub(crate) guard: Option<(*mut u8, usize)>,
}

/// Declares the fields a call context and a record share by name and type: `SWITCHED`, their
/// offsets in the two structs with the machine type of each, which generated code copies to
/// enter another instance, and `CallContext::enter`, which the VM copies them with.
macro_rules! switched {
    (head: { $($field:ident: $ty:expr),+ $(,)? }, tail: { $($tail:ident: $tail_ty:expr),+ $(,)? } $(,)?) => {
        /// The offset in a call context, the offset in a record and the machine type of each
        /// field that belongs to an instance.
        pub(crate) const SWITCHED: &[(i32, i32, ir::Type)] = &[
            $((
                std::mem::offset_of!(CallContext, head.$field) as i32,
                std::mem::offset_of!(InstanceContext, $field) as i32,
                $ty,
            ),)+
            $((
                std::mem::offset_of!(CallContext, $tail) as i32,
                std::mem::offset_of!(InstanceContext, $tail) as i32,
                $tail_ty,
            ),)+
        ];

        #[quench_macros::ext(CallContextOps2)]
        impl CallContext {
            /// Enters the instance of `record`: the context takes its words.
            pub(crate) fn enter(&mut self, record: &InstanceContext) {
                $(self.$field = record.$field;)+
                $(self.$tail = record.$tail;)+
            }
        }
    };
}

switched! {
    head: {
    env: types::I64,
    globals: types::I64,
    table_words: types::I64,
    table: types::I64,
    compiled: types::I64,
    program: types::I32,
    memory: types::I64,
    memory_length: types::I64,
    guard_start: types::I64,
    guard_end: types::I64,
    call_reference_diagnostic: types::I64,
    reference_call_sites: types::I64,
    reference_call_site_count: types::I64,
    import_callout_counts: types::I64,
    import_callout_count: types::I64,
    },
    tail: { instance: types::I64 },
}

#[quench_macros::ext(InstanceContextOps1)]
impl InstanceContext {
    pub(crate) const MEMORY_BASE: i32 = std::mem::offset_of!(Self, memory_base) as i32;
    pub(crate) const ABI: i32 = std::mem::offset_of!(Self, abi) as i32;
    pub(crate) const TABLE: i32 = std::mem::offset_of!(Self, table) as i32;
    pub(crate) const ENV: i32 = std::mem::offset_of!(Self, env) as i32;

    /// The address a mirror row names the instance by.
    pub(crate) fn address(&self) -> usize {
        self.instance as usize
    }

    /// Fills the areas from the bindings of the environment, by slot.
    pub(crate) fn bind(
        &mut self,
        globals: impl IntoIterator<Item = (usize, *mut u64)>,
        tables: impl IntoIterator<Item = (usize, *const TableWords)>,
    ) {
        for (slot, address) in globals {
            self.global_area[slot] = address;
        }
        for (slot, words) in tables {
            self.table_area[slot] = words;
        }
    }

    /// Publishes `compiled` as the code the instance runs natively, and the memory it runs on:
    /// the artifact's words are written when it differs from the last, the memory's each time,
    /// as it may have grown.
    /// Returns whether the artifact changed, so the rows that name this instance take its bodies.
    pub(crate) fn publish(&mut self, compiled: &Rc<CompiledProgram>, memory: &MemoryBinding) -> bool {
        let changed = !std::ptr::eq(self.compiled, Rc::as_ptr(compiled));
        if changed {
            self.compiled = Rc::as_ptr(compiled);
            self.table = compiled.table.as_ptr();
            self.abi = compiled.abi();
            let stats = compiled.diagnostics.as_ref();
            self.call_reference_diagnostic = stats.map_or(0, |_| {
                diagnostic_reference_callout as DiagnosticReferenceCallout as usize
            });
            self.reference_call_sites = stats.map_or(std::ptr::null(), |stats| stats.reference_call_sites.as_ptr());
            self.reference_call_site_count = stats.map_or(0, |stats| stats.reference_call_sites.len());
            self.import_callout_counts = stats.map_or(std::ptr::null(), |stats| stats.import_callout_counts.as_ptr());
            self.import_callout_count = stats.map_or(0, |stats| stats.import_callout_counts.len());
            if !self.retained.iter().any(|kept| Rc::ptr_eq(kept, compiled)) {
                self.retained.push(Rc::clone(compiled));
            }
        }
        let (start, end) = memory.guard.map_or((0, 0), |(start, size)| (start as usize, start as usize + size));
        self.memory = memory.binding;
        self.memory_base = memory.base;
        self.memory_length = memory.length;
        (self.guard_start, self.guard_end) = (start, end);
        changed
    }
}

pub use quench_runtime::vm::engine_types::InstanceRecord;

#[quench_macros::ext(InstanceRecordOps1)]
impl InstanceRecord {
    pub(crate) fn new(env: Value, program: ProgramId, slots: usize) -> Self {
        Self(InstanceContext::new(env, program, slots))
    }
}









impl MemoryArguments {
    const BASE: u64 = 1;
    const LENGTH: u64 = 2;

    /// The arguments as the word a record publishes and a call site compares.
    pub(crate) fn word(self) -> u64 {
        (if self.base { Self::BASE } else { 0 }) | (if self.length { Self::LENGTH } else { 0 })
    }
}

#[quench_macros::ext(CompiledProgramOps3)]
impl CompiledProgram {
    /// What a caller outside this instance must match to call its bodies natively: the memory
    /// arguments, where the memory cannot change length unseen, else `ABI_NOT_DIRECT`.
    fn abi(&self) -> u64 {
        if self.memory_slot.is_some() && !self.guarded {
            return ABI_NOT_DIRECT;
        }
        MemoryArguments::of(self.fixed_base, self.guarded).word()
    }
}

#[quench_macros::ext(CompiledInstanceContextVm)]
impl Vm {
    /// The environment slot of `env`'s instance record: the last before the imported functions,
    /// which end the layout (`wasm_function_reference` derives their base from the end).
    pub(crate) fn wasm_context_slot(&self, env: Value, program: ProgramId) -> Option<usize> {
        let Some(quench_runtime::heap::EnvironmentRecord { slots, .. }) = self.heap_reads().environment(env) else {
            return None;
        };
        let imports = self.programs.wasm_signatures(program)?.imports().len();
        slots.len().checked_sub(imports + 1)
    }

    /// The record of the instance `env` of `program`.
    pub(crate) fn wasm_instance_record(&self, env: Value, program: ProgramId) -> Option<&InstanceContext> {
        let slot = self.wasm_context_slot(env, program)?;
        match self.heap_reads().get(self.heap_reads().environment_slot(env, slot)?)? {
            Cell::WasmInstance(record) => Some(&**record),
            _ => None,
        }
    }

    /// The address a mirror row names `env`'s instance by; none where it has no record.
    pub(crate) fn wasm_instance_address(&self, env: Value, program: ProgramId) -> Option<usize> {
        self.wasm_instance_record(env, program).map(InstanceContext::address)
    }

    /// Allocates the record of the instance `env` into its slot, before anything creates a
    /// function of the instance.
    pub(crate) fn install_wasm_instance(&mut self, env: Value, program: ProgramId) -> Option<()> {
        let slot = self.wasm_context_slot(env, program)?;
        let Some(quench_runtime::heap::EnvironmentRecord { slots, .. }) = self.heap_reads().environment(env) else {
            return None;
        };
        let record = InstanceRecord::new(env, program, slots.len());
        let record = self.heap.alloc(Cell::WasmInstance(record));
        *self.heap.environment_slot_mut(env, slot)? = record;
        Some(())
    }

    /// Binds the globals and tables the environment holds into the instance's areas: once they
    /// are all installed, which an initializer's evaluation precedes.
    pub(crate) fn bind_wasm_instance(&mut self, env: Value, program: ProgramId) -> Option<()> {
        let count = self.wasm_instance_record(env, program)?.global_area.len();
        let (mut globals, mut tables) = (Vec::new(), Vec::new());
        for slot in 0..count {
            let Some(binding) = self.heap_reads().environment_slot(env, slot) else {
                continue;
            };
            match self.heap.get_mut(binding) {
                Some(Cell::WasmGlobal { bits, .. }) => globals.push((slot, bits as *mut u64)),
                Some(Cell::WasmTable { elements, .. }) => {
                    tables.push((slot, elements.words() as *const TableWords));
                }
                _ => {}
            }
        }
        let context = self.wasm_context_slot(env, program)?;
        let record = self.heap_reads().environment_slot(env, context)?;
        let Some(Cell::WasmInstance(record)) = self.heap.get_mut(record) else {
            return None;
        };
        record.bind(globals, tables);
        Some(())
    }

    /// The memory `compiled` is entered with in the instance `env`: its binding, base, length
    /// and guard reservation, none where the memory cannot be entered with this artifact (its
    /// code relies on a guard or a base the memory does not have).
    pub(crate) fn memory_binding(&self, compiled: &CompiledProgram, env: Value) -> Option<MemoryBinding> {
        let Some(slot) = compiled.memory_slot else {
            return Some(MemoryBinding { binding: Value::UNDEFINED, base: 0, length: 0, guard: None });
        };
        let binding = self.heap_reads().environment_slot(env, slot as usize)?;
        let ((base, length), guard) = super::dispatch_fast::direct_memory_and_guard(self, binding)?;
        // Code compiled without bounds checks needs this memory's guard, and code compiled for a
        // base needs this memory's.
        if compiled.guarded && guard.is_none()
            || compiled.fixed_base.is_some_and(|fixed| {
                guard.map(|(base, _)| displacement_base(base)) != Some(Some(fixed))
            })
        {
            return None;
        }
        Some(MemoryBinding { binding, base: base as u64, length: length as u64, guard })
    }

    /// Publishes `compiled` in the record of the instance `env` of `program` and refreshes its
    /// memory: the record the call context enters, and the memory's base and length.
    pub(crate) fn publish_wasm_instance(
        &mut self,
        compiled: &Rc<CompiledProgram>,
        env: Value,
        program: ProgramId,
    ) -> Option<(*const InstanceContext, (u64, u64))> {
        let memory = self.memory_binding(compiled, env)?;
        let context = self.wasm_context_slot(env, program)?;
        let record = self.heap_reads().environment_slot(env, context)?;
        let Some(Cell::WasmInstance(record)) = self.heap.get_mut(record) else {
            return None;
        };
        let address = &**record as *const InstanceContext as usize;
        let changed = record.publish(compiled, &memory);
        if changed {
            self.refresh_wasm_bodies(address, compiled);
        }
        Some((address as *const InstanceContext, (memory.base, memory.length)))
    }

    /// Rewrites the body of each row, in every Wasm table of the heap, that names the instance at
    /// `address`: the bodies `compiled` publishes for it. Called when a publication changes the
    /// artifact, so no row keeps a body the record no longer publishes.
    fn refresh_wasm_bodies(&mut self, address: usize, compiled: &CompiledProgram) {
        self.heap.for_each_wasm_table_mut(|elements| {
            elements.refresh_bodies(|row| {
                (row.instance == address as u64)
                    .then(|| compiled.table.get(row.entry as usize).copied().unwrap_or(0))
            });
        });
    }
}

pub use quench_runtime::vm::engine_types::NativeRoots;

// SAFETY: the record owns its buffer outright and no other holder reads or writes it, so moving the
// record to another thread moves the buffer with it.


#[quench_macros::ext(NativeRootsOps1)]
impl NativeRoots {
    pub(crate) const DEPTH: i32 = std::mem::offset_of!(Self, depth) as i32;
    pub(crate) const CAPACITY: i32 = std::mem::offset_of!(Self, capacity) as i32;
    /// The base address of the buffer: root `k` is `size_of::<Value>()` bytes past it, `k` times.
    pub(crate) const SLOTS: i32 = std::mem::offset_of!(Self, slots) as i32;

    /// The depth an entry restores when it ends, whatever left native frames unreturned.
    pub(crate) fn depth(&self) -> u64 {
        self.depth
    }

    pub(crate) fn set_depth(&mut self, depth: u64) {
        self.depth = depth;
    }
}



#[cfg(test)]
mod tests {
    use super::*;

    const WORDS: [usize; 4] = [0x1000, 0x2000, 0x3000, 0x4000];

    /// A record whose words all differ from a fresh call context's.
    fn record() -> InstanceRecord {
        let mut record = InstanceRecord::new(Value::from_bits(0x11), ProgramId::MAIN, 4);
        record.memory = Value::from_bits(0x22);
        record.memory_length = 7;
        (record.guard_start, record.guard_end) = (WORDS[0], WORDS[1]);
        record.call_reference_diagnostic = WORDS[2];
        record.reference_call_site_count = 3;
        record.import_callout_count = 5;
        record.table = WORDS[3] as *const usize;
        record
    }

    /// The bytes of `length` at `offset` into `value`.
    fn bytes<T>(value: &T, offset: i32, length: usize) -> &[u8] {
        // SAFETY: the offsets are those of fields of `T`, which are initialized.
        unsafe { std::slice::from_raw_parts((value as *const T).cast::<u8>().add(offset as usize), length) }
    }

    #[test]
    fn a_context_takes_the_words_generated_code_copies() {
        let record = record();
        let mut context = CallContext::native_adapter(std::ptr::null_mut(), std::ptr::null(), None);
        context.enter(&record);
        for (at, from, ty) in SWITCHED {
            let length = ty.bytes() as usize;
            assert_eq!(bytes(&context, *at, length), bytes(&*record, *from, length), "{at} {from}");
        }
        assert_eq!(context.instance as usize, record.address());
        assert_eq!(context.table as usize, WORDS[3]);
    }

    #[test]
    fn a_record_names_itself_and_so_does_a_copy_of_it() {
        let record = record();
        let copy = record.clone();
        assert_eq!(record.address(), &*record as *const InstanceContext as usize);
        assert_eq!(copy.address(), &*copy as *const InstanceContext as usize);
        assert_ne!(copy.address(), record.address());
        assert_eq!(copy.globals, copy.global_area.as_ptr());
    }

    #[test]
    fn a_record_is_unpublished_until_an_artifact_is() {
        let record = record();
        assert!(record.table != std::ptr::null() && record.abi == ABI_NOT_DIRECT);
        let arguments = MemoryArguments { base: true, length: false };
        assert_eq!(arguments.word(), MemoryArguments::BASE);
        assert_ne!(arguments.word(), ABI_NOT_DIRECT);
    }

    #[test]
    fn the_native_roots_hold_what_was_pushed_and_no_more() {
        let mut roots = NativeRoots::new();
        assert!(roots.live().is_empty());
        // SAFETY: the buffer holds `capacity` values, and slot 0 is one of them.
        unsafe { *roots.slots = Value::from_bits(5) };
        roots.set_depth(1);
        assert_eq!(roots.live(), [Value::from_bits(5)]);
        assert_eq!(roots.depth(), 1);
    }
}
