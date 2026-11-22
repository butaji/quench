//! The types the VM core holds for its engine layer (`quench-runtime-engine`: lane handlers
//! and compiled tiers). Their behaviour stays in the engine, as extension traits on them.
use super::*;

/// Activations a lane runs without VM frames, innermost last, over the
/// bottom VM frame of the lane run. They exist only while the lane runs:
/// every exit makes them VM frames with pending calls, exactly as frame
/// pushes would have, so the general loop, traps and collection see the
/// usual frame stack. The register storage grows only between activations,
/// so register windows stay put while any exists.
pub struct LaneStack {
    /// Per call record (by its `call_link`'s address), the callees it resolved or memoized,
    /// in the order first seen, up to `tier_policy::POLYMORPHISM` (`note_site_callee`): the targets a region
    /// inlines as a guarded switch (`lane_unit::unit`). A link's slot may hold another site's
    /// callee since; this record alone names the site's own. A collection drops the callees it
    /// swept (`forget_dead_callees`).
    pub call_sites: std::cell::RefCell<rustc_hash::FxHashMap<usize, smallvec::SmallVec<[crate::compiled::CallTarget; INLINE_SITE_TARGETS]>>>,
    /// Pairs of a call record's link address and a callee its `call_sites` record holds, direct-mapped
    /// (`noted_slot`): a polymorphic site's steady state re-notes the callees it alternates between, and
    /// a hit here skips the map. Any callee leaving a record empties it.
    pub noted: Box<[std::cell::Cell<Option<(usize, crate::compiled::CallTarget)>>; NOTED_SITE_CALLEES]>,
    /// `noted` for the script-target entries a link resolved (`Vm::note_linked_callee`): a link address and
    /// the callee its record holds as `CalleeBinding::Identity`, by link address and entry index.
    pub linked: Box<[std::cell::Cell<(usize, Value)>; NOTED_SITE_CALLEES]>,
    pub values: LaneValues,
    /// The activation records. Read and written only through `activations` and
    /// `activations_mut` (call protocol v2 stage C): a plain activation keeps its frame header
    /// in the value stack and becomes a record only when something reads the records.
    pub activation_records: std::cell::UnsafeCell<LaneActivations>,
    /// The plain activations above the innermost record, whose headers the value stack holds.
    pub plain: PlainFrames,
    /// The side records, innermost last, one per frame that has one (one-frame-one-call.md F4):
    /// each plain header whose packed word carries a side flag (`frame::SIDE_FLAGS`), and each
    /// record whose completion carries a payload (`LaneStack::record_sided`). A side record is
    /// tagged by its header's flags and holds what a record would beyond the header, and what
    /// the frame's completion carries (`lane_frames::SidePayload`: a skipped forwarder, a
    /// settling promise, a Wasm caller, a trap's invariants), so it pushes and pops with its
    /// frame. Written through `&mut self` and by `spill_plain`, which makes its frames records.
    pub sides: std::cell::UnsafeCell<Vec<lane_frames::Side>>,
    /// The plain headers each record pushed over them sealed (`frame_walk::PlainSegment`),
    /// innermost last: a record's push saves the `plain` words instead of spilling the headers,
    /// and its pop restores them (`LaneStack::seal_plain`, `unseal_plain`).
    pub segments: std::cell::UnsafeCell<Vec<frame_walk::PlainSegment>>,
    /// The headers the segments hold, which `activation_count` counts.
    pub sealed: std::cell::Cell<usize>,
    /// The admission bound emitted code checks native headers against (`JsStepDispatch::plain_depth_limit`):
    /// the activation limit while the frames are bound (`native_frames_enabled`), else 0. Rust's frame
    /// changes void it (`frames_changed`), so emitted calls take the helper row until the next binding.
    pub plain_depth_limit: Box<std::cell::Cell<usize>>,
    /// The frames below the plain headers, records and sealed alike (`frames_below_plain`), as of the last
    /// binding: emitted pushes index their tables by it plus the plain depth, which is `activation_count()`.
    pub frames_below: Box<std::cell::Cell<usize>>,
    /// The total frame count emitted native pushes index their body and descriptor tables by
    /// (`JsStepDispatch::body_top`): `activation_count()` as of the last binding, then moved by emitted
    /// pushes and pops. Indexing by the total count, not the plain depth, keeps a record's push from
    /// reusing the entry of a frame it sealed (one-frame-one-call.md F5).
    pub body_top: Box<std::cell::Cell<usize>>,
    /// The running body (`LaneMethod`) of each native header, by its total frame index (`body_table`).
    pub body_table: Vec<usize>,
    /// The descriptor (`DirectCallee`) of each native header, by its total frame index (`native_table`).
    pub native_table: Vec<usize>,
    /// The return trampoline of the native callbacks pushed as plain activations
    /// (`CallbackTrampoline`), at an address their headers name.
    pub callback_return: Box<CallbackTrampoline>,
    /// Wasm activation payload represented by `values` before lane materialization.
    pub wasm_storage_bytes: usize,
    /// Resolved targets of lane `Call` records (`Vm::script_call_entry`) of callees of the
    /// running program.
    pub script_targets: ScriptTargets,
    /// Resolved targets of callees of another program than the calling one, which those
    /// calls only activate (`Vm::resolve_script_call_target`): one per callee serves every
    /// other program, apart from the target its own program's calls use.
    pub cross_targets: ScriptTargets,
    /// The last target resolved for a closure of each function (`FUNCTION_TARGETS`), under
    /// its generation and program: every field but the closure's own (`callee`, `env`,
    /// `capture`, an arrow's `lexical_this`) depends on the function alone.
    pub function_targets: Box<[ScriptCallTarget; FUNCTION_TARGETS]>,
    /// `function_targets` of other programs' callees (`cross_targets`), whose closures a
    /// program's code makes afresh as often as its own.
    pub cross_function_targets: Box<[ScriptCallTarget; FUNCTION_TARGETS]>,
    /// Constructors' plans (`initialize_fields_in_lane`, `planned_receiver_in_lane`).
    pub construct_plans: Box<[ConstructPlan; CONSTRUCT_PLANS]>,
    /// Per plan, the one dependency its proof is (`watch_construct_plan`): published while
    /// every object the plan read keeps its values and layout, and the chains above them their
    /// layouts.
    pub construct_watches: Box<[Option<crate::heap::PrototypeValueWatch>; CONSTRUCT_PLANS]>,
    /// The reads call sites answered (`ReadMemo`), each in the slot its record names
    /// (`CallOperand::memo`).
    pub read_memos: Vec<ReadMemo>,
    /// The store sites a general store called a setter at (`setters::met_setter`), one bit per
    /// cache index modulo `setters::SETTER_SITE_BITS`; empty until the first such call, so a
    /// VM that never meets a setter holds none of it. Only a filter: a clear bit sends the
    /// store to the general path, which calls the setter and sets it.
    pub setter_sites: Vec<u64>,
    /// The callee each in-place native row admits in this generation (`native_calls::NativeIdentities`).
    pub native_identities: native_calls::NativeIdentities,
    /// The stack bounds the lane's native calls are checked against (`Vm::call_native_from_lane`),
    /// read at each lane entry (`enter_generation`), under the guard that entry holds.
    pub native_stack: crate::stack::StackLimit,
    /// The array iterations of the `Completion::ArrayStep` activations, innermost last; each
    /// activation's frame takes its own when it materializes (`CallCompletion::ArrayStep`).
    /// Not a side record: an iteration outlives each of its callback's frames, which it pushes
    /// again per element (`step_header_again_in_lane`), and a trampoline's header names its entry
    /// by index (one-frame-one-call.md F4 (3)).
    pub steps: Vec<LaneArrayStep>,
    /// The iterators the activations' `for-of` loops would close, each with the index of
    /// the activation whose loop pushed it (`IteratorCleanupPush`), innermost last: a
    /// frame's `active_iterators`, which the activation's frame takes when it materializes.
    /// A completion drops its activation's entries, as a frame's pop drops its list.
    pub iterators: Vec<(usize, crate::vm::ActiveIterator)>,
    /// The JavaScript-backed import a Wasm call last activated (`wasm_calls::lane_activate_js_import`),
    /// read off its wrapper once; a collection forgets it (`forget_dead_callees`).
    pub js_import: Option<JsImportPlan>,
    /// The bound function of a user function a `function_call_miss` last forwarded through,
    /// with its target, bound receiver and list of bound arguments (`undefined` for none), which
    /// never change (`Vm::bound_function_words`): the next call of it reads no parts. Not roots:
    /// the bound function in the call's callee register keeps them, and a collection forgets
    /// them (`forget_dead_callees`) before any id is reused.
    pub bound_call: Option<(Value, Value, Value, Value)>,
    /// The `with` objects of the activations' scope chains (`with_scopes`), each with the index of
    /// the activation that owns it, innermost last: a frame's `with_objects`, which the
    /// activation's frame takes when it materializes. A completion drops its activation's
    /// entries, as a frame's pop truncates the VM's scope stack.
    pub with_objects: Vec<(usize, Value)>,
    /// The records ordinary activations made for a direct eval (`lane_eval`), each with the
    /// index of the activation that owns it, innermost last; the activation names it
    /// (`ScriptActivation::record`). A completion drops its activation's, and a materialization
    /// its frame's own state takes over: the environment the eval promoted.
    pub records: Vec<(usize, Box<crate::vm::activation::FrameRecord>)>,
    /// The forwarding constructor bodies call targets name (`ScriptCallTarget::forwarded_method`),
    /// each once: a program's bodies are fixed, so the table only grows with distinct ones.
    pub forwarded_methods: Vec<crate::bytecode::ForwardedThisMethod>,
    /// The activation count at which a JavaScript lane call leaves for
    /// `script_call_frame_in_lane`: the activations' capacity, the guest calls the thread still
    /// admits (the activations hold no guards; their frames take them when they materialize,
    /// which `call_native_in_lane` does before any guest code runs), or none while a collection
    /// is due (`note_allocation`). One comparison stands for the three.
    pub activation_limit: std::cell::Cell<usize>,
    /// Names the collection epoch and realm of the running lane, which no lane instruction
    /// changes: a target resolved under another generation is stale. Lane runs move between
    /// programs far more often than either changes, so each target names its program instead.
    pub generation: ScriptGeneration,
    /// `generation` and the running program (`Vm::active_program`), which a cached target's
    /// own admission must equal.
    pub admission: Admission,
    /// `Admission::REFUSED` once no cached call may run (`refuse_cached_calls`), else 0.
    pub refused: u64,
    /// The epoch and realm `generation` names.
    pub generation_key: (u64, Value),
    /// The boundary below a deferred native's callback while it runs (`NativeBoundary`).
    pub native_boundary: Option<NativeBoundary>,
    /// The frame the innermost activation a body's call runs in the lane became, once a
    /// materialization made it one (`body_calls::finish`): its return leaves the lane.
    pub body_boundary: Option<usize>,
    /// The frames the boundary's caller activations became, once a materialization made them.
    pub boundary_frames: Option<usize>,
    /// The exported Wasm function a JavaScript call last entered (`Vm::wasm_export_frame_call`),
    /// so the next call of it reads its closure and signature once. A collection forgets it.
    pub wasm_export: Option<crate::vm::wasm_js::WasmExportCall>,
    /// The window plans last derived for frames that made no environment of their own
    /// (`generator_windows::cached_window_plan`), one slot per function index modulo
    /// `SHARED_PLANS`: every new activation of the same function under the same environment,
    /// as each call of an async function is, shares it.
    pub last_window_plan: [Option<(ProgramId, u32, generator_windows::WindowPlan)>; generator_windows::SHARED_PLANS],
    /// The records of the two generators last looked up, the last first, each under the
    /// collection epoch it was found in (`Vm::generator_record_mut`): a step, and a delegate's
    /// step alternating with its delegating generator's, finds them without a heap lookup. A
    /// collection, the only way a generator's cell is freed and its index reused, retires them.
    pub last_generator: [(Value, u64, *mut crate::vm::activation::GeneratorRecord); 2],
}

/// A lane stack a nested run parked (`Vm::nested_lane_run`) with the receiver and environment of
/// its innermost activation, which the lane's context held.
pub struct ParkedLane {
    pub stack: Box<LaneStack>,
    pub innermost: [Value; 2],
}

/// A lane native call made beside the lane's activations (`Vm::runs_beside_activations`):
/// the lane's context and the call's record, whose frames the first observer of the frame
/// stack makes (`Vm::materialize_deferred_lane`).
#[derive(Clone, Copy)]
pub struct DeferredLaneCall {
    /// The lane's `LaneContext`, of the VM's host.
    pub cx: *mut (),
    pub call_pc: usize,
    /// The frame count the native runs at, below its caller's activations.
    pub frames: usize,
}

/// What a lookup reads of the window activation making it, which it reads as it reads a frame
/// (`CallerView`), and the registers and locals its steps read and write.
pub struct WindowScope {
    pub function: u32,
    pub program: ProgramId,
    /// The activation's receiver as the lane holds it.
    pub this: Value,
    /// The activation's environment: its own once a closure made one (`captured`), else the
    /// closure's.
    pub env: Value,
    pub captured: bool,
    /// The activation's record, null for an ordinary function's: it holds no dynamic bindings,
    /// which only a frame's direct eval makes.
    pub record: *mut FrameRecord,
    /// The resume pc of the binding site the access is at (`Frame::binding_site_pc`), if it is
    /// at one.
    pub site: Option<u32>,
    /// The `with` objects of the activation's scope chain (`LaneStack::with_chain`), outermost
    /// first, which no `with` statement has entered while none is (`Vm::with_scope_entered`).
    pub chain: WithChain,
    /// The activation's register file, which its environment window follows.
    pub registers: Registers,
    /// The first of the window's locals, which the activation's `Function::locals` fill.
    pub locals: *mut Value,
    pub local_count: usize,
}

/// A native a lane called from its running frame after its activations became frames
/// (`call_native_in_lane`): the lane's `LaneContext`, the call's record and the frame count the
/// native runs at. A JavaScript call the native makes while all three still hold runs as an
/// activation of that lane (`Vm::native_callback_in_lane`).
#[derive(Clone, Copy)]
pub struct LaneNativeCall {
    pub cx: *mut (),
    pub call: Ip,
    pub frames: usize,
}

/// A function's lane view: one record per instruction, and the one register
/// its direct memory accesses go through. Accesses through any other
/// register, and lane instructions other than the binding load that would
/// write this one, take the general path, so the memory view tracks the
/// register exactly.
pub struct LaneView {
    /// A record's handler may change while the lane runs (`requicken`); its other fields do not.
    pub records: Records,
    /// Per instruction, whether the general loop enters the lane there: the
    /// straight-line lane run from it is long enough to repay the lane's
    /// entry cost, and its record does not read the accumulator. Kept apart
    /// from the records, which only the lane reads.
    pub entries: Box<[u64]>,
    /// The thresholds its VM's tier controllers read, as the VM held them when the view was derived.
    pub tiers: &'static Tiers,
    pub memory_register: Option<u16>,
    /// How a lane call activates the function without a frame; `None`
    /// when its activation needs a frame (locals outside registers).
    pub activation: Option<ActivationShape>,
    /// The facts of its body a frameless call target records (`TargetBody`), found on its
    /// first resolution: every closure of the function shares them.
    pub target_body: std::cell::OnceCell<TargetBody>,
    pub _string_case_filters: Box<[string_case_filter::StringCaseFilter]>,
    /// The window of an environment activation, when its local and closure records also run
    /// in one (`specialize_environment_window`).
    pub environment_window: Option<EnvironmentWindow>,
    /// Whether the straight-line lane run from the function's first instruction reaches its
    /// `Return` (`entry_run_returns`): a call from a native runs it whole in the lane
    /// (`Vm::run_frame_from_lane`), whatever the run's length, since that call enters no
    /// general loop the lane would save.
    pub returns_from_entry: bool,
    /// The loops whose backward jumps run `loop_jump_row` (`loop_jump_miss` on a miss). The view owns them for the
    /// records that point into them, which are their only readers.
    pub loops: Box<[LoopSite]>,
    /// The backward jumps' PCs, ascending, each with the index in `loops` of the first loop
    /// it closes: a leave finds its loop by a binary search over the few loops, where a table
    /// over every instruction cost 4 bytes per instruction of every view.
    pub loop_at_backedge: Box<[(u32, u32)]>,
    /// The vector regions whose first records point into them (`vector_region`): only
    /// those records read them.
    #[cfg_attr(not(test), allow(dead_code))]
    pub vector_regions: Box<[vector_region::VectorRegion]>,
    /// The loop kernels whose test records name them (`loop_kernel`).
    pub _loop_kernels: Box<[Box<loop_kernel::LoopKernel>]>,
    /// The record an activation starts at (`LaneView::start`): its first, or `prologue`
    /// while the JavaScript tier may claim the function (`LaneView::claim_for_tier`).
    pub start: std::cell::Cell<Ip>,
    /// The tier prologue (`tier_prologue_in_lane`), outside the records so the function's PCs
    /// stay its records'; written when the tier may claim the function.
    pub prologue: std::cell::UnsafeCell<LaneInstruction>,
    /// The prologue's direct run of the compiled body, once the tier compiled it
    /// (`Vm::direct_tier_entry`).
    pub tier_direct: std::cell::Cell<Option<crate::compiled::DirectTierEntry>>,
    /// The lane-body tier's state of this function (`LaneTier`), made when the tier first
    /// looks at it: a view the tier never touches (every view under `--no-jit`, most views
    /// otherwise) keeps one word instead of about 540 bytes.
    pub tier: std::cell::OnceCell<Box<LaneTier>>,
    /// What native callers read to enter its body (`compiled::EntryCell`); the one entry word
    /// of the function, published and withdrawn with its body.
    pub entry: crate::compiled::entry_cell::EntryCell,
    /// Per record, the register whose value it reads as the carried accumulator
    /// (`NO_ACCUMULATOR` where it reads none): a lane body that leaves at it resumes
    /// with that register carried, as falling through from its producer would.
    pub accumulator_registers: Box<[u16]>,
    pub plain_backward_jump_sites: usize,
    pub osr_rejections: Option<Box<[(crate::compiled::RegionRejection, usize)]>>,
    /// The `GetField`, `SetField` and `LoadCapture` records past the loop headers its sites
    /// hold, whose own-field hints a shape compaction clears.
    pub field_records: Box<[u32]>,
    /// The by-name load and store records past the loop headers its sites hold, whose
    /// global-object hints (`name_row`, the `StoreName` row's `global_name_store`)
    /// a shape compaction clears, and a change of the global
    /// lexical names they answer past (`LaneView::clear_name_hints`).
    pub name_records: Box<[u32]>,
    /// The operator records whose proven form reads a speculative int32 register
    /// (`int_kinds::Kind::Speculative`), each with that proven handler and its checked form:
    /// what `retire_int_speculation` restores once an update overflowed one.
    pub int_speculating: Box<[(u32, *const (), *const ())]>,
}

/// The state every compiled function receives. Field offsets are the ABI
/// compiled code loads from.
#[repr(C)]
pub struct CallContext {
    /// Every field compiled code loads, at its `CallContextHead` offset (`CALL_CONTEXT_ADAPTER_PAYLOAD`).
    pub head: CallContextFields,
    // Fields below are the VM's, never read by the stencils; the last one, `wasm_gc_records`, is read by the
    // u-jit's emitted code at its `offset_of!`, and is last so the offsets above stay where the stencils hold them.
    /// Borrowed only during a running entry; a non-reentrant region may prove unique ownership.
    pub interrupt_owner: *const crate::ExecutionInterrupt,
    /// The record of the instance this context is in (`InstanceContext`), whose words the fields
    /// it shares by name hold (`SWITCHED`): what a mirror row's instance is compared with.
    pub instance: *const InstanceContext,
    /// The VM's roots for the instances native code enters beyond this one.
    pub native_roots: *mut NativeRoots,
    /// The VM's `memory_grow_callout`: (context, pages) -> the size before in pages, or -1.
    /// A stitched `memory.grow` row calls it (`CONTEXT_MEMORY_GROW`).
    pub memory_grow: usize,
    /// The base of the Wasm GC record range (`Heap::wasm_gc_record_base`), zero until it is reserved. The
    /// u-jit's array reads take an element's slot from the record at `base + id * record size`, and call
    /// the `field_slot` callout where it is zero. Read by the u-jit's emitted code, not the stencils.
    pub wasm_gc_records: usize,
}

/// The values native code keeps alive across a call that can collect: the environments of the
/// instances it is running in beyond its own (`wasm_instance_switch`), and the reference registers
/// of the compiled frames it runs (`wasm_roots`), which no frame or table need hold.
///
/// The slots are a heap buffer of `capacity` values, owned by the record and freed with it. Code
/// reads the buffer's base from `SLOTS` and the depth from `DEPTH`, and checks `CAPACITY` before it
/// pushes.
#[repr(C)]
pub struct NativeRoots {
    pub depth: u64,
    pub capacity: u64,
    pub slots: *mut Value,
}

/// The per-VM storage of the callee admission cache.
#[derive(Default)]
pub struct CalleeAdmissions {
    /// The cache, made when native code first binds it (`bind`): a VM that runs no native body keeps
    /// none, and one that does commits only the pages its admissions reach (`zeroed_boxed_slice`).
    pub cache: std::sync::OnceLock<Box<[CallEntry]>>,
    /// Each function's last admission, by function index, with its program and closure
    /// environment: a fresh closure of it shares all but its callee and captures (`admit`).
    pub functions: Vec<(u32, Value, CallEntry)>,
}

/// A numeric function compiled ahead of time: position-independent code with the relocations
/// that link it, and what its entry returns. It is the same lowering as the live JIT's
/// (`lower_numeric_region`); loading links it into the artifact the JIT publishes.
#[derive(Debug)]
pub struct NumericImage {
    pub code: crate::vm::jit_backend::relocatable_code::RelocatableCode,
    pub params: usize,
    pub number_result: NativeNumberResult,
}

/// A program's compiled functions, sharing one code mapping and one call table.
pub struct CompiledProgram {
    pub _code: CodeMemory,
    pub table: Box<[usize]>,
    /// Each function's entry address, which stitched rows call through (`wasm_stitch::World`).
    pub _entry_cells: Box<[std::cell::Cell<usize>]>,
    /// The compiled functions in index order: a selective tier holds a few of a module's
    /// thousands, so only `positions` is as long as the module.
    pub functions: Box<[CompiledFunction]>,
    /// Each module function's place in `functions`; `NOT_COMPILED` where it has none.
    pub positions: Box<[u32]>,
    /// The environment slot of the memory compiled code accesses.
    pub memory_slot: Option<u32>,
    /// Memory accesses rely on the memory's guard reservation instead of
    /// bounds checks.
    pub guarded: bool,
    /// The guarded memory's base, when code addresses it as a constant
    /// displacement (see `displacement_base`); calls with another base fall
    /// back to the interpreter.
    pub fixed_base: Option<i32>,
    pub diagnostics: Option<WasmJitDiagnostics>,
    /// How often the VM's callout resolved each target at each actual compiled `call_indirect`
    /// site.
    pub indirect_hits: indirect_profile::SiteHits,
    /// The owner, bytecode PC and table binding of each site `indirect_hits` counts.
    pub reference_cache_sites: Vec<(usize, usize, u32)>,
}

/// Publication owns future entry; active runs and direct callers own code leases.
/// Retirement unlinks entry without releasing code that execution or recovery uses.
/// Guest locator validity and exact exits remain separate obligations.
pub type PublishedLaneMethod = crate::code_dependencies::PublishedCode<LaneMethod>;

/// A program's ahead-of-time image, and its link into this process once a VM loads it.
pub struct AheadImages {
    pub image: AotImage,
    /// The programs (by number) checked against it, and whether each is it.
    pub known: std::cell::RefCell<Vec<(u32, bool)>>,
    /// The image placed in the loading VM's executable memory; `None` where it did not link.
    pub linked: Option<LiveImage>,
    /// The address of the entry table the system linker placed it at, for an image of the executable (`of_linked`).
    pub placed: Option<usize>,
    /// The program's closed-world facts, which shaped its lane bodies' units (`unit_of`): the
    /// compile's, or derived once again from the program where the image was read from a file.
    pub facts: std::cell::OnceCell<AotFacts>,
}

/// What the tier finds in a function at its entries. Each fact depends only on the function and
/// its own program, which never change, so it is derived once per function
/// (`ProgramStore::js_entry_facts`).
#[derive(Default)]
pub struct JsEntryFacts {
    /// Whether the function is a numeric tier candidate (`function_candidate`); only such a
    /// function enters native code at its entry.
    pub candidate: bool,
}

/// A guarded JavaScript numeric function compiled from a narrow residual
/// bytecode subset. It owns one W^X executable mapping and has no heap roots.
pub struct CompiledJsFunction {
    pub _code: CodeMemory,
    pub entry: BodyEntry,
    pub result: NativeNumberResult,
    pub params: usize,
    pub mapped_bytes: usize,
    #[cfg(test)]
    pub native_entries: std::cell::Cell<u64>,
}
