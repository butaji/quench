//! The types the VM core holds for its engine layer (`quench-runtime-engine`: lane handlers
//! and compiled tiers). Their behaviour stays in the engine, as extension traits on them.
#![allow(unused_imports)]
use super::*;
use crate::bytecode::InstructionField;
use crate::staged::effects::{call_form, CallForm, Signature};
use std::cell::{RefCell, UnsafeCell};
use std::ops::{Deref, DerefMut};
use smallvec::SmallVec;
use crate::bytecode::Function;
use crate::staged::frame::{CONTEXT_THIS, CONTEXT_ENV};
use super::lane_values::HEAP_WINDOW;
use super::tier_machine::{TierEvent, TierState};
use quench_bjit::shared_words::KIND_NATIVE;
use crate::heap::PlainPropertyStorage;
use super::activation::JobRoot;
use super::caller_view::CallerView;
use super::activation::frame_record::extend_this_env_roots;
use super::engine_calls::names_dead_callee;
use super::error::stack_capture::FrameSite;
use crate::staged::frame::{CONTEXT_PACKED, CONTEXT_CALLEE, CONTEXT_CALL, CONTEXT_CALLER_REGISTERS, CONTEXT_CALLER_BASE};
use quench_bjit::call_record::LANE_BODY_NOT_ENTERED;
use crate::folded_roots::{FoldedRoots, RootedCode};
use super::jit_backend::relocatable_code::RelocatableCode;
use crate::bytecode::{NESTED_ARGUMENTS, NESTED_CHAIN};
use crate::staged::plan_fields::FieldClass;
use crate::heap::InlineObjectHeader;
use crate::staged::wasm_instance::Storage;
use crate::staged::record_heads::CallContextHead;
use crate::heap::TableWords;
use super::dispatch_frame::PendingGeneralCall;
use crate::typed_ir::Type;
use crate::code_dependencies::Invalidation;
use crate::staged::js_step_head::JsStepHead;
use quench_bjit::call_record::CallRecord;
use super::operations::{NumberArithmetic, NumberBitwise, NumberRelation};
use super::lane_values::LaneValues;
use super::lane_activations::LaneActivations;
use super::activation::FrameRecord;


use crate::compile::aot_facts::AotFacts;
use super::tier_policy::Tiers;

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
    pub call_sites: std::cell::RefCell<rustc_hash::FxHashMap<usize, smallvec::SmallVec<[CallTarget; INLINE_SITE_TARGETS]>>>,
    /// Pairs of a call record's link address and a callee its `call_sites` record holds, direct-mapped
    /// (`noted_slot`): a polymorphic site's steady state re-notes the callees it alternates between, and
    /// a hit here skips the map. Any callee leaving a record empties it.
    pub noted: Box<[std::cell::Cell<Option<(usize, CallTarget)>>; NOTED_SITE_CALLEES]>,
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
    /// the frame's completion carries (`SidePayload`: a skipped forwarder, a
    /// settling promise, a Wasm caller, a trap's invariants), so it pushes and pops with its
    /// frame. Written through `&mut self` and by `spill_plain`, which makes its frames records.
    pub sides: std::cell::UnsafeCell<Vec<Side>>,
    /// The plain headers each record pushed over them sealed (`PlainSegment`),
    /// innermost last: a record's push saves the `plain` words instead of spilling the headers,
    /// and its pop restores them (`LaneStack::seal_plain`, `unseal_plain`).
    pub segments: std::cell::UnsafeCell<Vec<PlainSegment>>,
    /// The headers the segments hold, which `activation_count` counts.
    pub sealed: std::cell::Cell<usize>,
    /// The admission bound emitted code checks native headers against (`JsStepDispatch::plain_depth_limit`):
    /// the activation limit while the frames are bound, else 0. Rust's frame
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
    pub body_table: ZeroedWords,
    /// The descriptor (`DirectCallee`) of each native header, by its total frame index (`native_table`).
    pub native_table: ZeroedWords,
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
    /// cache index modulo `SETTER_SITE_BITS`; empty until the first such call, so a
    /// VM that never meets a setter holds none of it. Only a filter: a clear bit sends the
    /// store to the general path, which calls the setter and sets it.
    pub setter_sites: Vec<u64>,
    /// The callee each in-place native row admits in this generation (`NativeIdentities`).
    pub native_identities: NativeIdentities,
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
    /// The JavaScript-backed import a Wasm call last activated (`WasmActivation::activate_import` in `wasm_calls`),
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
    pub last_window_plan: [Option<(ProgramId, u32, WindowPlan)>; SHARED_PLANS],
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
    /// Whether its records may read a field through the activation's `this` (a `this` site), so a
    /// row of it may trust `this` (`dispatch_fast::note_lane_this`); conservative.
    pub this_rows: bool,
    /// The facts of its body a frameless call target records (`TargetBody`), found on its
    /// first resolution: every closure of the function shares them.
    pub target_body: std::cell::OnceCell<TargetBody>,
    pub _string_case_filters: Box<[StringCaseFilter]>,
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
    /// The record an activation starts at (`LaneView::start`): its first, or `prologue`
    /// while the JavaScript tier may claim the function (`LaneView::claim_for_tier`).
    pub start: std::cell::Cell<Ip>,
    /// The tier prologue (`tier_prologue_in_lane`), outside the records so the function's PCs
    /// stay its records'; written when the tier may claim the function.
    pub prologue: std::cell::UnsafeCell<LaneInstruction>,
    /// The prologue's direct run of the compiled body, once the tier compiled it
    /// (`Vm::direct_tier_entry`).
    pub tier_direct: std::cell::Cell<Option<DirectTierEntry>>,
    /// The lane-body tier's state of this function (`LaneTier`), made when the tier first
    /// looks at it: a view the tier never touches (every view under `--no-jit`, most views
    /// otherwise) keeps one word instead of about 540 bytes.
    pub tier: std::cell::OnceCell<Box<LaneTier>>,
    /// What native callers read to enter its body (`EntryCell`); the one entry word
    /// of the function, published and withdrawn with its body.
    pub entry: EntryCell,
    /// Per record, the register whose value it reads as the carried accumulator
    /// (`NO_ACCUMULATOR` where it reads none): a lane body that leaves at it resumes
    /// with that register carried, as falling through from its producer would.
    pub accumulator_registers: Box<[u16]>,
    pub plain_backward_jump_sites: usize,
    pub osr_rejections: Option<Box<[(RegionRejection, usize)]>>,
    /// The `GetField`, `SetField` and `LoadCapture` records past its sites' loop headers, whose own-field hints a shape compaction clears.
    pub field_records: Box<[u32]>,
    /// The by-name records past its sites' loop headers, whose global-object hints (`name_row`, `global_name_store`) a shape
    /// compaction clears, and a change of the global lexical names they answer past (`LaneView::clear_name_hints`).
    pub name_records: Box<[u32]>,
    /// The held back edges' `IncDec` records whose counter the solve's int32 cells prove, with it (`int_steps`).
    pub proven_counters: Box<[(u32, u16)]>,
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
    pub cache: std::sync::OnceLock<Box<[FramelessCallEntry]>>,
    /// Each function's last admission, by function index, with its program and closure
    /// environment: a fresh closure of it shares all but its callee and captures (`admit`).
    pub functions: Vec<(u32, Value, FramelessCallEntry)>,
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
    /// Each function's entry address, which stitched rows call through (`World`).
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
    pub indirect_hits: SiteHits,
    /// The owner, bytecode PC and table binding of each site `indirect_hits` counts.
    pub reference_cache_sites: Vec<(usize, usize, u32)>,
}

/// Publication owns future entry; active runs and direct callers own code leases.
/// Retirement unlinks entry without releasing code that execution or recovery uses.
/// Guest locator validity and exact exits remain separate obligations.
pub type PublishedLaneMethod = crate::code_dependencies::PublishedCode<LaneMethod>;

/// A program's ahead-of-time image, and its link into this process once a VM loads it.
impl AheadImages {
    /// Whether the image's value table names the closure of `function` (`HeapValueRecipe::ClosureOf`).
    pub fn names_closure(&self, function: u32) -> bool {
        self.image.heap_values.iter().any(|recipe| matches!(recipe, HeapValueRecipe::ClosureOf(found) if *found == function))
    }
}

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
    /// Test counter, kept in every build: quench-runtime's `test-support` feature unifies on in a
    /// workspace check while the engine crate that builds this struct is not `cfg(test)`.
    pub native_entries: std::cell::Cell<u64>,
}

/// What a frame holds beyond its 8-word header (one-frame-one-call.md F4): one entry of
/// `LaneStack::sides` per frame that has one, innermost last, tagged by its header's flags
/// (`frame::SIDE_FLAGS`, `staged::call_complete::side_flags`), or, for a record, by its
/// completion (`LaneStack::record_sided`, which reads only `payload`). A frame finds its own as
/// the top of the side records while it is the innermost; a walk pairs frames with sides
/// innermost first, taking one side per sided frame (`LaneStack::walk_innermost`).
#[derive(Clone, Copy)]
pub struct Side {
    /// The table index (`LaneStack::native_table`, `body_table`) of a linked native header, or
    /// `NO_NATIVE` (one-frame-one-call.md F5).
    pub native: usize,
    /// What its return yields to its call (`frame::COMPLETION_FLAG`, `frame::CROSSES_FLAG`).
    pub completion: Completion,
    /// It keeps the values passed past its registers (`frame::KEEPS_FLAG`, `ScriptActivation::forwards`).
    pub forwards: bool,
    /// The register of the first value it keeps, 0 for none (`ScriptActivation::kept`).
    pub kept: u16,
    /// Its environment and frame locals follow its registers (`frame::ENV_FLAG`).
    pub environment: bool,
    /// The program its caller runs: another than its own where it crosses.
    pub caller_program: ProgramId,
    /// The eval record it owns (`frame::OWNS_FLAG`, `LaneStack::records`), null for none: a
    /// record's `ScriptActivation::record`.
    pub record: *mut crate::vm::activation::FrameRecord,
    /// What its completion carries (one-frame-one-call.md F4 (3)).
    pub payload: SidePayload,
}

/// A compiled body a frameless activation runs directly (`Vm::direct_tier_entry`): the
/// artifact, held weakly so that the tier retiring it ends the direct run,
/// and the `Return` its result completes. The artifact owns the input signature.
pub struct DirectTierEntry {
    pub compiled: std::rc::Weak<CompiledJsFunction>,
    pub return_pc: usize,
    pub return_register: crate::bytecode::Register,
    /// The function the entry runs, whose entries the diagnostics report counts.
    pub function: (ProgramId, u32),
    pub params: u16,
}

/// The store sites a general store has called a setter at, one bit each by the site's cache
/// index modulo the set's width: the lane looks for a setter only at such a site, so a store
/// that meets none pays no lookup. Sites sharing a bit only make that lookup.
/// The set is the lane stack's own (`LaneStack::setter_sites`): a plain load per store, no
/// thread-local access or borrow.
pub const SETTER_SITE_BITS: usize = 1 << 16;

/// Per call site, the targets resolved there, ordered by function index.
pub struct SiteHits(pub RefCell<Vec<Vec<TargetHits>>>);

/// The callee word each in-place native row admits (`staged::call_native::admit_identity`), one
/// slot per row: the function the lane proved, from its record, to be the row's native, `UNPROVEN`
/// before. The proof is a dependency of the lane's generation: a new one (a collection, which may
/// free a function and reuse its id, or another realm: `LaneStack::enter_generation`) forgets
/// every identity, as it retires the script targets, and the next call proves it again.
pub struct NativeIdentities(pub [u64; IDENTITY_SLOTS]);

/// What a frame's completion carries past its header, in its side record, so it pushes and pops
/// with its frame (one-frame-one-call.md F4 (3)); its frame takes it when it materializes
/// (`Vm::materialize_lane_activations`). Exactly the frames whose completion is one of these,
/// a native frame's skipped constructor apart (its descriptor's), carry one.
#[derive(Clone, Copy)]
pub enum SidePayload {
    None,
    /// `Completion::Forwarder`: the forwarding constructor the construction skipped.
    Forwarder(SkippedForwarder),
    /// `Completion::SettlesPromise`: the call's rooted promise, which its return resolves.
    /// `None` for a call whose result only the `await` after it reads
    /// (`ImmediateLayout::LANE_CALL_RESULT_AWAITED_FLAG`): its promise is made only where one
    /// is observable, and a primitive return reaches the `await` as it is.
    Settles(Option<crate::heap::RootId>),
    /// `Completion::ToWasm`: what its return restores of its Wasm caller.
    ToWasm(ToWasmCall),
    /// `Completion::ReadsTrapped` and `AssignsTrapped`: what the trap owes its target's
    /// invariants (`CallCompletion::Trapped`).
    Trapped(crate::vm::activation::TrapCheck),
}

/// The code of `function`: the entry, the trap routine, then the rows, every successor patched;
/// `None` where an instruction has no rows yet. The code is position independent: it calls and
/// jumps only within itself.
/// What the rows of a module's functions name outside the function: its constants' frame words
/// (`None` for one a row cannot hold, a vector), the instance binding of the memory the entry
/// passes (`WasmEntry`'s base and length), whose binding instruction then places nothing, and
/// the address of the cell holding a function's entry, which its direct calls call through
/// (`None` for a function that is not stitched: its direct calls then go through `vm_call`).
pub struct World<'a> {
    pub constant: &'a dyn Fn(usize) -> Option<u64>,
    pub memory_slot: Option<u32>,
    /// Whether the frame's memory keeps its base when it grows (a reservation, or no memory):
    /// rows that can run a growth (`memory.grow`, an indirect call's VM callout) reload only
    /// the length, so over a movable memory they have none.
    pub memory_stable: bool,
    pub callee: &'a dyn Fn(usize) -> Option<u64>,
    /// The defined functions that may grow the memory (`wasm_growth`): a direct call of one that
    /// may not plans `CALL_KEEP`. A function past the set's bound counts as growing.
    pub grows_memory: &'a crate::bitset::BitSet,
    /// The VM callout a `CALL_VM` row makes for a direct call of an unstitched function, as an
    /// address (`wasm_call_vm.rs`'s `vm_call_callout`).
    pub vm_call: u64,
    /// The global a global access at `pc` binds: its environment slot and its storage; `None`
    /// where the plan did not admit the access.
    pub global: &'a dyn Fn(usize) -> Option<(u32, Storage)>,
    /// The byte offset of the context's `globals` pointer (`CONTEXT_GLOBALS`).
    pub globals_offset: u64,
    /// The indirect site of a `Call::Reference` at `pc` (`indirect::site`).
    pub indirect: &'a dyn Fn(usize) -> Option<IndirectSite>,
    /// The table or segment operation's callout, `table_op_callout` as an address: the
    /// stitched code is the program's own, so it names its Host type's callout.
    pub table_callout: u64,
    /// The GC operations' callout, `wasm_gc_callout` as an address (`wasm_gc_callout.rs`):
    /// the stitched code names its Host type's callout, as `table_callout` does.
    pub gc_callout: u64,
    /// The allocating GC steps' callout, `allocate_frame` as an address (`wasm_gc_allocate.rs`).
    pub allocate_callout: u64,
    /// The reference registers a pc's calls hold (`Plan::references`), which the native roots hold
    /// across them (`wasm_stitch_roots.rs`).
    pub references: &'a dyn Fn(usize) -> Vec<u16>,
    /// The operand registers of an allocating GC step at a pc, each with its kind's code
    /// (`Plan::gc_reads`, the kind as `Kind as u64`), in the order `allocating_reads` gives them.
    pub gc_reads: &'a dyn Fn(usize) -> Vec<(u16, u64)>,
    /// The environment slots a table or segment operation at `pc` binds (`TableOp::bindings`:
    /// the first, then the second), from the function's full plan; `None` where the plan did not
    /// reach `pc` or names no first binding.
    pub table_bindings: &'a dyn Fn(usize) -> Option<(Option<u32>, Option<u32>)>,
    /// The function's register count: the scalar frame's, which places the vector area.
    pub registers: u16,
    /// A v128 constant's bytes, `None` for a constant that is not a vector.
    pub vector_constant: &'a dyn Fn(usize) -> Option<[u8; crate::wasm::V128_BYTES]>,
    /// Whether the move at `pc` moves a vector (the u-jit's slot of the move is `Slot::Vector`).
    pub vector_move: &'a dyn Fn(usize) -> bool,
}

/// Plans `shared_window_plan` keeps, one slot per function index modulo this.
pub const SHARED_PLANS: usize = 8;

pub struct StringCaseFilter {
    pub short_lengths: u64,
    pub max_length: usize,
    pub buckets: Box<[Bucket]>,
    pub cases: Box<[Case]>,
}

/// What a function's native callers read to enter its lane body: the entry (the stub where no
/// body is published or it retired) and the body's own recovery space, which it is stored in
/// once, per function, at a stable address (`LaneView::entry_cell`).
///
/// Two 64-byte lines whose every offset is the designed cell's (`staged::call::ENTRY_CELL`, asserted
/// below): `entry` 0, `body` 8, `facts` 16, `validity` 24, `recovery` 32, `counters` 40, `scalar` 48, `rows` 56,
/// `proven` 64, `numbers` 72, `uses_this` 80, `proven_entry` 88, `proven_body` 96.
/// Writers: `entry`, `scalar`, `recovery`, `body`, `rows`, `proven`, `numbers`, `uses_this`, `proven_entry` and `proven_body` by `publish`/`withdraw`;
/// `facts` once per function (`set_facts`); `counters` only on cold paths (completion evidence).
#[repr(C, align(64))]
pub struct EntryCell {
    pub entry: std::cell::Cell<usize>,
    /// The address of the published body (`LaneMethod`), which the view keeps until it drops
    /// (`LaneView::pinned`): a leave inside a call through the cell recovers through it.
    pub body: std::cell::Cell<usize>,
    /// The function's registers, room, parameters and entry kind (`CellFacts`), which a native
    /// call's admission reads.
    pub facts: std::cell::Cell<u64>,
    /// What the body's native callers compare to admit a call (the layout epoch the body was
    /// published under).
    pub validity: std::cell::Cell<u64>,
    /// The address of the published body's `LaneMethod::recovery_space` (`NO_RECOVERY` for the stub).
    pub recovery: std::cell::Cell<usize>,
    /// Leaves (low half) and completions (high half) of this function's body since it
    /// specialized (`EVIDENCE`): the one home of its run statistics, written only on paths that
    /// already do work (a handler's run end, a leave's materialization), never by a call or a return.
    pub counters: std::cell::Cell<u64>,
    /// The address of the published body's scalar entry (`LaneMethod::scalar_address`), which a native
    /// caller whose arguments are proved numbers calls instead of `entry`: it takes the numbers in
    /// registers and pushes no frame. The stub where the body has none or retired.
    pub scalar: std::cell::Cell<usize>,
    /// The address of the published b-jit body's row entry (`LaneMethod::rows_address`), which a
    /// b-jit direct call row calls instead of `entry`; 0 where the body has none or retired.
    pub rows: std::cell::Cell<usize>,
    /// The address of the published b-jit body's proven-arguments row entry (`RowEntries::proven`): `entry_rows` past
    /// its Number guards. A direct row calls it only where `numbers` is covered by the call's proven arguments.
    pub proven: std::cell::Cell<usize>,
    /// The parameter registers whose Number guards `proven` skips (`RowEntries::numbers`).
    pub numbers: std::cell::Cell<u64>,
    /// Whether the body reads its `this` from the context header (`RowEntries::uses_this`).
    pub uses_this: std::cell::Cell<u64>,
    /// The address of the published body's proven entry (`LaneMethod::proven_address`), which a direct call calls
    /// instead of `entry` where its arguments have the entry types the body names (`LaneMethod::entry_param_types`),
    /// and the body it belongs to is the one the call pinned (`proven_body`). Zero where the body has none or retired.
    pub proven_entry: std::cell::Cell<usize>,
    /// The address of the body `proven_entry` belongs to (`Rc::as_ptr`), so a call takes it only for the body whose
    /// parameter types its caller proved (`DirectCallee::body`).
    pub proven_body: std::cell::Cell<usize>,
}

/// What a suspended frame's windowed resumes derive from its function and environment alone,
/// kept on its record (`FrameRecord::window_plan`) while its key holds (`PlanKey`). Each resume
/// of a generator or async body re-derived them. The key and the locals lie flat, their flags
/// together: as a tuple and an option they took each record 88 bytes, flat 72.
#[derive(Clone, Copy)]
pub struct WindowPlan {
    pub collection_epoch: u64,
    pub context_epoch: u64,
    pub key_env: Value,
    pub view: *const LaneView,
    pub parent: Value,
    pub capture: *const usize,
    /// The window's locals (`locals`): their captured-slot mask and own slot storage.
    pub locals_captured: u64,
    pub own_slots: *const crate::heap::EnvironmentSlots,
    pub key_captured: bool,
    pub has_locals: bool,
    pub has_own_slots: bool,
    /// Whether the window carries an environment after the registers (`Vm::window_layout`).
    pub environment: bool,
}

/// The plain headers a record was pushed over (`LaneStack::seal_plain`): the `PlainFrames` words
/// as they stood, which come back when the records count `records` again (`unseal_plain`).
#[derive(Clone, Copy)]
pub struct PlainSegment {
    /// The records below the headers: they lie between record `records - 1` and record `records`.
    pub records: usize,
    pub depth: usize,
    pub registers: *mut Value,
    pub program: ProgramId,
}

/// The script target a call site links, as the unit inlines it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallTarget {
    pub callee: CalleeBinding,
    pub function: u32,
}

/// Why a backward jump was excluded from OSR before native code generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionRejection {
    RegionTooLarge,
    UnsupportedInstruction(u16),
    RegisterLimit,
    CaptureLimit,
    ArrayLimit,
    FieldLimit,
    FieldStoreLimit,
    FieldMutation,
    /// An array the region indexes is a register it defines other than by reading an
    /// element or field or moving one (`LoopRegion::dynamic_arrays`), as a capture's value.
    ArrayRedefined,
}

/// A function's register window as a lane activation starts it: its initial value in the first
/// `fill` registers, then the parameters at their base.
#[derive(Clone, Copy)]
pub struct ActivationShape {
    pub registers: u16,
    pub parameters: u16,
    pub params: u16,
    pub initial: Value,
    /// The registers initialized: the prefix an entry may read before writing
    /// (`RegisterRoots::entry_fill`), or all of them where a collection scans the window whole.
    pub fill: u16,
}

/// A lane body's entry: (context, receiver, capture word, registers) -> (status, result word).
pub type BodyEntry = unsafe extern "C" fn(*mut CallContext, u64, u64, *const u64) -> BodyReturn;

/// The ABI prefix of a call context, with the runtime's own field types (`staged::record_heads`).
pub type CallContextFields = CallContextHead<Value, CompiledProgram, crate::heap::TableWords, ReferenceCallSiteDiagnostics, ProgramId, Option<Raised>>;

/// The return trampoline of the native callbacks pushed as plain activations
/// (`LaneStack::push_callback_header`), as JSC and V8 return into native code through a return
/// thunk. A callback's header names `records[0]` as its call record and `result` as its
/// caller's registers, so the plain return (`return_from_in_lane`) writes the value into `result`
/// and resumes at `records[1]`, whose handler (`callback_trampoline_returned`) ends the run: the
/// plain return makes no test of its own for a callback. `call` and `caller_registers` are the
/// innermost callback's own, which that handler and a spill (`LaneStack::spill_plain`) read.
#[repr(C)]
pub struct CallbackTrampoline {
    pub records: [LaneInstruction; 2],
    /// The array steps' pair (`array_steps::array_step_returned`): an in-lane array step's
    /// callback header names `steps[0]` as its call record and `result` as its caller's
    /// registers, so its plain return resumes at `steps[1]`, which steps the innermost
    /// iteration. Steps nest over plain activations, so their own call record, caller
    /// registers and base live in their entries (`LaneArrayStep`), not here.
    pub steps: [LaneInstruction; 2],
    pub result: std::cell::Cell<Value>,
    pub call: std::cell::Cell<Ip>,
    pub caller_registers: std::cell::Cell<Registers>,
}

/// A compiled body's executable memory in the code zone.
pub struct CodeMemory {
    pub pointer: *mut u8,
    pub length: usize,
    pub block: Block,
}

pub struct CompiledFunction {
    pub entry: WasmEntry,
    pub params: Vec<crate::WasmType>,
    pub result: Option<crate::WasmType>,
    /// Test counter, kept in every build: quench-runtime's `test-support` feature unifies on in a
    /// workspace check while the engine crate that builds this struct is not `cfg(test)`.
    pub native_entries: std::cell::Cell<u64>,
    pub direct_reference_entries: std::cell::Cell<u64>,
}

/// An environment activation's window past its registers (`activate_environment_in_lane`):
/// its environment, a pointer to the environment's slot storage (`WINDOW_SLOTS`), then its
/// `locals`; bit `k` of `captured` is set when local `k` lives in the environment
/// (`Function::local_slot_uses_environment`).
#[derive(Clone, Copy)]
pub struct EnvironmentWindow {
    pub locals: u16,
    pub captured: u64,
    /// Whether the function has a rest parameter (`Function::rest`).
    pub rest: bool,
    /// The local of a named function expression's own name (`Function::self_binding_slot`),
    /// which holds the callee.
    pub self_binding: Option<u16>,
    /// The leading parameters the compiler promoted in place (`Function::parameters_promoted_in_place`):
    /// parameter `k` below this lives in register `k`, which its call fills
    /// (`call_environment::EnvironmentPassed::in_registers`), and its local stays `undefined`.
    pub parameter_registers: u16,
}

/// Entries of the function target memo (`LaneStack::function_targets`), one per function
/// id modulo the count: a closure first called finds its function's target derived when a
/// closure of the same function was resolved before, as each call of a closure-making
/// function makes a new one. A conflict overwrites the entry, which only costs the next
/// closure of the displaced function a full resolution.
pub const FUNCTION_TARGETS: usize = 64;

/// The callees a site's history holds unspilled: sites stay monomorphic (mandreel: 131K, none
/// polymorphic), so one keeps an entry at 32 bytes instead of 112.
pub const INLINE_SITE_TARGETS: usize = 1;

#[repr(C)]
pub struct InstanceContext {
    pub env: Value,
    /// The record itself: what a call context names the running instance by, and a mirror row
    /// names its function's.
    pub instance: *const InstanceContext,
    /// Per environment slot, the address of the native slot of the global bound there (null for
    /// any other binding): the global area. A global is shared by every instance that imports
    /// it, so the area holds its address, which its cell keeps.
    pub globals: *const *mut u64,
    /// Per environment slot, the words that publish the table bound there (null for any other).
    pub table_words: *const *const TableWords,
    /// The code table of the artifact last published (`publish`): per function, its body or null.
    pub table: *const usize,
    pub compiled: *const CompiledProgram,
    pub program: ProgramId,
    pub memory: Value,
    pub memory_length: u64,
    pub guard_start: usize,
    pub guard_end: usize,
    pub call_reference_diagnostic: usize,
    pub reference_call_sites: *const ReferenceCallSiteDiagnostics,
    pub reference_call_site_count: usize,
    pub import_callout_counts: *const std::cell::Cell<u64>,
    pub import_callout_count: usize,
    /// The base of the memory, which a callee that takes one is passed.
    pub memory_base: u64,
    /// The memory arguments of the artifact's bodies (`MemoryArguments::word`), or
    /// `ABI_NOT_DIRECT`.
    pub abi: u64,
    pub global_area: Box<[*mut u64]>,
    pub table_area: Box<[*const TableWords]>,
    /// Every artifact published, kept until the instance dies: a body another instance's code
    /// is running may not be freed by a later publication.
    pub retained: Vec<Rc<CompiledProgram>>,
}

/// A JavaScript-backed import's call facts, fixed by its wrapper (`Cell::WasmHostFunction`):
/// the wrapper and its realm, the JavaScript callee, the scalar kind of each parameter
/// (`JS_IMPORT_I32`, `_F32`, `_F64`) and the result type.
#[derive(Clone, Copy)]
pub struct JsImportPlan {
    pub wrapper: Value,
    pub realm: Value,
    pub callee: Value,
    pub argc: u8,
    pub kinds: [u8; JS_IMPORT_ARGUMENTS],
    pub result: Option<crate::WasmType>,
}

/// An array callback method's iteration a lane runs (`begin_array_iteration_in_lane`), one
/// entry per `Completion::ArrayStep` activation, innermost last: the loop's state and what
/// entering its callback's activation again for the next element reads.
#[derive(Clone, Copy)]
pub struct LaneArrayStep {
    pub iteration: crate::vm::array_indexed::ArrayIteration,
    /// The element the running callback was passed, at `index`: a filter keeps it, a find
    /// answers it.
    pub element: Value,
    pub index: usize,
    pub entry: PreparedEntry,
    /// Whether the iteration is a lane body's call (`body_array_iteration_in_lane`), whose
    /// completion ends the nested run the body made (`array_complete_in_lane`) instead of
    /// continuing the lane at the next record.
    pub to_body: bool,
    /// Where the step's callback is a plain activation (`push_array_step_in_lane`): its header,
    /// whose call record is the array steps' trampoline record (`CallbackTrampoline::step_call`)
    /// and whose caller base word is this entry's index in `LaneStack::steps`, with the
    /// callback's own call record and caller registers. `None` for a step whose callback is
    /// its record (a keeping callback, a lane body's iteration). A spill makes the header the
    /// record either way would push (`LaneStack::plain_frame`).
    pub header: Option<CallbackHeader>,
    /// The caller's records base, which the header's caller base word does not hold.
    pub caller_base: Ip,
}

pub struct LaneMethod {
    /// The slot observations and holder shapes its code relies on (`LaneDependencies`).
    pub dependencies: LaneDependencies,
    /// The Stable facts its code folded, each a dependency its publication takes.
    pub stable: StableFacts,
    /// The word its code reads at its invalidation points, set when it retires.
    pub invalidation: crate::code_dependencies::Invalidation,
    /// Its code, none for a body the system linker placed in the executable (`of_linked_base`).
    pub _code: Option<CodeMemory>,
    pub entry: BodyEntry,
    /// Where its scalar entry lies in the same code (`lane_region::scalar`), which a native call
    /// that proves its numeric arguments calls by the entry cell's `scalar` word.
    pub scalar: Option<usize>,
    /// Where its proven entry lies in the same code, with the entry types of the parameters it skips the guards of
    /// (`LoweredRegion::entry_types`): a direct caller that proves each argument has its type calls it
    /// (`EntryCell::proven_entry`).
    pub proven: Option<(usize, Box<[crate::typed_ir::Type]>)>,
    /// Where its row entries lie in the same code (`entry_rows` and its proven-arguments copy, a b-jit body's), which a
    /// b-jit direct call calls by the entry cell's `rows` and `proven` words; none for a Cranelift body.
    pub rows: Option<RowEntries>,
    pub recovery_space: LaneRecoverySpace,
    /// The lowering requires an owned, resumable poll-page capability.
    pub fault_poll: bool,
    /// Whether its code names shape ids as constants, which a shape compaction renumbers:
    /// the compaction unlinks it (`LaneView::invalidate_shape_bindings`), so no entry checks them.
    pub names_shapes: bool,
    /// The calls it runs inlined (`lane_unit::unit`), by frame.
    pub inlined: Vec<InlinedCall>,
    /// Its leaves, by place (`ExitTable`).
    pub exits: ExitTable,
    /// The scratch registers a leave inside an inlined call fills (`BodyRunContext::scratch`): one
    /// per register of the inlined callees, the unit's past the caller's.
    pub scratch_width: usize,
    /// Whether its inlined frames' registers live in the activation's register file past the
    /// caller's (a stitched body's) and not in the scratch window its code fills: a leave inside
    /// one adopts them from the file (`Vm::adopt_window_in_lane`).
    pub framed: bool,
    pub caller_registers: u16,
    /// The frame slots past `caller_registers` the code writes in the activation's own storage (a b-jit
    /// body's scratch and inlined windows, past the environment window); 0 where it writes none there.
    pub frame_reach: usize,
    /// Whether a call's entry of it pays for itself (`LaneMethod::amortizes_entry`).
    pub amortizes: bool,
    /// The callees it calls by their compiled entries, which its code names.
    pub direct: Box<[DirectCallee]>,
    /// Whether its code calls callees by their entries through an AOT image's call records
    /// (`DirectSite::Proven`, `AotImage::calls`), which name no `direct` callee: its runs bind the
    /// shadow frames those calls push, as a body with `direct` callees does (`Vm::stack_words_in_lane`).
    pub proven_calls: bool,
    /// Whether every call of it that has run is inlined or direct: under a direct caller's native
    /// frames the lane's helper call misses (`dispatch_fast::body_call_helper`), so only such
    /// a body is called directly.
    pub closed: bool,
    /// The call records its code reads (`CallRecord`), by address: the code must not outlive them.
    pub _records: Vec<Box<quench_bjit::call_record::CallRecord>>,
    /// The constructors its `instanceof` rows guard their operands against by identity: a collection that frees one
    /// retires it (`ProgramStore::retire_dead_native_calls`), so a word they name is never a reused one.
    pub constructors: Box<[Value]>,
    /// Whether it runs only in the realm it was compiled in: its entry proves that realm active
    /// (`Heap::Realm`) and every callee it names is that realm's, so a switch to another realm
    /// changes nothing it relies on (`ProgramStore::retire_native_calls_across_realms`).
    pub realm_bound: bool,
    /// Its entry types and its activation's result type (`TypeSummary`), where it is a
    /// whole function's body: a direct caller types the call's result with it (`DirectCallee::summary`).
    /// Read through `summary()`, which a withdrawal ends.
    pub summary: Option<TypeSummary>,
    /// Whether its entry cell has withdrawn it (`EntryCell::withdraw`): a body the cell no longer publishes
    /// types no call, so its summary is not read.
    pub withdrawn: std::cell::Cell<bool>,
}

/// The lane-body tier's per-function state (`LaneView::tier`).
pub struct LaneTier {
    /// The frameless body the prologue runs once the JavaScript tier declined the function (`Vm::compile_lane_method`).
    pub tier_lane: std::cell::UnsafeCell<LaneBody>,
    /// Counts specialization attempts (`specialize_lane_body`); a refused loop compile retries once it moves.
    pub body_epoch: std::cell::Cell<u32>,
    /// Every body it published, kept until the view drops: a retired body's frame leaves through `Activation::body`.
    pub pinned: std::cell::RefCell<Vec<std::rc::Rc<LaneMethod>>>,
    /// What its bodies cost and earned so far (`LaneBodyHistory`).
    pub history: std::cell::Cell<LaneBodyHistory>,
    /// What its lane bodies did, under `QUENCH_TRACE=js-jit` (`LaneBodyStats`).
    pub body_stats: LaneBodyStats,
    /// Bodies it specialized again past (`LaneView::note_lane_body_end`), whose code the
    /// direct callers that name them still run until they retire with them at a shape
    /// compaction.
    pub retired: std::cell::RefCell<Vec<std::rc::Rc<PublishedLaneMethod>>>,
    /// The views whose bodies compiled a call of this function (`LaneView::note_caller`):
    /// they specialize again when its body publishes, to call it by its entry.
    pub callers: std::cell::RefCell<Vec<*const LaneView>>,
}

/// A loop the shared compiler may run natively: the region its backward jump
/// closes, the jump's distance to the header record, and the tier's state for it.
pub struct LoopSite {
    /// The bytecode function whose lane view owns this site. A lane activation may be
    /// frameless, so the VM frame's function is not necessarily the one taking this edge.
    pub function: u32,
    /// Observe backedge work for the JS function tier when OSR rejected this loop. These
    /// sites never attempt native loop code; the shared function compiler owns promotion.
    pub promotion_only: bool,
    /// First analysis failure, when admission did not produce a region.
    pub analysis_failure: Option<RegionAnalysisFailure>,
    /// Static effectful calls that leave native code at their exact PC.
    pub call_boundary_pcs: Box<[u32]>,
    /// The admitted region; a loop whose calls run inlined makes its region once it is hot
    /// (`inlining_region_in_lane`), when its callees are known.
    pub region: std::cell::OnceCell<LoopRegion>,
    /// One guarded-inline attempt for an admitted call-boundary region, before compiling.
    /// Failure retains the original exact-PC effect boundary.
    pub inline_region: std::cell::OnceCell<Option<Box<LoopRegion>>>,
    /// The loop's header and backward jump PCs.
    pub header: u32,
    pub backedge: u32,
    pub jump: isize,
    /// The header record's own handler while the site holds that record
    /// (`loop_header_miss`), which enters the loop's native code wherever control arrives at
    /// the header; null otherwise.
    pub header_handler: std::cell::Cell<*const ()>,
    /// Whether the site may hold its header record (`loop_hold_header_in_lane`): one that is
    /// no call record, whose `call_link` names a call target, and the header of no other
    /// site. The record's `call_link` then names the site.
    pub hookable: bool,
    /// Backward jumps taken in the interpreter, counted up to `needed`, and one past it once the
    /// tier model has admitted the compile (`LoopSite::interprets`).
    pub taken: std::cell::Cell<u32>,
    /// The backward jumps the interpreter takes before the model weighs the compile: the warm-up
    /// first, then what the model finds the compile needs (`loop_model::admission_taken`).
    pub needed: std::cell::Cell<u32>,
    /// Whether the site still enters native code: until its runs and declined entries cost more
    /// than their iterations save (`LoopSite::note_run`).
    pub entering: std::cell::Cell<bool>,
    /// The sites of the function whose loops enclose this one (`install_loop_sites`), set once the
    /// sites are boxed: the same storage the records name the sites by, which lives as long as the lane view.
    pub enclosing: std::cell::OnceCell<Box<[*const LoopSite]>>,
    /// Entries declined in a row since the last native run, and the entries still to pass to the
    /// interpreter before the next attempt (`LoopSite::note_decline`): a decline whose cause
    /// persists costs attempts logarithmic in the iterations, not one per iteration.
    pub declines_in_row: std::cell::Cell<u8>,
    /// The attempts that entered nothing, by reason (`count_decline`, `RegionDecline`).
    pub declines: [std::cell::Cell<u64>; RegionDecline::COUNT],
    pub skipped_entries: std::cell::Cell<u32>,
    /// Whether this VM requested OSR diagnostics when its lane view was derived.
    pub diagnostics: bool,
    /// The thresholds its VM's tier controllers read.
    pub tiers: &'static Tiers,
    pub lane_backedge_count: std::cell::Cell<u64>,
    pub native_entry_attempts: std::cell::Cell<u64>,
    pub native_region_runs: std::cell::Cell<u64>,
    /// Bodies published for this loop (`plan_loop_body`) and runs of them (`run_loop_body`).
    pub tier_ups: std::cell::Cell<u64>,
    pub body_runs: std::cell::Cell<u64>,
    pub entry_guard_misses: std::cell::Cell<u64>,
    pub native_exits: std::cell::Cell<u64>,
    pub widenings: std::cell::Cell<u64>,
    pub compile_attempts: std::cell::Cell<u64>,
    pub compile_successes: std::cell::Cell<u64>,
    pub tier: std::cell::RefCell<LoopTier>,
    /// The function's lane body planned to start at this loop's header (`run_loop_body`).
    pub body: std::cell::RefCell<LoopBody>,
    /// The header dispatches that found the body idle (`plan_loop_body`), each an iteration
    /// of the interpreter's or a region's entry, and the iterations the loop takes before its
    /// body is planned again (`loop_model::body_taken`, at the size its last plan had).
    pub body_seen: std::cell::Cell<u64>,
    /// The header dispatches an idle body still waits for before its plan (`LoopSite::body_waits`).
    pub body_wait: std::cell::Cell<u64>,
    /// Whether a call boundary no region runs made the loop a body's (`loop_body::offer_body`).
    pub body_offered: std::cell::Cell<bool>,
    /// The body epoch (`LaneView::body_epoch`) a refused compile was refused under, and the back
    /// edges still plain under it, and the wait that last refusal set (`LoopSite::refuse_warming`).
    pub refused_epoch: std::cell::Cell<u32>,
    /// The view that refused it: the site lies in that view, so its epoch is read through this pointer.
    pub refused_view: std::cell::Cell<*const LaneView>,
    pub refused_wait: std::cell::Cell<u32>,
    pub refused_span: std::cell::Cell<u32>,
    /// Whether an enclosing admitted loop still enters (`LoopSite::inside_admitted_loop`); false once none does.
    pub enclosed: std::cell::Cell<bool>,
}

/// `LaneStack::noted` entries.
pub const NOTED_SITE_CALLEES: usize = 64;

/// The activations below a callback a deferred native runs in the lane
/// (`Vm::deferred_callback_in_lane`): they are the native's caller's, whose pending calls and
/// executing program are the lane's outer ones, not the callback's.
#[derive(Clone, Copy)]
pub struct NativeBoundary {
    /// The callback's activation: those before it are the caller's.
    pub activation: usize,
    pub pending: *mut Vec<PendingGeneralCall>,
    pub executing: *mut crate::vm::dispatch_frame::ExecutingProgram,
    pub deferred: DeferredLaneCall,
    /// The promise natives running when the callback began (`active_native`): the native
    /// caller's, which its activations' frames move; those the callback starts keep theirs.
    pub natives: usize,
}

/// The verified Number result's machine view in the common native result word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeNumberResult { Int32, Float64 }

/// The plain activations (call protocol v2 stage C): each keeps its frame header in the
/// context words below its window (`Registers::write_plain_header`) and pushes no record.
/// They are always the innermost activations; `LaneStack::activations` makes them records
/// before anything reads the records. Emitted code reads these words at fixed offsets.
#[repr(C)]
pub struct PlainFrames {
    /// How many plain activations sit above the innermost record (offset 0).
    pub depth: std::cell::Cell<usize>,
    /// The innermost plain activation's registers (offset 8).
    pub registers: std::cell::Cell<*mut Value>,
    /// The program the innermost plain activation's caller runs (offset 16), which each push
    /// writes. A header without a side record never crosses, so it runs that program too; a
    /// crossing one's side record keeps its own caller program, which its pop writes back
    /// (`lane_frames::pop_plain_frame`; one-frame-one-call.md F4).
    pub program: std::cell::Cell<ProgramId>,
    /// The activation limit and the values' end as one address (offset 24): a proved call
    /// admits one more plain activation where its window's room ends at or below it
    /// (`LaneStack::plain_room`), one compare for both stacks. With `floor` the values' top at
    /// the first plain activation's push over the records and `allowed = activation_limit -
    /// records`, it is `min(values end, floor + (allowed - 1) * CONTEXT words)`, or 0 where
    /// `allowed` is 0 or nothing refreshed it since the records changed. Each plain window
    /// takes at least `CONTEXT` words above `floor` (`fill_window_in_lane`), so
    /// `depth * CONTEXT <= top - floor`, and `top + room <= bound` gives `depth < allowed`:
    /// never past the limit, though it may refuse below it, where the exact test decides
    /// (`plain_room_exact`). Any change of the records or a move of the values clears it
    /// (`activations_mut`, `spill_plain`), as a change of the limit does
    /// (`refresh_activation_limit`, a due collection); a push at depth 0 refreshes it
    /// (`refresh_plain_bound`). See link-proofs.md.
    pub bound: std::cell::Cell<usize>,
}

/// A call site's memo of its read (`MemoRead`), valid for `callee` in the generation and
/// program it was made in, as `ScriptCallTarget` is.
#[derive(Clone, Copy)]
pub struct ReadMemo {
    /// The call record that made it, whose slot (`CallOperand::memo`) it is.
    pub site: Ip,
    pub callee: Value,
    pub admission: Admission,
    pub this_binding: ThisBinding,
    pub read: MemoRead,
}

/// A user function a lane `Call` may activate without a frame, valid as the general loop's
/// `InLoopCallTarget` is: for `callee` while the collection epoch and realm it was resolved
/// under hold, which its `generation` names. A target serves calls from `program`, the
/// callee's own, or, held apart (`LaneStack::cross_targets`), from any other program, whose
/// calls it only activates, switching the lane to its program (`enter_program_in_lane`).
#[derive(Clone, Copy)]
pub struct ScriptCallTarget {
    pub callee: Value,
    /// The generation it was resolved in and the program it serves (`Admission`).
    pub admission: Admission,
    pub function: u32,
    pub env: Value,
    pub view: *const LaneView,
    pub registers: u16,
    /// The lane-stack room its plain activation reserves (`fill_room::<SCRIPT_FILL_BLOCK>` of
    /// `registers`), derived once.
    pub room: u16,
    pub params: u16,
    /// How a call binds its `this` (`ThisBinding`).
    pub this_binding: ThisBinding,
    /// The `this` a plain call (an `undefined` receiver) binds: an arrow's own
    /// (`ThisBinding::Lexical`), found in its environment when it was resolved, the realm's
    /// global object for a sloppy function, `undefined` for any other binding. A plain call
    /// reads it in one load (`plain_call_activation`).
    pub lexical_this: Value,
    /// `TARGET_CONSTRUCTS` where `new` may activate it too (`Vm::inline_constructible`), and
    /// `TARGET_INITIALIZES` where `new` may run its body as the stores alone
    /// (`Function::only_initializes_this_fields`).
    pub forms: u8,
    /// The registers its plain activation starts `undefined` (`ResidualProgram::entry_fill`),
    /// at most `u8::MAX`: past one `WINDOW_FILL_BLOCK` it starts every register so.
    pub fill: u8,
    /// How a plain call enters it.
    pub entry: CallEntry,
    /// The dense storage header its depth-0 captures read (`LaneContext::capture`), or null where the walk
    /// does not end at a function environment or its storage is sparse. The storage of a live environment never
    /// moves, and the generation retires the target at the next collection.
    pub capture: *const usize,
    /// The `LaneStack::forwarded_methods` index of a constructor body forwarding to a method
    /// of `this` (`Function::forwarded_this_method`), which a construction may activate
    /// directly, or `NO_FORWARDED_METHOD`. An index, not the record, keeps every target's
    /// cache entry its size.
    pub forwarded_method: u16,
}

/// Where the call row's one guard (`staged::call::guard_bound_target`) reads a `ScriptCallTarget`: its
/// callee word and its admission word, and the entry's size, for a tier that addresses the entries itself.
pub const SCRIPT_TARGET_CALLEE_OFFSET: usize = core::mem::offset_of!(ScriptCallTarget, callee);
pub const SCRIPT_TARGET_ADMISSION_OFFSET: usize = core::mem::offset_of!(ScriptCallTarget, admission);
pub const SCRIPT_TARGET_BYTES: usize = core::mem::size_of::<ScriptCallTarget>();

/// A lane run's generation (`LaneStack::generation`), narrow so a cache entry stays one
/// line; its wrap clears every entry, so no stale one can match a reused number.
pub type ScriptGeneration = u32;

/// A script call cache (`LaneStack::script_targets`, `cross_targets`): sets of
/// `SCRIPT_TARGET_WAYS` entries, growing from `SCRIPT_CALL_TARGETS_INITIAL` entries.
/// Entry pointers (`Vm::script_call_entry`) are not held across a resolution, which is
/// where it grows.
pub struct ScriptTargets {
    pub entries: Box<[ScriptCallTarget]>,
    /// Live targets displaced from their set since it last grew.
    pub displaced: usize,
    /// The hash shift of its sets (`shape_memo::direct_mapped_shift`), derived with its size.
    pub set_shift: u32,
}

/// What a function's body fixes about a frameless call target of it
/// (`Vm::frameless_call_target`): its entry apart from a kept `arguments`, and whether `new`
/// may activate it or run it as its stores alone.
#[derive(Clone, Copy)]
pub struct TargetBody {
    pub entry: CallEntry,
    pub constructs: bool,
    pub initializes: bool,
    /// Whether an `Activates` body of a frameless call form runs as a leaf (`leaf_body`), and what its
    /// records then read of the context: only where the tier could not claim it at the first resolution,
    /// which specialized its returns for good. None for a framed call form, whose entries never run a leaf.
    pub leaf: Option<LeafForm>,
}

/// Counters allocated only for opt-in benchmark diagnostics. These runs are
/// intended to explain coverage; their timings are not qualification scores.
pub struct WasmJitDiagnostics {
    pub program: u32,
    pub total_functions: usize,
    pub compiled_functions: usize,
    pub code_bytes: usize,
    pub mapped_bytes: usize,
    pub compile_ns: u128,
    pub native_entry_requests: Vec<std::cell::Cell<u64>>,
    pub interpreter_fallback_entries: Vec<std::cell::Cell<u64>>,
    pub compiled_to_vm_callouts: std::cell::Cell<u64>,
    pub compiled_import_callouts: std::cell::Cell<u64>,
    pub compiled_known_callouts: std::cell::Cell<u64>,
    pub compiled_reference_callouts: std::cell::Cell<u64>,
    pub emscripten_memcpy_fast_path_hits: std::cell::Cell<u64>,
    pub import_callout_names: Vec<(u32, u32, String, String)>,
    pub import_callout_counts: Vec<std::cell::Cell<u64>>,
    pub reference_call_sites: Vec<ReferenceCallSiteDiagnostics>,
}

/// The objects of one activation's `with` chain, kept inline up to the nesting a function usually has.
pub type WithChain = SmallVec<[Value; WITH_CHAIN_INLINE_OBJECTS]>;

/// A generation and a program in one word, so admitting a cached target or memo against the
/// running lane (`LaneStack::admission`) is one compare.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Admission(pub u64);

/// An admitted callee, by its value, in the collection epoch it was admitted in: its native
/// entry, its closure's dense capture slots and whether they take direct writes, its function
/// (low 32 bits), register count and parameter count (the next 16 bits each), and
/// how it binds `this`.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct FramelessCallEntry {
    pub callee: u64,
    pub epoch: u64,
    pub code: u64,
    pub capture_values: u64,
    pub capture_len: u64,
    pub capture_stores: u64,
    pub meta: u64,
    pub this: u64,
    /// Whether `new` may construct with it in place (`Vm::inline_constructible`).
    pub constructs: u64,
}

/// A view's records, with the feedback bytes of its sites in front of them: one byte per record,
/// counted back from the first.
pub struct Records {
    pub cells: Box<[std::cell::UnsafeCell<LaneInstruction>]>,
    /// The records of `cells` before the first record, which hold the feedback bytes.
    pub front: usize,
    /// The first record, kept so each activation's entry reads it with one load (`base`).
    pub first: Ip,
}

/// A b-jit body's row entries in its code, as the cell publishes them: its row entry (`rows`), its proven-arguments
/// row entry (`proven`, past the Number guards `numbers` names) and whether it reads `this` from its context header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowEntries {
    pub rows: usize,
    pub proven: usize,
    pub numbers: u64,
    pub uses_this: bool,
}


/// What a lane body's code relies on beyond its guards, its direct callees' included, once
/// each. A receiver's shape is guarded at each field use (and proved again after any call,
/// `State::call_effect`, unless the VM holds it stable: `StableFacts::shapes`), so an object
/// leaving it falls out at the guard: only its slots' observations are dependencies, and a
/// compaction retires code that names shape ids (`LaneView::invalidate_shape_bindings`). A
/// holder's (prototype's) layout is read without a guard, so its departures are dependencies too.
#[derive(Default)]
pub struct LaneDependencies {
    pub slots: Vec<crate::code_dependencies::SlotDependency>,
    pub departure_shapes: Vec<u32>,
    /// The direct callees' bodies, whose entries the code calls: a change of a Stable fact one of
    /// them folded retires it, and so the code that would still enter it.
    pub callees: Vec<Rc<LaneMethod>>,
}

/// The Stable facts a region's code folded (`typed_ir::BindingTime::Stable`), each the subject
/// of a code dependency its publication takes (`ProgramStore::publish_lane_method`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StableFacts {
    /// Exact shapes of prototypes whose values it holds (`heap::subscribe_prototype_values`).
    pub holders: Box<[u32]>,
    /// The chains it read those values through, whose prototypes' layouts it holds
    /// (`Heap::subscribe_prototype_chain`).
    pub chains: Box<[crate::code_dependencies::PrototypeChain]>,
    /// Atoms it stores without a method-cache test (`MethodCacheAtoms::readers`).
    pub atoms: Box<[Atom]>,
    /// Global cells whose values it holds (`heap::subscribe_global_value`).
    pub globals: Box<[crate::heap::GlobalFact]>,
    /// The heap identities its code names (`Lowering::is_identity`), which its publication keeps alive.
    pub roots: crate::folded_roots::FoldedRoots,
    /// Global objects whose layout it holds (`heap::subscribe_global_layout`).
    pub layouts: Box<[crate::heap::GlobalLayoutFact]>,
    /// Global slots it stores to, or reads the `typeof` of, by the slot's observed type
    /// (`Heap::subscribe_slot_observation`).
    pub slots: Box<[crate::code_dependencies::SlotDependency]>,
    /// Constructors whose `prototype` it holds for an `instanceof` (`Vm::instanceof_fact`).
    pub instanceofs: Box<[crate::code_dependencies::InstanceofFact]>,
    /// Exact shapes of objects whose layout it holds past a call (`Vm::shape_is_stable`): the
    /// first object to leave one retires it (`Heap::subscribe_shape`).
    pub shapes: Box<[u32]>,
    /// The program whose root constant the code folds (`Lowering::root_constant_read`): its
    /// publication subscribes the program's second root run (`ProgramStore::subscribe_root_constants`).
    pub root_program: Option<crate::vm::program_store::ProgramId>,
    /// Whether it holds the typed views it proved past a call (`staged::HeldViews`): the first
    /// change of a live view retires it (`heap::subscribe_typed_views`).
    pub views: bool,
    /// Whether it takes every activation's `this` as a direct object under the lane's `this` trust
    /// (`Vm::this_trust`): retiring the trust retires it (`heap::subscribe_this_trust`).
    pub this_trust: bool,
    /// Live cell keys of the covered functions whose solved register cells it trusts (`lane_region::narrow_to_cells`):
    /// an edge that widens one retires it (`FactWatch::subscribe_cell`).
    pub cells: Box<[crate::typed_ir::FactKey]>,
}

/// A loop analysis failure attributed to the function PC whose requirement blocked it.
/// Limits that apply to the whole region point at its backedge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegionAnalysisFailure {
    pub reason: RegionRejection,
    pub pc: u32,
}

/// A loop's admissible region and the facts its entry needs.
pub struct LoopRegion {
    pub header: u32,
    pub backedge: u32,
    /// The Number registers; a register's index here names its entry definition (`Webs`).
    pub numbers: Vec<Register>,
    /// Registers with a tagged value lifetime; each web owns its representation.
    pub tagged: Vec<Register>,
    /// Per-value roles when an overwritten physical slot carries unrelated types.
    /// The original slots and PCs remain the authority for entry and recovery.
    /// The arrays, in entry-record order. Entry admits one the region writes only when it is
    /// dense (`Vm::dense_array_length`), owns its backing alone, and no prototype of it holds
    /// an index (`Vm::prototypes_hold_no_index`).
    pub arrays: Vec<ArrayUse>,
    /// The depth-0 capture slots `LoadCapture` reads and the region does not store, in
    /// entry-record order.
    pub captures: Vec<u16>,
    /// The depth-0 capture slots the region both reads and stores: no entry snapshot holds
    /// them, so each read takes the lane's own lookup where it runs.
    pub stored_captures: Vec<u16>,
    /// The cached data fields the region reads, in entry-record order: a field read
    /// through another's value follows it (`intern_field`).
    pub fields: Vec<FieldUse>,
    /// The field each `GetField` of the region reads, by offset from the header.
    pub field_reads: std::collections::BTreeMap<u32, usize>,
    /// Reads whose direct register receiver is redefined in this loop. These use a
    /// side-effect-free field-cache callout at the instruction instead of an entry snapshot.
    pub dynamic_field_reads: std::collections::BTreeMap<u32, usize>,
    /// The array registers the region defines: each access reads the array its register
    /// holds then, from the array's record, and only a present
    /// element below the backing's length, which no prototype answers, stays native.
    pub dynamic_arrays: std::collections::BTreeSet<Register>,
    /// Existing own-data Number slots stored by the region, by offset from its header.
    pub field_stores: std::collections::BTreeMap<u32, usize>,
    /// Own-data stores whose receiver the region redefines, by offset from its header: each
    /// stores through the VM's cached store (`OSR_FIELD_STORE`), so every read of the same
    /// name in the region reads the receiver's current slot (`dynamic_field_reads`).
    pub dynamic_field_stores: std::collections::BTreeMap<u32, (crate::bytecode::Atom, u16)>,
    /// Receivers and cache sites whose flat slots are guarded before native entry.
    pub field_store_sites: Vec<FieldStore>,
    /// The registers that hold a field's value at every read (`held_registers`), with the
    /// path they hold and that field's index.
    pub held: std::collections::BTreeMap<Register, (FieldPath, usize)>,
    /// Nullish equality branches against `null` or `undefined` that read an unchanged
    /// tagged register directly from the activation's rooted register file.
    pub identity_branches: std::collections::BTreeMap<u32, IdentityBranch>,
    /// The region's instructions, from the header.
    pub instructions: Vec<WideInstruction>,
    /// The Number constants the region reads, by constant index.
    pub constants: std::collections::BTreeMap<usize, f64>,
    /// The function's live registers before each instruction (`compile::liveness`), when
    /// liveness covers it; for an inlining region, by its own PCs (`Inlining`). A plain
    /// region shares its function's masks (`ResidualProgram::shared_register_root_masks`).
    pub live: Option<std::rc::Rc<crate::compile::liveness::RegisterMasks>>,
    /// Where an inlining region's instructions come from, when the region inlines calls.
    pub inlining: Option<Box<Inlining>>,
    /// Whether its iterations leave natively (at a call no row completes, a throw), so only the stencil tier
    /// takes it, which lowers each instruction: Cranelift never compiles it (`osr_loop::admit_region`).
    pub stencil_only: bool,
}

/// Storage and guest-call allowance proved before an inlined body runs.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct LaneRecoverySpace {
    pub values: usize,
    pub activations: usize,
}

/// What a published lane body's activation returns natively, so a direct caller types the call's
/// result with it (`call_results`): the join of the types its own activation returns, from the facts
/// at each return. A value a leave returns to the caller is not in the result: the caller guards the
/// result at its completion.
#[derive(Clone, Debug, PartialEq)]
pub struct TypeSummary {
    pub result: Type,
}



/// What a lane body returns in two registers: its status and its result word.
#[repr(C)]
pub struct BodyReturn {
    pub status: u32,
    pub value: u64,
}

#[derive(Clone, Copy, Default)]
pub struct Bucket {
    pub key: u64,
    pub first: u32,
    pub count: u16,
}

/// A `PreparedShape`'s callback pushed as a plain activation (`LaneStack::push_callback_header`),
/// made once per shape: the header `push_plain` writes, whose call record and caller registers
/// are the return trampoline's (`CallbackTrampoline::bind`), its program, and the callback's
/// own call record and caller registers, which the trampoline takes at each push.
#[derive(Clone, Copy)]
pub struct CallbackHeader {
    pub plain: crate::staged::protocol::PlainHeader<u64>,
    pub program: u64,
    pub call: Ip,
    pub caller_registers: Registers,
}

/// Binding time of a callee: a published closure, whose identity the call compares, or a
/// static body whose actual closure and environment are admitted at the call. Function
/// bindings are accepted for verified plain bodies with explicit restart or exact-state
/// recovery. A forwarded binding is a closure the site called through a native that
/// forwards its receiver as the callee (`Function.prototype.call`, `Vm::forwarded_call`):
/// the call guards the native's identity too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalleeBinding { Identity(Value), Function, Forwarded { through: Value, callee: Value } }

#[derive(Clone, Copy)]
pub struct Case {
    pub index: u16,
    pub record: u32,
}

/// A callee a lane body calls by its compiled entry (`lane_region`'s direct call): the
/// target the lane would activate, fixed by the callee's identity, which the call guards,
/// and its specialized body. The caller's body owns it.
#[derive(Clone)]
pub struct DirectCallee {
    pub target: ScriptCallTarget,
    /// The callee's published body (`LaneBody::Specialized`): the caller's code calls its entry,
    /// so the caller retires with it, at a shape compaction (`LaneMethod::names_shapes`) or at
    /// a lifecycle edge (`ProgramStore::retire_dead_native_calls`).
    /// `None` where the call runs what the callee's `cell` holds then, as a call of the function itself.
    pub method: Option<Rc<PublishedLaneMethod>>,
    /// Pins the exact entry embedded by the caller, including recovery after retirement.
    pub body: Option<Rc<LaneMethod>>,
    /// The caller's call record, which the callee's activation returns to.
    pub call: usize,
    /// The caller's base instruction pointer (its view's records), which the callee's header saves for the return.
    pub caller_base: usize,
    pub cell: usize,
    pub callee: Value,
    pub registers: u16,
    /// The global object a sloppy callee binds for an `undefined` or `null` receiver, which
    /// binds a direct object itself (`receiver_this_in_lane`); `None` for a strict callee, which
    /// takes any receiver as it is.
    pub sloppy: Option<Value>,
    /// The callee's dense capture slots (`EnvironmentSlots::native_dense_view`).
    pub capture_values: *const Value,
    pub capture_len: usize,
    /// Whether those slots admit every direct store (`dispatch_fast::capture_stores`).
    pub capture_stores: bool,
    /// For a construction site, the forwarding constructor whose activation the call skips:
    /// `callee` is that constructor, which the call guards, and `target` its forwarded
    /// initializer, which runs with the receiver the constructor's plan allocates and
    /// whose result the construction ignores (`construct_keeping_activation_in_lane`).
    pub forwarder: Option<SkippedForwarder>,
    /// The closure of the function the site calls now and its capture word, which a u-jit call reads at run time.
    pub bound: BoundClosure,
}

/// A direct call's closure and capture word (`DirectCallee::bound`): its own `callee` and captures, until the site's
/// helper call resolves another closure of the same function and rebinds them (`direct_callee::rebind_closure`), so a
/// closure made per call of the caller misses once, not per call. `read` says the code reads them (a Cranelift call
/// guarded by them, `call_frames::EmitFrame::rebinds`). A collection unbinds a dead closure (`unbind_dead`): the words
/// then name none, never a reused locator, and the next call misses once and rebinds.
#[derive(Clone, Default, Debug)]
#[repr(C)]
pub struct BoundClosure {
    pub callee: std::cell::Cell<u64>,
    pub captures: std::cell::Cell<u64>,
    pub read: std::cell::Cell<bool>,
}

impl BoundClosure {
    pub fn new(callee: u64, captures: u64) -> Self {
        Self { callee: std::cell::Cell::new(callee), captures: std::cell::Cell::new(captures), read: std::cell::Cell::new(false) }
    }

    /// Unbinds the closure if `live` says it died (`Value::DELETED` names no closure).
    pub fn unbind_dead(&self, live: &impl Fn(Value) -> bool) {
        if self.callee.get() != Value::DELETED.to_bits() && !live(Value::from_bits(self.callee.get())) { self.callee.set(Value::DELETED.to_bits()); }
    }
}

pub use super::fact_edge::{ExitFact, FactMissing};
/// A unit's leaves, each place once, ordered by leave word, over its interned sigs, and per site its crossings
/// (`FactMissing`). Built once per compile and kept by the body it publishes; its tables hold no spare capacity.
#[derive(Default)]
pub struct ExitTable {
    pub sigs: Vec<ExitSig>,
    pub sites: Vec<ExitSite>,
    pub crossings: Box<[std::cell::Cell<u32>]>,
}


/// The identity slots of the in-place native rows: one per builtin row, then one per Math row,
/// then one per lean row.
/// An upper bound the engine checks (`native_calls`): the builtin, Math and lean rows it derives.
pub const IDENTITY_SLOTS: usize = 224;

/// An inlined call. Its registers are the unit's: the caller's are its first, and each
/// callee's start at its `base`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InlinedCall {
    pub recovery: CallRecovery,
    /// The frame whose function makes the call; `None` for the caller's.
    pub parent: Option<usize>,
    /// The PC of the call in its parent's function.
    pub call: u32,
    pub target: CallTarget,
    /// The register holding the callee, guarded at the call.
    pub callee_register: Register,
    /// An OSR region's entry word holding the callee, which the region resolved at compile
    /// time and dropped the caller's read for; the marker reads it there instead of the
    /// register, which the loop may reuse.
    pub callee_entry_word: Option<u32>,
    /// The register holding the call's receiver, the callee's `this`.
    pub receiver: Register,
    /// The call's first argument register.
    pub window: Register,
    pub arguments: u16,
    /// The register the call's completion writes in its parent, dead until then; `None` for a returning call.
    pub result: Option<Register>,
    /// The callee's register 0.
    pub base: Register,
    pub registers: u16,
    /// The unit PC of the call's next alternative's marker, where a callee other than this
    /// frame's goes; `None` for the last, which leaves at the call.
    pub alternative: Option<usize>,
    /// For a call through a forwarding native (`CalleeBinding::Forwarded`), the register
    /// holding the native and its identity, which the call guards too; the callee register
    /// then holds the function it calls, the receiver is the first argument and the window
    /// the rest (`Planner::through_call`).
    pub function_call: Option<(Register, Value)>,
}

/// The most parameters an activated import passes (`JsImportPlan`).
pub const JS_IMPORT_ARGUMENTS: usize = 8;

/// A lane view's frameless body (`LaneView::tier_lane`). Shape compaction
/// moves a body that names shape IDs back to `Unspecialized`.
pub enum LaneBody {
    /// Waiting for its next entry to specialize (a function with no static benefit stays here at its
    /// first entry, and runs in the interpreter); `recompiles` bodies before it left too often for
    /// their feedback (`LaneView::note_lane_body_end`), the last of them, where `stuck`, at those few
    /// leaves only.
    Unspecialized { recompiles: u8, stuck: LaneLeaves },
    /// Its body's code compiling on the code generator's thread; the function runs in the
    /// lane meanwhile, and looks for the code (`Vm::body_work_in_lane`) at entries spaced twice
    /// as far apart after each of its `looks`, up to `TIERS.lane_body.compile_poll_interval`:
    /// a function entered seldom (a hot loop's) finds its code at its next entries, and a hot
    /// one pays few looks. `wait` counts the entries left before the next.
    Compiling { pending: Box<dyn std::any::Any>, wait: std::cell::Cell<u16>, looks: u8, recompiles: u8, stuck: LaneLeaves },
    /// The body its prologue record enters directly (`body_entry_in_lane`); `tier_prologue` is
    /// the handler that record had, which it takes back when the body retires. Its
    /// publication may retire at a lifecycle edge (`ProgramStore::retire_dead_native_calls`),
    /// which the next entry settles (`LaneView::settle_retired_body`).
    Specialized {
        method: std::rc::Rc<PublishedLaneMethod>,
        tier_prologue: *const (),
        /// Its leaves' places (`LaneLeaves`); the counts of its leaves and completed runs since
        /// it specialized live in its entry cell (`EntryCell::evidence`), toward
        /// `TIERS.lane_body.reoptimize_exits` and `TIERS.lane_body.reoptimize_share`.
        leaves_at: std::cell::Cell<LaneLeaves>,
        /// Whether a run of it completed, natively called ones included.
        completed: std::cell::Cell<bool>,
        recompiles: u8,
        /// Its predecessor's few leaves, where it left only there.
        stuck: LaneLeaves,
    },
    Declined,
}

/// A view's lane-body record over the run, which a lifecycle edge keeps: the bodies such
/// edges (a shape compaction, a dead native callee) dropped, and those of them no run of
/// which completed (`LaneView::drop_body`). Each dropped body doubles the next one's warm-up,
/// so a function whose bodies keep being discarded stops paying a compile for each;
/// `self.tiers.lane_body.max_lifetimes` dropped bodies, decline it.
#[derive(Clone, Copy, Default)]
pub struct LaneBodyHistory {
    pub lifetimes: u8,
    /// The inlining budget halvings its last plan needed to fit (`PendingLaneMethod::shrinks`).
    pub shrinks: u8,
    /// The bodies it specialized past because a callee published one (`respecialize_callers`).
    pub callee_recompiles: u8,
    pub late_calls: u32,
    /// Whether a compile of its body waited for its entries to pay for it (`LaneBodyOutcome::Unpaid`).
    pub unpaid: bool,
    /// Whether its body was declined for its length alone (`LaneBodyOutcome::Short`).
    pub short: bool,
    /// Whether a caller's compile asked for its body (`LaneView::demand_body`).
    pub demanded: bool,
    /// Whether a loop region called it while it had no body (`LaneView::note_native_caller`).
    pub region_called: bool,
}

/// A view's lane-body counters, which only a diagnostics run keeps
/// (`ProgramStore::js_jit_diagnostics_enabled`): its function, its compiles and the frames the
/// last inlined, and its runs by how they ended (`LaneExit`), in that order.
#[derive(Default)]
pub struct LaneBodyStats {
    pub function: std::cell::Cell<u32>,
    pub compiles: std::cell::Cell<u32>,
    pub frames: std::cell::Cell<u32>,
    /// The last compile's inlined frames as `index:function<-parent@call-pc`, `+` marking an alternative.
    pub shape: std::cell::RefCell<String>,
    /// Wall-clock milliseconds since the epoch of each compile's publication.
    pub compiled_at: std::cell::RefCell<Vec<u64>>,
    /// Completed, left at a caller PC, inside an inlined callee, in a native call's callee,
    /// or irreversibly terminated.
    pub ends: [std::cell::Cell<u32>; super::tier_events::Event::END_COUNT],
    /// The PCs it left at, and inside inlined frames as `frame:pc`, with their counts.
    pub leaves: std::cell::RefCell<Vec<(String, u32)>>,
    /// Entries of the specialized body that ran in the lane instead (`body_admits_in_lane`).
    pub refused: std::cell::Cell<u32>,
    /// The leaves into a callee that only lacked a body while warming toward one, which count
    /// toward nothing (`dispatch_fast::leave_counts_in_lane`).
    pub warming_leaves: std::cell::Cell<u32>,
    /// Why its body declined (`decline`).
    pub declined: std::cell::Cell<&'static str>,
}

/// What a leaf's records read of the context its call runs them in (`leaf_body`), which is
/// what the call makes its own and its return restores: the forms of its call
/// (`script_call_leaf_prove_miss`), its entry (`CallEntry::leaf`) and its return records
/// (`LaneView::specialize_leaf_returns`). `NOT_LEAF` is a record of no leaf's records.
/// The forms are the staged leaf row's (`staged::call_leaf`), the one definition every tier reads.
pub type LeafForm = crate::staged::call_leaf::LeafForm;

/// What a loop site has of its body.
pub enum LoopBody {
    /// Not yet compiled: where its earlier bodies' leaves went (`stuck`) is the next one's
    /// feedback, over `attempts` bodies.
    Idle { stuck: LaneLeaves, attempts: u8 },
    /// Its code compiling on the code generator's thread; `wait` counts the backedges left
    /// before the next look.
    Compiling { pending: Box<dyn std::any::Any>, wait: u16, stuck: LaneLeaves, attempts: u8 },
    /// `exits` counts its runs that left inside the loop, and `leaves` says where they went.
    /// `runs` counts its runs, those that left inside the loop among them.
    Ready { method: std::rc::Rc<PublishedLaneMethod>, runs: u32, exits: u16, leaves: LaneLeaves, stuck: LaneLeaves, attempts: u8 },
    Declined,
}

/// A loop site's compilations: the current code, if any, the compile under way, the element
/// views both assume (those the latest entry found), and the values earlier code found were
/// no `i32`, which the next compilation holds as `f64`s.
#[derive(Default)]
pub struct LoopTier {
    pub views: ElementViews,
    pub code: Option<Rc<crate::code_dependencies::PublishedCode<CompiledOsrRegion>>>,
    /// The compile under way, and the entries left before the next look for its code
    /// (`Tiers::lane_body`'s `compile_poll_interval`, as a lane body's).
    pub pending: Option<Box<dyn std::any::Any>>,
    pub wait: u16,
    pub compilations: u32,
    /// Native runs of the region and the iterations they took: the loop's mean trip count.
    pub runs: u32,
    pub iterations: u64,
    /// Entries the guards declined: each paid for the entry's guards and ran no iteration.
    pub declined: u32,
    /// Each operation the code left at, with its leaves so far; and those whose int32
    /// speculation the next compilation drops (`lane_region`'s `generic`), sorted.
    pub leaves: Vec<(usize, u16)>,
    pub widened: Vec<usize>,
    /// The element reads among them that left again as Numbers, which the next compilation reads untyped.
    pub abandoned: Vec<usize>,
    /// The field stores the code left at often enough once, which the next compilation planned
    /// again from their sites' caches (`left_store`), sorted.
    pub refed: Vec<usize>,
    /// Entries the published code's guards declined in a row, and whether the next compilation
    /// versions no loop bound on entry (`RegionPlan::entry_bounds`): a loop that appends past its
    /// bound misses every versioned entry.
    pub entry_misses: u16,
    pub unbounded: bool,
    /// Runs in a row that left inside the loop before its first backward edge (`LoopTier::weigh_run`): code that
    /// pays its entry for no iteration, whose site gives up its native code past `widen_after_leaves` of them.
    pub fruitless: u16,
    /// Whether the current code (or the compile under way) is a b-jit region the default mode
    /// picked (`loop_model::base_region`), which moves to u-jit once its iterations repay that
    /// compile (`promote`).
    pub base: bool,
    /// The iterations at which a b-jit region is due to move to u-jit (`loop_model::promotion_taken`),
    /// where the default mode picked b-jit for the region; none where u-jit never gets ahead.
    pub promote_at: Option<u64>,
    /// Whether the site moves from its b-jit region to u-jit (`promote`): the stencil code runs
    /// until the u-jit region publishes in its place.
    pub promoting: bool,
    /// The entries up to which the site is known to repay its native code (`LoopSite::settle`):
    /// the iterations only add, so until the entries pass this count the check is not due.
    pub settle_at: u64,
    /// The current code's native runs and those that left inside the loop: what the u-jit region a
    /// promotion publishes is measured against (`LoopTier::weigh_run`).
    pub tier_runs: u32,
    pub tier_left: u32,
    /// The b-jit region's share of runs that left, per mille, measured when the u-jit region took its
    /// place; none before a promotion publishes.
    pub base_left_permille: Option<u32>,
    /// Whether the u-jit region left more often than the b-jit region it replaced: the site keeps the
    /// b-jit region from then on (`loop_model::base_region` is not asked again).
    pub demoted: bool,
    /// Whether BaseJIT declined the site's b-jit region in the default mode: the site compiles u-jit regions only.
    pub stencil_declined: bool,
}

/// What a call site's read target answered with (`script_call_memo_miss`), so the site's
/// next call of the same callee makes the hits alone: the target's read, or a forwarding
/// body's field and method reads and the read of the method they found. Every part depends
/// on the callee alone, whose body is fixed, and on the shapes its steps name.
#[derive(Clone, Copy)]
pub enum MemoRead {
    Leaf(MemoLeaf),
    Forward {
        /// The forwarding body's field read: the object `base` names, then its method.
        base: u16,
        field: crate::vm::FieldCache,
        method_field: crate::vm::FieldCache,
        /// The forwarding body's parameters, which it passes on in order.
        params: u16,
        /// The method the reads found, a `CallEntry::reads` target, whose `this` is not lexical.
        method: Value,
        read: MemoLeaf,
    },
}

/// What entering an activation of a `PreparedShape` for one receiver reads
/// (`enter_prepared_in_lane`): the shape's window layout and the target's view, environment and
/// captures.
#[derive(Clone, Copy)]
pub struct PreparedEntry {
    pub view: *const LaneView,
    pub env: Value,
    pub capture: *const usize,
    pub this: Value,
    pub registers: usize,
    pub passed: u16,
    pub argc: u16,
    pub keeps: bool,
    pub size: usize,
    pub program: ProgramId,
    pub function: u32,
}

/// An error, or a panic to resume once compiled frames are gone.
pub enum Raised {
    Error(JsError),
    Panic(Box<dyn std::any::Any + Send>),
}

pub struct ReferenceCallSiteDiagnostics {
    pub function: usize,
    pub pc: usize,
    pub counts: std::cell::Cell<ReferenceCallSiteCounts>,
    pub targets: std::cell::RefCell<Vec<ReferenceCallTargetCounts>>,
}

/// Why a loop site's compiled region was not entered at its header on one attempt. Each decline is
/// counted on its site (`LoopSite::declines`) and reported under `QUENCH_TRACE=js-jit`, the first of
/// each reason as a trace line. An entry-contract decline also feeds back as a leave at the header
/// (`lane_loop_run`), so the next compile widens the contract that failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionDecline {
    /// The region's code is still awaited: its compile is under way and its poll waits (`LoopTier::awaits_code`).
    Awaiting,
    /// This attempt started the region's compile, so the interpreter runs the header meanwhile.
    Compiling,
    /// An inlined callee's capture has no environment or no value (`loop_entry_checked_in_lane`).
    CalleeCapture,
    /// A capture the region reads has no initialized value at entry.
    Capture,
    /// A field the region reads has no cached value for its receiver, or is a private name.
    Field,
    /// A dynamic field's entry value is not a Number its representation holds.
    FieldNumber,
    /// The entry record has no room left for a field value.
    EntryFull,
    /// The arrays do not match the region: a non-dense or prototype-indexed written array, a
    /// typed array through a field, mixed views, or no record (`loop_tier::lane_loop_arrays`).
    Array,
    /// A field store's slot address is not admitted (`lane_loop_field_stores`).
    FieldStore,
    /// An inlined call's callee is not activated in place now (`lane_inlined_target`).
    Inlined,
    /// The entry's element views differ from the compiled code's, and no compilation remains (`LoopTier::select`).
    Views,
    /// The site may not compile here: its compilations ran out, or the memory policy compiles no loop regions (`LoopTier::may_run`).
    NoCompile,
    /// The region needs a VM frame the frameless lane lacks (`CompiledOsrRegion::needs_frame`).
    Frame,
    /// The lane has no room for the activations the region's calls would make; it grows for the next entry.
    Room,
    /// The activation keeps its locals in the window past its registers (`lane_context_locals_in_window`).
    Locals,
    /// The code left at its header before any iteration: an entry contract, the entry bounds, the
    /// call budget or the fault guard failed. Fed back as a leave at the header.
    EntryContract,
    /// The region's entry cell held its stub, so its code did not run.
    Stub,
}

/// A forwarding constructor whose activation a construction skipped
/// (`forwarded_construction_in_lane`): what its frame holds when activations become frames,
/// pending at its forwarding call.
#[derive(Clone, Copy)]
pub struct SkippedForwarder {
    pub function: u32,
    pub callee: Value,
    pub env: Value,
    pub method: crate::bytecode::ForwardedThisMethod,
}

/// A target function and how often the callout resolved it at a site.
#[derive(Clone, Copy)]
pub struct TargetHits {
    pub function: u32,
    pub hits: u64,
}

/// How a call of a `ScriptCallTarget` binds `this`, one byte so the entry stays one cache
/// line: the receiver as is (strict), the receiver boxed or the global for nullish
/// (sloppy), or for an arrow its environment's `this` (`ScriptCallTarget::lexical_this`),
/// or the caller's receiver when its environment binds none. An arrow whose binding is
/// still uninitialized (before `super()`) is never resolved, and an initialized binding
/// never changes.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ThisBinding {
    Sloppy,
    Strict,
    Lexical,
    CallerReceiver,
}

/// What a `Completion::ToWasm` activation's return restores of its Wasm caller: the instance
/// it ran over (`LaneContext::instance`), the memory binding its view was of, read again on
/// return since the callee may grow the memory, and the import's result type.
#[derive(Clone, Copy)]
pub struct ToWasmCall {
    pub instance: Value,
    pub memory: Option<Value>,
    pub result: Option<crate::WasmType>,
}

pub const WITH_CHAIN_INLINE_OBJECTS: usize = 2;

/// A compiled Wasm function's entry: (context, memory base, memory length,
/// arguments, result) -> status.
pub type WasmEntry = unsafe extern "C" fn(*mut CallContext, u64, u64, *const u64, *mut u64) -> u32;

/// What a JavaScript activation's return yields to its call: one byte, so a plain return
/// tests it once.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Completion {
    /// The returned value.
    Plain,
    /// The returned value, from an activation of another program than its caller's, whose
    /// return makes the caller's program the lane's again (`enter_program_in_lane`).
    Cross,
    /// A `new` made it: `this` unless it returns an object.
    Constructs,
    /// A `new` of another program's constructor: `Constructs`, whose return makes the
    /// caller's program the lane's again as `Cross` does.
    CrossConstructs,
    /// It runs the method a forwarding constructor's body would have called
    /// (`ScriptCallTarget::forwarded_method`), whose frame its side record names
    /// (`SidePayload::Forwarder`): the construction yields `this`, the receiver, whatever it returns.
    Forwarder,
    /// A native's call of JavaScript run frameless from the lane that called the native
    /// (`Vm::native_callback_in_lane`): its return ends that run with the value, which the
    /// native receives. Made a frame, it is the entry frame of the general loop that finishes
    /// it, which no pending call names.
    ReturnsToNative,
    /// `Generates` of a for-of step whose `done` and `value` reads its records make
    /// (`iterator_step_reads_miss`): a yield or return writes them to those reads' registers and
    /// the caller goes on past them, the result object unmade. Made a frame, it is `Generates`.
    StepReads(ForOfTest),
    /// A plain call of a non-suspending async function of the caller's program
    /// (`CallEntry::framed(Framed::Settles)`): its return resolves the promise the call made, which its side
    /// record keeps (`SidePayload::Settles`), and the call yields that promise. Made a frame, its
    /// pending call settles it (`PendingGeneralCall::completion`).
    SettlesPromise,
    /// A store's setter (`AccessorCall::store_setter`): the assignment yields the value
    /// stored, which its record's register still holds, whatever the setter returns. Made a
    /// frame, its pending call yields it the same way (`CallCompletion::Assigned`).
    Assigns,
    /// A proxy `get` trap (`AccessorCall::read_get_trap`): the read's value, once the
    /// target's invariants have checked it (`SidePayload::Trapped`); a read the target's
    /// changed shape now constrains returns from the trap's frame, whose pending call checks
    /// it in full (`CallCompletion::Trapped`).
    ReadsTrapped,
    /// A proxy `set` trap (`AccessorCall::store_set_trap`): `Assigns` for a trap whose
    /// result the target and a strict store accept, checked as `ReadsTrapped` is.
    AssignsTrapped,
    /// An array callback method's callback the lane runs for one element
    /// (`begin_array_iteration_in_lane`): its return steps the iteration `LaneStack::steps`
    /// keeps (`step_array_iteration_in_lane`). Made a frame, its pending call finishes the
    /// iteration on the native's path (`CallCompletion::ArrayStep`).
    ArrayStep,
    /// A plain call a lane body made of a callee that runs in the lane (`body_calls`): its
    /// return ends that nested run with the value, which the body's call takes. Made a
    /// frame, it is a plain call's.
    ReturnsToBody,
    /// `ReturnsToBody` of a body's `new`. Made a frame, it is a construction's.
    ConstructsToBody,
    /// A Wasm call of a JavaScript-backed import (`WasmActivation::activate_import` in `wasm_calls`), of
    /// another program than its Wasm caller's: its return gives the caller the value's Wasm
    /// form and runs the caller's program and instance again, as its side record keeps them
    /// (`SidePayload::ToWasm`, `lane_complete_to_wasm`). Made a frame, its pending call converts the value
    /// (`CallCompletion::ToWasm`).
    ToWasm,
    /// A generator the lane resumed over its suspended frame's register file
    /// (`generator_windows`): its `yield` suspends it back into the record and its return
    /// completes it, the call yielding the step's iterator result, which only a step's
    /// `done` and `value` reads observe (`Vm::reused_iterator_result`). The activation's
    /// callee is the generator. Made a frame, its pending call completes the step
    /// (`CallCompletion::Generator`).
    Generates,
    /// `Generates` for a `yield*` step of a delegate generator: the step's fresh result goes to
    /// the delegating record's state register, which that record, made again, reads
    /// (`yield_star_in_lane`).
    Delegates,
    /// The async function or async generator a promise job resumed over its suspended frame's
    /// register file, the root of its lane run (`LaneRoot::Job`, `Vm::run_lane_job`): its
    /// `await` suspends it back into its owner (`await_in_lane`) and its return ends the run with
    /// the value, which the job delivers. No call record is its caller's. Made a frame, it is
    /// the entry frame of the general loop that finishes it, which no pending call names. The
    /// activation's callee is its owner.
    ResumesJob,
    /// `Generates` for a `Call` of a generator's `next` (`generator_windows::resume_called_generator`):
    /// the step's fresh iterator result goes to the call's register, as the native's call returns it.
    /// Made a frame, its pending call completes the step with a fresh result
    /// (`CallCompletion::Generator`).
    Steps,
    /// An async generator a `next` call resumed over its record's register file
    /// (`async_requests`): its `await` or `yield` suspends it back into its record, which
    /// awaits, and its return completes it; the call yields the request's promise, which
    /// `LaneStack::settling` keeps. The activation's callee is the generator. Made a frame, its
    /// pending call settles the request (`CallCompletion::AsyncRequest`).
    AsyncRequest,
}

/// The indirect site of a `Call::Reference` (`indirect::site`): its table binding, its type
/// index, that type's canonical word and the artifact's site id.
#[derive(Clone, Copy)]
pub struct IndirectSite {
    pub table: u32,
    pub signature: u32,
    pub canonical: u32,
    pub site: u32,
}

/// Where a body lives: its region and its byte offset there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block {
    pub region: u32,
    pub offset: u32,
}


/// An array a region indexes, by its register; `written` when the region stores into it.
/// The array is the register's value at entry, or, with `field`, the value of that cached
/// field (`LoopRegion::fields`), which the field's one `GetField` in the region writes to
/// the register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArrayUse {
    pub register: Register,
    pub written: bool,
    pub field: Option<usize>,
}

/// How a leave inside a function-bound frame recovers: restart the call, which ran no
/// effect yet, or materialize the callee's exact activation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallRecovery { RestartCall, Materialize }

/// A native OSR region published by the shared compiler. Its lowerer is private to this
/// module; lanes only ask the region to run and inspect its exact-PC widening result.
pub struct CompiledOsrRegion {
    pub code: CodeMemory,
    pub entry: BodyEntry,
    pub facts: OsrFacts,
    pub crossing: Crossing,
    /// Where a run may re-enter the code mid-region (stencil regions only).
    pub resumes: ResumeTable,
    /// What its code owns of its calls: the callees it runs as their own rows, whose registers
    /// follow the activation's (`CallCode`).
    pub calls: CallCode,
    /// The dispatch the region's runs reuse (`Crossing::General`): its last run's, put back as the
    /// run returns, so an entry rebinds the words that change between runs instead of building
    /// the dispatch again (`JsStepDispatch::rebind_region`). A run entered while another of the
    /// region runs finds none here and builds its own.
    pub parked: std::cell::Cell<Option<Box<JsStepDispatch>>>,
    /// Whether the region has parked its dispatch (`parked`).
    pub made: std::cell::Cell<bool>,
    /// Whether the region's instructions call, which decides how its captures bind
    /// (`osr_lane::bind_captures`): found at its first run, not rescanned at each.
    pub region_calls: std::cell::OnceCell<bool>,
    /// The dense capture copy a run of a region that reads its captures from the entry points
    /// the dispatch at (`osr_lane::bind_captures`), kept between runs so an entry fills it in
    /// place instead of allocating it. A run entered while another runs takes an empty one.
    pub dense: std::cell::Cell<Vec<crate::Value>>,
    /// What the parked dispatch was last fully bound for (`EntryStamp`): an entry that finds
    /// the same rebinds only the words a run changes (`JsStepDispatch::rewarm_region`).
    pub stamp: std::cell::Cell<Option<EntryStamp>>,
}

/// Which arrays a region's loops index in place (`LoopArray`): dense arrays' elements, or
/// typed arrays' (`Vm::typed_array_elements_in_lane`). A region assumes one, proved on each
/// loop's entering edge, so no access chooses between them; a static image publishes a
/// typed variant beside the dense one, which its dispatcher tries next at the header, and a
/// loop site compiles the one its entry found (`LoopTier::views`). A typed view carries the
/// kind the site observed, which the proof compares once and the accesses fold; `None` where
/// the entries showed several kinds (or none was observed), the accesses dispatching on the
/// record's.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ElementViews {
    #[default]
    Dense,
    Typed(Option<crate::heap::TypedArrayKind>),
    /// Record arrays whose backings are double (`heap::DOUBLE_LEN`) and hold no hole, each read
    /// and written in place as the raw doubles a `Float64Array` holds (`elements::double_view`):
    /// an element read is a bounds test and one 8-byte load into a Float64 register, a Number
    /// store one 8-byte store. A helper that may change a backing reads it again
    /// (`reload_dense_arrays`), and one no longer double reads as empty.
    Double,
}


/// What a leave is and the values it stores (`Lowering::inlined_stores`, the emitter's own
/// query, which a leave inside an inlined frame stores and no other).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ExitSig {
    pub kind: ExitKind,
    pub live: Box<[LiveValue]>,
}

/// One leave of a unit: its sig, its place, its dense site id, the first unit PC that leaves there, and its missing fact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExitSite {
    /// The leave's word (`LaneExit::word`, read back by `decode`), the table's key; not a `LaneExit`, three words wide.
    pub word: u64,
    pub sig: u32, pub id: u32, pub unit_pc: u32, pub fact: ExitFact,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldStore {
    /// `this`, or a register the region never writes.
    pub receiver: FieldReceiver,
    pub atom: crate::bytecode::Atom,
    pub cache_site: u16,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ForOfTest {
    /// `JumpFalse` to the `value` read, past a `Jump` out of the loop.
    Emitted,
    /// `JumpTrue` out of the loop, the `value` read after it.
    Threaded,
}


#[derive(Clone, Copy)]
pub struct IdentityBranch {
    pub register: Register,
}

/// An inlining region's PCs and frames. Its instructions take consecutive PCs from the
/// header, then each jump out of the region takes one past them (`Origin::Leave`).
pub struct Inlining {
    /// By offset from the header.
    pub origins: Vec<Origin>,
    pub frames: Vec<InlineFrame>,
    /// The captures inlined callees read, by the frame and its depth-0 slot: a callee's
    /// `LoadCapture` reads `CALLEE_CAPTURE` with its index here, which the entry resolves
    /// in the callee's own environment (`loop_entry_in_lane`).
    pub callee_captures: Vec<(usize, u16)>,
    /// The caller's register count: every register from it on is an inlined callee's.
    pub caller_registers: u16,
    /// The caller's backward jump PC: the last caller PC of the loop it closes.
    pub latch: u32,
}

/// Where a specialized body's leaves went: at few exits, each once named, or scattered.
#[derive(Clone, Copy, PartialEq)]
pub enum LaneLeaves {
    Few([Option<LaneExit>; LANE_LEAVE_SITES]),
    Scattered,
}


/// A read a call site's memo makes (`MemoRead`): its field steps as copies of the field
/// cache entries that answered them (`Vm::record_field_entry_hit`), so a step is one shape
/// test. A read of a polymorphic or inherited entry, which probes further, has no memo.
#[derive(Clone, Copy)]
pub enum MemoLeaf {
    /// The field of the object `base` names (`read_base_in_lane`).
    Field { base: u16, field: crate::vm::FieldCache },
    /// That field's array, then its dense element the parameter `index` names.
    Element { base: u16, field: crate::vm::FieldCache, index: u16 },
    /// That field's array, then its `length` (`Vm::array_length_field`).
    Length { base: u16, field: crate::vm::FieldCache },
    /// That field's object, then its own field `second`.
    Nested { base: u16, field: crate::vm::FieldCache, second: crate::vm::FieldCache },
}




#[derive(Clone, Copy, Default)]
pub struct ReferenceCallSiteCounts {
    pub total: u64,
    pub native: u64,
    pub fallback: u64,
    pub same_artifact_direct: u64,
}

#[derive(Clone, Copy, Default)]
pub struct ReferenceCallTargetCounts {
    pub function: usize,
    pub total: u64,
    pub native: u64,
    pub same_artifact_native: u64,
}



/// Where a region's code can be re-entered: the resume thunk and, sorted by PC, each stub.
pub struct ResumeTable {
    pub thunk: u32,
    pub pcs: Box<[u32]>,
    pub offsets: Box<[u32]>,
}

/// What a region's published code owns of its calls: those it runs as their callees' rows (the
/// frames a leave inside one recovers, over registers past its function's own, and the room that
/// recovery needs), the callees it calls by their entries and the records its direct rows read,
/// which its code must not outlive, and the invalidation word its rows read where a call may have
/// changed a fact they fold. A collection that frees a callee it names retires the code
/// (`ProgramStore::retire_dead_native_calls`).
#[derive(Default)]
pub struct CallCode {
    pub inlined: Vec<InlinedCall>,
    pub direct: Box<[DirectCallee]>,
    pub records: Vec<Box<CallRecord>>,
    /// The callees its records were rebound to after their planned closure's guard missed
    /// (`dispatch_fast::rebind_region_call`): each record's descriptor word names one of these, which
    /// stay allocated for the code's life, as a frame running an earlier one keeps reading it.
    pub rebound: std::cell::RefCell<Vec<Box<DirectCallee>>>,
    /// The callee identities its entry checks once (`Held::Captured`): code that names one by a constant, so its
    /// death retires the code. Any other callee a record names is guarded at each call (`names_dead`).
    pub held: Box<[u64]>,
    /// The registers of the function the region belongs to: its inlined callees' follow them.
    pub caller_registers: u16,
    pub recovery: LaneRecoverySpace,
    pub invalidation: Invalidation,
}


/// What a run of a region builds around its code.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Crossing {
    /// A loop region's activation, dispatch and views.
    General,
    /// A stencil region that reads the heap: the activation and dispatch, without the views.
    Lean,
    /// A stencil region that reads no heap row: a payload of the entry record's address only (`run_bare`).
    Bare,
}

/// The activation a region's parked dispatch was bound for: its VM, program and function,
/// its VM frame (by index and by the frame's own address, which a pop and push at the same
/// index changes), the lane context and register file, and whether the program runs
/// specialized. An entry of the same activation (a loop entered again by an outer loop of
/// the same frame, the case region entry costs most in) finds its frame, register and local
/// words, `this` slot and capture view as they were: the run binds only what changes between
/// runs, about a tenth of a full rebind.
#[derive(Clone, Copy)]
pub struct EntryStamp {
    pub vm: usize,
    pub program: ProgramId,
    pub function: u32,
    pub frame: usize,
    pub frame_record: usize,
    /// The frame's locals' storage, which a frame's locals keep while the frame lives.
    pub locals: usize,
    pub lane_context: usize,
    pub registers: usize,
    /// Whether the program runs specialized (fixed for a program).
    pub specialized: bool,
    /// The capture view the window's captures word was last bound to (`captures_key`): an entry
    /// rebinds it only where this changed. Not part of the activation's identity (`binds`).
    pub captures: (usize, u64),
}

/// What a leave is, which a leave word holds in its top two bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u64)]
pub enum ExitKind {
    /// At a caller PC.
    At,
    /// At a PC of an inlined frame's callee.
    Inlined,
    /// Into a native call's callee activation.
    Callee,
    /// At the body's entry, whose proof of its inputs failed.
    Entry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FieldReceiver {
    This,
    /// A register the region never writes.
    Register(Register),
    /// The value of the region's field at this index (`LoopRegion::fields`), an earlier one.
    Field(usize),
    /// The value of this depth-0 capture slot, which no region stores (`receivers::captured`).
    Capture(u16),
}


#[derive(Clone, Copy, Debug)]
pub struct InlineFrame {
    /// The caller's PC of the call.
    pub call: u32,
    /// The function inlined; `guard` verifies its current field or capture at entry.
    pub callee: Value,
    /// The callee's function id.
    pub function: u32,
    /// The caller register the call read the callee from.
    pub callee_register: Register,
    /// The call's first argument register and count.
    pub window: Register,
    pub arguments: u16,
    /// The caller register the call's result lands in.
    pub result: Register,
    pub guard: InlineGuard,
    /// The caller register holding the call's receiver, which the region never writes.
    pub receiver: Register,
    /// The callee's register 0 in the region.
    pub base: Register,
    pub registers: u16,
    /// The callee's parameters bound to caller registers, which hold its values for them.
    pub bound: [Option<Register>; MAX_INLINE_PARAMS],
}

/// Generated code reads this payload by field offset, and an image's code is compiled by a
/// different build than the runtime it links: the layout is declared, not left to the compiler.
#[repr(C)]
pub struct JsStepDispatch {
    /// The word-sized fields generated code reads by offset (`staged::js_step_head`); the stencils take
    /// their `JS_STEP_*` offsets from the same struct, and it is first so its offsets are the payload's.
    pub head: JsStepHead<Value>,
    pub program_id: ProgramId,
    pub frame: usize,
    pub allow_inline_calls: bool,
    pub start_pc: usize,
    pub native_registers: *mut Value,
    pub register_count: usize,
    /// Canonical receiver slot when `this` has already been initialized. Callouts
    /// may move frames or initialize a derived constructor, so refresh this before
    /// returning to generated code. Direct field reads do not cross a safepoint.
    pub fixed_this_slot: *const Value,
    /// Immutable function admitted with `residual` by `JsStepDispatch::with_function`.
    pub body: *const crate::bytecode::Function,
    /// A lane body's scratch window (`BodyRunContext::scratch`), where a leave inside an inlined
    /// callee writes the callee's registers.
    pub inlined: usize,
    /// The lowering line of the guard a lane body last left at, where its code was compiled
    /// for diagnostics (`VmFacts::diagnostics`); zero otherwise.
    pub exit_line: u64,
    /// The element read past a present dense element (`general_elements`).
    pub element_get: usize,
    /// The VM's frameless call admissions (`frameless`), valid in the heap's collection
    /// `epoch`, and the admission a site whose entry misses asks.
    pub call_cache: usize,
    pub layout_seen: u64, // `Heap::native_layout_epoch` at the last reread of the bases (`refresh_object_bases`); `u64::MAX`: never
    /// The VM whose bases the last reread read (`refresh_object_bases`), apart from `vm`, the VM whose storage the
    /// dispatch is bound to (`bind_lane_storage`): a region's dispatch refreshes its bases without that binding.
    pub bases_vm: usize,
    pub frameless: usize,
    /// The VM's in-loop call targets (`Vm::in_loop_call_target`), which an inlined body's
    /// binding probes before its helper; an empty table where an eval binds no callee.
    pub call_targets: usize,
    /// Whether the host makes a strict function's terminal calls proper tail calls
    /// (`Host::proper_tail_calls`), which leaves its returning calls to their exact step.
    pub proper_tail_calls: u64,
    pub outcome: Option<JsNativeStep>,
    /// Past every word generated code reads: a feature's field leaves their offsets as they are.
    #[cfg(feature = "native-execution")]
    pub stop_pc: usize,
}

/// The most distinct places a body's leaves may go for them to be its specialization's
/// feedback; past them they are scattered.
pub const LANE_LEAVE_SITES: usize = 4;

/// Where a lane body leaves: at a PC of its own function, or at a PC of an inlined callee
/// whose activation the lane then makes (`Vm::body_inlined_activation_in_lane`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneExit {
    At(usize),
    Inlined { frame: usize, pc: u32 },
    Terminated,
    /// A native call made its callee's activation and the lane continues in it
    /// (`dispatch_fast::body_call_helper`).
    Callee,
    /// The activation completes with `value`, by the `Return` or the returning call at `pc`.
    Returned { pc: usize, value: Value },
    /// The activation ends by the exception in `Vm::lane_raised`, raised at the record `pc`,
    /// which no native handler of its function took: the lane unwinds from there.
    Threw { pc: usize },
    /// The entry's proof of its inputs failed before any instruction ran: the activation
    /// resumes at its first record, and a plan again proves no input contract.
    Entry,
    /// The call went through an entry cell that held the stub, so no body ran: the caller
    /// takes its general path at its own first record (`LANE_BODY_NOT_ENTERED`). Not a leave of
    /// any body: a loop or OSR root, whose entry proves its own inputs, never reads it as one.
    Stub,
}

/// The registers the interpreter may read resuming at each row, one set per row, flat.
pub struct Live {
    pub words: usize,
    pub bits: Vec<u64>,
}

/// What a region's code relies on beyond its entry guards: the exits that widen a value,
/// and the facts its own-field descriptors embed, which retire it when they change.
pub struct OsrFacts {
    /// Shape IDs the code embeds, the receiver's included. Shape compaction retires this code
    /// through its lane view before execution can resume with remapped IDs.
    pub shape_dependencies: Vec<u32>,
    /// Holder shapes: their layout is read without the receiver-shape guard, so their
    /// departure discards the compile.
    pub departure_shapes: Vec<u32>,
    pub slot_dependencies: Vec<crate::code_dependencies::SlotDependency>,
    /// The heap identities the code names (a call's callee), which its publication keeps alive.
    pub roots: crate::folded_roots::FoldedRoots,
    /// The Stable facts a BaseJIT region folded: atoms its stores run in place for, which no method
    /// cache names (a cache naming one retires it, `MethodCacheAtoms::name`), and the global cells
    /// that hold the callees it inlines.
    pub stable: StableFacts,
    /// What the callees its calls name depend on (`LaneDependencies::of`).
    pub calls: LaneDependencies,
}


/// A call an inlining region runs in place (`inline_region`): the callee's instructions in
/// the caller's region, its registers above the caller's from `base`, its numeric
/// parameters the call's arguments and the others the caller registers they were moved
/// from, its receiver the caller's.
#[derive(Clone, Copy, Debug)]
pub enum InlineGuard {
    Field(usize),
    /// The callee's read at this region offset, kept in the region because its receiver
    /// changes in the loop: native code compares its value with the callee right after it,
    /// leaving before the read where it differs.
    AtRead,
    /// A depth-zero plain capture, checked by the VM at every native entry.
    Capture(u16),
    /// A caller register the loop never writes, live at its header (`const f = …; for (…) f(…)`),
    /// checked by the VM at every native entry.
    Register(Register),
}

/// The instruction transition that returns from native code to the ordinary frame loop. Its `StepResult` is already executed,
/// so the dispatcher consumes it instead of replaying the bytecode.
pub struct JsNativeStep {
    pub instruction_pc: usize,
    #[cfg_attr(not(feature = "native-execution"), allow(dead_code))]
    pub next_pc: usize,
    pub result: Result<crate::vm::StepResult, JsError>,
}


/// The parameters an inlined callee may have, each bound or numeric (`InlineFrame::bound`).
pub const MAX_INLINE_PARAMS: usize = 8;

/// The natives a `Call` site calls through the lean native row (`lane_call_in_place`), one
/// instantiation per row: the native is a constant of the handler, so the site's whole protocol is
/// the identity compare, the native's in-place body reading the call's window where it lies, and
/// the store of its value. No `native_call_miss` dispatch on the callee's record, no
/// `native_value_in_lane` match, no activation, root or stack guard: an in-place body runs no guest
/// code, raises nothing and observes no frame (`Vm::in_place_native_value`). Any other callee or
/// case takes `native_call_miss`, which makes the same in-place attempt first.
///
/// The rows are derived from the catalog, not listed: every native with an in-place family
/// (`in_place_natives::runs_no_guest_code`, the fact `IN_PLACE_FAMILIES` keeps per native) gets
/// one, in discriminant order, so a native that gains an in-place body gains its row.
pub const MAX_IN_PLACE_ROWS: usize = 256;



/// Where an inlining region's PC comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// The caller's instruction at this PC.
    Caller(u32),
    /// The instruction at `pc` of `frame`'s callee.
    Inlined { frame: u16, pc: u32 },
    /// No instruction: a jump leaving the region, which the caller continues at this PC.
    Leave(u32),
}



impl RegionDecline {

    /// Every decline, in the order the report lists them.
    pub const ALL: [Self; 17] = [
        Self::Awaiting, Self::Compiling, Self::CalleeCapture, Self::Capture, Self::Field, Self::FieldNumber,
        Self::EntryFull, Self::Array, Self::FieldStore, Self::Inlined, Self::Views, Self::NoCompile,
        Self::Frame, Self::Room, Self::Locals, Self::EntryContract, Self::Stub,
    ];
    /// The number of declines.
    pub const COUNT: usize = Self::ALL.len();

    pub fn index(self) -> usize {
        self as usize
    }

    /// The name the report and the trace give this decline.
    pub fn name(self) -> &'static str {
        match self {
            Self::Awaiting => "awaiting",
            Self::Compiling => "compiling",
            Self::CalleeCapture => "callee-capture",
            Self::Capture => "capture",
            Self::Field => "field",
            Self::FieldNumber => "field-number",
            Self::EntryFull => "entry-full",
            Self::Array => "array",
            Self::FieldStore => "field-store",
            Self::Inlined => "inlined",
            Self::Views => "views",
            Self::NoCompile => "no-compile",
            Self::Frame => "frame",
            Self::Room => "room",
            Self::Locals => "locals",
            Self::EntryContract => "entry-contract",
            Self::Stub => "stub",
        }
    }
}




/// One operand of a word node: an earlier word node's words (its position), a scalar, or a
/// column another operation wrote.
#[derive(Clone, Copy)]
pub enum WordSource {
    Word(usize),
    Scalar(u8),
    Column(u16),
}

/// The active frame's registers. Lane instructions never resize registers or
/// change the frame stack, so the base stays valid for the whole lane run.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Registers(pub *mut Value);

/// Entries of the construct plan cache, indexed by the callee's heap index.
pub const CONSTRUCT_PLANS: usize = 64;

/// What a constructor's initializer body (`ScriptCallTarget::initializes`) made of a receiver
/// whose adds all hit: the callee's receiver template under `prototype`, which the callee and
/// that prototype alone determine, ends at `shape`, its fields the arguments for
/// `parameters[..fields]` in order, a defaulted one's fallback in place of a falsy argument.
/// While the plan's dependency holds (`watch_construct_plan`: the values and layouts of the
/// objects it read and of the chains above them), those adds would hit again with the same
/// transitions, so the object can be allocated finished.
#[derive(Clone, Copy)]
pub struct ConstructPlan {
    pub callee: Value,
    /// The callee's own layout that fixes where its `prototype` lives
    /// (`Vm::function_prototype_layout`), which a construct reads it through.
    pub callee_layout: (u32, usize),
    pub prototype: Value,
    pub shape: u32,
    /// The receiver's header (`ConstructPlan::derive_header`): of inline storage when its
    /// fields fit the inline slots, else of an arena run.
    pub header: InlineObjectHeader,
    pub kind: ConstructPlanKind,
    pub fields: u8,
    pub parameters: [u16; CONSTRUCT_PLAN_FIELDS],
    /// Fields whose store defaults a falsy argument (`InitializerStore::fallback`), one bit
    /// per field index, each taking `fallbacks` at its index then.
    pub defaulted: u8,
    /// Fields whose store writes a constant (`InitializerValue::Constant`), one bit per field
    /// index, each taking `fallbacks` at its index whatever is passed.
    pub constant: u8,
    /// Fields whose store computes a Number from its parameter (`InitializerValue::Numeric`),
    /// one bit per field index, each with `operators` and the constant in `fallbacks` at it.
    pub computed: u8,
    /// Each field's class, derived from the masks with the header (`derive_header`), so a
    /// construction walks one dense array.
    pub classes: [FieldClass; CONSTRUCT_PLAN_FIELDS],
    pub operators: [u8; CONSTRUCT_PLAN_FIELDS],
    pub fallbacks: [Value; CONSTRUCT_PLAN_FIELDS],
    /// For a forwarding constructor's plan, where its body's reads resolved
    /// (`forwarded_construction_in_lane`), which the plan's own proof keeps. Its fields, if any, are
    /// those its forwarded initializer stores (`record_forwarded_fields_in_lane`).
    pub forwarded: Option<ForwardedResolution>,
    /// The field a nested construction gives a fresh object each time (`NestedField`).
    pub nested: Option<NestedField>,
    /// For a class whose constructor only constructs its parent (`super_construction`),
    /// the parent whose stores the plan holds; `undefined` for any other plan. The plan holds
    /// while the class's [[Prototype]] is still that parent, which `super(...)` constructs.
    pub super_parent: Value,
}

/// What a `GetField` reads: named fields one after another from a root receiver (`this` or
/// a register), each step with the cache site that answers it.
#[derive(Clone, Debug)]
pub struct FieldPath {
    pub root: FieldReceiver,
    pub steps: Vec<(crate::bytecode::Atom, u16)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldUse {
    pub receiver: FieldReceiver,
    pub atom: crate::bytecode::Atom,
    pub cache_site: u16,
    pub tagged: bool,
}


pub type Ip = *const LaneInstruction;

/// One instruction of a lane view. The first 16 bytes hold what every dispatch reads: the handler
/// and the three register fields, with the call link filling the word. Two records share a 64-byte
/// cache line. The operand and the fused `mode` word follow.
#[repr(C)]
pub struct LaneInstruction {
    /// The opcode's [`Handler`] for the VM that built the view.
    pub handler: *const (),
    pub a: u16,
    pub b: u16,
    pub c: u16,
    /// A call record's hint of the script call target cache entry that answered it last
    /// (`Vm::linked_script_call_entry`), which the record's next call tests first; `NO_CALL_LINK`
    /// for none. It fills the record's word, so every record keeps its size.
    pub call_link: std::cell::Cell<u16>,
    pub operand: LaneOperand,
    /// The runtime settings of a fused record (`staged::MODE_DEAD_FIRST`, `MODE_BACKWARD`, and the raw
    /// mask from `staged::RAW_SHIFT`), written by the fusion passes with its handler and read by the
    /// handler at run time. Zero for every other record, whose handlers ignore it.
    pub mode: u64,
}

/// A value a leave stores: its unit register and where the leave stores it. Every stored
/// value is stored tagged, so no representation is recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LiveValue {
    pub slot: u16,
    pub source: Source,
}


/// Where a live value is when its leave happens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    /// An inlined callee's register, which the leave stores to this slot of the entry's
    /// scratch window (`BodyRunContext::scratch`).
    Scratch(u16),
}

/// Fields a `ConstructPlan` names; an initializer storing more keeps running its adds.
pub const CONSTRUCT_PLAN_FIELDS: usize = 8;

/// Which constructions allocate from a `ConstructPlan`, and how they compute its fields.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ConstructPlanKind {
    /// An initializer constructor's plan (`initialize_fields_in_lane`) whose fields are the
    /// arguments as passed.
    Passed,
    /// An initializer constructor's plan with defaulted or computed fields
    /// (`ConstructPlan::defaulted`, `ConstructPlan::computed`).
    Defaulted,
    /// A forwarding constructor's plan (`planned_receiver_in_lane`), which holds only with its
    /// forwarding (`forwarded_fields_in_lane`).
    Forwarding,
}

/// Where a forwarding constructor's reads of `this.<method>` and its `apply` resolved under
/// a construct plan (`ConstructPlan::forwarded`). The plan's dependency covers the plan's
/// prototype (the method's slot value), the method (its shape and prototype, which could
/// shadow `apply`) and the holder of `apply`, so while the plan holds `method` is what the
/// forwarding reads and `apply` is the intrinsic: a use reads them from here.
#[derive(Clone, Copy)]
pub struct ForwardedResolution {
    pub method: Value,
    pub apply_holder: Value,
    /// The forwarded method's function whose initializer stores the plan's fields record
    /// (`record_forwarded_fields_in_lane`), when it has fields: the method this resolution
    /// reads must still be of it.
    pub initializer: Option<(ProgramId, u32)>,
}

/// The immediate, resolved by its role: a jump target becomes the byte
/// distance from the record, a constant index becomes the constant.
#[derive(Clone, Copy)]
pub union LaneOperand {
    pub imm: u32,
    /// Immutable candidates for a string selector, owned by the lane view.
    pub string_case_filter: *const StringCaseFilter,
    /// A `GetField` record's immediate and the receiver shape of its own-field hint
    /// (`LaneInstruction::field_hint`).
    pub field: FieldOperand,
    /// A call record's immediate and its read memo's slot (`CallOperand::memo`).
    pub call: CallOperand,
    pub jump: isize,
    pub constant: Value,
}

/// A field of a plan that takes a fresh object of `constructor`, constructed from the
/// constants `arguments` by its own plan (`InitializerValue::Construct`). The plan's
/// dependency covers the objects the constructor was read through (`held`, the first `depth`),
/// so the constructor holds while the plan does.
#[derive(Clone, Copy)]
pub struct NestedField {
    pub field: u8,
    pub constructor: Value,
    pub arguments: [Value; NESTED_ARGUMENTS],
    /// Where its chain starts: the capture (a `LoadCapture` immediate) of the initializer's
    /// closure environment `env`, holding `root` (`nested_root_holds`).
    pub env: Value,
    pub capture: u32,
    pub root: Value,
    pub held: [Value; NESTED_CHAIN],
    pub depth: u8,
    /// Whether the field takes a fresh object of `constructor`; else it takes `constructor`
    /// itself, the value its chain read (`NestedConstruct::constructs`), which the plan keeps
    /// as the field's constant.
    pub constructs: bool,
}

/// A call record's operand: its immediate, and the slot of `LaneStack::read_memos` its read
/// memo was made in (`memoize_read_in_lane`), `NO_READ_MEMO` before its first. The slot is the
/// site's alone until the table refills (`READ_MEMOS`), so which sites keep a memo does not
/// depend on where their records lie.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct CallOperand {
    pub imm: u32,
    pub memo: u32,
}

/// A `GetField` record's operand: its immediate, and the exact receiver shape its own-field
/// hint names, `NO_FIELD_RECEIVER` for none. The hint's slot is the record's `call_link`,
/// which no field record reads otherwise (`holds_no_call_link`). An inherited read's record
/// hints the inherited entry that last answered it instead (`set_inherited_hint`): its shape
/// is then `INHERITED_HINT` and its `call_link` that entry's index; a polymorphic read's, the
/// prototype-held read that last answered it (`set_holder_hint`), `HOLDER_HINT` and its index.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct FieldOperand {
    pub imm: u32,
    pub shape: u32,
}

/// A program's AOT image: what all three linkers consume.
pub struct AotImage {
    /// The program the image is valid for (`ahead_images::fingerprint`) and the build it came from.
    pub fingerprint: u64,
    pub build_id: u64,
    /// How closed the program is (`aot_facts::world`) as its compile found: its facts derive
    /// again from this (`aot_facts::analyze_as`).
    pub world: crate::compile::aot_facts::World,
    /// By function id, ascending; every function of the program has one.
    pub functions: Vec<AotFunction>,
    /// The shapes the code folds, by `Symbol::Shape` key.
    pub shapes: Vec<ShapeRecipe>,
    /// The heap values the code folds, by `Symbol::HeapValue` key.
    pub heap_values: Vec<HeapValueRecipe>,
    /// Atom texts by `Symbol::Atom` key (atom numbers are per VM).
    pub atoms: Vec<Box<str>>,
    /// The `(function, pc)` labels of `Symbol::BasePc` keys.
    pub labels: Vec<(u32, u32)>,
    /// The calls the closed world proves that bodies make by their callee's entry, by `Symbol::CallRecord` key.
    pub calls: Vec<CallSite>,
    /// A Wasm program's stitched rows, which the instance's Wasm tier links in place of stitching.
    pub wasm: Option<WasmRows>,
    /// The native searches of the program's RegExp literals (`aot_image::regexps`).
    pub regexps: Vec<AotRegexp>,
}

/// A RegExp literal's native search in an image: its pattern's key (`quench_regexp::backtrack::native::AheadKey`)
/// and its code, which names no address (`regexp_native::compile`), so it runs wherever it is placed.
pub struct AotRegexp {
    pub key: quench_regexp::backtrack::native::AheadKey,
    pub code: Box<[u8]>,
}

/// A Wasm program's rows in its image (`compiled::wasm_image`): the memory slot their entry reads
/// (`WasmEntry`'s base and length), and by function index its rows, `None` where it has none. A row's
/// absolute words name run-time addresses by relocation: only `Symbol::EntryCell`, a callee's entry cell.
pub struct WasmRows {
    pub memory_slot: Option<u32>,
    pub functions: Vec<Option<RelocatableCode>>,
}

/// An image linked into this process: its code placed and every symbol resolved.
pub struct LiveImage {
    /// Every base and lane body, placed once; the addresses below point into it. None for an image
    /// the system linker placed in the executable (`linked`).
    pub _code: Option<CodeMemory>,
    /// The image's one invalidation word, which every body's invalidation points read; it lives
    /// as long as the code, and every lane body published from the image retires through it
    /// (`Vm::install_ahead_body`), so a broken fact of any one stops every fold.
    pub invalidation: Invalidation,
    /// The records the direct rows of proven calls read in place, and their bindings; they live as
    /// long as the code.
    pub calls: BoundCalls,
    /// Each function's entry: its lane body where it has one, else its base body.
    pub entries: Box<[usize]>,
    /// Each function's base body.
    pub bases: Box<[usize]>,
    /// Each function's lane body, where it has one.
    pub lanes: Box<[Option<usize>]>,
    /// Each lane helper's address by number, as the system linker bound the helpers the image names
    /// (0 for the rest); `None` for an image placed in process, whose helpers are the process's table.
    pub helpers: Option<&'static [usize]>,
}

/// One function of the image.
pub struct AotFunction {
    pub id: u32,
    /// The base body: every record lowered (a stencil row, or the generic semantic row).
    pub base: BaseImage,
    /// The lane body, where the facts gave it native records; its leaves enter `base`.
    pub lane: Option<LaneImage>,
}

/// The image's call records, by `Symbol::CallRecord` key, with what their bindings keep alive.
pub struct BoundCalls {
    /// The records the direct rows read in place.
    pub records: BoundRecords,
    /// Each record's key by its site, sorted: the VM's call of a site finds its record here.
    pub sites: Box<[((u32, u32, u32), usize)]>,
    pub held: Rc<Held>,
    /// Whether the heap roots `held` (`Heap::hold_folded_roots`), which the first binding asks.
    pub rooted: std::cell::Cell<bool>,
}

/// How the loader finds a heap value the code folded, in the VM that runs the image.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum HeapValueRecipe {
    /// The realm intrinsic numbered `id` (`%Array.prototype%` and the like): `Native::ALL[id]`.
    Intrinsic(u32),
    /// The value of the global binding named by image atom `atom` once the realm is initialized.
    Global(u32),
    /// A string whose text is image atom `atom`: a string constant a lane body loads, allocated
    /// once by the loader (its code roots it), as a frozen image materializes its constant block.
    String(u32),
    /// The `prototype` object of function `function`'s closure in the root activation.
    PrototypeOf(u32),
    /// Function `function`'s closure, where the running program made exactly one: the callee a
    /// construction the closed world proves (`AotFacts::construct_target`) guards by identity.
    ClosureOf(u32),
}

/// How the loader builds a shape the code folded: the prototype it has, the constructor whose
/// construction made the object (`Vm::constructed_root_shape`), and its own fields in insertion
/// order, each by image atom key and attribute bits, interned in the VM's shape tree.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ShapeRecipe {
    /// The `HeapValueRecipe` key of the prototype, or `None` for a null prototype.
    pub prototype: Option<u32>,
    /// The `HeapValueRecipe` key of the closure `new` constructed the object by, or `None` for an
    /// object no construction made.
    pub constructor: Option<u32>,
    pub fields: Box<[(u32, u8)]>,
}

/// A call the closed world proves (`AotFacts::call_target`) that a body makes by its callee's entry:
/// its site, its callee, and the record its direct row reads, as the plan made it. The record's
/// run-time words (the callee's identity, cell, descriptor, captures and global) are a closure's,
/// which the program makes as it runs: the image's record names no callee (`unbound_callee`), so the
/// row refuses every call to the helper until the loader binds the site to the closure it resolved
/// (`BoundCalls::bind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallSite {
    pub function: u32,
    pub pc: u32,
    pub callee: u32,
    pub record: quench_bjit::call_record::CallRecord,
}

/// A base body: code, relocations, and where each record starts.
pub struct BaseImage {
    pub code: RelocatableCode,
    /// Byte offset in `code` of record `pc`'s first stencil; `NO_ENTRY` where no control
    /// enters it from outside the body (so no lane leave may target it).
    pub pc_offsets: Box<[u32]>,
    /// The records lowered as the generic semantic row, ascending.
    pub generic: Box<[u32]>,
    /// The registers past the function's own its rows reach (`ImageBody::scratch`).
    pub scratch: u32,
}

/// What the bindings keep: every closure bound, and by key every descriptor the record named.
pub struct Held {
    pub callees: RefCell<FoldedRoots>,
    pub descriptors: Box<[RefCell<Vec<Box<DirectCallee>>>]>,
}

/// A lane body compiled ahead: its code, and what its lowering proved about the entry ABI.
pub struct LaneImage {
    pub code: RelocatableCode,
    /// Whether its back edges poll by fault (`LoweredRegion::fault_poll`): the runner maps the poll page
    /// for it, as for a JIT lane body, and a fault lands at the entry's termination.
    pub fault_poll: bool,
    /// The leaves its lowering laid out (`LoweredRegion::exits`): each place, its missing fact, and the values a leave inside
    /// an inlined frame stores, which the install takes as the body's table, as a JIT lane body keeps its own.
    pub exits: ExitTable,
}

/// Where the records live: in the loader's own allocation (`aot_image::live::install`), or in the
/// executable's writable data, where the system linker placed them beside the code that reads them
/// (`aot_image::object`, `live::linked`).
pub enum BoundRecords {
    Owned(Box<[UnsafeCell<CallRecord>]>),
    Linked(&'static [UnsafeCell<CallRecord>]),
}

macro_rules! miss_reasons {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        #[derive(Clone, Copy)]
        pub enum Miss { $($variant),+ }
        pub const MISS_NAMES: [&str; [$($name),+].len()] = [$($name),+];
    };
}
miss_reasons! {
    Form => "form",
    NoScriptCalls => "no-script-calls",
    CallConstructing => "call:constructing",
    WithScope => "with-scope",
    CallReturningNotAdmitted => "call:returning-not-admitted",
    Receiver => "receiver",
    Callee => "callee",
    CalleeDeclined => "callee:declined",
    EntryNotActivation => "entry-not-activation",
    NoActivationRoom => "no-activation-room",
    CallReadAnswer => "call:read-answer",
    Busy => "target-or-collection",
    Returning => "returning",
    ConstructNotConstructor => "construct:not-constructor",
    NoPlan => "no-plan",
    HelperConstructContext => "helper-construct:context",
    PlanNoUnit => "plan:no-unit",
    PlanInvalidBindings => "plan:invalid-bindings",
    PlanFrameNotAdmitted => "plan:frame-not-admitted",
    PlanNoEntry => "plan:no-entry-instruction",
    PlanCompileBound => "plan:compile-bound",
    PlanNothingNative => "plan:nothing-native",
    PlanNotAmortized => "plan:not-amortized",
    PlanDependencies => "plan:dependencies",
    NativeNotNative => "native:not-native",
    NativeNotCompleted => "native:not-completed",
}

/// Compose a host's POSIX signal delivery with owned native-fault recovery.
/// Prepare before installing the host observer, then let recovery consume its
/// owned faults before enqueuing ordinary host signal notifications.
pub struct NativeFaultHandler;

/// How the shared VM executes guest functions. The dynamic modes share the
/// interpreter/JIT; NativeOnly requires linked code. All use the same heap,
/// frames and host APIs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExecutionMode {
    /// The shared interpreter: general loop and fast lane.
    #[default]
    Interpreted,
    /// Hot supported JavaScript functions and Wasm functions run as native code.
    Compiled,
    /// Require linked guest code at every entry; reject interpreter/JIT fallback.
    /// This execution check does not itself remove compiler code from the binary.
    #[cfg(feature = "native-execution")]
    NativeOnly,
}

/// A record in a heap cell: boxed, so its address stays, and a copy is a record of its own.
pub struct InstanceRecord(pub Box<InstanceContext>);

/// An environment window's entries after its registers: its environment, `DELETED` until
/// one is made (`window_environment`); the address of the environment's slot storage, which
/// never moves while it lives, held in a `Value`'s bits; the `EnvironmentWindow::captured`
/// mask, likewise; the words of the slot storage the window holds in place
/// (`staged::call_environment::lay_out_entries`) over its locals, which follow. Until a closure or a
/// frame needs the environment, every slot lives among the locals and the storage address
/// names that in-place storage, so a captured slot reads the same way before and after.
pub const WINDOW_ENVIRONMENT: u16 = 0;

pub const WINDOW_LOCALS: u16 = WINDOW_STORAGE + crate::heap::IN_PLACE_SLOTS_WORDS as u16;

pub const WINDOW_SLOTS: u16 = 1;

/// An activation a lane call made without a VM frame. Its registers are
/// a window of the lane stack; it returns to the record after `call` in its
/// caller, whose registers, view and memory it keeps for the return.
#[derive(Clone, Copy)]
#[repr(C, align(128))]
pub struct Activation {
    /// The frame prefix (`staged::frame`'s CALLEE, THIS, BODY, WINDOW words): the callee's
    /// descriptor, receiver, body and window, which emitted code writes for a direct call's frame
    /// (`ActivationKind::Native`) and which stays through its link (`NativeFrames`); only
    /// `window` for an activation the lane made. The callee is null for a frame the lane made.
    pub callee: *const DirectCallee,
    pub this: u64,
    pub body: usize,
    /// The address of its window of registers.
    pub window: usize,
    pub function: u32,
    pub call: Ip,
    pub caller_base: Ip,
    pub caller_registers: Registers,
    /// The program its caller runs, which its return makes the lane's again
    /// (`enter_program_in_lane`); its own is the next activation's caller program, or the
    /// lane's for the innermost.
    pub caller_program: ProgramId,
    pub kind: ActivationKind,
}

/// Where the kind's tag lies in an activation.
pub const KIND_OFFSET: usize = std::mem::offset_of!(Activation, kind);

/// The cache tier a single-read `GetField` site's entries call for, to which a miss of its
/// single-entry handler moves the record (`row_get_field_miss`, `row_returning_field_miss`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FieldSiteTier {
    /// A nested read whose steps hit primary own entries, or an array's own `length`
    /// (`lane_nested_value`).
    Nested,
    /// A nested read with another step, which only the general cached read makes.
    General,
    /// A read that met an accessor the site cached (`Vm::field_site_getter`).
    Getter,
    /// Past one shape (`Vm::field_site_polymorphic`).
    Polymorphic,
    /// A primary entry naming a prototype holder (`Vm::field_site_holder`).
    Holder,
    /// An entry beyond the immediate prototype (`Vm::field_site_inherited`).
    Inherited,
    /// The primary own entry, which the record already reads.
    Own,
}

#[derive(Debug, Default)]
pub struct OsrDiagnostics {
    pub plain_backward_jump_sites: usize,
    pub admitted_sites: usize,
    pub promotion_only_sites: usize,
    pub compile_attempts: u64,
    pub compile_successes: u64,
    pub currently_compiled_sites: usize,
    pub native_entry_attempts: u64,
    pub native_region_runs: u64,
    pub tier_ups: u64,
    pub body_runs: u64,
    pub entry_guard_misses: u64,
    pub exits: u64,
    pub widenings: u64,
    /// The attempts that entered no region code, by why, over all sites.
    pub declines: [u64; RegionDecline::COUNT],
    pub analysis_rejections: Vec<(RegionRejection, usize)>,
    pub sites: Vec<OsrSiteDiagnostics>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OsrSiteDiagnostics {
    pub header_pc: u32,
    pub backedge_pc: u32,
    pub status: OsrSiteStatus,
    pub failure: Option<RegionAnalysisFailure>,
    pub admitted: bool,
    pub deferred_call: bool,
    pub tier_threshold_reached: bool,
    pub lane_backedge_count: u64,
    pub analysis_rejection: Option<RegionRejection>,
    pub call_boundary_pcs: Vec<u32>,
    pub compile_attempts: u64,
    pub compile_successes: u64,
    pub native_entry_attempts: u64,
    pub native_region_runs: u64,
    pub entry_guard_misses: u64,
    pub exits: u64,
    pub widenings: u64,
    /// The attempts that entered no region code, by why.
    pub declines: [u64; RegionDecline::COUNT],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OsrSiteStatus {
    Admitted,
    CallBoundary,
    PromotionOnly,
    Rejected,
}

/// The words a VM frame's register file keeps spare past its environment window for the windows of the
/// callees a loop region inlines (`base_region::function::inline_base`), which start past that window:
/// the stitch policy's window and the context words around it.
pub const INLINED_WINDOW_ROOM: usize = 64;

/// What a native's loop of calls of one callee with one receiver from the lane that called
/// it proves once (`Vm::prepare_lane_callback`), as JSC's `CachedCall` links its callee once
/// per loop: the lane, the native's site in it and the context its caller resumes with, the
/// callee's frameless target and the `this` it binds, resolved under `epoch`, the native
/// stack the calls run on and the pending calls their frames would take. Each call
/// (`Vm::call_prepared_lane`) tests only what the native's own steps between calls can
/// change, and leaves any other call to `Vm::call_value`.
pub struct PreparedLaneCallback {
    pub cx: *mut LaneContext,
    pub site: LaneNativeCall,
    pub receiver: Value,
    pub target: ScriptCallTarget,
    pub this: Value,
    /// The collection epoch `target` was resolved under; a collection retires it.
    pub epoch: u64,
    /// The lane's context as the native's caller left it, which a callback that leaves the
    /// lane restores (`restore_context_in_lane`). It holds no drop glue.
    pub caller: LaneContext,
    /// The activation calls passing its argument count push, made once (`PreparedShape`).
    pub shape: Option<PreparedShape>,
    pub pending: Vec<PendingGeneralCall>,
    /// The executing program the calls' frames would take, which the context names with
    /// `pending` from the first call to the loop's end (`Vm::call_prepared_lane`).
    pub executing: crate::vm::dispatch_frame::ExecutingProgram,
    pub _stack: crate::stack::StackGuard,
}

/// The kinds of operand a site met where its quickened int32 handler declined, as a set of bits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OperandKinds(pub u8);

/// How a job's lane run ended (`Vm::run_lane_job`).
pub enum JobEnd {
    /// The activation returned the value: its owner is finished.
    Returned(Value),
    /// The activation awaited the value in the lane, its owner holding the frame and the
    /// await's reactions the job's to attach, or a sync generator's root yielded it, its record
    /// suspended.
    Suspended(Value),
    /// An async generator's root yielded the value in the lane: its frame parked in its record,
    /// the yield awaits the value before it settles the request (`Vm::async_generator_yield`).
    Yielded(Value),
    /// The activation raised an exception no handler of it covers, which ended it in its owner
    /// (`raised_windows`): the promise or request the job settles rejects with the error.
    Threw(JsError),
    /// The lane left the activation to the general path, or did not lay its function out as a
    /// window (`Vm::run_job`), which ran the VM frame the activation became: the bottom one
    /// stays on the frame stack, with this outcome.
    Frames(Result<FrameOutcome, JsError>),
}

/// The text that names the build in a library's bytes, followed by the engine's build ID in 16 hex
/// digits (`quench_runtime_engine`'s `AHEAD_BUILD_ID_MARK`, which names the engine and the core).
pub const AHEAD_BUILD_ID_PREFIX: &[u8; 16] = b"QUENCH_BUILD_ID=";

/// An own member a constant-marked object holds (`Vm::own_constant_of`): the object a lane body
/// guards by identity, the exact shape it depends on (whose write retires the body), and the
/// value the member holds while the object is marked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OwnConstant {
    pub object: Value,
    pub shape: u32,
    pub value: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeFieldLoad {
    pub shape: u32,
    /// The exact shape of the receiver's prototype, an object record, that holds the slot;
    /// `None` for the receiver's own (`FieldCache::holder`).
    pub holder: Option<u32>,
    pub slot: u16,
    pub storage_len: usize,
    pub storage: Option<PlainPropertyStorage>,
    /// The slot's type when the plan was made (`Heap::slot_type`), which every store maintains:
    /// an online body subscribes to it (`LaneDependencies`) and reads the slot at it with no tag
    /// test; code that does not subscribe tests the tag and leaves where it fails.
    pub ty: crate::typed_ir::Type,
    /// For a holder read, the value its proved holder holds when the code was compiled
    /// (`Vm::inherited_constant`), with the chain from the receiver's prototype it was read
    /// through: a Stable fact. A read takes it while the receiver's prototype is that one; a
    /// change of the value retires the code that took it (`heap::subscribe_prototype_values`), as
    /// does a layout change of a prototype on its chain (`Heap::subscribe_prototype_chain`).
    pub inherited: Option<InheritedConstant>,
    /// Whether a live function record had the receiver shape when the plan was made: a read
    /// then proves a readable record (an object or a function's), never a direct object.
    pub function_shape: bool,
    /// Whether the slot's arena words are raw doubles (its owner's type is Number under float
    /// slots): its load reads the double and boxes it (`staged::object::read_slot`).
    pub raw_double: bool,
    /// For an own read, the layout id (`Object::hint_shape`) of the receivers the site's lane
    /// record last answered: the read guards it in one compare of the record's own word,
    /// proving the shape and the storage form, and reads that form's layout, a fact of the id,
    /// with no test (`IcOp::GuardLayout`).
    pub formed: Option<u32>,
    /// For a holder read, the layout id of the holder (`FieldCache::holder_id`) where the entry
    /// kept one: a guard of it proves the holder's whole header, which names `holder` as its
    /// shape, so it replaces the holder's shape guard (`IcOp::GuardLayout`, `field_plans`).
    pub holder_id: Option<u32>,
}

/// An inherited read's values, each fixed by the receiver's prototype link that names its
/// holder, as they were when the code was compiled, and the prototypes from that link to the
/// holder each was read through (`PrototypeChain::depth`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InheritedConstant {
    pub values: [(u32, Value); INHERITED_PROTOTYPES],
    pub depths: [u8; INHERITED_PROTOTYPES],
    pub count: u8,
}

/// What the system linker placed of an image in this executable (`object::emit`): each function's
/// entry and base body, and the image's file and call records, as its entry table
/// lists them.
#[derive(Clone, Copy)]
pub struct LinkedTable {
    pub entries: &'static [usize],
    pub bases: &'static [usize],
    pub file: &'static [u8],
    pub calls: *mut CallRecord,
    /// The lane helpers the image names, by number (`LiveImage::helpers`).
    pub helpers: &'static [usize],
    /// The native search of each of the image's RegExp literals (`AotImage::regexps`), by index.
    pub regexps: &'static [usize],
}

/// Every body starts on a cache line, which also satisfies each target's function alignment.
pub const CODE_GRANULE: usize = 64;

/// The AOT image the system linker placed in this executable for a program (`aot_image::object`):
/// a VM that runs the program loads it as its ahead images (`Vm::load_linked_image`), and its
/// functions run their bodies on the lane runner (`Vm::install_ahead_body`).
pub enum NativeJsCode {
    /// Code the system linker placed in this executable (`NativeJsProgram::from_linked_image`).
    Placed(LinkedTable),
    /// An image compiled into this process (`NativeJsProgram::compile_eager`): its file, linked
    /// into each VM that attaches it (`aot_image::live::install`).
    Eager(Vec<u8>),
}

pub type NativeProgramResolver = dyn Fn(&str, &[&str]) -> Result<Option<NativeJsProgram>, crate::NativeSourceError>;

/// A program bound to native entries compiled for the same host and semantic ABI.
pub struct NativeJsProgram {
    pub program: Rc<ResidualProgram>,
    pub code: Rc<NativeJsCode>,
    pub counts: NativeJsPublicationCounts,
    pub frozen: Option<FrozenAssumptions>,
}

unsafe extern "Rust" {
    fn quench_engine_native_from_linked_image(program: ResidualProgram, image: *const u8) -> Result<NativeJsProgram, String>;
}

impl NativeJsProgram {
    /// Adopt a statically linked image table (the engine's `NativeJsProgramOps1::from_linked_image`),
    /// named here so an image's driver, which links the core and its host, reaches it
    /// (`linked_native_sources!`).
    ///
    /// # Safety
    /// `image` must be the aligned entry-table symbol for this exact program, emitted by this
    /// build's object linker; its code, image file and records must live for the process's lifetime.
    #[cfg(feature = "native-execution")]
    pub unsafe fn from_linked_image(program: ResidualProgram, image: *const u8) -> Result<Self, String> {
        // SAFETY: the caller's contract is the hook's.
        unsafe { quench_engine_native_from_linked_image(program, image) }
    }
}

/// What a publication lowered, so admission is measured rather than assumed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NativeJsPublicationCounts {
    /// Functions in the program.
    pub functions: usize,
    /// Functions whose entry runs a lane body, leaving to per-PC semantic entries.
    pub bodies_in_lane: usize,
    /// Functions left on the call-threaded body (per-PC semantic entries).
    pub call_threaded: usize,
    /// Instruction sites whose continuation is a folded static transfer.
    pub static_transfers: usize,
    /// Instruction sites that call their opcode's semantic entry.
    pub semantic_entry_sites: usize,
    /// Whole-program globals the image folds, each checked at its read site.
    pub frozen_globals: usize,
    /// Whole-program prototype values the image folds, each guarded at its read site.
    pub frozen_fields: usize,
    /// Functions whose windowed root map would exceed `MAX_MASK_BYTES`: their frames carry
    /// one conservative all-live map.
    pub wide_frame_functions: usize,
}

/// A statically selected semantic helper's C ABI. Operands use `NativeJsOperands`.
#[cfg(feature = "native-execution")]
pub type NativeJsOperationEntry = unsafe extern "C" fn(
    *mut std::ffi::c_void, u64, *const u64, *mut u64,
) -> i64;

#[cfg(feature = "native-execution")]
pub struct StaticJsOperation {
    pub opcode: Op,
    pub execute: fn(&mut Vm, &ResidualProgram, usize, WideInstruction, &mut usize, bool)
        -> Result<StepResult, JsError>,
}

/// Copies the `count` values at `source` to `destination`: a window of at most
/// `FIXED_ARGUMENT_COPIES` values as one move of its size, which needs no loop.
///
/// # Safety
/// Both ranges hold `count` values and do not overlap.
#[inline(always)]
pub unsafe fn copy_arguments(destination: *mut Value, source: *const Value, count: u16) {
    /// One move of `N` values.
    #[inline(always)]
    unsafe fn copy<const N: usize>(destination: *mut Value, source: *const Value) {
        // SAFETY: the caller's contract, for `N` values.
        unsafe { destination.cast::<[Value; N]>().write_unaligned(source.cast::<[Value; N]>().read_unaligned()) }
    }
    const _: () = assert!(FIXED_ARGUMENT_COPIES == 4);
    // SAFETY: the function's contract.
    unsafe {
        // Compares, not a table, so the common windows of a value or two take a branch each.
        if count <= 2 {
            if count == 2 {
                copy::<2>(destination, source);
            } else if count == 1 {
                copy::<1>(destination, source);
            }
        } else if count <= FIXED_ARGUMENT_COPIES {
            if count == FIXED_ARGUMENT_COPIES {
                copy::<4>(destination, source);
            } else {
                copy::<3>(destination, source);
            }
        } else {
            std::ptr::copy_nonoverlapping(source, destination, usize::from(count));
        }
    }
}

/// Initializes exactly the live register range. Admission may reserve a larger room,
/// but padding is not part of the activation's values or materialized frame.
///
/// # Safety
/// `registers` has writable storage for `count` values, none of them live.
#[inline(always)]
pub unsafe fn fill_window(registers: *mut Value, count: usize, initial: Value) {
    if count >= WINDOW_INLINE_FILL_LIMIT {
        for index in 0..count {
            // SAFETY: each destination lies within the caller's uninitialized range.
            unsafe { registers.add(index).write_unaligned(initial) };
        }
        return;
    }
    #[inline(always)]
    unsafe fn pair(destination: *mut Value, initial: Value) {
        // SAFETY: the caller provides both uninitialized values. Individual stores
        // avoid aggregate-array temporaries in the production call handlers.
        unsafe {
            destination.write_unaligned(initial);
            destination.add(1).write_unaligned(initial);
        }
    }
    #[inline(always)]
    unsafe fn half(destination: *mut Value, initial: Value) {
        // SAFETY: the caller provides a complete half block.
        unsafe {
            pair(destination, initial);
            pair(destination.add(WINDOW_FILL_PAIR), initial);
        }
    }
    let mut tail = 0;
    if count >= WINDOW_FILL_BLOCK {
        // SAFETY: the logical range contains one complete block.
        unsafe {
            half(registers, initial);
            half(registers.add(WINDOW_FILL_HALF), initial);
        }
        tail = WINDOW_FILL_BLOCK;
    }
    if count & WINDOW_FILL_HALF != 0 {
        // SAFETY: this bit proves that a half block remains in the exact tail.
        unsafe { half(registers.add(tail), initial) };
        tail += WINDOW_FILL_HALF;
    }
    if count & WINDOW_FILL_PAIR != 0 {
        // SAFETY: this bit proves that a pair remains after the half block.
        unsafe { pair(registers.add(tail), initial) };
        tail += WINDOW_FILL_PAIR;
    }
    if !count.is_multiple_of(WINDOW_FILL_PAIR) {
        // SAFETY: the odd final value is within the logical range.
        unsafe { registers.add(tail).write_unaligned(initial) };
    }
}

/// What an activation keeps beyond its registers, by the kind of function it runs.
#[derive(Clone, Copy)]
#[repr(C, u8)]
pub enum ActivationKind {
    /// A native direct call's frame as emitted code wrote it: only the prefix words
    /// (`Activation::native`, the callee's descriptor, receiver, body and window) and this
    /// tag, no link state. `NativeFrames::link` makes it a `Script` activation when the lane
    /// first observes it; the prefix stays, as the descriptor of the frame above it.
    Native = KIND_NATIVE,
    Script(ScriptActivation) = KIND_SCRIPT,
    /// A Wasm activation keeps its caller's memory for the return and its requested
    /// register payload for guest-stack accounting, excluding admission padding. `program` is
    /// the function's own program, and `pc` the record its lane last left it at for a callout
    /// that may collect (`LaneStack::note_wasm_pc`): the collector reads its live registers
    /// there while no activation sits above it.
    Wasm { memory: MemoryView, frame_storage_bytes: usize, program: ProgramId, pc: u32 } = KIND_WASM,
    /// A Wasm activation a JavaScript call of an exported function made
    /// (`lane_wasm_export_frame`): it runs over `instance`, the export's instance environment
    /// (`LaneContext::instance`), and its return gives its caller, JavaScript, the result's
    /// JavaScript form (`CallCompletion::WasmExport`). The caller's receiver and environments
    /// stay in the context while it runs: no Wasm record reads or writes them.
    WasmEntry { instance: Value, result: Option<crate::WasmType>, frame_storage_bytes: usize, program: ProgramId, pc: u32 },
}

/// The values of a call window to copy as one fixed-size move each; a longer window copies
/// value by value.
pub const FIXED_ARGUMENT_COPIES: u16 = 4;

/// The whole-program assumptions an eager image folded, replayed when it loads.
pub struct FrozenAssumptions {
    pub invalidation: crate::code_dependencies::Invalidation,
    /// Method atoms the code holds unnamed (`StableFacts::atoms`).
    pub atoms: Box<[crate::bytecode::Atom]>,
    /// Whole-program globals the code folds (its read-site guards verify them).
    pub globals: Box<[crate::bytecode::Atom]>,
    /// Whole-program prototype values the code folds.
    pub fields: Box<[crate::bytecode::Atom]>,
    /// Set when any assumption no longer holds; the code leaves at its invalidation point.
    pub watch: Rc<crate::code_dependencies::PublishedCode<()>>,
    /// The program's constants, materialized into this block after realm initialization.
    pub constants: Rc<std::cell::RefCell<Box<[Value]>>>,
}

/// The most prototypes of one holder shape an inherited read folds: sibling classes whose
/// prototypes hold the same names share a shape, and each holds its own values.
pub const INHERITED_PROTOTYPES: usize = 4;

/// Lane state that handlers read rarely; the hot state travels in handler
/// arguments so it stays in machine registers across dispatches. Its fields keep their
/// declared order: a plain call saves and restores `this`, `env` and `capture` together
/// (`push_passed_in_lane`), so they stay adjacent, and the Wasm tier's rarely read counters
/// come last.
#[repr(C)]
pub struct LaneContext {
    pub binding: LaneBinding,
    pub vm: *mut Vm,
    /// The executing program, which every inline call stays within.
    pub code: *const ResidualProgram,
    pub program: ProgramId,
    /// What the run starts from: the VM frame it runs, or a job's frame in its record
    /// (`LaneRoot`). The registers and the view of the function the lane runs follow.
    pub root: LaneRoot,
    pub registers: Registers,
    pub base: Ip,
    /// The general loop's pending inline calls, which lane calls extend and
    /// lane returns complete exactly as the general loop would.
    pub pending: *mut Vec<PendingGeneralCall>,
    /// The general loop's executing program, which a materialized call into another
    /// program sets aside in its pending call as the loop's own push does.
    pub executing: *mut crate::vm::dispatch_frame::ExecutingProgram,
    /// The program's lane views by function, which a call reads when its
    /// callee's view is derived.
    pub views: *const [crate::vm::program_store::LaneViewCell],
    /// The program's constants, which constant operands index. A program's lane views are
    /// derived from its constants and replaced with them, so they hold for the run.
    pub constants: *const [Value],
    /// Lane calls and returns manage activations: the interpreter runs a
    /// Wasm program in a loop that allows inline calls. A compiled callee
    /// runs from the general path, which raises its traps.
    pub inline_calls: bool,
    /// The view handlers start from when the lane loop dispatches.
    pub memory: MemoryView,
    /// The last memory binding resolved in this lane run and its view. No
    /// lane instruction grows memory or runs host code, so a binding's view
    /// holds for the whole run; calls and returns within one instance reuse
    /// it instead of resolving the binding again.
    pub resolved: (Value, MemoryView),
    /// JavaScript calls may activate their callees without frames: the general loop
    /// would push them in place (`Vm::try_inline_user_call`).
    pub script_calls: bool,
    /// `new` may activate its callee without a frame too: the run's frame is an ordinary
    /// body, from which the general loop pushes constructions (`Vm::inline_construct_target`).
    pub script_constructs: bool,
    /// Whether a compiled tier may run (`Vm::tier_may_run`), decided once when the run starts: the
    /// execution mode, memory policy and JIT mode hold for the whole run, so a call reads this
    /// instead of asking the policy again.
    pub tiers_run: bool,
    /// Whether a return completes a script frame the run entered from a Wasm export
    /// (`lane_return_script_frame`): script calls on and inline calls off, kept by
    /// `lane_set_script_calls`, which every switch of the two makes after setting
    /// `inline_calls`. A return reads this one flag.
    pub returns_script_frame: bool,
    /// The running activation's `this`, or `DELETED` when the frame's binding decides
    /// it (`Vm::current_this`).
    /// The instance environment of the Wasm activations above a JavaScript call of an export
    /// (`ActivationKind::WasmEntry`), or `DELETED` where the frame's environment is the
    /// instance's (`instance_env_in_lane`).
    pub instance: Value,
    /// The last outer capture walk an activation read through (`Reads::of_captures`).
    /// One walk measured better than one per depth parity on closure-heavy code.
    pub outer: OuterWalk,
    /// The last walk a root binding read took (`lane_root_slots`): from the running
    /// activation's environment word, or its frame's, out to the root environment's slots.
    pub root_walk: OuterWalk,
    /// The storage of the environment a frame run's captured locals live in, once a local
    /// access found it (`frame_storage_in_lane`).
    pub frame_storage: FrameStorage,
    /// The instance environment the run's Wasm bindings last read and its dense slot storage
    /// (`wasm_instance_slot`), found this run as `frame_storage` is.
    pub instance_slots: (Value, *const usize),
    /// The row of the last indirect target the lane made (`IndirectSlot`), which the reference
    /// call right after it reads its callee from.
    pub indirect: IndirectSlot,
    /// The running instance's record address, cached for the environment (its bits) and program
    /// it was found for (`wasm_calls::running_instance_address`); 0 where the environment has no record.
    pub instance_address: (u64, ProgramId, usize),
    /// The running program's Wasm signature pool, shared with the program, for the program it is
    /// held for (`lane_signatures`): an indirect target reads its expected canonical from it.
    pub signatures: (ProgramId, Option<std::rc::Rc<crate::wasm::WasmSignatures>>),
    /// Backward jumps left before the running Wasm function is credited with an
    /// interpreter entry's worth of loop work (`TIERS.wasm.backedges_per_entry`), or `None`
    /// when this run's code earns none (`Vm::credits_backedges_in_lane`). A crediting run keeps
    /// its floor raised, so every backward jump reaches `safepoint_at`, which counts: the
    /// handlers stay as they are.
    pub backedges_left: Option<u32>,
    /// The accumulator the lane loop dispatches its next record with: the one a safepoint
    /// jump carried to its target (`safepoint_at`), else none.
    pub resumed_acc: Slot<Value>,
    /// The record a lane body's native call left the lane to continue at, one of the callee's
    /// records in the activation it made (`body_call_helper`), and the accumulator that record reads.
    pub callee_resume: Ip,
    pub callee_acc: Value,
    /// The call a lane body's helper decoded last (`BodyCall::at`): its program, function, record and
    /// form, which the run's code fixes, so a loop's repeated call or construction decodes
    /// its instruction once per run.
    pub body_site: Option<(*const ResidualProgram, u32, u64, crate::staged::Invocation)>,
    /// The call record whose caller's late calls last counted past the threshold
    /// (`lane_note_late_call`), which counts nothing more for it in this run.
    pub late_settled: Ip,
    /// What this run proved about the storage of the arrays its loops index
    /// (`ArrayStorageProofs`), which the proved element records read
    /// (`specialize_proved_arrays`). The heap retires them at any change that could falsify
    /// one; a nested run retires them on entry, and they end with the run.
    pub proofs: crate::heap::ArrayStorageProofs,
    /// The run's floor cell (`lane_floor`), which a JavaScript safepoint reads from here
    /// (`script_below_floor`).
    pub floor: *const std::sync::atomic::AtomicUsize,
    /// Backward jumps left before one reaches `safepoint_at` (`script_fuel_out`), which
    /// credits the work clock and refills it; zero while the floor is raised at entry or a
    /// collection is due (`note_allocation`), so every backward jump reaches it.
    pub fuel: u32,
    /// Whether that callee left the lane because it has no body yet and is still warming
    /// toward one (`body_activate_in_lane`): its caller's leave says nothing of the caller's own
    /// specialization (`leave_counts_in_lane`).
    pub callee_warming: bool,
    /// The leaf call whose records run (`script_call_leaf_prove_miss`), which holds what its
    /// activation would until its return or materialization; `NO_LEAF_CALL` for none.
    pub leaf: LeafCall,
    /// The dispatch payload of the native loop region running on this activation, zero for none
    /// (`region_payload_in_lane`): the heap bases a call it makes may move are reloaded there too
    /// (`region_call_in_lane`).
    pub region_payload: usize,
    /// The `CallCode` of the native loop region running on this activation, zero for none: a call its
    /// direct row missed rebinds the row's record there (`rebind_region_call`).
    pub region_calls: usize,
}

/// The activation of a native's callback for calls passing `argc` values (`push_callback_in_lane`),
/// made once per prepared loop: every field but its window, which each call takes from the
/// lane stack, and the window's layout: its `registers`, the `passed` values its parameters
/// bind, whether it `keeps` every value passed after its registers (where a forwarded
/// `arguments` or a sloppy function's legacy reflection can observe those past its
/// parameters), its whole `size` and the `room` its admission needs.
#[derive(Clone, Copy)]
pub struct PreparedShape {
    pub argc: usize,
    pub activation: Activation,
    /// The plain header a callback that keeps nothing pushes instead of `activation`
    /// (`push_prepared_in_lane`); `None` for one that keeps its values. An array step's names the
    /// array steps' trampoline record instead (`array_steps::push_array_step_in_lane`).
    pub header: Option<CallbackHeader>,
    pub registers: usize,
    pub passed: usize,
    pub keeps: bool,
    pub size: usize,
    pub room: usize,
}

pub const WINDOW_FILL_BLOCK: usize = WINDOW_FILL_HALF * 2;

pub const WINDOW_FILL_HALF: usize = WINDOW_FILL_PAIR * 2;

pub const WINDOW_FILL_PAIR: usize = 2;

// Bound inline work to one complete block and its binary tail. Splitting longer
// ranges between inline stores and a library fill costs more than one direct fill.
pub const WINDOW_INLINE_FILL_LIMIT: usize = WINDOW_FILL_BLOCK * 2;

pub const WINDOW_STORAGE: u16 = 3;

/// The slot storage of environment `env`, which frame `frame` held when a lane local access
/// looked it up; null for none. A live environment's storage never moves, and no
/// collection runs in the lane, so it holds while that frame runs with that environment.
#[derive(Clone, Copy)]
pub struct FrameStorage {
    pub frame: usize,
    pub env: Value,
    pub slots: *mut crate::heap::EnvironmentSlots,
}

/// The row of the last indirect target the lane made: the record after the target, the register
/// and reference it wrote, and the row's entry and instance (`TableTarget`). A reference call
/// that is that record, reading that register and reference, takes its callee from the row.
#[derive(Clone, Copy)]
pub struct IndirectSlot {
    pub next: Ip,
    pub register: u16,
    pub reference: u64,
    pub entry: u32,
    pub instance: u64,
}

pub const KIND_SCRIPT: u8 = 1;

pub const KIND_WASM: u8 = 2;

/// A lane's immutable record-range binding and its owning Host's resume operation.
/// Every context starts with this prefix, so nested runs can resume a different Host safely.
#[repr(C)]
pub struct LaneBinding {
    pub resume: unsafe fn(*mut ()),
    /// Drops what the context resolved under the collection epoch before
    /// (`retire_lane_binding`), for its owning Host.
    pub retire: unsafe fn(*mut ()),
    pub object_base: *const Object,
}

/// What a lane run starts from.
#[derive(Clone, Copy)]
pub enum LaneRoot {
    /// The VM frame the run continues, the innermost when it starts.
    Frame(usize),
    /// The activation of a promise job or a native generator step, which the lane runs
    /// frameless over its owner's record.
    Job(JobRoot),
}

/// A running leaf call (`CallEntry::frameless(Frameless::Leaf)`): what its activation would hold that the context
/// does not. The context's registers and records are the callee's; what its form makes its
/// own (`LeafForm`) of the receiver and environment is the callee's, the rest the caller's.
/// Laid out as the staged leaf row's record (`staged::call_leaf::LEAF_*_AT`), which writes
/// it at the call (`enter_leaf`) and reads it at the return (`complete_leaf`).
#[derive(Clone, Copy)]
#[repr(C)]
pub struct LeafCall {
    /// The call record, null while no leaf runs.
    pub call: Ip,
    pub caller_registers: Registers,
    pub caller_base: Ip,
    /// The target the call resolved, which stays in its cache while the leaf runs: nothing
    /// in a leaf resolves a target.
    pub target: *const ScriptCallTarget,
    /// The values the call passed, which its activation records.
    pub argc: u16,
    /// The form the call ran as, which decides what its activation takes from the caller
    /// (`materialize_leaf_in_lane`): a published body's entry (`enter_cell_body_in_lane`) runs as `CONTEXT_LEAF`.
    pub form: LeafForm,
    /// The caller's program, for a leaf of another program (`cross_leaf_call_in_lane`), which
    /// its return enters again; `None` for one of the caller's own.
    pub caller_program: Option<ProgramId>,
}

#[derive(Clone, Copy)]
pub struct MemoryView {
    pub bytes: *mut u8,
    /// The memory's length less the widest direct access: an access
    /// starting at or below it is in bounds whatever its width, so the hot
    /// path compares the address alone. Negative when the memory is
    /// narrower than that access. Unset in a JavaScript view (`MemoryView::script`).
    pub reach: Slot<isize>,
}

/// A walk from environment `env`, `DELETED` for none, `depth` layers out to the dense slots
/// `slots`. An environment's chain is fixed and its storage stays put while no collection
/// runs, so the walk holds while the running activation's environment is `env`; a re-entry
/// after the general path, which may collect, forgets it.
#[derive(Clone, Copy)]
pub struct OuterWalk {
    pub env: Value,
    pub depth: u16,
    pub slots: *const crate::heap::EnvironmentSlots,
}

/// What a JavaScript activation's frame would hold beyond its registers
/// (`Vm::activate_plain_frame`), and the caller's lane state its return restores. Its own
/// receiver and environment live in the context while it runs (`LaneContext::this`, `env`)
/// and in the next script activation's caller state beneath that (`ActivationState::of`):
/// one fact, held once.
#[derive(Clone, Copy)]
pub struct ScriptActivation {
    pub callee: Value,
    /// Arguments passed, all in parameter registers.
    pub argc: u16,
    /// What its return yields to the call.
    pub completion: Completion,
    /// The `argc` values passed follow the registers in the window, for a forwarded
    /// `arguments` (`ScriptCallTarget::forwards`), for the construction a skipped
    /// forwarding constructor passed on (`Completion::Forwarder`), or for values passed past
    /// the parameters.
    pub forwards: bool,
    /// The register past its registers that holds the first value it kept (`forwards`), its
    /// function's register count, so a read of them looks up no function; 0 when it keeps none.
    pub kept: u16,
    /// Its environment and frame locals follow its registers (`CallEntry::framed(Framed::Environment)`).
    pub environment: bool,
    /// The record of the suspended frame a windowed activation runs over (`generator_windows`,
    /// `lane_roots`), which the lane's name lookups read the dynamic bindings of; null for any
    /// other activation.
    pub record: *mut crate::vm::activation::FrameRecord,
}

/// The direct memory bytes behind the active function's memory register.
/// The view is resolved whenever that register can change under the lane:
/// at lane entry, when a lane call or return switches functions, and when
/// the binding instruction loads the register; no other lane instruction
/// writes it (see `LaneView`). The lane never collects, calls out or grows
/// memory, so the bytes stay alive, unborrowed and unresized at a stable
/// address. A memory the lane cannot borrow resolves to the empty view, on
/// which every access leaves the lane.
/// A carried value a record may leave unset: the accumulator after a record that produces
/// none, which only the record a producer falls through to reads, and a JavaScript view's
/// memory reach, which no JavaScript record reads. An unset slot costs its dispatch no
/// instruction; a reader gets one only where the view proves it was set.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Slot<T: Copy>(pub std::mem::MaybeUninit<T>);

/// A fixed table of words, zero until written, in a mapping of its own: untouched zero pages, so it
/// costs memory only as far as it is ever written. A zeroed allocation below the allocator's
/// mapping threshold is written zero byte by byte on a worker thread's arena instead: the lane
/// stack's two 512 KiB tables were set and faulted at every VM's start.
pub struct ZeroedWords {
    base: std::ptr::NonNull<usize>,
    len: usize,
}

impl ZeroedWords {
    pub fn new(len: usize) -> Self {
        Self { base: super::site_table::storage::map_zeroed(len * size_of::<usize>()).cast(), len }
    }
}

impl std::ops::Deref for ZeroedWords {
    type Target = [usize];
    #[inline(always)]
    fn deref(&self) -> &[usize] {
        // SAFETY: `base` maps `len` words, zero or written, which this table owns.
        unsafe { std::slice::from_raw_parts(self.base.as_ptr(), self.len) }
    }
}

impl std::ops::DerefMut for ZeroedWords {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut [usize] {
        // SAFETY: as `deref`, borrowed uniquely.
        unsafe { std::slice::from_raw_parts_mut(self.base.as_ptr(), self.len) }
    }
}

impl Drop for ZeroedWords {
    fn drop(&mut self) {
        // SAFETY: the mapping `new` made, which nothing reads again.
        unsafe { super::site_table::storage::unmap(self.base.cast(), self.len * size_of::<usize>()) };
    }
}

// SAFETY: the table owns its mapping as a `Vec` owns its buffer.
unsafe impl Send for ZeroedWords {}

impl Default for LaneStack {
    fn default() -> Self {
        Self {
            values: LaneValues::with_capacity(LANE_STACK_VALUES),
            activation_records: std::cell::UnsafeCell::new(LaneActivations::with_capacity(LANE_STACK_ACTIVATIONS)),
            plain: PlainFrames {
                depth: std::cell::Cell::new(0),
                registers: std::cell::Cell::new(std::ptr::null_mut()),
                program: std::cell::Cell::new(ProgramId::MAIN),
                bound: std::cell::Cell::new(0),
            },
            sides: std::cell::UnsafeCell::new(Vec::new()),
            segments: std::cell::UnsafeCell::new(Vec::new()),
            sealed: std::cell::Cell::new(0),
            plain_depth_limit: Box::new(std::cell::Cell::new(0)),
            frames_below: Box::new(std::cell::Cell::new(0)),
            body_top: Box::new(std::cell::Cell::new(0)),
            body_table: ZeroedWords::new(LANE_STACK_ACTIVATIONS),
            native_table: ZeroedWords::new(LANE_STACK_ACTIVATIONS),
            callback_return: Box::new(CallbackTrampoline::new()),
            wasm_storage_bytes: 0,
            call_sites: std::cell::RefCell::default(),
            noted: Box::new([const { std::cell::Cell::new(None) }; NOTED_SITE_CALLEES]),
            linked: Box::new([const { std::cell::Cell::new((0, Value::UNDEFINED)) }; NOTED_SITE_CALLEES]),
            script_targets: ScriptTargets::with_len(SCRIPT_CALL_TARGETS_INITIAL),
            cross_targets: ScriptTargets::with_len(SCRIPT_CALL_TARGETS_INITIAL),
            function_targets: Box::new([EMPTY_SCRIPT_CALL_TARGET; FUNCTION_TARGETS]),
            cross_function_targets: Box::new([EMPTY_SCRIPT_CALL_TARGET; FUNCTION_TARGETS]),
            construct_plans: Box::new([EMPTY_CONSTRUCT_PLAN; CONSTRUCT_PLANS]),
            construct_watches: Box::new([const { None }; CONSTRUCT_PLANS]),
            read_memos: Vec::new(),
            setter_sites: Vec::new(),
            native_identities: NativeIdentities::new(),
            native_stack: crate::stack::StackLimit::CLOSED,
            steps: Vec::new(),
            iterators: Vec::new(),
            js_import: None,
            bound_call: None,
            with_objects: Vec::new(),
            records: Vec::new(),
            forwarded_methods: Vec::new(),
            activation_limit: std::cell::Cell::new(0),
            generation: NO_SCRIPT_GENERATION,
            admission: Admission::new(NO_SCRIPT_GENERATION, ProgramId::MAIN),
            refused: 0,
            generation_key: (0, Value::UNDEFINED),
            native_boundary: None,
            body_boundary: None,
            boundary_frames: None,
            wasm_export: None,
            last_window_plan: [None; SHARED_PLANS],
            last_generator: [(Value::UNDEFINED, u64::MAX, std::ptr::null_mut()); 2],
        }
    }
}

impl CallerView for WindowScope {
    fn program(&self) -> ProgramId {
        self.program
    }

    fn function(&self) -> u32 {
        self.function
    }

    fn this(&self) -> Value {
        self.this
    }

    fn environment(&self) -> Value {
        self.env
    }

    fn owns_environment(&self) -> bool {
        self.captured
    }

    fn binding_site(&self) -> Option<u32> {
        self.site
    }

    fn held_bindings(&self) -> &[(Atom, Value)] {
        self.record_bindings().map_or(&[], |bindings| bindings.as_slice())
    }

    fn scope_chain<'a>(&'a self, _entered: &'a [Value]) -> &'a [Value] {
        &self.chain
    }
}

impl std::ops::Deref for InstanceRecord {
    type Target = InstanceContext;
    #[inline(always)]
    fn deref(&self) -> &InstanceContext {
        &self.0
    }
}

impl std::ops::DerefMut for InstanceRecord {
    #[inline(always)]
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

/// The frames a stack capture reads beside a lane's activations (`Vm::lane_stack_sites`).
pub struct LaneSites {
    /// The source pc of the lane's root frame, the innermost VM frame: its call of the outermost
    /// activation, or the deferred call where no activation is (`Vm::resume_caller_frame`,
    /// `Vm::resume_deferred_caller`). `None` for a job's run, which has no root frame: its bottom
    /// activation becomes the general loop's entry frame above the VM frames as they are.
    pub root_pc: Option<usize>,
    /// The activations outermost first, each as the frames its materialization pushes.
    pub sites: Vec<FrameSite>,
}

/// The words of an innermost activation's state that a parked lane keeps: its receiver and its environment.
pub const INNERMOST_STATE_LEN: usize = 2;

pub const EMPTY_CONSTRUCT_PLAN: ConstructPlan = ConstructPlan {
    callee: Value::UNDEFINED,
    callee_layout: (0, 0),
    prototype: Value::UNDEFINED,
    shape: 0,
    header: InlineObjectHeader::EMPTY,
    kind: ConstructPlanKind::Passed,
    fields: 0,
    parameters: [0; CONSTRUCT_PLAN_FIELDS],
    defaulted: 0,
    constant: 0,
    computed: 0,
    classes: [FieldClass::Passed; CONSTRUCT_PLAN_FIELDS],
    operators: [0; CONSTRUCT_PLAN_FIELDS],
    fallbacks: [Value::UNDEFINED; CONSTRUCT_PLAN_FIELDS],
    forwarded: None,
    nested: None,
    super_parent: Value::UNDEFINED,
};

pub const EMPTY_SCRIPT_CALL_TARGET: ScriptCallTarget = ScriptCallTarget {
    callee: Value::UNDEFINED,
    // Never proved, so no proved call's compare can match it (`Admission::UNPROVEN`).
    admission: Admission(Admission::new(NO_SCRIPT_GENERATION, ProgramId::MAIN).0 | Admission::UNPROVEN),
    function: 0,
    env: Value::UNDEFINED,
    view: std::ptr::null(),
    registers: 0,
    room: 0,
    params: 0,
    this_binding: ThisBinding::Sloppy,
    lexical_this: Value::UNDEFINED,
    forms: 0,
    fill: 0,
    entry: CallEntry::framed(Framed::Activates),
    capture: std::ptr::null(),
    forwarded_method: NO_FORWARDED_METHOD,
};

pub const LANE_STACK_ACTIVATIONS: usize = 1 << 16;

/// The capacity the lane stack reserves once (virtual until touched): values and activations
/// keep their addresses below it.
pub const LANE_STACK_VALUES: usize = 1 << 20;

/// A generation no lane runs under: an empty target entry never matches.
pub const NO_SCRIPT_GENERATION: ScriptGeneration = 0;

/// Entries a script call cache starts with: a program that calls few distinct callees keeps
/// 16 KiB per table resident instead of 64 (V8-v7 max RSS, both tables: about 100 KiB per
/// fixture). A cache grows to `SCRIPT_CALL_TARGETS` by this factor each time it has
/// displaced more live targets than it holds.
pub const SCRIPT_CALL_TARGETS_INITIAL: usize = 256;

impl Miss {

    /// How many reasons there are.
    pub const COUNT: usize = MISS_NAMES.len();
}

impl NativeIdentities {

    pub const fn new() -> Self {
        Self([UNPROVEN; IDENTITY_SLOTS])
    }
}

impl ScriptTargets {

    pub fn with_len(len: usize) -> Self {
        Self {
            entries: vec![EMPTY_SCRIPT_CALL_TARGET; len].into_boxed_slice(),
            displaced: 0,
            set_shift: crate::vm::shape_memo::direct_mapped_shift(len / SCRIPT_TARGET_WAYS),
        }
    }
}

impl CallbackTrampoline {

    /// A trampoline no handler is bound to yet (`bind`), which no header names.
    pub fn new() -> Self {
        let record = || LaneInstruction {
            handler: std::ptr::null(),
            operand: LaneOperand { imm: 0 },
            a: CALLBACK_RESULT,
            b: 0,
            c: 0,
            mode: 0,
            call_link: std::cell::Cell::new(NO_CALL_LINK),
        };
        Self {
            records: [record(), record()],
            steps: [record(), record()],
            result: std::cell::Cell::new(Value::UNDEFINED),
            call: std::cell::Cell::new(std::ptr::null()),
            caller_registers: std::cell::Cell::new(Registers(std::ptr::null_mut())),
        }
    }
}

/// The register of the trampoline's window (`CallbackTrampoline::result`) a callback's return
/// writes: its call record's `a`, a result register (no `RETURN_REGISTER`), so the plain return
/// takes it as any call's.
pub const CALLBACK_RESULT: u16 = 0;

/// A `LaneInstruction::call_link` that names no entry: no cache holds this many.
pub const NO_CALL_LINK: u16 = u16::MAX;

/// A `ScriptCallTarget::forwarded_method` that names no record. A program with more distinct
/// forwarding bodies than the index spans leaves the rest unrecorded.
pub const NO_FORWARDED_METHOD: u16 = u16::MAX;

/// Entries per set: the one most recently resolved first, then the one before it, so two
/// callees whose indices share a set both stay resolved.
pub const SCRIPT_TARGET_WAYS: usize = 2;

/// A slot no callee matches: the direct function word of an id no record has (record ids lie
/// below the record space's capacity, 2^24).
pub const UNPROVEN: u64 = crate::value::ValueLayout::DIRECT_FUNCTION_TAG | u32::MAX as u64;

/// How a lane call enters its target (`ScriptCallTarget::entry`). One call protocol, three forms
/// (`CallForm`): the form is the one `staged::effects::call_form` gives the callee's composed
/// signature, in every tier. `Frameless` for a callee that cannot leave its activation, collect,
/// or run guest code; `Framed` for any other. The shape says which frameless answer
/// (`Frameless`) or which activation layout (`Framed`) the target takes. The lane has no
/// `CallForm::Inline` entry: an inlinable callee is spliced into its caller's unit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct CallEntry(u8);

/// The shape of a `CallForm::Frameless` entry: which answer a call gets without an activation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Frameless {
    /// Its body is one returning `GetField` (`returning_kind` of its first instruction),
    /// whose hit a call answers without activating it (`call_read_in_lane`).
    Reads,
    /// Its body reads a field of `this` or a parameter, then an element of that by a
    /// parameter, and returns it (`element_read_body`), which a call answers likewise.
    ReadsElement,
    /// Its body returns a call of a method of a field of `this` or a parameter with its
    /// parameters in order (`forwarding_read_body`), which a call answers where that method
    /// is a `Reads` or `ReadsElement` target whose read hits.
    ForwardsRead,
    /// Its body returns one of its captures (`capture_read_body`), as a bundler's export
    /// getter `() => binding` compiles, which a call answers by reading the capture.
    ReadsCapture,
    /// Its body returns its `this` (`this_read_body`), as `get self() { return this; }`
    /// compiles, which a call answers with the receiver it binds.
    ReadsThis,
    /// Its body is a leaf (`leaf_body`): a plain call of its own program runs its records on
    /// a window of the lane stack without an activation (`script_call_leaf_prove_miss`), which
    /// anything observing activations makes first (`materialize_leaf_in_lane`). Any other
    /// call activates it as `Framed::Activates`.
    Leaf,
    /// A leaf whose records read their captures and not their receiver (`CAPTURE_LEAF`), so its
    /// call makes the context's environment its own, which its return restores.
    CaptureLeaf,
    /// A leaf whose records read its receiver (`CONTEXT_LEAF`), so its call makes the
    /// context's receiver and environment its own, which its return restores.
    ContextLeaf,
    /// A leaf whose records read its receiver and not its captures (`THIS_LEAF`), so its
    /// call binds the receiver, which its return restores, and keeps the caller's environment.
    ThisLeaf,
    /// A proven conditional append to a captured binding, completed without activation
    /// (`capture_append::run`).
    AppendsCapture,
    /// A body with admitted native code, whose call runs that code's entry on a window at the lane
    /// stack's top with no activation and no frame (`native_call_in_lane`). The callee's leaves are
    /// `Never`, so nothing observes the window. Chosen where the target resolves, while the body is
    /// published; a body retired since falls back to the general form.
    Native,
}

/// The shape of a `CallForm::Framed` entry: which activation layout a call makes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Framed {
    /// Its activation holds its registers alone (`activate_in_lane`).
    Activates,
    /// It keeps the values it was passed for a forwarded `arguments`
    /// (`CallPlan::forwards_arguments`, `activate_keeping_in_lane`).
    Keeps,
    /// It keeps them, and its rest parameter, in the register after the others, starts as
    /// the array of those past the others (`activate_keeping_in_lane`).
    Gathers,
    /// It needs a VM frame (`CallPlan::registers_only` fails, or its view has no
    /// activation): a call takes `frame_call_in_lane`. Only the cache and the call's own
    /// out-of-line form see such a target (`script_target_in_lane` leaves it out).
    Frame,
    /// It keeps frame locals and makes closures (`CallPlan::environment_activation`): its
    /// activation's window holds its environment and locals after its registers
    /// (`activate_environment_in_lane`). Only a plain call activates it.
    Environment,
    /// An async function whose body never suspends, laid out as its registers
    /// (`CallPlan::settles_registers_only`): only a plain call of its own program activates
    /// it (`script_call_settles_miss`), whose promise its return settles
    /// (`Completion::SettlesPromise`).
    Settles,
}

impl CallEntry {
    /// The form bit: set for a `CallForm::Framed` entry, clear for a `CallForm::Frameless` one.
    /// The low bits hold the shape, so the entry stays one byte (`ScriptCallTarget` is 64 bytes).
    const FRAMED: u8 = 1 << 7;

    /// The entry of a frameless shape.
    #[inline(always)]
    pub const fn frameless(shape: Frameless) -> Self { Self(shape as u8) }

    /// The entry of a framed shape.
    #[inline(always)]
    pub const fn framed(shape: Framed) -> Self { Self(Self::FRAMED | shape as u8) }

    /// The form of the call this entry takes: `Frameless` or `Framed`, never `Inline`.
    #[inline(always)]
    pub const fn form(self) -> CallForm {
        if self.0 & Self::FRAMED == 0 { CallForm::Frameless } else { CallForm::Framed }
    }

    /// The entry's byte as an index of a table of all 256 entries. The byte holds both the form bit
    /// and the shape, so a table over it answers a framed entry without a test of the form.
    #[inline(always)]
    pub const fn table_index(self) -> usize {
        self.0 as usize
    }
}

/// Whether a call of `function` takes a frameless entry: the form `staged::effects::call_form` gives the
/// function's composed signature (`FunctionFacts::signature`), of the program's facts `facts`
/// (`ProgramStore::aot_facts`). The facts are not inlinable here, since a lane target is never spliced.
/// A function the solve did not reach, or a program without facts, has the top signature, whose form is
/// framed.
pub fn frameless_call(facts: Option<&AotFacts>, function: u32) -> bool {
    let signature = facts.map_or(Signature::ANY, |facts| facts.function_facts(function).signature());
    call_form(signature, false) == CallForm::Frameless
}

impl Admission {

    #[inline(always)]
    pub const fn new(generation: ScriptGeneration, program: ProgramId) -> Self {
        debug_assert!(program.raw() as u64 <= Self::PROGRAM_MASK, "a program id below the flag bits");
        Self(generation as u64 | (program.raw() as u64) << Self::PROGRAM_SHIFT)
    }
    /// A bit a script call cache entry's admission carries until a plain call's link proves it
    /// (`script_call_prove_miss`, link-proofs.md), or a leaf call's link proves a leaf entry
    /// (`script_call_leaf_prove_miss`, leaf-keeping-c.md): its entry activates as its
    /// registers alone (`CallEntry::activates`), the leaf call form's entry checked too, and its
    /// cell body unpublished. Every entry is remembered with it (`ScriptTargets::remember`), and
    /// publishing a body sets it again on every entry of the view (`Vm::unprove_view_entries`).
    /// The lane's admission never carries it, so the proved call's one admission compare
    /// (`script_call_prove_miss`) also tests the proof; every other test of an entry ignores it
    /// (`Admission::admits`).
    pub const UNPROVEN: u64 = 1 << 61;
}

impl AheadImages {

    /// Whether this is the image of `program`, the program numbered `id`; each program is
    /// fingerprinted once.
    #[inline]
    pub fn is_of(&self, id: u32, program: &ResidualProgram) -> bool {
        let mut known = self.known.borrow_mut();
        if let Some((_, of)) = known.iter().find(|(known, _)| *known == id) { return *of; }
        let of = ahead_program_fingerprint(program) == self.image.fingerprint;
        known.push((id, of));
        of
    }

    /// Whether this is the image the system linker placed at `table`'s entries.
    #[inline]
    pub fn is_linked_at(&self, table: &LinkedTable) -> bool {
        self.placed == Some(table.entries.as_ptr().addr())
    }

    /// Adds `program`'s image while recording; a program that does not lower adds none.
    #[inline]
    pub fn record(program: &ResidualProgram) {
        let Some(lower) = recorded().as_ref().map(|recording| recording.lower) else { return };
        Self::keep(program, lower(program));
    }

    /// Adds Wasm program `id`'s image while recording, planned from its module's binding types.
    #[inline]
    pub fn record_wasm(vm: &Vm, id: ProgramId, program: &ResidualProgram, bindings: &[crate::WasmBindingType]) {
        let Some(lower) = recorded().as_ref().map(|recording| recording.lower_wasm) else { return };
        Self::keep(program, lower(vm, id, program, bindings));
    }

    /// Adds a lowering's image to the recording, or traces why `program` has none.
    fn keep(program: &ResidualProgram, compiled: Result<Self, String>) {
        let images = match compiled {
            Ok(images) => images,
            Err(why) => {
                if crate::trace::Channel::AotLanes.on() { eprintln!("aot-lanes: {} records no image: {why}", program.source_name); }
                return;
            }
        };
        if !images.is_empty() { if let Some(recording) = recorded().as_mut() { recording.images.push(images.image); } }
    }
}

impl CallCode {

    /// Releases the capacity its vectors hold past their length (`LaneDependencies::trim`).
    #[inline]
    pub fn trim(&mut self) {
        self.inlined.shrink_to_fit();
        self.records.shrink_to_fit();
    }

    /// Whether the code names a callee `live` says died.
    #[inline]
    pub fn names_dead(&self, live: impl Fn(Value) -> bool) -> bool {
        if names_dead_callee(&self.inlined, &[], &live) || self.direct.iter().any(|callee| callee.method.as_ref().is_some_and(|method| !method.is_published())) {
            return true;
        }
        // A dead callee a record's guard compares at each call only unbinds that record: its row misses from then on
        // (`Value::DELETED` names no closure) and the helper rebinds it to the next closure it calls
        // (`dispatch_fast::rebind_region_call`). One an entry check holds by a constant retires the code.
        let unbind = |word: u64| for record in self.records.iter().filter(|record| record.callee == word) {
            // SAFETY: the records are read in place by the code on this thread, which no collection runs inside.
            unsafe { (*std::ptr::from_ref(&**record).cast_mut()).callee = Value::DELETED.to_bits() };
        };
        // A planned callee whose site has no record is named by constants (a Cranelift region's guard and capture
        // words), as is one an entry check holds: its death retires the code.
        // One whose code reads its closure from its bound words (`BoundClosure::read`) was unbound instead (`unbind_dead`).
        let constant = self.direct.iter().filter(|callee| !live(callee.callee)).any(|callee| {
            let word = callee.callee.to_bits();
            if self.held.contains(&word) { return true; }
            if callee.bound.read.get() { return false; }
            if !self.records.iter().any(|record| record.call == callee.call as u64) { return true; }
            unbind(word);
            false
        });
        if constant { return true; }
        self.rebound.borrow().iter().filter(|callee| !live(callee.callee)).for_each(|callee| unbind(callee.callee.to_bits()));
        false
    }
}

impl CalleeAdmissions {

    /// Forgets every admission (`Vm::clear_call_targets`).
    #[inline]
    pub fn clear(&mut self) {
        if let Some(cache) = self.cache.get_mut() {
            cache.fill(FramelessCallEntry::default());
        }
        self.functions.clear();
    }
}

impl CompiledJsFunction {

    #[inline]

    pub fn mapped_bytes(&self) -> usize {
        self.mapped_bytes
    }
}

impl CompiledProgram {
    #[inline]
    pub fn has_function(&self, function: usize) -> bool { self.function(function).is_some() }
    /// The functions it holds code for.
    #[inline]
    pub fn held_functions(&self) -> impl Iterator<Item = usize> + '_ { (0..self.positions.len()).filter(|&function| self.has_function(function)) }

    #[inline]

    pub fn mapped_bytes(&self) -> usize {
        self._code.length()
    }

    #[inline]

    pub fn has_function_or_note_fallback(&self, function: usize) -> bool {
        let available = self.has_function(function);
        if !available
            && let Some(stats) = &self.diagnostics
            && let Some(fallbacks) = stats.interpreter_fallback_entries.get(function)
        {
            fallbacks.set(fallbacks.get().saturating_add(1));
        }
        available
    }
}

impl ExecutionMode {

    // Expose the build capability to callers in host crates, so unavailable
    // fallback branches cannot retain an interpreter or source compiler.
    #[inline(always)]
    pub fn allows_dynamic_execution(self) -> bool {
        crate::DYNAMIC_EXECUTION_ENABLED && match self {
            Self::Interpreted | Self::Compiled => true,
            #[cfg(feature = "native-execution")]
            Self::NativeOnly => false,
        }
    }
}

impl InheritedConstant {

    /// The values `holders` (prototype link, value, and the prototypes from the link to the
    /// holder) give; `None` where there are none or more than the read folds.
    #[inline]
    pub fn of(holders: impl IntoIterator<Item = (u32, Value, u8)>) -> Option<Self> {
        let mut constant = Self { values: [(0, Value::UNDEFINED); INHERITED_PROTOTYPES], depths: [0; INHERITED_PROTOTYPES], count: 0 };
        for (link, value, depth) in holders {
            let at = usize::from(constant.count);
            *constant.values.get_mut(at)? = (link, value);
            constant.depths[at] = depth;
            constant.count += 1;
        }
        (constant.count > 0).then_some(constant)
    }

    /// These values and `other`'s, each prototype once; `None` where a prototype holds two
    /// values or there are more than the read folds. A prototype both name keeps the longer
    /// chain, which covers the shorter.
    pub fn joined(self, other: Self) -> Option<Self> {
        let mut values: Vec<(u32, Value, u8)> = self.values().iter().zip(self.depths).map(|((link, value), depth)| (*link, *value, depth)).collect();
        for ((prototype, value), depth) in other.values().iter().zip(other.depths) {
            match values.iter_mut().find(|(known, _, _)| known == prototype) {
                Some((_, held, _)) if held != value => return None,
                Some((_, _, known)) => *known = (*known).max(depth),
                None => values.push((*prototype, *value, depth)),
            }
        }
        Self::of(values)
    }
}

impl InstanceContext {

    /// The record of the instance of `program` whose environment is `env` and which binds
    /// `slots` things; nothing is published yet.
    pub fn new(env: Value, program: ProgramId, slots: usize) -> Box<Self> {
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

    /// The body the record at `address` publishes for function `entry`, or 0 when it publishes
    /// none for it, or `entry` is `NO_ENTRY`. A table row's `body` is this value (`TableTarget`).
    pub fn published_body(address: usize, entry: u32) -> usize {
        if address == 0 || entry == u32::MAX {
            return 0;
        }
        // SAFETY: a row names a record its function keeps alive (the function's environment holds
        // the record's cell), so the address is a live record.
        let record = unsafe { &*(address as *const InstanceContext) };
        // SAFETY: `compiled` is null or the artifact `publish` retained, which lives as long as
        // the record (`retained`).
        let Some(compiled) = (unsafe { record.compiled.as_ref() }) else {
            return 0;
        };
        compiled.table.get(entry as usize).copied().unwrap_or(0)
    }
}

impl JsEntryFacts {

    #[inline]

    pub fn of(program: &ResidualProgram, function: &Function, priced: bool) -> Self {
        let has_calls = function.instructions().any(|instruction|
            instruction.op().control_flow_layout() == crate::bytecode::ControlFlowLayout::Call);
        Self { candidate: function_candidate(function, program) && (!priced || has_calls || pays_native_entry(function)) }
    }
}

impl LaneDependencies {

    /// Makes `code` retire when a fact it took changes: a slot's observation, a holder
    /// shape's departure, or one of its Stable facts or its direct callees'; `false` where a reader table is full.
    #[inline]
    pub fn subscribe(&self, stable: &StableFacts, code: &Rc<dyn crate::code_dependencies::DependentCode>,
        facts: &mut crate::vm::tier_queries::FactWatch<'_>) -> bool {
        self.slots.iter().all(|dependency| facts.subscribe_slot_observation(*dependency, code))
            && self.departure_shapes.iter().all(|shape| facts.subscribe_shape(*shape, code))
            && subscribe_stable(stable, code, facts)
            && self.subscribe_callees(code, facts)
    }

    /// The programs whose root constants the code folds, its own and its direct callees' and theirs
    /// in turn (`StableFacts::root_program`), each once: `ProgramStore::subscribe_root_constants`
    /// subscribes the code to them.
    pub fn root_programs(&self, stable: &StableFacts) -> Vec<crate::vm::program_store::ProgramId> {
        let mut programs: Vec<crate::vm::program_store::ProgramId> = Vec::new();
        programs.extend(stable.root_program);
        for callee in &self.callees {
            for program in callee.dependencies.root_programs(&callee.stable) {
                if !programs.contains(&program) { programs.push(program); }
            }
        }
        programs
    }
}

impl LaneMethod {

    /// Sizes this thread's scratch window for it (`BodyRunContext::scratch`): the one event that
    /// grows the window, so a run never checks it. Its publication does it
    /// (`ProgramStore::publish_lane_method`), on the thread that runs it.
    #[inline]
    pub fn reserve_scratch(&self) {
        reserve_lane_scratch(self.scratch_width);
    }

    /// Releases the capacity its vectors hold past their length, as its publication keeps them for
    /// as long as the body is published (`ProgramStore::publish_lane_method`).
    #[inline]
    pub fn trim_published(&mut self) {
        self.dependencies.trim();
        self.inlined.shrink_to_fit();
        self._records.shrink_to_fit();
    }
}

impl LaneStack {

    /// Forget resolved call targets, as the general loop's call target cache is reset.
    #[inline]
    pub fn clear_script_targets(&mut self) {
        self.script_targets.clear();
        self.cross_targets.clear();
        *self.function_targets = [EMPTY_SCRIPT_CALL_TARGET; FUNCTION_TARGETS];
        *self.cross_function_targets = [EMPTY_SCRIPT_CALL_TARGET; FUNCTION_TARGETS];
        self.read_memos.clear();
    }

    /// Drops the callees a collection swept from the sites' records before any allocation
    /// reuses their records: a body inlining one would take another function for it.
    #[inline]
    pub fn forget_dead_callees(&mut self, live: impl Fn(Value) -> bool) {
        self.wasm_export = None;
        self.js_import = None;
        self.bound_call = None;
        self.forget_noted();
        self.call_sites.get_mut().retain(|_, targets| { targets.retain(|target| target.identity().is_none_or(&live)); !targets.is_empty() });
    }

    /// Retires everything this stack resolved before the collection that ended under `key`
    /// (the new epoch and the realm): call targets, function targets and read memos name
    /// callees, environments and storages the collection may have swept, whose heap indices
    /// the next allocations reuse. The collector calls it, so a lane run that continues past
    /// any call out that collected (a native such as `gc()`, a host callback, an allocation
    /// that made a collection due) never admits a target of a swept closure whose index a
    /// new closure took: its depth-0 captures would read the dead closure's storage.
    #[inline]
    pub fn retire_after_collection(&mut self, key: (u64, Value)) {
        self.enter_generation(key);
    }

    /// Notes the program the lane now runs (`Vm::activate_program`).
    #[inline(always)]
    pub fn enter_program(&mut self, program: ProgramId) {
        self.admission = Admission(Admission::new(self.generation, program).0 | self.refused);
    }

    /// Whether no cached target or memo admits a call (`Admission::REFUSED`): while a
    /// construction's callee is pending (`Vm::construct_target`), under which a lane call leaves
    /// the lane, so the cached paths need not test it (`Vm::refresh_call_refusal`).
    #[inline]
    pub fn set_refused(&mut self, refused: bool) {
        self.set_condition(Admission::REFUSED, refused);
    }

    /// Every activation, the plain ones counted without making them records: what the
    /// activation limit bounds.
    #[inline(always)]
    pub fn activation_count(&self) -> usize {
        self.frames_below_plain() + self.plain.depth.get()
    }

    /// Every activation, records and plain headers alike (`activation_count`): whether any is.
    #[inline(always)]
    pub fn has_frames(&self) -> bool { self.activation_count() != 0 }

    /// The chain of the activation at `depth`, the innermost, moved off the stack to the end
    /// of `record`'s, the suspending frame's (`FrameRecord::with_objects`); usually none.
    #[inline(always)]
    pub fn take_with_objects(&mut self, depth: usize, record: &mut crate::vm::activation::FrameRecord) {
        if self.with_objects.last().is_some_and(|(owner, _)| *owner >= depth) {
            let first = self.with_objects.partition_point(|(owner, _)| *owner < depth);
            record.with_objects_mut().extend(self.with_objects.drain(first..).map(|(_, object)| object));
        }
    }

    /// Leaves the `with` scopes of the activation at `depth`, the innermost, past the first
    /// `entered` of its chain: what a handler's entry unwinds (`Vm::unwind_handler_scopes`).
    #[inline]
    pub fn unwind_with_objects(&mut self, depth: usize, entered: usize) {
        let first = self.with_objects.partition_point(|(owner, _)| *owner < depth);
        self.with_objects.truncate(first + entered);
    }

    /// The iterators the activation at `depth`, the innermost, pushed (its entries, the last),
    /// moved to the end of `open`, the suspending frame's (`FrameRecord::active_iterators`).
    #[inline(always)]
    pub fn take_iterators(&mut self, depth: usize, open: &mut Vec<crate::vm::ActiveIterator>) {
        if self.iterators.last().is_some_and(|(owner, _)| *owner >= depth) {
            let first = self.iterators.partition_point(|(owner, _)| *owner < depth);
            open.extend(self.iterators.drain(first..).map(|(_, iterator)| iterator));
        }
    }

    /// The home of parameter `slot` in the innermost window activation of `code` whose `arguments`
    /// binding holds `object`, where its parameters are.
    ///
    /// The object names its activation's depth (`Vm::record_window_owner`), so the lookup reads
    /// one activation; an object whose activation ended, or whose depth another activation took,
    /// fails the binding check.
    pub fn window_parameter(
        &self,
        heap: &crate::heap::Heap,
        code: &ResidualProgram,
        object: Value,
        slot: usize,
    ) -> Option<ParameterHome> {
        let (body, entries) = self.window_at(code, heap.object_ref(object)?.record().window_depth()?)?;
        // SAFETY: an environment activation's window holds a local for each slot the function binds.
        let arguments = unsafe { ParameterHome::of(entries, body, usize::from(body.arguments_slot?)) }.read(crate::vm::tier_queries::HeapReads::of(heap));
        (arguments == object).then(|| unsafe { ParameterHome::of(entries, body, slot) })
    }

    /// Appends to `out` every value the stack's activations hold; `programs` hold the liveness
    /// of the Wasm activations' registers.
    #[inline]
    pub fn extend_roots(&self, programs: &ProgramStore, out: &mut Vec<Value>) {
        self.extend_window_roots(programs, out);
        out.extend(self.frames().filter_map(|frame| frame.script()).map(|script| script.callee));
        out.extend(self.with_object_roots());
        out.extend(self.side_roots());
        out.extend(self.steps.iter().flat_map(LaneArrayStep::roots));
        for (_, record) in &self.records {
            record.extend_roots(out);
        }
    }
}

impl LaneView {

    /// Admits every record as a lane entry: a body compiled ahead (`AheadImages`) leaves at any record it does not
    /// make, and the lane takes the record on there (`ExecutionMode::NativeOnly` has nothing else to run it with). The
    /// derived set keeps only the records a straight-line run from a jump target repays, which an image body need not be.
    pub fn admit_every_record(&mut self) {
        self.entries.fill(u64::MAX);
    }

    /// Its lane-body tier state, made on first use.
    #[inline]
    pub fn tier(&self) -> &LaneTier {
        self.tier.get_or_init(Box::default)
    }

    /// The name of its body's state, for the diagnostics report (`LaneBodyStats::report`).
    #[inline]
    pub fn body_state_in_lane(&self) -> &'static str {
        // SAFETY: as `lane_body`.
        match unsafe { &*self.tier().tier_lane.get() } {
            LaneBody::Unspecialized { .. } => "unspecialized",
            LaneBody::Compiling { .. } => "compiling",
            LaneBody::Specialized { .. } => "specialized",
            LaneBody::Declined => "declined",
        }
    }

    /// Its body's state, and whether the tier claimed its prologue and is still warming, for the
    /// diagnostics report of a view that never compiled (`QUENCH_LANE_STATE`).
    #[inline]
    pub fn body_detail_in_lane(&self) -> (&'static str, bool, bool) {
        (self.body_state_in_lane(), self.start.get() == self.prologue.get().cast_const(), self.warming())
    }

    /// Clears the global-object hints of its by-name loads and stores: a global
    /// lexical name was declared (`Vm::note_global_lexical_names`).
    #[inline]
    pub fn clear_name_hints(&self) {
        self.clear_hints(&self.name_records);
    }

    /// Gives every record whose handler `map` names another that one, the loop headers' held
    /// handlers too: a retired `this` trust's rows take their checked forms.
    pub fn rewrite_handlers(&self, map: &impl Fn(*const ()) -> Option<*const ()>) {
        for cell in self.records.cells.iter() {
            let record = std::cell::UnsafeCell::raw_get(cell);
            // SAFETY: the lane runs on the view's thread, and no reference to a handler field is live.
            unsafe {
                if let Some(checked) = map((*record).handler) { (*record).handler = checked; }
            }
        }
        for site in &self.loops {
            if let Some(checked) = map(site.header_handler.get()) { site.header_handler.set(checked); }
        }
    }

    /// Starts activations at the first record again, once the tier declines the function.
    #[inline]
    pub fn release_from_tier(&self) {
        self.start.set(self.records());
    }

    /// OSR tier coverage and outcomes, for opt-in JIT diagnostics.
    pub fn osr_diagnostics(&self) -> OsrDiagnostics {
        let mut sites = self.loops.iter().map(|site| {
            let call_boundary_pcs = site.active_region().map_or_else(
                || site.call_boundary_pcs.to_vec(),
                LoopRegion::call_boundary_pcs,
            );
            let deferred_call = !call_boundary_pcs.is_empty();
            OsrSiteDiagnostics {
                header_pc: site.header,
                backedge_pc: site.backedge,
                status: if site.promotion_only {
                    OsrSiteStatus::PromotionOnly
                } else if site.analysis_failure.is_some_and(|failure| failure.is_call_boundary()) {
                    OsrSiteStatus::CallBoundary
                } else if site.analysis_failure.is_some() {
                    OsrSiteStatus::Rejected
                } else {
                    OsrSiteStatus::Admitted
                },
                failure: site.analysis_failure,
                call_boundary_pcs,
                admitted: site.is_admitted(),
                deferred_call,
                tier_threshold_reached: site.is_hot(),
                lane_backedge_count: site.lane_backedge_count.get(),
                analysis_rejection: site.analysis_failure.map(|failure| failure.reason),
                compile_attempts: site.compile_attempts.get(),
                compile_successes: site.compile_successes.get(),
                native_entry_attempts: site.native_entry_attempts.get(),
                native_region_runs: site.native_region_runs.get(),
                entry_guard_misses: site.entry_guard_misses.get(),
                exits: site.native_exits.get(),
                widenings: site.widenings.get(),
                declines: std::array::from_fn(|index| site.declines[index].get()),
            }
        }).collect::<Vec<_>>();
        sites.sort_unstable_by_key(|site| (site.header_pc, site.backedge_pc));
        OsrDiagnostics {
            plain_backward_jump_sites: self.plain_backward_jump_sites,
            admitted_sites: self.loops.iter().filter(|site| site.is_admitted()).count(),
            promotion_only_sites: self.loops.iter().filter(|site| site.promotion_only).count(),
            compile_attempts: self.loops.iter().map(|site| site.compile_attempts.get()).sum(),
            compile_successes: self.loops.iter().map(|site| site.compile_successes.get()).sum(),
            currently_compiled_sites: self.loops.iter().filter(|site| site.tier.borrow().is_published()).count(),
            native_entry_attempts: self.loops.iter().map(|site| site.native_entry_attempts.get()).sum(),
            native_region_runs: self.loops.iter().map(|site| site.native_region_runs.get()).sum(),
            entry_guard_misses: self.loops.iter().map(|site| site.entry_guard_misses.get()).sum(),
            tier_ups: self.loops.iter().map(|site| site.tier_ups.get()).sum(),
            body_runs: self.loops.iter().map(|site| site.body_runs.get()).sum(),
            exits: self.loops.iter().map(|site| site.native_exits.get()).sum(),
            widenings: self.loops.iter().map(|site| site.widenings.get()).sum(),
            declines: std::array::from_fn(|index| self.loops.iter().map(|site| site.declines[index].get()).sum()),
            analysis_rejections: self
                .osr_rejections
                .as_deref()
                .unwrap_or_default()
                .to_vec(),
            sites,
        }
    }
}

impl NativeJsProgram {



    /// The canonical metadata used by host job checkpoints and source reporting.
    #[inline]
    pub fn residual(&self) -> &ResidualProgram {
        &self.program
    }
}

impl NativeRoots {

    #[inline]

    pub fn new() -> Box<Self> {
        // Zeroed words, not `Value::UNDEFINED`, in a mapping of their own: untouched zero pages,
        // so the buffer costs memory only as deep as roots are ever pushed. (A zeroed allocation
        // below the allocator's mapping threshold is written zero byte by byte on a worker thread's
        // arena: 512 KiB set and faulted at every VM's start.) Only `..depth` is read, and every
        // slot there was written first.
        const _: () = assert!(size_of::<Value>() == size_of::<u64>() && align_of::<Value>() == align_of::<u64>());
        let slots = super::site_table::storage::map_zeroed(NATIVE_ROOT_CAPACITY * size_of::<u64>());
        Box::new(Self {
            depth: 0,
            capacity: NATIVE_ROOT_CAPACITY as u64,
            slots: slots.as_ptr().cast::<Value>(),
        })
    }

    /// The environments held now: the roots of a collection.
    #[inline]
    pub fn live(&self) -> &[Value] {
        // SAFETY: `slots` is the buffer `new` leaked, of `capacity` values, and every push keeps
        // `depth` within it (`CAPACITY` checks).
        unsafe { std::slice::from_raw_parts(self.slots, self.depth as usize) }
    }
}

impl OperandKinds {
    /// An element access met a present dense element (`GetIndex`, `SetIndex`).
    pub const DENSE: Self = Self(1 << 6);
    /// An element read met a number of a typed array.
    pub const TYPED: Self = Self(1 << 7);
}

impl OsrFacts {

    /// Releases the capacity its vectors hold past their length, before the region is published.
    #[inline]
    pub fn trim(&mut self) {
        self.shape_dependencies.shrink_to_fit();
        self.departure_shapes.shrink_to_fit();
        self.slot_dependencies.shrink_to_fit();
        self.calls.trim();
    }

    /// Subscribes `code` to the facts; whether all hold. A receiver shape is guarded at each
    /// field use, so only a holder shape's departure discards it; a receiver shape ID still
    /// retires it at compaction (`shape_dependencies`).
    #[inline]
    pub fn subscribe(&self, code: &Rc<dyn crate::code_dependencies::DependentCode>, facts: &mut crate::vm::tier_queries::FactWatch<'_>) -> bool {
        self.departure_shapes.iter().all(|shape| facts.subscribe_shape(*shape, code))
            && self.slot_dependencies.iter().all(|dependency| facts.subscribe_slot_observation(*dependency, code))
            && self.calls.subscribe(&self.stable, code, facts)
    }

    /// The programs whose root constants the region's calls fold (`LaneDependencies::root_programs`),
    /// which its code subscribes to (`ProgramStore::subscribe_root_constants`).
    #[inline]
    pub fn root_programs(&self) -> Vec<crate::vm::program_store::ProgramId> {
        self.calls.root_programs(&self.stable)
    }
}

impl ParkedLane {

    /// Appends to `out` every value the parked run holds.
    #[inline]
    pub fn extend_roots(&self, programs: &crate::vm::program_store::ProgramStore, out: &mut Vec<Value>) {
        self.stack.extend_roots(programs, out);
        let [this, env] = self.innermost;
        crate::vm::activation::extend_this_env_roots(out, this, env);
    }
}

impl WindowScope {

    /// The dynamic bindings the activation's record holds, until a closure's environment takes
    /// them over (`captured`); none for an activation without a record.
    #[inline]
    pub fn record_bindings(&self) -> Option<&mut Vec<(Atom, Value)>> {
        // SAFETY: the record is the running activation's own, which nothing else accesses.
        (!self.captured && !self.record.is_null()).then(|| unsafe { (*self.record).dynamic_bindings_mut() })
    }

    /// The window's locals.
    #[inline]
    pub fn locals(&self) -> &[Value] {
        // SAFETY: the window holds `Function::locals` locals after its entries, which nothing
        // else accesses while a step of the activation runs.
        unsafe { std::slice::from_raw_parts(self.locals, self.local_count) }
    }

    /// The window's locals, to write.
    #[inline]
    pub fn locals_mut(&mut self) -> &mut [Value] {
        // SAFETY: as `locals`.
        unsafe { std::slice::from_raw_parts_mut(self.locals, self.local_count) }
    }

    /// The register file's `register`.
    #[inline]
    pub fn register(&self, register: u16) -> Value {
        self.registers.get(register)
    }

    #[inline]

    pub fn set_register(&self, register: u16, value: Value) {
        self.registers.set(register, value);
    }

    /// Replaces the binding site the activation's accesses are at, which `site` names.
    #[inline]
    pub fn replace_site(&mut self, site: Option<u32>) -> Option<u32> {
        std::mem::replace(&mut self.site, site)
    }
}

/// The `abi` of a record whose artifact cannot be called natively by another instance's code: its
/// memory can change length under a caller that does not pass the current one.
pub const ABI_NOT_DIRECT: u64 = u64::MAX;

/// How many roots native code may hold at once. A push past it traps `CallStackExhausted` (for a
/// compiled frame) or leaves through the VM (for an instance switch), which roots its own.
pub const NATIVE_ROOT_CAPACITY: usize = 65536;

/// Where a parameter lives in an environment window: its local among the window's locals, or,
/// for a parameter a closure captured once the window's environment exists, its slot there. A
/// captured parameter rides the window's locals until a closure makes the environment, which takes
/// it over (`window_environment`).
#[derive(Clone, Copy)]
pub enum ParameterHome {
    Local(*mut Value),
    Environment(Value, usize),
}

impl Admission {

    /// Where the program sits in the word: above the generation.
    pub const PROGRAM_SHIFT: u32 = ScriptGeneration::BITS;

    /// A bit no target's or memo's admission has: the lane's carries it while a condition
    /// no cached call may run under holds (`LaneStack::refuse_cached_calls`), so every
    /// admission fails, and the paths that resolve again decide.
    pub const REFUSED: u64 = 1 << 63;
    /// The program's bits above `PROGRAM_SHIFT`, below the flag bits.
    pub const PROGRAM_MASK: u64 = (1 << (Self::DIAGNOSED.trailing_zeros() - Self::PROGRAM_SHIFT)) - 1;
}

impl AheadImages {
}

impl CallTarget {

    #[inline]

    pub fn identity(self) -> Option<Value> {
        match self.callee { CalleeBinding::Identity(value) | CalleeBinding::Forwarded { callee: value, .. } => Some(value), _ => None }
    }
}

impl CodeMemory {

    /// The bytes the zone holds for the body: its length rounded to the zone's granule.
    #[inline]
    pub fn length(&self) -> usize {
        self.length
    }
}

impl CompiledProgram {

    /// Module function `index`'s compiled code, if this artifact holds it.
    #[inline]
    pub fn function(&self, index: usize) -> Option<&CompiledFunction> {
        self.functions.get(*self.positions.get(index)? as usize)
    }
}

impl InheritedConstant {

    #[inline]

    pub fn values(&self) -> &[(u32, Value)] {
        &self.values[..usize::from(self.count)]
    }
}

impl JsEntryFacts {

    /// Whether every fact is unset (`Self::default`).
    #[inline]
    pub const fn is_unset(&self) -> bool {
        !self.candidate
    }
}

impl LaneArrayStep {

    /// The values the step keeps alive: its iteration's and the entry's environment and receiver.
    #[inline]
    pub fn roots(&self) -> impl Iterator<Item = Value> {
        self.iteration.values(self.element).into_iter().chain([self.entry.env, self.entry.this])
    }
}

impl LaneBodyStats {

    /// Runs of the body, however they ended (`ends`): the tier census's entries.
    #[inline]
    pub fn runs(&self) -> u64 {
        self.ends.iter().map(|end| u64::from(end.get())).sum()
    }

    /// The report line of a view that compiled a body, `QUENCH_LANE_BODY {...}`, with the
    /// state its body ended the run in.
    pub fn report(&self, program: usize, state: &str, op_at: &dyn Fn(u32, u32) -> Option<String>) -> Option<String> {
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

impl LaneDependencies {

    /// Releases the capacity a publication's vectors hold past their length: they are built by
    /// `collect` and `dedup`, and kept for as long as the body is, its callers' too.
    #[inline]
    pub fn trim(&mut self) {
        self.slots.shrink_to_fit();
        self.departure_shapes.shrink_to_fit();
        self.callees.shrink_to_fit();
    }

    /// Subscribes `code` to the Stable facts of its direct callees' bodies, and theirs in turn.
    #[inline]
    pub fn subscribe_callees(&self, code: &Rc<dyn crate::code_dependencies::DependentCode>,
        facts: &mut crate::vm::tier_queries::FactWatch<'_>) -> bool {
        self.callees.iter().all(|callee| subscribe_stable(&callee.stable, code, facts)
            && callee.dependencies.subscribe_callees(code, facts))
    }
}

impl LaneStack {

    #[inline]

    pub fn forget_noted(&self) {
        self.noted.iter().for_each(|noted| noted.set(None));
        self.linked.iter().for_each(|linked| linked.set((0, Value::UNDEFINED)));
    }

    #[inline(always)]
    pub fn set_condition(&mut self, bit: u64, holds: bool) {
        self.refused = self.refused & !bit | if holds { bit } else { 0 };
        self.admission = Admission(self.admission.0 & !Admission::LANE_BITS | self.refused);
    }

    /// The frames below the innermost plain headers: the records and the headers they sealed.
    #[inline(always)]
    pub fn frames_below_plain(&self) -> usize {
        // SAFETY: reads the length alone.
        unsafe { (*self.activation_records.get()).len() + self.sealed.get() }
    }

    /// Every activation, outermost first, as `activations().iter()` would yield them once
    /// spilled, without spilling: the walk's frames (`walk_innermost`), reversed. The walk reads
    /// no record a spill could write during it: a reader calls no `activations` while it walks.
    #[inline]
    pub fn frames(&self) -> impl Iterator<Item = ActivationView> + '_ {
        let mut frames: Vec<ActivationView> = self.frames_innermost().collect();
        frames.reverse();
        frames.into_iter()
    }

    /// The values the side records keep alive: the skipped forwarders' callees and environments.
    #[inline]
    pub fn side_roots(&self) -> impl Iterator<Item = Value> + '_ {
        // SAFETY: a root walk changes nothing.
        unsafe { &*self.sides.get() }.iter().filter_map(|side| side.payload.forwarder()).flat_map(|forwarder| [forwarder.callee, forwarder.env])
    }

    /// Every object a chain holds, which the collector keeps alive.
    #[inline]
    pub fn with_object_roots(&self) -> impl Iterator<Item = Value> + '_ {
        self.with_objects.iter().map(|(_, object)| *object)
    }

    /// The generation of a lane run under `key`: the current one while the key holds, else
    /// a new one, which retires every target resolved before.
    #[inline]
    pub fn enter_generation(&mut self, key: (u64, Value)) {
        // The bounds this run's native calls are checked against (`Vm::call_native_from_lane`).
        self.native_stack = crate::stack::StackLimit::here();
        if self.generation == NO_SCRIPT_GENERATION || self.generation_key != key {
            self.generation_key = key;
            self.advance_generation();
        }
    }

    /// The function of the environment window activation at `depth` and where its entries start.
    #[inline]
    pub fn window_at<'a>(&self, code: &'a ResidualProgram, depth: usize) -> Option<(&'a Function, *mut Value)> {
        let activation = self.frame(depth)?;
        activation.script().filter(|script| script.environment)?;
        let body = code.function_get(activation.function() as usize)?;
        // SAFETY: an environment activation's window holds its entries after its registers.
        Some((body, unsafe { activation.window_ptr().add(usize::from(body.registers)) }))
    }

    /// The values of the windows on the stack's own values: each script activation's registers,
    /// up to the next window's context words (`lane_values::CONTEXT`, which end every span, the
    /// top's included), with its own receiver and environment words, and each Wasm activation's
    /// live registers. The capture word is an address, not a value, and is never traced. A
    /// window of another register file (`HEAP_WINDOW`) belongs to the record that owns it,
    /// which its callee keeps alive.
    pub fn extend_window_roots(&self, programs: &ProgramStore, out: &mut Vec<Value>) {
        let (span, top) = (self.values.span(), self.values.top().addr());
        let mut windows = self.frames().enumerate().filter(|(_, activation)| span.contains(&activation.window_word())).peekable();
        while let Some((depth, activation)) = windows.next() {
            let end = windows.peek().map_or(top, |(_, next)| next.window_word());
            let len = end.wrapping_sub(activation.window_word()) / std::mem::size_of::<Value>() - lane_values::CONTEXT;
            // SAFETY: the values `base..top` are initialized, and the span lies within them.
            let window = unsafe { std::slice::from_raw_parts(activation.window_ptr(), len) };
            match activation.kind() {
                ActivationKind::Script(_) => {
                    let registers = Registers(activation.window_ptr());
                    extend_this_env_roots(out, registers.this(), registers.env());
                    out.extend_from_slice(window);
                }
                ActivationKind::Wasm { .. } => self.extend_wasm_roots(programs, depth, window, out),
                // An export's activation also holds the instance environment it runs over.
                ActivationKind::WasmEntry { instance, .. } => {
                    out.push(instance);
                    self.extend_wasm_roots(programs, depth, window, out);
                }
                ActivationKind::Native => {}
            }
        }
    }
}

impl LaneView {

    /// Whether it has no body yet and counts entries toward one, its first or a next.
    #[inline]
    pub fn warming(&self) -> bool {
        // SAFETY: as `lane_body`.
        matches!(unsafe { &*self.tier().tier_lane.get() }, LaneBody::Unspecialized { .. }) || self.compiling()
    }

    #[inline]

    pub fn clear_hints(&self, pcs: &[u32]) {
        for &pc in pcs {
            let record = std::cell::UnsafeCell::raw_get(&self.records[pc as usize]);
            // SAFETY: each listed record's operand is `field`, and it is no loop's header
            // (`holds_field_hint`); its handler reads the hint whole, between runs that write it.
            unsafe {
                (*record).operand.field.shape = crate::vm::NO_FIELD_RECEIVER;
                (*record).call_link.set(NO_CALL_LINK);
            }
        }
    }

    #[inline] pub fn records(&self) -> Ip {
        self.records.base()
    }
}

impl LoopRegion {


    #[inline]


    pub fn call_boundary_pcs(&self) -> Vec<u32> {
        self.instructions
            .iter()
            .enumerate()
            .filter(|(_, instruction)| crate::staged::calls_ordinary(instruction.op()))
            .map(|(offset, _)| self.header.saturating_add(offset as u32))
            .collect()
    }
}

impl LoopSite {

    #[inline]

    pub fn active_region(&self) -> Option<&LoopRegion> {
        self.inline_region.get().and_then(|region| region.as_deref()).or_else(|| self.region.get())
    }

    #[inline]

    pub fn is_admitted(&self) -> bool {
        !self.promotion_only
            && self.analysis_failure.is_none_or(|failure| failure.is_call_boundary())
    }
}

impl RegionAnalysisFailure {

    /// The region stops only at a call, which may run inlined once the site is hot.
    #[inline]
    pub fn is_call_boundary(self) -> bool {
        matches!(self.reason, RegionRejection::UnsupportedInstruction(opcode) if is_call_rejection(opcode))
    }
}

impl Registers {

    #[inline(always)]
    pub fn get(self, register: u16) -> Value {
        // SAFETY: the view admits only register fields below
        // Function.registers, and frame setup sizes the register file to it.
        unsafe { *self.0.add(usize::from(register)) }
    }

    #[inline(always)]
    pub fn set(self, register: u16, value: Value) {
        // SAFETY: as for `get`; no reference into the register file is live.
        unsafe { *self.0.add(usize::from(register)) = value }
    }
}

impl ScriptTargets {

    #[inline]

    pub fn clear(&mut self) {
        self.entries.fill(EMPTY_SCRIPT_CALL_TARGET);
        self.displaced = 0;
    }
}

/// One frame, as every reader and every return reads it (one-frame-one-call.md F4 (3),
/// Ledger (3)): the words every frame has (its native prefix, function, call record and caller
/// state), what kind it is, and what its side record carries. A plain header's are read from
/// its header words and side record (`LaneStack::plain_frame`), building no record; a record's
/// are its own (`ActivationView::of_record`). The walk (`frame_walk`), the pop
/// (`LaneStack::pop_activation`) and the innermost's read (`LaneStack::top_frame`) produce it,
/// and a frame that must become a record again (a spill, a materialization) is `record`. A
/// frame is a copy: writers edit it where it is.
#[derive(Clone, Copy)]
pub struct ActivationView {
    /// Its frame prefix: a record's own, a header's with no callee or body but its window.
    pub callee: *const DirectCallee,
    pub this: u64,
    pub body: usize,
    pub window: usize,
    pub function: u32,
    pub call: Ip,
    pub caller_base: Ip,
    pub caller_registers: Registers,
    pub caller_program: ProgramId,
    pub kind: ActivationKind,
    /// What its completion carries, from its side record (`SidePayload`).
    pub payload: SidePayload,
}

impl Admission {
    /// A bit the lane's admission carries while the tier's diagnostics count every entry
    /// (`LaneStack::set_diagnosed`), which a proved call does not: no entry proved before they
    /// were enabled admits a proved call, and none is proved while they are. Unlike the
    /// conditions, targets and memos made under them carry it too (`current_admission`), so
    /// the unproved paths still hit.
    pub const DIAGNOSED: u64 = 1 << 60;
    /// The bits the lane's admission takes from `LaneStack::refused`.
    pub const LANE_BITS: u64 = Self::CONDITIONS | Self::DIAGNOSED;
}

impl AheadImages {

    #[inline]

    pub fn of(image: AotImage) -> Self {
        Self { image, known: Default::default(), linked: None, placed: None, facts: std::cell::OnceCell::new() }
    }

    /// Whether the image has no functions.
    #[inline]
    pub fn is_empty(&self) -> bool { self.image.functions.is_empty() }
}

impl LaneStack {

    /// Every frame, innermost first, where it is (`walk_innermost`). A reader calls no
    /// `activations` while it walks.
    #[inline]
    pub fn frames_innermost(&self) -> impl Iterator<Item = ActivationView> + '_ {
        self.walk_innermost().map(|(frame, _)| frame)
    }

    /// The activation `depth` above the outermost, as `activations()[depth]` would read it once
    /// spilled; `None` past the innermost.
    #[inline]
    pub fn frame(&self, depth: usize) -> Option<ActivationView> {
        let above = self.activation_count().checked_sub(depth + 1)?;
        self.frames_innermost().nth(above)
    }

    /// Takes a new generation, which retires every target and memo resolved before
    /// (`enter_generation`, `Vm::enter_with_scope`).
    pub fn advance_generation(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        // An in-place native row's admitted callee names a target of the generation retired here.
        self.native_identities.forget();
        if self.generation == NO_SCRIPT_GENERATION {
            // A wrapped number could name entries of a generation long past.
            self.clear_script_targets();
            self.generation = NO_SCRIPT_GENERATION + 1;
        }
        self.admission = Admission(Admission::new(self.generation, self.admission.program()).0 | self.refused);
    }

    /// The registers of the Wasm activation at `depth` that `window` holds and the function's
    /// liveness keeps at the record it is at: the record its callee resumes at, or, for the
    /// innermost activation or one whose callee is native, the record it last noted
    /// (`note_wasm_pc`). Its registers hold Wasm values the collector reads as it reads a frame's.
    /// A function with no masks has its whole window scanned, so its window is initialized whole.
    pub fn extend_wasm_roots(&self, programs: &ProgramStore, depth: usize, window: &[Value], out: &mut Vec<Value>) {
        let Some(activation) = self.frame(depth) else { return out.extend_from_slice(window) };
        let (program, noted) = match activation.kind() {
            ActivationKind::Wasm { program, pc, .. } | ActivationKind::WasmEntry { program, pc, .. } => (program, pc as usize),
            ActivationKind::Native | ActivationKind::Script(_) => return out.extend_from_slice(window),
        };
        let call_pc = self.frame(depth + 1).and_then(|above| above.caller_pc()).unwrap_or(noted);
        let masks = programs.residual(program).and_then(|program| program.register_root_masks(activation.function()));
        let raw = programs.raw_register_masks(program, activation.function());
        match masks {
            Some(masks) => masks.extend_live_except(call_pc, raw.as_deref(), window, out),
            None => out.extend_from_slice(window),
        }
    }
}

impl ParameterHome {

    /// The home of parameter `slot` of `function` in the window whose entries start at `entries`.
    ///
    /// # Safety
    /// `entries` starts the entries of a live environment window of `function`
    /// (`activate_environment_in_lane`), which holds a local for each of its slots.
    pub unsafe fn of(entries: *mut Value, function: &Function, slot: usize) -> Self {
        // SAFETY: as the caller guarantees.
        let environment = unsafe { *entries.add(usize::from(WINDOW_ENVIRONMENT)) };
        if !environment.is_deleted() && function.local_slot_uses_environment(slot) {
            return Self::Environment(environment, slot);
        }
        // SAFETY: as above.
        Self::Local(unsafe { entries.add(usize::from(WINDOW_LOCALS) + slot) })
    }

    /// Stores `value` to the parameter's binding, an environment's after its barrier.
    pub fn write(self, vm: &mut Vm, value: Value) {
        match self {
            // SAFETY: a local of a live window.
            Self::Local(binding) => unsafe { *binding = value },
            Self::Environment(environment, slot) => {
                vm.heap.remember_cell(environment);
                if let Some(binding) = vm.heap.environment_slot_mut(environment, slot) {
                    *binding = value;
                }
            }
        }
    }
}

impl Records {

    /// The first record.
    #[inline(always)]
    pub fn base(&self) -> Ip {
        self.first
    }
}

impl Registers {

    /// The frame's receiver, the context word just below register 0 (`FRAME_CONTEXT`): every
    /// register file (`RegisterFile`) and lane window (`LaneValues`) has it there.
    #[inline(always)]
    pub fn this(self) -> Value {
        // SAFETY: every frame's registers follow its context words.
        unsafe { *self.0.sub(CONTEXT_THIS) }
    }

    /// The frame's environment, the context word below its receiver: a lane activation's
    /// closure environment, `DELETED` for a VM frame, whose environment its frame holds.
    #[inline(always)]
    pub fn env(self) -> Value {
        // SAFETY: as for `this`.
        unsafe { *self.0.sub(CONTEXT_ENV) }
    }
}

impl SidePayload {

    /// A forwarder frame's skipped constructor; `None` for a native frame's, its descriptor's.
    #[inline]
    pub fn forwarder(self) -> Option<SkippedForwarder> {
        match self { Self::Forwarder(skipped) => Some(skipped), _ => None }
    }
}

impl ActivationView {

    /// The function it runs.
    #[inline(always)] pub fn function(&self) -> u32 { self.function }
    /// What kind of activation it is (a plain header is a `Script` activation that completes
    /// plainly, returns to a native through the callback trampoline, or steps an array
    /// iteration through the array steps' trampoline).
    #[inline(always)] pub fn kind(&self) -> ActivationKind { self.kind }
    /// Its prefix's WINDOW word, heap mark included (`Activation::window_word`).
    #[inline(always)] pub fn window_word(&self) -> usize { self.window }
    /// The address of its first register (`Activation::window_ptr`).
    #[inline(always)]
    pub fn window_ptr(&self) -> *mut Value { (self.window & !HEAP_WINDOW) as *mut Value }

    /// Its JavaScript frame state; `None` for Wasm and an unlinked native frame.
    #[inline(always)]
    pub fn script(&self) -> Option<ScriptActivation> {
        match self.kind {
            ActivationKind::Script(script) => Some(script),
            ActivationKind::Native | ActivationKind::Wasm { .. } | ActivationKind::WasmEntry { .. } => None,
        }
    }

    /// The pc of the call record it was entered from, in its caller's body; `None` for a native
    /// frame the lane has not linked, which keeps no link state.
    #[inline]
    pub fn caller_pc(&self) -> Option<usize> {
        // SAFETY: a linked activation's call record lies in its caller's body.
        (!matches!(self.kind, ActivationKind::Native)).then(|| unsafe { self.call.offset_from(self.caller_base) } as usize)
    }
}

impl Admission {
    /// Every bit only the lane's admission carries.
    pub const CONDITIONS: u64 = Self::REFUSED | Self::NO_SCRIPT_CALLS;

    #[inline(always)]
    pub const fn program(self) -> ProgramId {
        ProgramId::from_raw(((self.0 >> Self::PROGRAM_SHIFT) & Self::PROGRAM_MASK) as u32)
    }
}

impl LaneStack {

    /// Every frame innermost first, where it is (`Walk`). The side records, seals and
    /// records change only through `&mut self` and `spill_plain`, which no walk calls: a reader
    /// calls no `activations` while it walks.
    #[inline]
    pub fn walk_innermost(&self) -> Walk<'_> {
        // SAFETY: the walk's contract above.
        let (records, sides, segments) = unsafe { (&self.records_unspilled()[..], (&*self.sides.get()).as_slice(), (&*self.segments.get()).as_slice()) };
        Walk {
            stack: self,
            records,
            sides,
            segments,
            left: sides.len(),
            headers: self.plain.depth.get(),
            window: Registers(self.plain.registers.get()),
            program: self.plain.program.get(),
            next_record: records.len(),
            next_segment: segments.len(),
        }
    }
}

impl NativeIdentities {

    #[inline]

    pub fn forget(&mut self) {
        self.0 = [UNPROVEN; IDENTITY_SLOTS];
    }
}

impl ParameterHome {

    /// The parameter's binding as it stands, `DELETED` for one not initialized.
    #[inline]
    pub fn read(self, heap: crate::vm::tier_queries::HeapReads<'_>) -> Value {
        match self {
            // SAFETY: a local of a live window.
            Self::Local(binding) => unsafe { *binding },
            Self::Environment(environment, slot) => heap.environment_slot(environment, slot).unwrap_or(Value::DELETED),
        }
    }
}

/// The frames innermost first (`LaneStack::walk_innermost`): the innermost plain headers, then
/// each record followed by the headers it sealed, taking one side record per sided frame. Each
/// frame is read once, where it is (`plain_frame`, `ActivationView::of_record`): a header's frame names
/// the next header down, so the walk advances by the frame it yields.
pub struct Walk<'a> {
    pub stack: &'a LaneStack,
    pub records: &'a [Activation],
    pub sides: &'a [Side],
    pub segments: &'a [PlainSegment],
    /// The side records of the frames not yet walked.
    pub left: usize,
    /// The headers left in the segment being walked, from `window` down, the first's caller
    /// running `program`.
    pub headers: usize,
    pub window: Registers,
    pub program: ProgramId,
    /// The records not yet walked, and the sealed segments.
    pub next_record: usize,
    pub next_segment: usize,
}

impl Admission {
    /// A bit the lane's admission carries while its run calls no script functions
    /// (`LaneContext::script_calls`, `LaneStack::set_script_calls`): every cached call misses, so
    /// the plain call's handler need not test the flag.
    pub const NO_SCRIPT_CALLS: u64 = 1 << 62;
}

impl LaneStack {

    /// The records alone, read without spilling.
    ///
    /// # Safety
    /// No `activations` call (which may spill through a shared reference) happens while the
    /// returned reference lives.
    #[inline(always)]
    pub unsafe fn records_unspilled(&self) -> &LaneActivations {
        // SAFETY: the caller's contract; only `spill_plain` writes through a shared reference.
        unsafe { &*self.activation_records.get() }
    }
}

impl LaneView {

    /// Replaces the specialized `body` with `next`: its entry's handler runs no more, its entry
    /// cell takes the stub, and its code waits for the callers that still name it.
    pub fn retire_body(&self, body: &mut LaneBody, next: LaneBody) {
        let body_state = body.tier_state();
        let LaneBody::Specialized { tier_prologue, .. } = body else { unreachable!() };
        let event = if matches!(next, LaneBody::Declined) { TierEvent::Decline } else { TierEvent::Demote };
        // SAFETY: as `claim_for_tier`; the prologue's handler runs no more for this entry.
        unsafe { (*self.prologue.get()).handler = *tier_prologue };
        let LaneBody::Specialized { method, .. } = std::mem::replace(body, next) else { unreachable!() };
        super::tier_machine::withdraw(&self.entry, body_state, event);
        self.tier().retired.borrow_mut().push(method);
    }

    /// Whether a loop region called it while it had no body (`note_native_caller`).
    #[inline]
    pub fn called_by_regions(&self) -> bool { self.tier().history.get().region_called }

    /// Whether a body compiled a call of this function (`note_caller`): native callers want its
    /// body whatever its length, which only the lane's own entries ask to amortize.
    #[inline]
    pub fn called_by_bodies(&self) -> bool { !self.tier().callers.borrow().is_empty() || self.tier().history.get().demanded }

    /// A specialized body that went at a lifecycle edge warms again from the start, keeping
    /// where it and its predecessors left, and its recompiles, as its successor's feedback: a call site's behavior
    /// outlives the shape ids the edge renumbered. One no run of which completed counts as
    /// wasted (`LaneBodyHistory`).
    pub fn drop_body(&self, body: &mut LaneBody) {
        let LaneBody::Specialized { leaves_at, stuck, recompiles, .. } = body else { return };
        let (stuck, recompiles) = (stuck.union(leaves_at.get()), *recompiles);
        let history = self.tier().history.get();
        self.tier().history.set(LaneBodyHistory {
            lifetimes: history.lifetimes.saturating_add(1),
            ..history
        });
        super::tier_machine::withdraw(&self.entry, body.tier_state(), TierEvent::Demote);
        *body = LaneBody::Unspecialized { recompiles, stuck };
    }
}

impl LoopSite {

    /// Whether the model has admitted the compile.
    #[inline]
    pub fn is_hot(&self) -> bool {
        self.taken.get() > self.needed.get()
    }
}

/// The images of each program stored while recording (`start_recording`); the entry may run
/// on a worker thread, so this is process-wide. It holds the lowered images alone: a linked
/// image belongs to the VM (and thread) that loaded it.
pub static RECORDED: std::sync::Mutex<Option<Recording>> = std::sync::Mutex::new(None);

/// A recording: the lowering that started it, which only a build links, and its images.
pub struct Recording {
    pub lower: fn(&ResidualProgram) -> Result<AheadImages, String>,
    /// The lowering of a Wasm program, from its module's binding types.
    pub lower_wasm: fn(&Vm, ProgramId, &ResidualProgram, &[crate::WasmBindingType]) -> Result<AheadImages, String>,
    pub images: Vec<AotImage>,
}

pub fn recorded() -> std::sync::MutexGuard<'static, Option<Recording>> { RECORDED.lock().unwrap_or_else(std::sync::PoisonError::into_inner) }



impl Default for LaneTier {
    fn default() -> Self {
        Self {
            tier_lane: std::cell::UnsafeCell::new(LaneBody::Unspecialized { recompiles: 0, stuck: LaneLeaves::NONE }),
            body_epoch: std::cell::Cell::new(0),
            pinned: std::cell::RefCell::default(),
            history: Default::default(),
            body_stats: LaneBodyStats::default(),
            retired: std::cell::RefCell::default(),
            callers: std::cell::RefCell::default(),
        }
    }
}


impl LaneBody {

    /// Its place in the per-function tier machine (`tier_machine`).
    #[inline]
    pub fn tier_state(&self) -> TierState {
        match self {
            Self::Unspecialized { .. } => TierState::Warming,
            Self::Compiling { .. } => TierState::Compiling,
            Self::Specialized { .. } if crate::vm::tier_policy::quick_only() => TierState::QJit,
            Self::Specialized { .. } => TierState::UJit,
            Self::Declined => TierState::Declined,
        }
    }
}

impl LaneLeaves {

    pub const NONE: Self = Self::Few([None; LANE_LEAVE_SITES]);

    /// These and `other`'s exits together.
    #[inline]
    pub fn union(self, other: Self) -> Self {
        other.exits().fold(self, Self::with)
    }
}



impl EntryCell {

    /// The body's completions and leaves since it specialized: the tier model's one reader.
    #[inline]
    pub fn evidence(&self) -> (u64, u64) {
        let word = self.counters.get();
        (word >> EVIDENCE.half_bits, word & EVIDENCE.half_mask())
    }
}

impl LaneLeaves {

    #[inline]

    pub fn with(self, exit: LaneExit) -> Self {
        let Self::Few(mut exits) = self else { return self };
        if exits.contains(&Some(exit)) { return self; }
        match exits.iter_mut().find(|slot| slot.is_none()) {
            Some(slot) => { *slot = Some(exit); Self::Few(exits) }
            None => Self::Scattered,
        }
    }

    /// The exits these name: none where scattered.
    #[inline]
    pub fn exits(&self) -> impl Iterator<Item = LaneExit> + '_ {
        let exits: &[_] = match self { Self::Few(exits) => exits, Self::Scattered => &[] };
        exits.iter().flatten().copied()
    }
}

/// The halves of a cell's counters word.
pub const EVIDENCE: Evidence = Evidence { half_bits: u32::BITS };

impl LaneView {

    /// Whether its body's code is compiling (`LaneBody::Compiling`).
    #[inline]
    pub fn compiling(&self) -> bool {
        // SAFETY: as `lane_body`.
        matches!(unsafe { &*self.tier().tier_lane.get() }, LaneBody::Compiling { .. })
    }
}

/// The layout of the counters word: `half_bits` wide leaves, then completions.
pub struct Evidence {
    pub half_bits: u32,
}

impl std::ops::Deref for Records {
    type Target = [std::cell::UnsafeCell<LaneInstruction>];

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.cells[self.front..]
    }
}

impl std::ops::Index<usize> for ScriptTargets {
    type Output = ScriptCallTarget;
    #[inline(always)]
    fn index(&self, index: usize) -> &ScriptCallTarget {
        &self.entries[index]
    }
}

impl Clone for FrozenAssumptions {
    fn clone(&self) -> Self {
        Self { invalidation: self.invalidation.clone(), atoms: self.atoms.clone(), globals: self.globals.clone(), fields: self.fields.clone(), watch: self.watch.clone(), constants: self.constants.clone() }
    }
}

impl Clone for NativeJsProgram {
    fn clone(&self) -> Self {
        Self { program: self.program.clone(), code: self.code.clone(), counts: self.counts, frozen: self.frozen.clone() }
    }
}

impl std::ops::Deref for CallContext {
    type Target = CallContextFields;
    #[inline(always)]
    fn deref(&self) -> &CallContextFields {
        &self.head
    }
}

impl std::ops::DerefMut for CallContext {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut CallContextFields {
        &mut self.head
    }
}

impl Drop for CompiledProgram {
    fn drop(&mut self) {
        let Some(stats) = &self.diagnostics else {
            return;
        };
        let entries = stats
            .native_entry_requests
            .iter()
            .enumerate()
            .filter_map(|(function, count)| {
                let count = count.get();
                (count > 0).then(|| format!("[{function},{count}]"))
            })
            .collect::<Vec<_>>()
            .join(",");
        let fallbacks = stats
            .interpreter_fallback_entries
            .iter()
            .enumerate()
            .filter_map(|(function, count)| {
                let count = count.get();
                (count > 0).then(|| format!("[{function},{count}]"))
            })
            .collect::<Vec<_>>()
            .join(",");
        let reference_sites = stats
            .reference_call_sites
            .iter()
            .filter_map(|site| {
                let counts = site.counts.get();
                (counts.total > 0).then(|| {
                    format!(
                        "[{},{},{},{},{}]",
                        site.function, site.pc, counts.total, counts.native, counts.fallback
                    )
                })
            })
            .collect::<Vec<_>>()
            .join(",");
        let mut reference_targets = Vec::new();
        for site in &stats.reference_call_sites {
            for target in site.targets.borrow().iter() {
                if target.total > 0 {
                    reference_targets.push(format!(
                        "[{},{},{},{},{},{}]",
                        site.function,
                        site.pc,
                        target.function,
                        target.total,
                        target.native,
                        target.same_artifact_native
                    ));
                }
            }
        }
        let reference_targets = reference_targets.join(",");
        let import_callout_targets = format_import_callout_targets(
            &stats.import_callout_names,
            &stats.import_callout_counts,
        );
        let attributed_import_callouts = stats
            .import_callout_counts
            .iter()
            .map(std::cell::Cell::get)
            .fold(0u64, u64::saturating_add);
        debug_assert_eq!(
            attributed_import_callouts,
            stats.compiled_import_callouts.get(),
            "per-import attribution must account for every compiled import callout"
        );
        let same_artifact_direct_hits = stats
            .reference_call_sites
            .iter()
            .map(|site| site.counts.get().same_artifact_direct)
            .fold(0u64, u64::saturating_add);
        eprintln!(
            "QUENCH_WASM_JIT_STATS {{\"program\":{},\"compile_attempts\":1,\"compiled_functions\":{},\"total_functions\":{},\"code_bytes\":{},\"mapped_bytes\":{},\"compile_ns\":{},\"native_entry_requests\":[{}],\"interpreter_fallback_entries\":[{}],\"compiled_to_vm_callouts\":{},\"compiled_import_callouts\":{},\"compiled_known_callouts\":{},\"compiled_reference_callouts\":{},\"import_callout_targets\":[{}],\"emscripten_memcpy_fast_path_hits\":{},\"reference_call_sites\":[{}],\"same_artifact_direct_reference_hits\":{},\"reference_call_targets\":[{}]}}",
            stats.program,
            stats.compiled_functions,
            stats.total_functions,
            stats.code_bytes,
            stats.mapped_bytes,
            stats.compile_ns,
            entries,
            fallbacks,
            stats.compiled_to_vm_callouts.get(),
            stats.compiled_import_callouts.get(),
            stats.compiled_known_callouts.get(),
            stats.compiled_reference_callouts.get(),
            import_callout_targets,
            stats.emscripten_memcpy_fast_path_hits.get(),
            reference_sites,
            same_artifact_direct_hits,
            reference_targets,
        );
    }
}

/// Reads and writes of the head's fields go through the head (`JsStepDispatch::head`).
impl Deref for JsStepDispatch {
    type Target = JsStepHead<Value>;
    #[inline(always)]
    fn deref(&self) -> &JsStepHead<Value> { &self.head }
}

impl DerefMut for JsStepDispatch {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut JsStepHead<Value> { &mut self.head }
}

impl RootedCode for Held {
    fn is_published(&self) -> bool { true }
    fn extend_roots(&self, roots: &mut Vec<crate::Value>) { self.callees.borrow().extend_roots(roots) }
}



impl Iterator for Walk<'_> {
    type Item = (ActivationView, Walked);

    #[inline]
    fn next(&mut self) -> Option<(ActivationView, Walked)> {
        loop {
            if self.headers != 0 {
                self.headers -= 1;
                let (window, program) = (self.window, self.program);
                let side = header_sided(window).then(|| self.take_side());
                // Each caller's window and program as the return reads them (an array step's
                // header names the trampoline's result window, its step the real one).
                let frame = self.stack.plain_frame(window, side, program);
                (self.window, self.program) = (frame.caller_registers(), frame.caller_program());
                return Some((frame, Walked::Header { window, program, side, side_at: self.left }));
            }
            let sealed = self.next_segment.checked_sub(1).map(|at| self.segments[at]).filter(|segment| segment.records == self.next_record);
            if let Some(segment) = sealed {
                self.next_segment -= 1;
                (self.headers, self.window, self.program) = (segment.depth, Registers(segment.registers), segment.program);
                continue;
            }
            self.next_record = self.next_record.checked_sub(1)?;
            let records = self.records;
            let record = &records[self.next_record];
            let payload = if LaneStack::record_sided(record) { self.take_side().payload } else { SidePayload::None };
            return Some((ActivationView::of_record(record, payload), Walked::Record { index: self.next_record }));
        }
    }
}

unsafe extern "Rust" {
    safe fn quench_engine_drop_code_memory(memory: &mut CodeMemory);
}

impl Drop for CodeMemory {
    #[inline]
    fn drop(&mut self) { quench_engine_drop_code_memory(self) }
}

impl Drop for NativeRoots {
    fn drop(&mut self) {
        // SAFETY: `slots` is the mapping `new` made, of `capacity` values, and nothing else owns it.
        unsafe {
            super::site_table::storage::unmap(std::ptr::NonNull::new_unchecked(self.slots.cast::<u8>()), self.capacity as usize * size_of::<u64>());
        }
    }
}
unsafe impl Send for NativeRoots {}

impl LoopTier {

    #[inline]

    pub fn is_published(&self) -> bool {
        self.code.as_ref().is_some_and(|code| code.is_published())
    }
}


/// Where a walked frame is, for a writer that edits it there (`LaneStack::recomplete_at`).
#[derive(Clone, Copy)]
pub enum Walked {
    /// A plain header below `window`, whose caller runs `program` where it has no side record,
    /// with its side record where it has one, at `side_at` in `LaneStack::sides`; where it has
    /// none, `side_at` is the index one inserted for it takes: above every side record of the
    /// frames below it.
    Header { window: Registers, program: ProgramId, side: Option<Side>, side_at: usize },
    /// The record at `index` of the records.
    Record { index: usize },
}

/// One reserved region: where code runs from and where the compiler writes it.
#[derive(Clone, Copy)]
pub struct Views {
    pub exec: *mut u8,
    pub write: *mut u8,
}

pub fn format_import_callout_targets(
    names: &[(u32, u32, String, String)],
    counters: &[std::cell::Cell<u64>],
) -> String {
    debug_assert_eq!(names.len(), counters.len());
    names
        .iter()
        .zip(counters)
        .filter_map(|((function, import_index, module, name), count)| {
            let count = count.get();
            (count > 0).then(|| {
                format!(
                    "{{\"function\":{function},\"import_index\":{import_index},\"module\":{},\"name\":{},\"count\":{count}}}",
                    json_string(module),
                    json_string(name)
                )
            })
        })
        .collect::<Vec<_>>()
        .join(",")
}

pub fn json_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\u{08}' => escaped.push_str("\\b"),
            '\u{0C}' => escaped.push_str("\\f"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character <= '\u{1F}' => {
                use std::fmt::Write;
                write!(escaped, "\\u{:04x}", character as u32)
                    .expect("writing to a String cannot fail");
            }
            character => escaped.push(character),
        }
    }
    escaped.push('"');
    escaped
}

/// Whether the frame header below `window` carries a side record.
#[inline(always)]
pub fn header_sided(window: Registers) -> bool {
    !crate::staged::call_complete::concrete_frame_completes_plainly(window.0.expose_provenance() as u64)
}

impl EntryCell {

    #[inline(always)] pub fn stub() -> usize { entry_stub_in_lane as BodyEntry as usize }

    #[inline]

    pub fn scalar_stub() -> usize { scalar_entry_stub as ScalarEntry as usize }

    /// Writes the unpublished state: the stub entry and scalar entry, no rows or proofs, and no
    /// recovery space or body. A new cell and a withdrawn one are both in this state.
    pub fn unpublish(&self) {
        // The body the cell held no longer types its callers' calls (`LaneMethod::summary`).
        if self.body.get() != 0 {
            // SAFETY: a published cell's `body` is `Rc::as_ptr` of a body its view pins (`publish`), and the word is
            // zeroed when that body retires (`PublishedCode::retire`), so a nonzero word names a live body.
            unsafe { (*(self.body.get() as *const LaneMethod)).withdraw() }
        }
        self.entry.set(Self::stub());
        self.rows.set(0);
        self.proven.set(0);
        self.proven_entry.set(0);
        self.proven_body.set(0);
        self.scalar.set(Self::scalar_stub());
        self.proven_entry.set(0);
        self.proven_body.set(0);
        self.recovery.set(std::ptr::addr_of!(NO_RECOVERY).addr());
        self.body.set(0);
        // The stub's entry reads the receiver from the header as any body without row entries does (`publish`).
        self.uses_this.set(1);
    }

    /// Publishes `method`'s entry and recovery space; it takes the cell back when it retires: the
    /// entry to the stub and the body word, which says whether the cell is published, to none.
    pub fn publish(&self, method: &PublishedLaneMethod) {
        let Some(body) = method.lease() else { return };
        self.recovery.set(std::ptr::addr_of!(body.recovery_space).addr());
        self.body.set(Rc::as_ptr(&body).addr());
        method.on_retire(&self.body, Rc::as_ptr(&body).addr(), 0);
        self.entry.set(body.entry_address());
        method.on_retire(&self.entry, body.entry_address(), Self::stub());
        self.rows.set(0);
        self.proven.set(0);
        self.numbers.set(0);
        // A body without row entries (a u-jit body) takes the receiver its caller writes, so a b-jit call row
        // writes it always: a b-jit caller of a body promoted to u-jit read a stale receiver otherwise.
        self.uses_this.set(1);
        if let Some(entries) = body.row_entries() {
            self.rows.set(entries.rows);
            method.on_retire(&self.rows, entries.rows, 0);
            self.proven.set(entries.proven);
            method.on_retire(&self.proven, entries.proven, 0);
            self.numbers.set(entries.numbers);
            self.uses_this.set(u64::from(entries.uses_this));
        }
        self.scalar.set(Self::scalar_stub());
        if let Some(scalar) = body.scalar_address() {
            self.scalar.set(scalar);
            method.on_retire(&self.scalar, scalar, Self::scalar_stub());
        }
        self.proven_entry.set(0);
        self.proven_body.set(0);
        if let Some(proven) = body.proven_address() {
            let owner = Rc::as_ptr(&body).addr();
            self.proven_body.set(owner);
            method.on_retire(&self.proven_body, owner, 0);
            self.proven_entry.set(proven);
            method.on_retire(&self.proven_entry, proven, 0);
        }
    }

    #[inline]

    pub fn withdraw(&self) {
        self.unpublish();
    }
}

impl Evidence {

    #[inline]
    pub const fn half_mask(&self) -> u64 { (1 << self.half_bits) - 1 }
}

impl LaneStack {

    /// The frame of the plain activation whose window is `window` (`ActivationView`), read from its
    /// header and `side`, its side record where its header carries one (`header_sided`), and
    /// `program`, the program its caller runs where it carries none: the innermost's is
    /// `PlainFrames::program`, each other's the caller program of the frame above it. No record
    /// is built. A native's callback (`CallbackTrampoline`), always the outermost plain
    /// activation, reads the call record and caller registers its trampoline holds, as
    /// `PreparedShape::of`'s record names them. An in-lane array step's callback
    /// (`steps_return`), whose caller base word is its step's index, reads as the
    /// `Completion::ArrayStep` record `lane_begin_array_iteration` pushed before, with the call
    /// record, caller registers and base its step holds. Its caller registers are the next
    /// plain activation's window where one remains below. The one definition the walk, the
    /// innermost's read, the pop and the spill share.
    #[inline(always)]
    pub fn plain_frame(&self, window: Registers, side: Option<Side>, program: ProgramId) -> ActivationView {
        let (call, caller_registers, caller_base) = window.plain_caller();
        let (call, caller_registers, caller_base, completion) = if let Some(side) = side {
            (call, caller_registers, caller_base, side.completion)
        } else if let Some((call, registers)) = self.callback_return.returns(call) {
            (call, registers, caller_base, Completion::ReturnsToNative)
        } else if self.callback_return.steps_return(call) {
            let step = &self.steps[caller_base.addr()];
            let header = step.header.expect("an array step's plain callback has its header");
            (header.call, header.caller_registers, step.caller_base, Completion::ArrayStep)
        } else {
            (call, caller_registers, caller_base, Completion::Plain)
        };
        // SAFETY: the window's header was written at its push (`push_plain`).
        let (packed, callee) = unsafe { (*window.0.sub(CONTEXT_PACKED).cast::<u64>(), *window.0.sub(CONTEXT_CALLEE)) };
        let (forwards, kept, environment, record) =
            side.map_or((false, 0, false, std::ptr::null_mut()), |side| (side.forwards, side.kept, side.environment, side.record));
        let (callee_at, body) = match side {
            Some(side) if side.native != NO_NATIVE => (self.native_table[side.native] as *const DirectCallee, self.body_table[side.native]),
            _ => (std::ptr::null(), 0),
        };
        ActivationView {
            callee: callee_at,
            this: 0,
            body,
            window: window.0.addr(),
            function: packed as u32,
            call,
            caller_base,
            caller_registers,
            caller_program: side.map_or(program, |side| side.caller_program),
            kind: ActivationKind::Script(ScriptActivation {
                callee,
                argc: (packed >> crate::staged::frame::ARGC_SHIFT) as u16,
                completion,
                forwards,
                kept,
                environment,
                record,
            }),
            payload: side.map_or(SidePayload::None, |side| side.payload),
        }
    }

    /// Whether `record` owns a side record: a script record whose completion carries a payload
    /// (`SidePayload`), but a native frame's skipped constructor, which its descriptor names
    /// until the lane continues in the frame (`native_frames::advance_into_frames`). A header
    /// spilled into a record carries a payload exactly then, so its side record stays its own.
    #[inline]
    pub fn record_sided(record: &Activation) -> bool {
        record.callee.is_null() && record.script().is_some_and(|script| script.completion.carries_payload())
    }
}

impl ActivationView {

    /// The frame a record holds, with what its side record carries.
    #[inline(always)]
    pub fn of_record(record: &Activation, payload: SidePayload) -> Self {
        Self {
            callee: record.callee,
            this: record.this,
            body: record.body,
            window: record.window,
            function: record.function(),
            call: record.call(),
            caller_base: record.caller_base(),
            caller_registers: record.caller_registers(),
            caller_program: record.caller_program(),
            kind: *record.kind(),
            payload,
        }
    }
}

impl Walk<'_> {

    /// Takes the next side record down, its frame's.
    #[inline]
    pub fn take_side(&mut self) -> Side {
        self.left -= 1;
        self.sides[self.left]
    }
}

/// `Side::native` of a side that names no native frame.
pub const NO_NATIVE: usize = usize::MAX;

/// The recovery space the stub's cell names: no body, nothing to recover.
pub static NO_RECOVERY: LaneRecoverySpace = LaneRecoverySpace { values: 0, activations: 0 };

/// A scalar entry: (context, numbers...) -> (status, result word), the status zero for a result.
pub type ScalarEntry = unsafe extern "C" fn(*mut CallContext) -> BodyReturn;

impl Activation {
    /// The function it runs.
    #[inline(always)] pub fn function(&self) -> u32 { self.function }
    /// The caller's records base, which its return makes the lane's again.
    #[inline(always)] pub fn caller_base(&self) -> Ip { self.caller_base }
    /// The caller's registers, which its return makes the lane's again.
    #[inline(always)] pub fn caller_registers(&self) -> Registers { self.caller_registers }
    /// The program its caller runs (`caller_program` field).
    #[inline(always)] pub fn caller_program(&self) -> ProgramId { self.caller_program }
    /// What kind of activation it is.
    #[inline(always)] pub fn kind(&self) -> &ActivationKind { &self.kind }

    /// The frame state of a JavaScript activation; `None` for Wasm and for a native frame
    /// the lane has not linked (`NativeFrames`), which no lane code observes.
    #[inline] pub fn script(&self) -> Option<&ScriptActivation> {
        match &self.kind {
            ActivationKind::Script(script) => Some(script),
            ActivationKind::Native | ActivationKind::Wasm { .. } | ActivationKind::WasmEntry { .. } => None,
        }
    }
}

impl ActivationView {
    /// The caller's registers.
    #[inline(always)] pub fn caller_registers(&self) -> Registers { self.caller_registers }
    /// The program its caller runs.
    #[inline(always)] pub fn caller_program(&self) -> ProgramId { self.caller_program }
}

impl CallbackTrampoline {

    /// The callback's own call record and caller registers where `call`, a plain header's, is
    /// the trampoline's call record; `None` for any other plain activation.
    #[inline(always)]
    pub fn returns(&self, call: Ip) -> Option<(Ip, Registers)> {
        std::ptr::eq(call, self.records.as_ptr()).then(|| (self.call.get(), self.caller_registers.get()))
    }

    /// Whether `call`, a plain header's, is the array steps' call record: the header is an
    /// array step's callback, whose caller base word is its step's index (`LaneArrayStep`).
    #[inline(always)]
    pub fn steps_return(&self, call: Ip) -> bool {
        std::ptr::eq(call, self.steps.as_ptr())
    }
}

impl LaneMethod {

    /// The address of its compiled entry, which a direct caller calls (`DirectCallee`).
    #[inline]
    pub fn entry_address(&self) -> usize {
        self.entry as usize
    }

    /// Ends its summary, as its entry cell withdraws it (`EntryCell::withdraw`).
    #[inline]
    pub fn withdraw(&self) { self.withdrawn.set(true) }

    /// The address of its scalar entry, if it has one (`EntryCell::scalar`).
    #[inline]
    pub fn scalar_address(&self) -> Option<usize> {
        self.scalar
    }

    /// The address of its proven entry, if it has one (`EntryCell::proven_entry`).
    #[inline]
    pub fn proven_address(&self) -> Option<usize> {
        self.proven.as_ref().map(|(address, _)| *address)
    }

    /// Its row entries, if it has them (`EntryCell::rows`, `proven`, `numbers`, `uses_this`).
    #[inline]
    pub fn row_entries(&self) -> Option<RowEntries> {
        self.rows
    }
}

impl Registers {

    /// The plain header's call record, caller registers and caller records.
    #[inline(always)]
    pub fn plain_caller(self) -> (Ip, Registers, Ip) {
        // SAFETY: the window's header was written at its push (`write_plain_header`).
        unsafe {
            (
                *self.0.sub(CONTEXT_CALL).cast::<Ip>(),
                *self.0.sub(CONTEXT_CALLER_REGISTERS).cast::<Registers>(),
                *self.0.sub(CONTEXT_CALLER_BASE).cast::<Ip>(),
            )
        }
    }
}

/// The entry no body backs: it enters nothing. Its address is the cell's unpublished state
/// (`EntryCell::stub`), so it has exactly one instance: a tiny function is otherwise inlinable and
/// each codegen unit may get its own copy at its own address.
#[inline(never)]
pub unsafe extern "C" fn entry_stub_in_lane(_: *mut CallContext, _: u64, _: u64, _: *const u64) -> BodyReturn {
    BodyReturn { status: LANE_BODY_NOT_ENTERED, value: 0 }
}

/// The scalar entry no body backs: it enters nothing, so its caller restarts the call. One
/// instance, as `entry_stub_in_lane`.
#[inline(never)]
pub unsafe extern "C" fn scalar_entry_stub(_: *mut CallContext) -> BodyReturn {
    BodyReturn { status: LANE_BODY_NOT_ENTERED, value: 0 }
}

impl Completion {

    /// Whether an activation completing so carries a payload in its side record (`SidePayload`).
    #[inline]
    pub fn carries_payload(self) -> bool {
        matches!(self, Self::Forwarder | Self::SettlesPromise | Self::AsyncRequest | Self::ToWasm | Self::ReadsTrapped | Self::AssignsTrapped)
    }
}

impl Activation {
    /// The caller's call record, which its return completes.
    #[inline(always)] pub fn call(&self) -> Ip { self.call }
}

impl ExitKind {

    #[inline]

    pub const fn word(self, place: u64) -> u64 {
        (self as u64) << Self::SHIFT | place
    }
}

impl MemoryView {

    #[inline]

    pub const fn new(bytes: *mut u8, len: usize) -> Self {
        Self {
            bytes,
            reach: Slot::of(len as isize - WIDEST_DIRECT_ACCESS as isize),
        }
    }
}

impl InstructionField {

    #[inline]

    pub const fn accumulator(self) -> u8 {
        match self {
            Self::A => ACC_A,
            Self::B => ACC_B,
            Self::C => ACC_C,
        }
    }
}

pub const WIDEST_DIRECT_ACCESS: usize = size_of::<u64>();
pub const ACC_A: u8 = 1;
pub const ACC_B: u8 = 2;
pub const ACC_C: u8 = 3;

impl ExitKind {

    pub const KINDS: [Self; 4] = [Self::At, Self::Inlined, Self::Callee, Self::Entry];
    pub const SHIFT: u32 = u64::BITS - Self::KINDS.len().trailing_zeros();
}

impl<T: Copy> Slot<T> {

    #[inline(always)]
    pub const fn of(value: T) -> Self {
        Self(std::mem::MaybeUninit::new(value))
    }
}
