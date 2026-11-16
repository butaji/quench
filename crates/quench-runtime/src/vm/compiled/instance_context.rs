//! The persistent context of a Wasm instance: one heap-stable record per instance, owned by the
//! instance's environment, that holds what native code reads of the instance in place: its
//! global area, its tables' published words, the code table of the artifact it runs, its linear
//! memory and the diagnostics of that artifact. A call context takes the record's words
//! (`SWITCHED`), so entering an instance, from the VM or from another instance's code, is the
//! same copy; a mirror row names its function's instance by the record's address, which lives as
//! long as the environment the function keeps alive.

use super::{
    CallContext, CompiledProgram, DiagnosticReferenceCallout, Host, MemoryArguments,
    ProgramId, ReferenceCallSiteDiagnostics, Vm, diagnostic_reference_callout, displacement_base,
};
use crate::Value;
use crate::heap::{Cell, TableWords};
use cranelift_codegen::ir::{self, types};
use std::rc::Rc;

/// The `abi` of a record whose artifact cannot be called natively by another instance's code: its
/// memory can change length under a caller that does not pass the current one.
pub(super) const ABI_NOT_DIRECT: u64 = u64::MAX;

#[repr(C)]
pub(crate) struct InstanceContext {
    env: Value,
    /// The record itself: what a call context names the running instance by, and a mirror row
    /// names its function's.
    instance: *const InstanceContext,
    /// Per environment slot, the address of the native slot of the global bound there (null for
    /// any other binding): the global area. A global is shared by every instance that imports
    /// it, so the area holds its address, which its cell keeps.
    globals: *const *mut u64,
    /// Per environment slot, the words that publish the table bound there (null for any other).
    table_words: *const *const TableWords,
    /// The code table of the artifact last published (`publish`): per function, its body or null.
    table: *const usize,
    compiled: *const CompiledProgram,
    program: ProgramId,
    memory: Value,
    memory_length: u64,
    guard_start: usize,
    guard_end: usize,
    call_reference_diagnostic: usize,
    reference_call_sites: *const ReferenceCallSiteDiagnostics,
    reference_call_site_count: usize,
    import_callout_counts: *const std::cell::Cell<u64>,
    import_callout_count: usize,
    /// The base of the memory, which a callee that takes one is passed.
    memory_base: u64,
    /// The memory arguments of the artifact's bodies (`MemoryArguments::word`), or
    /// `ABI_NOT_DIRECT`.
    abi: u64,
    global_area: Box<[*mut u64]>,
    table_area: Box<[*const TableWords]>,
    /// Every artifact published, kept until the instance dies: a body another instance's code
    /// is running may not be freed by a later publication.
    retained: Vec<Rc<CompiledProgram>>,
}

/// The memory an artifact is entered with in an instance: its binding, base, current length and
/// guard reservation.
pub(super) struct MemoryBinding {
    pub(super) binding: Value,
    pub(super) base: u64,
    pub(super) length: u64,
    pub(super) guard: Option<(*mut u8, usize)>,
}

/// Declares the fields a call context and a record share by name and type: `SWITCHED`, their
/// offsets in the two structs with the machine type of each, which generated code copies to
/// enter another instance, and `CallContext::enter`, which the VM copies them with.
macro_rules! switched {
    ($($field:ident: $ty:expr),+ $(,)?) => {
        /// The offset in a call context, the offset in a record and the machine type of each
        /// field that belongs to an instance.
        pub(super) const SWITCHED: &[(i32, i32, ir::Type)] = &[$((
            std::mem::offset_of!(CallContext, $field) as i32,
            std::mem::offset_of!(InstanceContext, $field) as i32,
            $ty,
        )),+];

        impl CallContext {
            /// Enters the instance of `record`: the context takes its words.
            pub(super) fn enter(&mut self, record: &InstanceContext) {
                $(self.$field = record.$field;)+
            }
        }
    };
}

switched! {
    env: types::I64,
    instance: types::I64,
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
}

impl InstanceContext {
    pub(super) const MEMORY_BASE: i32 = std::mem::offset_of!(Self, memory_base) as i32;
    pub(super) const MEMORY_LENGTH: i32 = std::mem::offset_of!(Self, memory_length) as i32;
    pub(super) const ABI: i32 = std::mem::offset_of!(Self, abi) as i32;
    pub(super) const TABLE: i32 = std::mem::offset_of!(Self, table) as i32;
    pub(super) const ENV: i32 = std::mem::offset_of!(Self, env) as i32;

    /// The record of the instance of `program` whose environment is `env` and which binds
    /// `slots` things; nothing is published yet.
    pub(crate) fn new(env: Value, program: ProgramId, slots: usize) -> Box<Self> {
        let mut record = Box::new(Self {
            env,
            instance: std::ptr::null(),
            globals: std::ptr::null(),
            table_words: std::ptr::null(),
            table: std::ptr::null(),
            compiled: std::ptr::null(),
            program,
            memory: Value::UNDEFINED,
            memory_length: 0,
            guard_start: 0,
            guard_end: 0,
            call_reference_diagnostic: 0,
            reference_call_sites: std::ptr::null(),
            reference_call_site_count: 0,
            import_callout_counts: std::ptr::null(),
            import_callout_count: 0,
            memory_base: 0,
            abi: ABI_NOT_DIRECT,
            global_area: vec![std::ptr::null_mut(); slots].into(),
            table_area: vec![std::ptr::null(); slots].into(),
            retained: Vec::new(),
        });
        let address: *const Self = &*record;
        record.instance = address;
        record.globals = record.global_area.as_ptr();
        record.table_words = record.table_area.as_ptr();
        record
    }

    /// The address a mirror row names the instance by.
    pub(crate) fn address(&self) -> usize {
        self.instance as usize
    }

    /// Fills the areas from the bindings of the environment, by slot.
    pub(super) fn bind(
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
    pub(super) fn publish<H: Host>(&mut self, compiled: &Rc<CompiledProgram>, memory: &MemoryBinding) {
        if !std::ptr::eq(self.compiled, Rc::as_ptr(compiled)) {
            self.compiled = Rc::as_ptr(compiled);
            self.table = compiled.table.as_ptr();
            self.abi = compiled.abi();
            let stats = compiled.diagnostics.as_ref();
            self.call_reference_diagnostic = stats.map_or(0, |_| {
                diagnostic_reference_callout::<H> as DiagnosticReferenceCallout as usize
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
    }
}

/// A record in a heap cell: boxed, so its address stays, and a copy is a record of its own.
pub(crate) struct InstanceRecord(Box<InstanceContext>);

impl InstanceRecord {
    pub(crate) fn new(env: Value, program: ProgramId, slots: usize) -> Self {
        Self(InstanceContext::new(env, program, slots))
    }
}

impl std::ops::Deref for InstanceRecord {
    type Target = InstanceContext;
    fn deref(&self) -> &InstanceContext {
        &self.0
    }
}

impl std::ops::DerefMut for InstanceRecord {
    fn deref_mut(&mut self) -> &mut InstanceContext {
        &mut self.0
    }
}

impl Clone for InstanceRecord {
    fn clone(&self) -> Self {
        let mut copy = InstanceContext::new(self.env, self.program, self.global_area.len());
        copy.global_area.copy_from_slice(&self.global_area);
        copy.table_area.copy_from_slice(&self.table_area);
        copy.table = self.table;
        copy.compiled = self.compiled;
        copy.abi = self.abi;
        copy.retained.clone_from(&self.retained);
        Self(copy)
    }
}

impl std::fmt::Debug for InstanceRecord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("InstanceRecord")
    }
}

impl MemoryArguments {
    const BASE: u64 = 1;
    const LENGTH: u64 = 2;

    /// The arguments as the word a record publishes and a call site compares.
    pub(super) fn word(self) -> u64 {
        (if self.base { Self::BASE } else { 0 }) | (if self.length { Self::LENGTH } else { 0 })
    }
}

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

impl<H: Host> Vm<H> {
    /// The environment slot of `env`'s instance record: the last before the imported functions,
    /// which end the layout (`wasm_function_reference` derives their base from the end).
    pub(crate) fn wasm_context_slot(&self, env: Value, program: ProgramId) -> Option<usize> {
        let Some(Cell::Environment { slots, .. }) = self.heap.get(env) else {
            return None;
        };
        let imports = self.programs.wasm_signatures(program)?.imports().len();
        slots.len().checked_sub(imports + 1)
    }

    /// The record of the instance `env` of `program`.
    pub(crate) fn wasm_instance_record(&self, env: Value, program: ProgramId) -> Option<&InstanceContext> {
        let slot = self.wasm_context_slot(env, program)?;
        match self.heap.get(self.heap.environment_slot(env, slot)?)? {
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
        let Some(Cell::Environment { slots, .. }) = self.heap.get(env) else {
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
            let Some(binding) = self.heap.environment_slot(env, slot) else {
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
        let record = self.heap.environment_slot(env, context)?;
        let Some(Cell::WasmInstance(record)) = self.heap.get_mut(record) else {
            return None;
        };
        record.bind(globals, tables);
        Some(())
    }

    /// The memory `compiled` is entered with in the instance `env`: its binding, base, length
    /// and guard reservation, none where the memory cannot be entered with this artifact (its
    /// code relies on a guard or a base the memory does not have).
    pub(super) fn memory_binding(&self, compiled: &CompiledProgram, env: Value) -> Option<MemoryBinding> {
        let Some(slot) = compiled.memory_slot else {
            return Some(MemoryBinding { binding: Value::UNDEFINED, base: 0, length: 0, guard: None });
        };
        let binding = self.heap.environment_slot(env, slot as usize)?;
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
    pub(super) fn publish_wasm_instance(
        &mut self,
        compiled: &Rc<CompiledProgram>,
        env: Value,
        program: ProgramId,
    ) -> Option<(*const InstanceContext, (u64, u64))> {
        let memory = self.memory_binding(compiled, env)?;
        let context = self.wasm_context_slot(env, program)?;
        let record = self.heap.environment_slot(env, context)?;
        let Some(Cell::WasmInstance(record)) = self.heap.get_mut(record) else {
            return None;
        };
        record.publish::<H>(compiled, &memory);
        Some((&**record as *const InstanceContext, (memory.base, memory.length)))
    }
}

/// The values native code keeps alive across a call into another instance: the environments of
/// the instances it is running in beyond its own, which no frame or table need hold.
#[repr(C)]
pub(crate) struct NativeRoots {
    depth: u64,
    slots: [Value; NATIVE_ROOT_CAPACITY],
}

/// How many instances native code may be inside beyond the one it entered; a deeper call leaves
/// through the VM, which roots its own.
pub(super) const NATIVE_ROOT_CAPACITY: usize = 256;

impl NativeRoots {
    pub(super) const DEPTH: i32 = std::mem::offset_of!(Self, depth) as i32;
    pub(super) const SLOTS: i32 = std::mem::offset_of!(Self, slots) as i32;

    pub(crate) fn new() -> Box<Self> {
        Box::new(Self { depth: 0, slots: [Value::UNDEFINED; NATIVE_ROOT_CAPACITY] })
    }

    /// The environments held now: the roots of a collection.
    pub(crate) fn live(&self) -> &[Value] {
        &self.slots[..self.depth as usize]
    }

    /// The depth an entry restores when it ends, whatever left native frames unreturned.
    pub(super) fn depth(&self) -> u64 {
        self.depth
    }

    pub(super) fn set_depth(&mut self, depth: u64) {
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
        roots.slots[0] = Value::from_bits(5);
        roots.set_depth(1);
        assert_eq!(roots.live(), [Value::from_bits(5)]);
        assert_eq!(roots.depth(), 1);
    }
}
