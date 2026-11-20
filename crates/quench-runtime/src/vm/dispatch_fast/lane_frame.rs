//! The interpreter's register frame: `staged` rows over the live file, a move or constant, a
//! jump, an allocation, a suspension (`suspends`).

use super::*;
use super::lane_reads::Reads;
use crate::bytecode::{Operand, Register};
use crate::typed_ir::Type;
use crate::bytecode::RegisterWindow;
use crate::staged::{Allocation, CallOut, Comparison, NumberOperation, Concrete, Constant, Edge, Frame, GlobalBinding, NumericFrame, Operation, Protocol, Step, WasmRep};

/// The maker of a frame's heap values: allocates in the lane context it is given and runs no
/// guest code; `None` leaves the instruction to the general path.
type AllocationMaker = unsafe fn(*mut (), Registers, Allocation) -> Option<Value>;

/// The definer of a frame's array literal element (`define_element_in_lane`): the VM's definition of
/// `value` at `index` into the literal array `array`; false leaves the definition to the general path.
type ElementDefiner = unsafe fn(*mut (), u64, u32, u64) -> bool;

/// The deleter of a frame's own `delete base[key]` hit: the `true` of a deletion that runs no
/// guest code; `None` leaves the instruction to the general path.
type PropertyDeleter = unsafe fn(*mut (), Value, Value) -> Option<Value>;

/// The reader of a frame's by-name load: the value of `atom` under a binding the record's first
/// run resolved, which runs no guest code; `None` leaves the instruction to the residual.
type NameReader = unsafe fn(*mut (), NameRead, u32) -> Option<Value>;

/// The fast answer of a `Names` row's iteration or for-in call-out over its operands (`fast_names_step`),
/// which runs no guest code; `None` leaves the call to the row's miss.
type NamesStep = unsafe fn(*mut (), CallOut, [Option<Value>; 2]) -> Option<Value>;

/// The values a keeping activation kept (`kept_values`).
type KeptValues = unsafe fn(*mut ()) -> Option<(u16, u16)>;

/// The test of a receiver's constant fields through a cache site (`constant_fields_value`), which
/// reads one entry and a shape.
type ConstantFieldsTest = unsafe fn(*mut (), Value, u16) -> bool;

/// How a by-name load's record reads its name once resolved (`name_resolution`).
#[derive(Clone, Copy)]
pub(super) enum NameRead {
    /// The active realm's function for the native the name stands for (`Vm::intrinsic_natives`).
    Native,
    /// An intrinsic name's value (`Vm::intrinsic_name_value`).
    Intrinsic,
    /// The global object's own data field the record's hint names.
    Field(crate::vm::FieldCache),
    /// The active realm's global lexical binding the record's hint locates by index (`slot`), taken
    /// only while it is the name's and initialized (`Vm::global_lexical_at`). Subsumes the hand
    /// `lane_load_name_lexical`.
    Lexical(crate::vm::FieldCache),
}

/// What a by-name load's record resolved its name to: the global `undefined`, a constant no
/// binding changes, or a read of the VM's heap.
#[derive(Clone, Copy)]
pub(super) enum NameBinding {
    Undefined,
    Read(NameRead),
}

/// The lane context (type-erased, so the frame is one type for every host) and what it makes
/// for a staged row: an allocation, a property deletion or a by-name load's value.
#[derive(Clone, Copy)]
struct LaneHost {
    cx: *mut (),
    allocate: AllocationMaker,
    define: ElementDefiner,
    delete: PropertyDeleter,
    read_name: NameReader,
    /// The `typeof` kind of a heap value, as the VM names it (`typeof_kind`), for a staged `typeof` row.
    type_of: unsafe fn(*mut (), Value) -> Option<crate::staged::value::TypeofKind>,
    /// The interned `typeof` result string of a name index in `TYPEOF_NAMES` (`typeof_names`).
    typeof_name: unsafe fn(*mut (), u64) -> Option<Value>,
    /// SetFunctionName of a function to a program atom through a cache site (`Vm::set_function_name`).
    set_name: unsafe fn(*mut (), Value, u32, u16) -> Option<()>,
    /// The values a keeping activation kept (`kept_values`): their count and the register
    /// that holds the first, past the function's registers.
    kept_values: KeptValues,
    /// What an `await`'s source conversion asks (`staged::await_source`, `AwaitSource::of`).
    awaits: AwaitSource,
    /// Whether a receiver already holds its constructor's run of constant fields
    /// (`Vm::constant_fields_held`).
    constant_fields: ConstantFieldsTest,
    /// What the by-name load the frame runs resolved its name to, if it runs one.
    name: Option<NameBinding>,
    /// The VM's `is_object_like` of a heap value, for a staged object test (`RequireIteratorResult`).
    object_like: unsafe fn(*mut (), Value) -> bool,
}

/// The lane's environment clone for a `CloneEnv` row (`clone_environment_of`): the lane context, the
/// register count of the environment window the record names (0 where it names none), and the clone.
#[derive(Clone, Copy)]
struct EnvironmentClones {
    context: *mut (),
    window: u16,
    run: unsafe fn(*mut (), Registers, u16, u32) -> bool,
}

/// The window scope a row of a window activation whose names resolve through its scope reads and
/// writes through (`FLAG_SCOPE`, `FLAG_SCOPE_ENVIRONMENT`, `FLAG_WINDOW_CLOSURE`): the lane context and
/// the record's ip, which the scope's lookup takes (`window_scope`), the closure's function and window
/// register count, and the lane's callouts for each answer.
#[derive(Clone, Copy)]
struct ScopeReads {
    context: *mut (),
    ip: super::Ip,
    /// The closure's function, and its window's register count (`environment_window_fields`).
    function: u32,
    window: u16,
    /// The name load's global-read cache site (`LOAD_NAME`'s `c`), which its `name` reads at.
    site: u16,
    /// The by-name read of the atom into the destination and, for a call, its receiver; the load's
    /// global-read cache site (`site`).
    name: unsafe fn(*mut (), Registers, super::Ip, u32, Register, Option<Register>, u16) -> bool,
    /// The store of a word to the atom's `with` object.
    store: unsafe fn(*mut (), Registers, super::Ip, u32, u64) -> bool,
    /// The store of a word to the window's local slot's `with` object.
    local: unsafe fn(*mut (), Registers, super::Ip, u16, u64) -> bool,
    /// The closure of the function, over the window's scope.
    closure: unsafe fn(*mut (), Registers, super::Ip, u32, u16) -> Option<Value>,
}

/// The lane context's capture stores, erased of its host so the frame names no `H`
/// (`store_to_capture`).
#[derive(Clone, Copy)]
struct CaptureStores {
    context: *mut (),
    store: fn(*mut (), u16, u16, Value) -> bool,
}

impl CaptureStores {
    /// The stores of the running lane's `cx` (`store_to_capture`).
    #[inline(always)]
    fn of<H: Host>(cx: *mut LaneContext<H>) -> Self {
        Self { context: cx.cast(), store: |context, depth, slot, value| store_to_capture(context.cast::<LaneContext<H>>(), depth, slot, value) }
    }

    /// The running function's own initialized slot written directly through its dense slots
    /// (`store_own_capture`), over the register file `r`; any other store is the row's miss
    /// (`row_miss`: the lane's walking store).
    #[inline(always)]
    fn own(r: Registers) -> Self {
        Self { context: r.0.cast(), store: |context, depth, slot, value| depth == 0
            && super::capture_stores(Registers(context.cast()).capture())
                // SAFETY: the running function's dense capture slots, which admit this write
                // (`CAPTURE_STORES`); no collection runs in the lane.
                .and_then(|slots| unsafe { crate::heap::EnvironmentSlots::write_storage(slots, usize::from(slot), value) })
                .is_some() }
    }
}

/// The lane context's steady decisions of a pair the words leave, erased of its host so the frame
/// names no `H` (`comparison_decision`).
#[derive(Clone, Copy)]
struct Decisions {
    context: *mut (),
    decide: fn(*mut (), u32, Value, Value) -> Option<bool>,
    /// The encoded `prototype` link of a constructor the VM proved Stable (`Vm::instanceof_fact`).
    prototype: fn(*mut (), Value) -> Option<u32>,
    /// The base of the object record table.
    records: fn(*mut ()) -> Option<u64>,
    /// `value instanceof constructor` by the VM's published proof of the constructor's chain (`Vm::record_instanceof`),
    /// else the ordinary walk (`Vm::ordinary_instanceof`): neither runs guest code.
    instance: fn(*mut (), Value, Value) -> Option<bool>,
}

impl Decisions {
    /// The decisions of the running lane's `cx`.
    #[inline(always)]
    fn of<H: Host>(cx: *mut LaneContext<H>) -> Self {
        Self { context: cx.cast(), decide: |context, operator, left, right|
            super::vm_steps::comparison_decision(unsafe { vm(context.cast::<LaneContext<H>>()) }, operator, left, right),
            prototype: |context, constructor| unsafe { vm(context.cast::<LaneContext<H>>()) }.instanceof_fact(constructor)
                .map(|fact| fact.link),
            records: |context| unsafe { vm(context.cast::<LaneContext<H>>()) }.heap.object_record_base().map(|base| base as u64),
            instance: |context, value, constructor| {
                let machine = unsafe { vm(context.cast::<LaneContext<H>>()) };
                machine.record_instanceof(value, constructor).or_else(|| machine.ordinary_instanceof(value, constructor))
            } }
    }
}

/// The lane context's own-field store and add hits, erased of its host so the frame names no `H`:
/// what `StorePlan::Layout` joins to (`field_store_host`), and what `StorePlan::Add` joins to
/// (`field_add`).
#[derive(Clone, Copy)]
struct FieldStores {
    context: *mut (),
    /// The VM's own-field store of `value` to `atom` through `site`, which takes the form hint it
    /// leaves for the record (`primary_own_field_store_hit`); `None` where the VM declines.
    own: fn(*mut (), Value, u32, u16, Value) -> Option<crate::vm::FieldCache>,
    /// The VM's add the proved chain makes (`proved_field_add_hit`), noting its allocation.
    proved: fn(*mut (), Value, u32, u16, Value) -> bool,
    /// The VM's cached add (`primary_field_add_hit`), noting its allocation.
    primary: fn(*mut (), Value, u32, u16, Value) -> bool,
    /// The VM's store through the record's hint (`hinted_primary_field_store_hit::<true>`, or the
    /// narrow store `hinted_narrow_field_store_hit` when `narrow`).
    hinted: fn(*mut (), Value, u32, Value, crate::vm::FieldCache, bool) -> bool,
    /// The VM's store through the record's own hint (`hinted_own_field_store_hit`).
    hinted_own: fn(*mut (), Value, u32, Value, crate::vm::FieldCache) -> bool,
    /// The VM's store through the site's polymorphic entries (`polymorphic_own_field_store_hit`),
    /// the own entry it hit, which the record's hint takes.
    polymorphic: fn(*mut (), Value, u32, u16, Value) -> Option<crate::vm::FieldCache>,
    /// The VM's general cached write (`cached_field_write_hit`) of a public name, noting its
    /// allocation; a private name leaves to its own path.
    general: fn(*mut (), Value, u32, u16, Value) -> bool,
    /// The VM's own-field store a sinking read makes to `this` (`primary_own_field_store_hit`
    /// without widening); a refusal is the general path's.
    sink: fn(*mut (), Value, u32, u16, Value) -> bool,
}

/// The VM's Math calls of Number arguments a Math row computes in place, erased of its host
/// so the frame names no `H` (`PropertyFrame::math_call`).
#[derive(Clone, Copy)]
struct MathCalls {
    context: *mut (),
    /// `number::math_inline_call` of the callee's native, the callee's identity read from its
    /// function record; `None` for any other callee or argument.
    call: fn(*mut (), Value, &[Value]) -> Option<Value>,
}

impl MathCalls {
    /// The Math calls of the running lane's `cx`.
    #[inline(always)]
    fn of<H: Host>(cx: *mut LaneContext<H>) -> Self {
        Self { context: cx.cast(), call: |context, callee, arguments| {
            let vm = unsafe { vm(context.cast::<LaneContext<H>>()) };
            let Some(FunctionRecord { kind: FunctionKind::Native(native), .. }) = vm.heap.function(callee) else { return None };
            super::number::math_inline_call(*native, arguments)
        } }
    }
}

impl FieldStores {
    /// The stores and adds of the running lane's `cx`.
    #[inline(always)]
    fn of<H: Host>(cx: *mut LaneContext<H>) -> Self {
        Self { context: cx.cast(),
            own: |context, object, atom, site, value| {
                let cx = context.cast::<LaneContext<H>>();
                let vm = unsafe { vm(cx) };
                // SAFETY: the context's program outlives the lane run.
                let code = unsafe { &*(*cx).code };
                if !vm.primary_own_field_store_hit::<true>(code, object, atom, value, site, crate::heap::PropertyStoreLayout::MayWiden) {
                    return None;
                }
                Some(vm.form_hint(vm.field_cache_entry(site), object))
            },
            proved: |context, object, atom, site, value| {
                let vm = unsafe { vm(context.cast::<LaneContext<H>>()) };
                let added = vm.proved_field_add_hit(object, atom, value, site);
                if added {
                    // Adding a property can allocate.
                    note_allocation(vm);
                }
                added
            },
            primary: |context, object, atom, site, value| {
                let cx = context.cast::<LaneContext<H>>();
                let vm = unsafe { vm(cx) };
                // SAFETY: the context's program outlives the lane run.
                let code = unsafe { &*(*cx).code };
                let added = vm.primary_field_add_hit(code, object, atom, value, site);
                if added {
                    note_allocation(vm);
                }
                added
            },
            hinted: |context, object, atom, value, hint, narrow| {
                let vm = unsafe { vm(context.cast::<LaneContext<H>>()) };
                if narrow { vm.hinted_narrow_field_store_hit(hint, object, atom, value) }
                else { vm.hinted_primary_field_store_hit::<true>(hint, object, atom, value) }
            },
            hinted_own: |context, object, atom, value, hint| {
                let vm = unsafe { vm(context.cast::<LaneContext<H>>()) };
                vm.hinted_own_field_store_hit(hint, object, atom, value)
            },
            polymorphic: |context, object, atom, site, value| {
                let cx = context.cast::<LaneContext<H>>();
                let vm = unsafe { vm(cx) };
                // SAFETY: the context's program outlives the lane run.
                let code = unsafe { &*(*cx).code };
                vm.polymorphic_own_field_store_hit(code, object, atom, value, site, crate::heap::PropertyStoreLayout::MayWiden)
            },
            general: |context, object, atom, site, value| {
                let cx = context.cast::<LaneContext<H>>();
                let vm = unsafe { vm(cx) };
                if vm.is_private_name(atom) { return false; }
                // SAFETY: the context's program outlives the lane run.
                let code = unsafe { &*(*cx).code };
                let stored = vm.cached_field_write_hit(code, object, atom, value, site);
                if stored {
                    // Adding a property can allocate.
                    note_allocation(vm);
                }
                stored
            },
            sink: |context, object, atom, site, value| {
                let cx = context.cast::<LaneContext<H>>();
                let vm = unsafe { vm(cx) };
                // SAFETY: the context's program outlives the lane run.
                let code = unsafe { &*(*cx).code };
                vm.primary_own_field_store_hit::<false>(code, object, atom, value, site, crate::heap::PropertyStoreLayout::Preserve)
            },
        }
    }
}

/// The hand-off of a call the lane frame admitted (`LaneCall::activate`): over the lane context
/// (erased), the caller's registers, the call record and where it is, the admitted entry, the plain
/// depth the room was admitted at, and the bound `this`, the callee's window filled and its plain
/// header pushed or its leaf entered: the callee's registers and first record, the context already
/// switched to them.
pub(in crate::vm::dispatch_fast) type CallActivation =
    unsafe fn(*mut (), Registers, Ip, *const LaneInstruction, *const ScriptCallTarget, usize, Value) -> (Registers, Ip);

/// The static form of a call row (`protocol_row` of `MISS`, `call_form`): its leaf form (`NOT_LEAF`
/// for a plain activation), its argument count (`ANY_ARGC` where the record's immediate holds it),
/// and whether its receiver is its function's `undefined` register (`script_call_miss`'s `PLAIN`).
#[derive(Clone, Copy)]
pub(in crate::vm::dispatch_fast) struct CallForm {
    leaf: LeafForm,
    argc: u16,
    plain: bool,
}

/// The call record a call row runs (`protocol_row`), erased of its host so the frame names no `H`:
/// what `CallFrame`'s hooks read of the lane (`lane_frame::staged_ops`), and what `call_entry` and
/// `call_room` admitted, for the hooks past them.
pub(in crate::vm::dispatch_fast) struct LaneCall {
    /// The lane context (`*mut LaneContext<H>`), which `activate` takes.
    context: *mut (),
    /// The running lane's stack (`Vm::lane_stack`) and the realm's global object, which a sloppy
    /// callee binds for `undefined` and `null`.
    stack: *const LaneStack,
    globals: *const Value,
    /// The call record and where it is.
    record: *const LaneInstruction,
    ip: Ip,
    form: CallForm,
    /// The entry `call_entry` admitted.
    target: std::cell::Cell<*const ScriptCallTarget>,
    /// The plain depth `call_room` admitted the call at (`LaneStack::plain_room`), which the plain
    /// header's push writes past.
    depth: usize,
    activate: CallActivation,
}

/// The interpreter's frame: the live register file and the accumulator view a producer
/// carries. `ACC` names the record's accumulator field (`read`).
pub(in crate::vm::dispatch_fast) struct LaneFrame<const ACC: u8> {
    registers: Registers,
    view: Carried,
    /// The register the carried accumulator stands for (the record's field B where `ACC` is
    /// `ACC_B`, `row_frame`); `NO_ACCUMULATOR` elsewhere. A row reads the accumulator only for
    /// this register: another operand it reads (an array literal's element beside its array)
    /// is the file's, which every producer writes.
    accumulated: u16,
    /// The activation's `this` (`LaneContext::this`): `Value::DELETED` where the lane does
    /// not hold it.
    this: Value,
    /// The activation's local slots and their count, for a staged local operation.
    locals: Option<(*mut Value, usize)>,
    /// The lane context and what it makes, for a staged allocation or deletion row.
    host: Option<LaneHost>,
    /// The capture stores of the running lane, for a staged capture store.
    captures: Option<CaptureStores>,
    /// The running lane's steady decisions, for a staged comparison jump.
    decisions: Option<Decisions>,
    /// The slots the running function's captures read from (`capture_reads`), for a staged capture read.
    capture_slots: Option<*const usize>,
    /// The heap whose records a row reads in place (`Frame::heap_records`), where the row is one
    /// that does (`Decoder::Names`).
    heap: Option<*const crate::heap::Heap>,
    /// The running frame's `this` where the lane's context word does not hold it (`this_in_lane`),
    /// read only by a row that asks for `this`.
    frame_this: Option<(*mut (), unsafe fn(*mut ()) -> Option<Value>)>,
    /// The running function's `arguments` count in a local slot (`arguments_count`), read only
    /// by the row that asks for it.
    arguments: Option<(*mut (), unsafe fn(*mut (), u16) -> Option<Value>)>,
    /// The running activation's kept `arguments` binding, read only by a local read of that slot
    /// (`FLAG_KEPT_MARKER`, `FLAG_KEPT_OBJECT`): the lane context and `kept_arguments_binding`, which
    /// answers the binding's marker or its object (`Frame::arguments_binding`).
    kept_binding: Option<(*mut (), unsafe fn(*mut (), Registers, u16) -> Option<Value>)>,
    /// The register a keeping activation's kept values follow its registers from (`ArgumentGet`'s
    /// record `c`, the running function's register count), read by `Frame::argument`.
    kept: u16,
    /// The window scope of a row whose names resolve through its scope (`ScopeReads`), where the row is one.
    scope: Option<ScopeReads>,
    /// The running frame's captured environment, for a staged environment row (`Decoder::Names`).
    environment: Option<EnvironmentReads>,
    /// The global object's own data field a global-name store writes through the record's hint,
    /// where the row is a `StoreName` (`global_name_store`).
    global_store: Option<GlobalStore>,
    /// The lane's pure binary operation (`binary_values`), which answers a `Binary` or an
    /// `Equality` call-out a row makes where its words do not decide it (`vm_step`).
    binary: Option<(*mut (), unsafe fn(*mut (), u32, Value, Value) -> Option<Value>)>,
    /// The lane context and the fast answers of an iteration or for-in call-out (`fast_names_step`), for
    /// the row that makes one (`Decoder::Names`).
    names: Option<(*mut (), NamesStep)>,
    /// The environment clone a `CloneEnv` row makes (`clone_environment_of`).
    clones: Option<EnvironmentClones>,
    /// The iterator cleanup stack an `IteratorCleanupPush`/`Pop` row keeps (`iterator_push_in_lane`).
    cleanups: Option<IteratorCleanups>,
    /// The string-case index a `StringCaseIndex` row answers (`case_index_in_lane`).
    string_cases: Option<StringCases>,
    /// The running function, which a `LoadCallee` row reads (`running_callee`), where the row is one.
    callee: Option<(*mut (), unsafe fn(*mut ()) -> Option<Value>)>,
    /// The lane context and the VM's Wasm global load and store, for a staged instance-state row
    /// (`Decoder::WasmState`).
    globals: Option<WasmGlobals>,
    /// The function's linear memory a direct access row addresses (`Decoder::WasmMemory`): the base
    /// and length the view resolved (`MemoryView::extent`).
    memory: Option<(*mut u8, usize)>,
    /// The lane context's resolver of a Wasm memory value, for a generic memory row that names its
    /// memory by register (`Frame::wasm_memory`), where the row is one (`Decoder::WasmMemory`).
    memories: Option<WasmMemories>,
    /// The proof span of the array a proved element row (`FLAG_SET`) reads or stores through, with the
    /// lane context it proves in (`proof_span`).
    element_span: Option<(*mut u8, ElementSpan)>,
    /// The lane context's vector heap callbacks, for a SIMD row (`Decoder::WasmVector`).
    vectors: Option<WasmVectors>,
    /// The constants, local slots and outer captures the staged row's operands name.
    reads: Reads,
    /// The window of an environment activation's run, where a window record reads its locals
    /// (`FLAG_WINDOW`): the register at its base, whose locals follow its environment and slots
    /// words (`WINDOW_LOCALS`). `None` in a frame's run.
    window: Option<*mut Value>,
    /// The plan of the named read the frame runs, as its record holds it (`RecordPlan`).
    plan: Option<RecordPlan>,
    /// The call record a call row runs (`LaneCall`), for the frame's call hooks.
    call: Option<LaneCall>,
    /// The object a `GetField` of a local slot reads, where the local holds a plain value
    /// (`plain_local_operand`); `PropertyFrame::local_base` answers it. `None` otherwise.
    local_object: Option<u64>,
    /// The VM's element steps an element row's host path runs at its site, where the row is one
    /// that accesses an element (`ElementFrame::index_set_host`, `PropertyFrame::index_get_host`).
    elements: Option<ElementSteps>,
    /// The VM's geometry and length reads a host read row runs (`GeometrySteps`), where the row is
    /// one that reads a view's geometry or a length (`TYPED_LENGTH_SITE`, `GEOMETRY_SITE`).
    geometry: Option<GeometrySteps>,
    /// The VM's stores and adds a named store row joins to, for a store record (`FieldStores`).
    stores: Option<FieldStores>,
    /// The form hint the VM's own-field store left for the store record, which the row writes to
    /// the record once the store completes (`PropertyFrame::field_store_host`).
    stored: Option<crate::vm::FieldCache>,
    /// The element kind a typed element row (`TYPED_SITE`) observed, and the typed array record
    /// pages the view proof reads (`Heap::typed_view_directory`); none elsewhere.
    typed: Option<(crate::staged::typed_elements::NativeKind, (*const usize, usize))>,
    /// The VM's Math calls, for a Math row (`PropertyFrame::math_call`); none elsewhere.
    math: Option<MathCalls>,
    /// The VM's entry lookups the running lane makes at the row's site, for a row whose plan takes
    /// one (`PropertyFrame::field_entry`); none elsewhere.
    entries: Option<EntryReads>,
    /// The lane context and the completion a raw `WasmReturn64` row hands its returned word
    /// (`lane_complete_wasm_activation`), where the row is one (`returns_raw_row`). `None` otherwise.
    wasm_return: Option<(*mut (), unsafe fn(*mut (), Value) -> Option<Resumed>)>,
    /// The row's double result keeps its double bits, not an int32 canonical form: set for a fused
    /// record's raw producer (its `mode` raw mask: `raw_bit(0)` for `A`, or a last link with `MODE_RAW_OUT`),
    /// whose one reader is the next Number row. A NaN still canonicalizes (`write`).
    raw_result: bool,
    /// How the running activation suspends in place, for a staged suspension row.
    suspends: Option<LaneSuspends>,
    /// The activation entries of the running lane context, for a staged prologue row (`PrologueFrame`);
    /// none elsewhere.
    entry: Option<EntryHost>,
}

/// The VM's entry lookups of the running lane at the row's site `ip`, erased of its host so the
/// frame names no `H`: `field_entry_read` makes the one `kind` names. Runs no guest code.
#[derive(Clone, Copy)]
struct EntryReads {
    context: *mut (),
    site: Ip,
    read: fn(*mut (), Ip, Value, crate::staged::property::EntryKind) -> Option<Value>,
}

impl EntryReads {
    /// The entry lookups of the running lane's `cx` at the row's site `ip`.
    #[inline(always)]
    fn of<H: Host>(cx: *mut LaneContext<H>, ip: Ip) -> Self {
        Self { context: cx.cast(), site: ip, read: super::field_entry_read::<H> }
    }
}

/// The element steps of the running lane, erased of its host so the frame names no `H`: the
/// read and the store an element row's host path makes at its site `ip` (`row_index_read`,
/// `row_index_write`). Neither runs guest code; a refusal leaves to the row's miss.
#[derive(Clone, Copy)]
struct ElementSteps {
    context: *mut (),
    site: Ip,
    read: fn(*mut (), Ip, Value, Value) -> Option<Value>,
    write: fn(*mut (), Ip, Value, Value, Value) -> bool,
}

impl ElementSteps {
    /// The element steps of the running lane's `cx` at the row's site `ip`.
    #[inline(always)]
    fn of<H: Host>(cx: *mut LaneContext<H>, ip: Ip) -> Self {
        Self { context: cx.cast(), site: ip, read: row_index_read::<H>, write: row_index_write::<H> }
    }
}

/// The VM's geometry and length reads of the running lane, erased of its host so the frame names
/// no `H`: the intrinsic geometry getter of a name on a receiver (`geometry_read_in_lane`), and the
/// `length` of a receiver that is no array or string (`length_read`). Neither runs guest code; a
/// refusal leaves to the row's miss.
#[derive(Clone, Copy)]
struct GeometrySteps {
    context: *mut (),
    geometry: fn(*mut (), Value, u32) -> Option<Value>,
    length: fn(*mut (), Value) -> Option<Value>,
}

impl GeometrySteps {
    /// The geometry steps of the running lane's `cx`.
    #[inline(always)]
    fn of<H: Host>(cx: *mut LaneContext<H>) -> Self {
        Self { context: cx.cast(),
            geometry: |context, base, atom| geometry_read_in_lane(unsafe { vm(context.cast::<LaneContext<H>>()) }, base, atom),
            length: |context, base| length_read(unsafe { vm(context.cast::<LaneContext<H>>()) }, base) }
    }
}

/// The proof span of an array a proved element row names (`dispatch_fast::proof_span`): the lane
/// context's bytes, the base register, its value, and whether a store writes it.
type ElementSpan = unsafe fn(*mut u8, u16, Value, bool) -> Option<(std::ptr::NonNull<Value>, usize)>;

/// The lane context's resolver of a Wasm memory value for a generic memory row: the bytes of the
/// memory the value names, and whether its index is an i64 (`wasm_memory_of`).
#[derive(Clone, Copy)]
struct WasmMemories {
    context: *mut (),
    resolve: unsafe fn(*mut (), Value) -> Option<(*mut u8, usize, bool)>,
}

/// `WasmMemories::resolve`: the bytes of the Wasm memory `value` names, as `direct_memory` gives
/// them, and whether its index type is i64. `None` where the memory is not addressed directly.
///
/// # Safety
/// `context` is the running lane's context.
unsafe fn wasm_memory_of<H: Host>(context: *mut (), value: Value) -> Option<(*mut u8, usize, bool)> {
    let vm = unsafe { vm(context.cast::<LaneContext<H>>()) };
    let (bytes, length) = direct_memory(vm, value)?;
    let wide = vm.wasm_memory_index_type(value).ok()? == crate::WasmType::I64;
    Some((bytes, length, wide))
}

/// The lane context's Wasm global rules for a staged instance-state row: the VM's load of the
/// global bound in a binding and its store (`wasm_global_load`, `wasm_global_store`).
#[derive(Clone, Copy)]
struct WasmGlobals {
    context: *mut (),
    load: unsafe fn(*mut (), Value) -> Option<Value>,
    store: unsafe fn(*mut (), Value, Value) -> bool,
    /// The running instance's slot a `WasmInstanceBinding` row reads (`wasm_instance_slot`).
    instance_slot: unsafe fn(*mut (), u32) -> Option<Value>,
}

/// The lane context's heap callbacks for the vector rows (`Decoder::WasmVector`): the 16 bytes of a
/// vector cell, and a fresh vector cell of given bytes.
#[derive(Clone, Copy)]
struct WasmVectors {
    context: *mut (),
    read: unsafe fn(*mut (), Value) -> Option<[u8; 16]>,
    alloc: unsafe fn(*mut (), [u8; 16]) -> Option<Value>,
}

/// The running frame's captured environment for a staged environment row: the storage of its
/// slots, which `read` finds for a read and `write` for a store (it takes the environment's barrier).
#[derive(Clone, Copy)]
struct EnvironmentReads {
    context: *mut (),
    read: unsafe fn(*mut ()) -> Option<*mut crate::heap::EnvironmentSlots>,
    write: unsafe fn(*mut (), u16) -> Option<*mut crate::heap::EnvironmentSlots>,
}

/// The lane context's iterator cleanup stack for an `IteratorCleanupPush`/`Pop` row (`iterator_push_in_lane`,
/// `iterator_pop_in_lane`), erased of its host so the frame names no `H`.
#[derive(Clone, Copy)]
struct IteratorCleanups {
    context: *mut (),
    push: unsafe fn(*mut (), Register, Register),
    pop: unsafe fn(*mut ()) -> bool,
}

/// A row's global-name store: the lane context and the record's own hint, which the store
/// writes through (`global_name_store`).
#[derive(Clone, Copy)]
struct GlobalStore {
    cx: *mut (),
    hint: crate::vm::FieldCache,
    store: unsafe fn(*mut (), crate::vm::FieldCache, u32, Value) -> bool,
}


/// A named read's plan as a lane record holds it: the ops the record's own-field hint proves
/// (`staged::ic`), over the object record table the lane bound. The generated row reads it as a
/// specializer reads the plan its site proved (`PlanSource`), so the record's quickened state is
/// data the one staged row runs, not a handler written for it.
#[derive(Clone, Copy)]
pub(in crate::vm::dispatch_fast) struct RecordPlan {
    ops: [crate::staged::ic::IcOp<'static>; 2],
    /// How many of `ops` the plan runs.
    len: usize,
    records: u64,
    /// The heap whose arrays, functions and ordinary builtins a plan any receiver runs
    /// (`IcOp::Held`) finds the object part of (`Heap::held_other_receiver`); `None` where the
    /// plan runs over direct objects alone.
    other: Option<*const crate::heap::Heap>,
    /// Whether the plan starts from the receiver's word rather than its object record
    /// (`IcOp::HeldString`).
    word: bool,
    /// The store the record's hint proves (`StorePlan::Layout`), or the add it proves
    /// (`StorePlan::Add`); `None` for a read's plan.
    store: Option<crate::staged::property::StorePlan<'static>>,
    /// Whether the plan's receivers are mostly functions (`own_other`): their object part is
    /// found inline, before any other family.
    functions: bool,
    /// A nested read `a.b.c` of two own hops (`FieldPlan::Chain`): each hop's layout and slot,
    /// the first from the record's hint and the second from its site's entry, and the base the
    /// first reads (`PropertyFrame::nested_base`). `None` for every other plan.
    nested: Option<([(u32, u16); 2], crate::bytecode::FieldBase)>,
    /// A read the frame's host answers (`FieldPlan::Geometry`, `FieldPlan::Length`), which addresses
    /// no record: the receiver's word is its operand. `None` for every other plan.
    host: Option<crate::staged::property::FieldPlan<'static>>,
    /// The VM's entry lookup the plan takes where its ops miss, or alone where `len` is zero
    /// (`FieldPlan::Entry`, `FieldPlan::Hinted`); `None` for every other plan.
    entry: Option<crate::staged::property::EntryKind>,
}

impl RecordPlan {
    /// The polymorphic read of the record's hint `(layout, slot)`: the own-slot read of the hint, else
    /// the site's inline or hashed entries through the VM (`EntryKind::Polymorphic`).
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn polymorphic(layout: u32, slot: u16, records: u64) -> Self {
        Self { entry: Some(crate::staged::property::EntryKind::Polymorphic), ..Self::own(layout, slot, records) }
    }

    /// A read the VM's entry lookup of `kind` answers alone, over the object records the lane bound.
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn entry(kind: crate::staged::property::EntryKind, records: u64) -> Self {
        let none = crate::staged::ic::IcOp::GuardLayout(0);
        Self { ops: [none, none], len: 0, records, other: None, word: false, store: None, functions: false, nested: None,
            host: None, entry: Some(kind) }
    }

    /// The private name's read of the record (`FieldPlan::Private`), whose atom the lookup names.
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn private(atom: u32) -> Self {
        Self::host_read(crate::staged::property::FieldPlan::Private { atom })
    }
}

impl RecordPlan {
    /// The own-slot read of the hint `(layout, slot)`: one compare of the record's layout id,
    /// whose storage form the site fixed where it fixed one (`own_formed_handler`).
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn own(layout: u32, slot: u16, records: u64) -> Self {
        use crate::staged::ic::IcOp;
        let load = IcOp::LoadSlot { slot, form: crate::staged::ic::form_of_layout(layout), storage_len: usize::from(slot) + 1,
            rep: crate::typed_ir::Representation::Tagged };
        Self { ops: [IcOp::GuardLayout(layout), load], len: 2, records, other: None, word: false, store: None, functions: false, nested: None, host: None, entry: None }
    }

    /// `own` of a receiver of any object part (`Heap::held_other_receiver`): a function's record
    /// takes the same layout compare.
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn own_other(layout: u32, slot: u16, records: u64, heap: *const crate::heap::Heap) -> Self {
        Self { other: Some(heap), functions: true, ..Self::own(layout, slot, records) }
    }

    /// The read of one object's own constant (`IcOp::OwnConstant`) from the receiver's word,
    /// which equals the object's own word (`Vm::own_constant_word_operands`): one compare of the
    /// register, no record addressed.
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn own_constant(object: u64, watch: u64, value: u64) -> Self {
        let read = crate::staged::ic::IcOp::OwnConstant { record: object, watch, value };
        Self { ops: [read, read], len: 1, records: 0, other: None, word: true, store: None, functions: false, nested: None, host: None, entry: None }
    }

    /// The `length` of the receiver's word read by `length` (`IcOp::ArrayLength` or
    /// `IcOp::StringLength`), no record addressed before it.
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn length(length: crate::staged::ic::IcOp<'static>) -> Self {
        Self { ops: [length, length], len: 1, records: 0, other: None, word: true, store: None, functions: false, nested: None, host: None, entry: None }
    }

    /// `own_other` of a layout whose storage form `form` the row's site fixed.
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn own_other_formed(layout: u32, form: u32, slot: u16, records: u64,
        heap: *const crate::heap::Heap) -> Self {
        let mut plan = Self::own_other(layout, slot, records, heap);
        plan.ops[0] = crate::staged::ic::IcOp::GuardFormed { id: layout, form };
        plan
    }

    /// The read through a prototype hint whose operands are `guard`, `watch` and `value`
    /// (`IcOp::Held`).
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn held(guard: u64, watch: u64, value: u64, records: u64,
        other: Option<*const crate::heap::Heap>) -> Self {
        use crate::staged::ic::IcOp;
        let held = IcOp::Held { guard, watch, value };
        Self { ops: [held, held], len: 1, records, other, word: false, store: None, functions: false, nested: None, host: None, entry: None }
    }
}

impl RecordPlan {
    /// The own-field store of the hint `(layout, slot)`: one compare of the record's layout id,
    /// whose entry names the storage form (`StorePlan::Layout`); what it refuses is the VM's.
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn own_store(layout: u32, slot: u16, records: u64) -> Self {
        use crate::staged::ic::IcOp;
        use crate::staged::property::{StoreJoin, StorePlan};
        let none = IcOp::GuardLayout(layout);
        let store = StorePlan::Layout { id: layout, slot, join: StoreJoin::Host };
        Self { ops: [none, none], len: 0, records, other: None, word: false, store: Some(store), functions: false, nested: None, host: None, entry: None }
    }

    /// The store the hint `(layout, slot)` proves for a tracked (`narrow` false) or narrow store
    /// (`StorePlan::Hinted`), a refusal leaving to the VM's own-field store.
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn own_hinted(layout: u32, slot: u16, records: u64, narrow: bool) -> Self {
        use crate::staged::property::{StoreJoin, StorePlan};
        let store = StorePlan::Hinted { id: layout, slot, narrow, join: StoreJoin::Host };
        Self { store: Some(store), ..Self::own_store(layout, slot, records) }
    }

    /// The store the hint `(layout, slot)` and the site's polymorphic entries prove
    /// (`StorePlan::Polymorphic`).
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn own_polymorphic(layout: u32, slot: u16, records: u64) -> Self {
        let store = crate::staged::property::StorePlan::Polymorphic { id: layout, slot };
        Self { store: Some(store), ..Self::own_store(layout, slot, records) }
    }

    /// The general cached write of a public name (`StorePlan::Host`): the VM's cached store or
    /// add, which a refusal leaves to the lane's general store.
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn general_store(records: u64) -> Self {
        Self { store: Some(crate::staged::property::StorePlan::Host), ..Self::own_store(0, 0, records) }
    }

    /// The adding store of a named store record whose site may add its property (`StorePlan::Add`):
    /// the VM's proved or cached add completes it (`PropertyFrame::field_add`).
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn own_add() -> Self {
        Self { store: Some(crate::staged::property::StorePlan::Add), ..Self::own_store(0, 0, 0) }
    }

    /// The read of a string receiver through a `String.prototype` hint whose operands are
    /// `watch` and `value` (`IcOp::HeldString`), from the receiver's word.
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn held_string(watch: u64, value: u64) -> Self {
        use crate::staged::ic::IcOp;
        let held = IcOp::HeldString { watch, value };
        Self { ops: [held, held], len: 1, records: 0, other: None, word: true, store: None, functions: false, nested: None, host: None, entry: None }
    }

    /// The read of a number receiver through a `Number.prototype` hint whose operands are `watch`
    /// and `value` (`IcOp::HeldNumber`), from the receiver's word.
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn held_number(watch: u64, value: u64) -> Self {
        use crate::staged::ic::IcOp;
        let held = IcOp::HeldNumber { watch, value };
        Self { ops: [held, held], len: 1, records: 0, other: None, word: true, store: None, functions: false, nested: None, host: None, entry: None }
    }

    /// The own field a getter's hint returns (`IcOp::GetterOwn`), over the object records the lane
    /// bound: the receiver's layout `guard`, the watch on its prototype and the slot `slot`.
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn getter(guard: u64, watch: u64, slot: u16, records: u64) -> Self {
        use crate::staged::ic::IcOp;
        let read = IcOp::GetterOwn { guard, watch, slot };
        Self { ops: [read, read], len: 1, records, other: None, word: false, store: None, functions: false, nested: None, host: None, entry: None }
    }
}

impl RecordPlan {
    /// The nested read `a.b.c` of two own hops: `first` the record's hint (the layout and slot
    /// of the first object), `second` the site's entry for the intermediate object, and `base`
    /// the base the first hop reads. One layout compare and one slot load per hop.
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn nested(first: (u32, u16), second: (u32, u16), base: crate::bytecode::FieldBase,
        records: u64) -> Self {
        let none = crate::staged::ic::IcOp::GuardLayout(first.0);
        Self { ops: [none, none], len: 0, records, other: None, word: false, store: None, functions: false,
            nested: Some(([first, second], base)), host: None, entry: None }
    }

    /// A read the frame's host answers (`plan`, a `FieldPlan::Geometry` or `FieldPlan::Length`):
    /// no record is addressed, so the plan's operands are the receiver's word alone.
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn host_read(plan: crate::staged::property::FieldPlan<'static>) -> Self {
        let none = crate::staged::ic::IcOp::GuardShape(0);
        Self { ops: [none, none], len: 0, records: 0, other: None, word: false, store: None, functions: false, nested: None, host: Some(plan), entry: None }
    }
}

/// The plans a lane frame holds: the record's own read, its store, its nested chain, or its host read, or none.
#[derive(Clone, Copy)]
pub(in crate::vm::dispatch_fast) struct LanePlans {
    read: Option<([crate::staged::ic::IcOp<'static>; 2], usize)>,
    store: Option<crate::staged::property::StorePlan<'static>>,
    nested: Option<[(u32, u16); 2]>,
    host: Option<crate::staged::property::FieldPlan<'static>>,
    entry: Option<crate::staged::property::EntryKind>,
}

impl crate::staged::property::PlanSource for LanePlans {
    #[inline(always)]
    fn field_plan(&self, _lookup: crate::bytecode::FieldLookup) -> Option<crate::staged::property::FieldPlan<'_>> {
        if let Some(host) = self.host { return Some(host); }
        use crate::staged::property::FieldPlan;
        match (self.read.as_ref(), self.entry) {
            (Some((ops, len)), Some(entry)) if *len > 0 => Some(FieldPlan::Hinted { ops: &ops[..*len], entry }),
            (_, Some(entry)) => Some(FieldPlan::Entry(entry)),
            (read, None) => read.map(|(ops, len)| FieldPlan::Ops(&ops[..*len])),
        }
    }
    /// A nested read's two own hops, as the record holds them.
    #[inline(always)]
    fn nested_hops(&self) -> Option<[(u32, u16); 2]> { self.nested }
    #[inline(always)]
    fn store_plan(&self, _site: u16) -> Option<crate::staged::property::StorePlan<'_>> {
        // The VM's own-field hit completes what the layout store refuses (`StoreJoin::Host`), and
        // the VM's adds complete an add record's store (`StorePlan::Add`).
        self.store
    }
}

impl<const ACC: u8> LaneFrame<ACC> {
    /// A frame for an operation that reads no `this`.
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn new(registers: Registers, view: Carried) -> Self {
        Self { registers, view, accumulated: NO_ACCUMULATOR, this: Value::DELETED, locals: None, host: None, captures: None, decisions: None,
            capture_slots: None, heap: None, frame_this: None, arguments: None, kept_binding: None, kept: 0, scope: None, environment: None, global_store: None, binary: None, names: None, clones: None, cleanups: None, string_cases: None, callee: None, globals: None, memory: None, memories: None, element_span: None, vectors: None, reads: Reads::NONE, window: None, plan: None, local_object: None, elements: None, geometry: None, stores: None, stored: None, typed: None, math: None, entries: None, wasm_return: None, raw_result: false, suspends: None, entry: None, call: None }
    }

    /// A frame whose allocation and deletion rows make their values in the lane context `cx`.
    #[inline(always)]
    fn hosted<H: Host>(registers: Registers, view: Carried, cx: *mut LaneContext<H>) -> Self {
        let host = LaneHost { cx: cx.cast(), allocate: lane_allocation::<H>, define: define_element_in_lane::<H>, delete: delete_value_in_lane::<H>,
            read_name: name_value_in_lane::<H>, type_of: type_of_in_lane::<H>, typeof_name: typeof_string_in_lane::<H>,
            set_name: set_function_name_in_lane::<H>, kept_values: kept_values::<H>, name: None,
            object_like: object_like_in_lane::<H>, awaits: AwaitSource::of::<H>(),
            constant_fields: constant_fields_value::<H> };
        Self { host: Some(host), ..Self::new(registers, view) }
    }

    /// This frame running a by-name load whose record resolved its name to `binding`.
    #[inline(always)]
    fn named(mut self, binding: NameBinding) -> Self {
        if let Some(host) = self.host.as_mut() {
            host.name = Some(binding);
        }
        self
    }

    /// A frame that may read and write the activation's local slots.
    #[inline(always)]
    fn with_locals(registers: Registers, view: Carried, this: Value,
        locals: *mut Value, len: usize) -> Self {
        Self { registers, view, this, locals: Some((locals, len)), host: None, captures: None, decisions: None,
            ..Self::new(registers, view) }
    }
}

/// The field reads a planned named read runs over the running lane's record: the cached read of
/// a site, the private field of a record, and the private store.
impl<const ACC: u8> LaneFrame<ACC> {
    /// `cached_read` of a read whose private name, if any, has the private-name site
    /// `private_site` (`private_slot`); `0` for none, which reads it uncached.
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn cached_read<H: Host>(
        cx: *mut LaneContext<H>,
        code: &ResidualProgram,
        object: Value,
        atom: crate::bytecode::Atom,
        site: u16,
        private_site: usize,
    ) -> Option<Value> {
        let probe = {
            let vm = unsafe { vm(cx) };
            // A string's own `length`, which its chain never shadows, as a tokenizer's
            // `this.input.length` reads: no cache records a primitive's, and the probes below
            // all decline it (`Vm::cached_string_field_hit`).
            if atom == vm.length_atom {
                if let Some(length) = vm.string_length_value(object) {
                    return Some(length);
                }
                // An array's own `length` (`Vm::own_array_length`), which no field cache records
                // either, as `lane_get_length` reads it: a returning or local-based `GetField`
                // (`return rest.length`) otherwise reaches it only past the getter and miss paths.
                if let Some(length) = vm.own_array_length(object) {
                    return Some(Value::length(length));
                }
            }
            let class = vm.atom_class(atom);
            // A private field, which no field cache records, at its site.
            if private_site != 0 && class.contains(super::object::AtomClass::PRIVATE) {
                return Self::private_value(cx, private_site, code, object, atom);
            }
            // A typed array's geometry, which no field cache records (`AtomClass::VIRTUAL_FIELD`).
            if class.contains(super::object::AtomClass::VIRTUAL_FIELD)
                && let Some(value) = vm.typed_array_geometry_read(object, atom)
            {
                return Some(value);
            }
            vm.prepare_named_function_info_lookup(object, atom);
            vm.cached_field_hit(code, object, site)
        };
        match probe {
            super::object::FieldCacheProbe::Hit(value) => Some(value),
            // A proxy's handler answers its reads (`getter_activation_in_lane` runs the trap): no
            // getter entry or data lookup below holds one.
            super::object::FieldCacheProbe::Ineligible if unsafe { vm(cx) }.proxy_parts(object).is_some() => None,
            // An accessor's entry answers before the data lookups, which would walk the chain to
            // reach the same accessor and decline it.
            // A getter the lane cannot answer in place is the activation's to call
            // (`getter_activation_in_lane`); only a virtual field may still answer.
            probe => match unsafe { super::AccessorCall::of(cx).getter_read(object, atom, site) } {
                Some(Some(value)) => Some(value),
                Some(None) => unsafe { vm(cx) }.pure_virtual_field(object, atom),
                // SAFETY: the context outlives the lane run.
                None => Self::cached_miss(unsafe { vm(cx) }, code, object, atom, site, probe, unsafe { (*cx).registers }.env()),
            },
        }
    }

    /// `cached_read` past its probe. Out of line so that no handler inlining the probe
    /// keeps the lookups' stack locals, which would cost it the tail call to the next record.
    #[inline(never)]
    fn cached_miss<H: Host>(
        vm: &mut Vm<H>,
        code: &ResidualProgram,
        object: Value,
        atom: crate::bytecode::Atom,
        site: u16,
        probe: super::object::FieldCacheProbe,
        environment: Value,
    ) -> Option<Value> {
        // A private field reads from the object's private elements before any cache, as the
        // general loop's `GetField` does. Its home follows the running function and the
        // environment its frame would capture from: the lane's frame, or a frameless activation's
        // function and environment, whose frame would hold that environment as its closure's.
        if vm.is_private_name(atom) {
            return match vm.lane_stack.top_frame() {
                None => vm.private_field_value(code, object, atom),
                Some(activation) => {
                    let function = activation.script().map(|_| activation.function())?;
                    vm.memoized_private_field_value(code, environment, function, object, atom)
                }
            };
        }
        match probe {
            super::object::FieldCacheProbe::Hit(value) => Some(value),
            // The order `Vm::get_field_cached` takes: an array's `length` before the lookups.
            super::object::FieldCacheProbe::OrdinaryMiss => vm
                .array_length_field(object, atom)
                // The entries of a site past one shape, whose handler may be a nested site's or a
                // local base's, as the hashed row reads them.
                .or_else(|| vm.polymorphic_field_hit(code, object, site))
                .or_else(|| vm.cell_polymorphic_holder_hit(object, site))
                .or_else(|| vm.probed_field_miss_value(object, atom, site)),
            super::object::FieldCacheProbe::Ineligible => vm
                .array_length_field(object, atom)
                .or_else(|| vm.cached_string_field_hit(code, object, atom, site))
                .or_else(|| vm.cached_primitive_field_hit(object, atom)),
        }
        // A collection's `size` or a buffer view's field, which no cache records
        // (`AtomClass::VIRTUAL_FIELD`).
        .or_else(|| vm.pure_virtual_field(object, atom))
    }

    /// The private-name slot of `object`'s field `atom` at the record address `site`: the
    /// record's last shape and slot under the home its environment finds
    /// (`PrivateSiteHomes::field_slot`). `None` for an object without the field, a private
    /// method's or accessor's brand included.
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn private_slot<H: Host>(cx: *mut LaneContext<H>, site: usize,
        code: &ResidualProgram, object: Value, atom: crate::bytecode::Atom) -> Option<usize> {
        let scope = private_site_in_lane(cx)?;
        (unsafe { vm(cx) }).site_private_field_slot(code, site, scope, object, atom)
    }

    /// A `GetField` record's private field value at the record address `site` (`private_slot`):
    /// `None` for an object without the field, which the general path throws for, or a private
    /// method's or accessor's name, which it reads.
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn private_value<H: Host>(cx: *mut LaneContext<H>, site: usize,
        code: &ResidualProgram, object: Value, atom: crate::bytecode::Atom) -> Option<Value> {
        let slot = Self::private_slot(cx, site, code, object, atom)?;
        let vm = unsafe { vm(cx) };
        vm.heap.property_get(vm.object_data(object)?, slot)
    }

    /// The store of private field `atom` of `object` at the record address `site`, under its
    /// brand there (`private_slot`). `false` leaves the store, a private accessor's or method's
    /// included, to the general path.
    #[inline(never)]
    pub(in crate::vm::dispatch_fast) fn private_store<H: Host>(cx: *mut LaneContext<H>, site: usize,
        code: &ResidualProgram, object: Value, atom: crate::bytecode::Atom, value: Value) -> bool {
        let Some(slot) = Self::private_slot(cx, site, code, object, atom) else {
            return false;
        };
        (unsafe { vm(cx) }).heap.property_set(object, slot, value);
        true
    }
}

/// The operands and operator of a binary call-out the lane's pure binary answers: a `Binary`, or an
/// `Equality` as the operator of its strictness and negation.
fn binary_call(operation: CallOut) -> Option<(Operand, Operand, u32)> {
    match operation {
        CallOut::Binary { left, right, operator } => Some((left, right, operator)),
        CallOut::Equality { left, right, strict, negated } =>
            Some((left, right, crate::vm::operations::equality_binary_operator(strict, negated))),
        _ => None,
    }
}

/// The lane frame's fact about a word: how it becomes a register's `Value`. Staged rows hand an
/// int32 result already boxed (`NumericFrame::int_result`) and a binary64 result as its raw bits
/// (`number_fact`), which the write boxes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::vm::dispatch_fast) enum Boxing {
    Value,
    Double,
}

impl<const ACC: u8> Frame<Concrete> for LaneFrame<ACC> {
    type Fact = Boxing;
    type Control = LaneControl;

    #[inline(always)]
    fn fact(&self, _register: Register) -> Self::Fact { Boxing::Value }

    /// A rest view's read from the values the running keeping activation kept past its
    /// registers (`kept_values`), read in place; a frame keeps none of its own.
    #[inline(always)]
    fn rest_view(&mut self, _stage: &mut Concrete, get: bool, fixed: usize, key: Option<Register>) -> Option<(u64, Self::Fact)> {
        let host = self.host?;
        // SAFETY: the reader was made for the lane context this frame runs in.
        let (argc, first) = unsafe { (host.kept_values)(host.cx) }?;
        // A rest view's fixed parameters are a function's (`Function::params`), a `u16`.
        let fixed = fixed as u16;
        let length = argc.saturating_sub(fixed);
        if !get {
            return Some((Value::integer(i32::from(length)).to_bits(), Boxing::Value));
        }
        // A negative index wraps past every length, so one unsigned compare bounds it.
        let index = self.registers.get(key?).as_int().map(|index| index as u32).filter(|index| *index < u32::from(length))?;
        Some((self.registers.get(first + fixed + index as u16).to_bits(), Boxing::Value))
    }

    #[inline(always)]
    fn read(&mut self, _stage: &mut Concrete, register: Register) -> u64 {
        // The carried accumulator stands for field B, the one register a single-operand row reads;
        // a row that reads several (one with heap records) reads the file, which every producer writes.
        if self.heap.is_some() || register != self.accumulated { return self.registers.get(register).to_bits(); }
        read::<ACC, ACC_B>(self.registers, register, self.view).to_bits()
    }

    #[inline(always)]
    fn write(&mut self, _stage: &mut Concrete, register: Register, value: u64, fact: Self::Fact) {
        let value = match fact {
            Boxing::Value => Value::from_bits(value),
            // A raw result stores its double bits as they are: the next Number row of its raw pair
            // reads them as a double, and nothing else reads the register. A NaN keeps its canonical
            // form: its raw bits could fall in the boxed values' tag space.
            Boxing::Double if self.raw_result && !f64::from_bits(value).is_nan() => Value::from_bits(value),
            Boxing::Double => Value::number(f64::from_bits(value)),
        };
        self.registers.set(register, value);
    }

    #[inline(always)]
    fn this(&mut self, _stage: &mut Concrete) -> u64 {
        if self.this.is_deleted() && let Some((context, read)) = self.frame_this {
            // SAFETY: the running lane's context, live for the row's step.
            return unsafe { read(context) }.unwrap_or(Value::DELETED).to_bits();
        }
        self.this.to_bits()
    }

    #[inline(always)]
    fn this_value(&mut self, stage: &mut Concrete) -> Option<(u64, Boxing)> {
        let this = self.this(stage);
        (this != Value::DELETED.to_bits()).then_some((this, Boxing::Value))
    }

    #[inline(always)]
    fn define_this(&mut self, _stage: &mut Concrete, destination: Register, this: u64) {
        self.registers.set(destination, Value::from_bits(this));
    }

    #[inline(always)]
    fn constant(&mut self, _stage: &mut Concrete, constant: Constant) -> (u64, Self::Fact) {
        (constant.value.to_bits(), Boxing::Value)
    }

    #[inline(always)]
    fn truthiness(&mut self, stage: &mut Concrete, register: Register) -> Option<bool> {
        Value::from_bits(self.read(stage, register)).truthiness()
    }

    #[inline(always)]
    fn branch(&mut self, _stage: &mut Concrete, condition: bool) -> LaneControl {
        LaneControl::Edge(if condition { Edge::Target } else { Edge::Following })
    }

    #[inline(always)]
    fn jump(&mut self, _stage: &mut Concrete) -> LaneControl {
        LaneControl::Edge(Edge::Target)
    }

    #[inline(always)]
    fn branch_table(&mut self, _stage: &mut Concrete, index: u64, entries: u32) -> Option<LaneControl> {
        // The selector is an int32 word, zero-extended; any selector past the cases takes the default.
        Some(LaneControl::Edge(Edge::Entry((index as u32).min(entries - 1))))
    }

    #[inline(always)]
    fn boolean(&mut self, _stage: &mut Concrete, condition: bool) -> (u64, Boxing) {
        (Value::boolean(condition).to_bits(), Boxing::Value)
    }

    /// The local `slot` of the running activation: its window's where the run holds one
    /// (`window`), else the frame's. An uninitialized binding (the deleted marker) is the row's miss.
    #[inline(always)]
    fn load_local(&mut self, _stage: &mut Concrete, slot: u16, _miss: Option<()>)
        -> Option<(u64, Boxing)> {
        let value = match self.window {
            // SAFETY: a window record's slot is one of its function's locals, which the window holds
            // after its environment and slots words (`activate_environment_in_lane`).
            Some(window) => unsafe { *window.add(usize::from(super::WINDOW_LOCALS) + usize::from(slot)) },
            None => {
                let (locals, len) = self.locals?;
                if usize::from(slot) >= len {
                    return None;
                }
                // SAFETY: `slot` is inside the activation's live local slots.
                unsafe { *locals.add(usize::from(slot)) }
            }
        };
        (!value.is_deleted()).then_some((value.to_bits(), Boxing::Value))
    }

    #[inline(always)]
    fn allocate_value(&mut self, _stage: &mut Concrete, destination: Register, allocation: Allocation, _proof: ()) -> Option<u64> {
        let host = self.host?;
        // SAFETY: the maker was made for the lane context this frame runs in; a window's closure is
        // made over the window's scope, at the record's ip (`ScopeReads::closure`).
        let value = match (self.scope, &allocation) {
            (Some(scope), Allocation::Closure { .. }) =>
                unsafe { (scope.closure)(scope.context, self.registers, scope.ip, scope.function, scope.window) }?,
            _ => unsafe { (host.allocate)(host.cx, self.registers, allocation) }?,
        };
        self.registers.set(destination, value);
        Some(value.to_bits())
    }

    #[inline(always)]
    fn define_element(&mut self, _stage: &mut Concrete, array: u64, index: u32, value: u64) -> Option<()> {
        let host = self.host?;
        // SAFETY: the definer was made for the lane context this frame runs in.
        unsafe { (host.define)(host.cx, array, index, value) }.then_some(())
    }

    /// The VM's `is_object_like` of `word` (`object_like_in_lane`): any object kind, which a direct
    /// record test does not all prove.
    #[inline(always)]
    fn object_like(&mut self, _stage: &mut Concrete, word: u64) -> Option<()> {
        let host = self.host?;
        // SAFETY: the test was made for the lane context this frame runs in.
        unsafe { (host.object_like)(host.cx, Value::from_bits(word)) }.then_some(())
    }

    /// Writes local `slot` of the running activation, its window's where the run holds one (`window`).
    /// The window's environment is young (`window_environment`) or remembered, so a plain local
    /// needs no barrier: no collection runs in the lane.
    #[inline(always)]
    fn case_index(&mut self, stage: &mut Concrete, source: Register, start: u32, count: u16) -> Option<(u64, Self::Fact)> {
        let cases = self.string_cases?;
        let value = Value::from_bits(self.read(stage, source));
        // SAFETY: the running lane's context, and the filter the record's operand names (null where none).
        let index = unsafe { (cases.index)(cases.context, cases.filter, value, start as usize, usize::from(count)) };
        Some((Value::integer(index as i32).to_bits(), Boxing::Value))
    }

    #[inline(always)]
    fn iterator_push(&mut self, _stage: &mut Concrete, iterator: Register, done: Register) -> Option<()> {
        let cleanups = self.cleanups?;
        // SAFETY: the context the cleanup stack was made for is the running lane's.
        unsafe { (cleanups.push)(cleanups.context, iterator, done) };
        Some(())
    }

    #[inline(always)]
    fn iterator_pop(&mut self, _stage: &mut Concrete) -> Option<()> {
        let cleanups = self.cleanups?;
        // SAFETY: as `iterator_push`.
        unsafe { (cleanups.pop)(cleanups.context) }.then_some(())
    }

    #[inline(always)]
    fn store_local(&mut self, _stage: &mut Concrete, slot: u16, value: u64, _miss: Option<()>) -> bool {
        match self.window {
            // SAFETY: as `load_local`'s read of the window's local.
            Some(window) => unsafe { *window.add(usize::from(super::WINDOW_LOCALS) + usize::from(slot)) = Value::from_bits(value) },
            None => {
                let Some((locals, len)) = self.locals else { return false };
                if usize::from(slot) >= len { return false; }
                // SAFETY: `slot` is inside the activation's live local slots.
                unsafe { *locals.add(usize::from(slot)) = Value::from_bits(value) };
            }
        }
        true
    }

    #[inline(always)]
    fn heap_records(&mut self, _stage: &mut Concrete) -> Option<(u64, u64, u64)> {
        // SAFETY: the running lane's heap, live for the row's step.
        let heap = unsafe { &*self.heap? };
        Some((heap.array_record_base()? as u64, heap.object_record_base()? as u64, heap.collection_epoch()))
    }

    /// The `typeof` kind of the heap value `word` (`type_of_in_lane`), for a `typeof` row.
    #[inline(always)]
    fn type_of(&mut self, _stage: &mut Concrete, word: u64) -> Option<crate::staged::value::TypeofKind> {
        let host = self.host?;
        // SAFETY: the running lane's context, live for the row's step.
        unsafe { (host.type_of)(host.cx, Value::from_bits(word)) }
    }

    /// The interned `typeof` result string of the name index `name` (`typeof_string_in_lane`).
    #[inline(always)]
    fn typeof_string(&mut self, _stage: &mut Concrete, name: u64) -> Option<(u64, Boxing)> {
        let host = self.host?;
        // SAFETY: as `type_of`.
        unsafe { (host.typeof_name)(host.cx, name) }.map(|text| (text.to_bits(), Boxing::Value))
    }

    /// SetFunctionName of `function` (`set_function_name_in_lane`), for a naming row.
    #[inline(always)]
    fn set_function_name(&mut self, _stage: &mut Concrete, function: u64, name: u32, site: u16) -> Option<()> {
        let host = self.host?;
        // SAFETY: as `type_of`.
        unsafe { (host.set_name)(host.cx, Value::from_bits(function), name, site) }
    }

    #[inline(always)]
    fn vm_step(&mut self, stage: &mut Concrete, operation: CallOut, _result: Option<Register>) -> Option<(u64, Boxing)> {
        // A receiver's constant fields test reads one cache entry and the receiver's shape in place.
        if let CallOut::Protocol { step: crate::staged::Protocol::ConstantFieldsHeld { site }, operands: [_, Some(receiver)] } = operation {
            let host = self.host?;
            let receiver = Value::from_bits(self.read(stage, receiver));
            // SAFETY: the host's context is the running lane's.
            let held = unsafe { (host.constant_fields)(host.cx, receiver, site) };
            return Some((Value::boolean(held).to_bits(), Boxing::Value));
        }
        if let Some((context, binary)) = self.binary
            && let Some((left, right, operator)) = binary_call(operation)
        {
            let (left, right) = (self.operand_word(stage, left)?, self.operand_word(stage, right)?);
            let (left, right) = (Value::from_bits(left), Value::from_bits(right));
            // SAFETY: the running lane's context, live for the row's step.
            return unsafe { binary(context, operator, left, right) }.map(|value| (value.to_bits(), Boxing::Value));
        }
        match operation {
            CallOut::Decide { left, right, operator } => {
                let Decisions { context, decide, .. } = self.decisions?;
                let (left, right) = (Value::from_bits(self.read(stage, left)), Value::from_bits(self.read(stage, right)));
                decide(context, operator, left, right).map(|holds| (Value::boolean(holds).to_bits(), Boxing::Value))
            }
            CallOut::Protocol { .. } | CallOut::ForInNext { .. } | CallOut::PropertyKey { .. } => {
                let (context, step) = self.names?;
                let operands = operation.operands().fixed;
                let operands = operands.map(|operand| operand.and_then(|operand| self.operand_word(stage, operand)).map(Value::from_bits));
                // SAFETY: the running lane's context, live for the row's step.
                unsafe { step(context, operation, operands) }.map(|value| (value.to_bits(), Boxing::Value))
            }
            // The clone defines no register; its word is a placeholder the row does not write.
            CallOut::CloneEnv { plan } => {
                let EnvironmentClones { context, window, run } = self.clones?;
                // SAFETY: the running lane's context and register file, live for the row's step.
                unsafe { run(context, self.registers, window, plan) }.then_some((Value::UNDEFINED.to_bits(), Boxing::Value))
            }
            _ => None,
        }
    }

    #[inline(always)]
    fn arguments_length(&mut self, _stage: &mut Concrete, slot: u16) -> Option<(u64, Boxing)> {
        let (context, count) = self.arguments?;
        // SAFETY: the running lane's context, live for the row's step.
        Some((unsafe { count(context, slot) }?.to_bits(), Boxing::Value))
    }

    /// The kept value at `index` of a keeping activation's `arguments` binding in local `slot`: the
    /// value after the running function's registers (`kept`), in range of the activation's count.
    #[inline(always)]
    fn argument(&mut self, _stage: &mut Concrete, slot: u16, index: u64) -> Option<(u64, Boxing)> {
        let (context, count) = self.arguments?;
        // SAFETY: the running lane's context, live for the row's step.
        let count = unsafe { count(context, slot) }?.as_int()?;
        let index = Value::from_bits(index).as_int().and_then(|index| u16::try_from(index).ok())?;
        if i32::from(index) >= count {
            return None;
        }
        Some((self.registers.get(self.kept + index).to_bits(), Boxing::Value))
    }

    /// The keeping activation's `arguments` binding in local `slot` (`kept_binding`): the marker a forwarding
    /// read passes on, the object it makes at its last read, or a frame's binding as the Vm reads it.
    #[inline(always)]
    fn arguments_binding(&mut self, _stage: &mut Concrete, slot: u16) -> Option<(u64, Boxing)> {
        let (context, binding) = self.kept_binding?;
        // SAFETY: the running lane's context and register file, live for the row's step.
        Some((unsafe { binding(context, self.registers, slot) }?.to_bits(), Boxing::Value))
    }

    /// `value instanceof constructor` by the VM's proof of the constructor's chain, else the ordinary walk
    /// (`Decisions::instance`), which runs no guest code; `None` where only the full algorithm decides.
    #[inline(always)]
    fn instance_of(&mut self, _stage: &mut Concrete, value: u64, constructor: u64) -> Option<bool> {
        let Decisions { context, instance, .. } = self.decisions?;
        instance(context, Value::from_bits(value), Value::from_bits(constructor))
    }

    /// The by-name read through the window's scope (`ScopeReads::name`): the destination and the call
    /// receiver are written by the callout, which finds the binding the scope holds.
    #[inline(always)]
    fn scoped_name(&mut self, _stage: &mut Concrete, destination: Register, receiver: Option<Register>, atom: u32) -> Option<()> {
        let ScopeReads { context, ip, name, site, .. } = self.scope?;
        // SAFETY: the running lane's context and register file, live for the row's step.
        unsafe { name(context, self.registers, ip, atom, destination, receiver, site) }.then_some(())
    }

    /// The store of `value` to the `with` object the window's scope holds the name in (`ScopeReads::store`).
    #[inline(always)]
    fn scoped_store(&mut self, _stage: &mut Concrete, atom: u32, value: u64) -> Option<()> {
        let ScopeReads { context, ip, store, .. } = self.scope?;
        // SAFETY: as `scoped_name`.
        unsafe { store(context, self.registers, ip, atom, value) }.then_some(())
    }

    /// The store of `value` to local `slot` through the window's scope (`ScopeReads::local`).
    #[inline(always)]
    fn scoped_local_store(&mut self, _stage: &mut Concrete, slot: u16, value: u64) -> Option<()> {
        let ScopeReads { context, ip, local, .. } = self.scope?;
        // SAFETY: as `scoped_name`.
        unsafe { local(context, self.registers, ip, slot, value) }.then_some(())
    }

    #[inline(always)]
    fn callee_value(&mut self, _stage: &mut Concrete) -> Option<(u64, Boxing)> {
        let (context, read) = self.callee?;
        // SAFETY: the running lane's context, live for the row's step.
        Some((unsafe { read(context) }?.to_bits(), Boxing::Value))
    }

    #[inline(always)]
    fn capture_slot(&mut self, _stage: &mut Concrete, depth: u16, slot: u16, root: bool) -> Option<(u64, Boxing)> {
        // The own scope's slot first, each test its own branch: the read is the hot capture.
        if depth == 0 && !root
            && let Some(slots) = self.capture_slots
            // SAFETY: the running function's dense storage, found this run
            // (`LaneContext::capture`); the scoped read retains the bounds and marker checks.
            && let Some(value) = unsafe { crate::heap::EnvironmentSlots::read_storage(slots, slot) }
        {
            return Some((value.to_bits(), Boxing::Value));
        }
        Some((self.reads.capture(depth, slot, root)?.to_bits(), Boxing::Value))
    }

    #[inline(always)]
    fn store_capture(&mut self, _stage: &mut Concrete, depth: u16, slot: u16, value: u64) -> Option<()> {
        let CaptureStores { context, store } = self.captures?;
        store(context, depth, slot, Value::from_bits(value)).then_some(())
    }

    #[inline(always)]
    fn environment(&mut self, _stage: &mut Concrete) -> Option<(u64, Boxing)> {
        let EnvironmentReads { context, read, .. } = self.environment?;
        // SAFETY: the running lane's context, live for the row's step.
        let storage = unsafe { read(context) }?;
        Some((storage as u64, Boxing::Value))
    }

    #[inline(always)]
    fn environment_write(&mut self, _stage: &mut Concrete, slot: u16, value: u64, initialize: bool) -> Option<()> {
        // A window whose names resolve through its scope leaves its local stores to the scope's rules (`scoped_local_store`).
        if self.scope.is_some() {
            return None;
        }
        let EnvironmentReads { context, write, .. } = self.environment?;
        // SAFETY: as `environment`; `write` took the environment's barrier, and no collection runs in the lane.
        let storage = unsafe { write(context, slot) }?;
        // An initializing store defines over the binding's dead zone; any other stores to a live binding.
        let value = Value::from_bits(value);
        let stored = if initialize {
            unsafe { crate::staged::environment::concrete_environment_slot_define(storage, slot, value) }
        } else {
            unsafe { crate::staged::environment::concrete_environment_slot_store(storage, slot, value) }
        };
        stored.then_some(())
    }

    /// A Wasm scalar read straight from the register file (never the carried accumulator, which
    /// stands for one operand only). An i32 or f32 is its encoded integer; a 64-bit payload is held
    /// in place where untagged; a raw f64 is its word.
    #[inline(always)]
    fn wasm_read(&mut self, _stage: &mut Concrete, register: Register, rep: WasmRep) -> Option<u64> {
        let value = self.registers.get(register);
        Some(match rep {
            WasmRep::Int32 => u64::from(value.as_int()? as u32),
            WasmRep::Bits64 => value.bits64()?,
            WasmRep::RawBits64 | WasmRep::Tagged => value.to_bits(),
        })
    }

    /// Define `register` with a Wasm scalar `word` in `rep`'s representation: an i32 or f32 as an
    /// encoded integer, a boxed 64-bit payload only where it is untagged (else the row leaves), a
    /// raw f64 or a tagged word as it is.
    #[inline(always)]
    fn wasm_write(&mut self, _stage: &mut Concrete, register: Register, word: u64, rep: WasmRep) -> Option<u64> {
        let value = match rep {
            WasmRep::Int32 => self.view.integers.encode(word as u32 as i32),
            WasmRep::Bits64 => Value::from_bits64(word)?,
            WasmRep::RawBits64 | WasmRep::Tagged => Value::from_bits(word),
        };
        self.registers.set(register, value);
        Some(value.to_bits())
    }

    /// A raw register's word, read as it is: a raw i64 or f64 record's operand (`raw_representation`)
    /// is its machine word, so no tag is checked and nothing is unboxed.
    #[inline(always)]
    fn raw_read(&mut self, _stage: &mut Concrete, register: Register) -> Option<u64> {
        Some(self.registers.get(register).to_bits())
    }

    /// Define `register` with the raw machine word `word`, as the raw record's result: stored as the
    /// register's bits, with no boxing (a word a tag claims is stored as it is).
    #[inline(always)]
    fn raw_write(&mut self, _stage: &mut Concrete, register: Register, word: u64) -> Option<u64> {
        self.registers.set(register, Value::from_bits(word));
        Some(word)
    }

    /// The row of a raw return (`returns_raw_row`) holds the Wasm completion it hands its word to.
    #[inline(always)]
    fn returns_raw(&self) -> bool {
        self.wasm_return.is_some()
    }

    /// A raw return's word completes the Wasm activation it returns from (`lane_complete_wasm_activation`),
    /// the word is the returned value where no tag claims it. The resumption
    /// is handed to `generated_row` (`LaneControl::Resumed`). A tag-claimed word, or no Wasm activation to
    /// return to, leaves the row to its miss.
    #[inline(always)]
    fn return_raw(&mut self, stage: &mut Concrete, register: Register) -> Option<LaneControl> {
        let (context, complete) = self.wasm_return?;
        let value = Value::from_bits64(self.raw_read(stage, register)?)?;
        // SAFETY: the running lane's context, which `row_frame` took the completion from.
        let resumed = unsafe { complete(context, value) }?;
        Some(LaneControl::Resumed(resumed))
    }

    /// The VM's load of the global bound in `binding`, where the frame holds the rules (`globals`).
    #[inline(always)]
    fn wasm_global_load(&mut self, _stage: &mut Concrete, binding: u64) -> Option<u64> {
        let globals = self.globals?;
        // SAFETY: the context is the running lane's, which `row_frame` took it from.
        let value = unsafe { (globals.load)(globals.context, Value::from_bits(binding))? };
        Some(value.to_bits())
    }

    /// The VM's store of `value` to the mutable global bound in `binding`, where the frame holds the
    /// rules (`globals`); a binding that is no such global leaves the row to its miss.
    #[inline(always)]
    fn wasm_global_store(&mut self, _stage: &mut Concrete, binding: u64, value: u64) -> Option<()> {
        let globals = self.globals?;
        // SAFETY: as for `wasm_global_load`.
        let stored = unsafe { (globals.store)(globals.context, Value::from_bits(binding), Value::from_bits(value)) };
        stored.then_some(())
    }

    /// The running instance's slot `slot`, as its register encoding, where the lane holds the
    /// instance-state rules (`globals`).
    #[inline(always)]
    fn wasm_instance_slot(&mut self, _stage: &mut Concrete, slot: u32) -> Option<u64> {
        let globals = self.globals?;
        // SAFETY: the context is the running lane's, which `row_frame` took it from.
        let value = unsafe { (globals.instance_slot)(globals.context, slot) }?;
        Some(value.to_bits())
    }

    /// The 16 bytes of the vector `register` holds, where the lane holds the vector rules (`vectors`).
    #[inline(always)]
    fn vector(&mut self, _stage: &mut Concrete, register: Register) -> Option<[u8; 16]> {
        let vectors = self.vectors?;
        // SAFETY: the context is the running lane's, which `row_frame` took it from.
        unsafe { (vectors.read)(vectors.context, self.registers.get(register)) }
    }

    /// The 16 bytes of the vector constant `constant` names: the constant's word, read as a vector.
    #[inline(always)]
    fn vector_constant(&mut self, stage: &mut Concrete, constant: Constant) -> Option<[u8; 16]> {
        let vectors = self.vectors?;
        let (word, _) = <Self as Frame<Concrete>>::constant(self, stage, constant);
        // SAFETY: as for `vector`.
        unsafe { (vectors.read)(vectors.context, Value::from_bits(word)) }
    }

    /// Define `register` with the vector `bytes` in a fresh cell, answering its word.
    #[inline(always)]
    fn set_vector(&mut self, _stage: &mut Concrete, register: Register, bytes: [u8; 16]) -> Option<u64> {
        let vectors = self.vectors?;
        // SAFETY: as for `vector`.
        let value = unsafe { (vectors.alloc)(vectors.context, bytes) }?;
        self.registers.set(register, value);
        Some(value.to_bits())
    }

    /// The bytes of the function's memory, where the row addresses them directly (`memory`).
    #[inline(always)]
    fn direct_memory(&mut self) -> Option<&mut [u8]> {
        let (bytes, length) = self.memory?;
        if length == 0 {
            return Some(Default::default());
        }
        // SAFETY: `bytes` is the memory the view resolved in this lane run, `length` bytes long; the
        // frame holds no other borrow of it.
        Some(unsafe { std::slice::from_raw_parts_mut(bytes, length) })
    }

    /// The bytes of the Wasm memory value in `register`, resolved through the lane context
    /// (`wasm_memory_of`), and its index type. A memory the lane cannot address leaves the row.
    #[inline(always)]
    fn wasm_memory(&mut self, _stage: &mut Concrete, register: Register) -> Option<(&mut [u8], bool)> {
        let WasmMemories { context, resolve } = self.memories?;
        // SAFETY: the context is the running lane's, live for the row's step.
        let (bytes, length, wide) = unsafe { resolve(context, self.registers.get(register)) }?;
        if length == 0 {
            return Some((Default::default(), wide));
        }
        // SAFETY: `bytes` is the memory the value names in this lane run, `length` bytes long; the
        // frame holds no other borrow of it.
        Some((unsafe { std::slice::from_raw_parts_mut(bytes, length) }, wide))
    }
}

/// The running frame's environment storage and the environment that holds it, where a staged
/// environment row may read its slots: a captured frame of a non-root function whose every local
/// lives in its environment and whose `arguments` alias no parameter, in a run that holds no
/// environment window. `store` names the slot a write targets, which the function's self binding
/// keeps from the row.
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
unsafe fn environment_of<H: Host>(cx: *mut LaneContext<H>, store: Option<u16>)
    -> Option<(*mut crate::heap::EnvironmentSlots, Value)> {
    if !unsafe { (*cx).registers }.env().is_deleted() {
        return None;
    }
    let storage = super::frame_storage_in_lane(cx)?;
    let vm = unsafe { vm(cx) };
    let frame = &vm.frames[unsafe { (*cx).frame() }];
    let function = unsafe { &*(*cx).code }.function(frame.function as usize);
    if function.selective_capture_slots.is_some() || function.arguments_alias_parameters()
        || store.is_some_and(|slot| function.is_self_binding_slot(usize::from(slot))) {
        return None;
    }
    Some((std::ptr::from_mut(storage), frame.env))
}

/// `EnvironmentReads::read`: the storage `environment_of` finds for a read.
///
/// # Safety
/// `cx` is the running lane's context.
unsafe fn environment_read<H: Host>(cx: *mut ()) -> Option<*mut crate::heap::EnvironmentSlots> {
    unsafe { environment_of::<H>(cx.cast(), None) }.map(|(storage, _)| storage)
}

/// `EnvironmentReads::write`: the storage `environment_of` finds for a store to `slot`, after the
/// environment's write barrier (`Heap::remember_cell`), as the general store takes it.
///
/// # Safety
/// `cx` is the running lane's context.
unsafe fn environment_write_storage<H: Host>(cx: *mut (), slot: u16) -> Option<*mut crate::heap::EnvironmentSlots> {
    let (storage, environment) = unsafe { environment_of::<H>(cx.cast(), Some(slot)) }?;
    unsafe { vm(cx.cast::<LaneContext<H>>()) }.heap.remember_cell(environment);
    Some(storage)
}

/// The by-name load's binding as the row asks for it: the global `undefined` is a constant; any
/// other is the lane's own read (`GlobalFrame::global_read`) of what its record resolved.
impl<const ACC: u8> crate::staged::GlobalFrame<Concrete> for LaneFrame<ACC> {
    #[inline(always)]
    fn global_binding(&mut self, _proof: ()) -> GlobalBinding {
        match self.host.and_then(|host| host.name) {
            Some(NameBinding::Undefined) => GlobalBinding::Constant(Value::UNDEFINED),
            _ => GlobalBinding::Dynamic,
        }
    }

    #[inline(always)]
    fn global_constant(&mut self, _stage: &mut Concrete, destination: Register, value: Value) -> Option<()> {
        self.registers.set(destination, value);
        Some(())
    }

    #[inline(always)]
    fn global_read(&mut self, _stage: &mut Concrete, destination: Register, atom: u32, _site: u16, _ty: Type, _proof: ()) -> Option<()> {
        let host = self.host?;
        let Some(NameBinding::Read(read)) = host.name else { return None };
        // SAFETY: the reader was made for the lane context this frame runs in.
        let value = unsafe { (host.read_name)(host.cx, read, atom) }?;
        self.registers.set(destination, value);
        Some(())
    }

    /// `LoadNameTypeof`'s read of the name: the binding's value, where the binding answers it. An
    /// absent name answers nothing, which the row's miss settles (`typeof` reads it as `undefined`).
    #[inline(always)]
    fn global_typeof(&mut self, _stage: &mut Concrete, destination: Register, atom: u32, _site: u16) -> Option<()> {
        let host = self.host?;
        let value = match host.name? {
            NameBinding::Undefined => Value::UNDEFINED,
            // SAFETY: the reader was made for the lane context this frame runs in.
            NameBinding::Read(read) => unsafe { (host.read_name)(host.cx, read, atom) }?,
        };
        self.registers.set(destination, value);
        Some(())
    }
    #[inline(always)]
    fn global_write(&mut self, _stage: &mut Concrete, _source: Register, atom: u32, _site: u16, value: u64, _initialize: bool) -> Option<()> {
        let GlobalStore { cx, hint, store } = self.global_store?;
        // SAFETY: the store was made for the lane context this frame runs in.
        unsafe { store(cx, hint, atom, Value::from_bits(value)) }.then_some(())
    }
}

/// The store of `value` to the global object's own writable data field `atom` that `hint` names
/// (`GlobalFrame::global_write`): it runs no guest code, and `false` where the hint misses.
///
/// # Safety
/// `cx` is the running lane's context.
unsafe fn global_name_store<H: Host>(cx: *mut (), hint: crate::vm::FieldCache, atom: u32, value: Value) -> bool {
    let vm = unsafe { vm(cx.cast::<LaneContext<H>>()) };
    vm.hinted_own_field_store_hit(hint, vm.realm.globals, atom, value)
}

impl<const ACC: u8> crate::staged::PropertyFrame<Concrete> for LaneFrame<ACC> {
    type Plans = LanePlans;

    #[inline(always)]
    fn plans(&self) -> LanePlans {
        LanePlans {
            read: self.plan.filter(|plan| plan.host.is_none()).map(|plan| (plan.ops, plan.len)),
            store: self.plan.and_then(|plan| plan.store),
            nested: self.plan.and_then(|plan| plan.nested.map(|(hops, _)| hops)),
            host: self.plan.and_then(|plan| plan.host),
            entry: self.plan.and_then(|plan| plan.entry),
        }
    }

    /// The nested chain's base: the register the record's site names, or the activation's `this`.
    #[inline(always)]
    fn nested_base(&mut self, stage: &mut Concrete, miss: ()) -> Option<u64> {
        let (_, base) = self.plan?.nested?;
        match base.register_index() {
            Some(register) => Some(self.tagged(stage, register)),
            None => self.field_this(stage, miss),
        }
    }

    /// The activation's `this` as the lane holds it: an unbound one (`Value::DELETED`) is no
    /// direct object, so the receiver's record test refuses it with no test of its own.
    #[inline(always)]
    fn field_this(&mut self, _stage: &mut Concrete, _miss: ()) -> Option<u64> { Some(self.this.to_bits()) }

    /// The plain local a `GetField` of a local slot reads (`row_frame`'s `local_object`).
    #[inline(always)]
    fn local_base(&mut self, _stage: &mut Concrete, _slot: u16, _miss: ()) -> Option<u64> { self.local_object }

    /// A direct object's record in the table, or, where the plan runs over any object part
    /// (`RecordPlan::other`), the object part of an array, function or builtin, out of line.
    #[inline(always)]
    fn receiver_record(&mut self, stage: &mut Concrete, _owner: crate::staged::property::Owner, object: u64, _readable: bool,
        miss: ()) -> Option<crate::staged::ic::Reached<u64, bool>> {
        if let Some(RecordPlan { word: true, .. }) = self.plan {
            return Some(crate::staged::ic::Reached::bare(object));
        }
        let Some(RecordPlan { records, other: Some(heap), functions, .. }) = self.plan else {
            return crate::staged::property::receiver(stage, self, object, miss);
        };
        // SAFETY: the heap is the lane's, live for the read.
        if functions && let Some(record) = unsafe { &*heap }.function_receiver(Value::from_bits(object)) {
            return Some(crate::staged::ic::Reached::bare(record as *const crate::heap::Object as u64));
        }
        if let Some(id) = Value::from_bits(object).direct_object_index() {
            return Some(crate::staged::ic::Reached::bare(crate::staged::object::object_record_at(stage, records, u64::from(id))));
        }
        // SAFETY: as above.
        let record = unsafe { &*heap }.held_other_receiver(Value::from_bits(object))?;
        Some(crate::staged::ic::Reached::bare(record as *const crate::heap::Object as u64))
    }

    /// `base[key]` past the dense and any-backing reads: the VM's element read at the row's site
    /// (`row_index_read`), which serves a hole, a symbol or string key, a typed or string element.
    #[inline(always)]
    fn index_get_host(&mut self, _stage: &mut Concrete, base: u64, key: u64, _miss: ()) -> Option<u64> {
        let steps = self.elements?;
        // SAFETY: the steps were made for the lane context this frame runs in.
        (steps.read)(steps.context, steps.site, Value::from_bits(base), Value::from_bits(key)).map(Value::to_bits)
    }

    /// The intrinsic geometry getter of `atom` on the receiver `object` (`geometry_read_in_lane`).
    #[inline(always)]
    fn geometry_read(&mut self, _stage: &mut Concrete, object: u64, atom: u32, _miss: ()) -> Option<u64> {
        let steps = self.geometry?;
        // SAFETY: the steps were made for the lane context this frame runs in.
        (steps.geometry)(steps.context, Value::from_bits(object), atom).map(Value::to_bits)
    }

    /// The `length` of a receiver that is no array or string (`length_read`).
    #[inline(always)]
    fn length_read(&mut self, _stage: &mut Concrete, object: u64, _miss: ()) -> Option<u64> {
        let steps = self.geometry?;
        // SAFETY: as `geometry_read`.
        (steps.length)(steps.context, Value::from_bits(object)).map(Value::to_bits)
    }

    /// The VM's entry lookup of `kind` for `object` (`EntryReads`): the polymorphic, hashed, holder
    /// or inherited answer at the row's site, which the row's record keeps as its hint where it does.
    #[inline(always)]
    fn field_entry(&mut self, _stage: &mut Concrete, _owner: crate::staged::property::Owner, object: u64,
        kind: crate::staged::property::EntryKind, _miss: ()) -> Option<u64> {
        let entries = self.entries?;
        // SAFETY: the reader was made for the lane context this frame runs in.
        (entries.read)(entries.context, entries.site, Value::from_bits(object), kind).map(Value::to_bits)
    }

    /// The VM's own-field store of `value` to `atom` through `site` (`LaneHelper::FieldSet`): the
    /// form hint it leaves is the record's, which the row writes once the store completes.
    #[inline(always)]
    fn field_store_host(&mut self, _stage: &mut Concrete, object: u64, atom: u32, site: u16, value: u64, _miss: ()) -> Option<()> {
        let stores = self.stores?;
        let object = Value::from_bits(object);
        // An unbound `this` is no object: the store leaves to the general path.
        if object.is_deleted() { return None; }
        let value = Value::from_bits(value);
        match self.plan.and_then(|plan| plan.store)? {
            crate::staged::property::StorePlan::Layout { .. } | crate::staged::property::StorePlan::Hinted { .. } => {
                self.stored = Some((stores.own)(stores.context, object, atom, site, value)?);
                Some(())
            }
            crate::staged::property::StorePlan::Host => (stores.general)(stores.context, object, atom, site, value).then_some(()),
            _ => None,
        }
    }

    /// The store through the record's hint, tracked or narrow (`StorePlan::Hinted`).
    #[inline(always)]
    fn field_store_hinted(&mut self, _stage: &mut Concrete, object: u64, atom: u32, _site: u16, value: u64,
        hint: (u32, u16), narrow: bool) -> Option<()> {
        let stores = self.stores?;
        let object = Value::from_bits(object);
        if object.is_deleted() { return None; }
        let hint = crate::vm::FieldCache::own(hint.0, hint.1);
        (stores.hinted)(stores.context, object, atom, Value::from_bits(value), hint, narrow).then_some(())
    }

    /// The store through the record's hint, the site's polymorphic entries, or the proved add
    /// (`StorePlan::Polymorphic`); the entry a polymorphic store hit is the record's hint.
    #[inline(always)]
    fn field_store_polymorphic(&mut self, _stage: &mut Concrete, object: u64, atom: u32, site: u16, value: u64,
        hint: (u32, u16)) -> Option<()> {
        let stores = self.stores?;
        let object = Value::from_bits(object);
        if object.is_deleted() { return None; }
        let value = Value::from_bits(value);
        let hint = crate::vm::FieldCache::own(hint.0, hint.1);
        if (stores.hinted_own)(stores.context, object, atom, value, hint) { return Some(()); }
        if let Some(own) = (stores.polymorphic)(stores.context, object, atom, site, value) {
            self.stored = Some(own);
            return Some(());
        }
        (stores.proved)(stores.context, object, atom, site, value).then_some(())
    }

    /// The VM's add of `value` to `atom` of `object` through `site` (`LaneHelper::FieldAdd`): the
    /// proved add, else the cached one; a refused add leaves to the general path.
    #[inline(always)]
    fn field_add(&mut self, _stage: &mut Concrete, object: u64, atom: u32, site: u16, value: u64, _miss: ()) -> Option<()> {
        let stores = self.stores?;
        let object = Value::from_bits(object);
        if object.is_deleted() { return None; }
        let value = Value::from_bits(value);
        let added = (stores.proved)(stores.context, object, atom, site, value)
            || (stores.primary)(stores.context, object, atom, site, value);
        added.then_some(())
    }

    /// The VM's Math call of `callee` on the first `count` of `arguments` (`MathCalls`), which a
    /// Math row runs past its field read; a callee or argument the VM does not compute leaves
    /// the call to its record.
    #[inline(always)]
    fn math_call(&mut self, _stage: &mut Concrete, callee: u64, arguments: [u64; 2], count: usize) -> Option<u64> {
        let MathCalls { context, call } = self.math?;
        let arguments = [Value::from_bits(arguments[0]), Value::from_bits(arguments[1])];
        call(context, Value::from_bits(callee), &arguments[..count]).map(Value::to_bits)
    }

    /// The VM's own-field store of `value` to `atom` on `this` through `site` (`FieldStores`'s
    /// `sink`), the store a sinking read makes after its read; an unbound `this` leaves it to the
    /// general path.
    #[inline(always)]
    fn sink_store(&mut self, _stage: &mut Concrete, atom: u32, site: u16, value: u64) -> Option<()> {
        let stores = self.stores?;
        if self.this.is_deleted() { return None; }
        (stores.sink)(stores.context, self.this, atom, site, Value::from_bits(value)).then_some(())
    }

    #[inline(always)]
    fn delete_property(&mut self, _stage: &mut Concrete, base: u64, key: u64, _strict: bool, _miss: ()) -> Option<(u64, Boxing)> {
        let host = self.host?;
        // SAFETY: the deleter was made for the lane context this frame runs in.
        let deleted = unsafe { (host.delete)(host.cx, Value::from_bits(base), Value::from_bits(key)) }?;
        Some((deleted.to_bits(), Boxing::Value))
    }

    /// The live object record table, which an `instanceof` walk and a planned read address.
    #[inline(always)]
    fn object_records(&mut self, _stage: &mut Concrete) -> Option<u64> {
        if let Some(plan) = self.plan { return Some(plan.records); }
        let Decisions { context, records, .. } = self.decisions?;
        records(context)
    }
}

impl<const ACC: u8> crate::staged::ElementFrame<Concrete> for LaneFrame<ACC> {
    /// The array records of the heap an element row reads (`row_frame` binds it).
    #[inline(always)]
    fn array_records(&mut self, _stage: &mut Concrete) -> Option<u64> {
        // SAFETY: the heap is the lane's, live for the row.
        // The live table: a direct array receiver proves it reserved, and any other receiver
        // leaves at the access's family test before it is addressed.
        self.heap.map(|heap| unsafe { (*heap).live_array_records() } as u64)
    }

    /// The VM's element store at the row's site (`row_index_write`): an added element, a hole, a
    /// typed or string-keyed store, a symbol or a plain index property. A refusal leaves to `miss`.
    #[inline(always)]
    fn index_set_host(&mut self, _stage: &mut Concrete, base: u64, key: u64, value: u64, _miss: ()) -> Option<()> {
        let steps = self.elements?;
        // SAFETY: as `index_get_host`.
        (steps.write)(steps.context, steps.site, Value::from_bits(base), Value::from_bits(key), Value::from_bits(value))
            .then_some(())
    }

    /// The lane keeps no views: a read takes the element of any backing in place.
    #[inline(always)]
    fn reads_any_backing(&self) -> bool { true }

    /// The array a proof of the lane's span gives (`proof_span`), where the row is one a proof names;
    /// `None` where the slot does not prove the array, which proves it on the access's own check.
    #[inline(always)]
    fn proved_view(&mut self, stage: &mut Concrete, base: Register, writable: bool) -> Option<crate::staged::elements::DenseView<u64>> {
        let (context, span) = self.element_span?;
        let object = Value::from_bits(self.read(stage, base));
        // SAFETY: the context and its proofs are the running lane's, live for the row.
        let (values, length) = unsafe { span(context, base, object, writable) }?;
        Some(crate::staged::elements::DenseView { values: values.as_ptr() as u64, length: length as u64, packed: false, within: false, double: None, uncounted: false })
    }

    /// The typed view of `base` for the element kind the site observed (`TYPED_SITE`): the view
    /// proof's loads, which leave to the host path (`row_index_read_miss`, `row_index_write`) where
    /// they fail. A row with no typed site keeps no view, as before.
    #[inline(always)]
    fn kept_view(&mut self, _stage: &mut Concrete, base: u64, writable: bool) -> Option<crate::staged::element_rows::ElementView<u64>> {
        let (native, directory) = self.typed?;
        let access = crate::staged::typed_view::ViewAccess { kind: Some(native), writable };
        crate::staged::typed_view::concrete_typed_access(directory, Value::from_bits(base), access)
            .map(crate::staged::element_rows::ElementView::Typed)
    }

    /// The array's write barrier admits `value` unlogged (`Heap::array_store_unlogged`).
    #[inline(always)]
    fn array_store_gate(&mut self, _stage: &mut Concrete, base: u64, value: u64, _miss: ()) -> Option<()> {
        // SAFETY: as `array_records`.
        let heap = unsafe { &*self.heap? };
        heap.array_store_unlogged(Value::from_bits(base), Value::from_bits(value)).then_some(())
    }
}

impl<const ACC: u8> NumericFrame<Concrete> for LaneFrame<ACC> {
    /// A register's word, or the constant or local the record names.
    #[inline(always)]
    fn operand_word(&mut self, stage: &mut Concrete, operand: Operand) -> Option<u64> {
        match operand.register_index() {
            Some(register) => Some(self.read(stage, register)),
            None => self.reads.word(operand).map(Value::to_bits),
        }
    }

    #[inline(always)]
    fn int_operand(&mut self, stage: &mut Concrete, operand: Operand, _miss: ()) -> Option<u64> {
        // A register no accumulator stands for: its tag half-word tested in memory
        // (`Registers::int`), not the whole word loaded and masked.
        if ACC == ACC_NONE && let Some(register) = operand.register_index() {
            return self.registers.int(register).map(|int| u64::from(int as u32));
        }
        let word = self.operand_word(stage, operand)?;
        Value::from_bits(word).as_int().map(|int| u64::from(int as u32))
    }

    /// The program constant a constant operand names (`Reads::word`), which the numeric stage folds.
    #[inline(always)]
    fn constant_operand(&mut self, operand: Operand) -> Option<Value> {
        if operand.kind() == Some(crate::bytecode::OperandKind::Constant) { self.reads.word(operand) } else { None }
    }

    #[inline(always)]
    fn number_operand(&mut self, stage: &mut Concrete, operand: Operand, _miss: ()) -> Option<u64> {
        let word = self.operand_word(stage, operand)?;
        Value::from_bits(word).as_number().map(f64::to_bits)
    }

    /// The constructor's live `prototype` link, where the VM proved its `instanceof` steady: the
    /// interpreter reads it from the objects each time, so it needs no dependency.
    #[inline(always)]
    fn stable_prototype(&mut self, stage: &mut Concrete, constructor: Register) -> Option<u64> {
        let Decisions { context, prototype, .. } = self.decisions?;
        prototype(context, Value::from_bits(self.read(stage, constructor))).map(u64::from)
    }

    /// ToInt32 of a Number operand, as the bitwise operators read it: an int32 is its own ToInt32, so it
    /// is read as an int and never converted to binary64.
    #[inline(always)]
    fn int32_operand(&mut self, stage: &mut Concrete, operand: Operand, _miss: ()) -> Option<u64> {
        let value = Value::from_bits(self.operand_word(stage, operand)?);
        if let Some(int) = value.as_int() {
            return Some(u64::from(int as u32));
        }
        // A double takes the staged row's own ToInt32 (`NumericStage::to_int32`), so the lane
        // computes no conversion of its own.
        let number = value.as_number()?;
        crate::staged::NumericStage::to_int32(stage, number.to_bits(), ())
    }

    #[inline(always)]
    fn prove_number(&mut self, stage: &mut Concrete, register: Register, _miss: ()) -> Option<()> {
        Value::from_bits(self.read(stage, register)).as_number().map(|_| ())
    }

    #[inline(always)]
    fn int_result(&mut self, _stage: &mut Concrete, word: u64) -> u64 {
        Value::integer(word as u32 as i32).to_bits()
    }

    #[inline(always)]
    fn miss(&self) {}
    #[inline(always)]
    fn int_fact(&self) -> Boxing { Boxing::Value }
    /// The lane's registers hold tagged words, so a Number is defined as its boxed word.
    #[inline(always)]
    fn tagged_number_fact(&self) -> Option<Boxing> { Some(Boxing::Value) }
    #[inline(always)]
    fn number_fact(&self) -> Boxing { Boxing::Double }
}

#[path = "lane_frame/staged_ops.rs"]
mod staged_ops;
pub(super) use staged_ops::*;
#[path = "lane_frame/suspends.rs"]
mod suspends;
pub(super) use suspends::*;

/// A lane record read as the staged decoder's fields (`staged::Fields`), its opcode the
/// handler's const generic. The lane admits a register-defining record only without result flags
/// (a returning one takes `returning_handler`), so its `a` is the result register as it is.
#[derive(Clone, Copy)]
struct LaneFields<'a, const OP: u16, const FLAG: u8 = FLAG_UNREAD>(&'a LaneInstruction);

impl<const OP: u16, const FLAG: u8> LaneFields<'_, OP, FLAG> {
    // A chain's absent fourth link (`staged::NO_LINK`) names no opcode; its untaken branch still
    // names this constant, so it stands for `Nop` there.
    const OPCODE: Op = match Op::from_index(OP as usize) {
        Some(op) => op,
        None if OP == crate::staged::NO_LINK => crate::staged::NO_LINK_OP,
        None => panic!("no such opcode"),
    };
    const FLAG_ROLE: bool = matches!(Self::OPCODE.immediate_role(), crate::bytecode::ImmediateRole::BooleanFlag);
    const REGISTER_B: bool = Self::OPCODE.field_layout(crate::bytecode::InstructionField::B).is_register_field();
    const REGISTER_C: bool = Self::OPCODE.field_layout(crate::bytecode::InstructionField::C).is_register_field();
    const RESULT_A: bool = matches!(Self::OPCODE.field_layout(crate::bytecode::InstructionField::A),
        crate::bytecode::FieldLayout::ResultRegister);
    const REGISTER_A: bool = Self::OPCODE.field_layout(crate::bytecode::InstructionField::A).is_register_field();
}

impl<const OP: u16, const FLAG: u8> crate::staged::Fields for LaneFields<'_, OP, FLAG> {
    #[inline(always)]
    fn op(self) -> Op {
        // A constant of the instantiation, so the decode's match folds to the one arm.
        Self::OPCODE
    }
    #[inline(always)] fn result_register(self) -> u16 { self.0.a }
    #[inline(always)] fn register_a(self) -> u16 { self.0.a }
    #[inline(always)] fn register_b(self) -> u16 { self.0.b }
    #[inline(always)]
    fn constant(self, _: crate::staged::DecodeContext<'_>) -> Option<Constant> {
        Some(Constant::untyped(self.0.constant()))
    }
    #[inline(always)]
    fn wide(self) -> WideInstruction {
        // SAFETY: the view admits a record only with its register fields inside the function's
        // registers (`fields_fit_in_lane`), which `REGISTER_MASK` bounds; stated so a row's operand
        // kind test folds.
        unsafe {
            // A record whose result register carries flags runs the general step instead
            // (`derive_lane_view`), and an input register field carries none.
            if Self::RESULT_A || Self::REGISTER_A { std::hint::assert_unchecked(self.0.a <= crate::bytecode::REGISTER_MASK) }
            if Self::REGISTER_B { std::hint::assert_unchecked(self.0.b <= crate::bytecode::REGISTER_MASK) }
            if Self::REGISTER_C { std::hint::assert_unchecked(self.0.c <= crate::bytecode::REGISTER_MASK) }
        }
        // A flag the compiler writes is 0 or 1; any other word reads as set, so the flag's decode
        // is the one test.
        let imm = match (Self::FLAG_ROLE, FLAG) {
            (true, FLAG_CLEAR) => 0,
            (true, FLAG_SET) => 1,
            (true, _) => self.0.imm().min(1),
            // The view proved the immediate's high half zero (`row_near`): a capture of the running
            // function's own scope, whose depth then folds.
            (false, FLAG_SET) => self.0.imm() & u32::from(u16::MAX),
            (false, _) => self.0.imm(),
        };
        // A sinking read's flag (`SET_THIS_REGISTER`) is its row's store, not the read's; the
        // read decodes by its register alone.
        let a = if const { FLAG == super::SINK_SITE } { self.0.a & crate::bytecode::REGISTER_MASK } else { self.0.a };
        WideInstruction::new(self.op(), a, self.0.b, self.0.c, imm)
    }
}

/// `generated_row`'s `FLAG`: a row whose immediate is a boolean flag (`ImmediateRole::BooleanFlag`) reads it
/// from the record until its first run, which requickens the record to the instantiation of its flag
/// (`FLAG_CLEAR`, `FLAG_SET`), so the flag is a static operand the row folds (an increment and a
/// decrement are two handlers of one row). For any other row, `FLAG_SET` states a fact the view
/// proved of the record's immediate: its high half is zero (`row_near`).
pub(in crate::vm::dispatch_fast) const FLAG_UNREAD: u8 = 0;
pub(in crate::vm::dispatch_fast) const FLAG_CLEAR: u8 = 1;
pub(in crate::vm::dispatch_fast) const FLAG_SET: u8 = 2;
/// `generated_row`'s `FLAG` for a by-name load (`NameLoad`) whose first run resolved its name to a
/// native an internal name stands for (`NameRead::Native`).
pub(in crate::vm::dispatch_fast) const FLAG_NAME_NATIVE: u8 = 3;
/// `FLAG` of a by-name load resolved to the global `undefined` (`NameBinding::Undefined`).
pub(in crate::vm::dispatch_fast) const FLAG_NAME_UNDEFINED: u8 = 4;
/// `FLAG` of a by-name load resolved to the global object's own data field its record's hint
/// names (`NameRead::Field`).
pub(in crate::vm::dispatch_fast) const FLAG_NAME_GLOBAL: u8 = 5;
/// `FLAG` of a by-name load resolved to an intrinsic name (`NameRead::Intrinsic`).
pub(in crate::vm::dispatch_fast) const FLAG_NAME_INTRINSIC: u8 = 6;
/// `FLAG` of a by-name load a probe settled onto the global object's own data field its record's
/// hint names, in a scope that may declare the name (`settle_name_record`): as `FLAG_NAME_GLOBAL`,
/// but a refresh checks the scopes' declarations first (`global_name_refresh_miss`, `SCOPED`).
pub(in crate::vm::dispatch_fast) const FLAG_NAME_GLOBAL_SCOPED: u8 = 7;
/// `FLAG` of a by-name load of a declared global lexical name: its record's hint locates the binding by
/// index (`NameRead::Lexical`), which a refresh of a global name requickens to (`global_name_refresh_miss`).
/// Subsumes the hand `lane_load_name_lexical`, whose fast read is this row's `global_read`.
pub(in crate::vm::dispatch_fast) const FLAG_NAME_LEXICAL: u8 = 16;
/// `FLAG` of a `StringCaseIndex` record its tier filtered (`string_case_filter::specialize`): the row
/// answers from the filter the record's operand names. Not a flag role, so its immediate is whole.
pub(in crate::vm::dispatch_fast) const FLAG_CASE_FILTER: u8 = 8;
/// `FLAG` of a local-access record of a plain slot (`plain_frame_local`) in an environment window's
/// function (`specialize_environment_window`): its immediate names the window's register base above
/// its slot (`staged::window_base`). In a window activation's run the row reads the window's locals;
/// in a frame's run, the frame's own locals, as an unflagged plain record does.
pub(in crate::vm::dispatch_fast) const FLAG_WINDOW: u8 = 9;
/// `FLAG` of a by-name load or store, or a closure, in a window activation whose names resolve through its
/// scope (`ScopeReads`): the row reads or writes the scope's binding first, and any miss takes the general step.
pub(in crate::vm::dispatch_fast) const FLAG_SCOPE: u8 = 10;
/// `FLAG` of an environment-local store in such a window whose environment holds the local.
pub(in crate::vm::dispatch_fast) const FLAG_SCOPE_ENVIRONMENT: u8 = 11;
/// `FLAG` of a closure in a window activation whose names do not resolve through its scope.
pub(in crate::vm::dispatch_fast) const FLAG_WINDOW_CLOSURE: u8 = 12;
/// `FLAG` of a by-name load whose scope the compiler left open (an eval's code, or a function with an eval of
/// its own): the row's miss is the probe (`name_probe`), which asks the general path where the name lands and
/// then settles the record (`settle_name_record`).
pub(in crate::vm::dispatch_fast) const FLAG_PROBE: u8 = 13;
/// `FLAG` of a loop site's backward jump (`dispatch_fast::loop_jump_row`): the jump row takes the jump while the
/// site warms up; past that, or out of fuel, the site's body (`loop_jump_miss`) runs.
pub(in crate::vm::dispatch_fast) const FLAG_LOOP: u8 = 14;
/// `FLAG` of a `LoadCapture` whose walk settled on a root binding (`load_capture_walk_miss`): its row decodes
/// the read as a root read (`Operation::rooted`), which the row's reads answer as the walk does.
pub(in crate::vm::dispatch_fast) const FLAG_ROOT: u8 = 15;
/// `FLAG` of a local read of a keeping activation's `arguments` binding that a forwarding call passes on
/// (`CallForwardingArguments` after it): the row answers the unmaterialized marker (`kept_arguments_binding`).
pub(in crate::vm::dispatch_fast) const FLAG_KEPT_MARKER: u8 = 17;
/// `FLAG` of a local read of a keeping activation's `arguments` binding the activation makes into an object
/// at its last read (`KeptArguments::Passes`): the row answers that object (`kept_arguments_binding`).
pub(in crate::vm::dispatch_fast) const FLAG_KEPT_OBJECT: u8 = 18;

/// Whether opcode `op`'s lane handler is its staged row (`generated_row`): a row that falls through,
/// of the flow family or one the catalog marks `ROW_HANDLER`. Derived from the catalog, so a row
/// that joins gets its handler with no line written here.
/// Whether opcode `op`'s row defines the carried accumulator: its record's `a` is its result
/// register, and its miss takes the general step, which carries the result on
/// (`Carried::stepped`). A capture read's miss walks the scopes and requickens its record to
/// handlers that carry none, and a local row defines none.
pub(in crate::vm::dispatch_fast) const fn row_produces(op: Op) -> bool {
    matches!(op.field_layout(crate::bytecode::InstructionField::A), crate::bytecode::FieldLayout::ResultRegister)
        && !matches!(op.staged(), Some(crate::bytecode::Decoder::Local))
        && matches!(crate::staged::miss(op), crate::staged::Miss::General)
}

pub(in crate::vm::dispatch_fast) const fn admits_row(op: Op) -> bool {
    // A suspension point's lane handler is its suspension row (`generator_yields::suspend_handler`),
    // which parks the activation: the general step it would miss to cannot suspend in place.
    !op.traits().has(crate::bytecode::Traits::SUSPENDS)
    && (matches!(op.control_flow_layout(), crate::bytecode::ControlFlowLayout::Fallthrough) || returns_raw_row(op))
        && (matches!(op.staged(), Some(crate::bytecode::Decoder::Flow))
            || op.traits().has(crate::bytecode::Traits::ROW_HANDLER))
}

/// Whether opcode `op` is a raw return's terminal row (`staged::raw_representation` on a `Terminal`
/// flow): its `Return` completes the Wasm activation through `Frame::return_raw`, so the row runs in the
/// lane with no hand handler. A JavaScript `Return` is not one: its completion stays `return_miss`.
pub(in crate::vm::dispatch_fast) const fn returns_raw_row(op: Op) -> bool {
    matches!(op.control_flow_layout(), crate::bytecode::ControlFlowLayout::Terminal)
        && crate::staged::raw_representation(op)
}

/// The window scope `FLAG` reads through (`ScopeReads`), for a row of a window whose names resolve
/// through its scope; `None` for any other row. The closure's function and window register count are
/// the record's (`environment_window_fields`), which a name or store row does not read.
#[inline(always)]
fn scope_reads<H: Host, const FLAG: u8>(cx: *mut LaneContext<H>, ip: super::Ip, i: &LaneInstruction) -> Option<ScopeReads> {
    if !matches!(FLAG, FLAG_SCOPE | FLAG_SCOPE_ENVIRONMENT | FLAG_WINDOW_CLOSURE) {
        return None;
    }
    let (function, window) = super::environment_window_fields(i);
    let closure: unsafe fn(*mut (), Registers, super::Ip, u32, u16) -> Option<Value> = if FLAG == FLAG_WINDOW_CLOSURE {
        super::window_closure_into::<H>
    } else {
        super::scoped_names::scoped_closure_into::<H>
    };
    Some(ScopeReads {
        context: cx.cast(), ip, function: function as u32, window, site: i.c,
        name: super::scoped_names::scoped_name_into::<H>,
        store: super::scoped_names::scoped_store_into::<H>,
        local: super::scoped_names::scoped_local_store_into::<H>,
        closure,
    })
}

/// The frame `generated_row` steps opcode `OP`'s row over: the register file and the carried view, and
/// the running frame's local slots where the row is a local one (`Decoder::Local`).
/// The carried accumulator stands for the record's field B alone (`LaneFrame::accumulated`).
#[inline(always)]
fn row_frame<H: Host, const OP: u16, const ACC: u8, const FLAG: u8>(cx: *mut LaneContext<H>, r: Registers, ip: super::Ip, i: &LaneInstruction, view: Carried) -> LaneFrame<ACC> {
    let mut frame = row_frame_of::<H, OP, ACC, FLAG>(cx, r, ip, i, view);
    if ACC == ACC_B {
        // Field B's register as the row decodes it: an operand or a field base names its
        // register within its encoding.
        use crate::bytecode::FieldLayout as L;
        frame.accumulated = match LaneFields::<OP>::OPCODE.field_layout(crate::bytecode::InstructionField::B) {
            L::Operand | L::NumericIndexOperand => crate::bytecode::Operand(i.b).register_index(),
            L::FieldBase => crate::bytecode::FieldBase(i.b).register_index(),
            _ => Some(i.b),
        }.unwrap_or(NO_ACCUMULATOR);
    }
    frame
}

/// `row_frame` before its accumulated register.
#[inline(always)]
fn row_frame_of<H: Host, const OP: u16, const ACC: u8, const FLAG: u8>(cx: *mut LaneContext<H>, r: Registers, ip: super::Ip, i: &LaneInstruction, view: Carried) -> LaneFrame<ACC> {
    if const { returns_raw_row(LaneFields::<OP>::OPCODE) } {
        // A raw return completes the Wasm activation it returns to (`lane_complete_wasm_activation`).
        let complete: unsafe fn(*mut (), Value) -> Option<Resumed> = |context, value| super::lane_complete_wasm_activation::<H>(context.cast(), value);
        return LaneFrame::<ACC> { wasm_return: Some((cx.cast(), complete)), ..LaneFrame::<ACC>::new(r, view) };
    }
    let locals = const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::Local)) };
    if const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::Element)) } {
        // A JavaScript view carries its context (`script_handler!`), so no thread-local load.
        let script = view.memory.bytes.cast::<LaneContext<H>>();
        let heap: *const crate::heap::Heap = &unsafe { vm(script) }.heap;
        // A read's host path is its miss sibling (`row_index_read_miss`): a call inside the row
        // would cost its fast path a stack frame.
        let reads = const { matches!(LaneFields::<OP>::OPCODE.field_layout(crate::bytecode::InstructionField::A), crate::bytecode::FieldLayout::ResultRegister) };
        let elements = if reads { None } else { Some(ElementSteps::of::<H>(script, ip)) };
        return LaneFrame::<ACC> { heap: Some(heap), elements, ..LaneFrame::<ACC>::new(r, view) };
    }
    if const { crate::staged::name_load(LaneFields::<OP>::OPCODE).is_some() } {
        // A by-name load reads the binding its record's first run resolved (`FLAG`, `name_row`);
        // an unresolved record holds none, so its row misses to the resolve (`name_load_miss`).
        let binding = match FLAG {
            FLAG_NAME_NATIVE => Some(NameBinding::Read(NameRead::Native)),
            FLAG_NAME_UNDEFINED => Some(NameBinding::Undefined),
            FLAG_NAME_GLOBAL | FLAG_NAME_GLOBAL_SCOPED => Some(NameBinding::Read(NameRead::Field(i.field_hint()))),
            FLAG_NAME_LEXICAL => Some(NameBinding::Read(NameRead::Lexical(i.field_hint()))),
            FLAG_NAME_INTRINSIC => Some(NameBinding::Read(NameRead::Intrinsic)),
            _ => None,
        };
        let frame = LaneFrame::<ACC> { scope: scope_reads::<H, FLAG>(cx, ip, i), ..LaneFrame::<ACC>::hosted::<H>(r, view, cx) };
        return match binding {
            Some(binding) => frame.named(binding),
            None => frame,
        };
    }
    if const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::Names)) } {
        let heap: *const crate::heap::Heap = &unsafe { vm(cx) }.heap;
        // A count the view proved to read its function's kept arguments (`FLAG_SET`) takes a
        // keeping activation's count without finding the function.
        let count: unsafe fn(*mut (), u16) -> Option<Value> = if FLAG == FLAG_SET {
            |context, slot| unsafe { super::arguments_count::<H, true>(context.cast(), slot) }
        } else {
            |context, slot| unsafe { super::arguments_count::<H, false>(context.cast(), slot) }
        };
        let environment = EnvironmentReads { context: cx.cast(), read: environment_read::<H>, write: environment_write_storage::<H> };
        // A global-name store writes through the record's own hint (`global_name_store`), which a window's
        // store does not take: its scope answers the name (`ScopeReads::store`).
        let global_store = if const { OP == crate::staged::GLOBAL_WRITE_ROW } && FLAG != FLAG_SCOPE {
            Some(GlobalStore { cx: cx.cast(), hint: i.field_hint(), store: global_name_store::<H> })
        } else {
            None
        };
        let names: NamesStep = fast_names_step::<H>;
        // A clone names the window its record carries above its plan (`specialize_environment_window`).
        let clones = if const { OP == crate::staged::CLONE_ENV_ROW } {
            Some(EnvironmentClones { context: cx.cast(), window: (i.imm() >> super::ENVIRONMENT_WINDOW_SHIFT) as u16,
                run: super::clone_environment_of::<H> })
        } else {
            None
        };
        // The iterator cleanup stack of the running lane (`iterator_push_in_lane`, `iterator_pop_in_lane`).
        let cleanups = IteratorCleanups { context: cx.cast(), push: iterator_push_in_lane::<H>, pop: iterator_pop_in_lane::<H> };
        // An element read names the running function's register count, which its kept values follow.
        let kept = if const { OP == crate::staged::ARGUMENT_GET_ROW } { i.c } else { 0 };
        return LaneFrame::<ACC> { heap: Some(heap), arguments: Some((cx.cast(), count)), kept, environment: Some(environment), global_store, names: Some((cx.cast(), names)),
            clones, cleanups: Some(cleanups), scope: scope_reads::<H, FLAG>(cx, ip, i), ..LaneFrame::<ACC>::hosted::<H>(r, view, cx) };
    }
    if const { OP == crate::bytecode::TIER_PROLOGUE as u16 || OP == crate::bytecode::BODY_ENTRY as u16 } {
        // A prologue record enters the activation's function in the lane context `cx` (`tier_prologue_resumed`,
        // `body_entry_resumed`), erased of its host so the frame names no `H`.
        let entry = EntryHost {
            cx: cx.cast(),
            ip,
            tier: |context, r, view, function, parameters| tier_prologue_resumed::<H>(context.cast(), r, view, function, parameters),
            body: |context, ip, r, view, function, binds_this| body_entry_resumed::<H>(context.cast(), ip, r, view, function, binds_this),
        };
        return LaneFrame::<ACC> { entry: Some(entry), ..LaneFrame::<ACC>::new(r, view) };
    }
    if const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::Lane)) } {
        // A string-case index reads its operand from the register file (`LaneFrame::read` stands for field B),
        // and answers from the case table of the record's filter (`FLAG_CASE_FILTER`), else the VM's selector.
        let string_case = const { OP == crate::staged::STRING_CASE_INDEX_ROW };
        let string_cases = if string_case {
            let filter = if FLAG == FLAG_CASE_FILTER { unsafe { i.operand.string_case_filter } } else { std::ptr::null() };
            Some(StringCases { context: cx.cast(), filter, index: super::string_case_filter::case_index_in_lane::<H> })
        } else {
            None
        };
        let heap: Option<*const crate::heap::Heap> = if string_case {
            let heap: *const crate::heap::Heap = &unsafe { vm(cx) }.heap;
            Some(heap)
        } else {
            None
        };
        return LaneFrame::<ACC> { heap, string_cases, ..LaneFrame::<ACC>::new(r, view) };
    }
    if const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::Element)) } {
        // A JavaScript view carries its context (`script_handler!`), so no thread-local load.
        let context = view.memory.bytes;
        let heap: *const crate::heap::Heap = &unsafe { vm(context.cast::<LaneContext<H>>()) }.heap;
        // A row a proof names (`FLAG_SET`) reads and stores its array through the proof's span.
        let element_span = if const { FLAG == FLAG_SET } { Some((context, proof_span::<H> as ElementSpan)) } else { None };
        return LaneFrame::<ACC> { heap: Some(heap), element_span, ..LaneFrame::<ACC>::new(r, view) };
    }
    // A deletion's staged row makes the host's deleter (`LaneFrame::delete_property`), which a hosted frame carries.
    if const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::Property))
        && OP != super::FIELD_LEAF_READ && LaneFields::<OP>::OPCODE.field_store().is_none() } {
        return LaneFrame::<ACC>::hosted::<H>(r, view, cx);
    }
    if const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::Value)) } {
        // A value row may read `this`: the context word, else the frame's (`this_in_lane`), read lazily.
        let read: unsafe fn(*mut ()) -> Option<Value> = |context| {
            let cx = context.cast::<LaneContext<H>>();
            unsafe { super::this_in_lane(cx, (*cx).registers) }
        };
        // A capture row reads the running function's dense slots (its miss walks: `row_miss`) and
        // stores through the lane's own store.
        let (capture_slots, captures) = (Some(capture_reads(r.capture())), Some(CaptureStores::own(r)));
        // A `LoadCapture` past the running function's own scope reads through the lane's remembered
        // walks (`Reads::of_captures`) before its walk miss.
        let reads = if const { OP == crate::staged::LOAD_CAPTURE_ROW } { Reads::of_captures::<H>(cx) } else { Reads::NONE };
        // A `LoadCallee` row reads the running function (`running_callee`).
        let callee: Option<(*mut (), unsafe fn(*mut ()) -> Option<Value>)> = if const { OP == crate::staged::LOAD_CALLEE_ROW } {
            let read: unsafe fn(*mut ()) -> Option<Value> = super::running_callee::<H>;
            Some((cx.cast(), read))
        } else {
            None
        };
        // A `typeof` row answers a heap value's kind and a name's string in the lane context (`LaneHost`).
        return LaneFrame::<ACC> { this: r.this(), frame_this: Some((cx.cast(), read)), capture_slots, captures, callee, reads,
            ..LaneFrame::<ACC>::hosted::<H>(r, view, cx) };
    }
    if const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::Generic)) } {
        // A generic row's word-decided answers, and the pure binary of its call-outs (`binary_values`).
        let binary: unsafe fn(*mut (), u32, Value, Value) -> Option<Value> =
            |context, operator, left, right| unsafe { super::binary_values::<H>(context.cast(), operator, left, right) };
        // Its operands may name a constant, a plain local or a capture, and a comparison pair its words
        // leave is the VM's steady decision (`staged::step` over the comparison).
        // Its `ToPropertyKey` answers a primitive's key through the names step (`fast_names_step`).
        let names: NamesStep = fast_names_step::<H>;
        return LaneFrame::<ACC> { binary: Some((cx.cast(), binary)), decisions: Some(Decisions::of(cx)), reads: Reads::of_jump::<H>(cx, None),
            names: Some((cx.cast(), names)), ..LaneFrame::<ACC>::new(r, view) };
    }
    if const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::Literal)) } {
        // An allocation or an element definition makes its value in the lane context, as a staged
        // allocation does (`LaneHost::allocate`, `LaneHost::define`).
        // A window's closure is made over the window's scope (`allocate_value`).
        return LaneFrame::<ACC> { scope: scope_reads::<H, FLAG>(cx, ip, i), ..LaneFrame::<ACC>::hosted::<H>(r, view, cx) };
    }
    if const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::WasmInt)) } {
        // An integer row reads its operands from the register file: the carried accumulator stands for
        // one field only, and a row reads two registers (`LaneFrame::read`).
        let heap: *const crate::heap::Heap = &unsafe { vm(cx) }.heap;
        return LaneFrame::<ACC> { heap: Some(heap), ..LaneFrame::<ACC>::new(r, view) };
    }
    if const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::WasmMemory)) } {
        // A direct access addresses the memory its view resolved, as the hand handler did.
        // SAFETY: the view resolved this memory in the current lane run (`MemoryView::extent`).
        let memory = unsafe { view.memory.extent() };
        // A generic access names its memory by register, which `wasm_memory_of` resolves.
        let memories = WasmMemories { context: cx.cast(), resolve: wasm_memory_of::<H> };
        return LaneFrame::<ACC> { memory: Some(memory), memories: Some(memories), ..LaneFrame::<ACC>::new(r, view) };
    }
    if const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::WasmState)) } {
        // An instance-state row reads and writes the VM's globals through the lane context.
        let globals = WasmGlobals { context: cx.cast(), load: super::wasm_instance::wasm_global_load::<H>,
            store: super::wasm_instance::wasm_global_store::<H>, instance_slot: super::wasm_instance::wasm_instance_slot::<H> };
        return LaneFrame::<ACC> { globals: Some(globals), ..LaneFrame::<ACC>::new(r, view) };
    }
    if const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::WasmVector)) } {
        // A SIMD row reads and defines vector cells through the lane context's heap.
        let vectors = WasmVectors { context: cx.cast(), read: super::wasm_instance::wasm_vector_read::<H>,
            alloc: super::wasm_instance::wasm_vector_alloc::<H> };
        return LaneFrame::<ACC> { vectors: Some(vectors), ..LaneFrame::<ACC>::new(r, view) };
    }
    // A `GetField` of a local slot reads the plain local its record names (`plain_local_operand`), which
    // the row's base is; any other local leaves to the record's miss (`local_field_miss`).
    if const { FLAG == super::LOCAL_FIELD_SITE } {
        let local = crate::bytecode::FieldBase(i.b).local_index().and_then(|slot| super::plain_local_operand(cx, slot));
        return LaneFrame::<ACC> { local_object: local.map(Value::to_bits), ..LaneFrame::<ACC>::new(r, view) };
    }
    if !locals {
        return LaneFrame::<ACC>::new(r, view);
    }
    // A keeping activation's `arguments` binding is answered by the Vm (`kept_arguments_binding`) whatever the
    // frame's locals hold, so the row needs no locals or window of its own: where the Vm declines it, the row
    // leaves to its miss, as the slot's read would.
    if const { FLAG == FLAG_KEPT_MARKER || FLAG == FLAG_KEPT_OBJECT } {
        let binding: unsafe fn(*mut (), Registers, u16) -> Option<Value> = if const { FLAG == FLAG_KEPT_OBJECT } {
            super::kept_arguments_binding::<H, true>
        } else {
            super::kept_arguments_binding::<H, false>
        };
        return LaneFrame::<ACC> { kept_binding: Some((cx.cast(), binding)), ..LaneFrame::<ACC>::new(r, view) };
    }
    // A local access holds the frame's locals only in an activation without its own environment, and
    // not a root body's frame. A window record (`FLAG_WINDOW`) in an activation that holds one reads the
    // window's locals instead. Any other leaves to its miss (`row_miss`).
    let environment_free = unsafe { (*cx).registers }.env().is_deleted();
    if FLAG == FLAG_WINDOW && !environment_free {
        return LaneFrame::<ACC> { window: Some(r.0.wrapping_add(usize::from(crate::staged::window_base(i.imm())))),
            ..LaneFrame::<ACC>::new(r, view) };
    }
    let vm = unsafe { vm(cx) };
    let frame = &mut vm.frames[unsafe { (*cx).frame() }];
    if !environment_free || frame.function == super::ROOT_FUNCTION_ID {
        return LaneFrame::<ACC>::new(r, view);
    }
    let (slots, len) = (frame.locals.as_mut_ptr(), frame.locals.len());
    LaneFrame::<ACC>::with_locals(r, view, Value::DELETED, slots, len)
}

/// The hand handler of checked local access `op` (`row_miss`), which the lane takes for a record
/// whose slot its frame's locals do not hold (an environment's, a mapped `arguments`' or a self
/// binding's), as the row would only leave there.
pub(in crate::vm::dispatch_fast) fn checked_local_hand<H: Host>(op: Op) -> Handler {
    row_miss::<H, FLAG_UNREAD>(op)
}

/// Where opcode `op`'s row leaves when its words do not decide it: the VM's general step, or for
/// a capture read the lane's walk, which requickens the record to the root binding it finds
/// (`load_capture_walk_miss`), for a store the first-run resolution of a global name
/// (`store_name_first_miss`).
pub(in crate::vm::dispatch_fast) const fn row_miss<H: Host, const FLAG: u8>(op: Op) -> Handler {
    match crate::staged::miss(op) {
        crate::staged::Miss::WalkScopes => super::load_capture_walk_miss::<H>,
        crate::staged::Miss::StoreNameFirst => match FLAG {
            // A window's store the scope does not take leaves to the general step.
            FLAG_SCOPE => super::exit_lane::<H> as Handler,
            _ => super::store_name_first_miss::<H> as Handler,
        },
        crate::staged::Miss::NameLoad(load) => name_load_miss::<H, FLAG>(load),
        crate::staged::Miss::General | crate::staged::Miss::CompareChain => general_step_in_lane::<H>,
    }
}

/// The row handler of by-name load `load` for a record in binding state `FLAG` (`generated_row`'s
/// instantiation over the load's opcode, with an accumulator-free record, since a load answers
/// no accumulator).
pub(in crate::vm::dispatch_fast) const fn name_row<H: Host, const FLAG: u8>(load: crate::staged::NameLoad) -> Handler {
    match load {
        crate::staged::NameLoad::Read => generated_row!(H, crate::staged::LOAD_NAME_ROW, ACC_NONE, FLAG) as Handler,
        crate::staged::NameLoad::Call => generated_row!(H, crate::staged::LOAD_NAME_CALL_ROW, ACC_NONE, FLAG) as Handler,
        crate::staged::NameLoad::Typeof => generated_row!(H, crate::staged::LOAD_NAME_TYPEOF_ROW, ACC_NONE, FLAG) as Handler,
    }
}

/// Where a by-name load's row leaves when its binding does not answer it: the first run (and a
/// native's, which is resolved again) resolves the name (`load_name_resolve_miss`); a global's
/// read takes its refresh (`global_name_refresh_miss`); an undefined or intrinsic read takes
/// the general path, which the lane exits to.
const fn name_load_miss<H: Host, const FLAG: u8>(load: crate::staged::NameLoad) -> Handler {
    match load {
        crate::staged::NameLoad::Read => name_miss_kind::<H, FLAG, { super::LOAD_NAME }>(),
        crate::staged::NameLoad::Call => name_miss_kind::<H, FLAG, { super::LOAD_NAME_CALL }>(),
        crate::staged::NameLoad::Typeof => name_miss_kind::<H, FLAG, { super::LOAD_NAME_TYPEOF }>(),
    }
}

/// `name_load_miss` for the by-name load of kind `KIND`.
const fn name_miss_kind<H: Host, const FLAG: u8, const KIND: u8>() -> Handler {
    match FLAG {
        FLAG_NAME_GLOBAL | FLAG_NAME_LEXICAL => super::global_name_refresh_miss::<H, KIND, false> as Handler,
        FLAG_NAME_GLOBAL_SCOPED => super::global_name_refresh_miss::<H, KIND, true> as Handler,
        FLAG_NAME_UNDEFINED | FLAG_NAME_INTRINSIC | FLAG_SCOPE => super::exit_lane::<H> as Handler,
        FLAG_PROBE => super::probe_handler::<H, KIND>(),
        _ => super::load_name_resolve_miss::<H, KIND> as Handler,
    }
}

/// The row handler of a window's by-name store (`GLOBAL_WRITE_ROW` at `FLAG_SCOPE`): the scope's store first.
pub(in crate::vm::dispatch_fast) const fn scoped_store_row<H: Host>() -> Handler {
    generated_row!(H, crate::staged::GLOBAL_WRITE_ROW, ACC_NONE, FLAG_SCOPE) as Handler
}

/// The row handler of a window's environment-local store (`STORE_ENV_LOCAL_ROW`) at `FLAG`.
pub(in crate::vm::dispatch_fast) const fn scoped_local_row<H: Host, const FLAG: u8>() -> Handler {
    generated_row!(H, crate::staged::STORE_ENV_LOCAL_ROW, ACC_NONE, FLAG) as Handler
}

/// The row handler of a closure (`MAKE_CLOSURE_ROW`) in a window, at `FLAG`.
pub(in crate::vm::dispatch_fast) const fn closure_row<H: Host, const FLAG: u8>() -> Handler {
    generated_row!(H, crate::staged::MAKE_CLOSURE_ROW, ACC_NONE, FLAG) as Handler
}

/// Opcode `OP`'s row handler for a record of which the view proved its row's fact (`FLAG_SET`),
/// where `handler` is the plain row (`FLAG_UNREAD`, no accumulator field): a capture access at
/// depth 0 then reads its slot without testing the depth.
pub(in crate::vm::dispatch_fast) fn row_near<H: Host, const OP: u16>(handler: *const ()) -> Option<Handler> {
    (handler == generated_row!(H, OP, ACC_NONE, FLAG_UNREAD) as Handler as *const ())
        .then_some(generated_row!(H, OP, ACC_NONE, FLAG_SET) as Handler)
}

/// Whether `handler` is opcode `OP`'s generated row handler (`generated_row`), at any accumulator field:
/// the passes that fuse a record by its handler find the row whichever field the view carried.
pub(in crate::vm::dispatch_fast) fn is_row<H: Host, const OP: u16>(handler: *const ()) -> bool {
    is_row_flagged::<H, OP, FLAG_UNREAD>(handler) || is_row_flagged::<H, OP, FLAG_CLEAR>(handler)
        || is_row_flagged::<H, OP, FLAG_SET>(handler)
}

/// `is_row` at one `FLAG`.
fn is_row_flagged<H: Host, const OP: u16, const FLAG: u8>(handler: *const ()) -> bool {
    let rows: [Handler; ACC_POSITIONS] = [generated_row!(H, OP, ACC_NONE, FLAG), generated_row!(H, OP, ACC_A, FLAG),
        generated_row!(H, OP, ACC_B, FLAG), generated_row!(H, OP, ACC_C, FLAG), generated_row!(H, OP, ACC_PAIR, FLAG)];
    rows.iter().any(|row| *row as *const () == handler)
}

/// The row of a local read of a keeping activation's `arguments` binding (`handler`, a local read row at its unread
/// flag, any accumulator) at the kept flag: `FLAG_KEPT_OBJECT` where the binding makes an object (`object`), else
/// `FLAG_KEPT_MARKER`. `None` where `handler` is no local read row. Subsumes the forwarded and kept-object loads
/// (`lane_load_forwarded_arguments`, `lane_load_kept_arguments_object`) of the lane's hand handlers.
pub(in crate::vm::dispatch_fast) fn kept_row<H: Host>(handler: *const (), object: bool) -> Option<Handler> {
    kept_row_of::<H, { crate::staged::LOCAL_LOAD_ROW }>(handler, object)
        .or_else(|| kept_row_of::<H, { crate::staged::LOAD_LOCAL_PLAIN_ROW }>(handler, object))
}

/// `kept_row` for opcode `OP`'s rows.
fn kept_row_of<H: Host, const OP: u16>(handler: *const (), object: bool) -> Option<Handler> {
    if object { kept_row_at::<H, OP, FLAG_KEPT_OBJECT>(handler) } else { kept_row_at::<H, OP, FLAG_KEPT_MARKER>(handler) }
}

/// `kept_row_of` at one kept `FLAG`: opcode `OP`'s row at `FLAG` for the accumulator field its unread row carries.
fn kept_row_at<H: Host, const OP: u16, const FLAG: u8>(handler: *const ()) -> Option<Handler> {
    let unread: [Handler; ACC_POSITIONS] = [generated_row!(H, OP, ACC_NONE, FLAG_UNREAD), generated_row!(H, OP, ACC_A, FLAG_UNREAD),
        generated_row!(H, OP, ACC_B, FLAG_UNREAD), generated_row!(H, OP, ACC_C, FLAG_UNREAD), generated_row!(H, OP, ACC_PAIR, FLAG_UNREAD)];
    let kept: [Handler; ACC_POSITIONS] = [generated_row!(H, OP, ACC_NONE, FLAG), generated_row!(H, OP, ACC_A, FLAG),
        generated_row!(H, OP, ACC_B, FLAG), generated_row!(H, OP, ACC_C, FLAG), generated_row!(H, OP, ACC_PAIR, FLAG)];
    unread.iter().position(|row| *row as *const () == handler).map(|position| kept[position])
}

/// `staged::step` of opcode `$op`'s row, compiled as only the part the row reaches
/// (`staged::row_step`): each family is an `if const` branch, which rustc compiles only when taken.
macro_rules! row_step {
    ($op:expr, $stage:expr, $frame:expr, $operation:expr) => {{
        use crate::staged::RowStep as R;
        if const { matches!(crate::staged::row_step($op), R::Flow) } { crate::staged::step_flow($stage, $frame, $operation) }
        else if const { matches!(crate::staged::row_step($op), R::Local) } { crate::staged::step_local($stage, $frame, $operation) }
        else if const { matches!(crate::staged::row_step($op), R::Number) } { crate::staged::step_number($stage, $frame, $operation) }
        else if const { matches!(crate::staged::row_step($op), R::Value) } { crate::staged::step_value($stage, $frame, $operation) }
        else if const { matches!(crate::staged::row_step($op), R::Names) } { crate::staged::step_names($stage, $frame, $operation) }
        else if const { matches!(crate::staged::row_step($op), R::Property) } { crate::staged::step_property($stage, $frame, $operation) }
        else if const { matches!(crate::staged::row_step($op), R::Keyed) } { crate::staged::step_keyed($stage, $frame, $operation) }
        else if const { matches!(crate::staged::row_step($op), R::Generic) } { crate::staged::step_generic($stage, $frame, $operation) }
        else if const { matches!(crate::staged::row_step($op), R::Arith) } { crate::staged::step_arith($stage, $frame, $operation) }
        else if const { matches!(crate::staged::row_step($op), R::WasmInt) } { crate::staged::step_wasm_int($stage, $frame, $operation) }
        else if const { matches!(crate::staged::row_step($op), R::WasmFloat) } { crate::staged::step_wasm_float($stage, $frame, $operation) }
        else if const { matches!(crate::staged::row_step($op), R::WasmJump) } { crate::staged::step_wasm_jump($stage, $frame, $operation) }
        else if const { matches!(crate::staged::row_step($op), R::WasmState) } { crate::staged::step_wasm_state($stage, $frame, $operation) }
        else if const { matches!(crate::staged::row_step($op), R::WasmMemory) } { crate::staged::step_wasm_access($stage, $frame, $operation) }
        else if const { matches!(crate::staged::row_step($op), R::WasmVector) } { crate::staged::step_wasm_vector($stage, $frame, $operation) }
        else if const { matches!(crate::staged::row_step($op), R::Entry) } { crate::staged::step_entry($stage, $frame, $operation) }
        // An opcode with no family step runs the whole `step`, out of line: inlined, it put every arm of the
        // interpreter into each of its row instances (1065 instances, half of the crate's code before optimization).
        else { crate::staged::step_out_of_line($stage, $frame, $operation) }
    }};
}

/// Where opcode `op`'s jump row leaves when its words do not decide the edge: a comparison the
/// frame leaves takes the lane's comparison chain (`binary_false_chain_miss`); any other, a truth
/// test of a heap value among them, takes the VM's general step.
const fn jump_row_miss<H: Host, const BACKWARD: bool>(op: Op) -> Handler {
    match crate::staged::miss(op) {
        crate::staged::Miss::CompareChain => super::binary_false_chain_miss::<H, BACKWARD>,
        _ => general_step_in_lane::<H>,
    }
}

/// Whether opcode `op` runs its generated jump row (`row_jump`): a jump the flow decoder
/// stages (`Decoder::Flow`), a loop's exit (`LANE_LOOP_EXIT`), whose names row makes its step and
/// its edge, a jump whose row answers its edge from a comparison (`staged::compare_jump`), or a
/// Wasm branch table, whose edge is the entry its index picks (`Edge::Entry`).
pub(in crate::vm::dispatch_fast) const fn jumps_by_row(op: Op) -> bool {
    crate::staged::wasm_branch_table(op)
        || (op.control_flow_layout().jumps()
            && (matches!(op.staged(), Some(crate::bytecode::Decoder::Flow)) || op.traits().has(crate::bytecode::Traits::LANE_LOOP_EXIT)
                || crate::staged::compare_jump(op) || crate::staged::wasm_i32_jump(op)))
}

/// Opcode `OP`'s jump row in direction `backward`.
pub(in crate::vm::dispatch_fast) const fn jump_row<H: Host, const OP: u16>(backward: bool) -> Handler {
    if backward {
        row_jump::<H, OP, true, FLAG_UNREAD, { crate::staged::NO_LINK }, { crate::staged::NO_LINK }, { crate::staged::NO_LINK }>
    } else {
        row_jump::<H, OP, false, FLAG_UNREAD, { crate::staged::NO_LINK }, { crate::staged::NO_LINK }, { crate::staged::NO_LINK }>
    }
}

/// One prefix row of a backward jump's target run (`FUSED_TARGET`), run at `$at`: a value row, its register
/// write and its accumulator carried on as a chain's link does. `$at` advances to the record after it. A miss
/// takes the row's own miss at its record. `$cx`, `$r`, `$at` and `$carried` are the caller's names; `$prefix`
/// is the row's const param.
macro_rules! target_prefix_row {
    ($cx:ident, $r:ident, $at:ident, $carried:ident, $prefix:ident) => {{
        // SAFETY: `$at` is a validated record of the view.
        let prefix_i = unsafe { &*$at };
        // The prefix at flag `$flag`.
        macro_rules! prefix_at { ($flag:expr) => {{
            let fields = LaneFields::<$prefix, { $flag }>(prefix_i);
            let Some(operation) = row_decode!($prefix, { $flag }, fields) else { sibling!(general_step_in_lane::<H>, $r, $at, $carried) };
            let mut stage = Concrete::new(std::ptr::null());
            let mut frame = row_frame::<H, $prefix, ACC_NONE, { $flag }>($cx, $r, $at, prefix_i, $carried);
            match row_step!(LaneFields::<$prefix>::OPCODE, &mut stage, &mut frame, operation) {
                Step::Value(value) => {
                    if const { !crate::staged::row_writes_result(LaneFields::<$prefix>::OPCODE) } {
                        $r.set(prefix_i.a, Value::from_bits(value));
                    }
                    if const { row_produces(LaneFields::<$prefix>::OPCODE) } {
                        $carried = Carried { acc: Slot::of(Value::from_bits(value)), ..$carried };
                    }
                    $at = following($at);
                }
                Step::Control(_) | Step::Unresolved => sibling!(const { row_miss::<H, { $flag }>(LaneFields::<$prefix>::OPCODE) }, $r, $at, $carried),
            }
        }}; }
        if const { LaneFields::<$prefix>::FLAG_ROLE } {
            if prefix_i.imm() != 0 { prefix_at!(FLAG_SET) } else { prefix_at!(FLAG_CLEAR) }
        } else {
            prefix_at!(FLAG_UNREAD)
        }
    }};
}

/// A backward jump's target run (`FUSED_TARGET`), run in the jump's record `$ip` once the jump is taken: the
/// back edge's fuel check (a safepoint at the target, before its run, as `take_jump` makes it), then the
/// target's prefix row `$prefix` when it is not `NO_LINK` (`target_prefix_row`), then the copy row `$copy`
/// when it is not `NO_LINK` (a `Move` between the prefix and the test: its register write is the copy's own),
/// then the conditional test `$test` over its own fields, which continues at the test's taken or fall-through
/// edge. A miss takes the missing row's own miss at its record, as a fused record's miss does. Every path returns.
/// `$cx`, `$r`, `$ip` and `$carried` are the handler's own names; `$prefix`, `$copy` and `$test` are its const params.
macro_rules! target_run {
    ($cx:ident, $r:ident, $ip:ident, $carried:ident, $prefix:ident, $copy:ident, $test:ident) => {{
        // SAFETY: the view records the distance to a validated jump target.
        let target: crate::vm::dispatch_fast::Ip = unsafe { $ip.byte_offset((*$ip).jump()) };
        if script_fuel_out($cx) {
            return crate::vm::dispatch_fast::safepoint_at::<H>($ip, target, $carried.acc);
        }
        let mut at: crate::vm::dispatch_fast::Ip = target;
        let mut carried: Carried = $carried;
        if const { $prefix != crate::staged::NO_LINK } {
            target_prefix_row!($cx, $r, at, carried, $prefix);
        }
        if const { $copy != crate::staged::NO_LINK } {
            target_prefix_row!($cx, $r, at, carried, $copy);
        }
        // SAFETY: `at` is the target or the record the prefix rows fall through to, a validated record of the view.
        let test_i = unsafe { &*at };
        // The test at comparison `$comparison` (`Some`, a constant of the arm), as `compare_jump_row`'s
        // instance runs it: its row folds to that comparison's test, and only that one's paths, which
        // keep the frame out of memory. `None` is the record's own comparison.
        macro_rules! test_run { ($comparison:expr) => {{
            let fields = LaneFields::<$test, { FLAG_UNREAD }>(test_i);
            let decoded = match ($comparison, row_decode!($test, { FLAG_UNREAD }, fields)) {
                (Some(comparison), Some(Operation::CompareJump { left, right, negate, .. })) =>
                    Some(Operation::CompareJump { comparison, left, right, negate }),
                (_, decoded) => decoded,
            };
            let Some(operation) = decoded else { sibling!(general_step_in_lane::<H>, $r, at, carried) };
            let mut stage = Concrete::new(std::ptr::null());
            let mut frame = row_frame::<H, $test, ACC_NONE, FLAG_UNREAD>($cx, $r, at, test_i, carried);
            match row_step!(LaneFields::<$test>::OPCODE, &mut stage, &mut frame, operation) {
                Step::Control(LaneControl::Edge(Edge::Target)) => {
                    // SAFETY: as the read of `test_i` above.
                    let backward_test = unsafe { (*at).jump() } <= 0;
                    resume!($cx, crate::vm::dispatch_fast::take_jump::<H>($cx, $r, at, backward_test, carried, None))
                }
                Step::Control(LaneControl::Edge(Edge::Following)) | Step::Value(_) => next!($cx, $r, following(at), carried),
                Step::Control(_) | Step::Unresolved => sibling!(const { row_miss::<H, FLAG_UNREAD>(LaneFields::<$test>::OPCODE) }, $r, at, carried),
            }
        }}; }
        if const { $test == crate::bytecode::Op::JumpBinaryFalse as u16 } {
            // A loop's test is almost always a relation or a strict equality: each of those runs its own
            // arm, any other comparison the record's decode.
            use oxc_ast::ast::BinaryOperator as B;
            const LT: u32 = B::LessThan as u32;
            const LE: u32 = B::LessEqualThan as u32;
            const GT: u32 = B::GreaterThan as u32;
            const GE: u32 = B::GreaterEqualThan as u32;
            const SEQ: u32 = B::StrictEquality as u32;
            const SNE: u32 = B::StrictInequality as u32;
            match u32::from(test_i.a) {
                LT => test_run!(Some(Comparison::of_operator(LT))),
                LE => test_run!(Some(Comparison::of_operator(LE))),
                GT => test_run!(Some(Comparison::of_operator(GT))),
                GE => test_run!(Some(Comparison::of_operator(GE))),
                SEQ => test_run!(Some(Comparison::of_operator(SEQ))),
                SNE => test_run!(Some(Comparison::of_operator(SNE))),
                _ => {}
            }
        }
        test_run!(None::<Comparison>)
    }};
}

/// The `FUSED_TARGET` copy slot of no row: `target_run`'s `$copy` for a target run with no copy.
const NO_COPY: u16 = crate::staged::NO_LINK;

/// Opcode `$op`'s staged operation decoded from `$fields` by its row family's one decoder. Each
/// decoder is an `if const` branch: rustc compiles only the taken one (it compiles every arm of a
/// `match` on a constant).
macro_rules! row_decode {
    ($op:ident, $flag:expr, $fields:ident) => {
        if const { matches!(LaneFields::<$op, $flag>::OPCODE.staged(), Some(crate::bytecode::Decoder::Flow)) } {
            Operation::decode_flow($fields, crate::staged::DecodeContext::EMPTY)
        } else if const { matches!(LaneFields::<$op, $flag>::OPCODE.staged(), Some(crate::bytecode::Decoder::Local)) } {
            Operation::decode_local(crate::staged::Fields::wide($fields))
        } else if const { matches!(LaneFields::<$op, $flag>::OPCODE.staged(), Some(crate::bytecode::Decoder::Value)) } {
            Operation::decode_value(crate::staged::Fields::wide($fields))
        } else if const { matches!(LaneFields::<$op, $flag>::OPCODE.staged(), Some(crate::bytecode::Decoder::Generic)) } {
            Operation::decode_generic(crate::staged::Fields::wide($fields))
        } else if const { matches!(LaneFields::<$op, $flag>::OPCODE.staged(), Some(crate::bytecode::Decoder::WasmInt)) } {
            Operation::decode_wasm_int(crate::staged::Fields::wide($fields))
        } else if const { matches!(LaneFields::<$op, $flag>::OPCODE.staged(), Some(crate::bytecode::Decoder::Names)) } {
            Operation::decode_names(crate::staged::Fields::wide($fields))
        } else if const { matches!(LaneFields::<$op, $flag>::OPCODE.staged(), Some(crate::bytecode::Decoder::Element)) } {
            Operation::decode_element(crate::staged::Fields::wide($fields))
        } else if const { matches!(LaneFields::<$op, $flag>::OPCODE.staged(), Some(crate::bytecode::Decoder::WasmFloat)) } {
            Operation::decode_wasm_float(crate::staged::Fields::wide($fields))
        } else if const { matches!(LaneFields::<$op, $flag>::OPCODE.staged(), Some(crate::bytecode::Decoder::WasmState)) } {
            Operation::decode_wasm_state($fields, crate::staged::DecodeContext::EMPTY)
        } else if const { matches!(LaneFields::<$op, $flag>::OPCODE.staged(), Some(crate::bytecode::Decoder::WasmMemory)) } {
            Operation::decode_wasm_access($fields, crate::staged::DecodeContext::EMPTY)
        } else if const { matches!(LaneFields::<$op, $flag>::OPCODE.staged(), Some(crate::bytecode::Decoder::WasmVector)) } {
            Operation::decode_wasm_vector($fields, crate::staged::DecodeContext::EMPTY)
        } else if const { matches!(LaneFields::<$op, $flag>::OPCODE.staged(), Some(crate::bytecode::Decoder::Property)) } {
            // A named read, a named store or `delete` (`f5df21c6c1`): without it every GetField row
            // decoded to nothing and left for the general step.
            Operation::decode_property(crate::staged::Fields::wide($fields))
        } else if const { matches!(LaneFields::<$op, $flag>::OPCODE.staged(), Some(crate::bytecode::Decoder::Literal)) } {
            Operation::decode_literal(crate::staged::Fields::wide($fields), ())
        } else if const { matches!(LaneFields::<$op, $flag>::OPCODE.staged(), Some(crate::bytecode::Decoder::Lane)) } {
            Operation::decode_lane(crate::staged::Fields::wide($fields))
        } else if const { matches!(LaneFields::<$op, $flag>::OPCODE.staged(), Some(crate::bytecode::Decoder::Proved)) } {
            // A by-name load's proof of its global is the lane's own binding (`row_frame`), so the
            // proof is the unit.
            Operation::decode_load_name(crate::staged::Fields::wide($fields), ())
        } else {
            None
        }
    };
}

/// The first `FLAG` of a comparison jump whose operator its instance fixes (`compare_jump_row`):
/// `FLAG_COMPARE + operator`, past every other flag.
const FLAG_COMPARE: u8 = 200;
/// The operators a comparison jump's instance fixes: every binary operator code (`binary_operator`).
const COMPARE_OPERATORS: u8 = 22;

/// The operator a comparison jump's `flag` fixes, `None` for any other flag.
const fn compare_flag(flag: u8) -> Option<u32> {
    if flag >= FLAG_COMPARE && flag < FLAG_COMPARE + COMPARE_OPERATORS { Some((flag - FLAG_COMPARE) as u32) } else { None }
}

/// The instance of the comparison jump row whose comparison is `operator`'s, `None` past the codes.
fn compare_jump_row<H: Host, const BACKWARD: bool>(operator: u32) -> Option<Handler> {
    macro_rules! rows { ($($n:literal)+) => { [$(row_jump::<H, { crate::bytecode::Op::JumpBinaryFalse as u16 }, BACKWARD, { FLAG_COMPARE + $n }, { crate::staged::NO_LINK }, { crate::staged::NO_LINK }, { crate::staged::NO_LINK }> as Handler),+] } }
    let table: [Handler; COMPARE_OPERATORS as usize] = rows!(0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21);
    table.get(operator as usize).copied()
}

lane_handler! {
    /// The lane handler of every jump `jumps_by_row` admits, generated from its staged row as
    /// `generated_row` is: the record's decoder (flow, or generic for a comparison) reads it, the row
    /// steps on `Concrete` over the lane's frame, and its edge picks the continuation. A jump to
    /// its target runs the lane's jump (`take_jump`), whose backward edge (`BACKWARD`, which the
    /// view fixes per record) is a safepoint. `TEST` (not `NO_LINK`) is a backward jump's `FUSED_TARGET`
    /// target run: its target's `PREFIX` row (or none), then the `COPY` row (or none), then the test `TEST`,
    /// run in this record (`target_run`). Any other `TEST` takes the jump as it always does.
    pub(super) fn row_jump<OP: u16, BACKWARD: bool, FLAG: u8, PREFIX: u16, COPY: u16, TEST: u16>(cx, r, ip, i, view) {
        // A loop site's backward jump (`FLAG_LOOP`) warming up takes its jump here, with the fuel
        // spent here, as the site's body does. Fuel out, or a site past its warm-up, leaves to that
        // body (`loop_jump_miss`), which spends the fuel and decides the site itself.
        if const { FLAG == FLAG_LOOP } {
            let site = i.loop_site();
            if unsafe { (*cx).fuel } == 0 || !site.warm_backedge() {
                sibling!(super::loop_jump_miss::<H>, r, ip, view)
            }
            unsafe { (*cx).fuel -= 1 };
            let header = unsafe { ip.byte_offset(site.jump) };
            next!(cx, r, header, Carried::script_in(cx, view))
        }
        // A comparison jump's operator is a constant of its record: its first run moves the record to
        // the instance whose `FLAG` names the operator (`compare_jump_row`), where the comparison is a
        // constant and its row folds to that operator's test rather than matching every comparison.
        if const { OP == crate::bytecode::Op::JumpBinaryFalse as u16 && FLAG == FLAG_UNREAD
            && PREFIX == crate::staged::NO_LINK && COPY == crate::staged::NO_LINK && TEST == crate::staged::NO_LINK } {
            // The operator is the record's field A (`binary_operator_field`); the instance is taken only
            // where that operator names the comparison the record decodes to.
            let operator = u32::from(i.a);
            let decodes_so = matches!(Operation::<u64, ()>::decode_generic(crate::staged::Fields::wide(LaneFields::<OP>(i))),
                Some(Operation::CompareJump { comparison, .. }) if comparison == crate::staged::Comparison::of_operator(operator));
            if decodes_so && let Some(handler) = compare_jump_row::<H, BACKWARD>(operator) {
                requicken::<H>(ip, handler);
                sibling!(handler, r, ip, view)
            }
        }
        let fields = LaneFields::<OP>(i);
        // A loop's exit decodes by its names row (`ForInNext`), any other jump by the flow decoder.
        let decoded = if const { compare_flag(FLAG).is_some() } {
            match Operation::decode_generic(crate::staged::Fields::wide(fields)) {
                Some(Operation::CompareJump { left, right, negate, .. }) => Some(Operation::CompareJump {
                    comparison: crate::staged::Comparison::of_operator(const { match compare_flag(FLAG) { Some(operator) => operator, None => 0 } }),
                    left, right, negate,
                }),
                other => other,
            }
        } else if const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::Names)) } {
            Operation::decode_names(crate::staged::Fields::wide(fields))
        } else if const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::Generic)) } {
            Operation::decode_generic(crate::staged::Fields::wide(fields))
        } else if const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::WasmJump)) } {
            Operation::decode_wasm_jump(crate::staged::Fields::wide(fields))
        } else {
            Operation::decode_flow(fields, crate::staged::DecodeContext::EMPTY)
        };
        let Some(operation) = decoded else {
            sibling!(general_step_in_lane::<H>, r, ip, view)
        };
        let mut stage = Concrete::new(std::ptr::null());
        // A loop's exit writes its result register, so no producer's accumulator carries past it.
        let carried = if const { LaneFields::<OP>::RESULT_A } { Carried::past_result(view) } else { view };
        let mut frame = row_frame::<H, OP, ACC_NONE, FLAG_UNREAD>(cx, r, ip, i, view);
        match row_step!(LaneFields::<OP>::OPCODE, &mut stage, &mut frame, operation) {
            Step::Control(LaneControl::Edge(Edge::Target)) => {
                if const { TEST != crate::staged::NO_LINK } {
                    target_run!(cx, r, ip, carried, PREFIX, COPY, TEST)
                } else {
                    resume!(cx, super::take_jump::<H>(cx, r, ip, BACKWARD, carried, None))
                }
            }
            // A table's entry is the jump record `k` places after the table record (validation places
            // `entries` of them after it), and the lane jumps from that record.
            Step::Control(LaneControl::Edge(Edge::Entry(k))) => resume!(cx, super::take_jump::<H>(cx, r, unsafe { ip.add(1 + k as usize) }, BACKWARD, carried, None)),
            Step::Control(LaneControl::Edge(Edge::Leave)) => unreachable!("a jump leaves no lane run"),
            Step::Control(LaneControl::Resumed(_) | LaneControl::Suspend(_) | LaneControl::Edge(Edge::Transfer)) => unreachable!("a jump transfers no control"),
            Step::Control(LaneControl::Edge(Edge::Following)) | Step::Value(_) => next!(cx, r, following(ip), carried),
            Step::Unresolved => sibling!(const { jump_row_miss::<H, BACKWARD>(LaneFields::<OP>::OPCODE) }, r, ip, view),
        }
    }
}

lane_handler! {
    /// Where a generated row's planned named read misses (`generated_row`'s `property_miss!`): the own
    /// read past its record's hint, which reads through the site's cache and rebinds the hint; a held
    /// read, the site's holder entry. Out of the row's body, so each row instance holds one jump here
    /// rather than this chain at each of its misses.
    #[inline(never)]
    pub(in crate::vm::dispatch_fast) fn property_miss_row<OP: u16, ACC: u8, FLAG: u8>(cx, r, ip, i, view) {
        let script: *mut LaneContext<H> = view.memory.bytes.cast();
        // A local's read past its record's hint: the cached read of its site's tier (`local_field_miss`).
        if const { FLAG == crate::vm::dispatch_fast::LOCAL_FIELD_SITE } {
            sibling!(crate::vm::dispatch_fast::local_field_miss::<H>, r, ip, Carried::script_in(script, view))
        }
        if const { crate::vm::dispatch_fast::general_site(FLAG) } {
            // The general read the host path did not answer: its activation path, as `lane_get_field` made it.
            sibling!(crate::vm::dispatch_fast::row_get_field_general::<H>, r, ip, Carried::script_in(script, view))
        }
        if const { crate::vm::dispatch_fast::sink_site(FLAG) } {
            // The general read and store, which repeats the side-effect-free reads.
            sibling!(crate::vm::dispatch_fast::row_get_field_sink::<H>, r, ip, Carried::script_in(script, view))
        }
        if const { crate::vm::dispatch_fast::entry_site(FLAG) } {
            // The polymorphic read past its inline entries probes the hash (`row_polymorphic_miss`); the
            // hashed read, the holder's and the inherited walk's take the general read, which moves the record on.
            if const { FLAG == crate::vm::dispatch_fast::POLYMORPHIC_SITE } {
                sibling!(crate::vm::dispatch_fast::row_polymorphic_miss::<H>, r, ip, Carried::script_in(script, view))
            }
            if const { FLAG == crate::vm::dispatch_fast::HASHED_SITE } {
                sibling!(crate::vm::dispatch_fast::row_get_field_general::<H>, r, ip, Carried::script_in(script, view))
            }
            sibling!(crate::vm::dispatch_fast::row_get_field_miss::<H>, r, ip, Carried::script_in(script, view))
        }
        if const { crate::vm::dispatch_fast::returning_field_site(FLAG) } {
            // A returning read's stale hint: the returning miss answers and moves the record on.
            by_leaf!(crate::vm::dispatch_fast::returning_leaf(FLAG), |L| sibling!(crate::vm::dispatch_fast::row_returning_field_miss::<H, L>, r, ip, Carried::script_in(script, view)))
        }
        if const { crate::vm::dispatch_fast::private_field_site(FLAG) } {
            // A method, accessor or missing private field: the general private read.
            sibling!(get_private_field_miss::<H>, r, ip, Carried::script_in(script, view))
        }
        if const { crate::vm::dispatch_fast::cached_field_site(FLAG) } {
            // A read the cache does not answer: the general read, which runs the getter.
            sibling!(crate::vm::dispatch_fast::row_get_field_general::<H>, r, ip, Carried::script_in(script, view))
        }
        if const { crate::vm::dispatch_fast::held_site(FLAG) && !crate::vm::dispatch_fast::held_other_site(FLAG) } {
            // A direct object's miss; the read of any other receiver's object part.
            if (const { crate::vm::dispatch_fast::own_site_this(FLAG) } || (const { crate::vm::dispatch_fast::runtime_base(FLAG) } && i.b == crate::bytecode::FieldBase::THIS.0)) {
                sibling!(generated_row!(H, OP, ACC, crate::vm::dispatch_fast::HELD_OTHER_THIS), r, ip, Carried::script_in(script, view))
            }
            sibling!(generated_row!(H, OP, ACC, crate::vm::dispatch_fast::HELD_OTHER_REGISTER), r, ip, Carried::script_in(script, view))
        }
        if const { FLAG == crate::vm::dispatch_fast::STRING_SITE } {
            sibling!(row_get_field_string_refresh::<H>, r, ip, Carried::script_in(script, view))
        }
        if const { FLAG == crate::vm::dispatch_fast::NUMBER_SITE } {
            sibling!(crate::vm::dispatch_fast::row_get_field_number_refresh::<H>, r, ip, Carried::script_in(script, view))
        }
        if const { FLAG == crate::vm::dispatch_fast::NESTED_SITE } {
            // A nested read whose hops the record does not hold (or whose steps miss): the hand
            // read that refreshes the hint from the steps' entries.
            sibling!(get_field_nested_miss::<H>, r, ip, Carried::script_in(script, view))
        }
        if const { crate::vm::dispatch_fast::own_constant_site(FLAG) } {
            // Another object, or the object was written: the site reads as its tier decides
            // and never holds an own constant again.
            sibling!(crate::vm::dispatch_fast::refuse_own_constant_row::<H>, r, ip, Carried::script_in(script, view))
        }
        if const { crate::vm::dispatch_fast::length_site(FLAG) } {
            // Another receiver, or a form the row does not read: the general length reads.
            sibling!(crate::vm::dispatch_fast::row_length_other::<H>, r, ip, Carried::script_in(script, view))
        }
        if const { FLAG == crate::vm::dispatch_fast::TYPED_LENGTH_SITE } {
            // A receiver the host read does not answer: an array's or a string's length is the array row's.
            sibling!(generated_row!(H, OP, ACC, crate::vm::dispatch_fast::ARRAY_LENGTH_SITE), r, ip, Carried::script_in(script, view))
        }
        if const { FLAG == crate::vm::dispatch_fast::GEOMETRY_SITE } {
            // A name the view's geometry does not answer (a shadowed or ordinary one): the general read.
            sibling!(crate::vm::dispatch_fast::row_get_field_general::<H>, r, ip, Carried::script_in(script, view))
        }
        if const { crate::vm::dispatch_fast::cell_own_site(FLAG) } {
            // The record moves on as the site's tier decides, which rebinds the hint.
            sibling!(row_get_field_miss::<H>, r, ip, Carried::script_in(script, view))
        }
        if const { crate::vm::dispatch_fast::getter_site(FLAG) } {
            // The getter's refresh makes the hint, or reads through the getter's activation.
            sibling!(crate::vm::dispatch_fast::getters::row_get_field_getter_refresh::<H>, r, ip, Carried::script_in(script, view))
        }
        if const { crate::vm::dispatch_fast::inherited_site(FLAG) && !crate::vm::dispatch_fast::inherited_other_site(FLAG) } {
            // A direct object's miss; the read of any other receiver's object part.
            if (const { crate::vm::dispatch_fast::own_site_this(FLAG) } || (const { crate::vm::dispatch_fast::runtime_base(FLAG) } && i.b == crate::bytecode::FieldBase::THIS.0)) {
                sibling!(generated_row!(H, OP, ACC, crate::vm::dispatch_fast::INHERITED_OTHER_THIS), r, ip, Carried::script_in(script, view))
            }
            sibling!(generated_row!(H, OP, ACC, crate::vm::dispatch_fast::INHERITED_OTHER_REGISTER), r, ip, Carried::script_in(script, view))
        }
        if const { crate::vm::dispatch_fast::inherited_other_site(FLAG) } {
            // The inherited chain walk (`INHERITED_WALK_SITE`) answers what the record's hint did not.
            sibling!(generated_row!(H, OP, ACC, crate::vm::dispatch_fast::INHERITED_WALK_SITE), r, ip, Carried::script_in(script, view))
        }
        if const { FLAG == crate::vm::dispatch_fast::HELD_CELL_REGISTER } {
            sibling!(row_get_field_holder_cell_refresh::<H>, r, ip, Carried::script_in(script, view))
        }
        if const { crate::vm::dispatch_fast::held_other_site(FLAG) } {
            if (const { crate::vm::dispatch_fast::own_site_this(FLAG) } || (const { crate::vm::dispatch_fast::runtime_base(FLAG) } && i.b == crate::bytecode::FieldBase::THIS.0)) {
                sibling!(row_get_field_holder_refresh::<H, true>, r, ip, Carried::script_in(script, view))
            }
            sibling!(row_get_field_holder_refresh::<H, false>, r, ip, Carried::script_in(script, view))
        }
        if (const { crate::vm::dispatch_fast::own_site_this(FLAG) } || (const { crate::vm::dispatch_fast::runtime_base(FLAG) } && i.b == crate::bytecode::FieldBase::THIS.0)) {
            sibling!(row_get_field_own_refresh::<H, true, true>, r, ip, Carried::script_in(script, view))
        }
        sibling!(row_get_field_own_refresh::<H, false, true>, r, ip, Carried::script_in(script, view))
    }
}

/// Whether a view can select accumulator field `acc` for opcode `op`'s record
/// (`accumulator_field`): a register field it reads, or the first register of its register pair.
const fn accumulator_readable(op: Op, acc: u8) -> bool {
    let fields = crate::bytecode::InstructionField::ALL;
    let mut at = 0;
    while at < fields.len() {
        if fields[at].accumulator() == acc && op.field_layout(fields[at]).reads_register() {
            return true;
        }
        at += 1;
    }
    acc == ACC_PAIR && matches!(op.immediate_layout(), ImmediateLayout::RegisterPair)
}

/// The shard module of `generated_row!(OP, ACC, FLAG)`'s body (`generated_rows_*`).
const fn generated_row_shard(op: u16, acc: u8, flag: u8) -> usize {
    (op as usize * 7 + flag as usize * 3 + acc as usize) % 16
}

/// The lane handler of opcode `OP`'s staged row at accumulator field `ACC` and site fact `FLAG`
/// (`generated_row!`, `generated_row.rs`). Its instances are many and large, and rustc keeps all
/// instances of one function in its module's codegen unit, so the body lives in 16 shard modules.
/// Only constant evaluation may call this (`generated_row!`): a compiled call would name every shard's instance.
pub(in crate::vm::dispatch_fast) const fn generated_row_handler<H: Host, const OP: u16, const ACC: u8, const FLAG: u8>() -> Handler {
    match generated_row_shard(OP, ACC, FLAG) {
        0 => generated_rows_0::generated_row::<H, OP, ACC, FLAG>,
        1 => generated_rows_1::generated_row::<H, OP, ACC, FLAG>,
        2 => generated_rows_2::generated_row::<H, OP, ACC, FLAG>,
        3 => generated_rows_3::generated_row::<H, OP, ACC, FLAG>,
        4 => generated_rows_4::generated_row::<H, OP, ACC, FLAG>,
        5 => generated_rows_5::generated_row::<H, OP, ACC, FLAG>,
        6 => generated_rows_6::generated_row::<H, OP, ACC, FLAG>,
        7 => generated_rows_7::generated_row::<H, OP, ACC, FLAG>,
        8 => generated_rows_8::generated_row::<H, OP, ACC, FLAG>,
        9 => generated_rows_9::generated_row::<H, OP, ACC, FLAG>,
        10 => generated_rows_10::generated_row::<H, OP, ACC, FLAG>,
        11 => generated_rows_11::generated_row::<H, OP, ACC, FLAG>,
        12 => generated_rows_12::generated_row::<H, OP, ACC, FLAG>,
        13 => generated_rows_13::generated_row::<H, OP, ACC, FLAG>,
        14 => generated_rows_14::generated_row::<H, OP, ACC, FLAG>,
        _ => generated_rows_15::generated_row::<H, OP, ACC, FLAG>,
    }
}

/// Protocol miss families (`protocol_row`'s `MISS`, high byte): which body completes a
/// protocol record the lane frame does not answer. The low byte carries the family's own
/// static parameters (an argument count, a leaf form, a plain bit), which its `*_miss_of`
/// table turns into the instantiation the record had.
pub(in crate::vm::dispatch_fast) mod protocol_family {
    /// Keyed reads and writes, deletes and named reads past their rows (`element_miss_of`).
    pub(crate) const ELEMENT: u16 = 1 << 8;
    /// Private names and defined fields (`private_miss_of`).
    pub(crate) const PRIVATE: u16 = 2 << 8;
    /// Script calls (`script_call_miss_of`).
    pub(crate) const SCRIPT_CALL: u16 = 3 << 8;
    /// Iterator and generator steps (`iteration_miss_of`).
    pub(crate) const ITERATION: u16 = 4 << 8;
    /// Native, applied, spread and array calls (`native_call_miss_of`).
    pub(crate) const NATIVE_CALL: u16 = 5 << 8;
    /// Loop headers and numeric protocol records (`loop_miss_of`).
    pub(crate) const LOOP: u16 = 6 << 8;
    /// Constructions (`construct_miss_of`).
    pub(crate) const CONSTRUCT: u16 = 7 << 8;
    /// Forwarded and kept calls, tier prologues and body entries (`entry_miss_of`).
    pub(crate) const ENTRY: u16 = 8 << 8;
    /// Returns and returning records (`return_miss_of`).
    pub(crate) const RETURN: u16 = 9 << 8;
    /// Frame calls and memoized script calls (`frame_call_miss_of`).
    pub(crate) const FRAME_CALL: u16 = 10 << 8;
    /// Leaf, Math and builtin native calls (`leaf_call_miss_of`).
    pub(crate) const LEAF_CALL: u16 = 11 << 8;
    /// Vector regions (`vector_miss_of`).
    pub(crate) const VECTOR: u16 = 12 << 8;
    /// Generator resumes and suspensions (`generator_miss_of`).
    pub(crate) const GENERATOR: u16 = 13 << 8;
    /// The family of `miss`.
    pub(crate) const fn of(miss: u16) -> u16 { miss & 0xff00 }
    /// The family's static parameters in `miss`.
    pub(crate) const fn parameters(miss: u16) -> u8 { (miss & 0xff) as u8 }
}

/// The body that completes protocol record `miss` (`protocol_row`) when its row leaves the record
/// unresolved: the one place a former hand handler's body remains, named `x_miss`. Each family's
/// table maps its parameters to the instantiation the record had.
pub(in crate::vm::dispatch_fast) const fn protocol_miss<H: Host>(miss: u16) -> Handler {
    use protocol_family as F;
    match F::of(miss) {
        F::ELEMENT => super::element_miss_of::<H>(F::parameters(miss)),
        F::PRIVATE => super::private_miss_of::<H>(F::parameters(miss)),
        F::SCRIPT_CALL => super::script_call_miss_of::<H>(F::parameters(miss)),
        F::ITERATION => super::iteration_miss_of::<H>(F::parameters(miss)),
        F::NATIVE_CALL => super::native_call_miss_of::<H>(F::parameters(miss)),
        F::LOOP => super::loop_miss_of::<H>(F::parameters(miss)),
        F::CONSTRUCT => super::construct_miss_of::<H>(F::parameters(miss)),
        F::ENTRY => super::entry_miss_of::<H>(F::parameters(miss)),
        F::RETURN => super::return_miss_of::<H>(F::parameters(miss)),
        F::FRAME_CALL => super::frame_call_miss_of::<H>(F::parameters(miss)),
        F::LEAF_CALL => super::leaf_call_miss_of::<H>(F::parameters(miss)),
        F::VECTOR => super::vector_miss_of::<H>(F::parameters(miss)),
        F::GENERATOR => super::generator_miss_of::<H>(F::parameters(miss)),
        _ => general_step_in_lane::<H>,
    }
}

/// Whether opcode `op`'s protocol row (`protocol_row`) completes no record: its decoder is none of the
/// ones `row_decode!` decodes (a call's, a named read's, or no staged decoder at all), so the row
/// decodes to nothing and leaves at its miss on every record. Keep this list in step with `row_decode!`.
pub(in crate::vm::dispatch_fast) const fn protocol_row_declines(op: Op) -> bool {
    !matches!(op.staged(), Some(
        crate::bytecode::Decoder::Flow | crate::bytecode::Decoder::Local | crate::bytecode::Decoder::Value
            | crate::bytecode::Decoder::Generic | crate::bytecode::Decoder::WasmInt | crate::bytecode::Decoder::Names
            | crate::bytecode::Decoder::Element | crate::bytecode::Decoder::WasmFloat | crate::bytecode::Decoder::WasmState
            | crate::bytecode::Decoder::WasmMemory | crate::bytecode::Decoder::WasmVector | crate::bytecode::Decoder::Literal
            | crate::bytecode::Decoder::Lane | crate::bytecode::Decoder::Proved))
}

/// The handler a protocol record of opcode `OP` at miss `MISS` is installed with: `protocol_row`, or, where
/// the row declines every record (`protocol_row_declines`), the record's protocol miss body itself, so the
/// record skips the row's decode and its tail call.
pub(in crate::vm::dispatch_fast) const fn protocol_handler<H: Host, const OP: u16, const ACC: u8, const MISS: u16>() -> Handler {
    // A plain or leaf script call's row runs its call hooks (`call_form`), whatever its decoder.
    if protocol_row_declines(LaneFields::<OP>::OPCODE) && call_form(MISS).is_none() {
        protocol_miss::<H>(MISS)
    } else {
        protocol_row::<H, OP, ACC, MISS> as Handler
    }
}

lane_handler! {
    /// The lane handler of a protocol record (a call, a construction, a return, an iterator or
    /// generator step, a frame or tier protocol record), generated from its staged row as
    /// `generated_row` is but with no site plan: the record decodes by its opcode's decoder, the row
    /// steps on `Concrete` over the lane's frame (`CallFrame`), and the step picks the
    /// continuation. What the lane frame does not answer leaves unresolved to the record's
    /// protocol body (`protocol_miss(MISS)`), so the body is the row's miss arm, never the record's
    /// handler. An opcode with no staged decoder decodes to nothing and takes that arm directly
    /// until its row lands.
    pub(super) fn protocol_row<OP: u16, ACC: u8, MISS: u16>(cx, r, ip, i, view) {
        // A plain or leaf script call's row (`call_form`) decodes by the call decoder, which the
        // generic decode (`row_decode!`) leaves out, and steps over a frame whose call hooks make the
        // proved call (`call_frame`, `CallFrame::call_activate`).
        if const { call_form(MISS).is_some() } {
            // A call row's records are JavaScript, whose view carries its context (`script_handler!`).
            debug_assert_eq!(view.memory.bytes.cast::<LaneContext<H>>(), context_in_lane::<H>(), "a JavaScript view carries its context");
            let fields = LaneFields::<OP>(i);
            let decoded = match const { LaneFields::<OP>::OPCODE.staged() } {
                Some(crate::bytecode::Decoder::Call) =>
                    Operation::decode_call(crate::staged::Fields::wide(fields), crate::staged::DecodeContext::EMPTY),
                _ => None,
            };
            let Some(operation) = decoded else {
                sibling!(const { protocol_miss::<H>(MISS) }, r, ip, view)
            };
            let mut stage = Concrete::new(std::ptr::null());
            let mut frame = call_frame::<H, OP, ACC, MISS>(cx, r, ip, i, view);
            match crate::staged::step(&mut stage, &mut frame, operation) {
                Step::Value(value) => {
                    r.set(i.a, Value::from_bits(value));
                    next!(cx, r, following(ip), Carried::script_in(cx, view))
                }
                Step::Control(LaneControl::Edge(Edge::Following)) => next!(cx, r, following(ip), Carried::script_in(cx, view)),
                // The frame activated the callee (`CallFrame::call_activate`): its first record, as the
                // proved handlers continue (`script_call_miss`, `script_call_leaf_miss`).
                Step::Control(LaneControl::Resumed(resumed)) => resume!(cx, resumed),
                Step::Control(_) | Step::Unresolved => sibling!(const { protocol_miss::<H>(MISS) }, r, ip, view),
            }
        }
        let fields = LaneFields::<OP>(i);
        let Some(operation) = row_decode!(OP, FLAG_UNREAD, fields) else {
            sibling!(const { protocol_miss::<H>(MISS) }, r, ip, view)
        };
        // A returning record's value is `returning_row`'s to complete: this row never computes it.
        if const { LaneFields::<OP>::OPCODE.result_layout().allowed_flags() & crate::bytecode::RETURN_REGISTER != 0 }
            && i.a & crate::bytecode::RETURN_REGISTER != 0
        {
            sibling!(const { protocol_miss::<H>(MISS) }, r, ip, view)
        }
        let mut stage = Concrete::new(std::ptr::null());
        let mut frame = row_frame::<H, OP, ACC, FLAG_UNREAD>(cx, r, ip, i, view);
        match row_step!(LaneFields::<OP>::OPCODE, &mut stage, &mut frame, operation) {
            Step::Value(value) => {
                if const { !crate::staged::row_writes_result(LaneFields::<OP>::OPCODE) } {
                    r.set(i.a, Value::from_bits(value));
                }
                next!(cx, r, following(ip), Carried::script_in(cx, view))
            }
            Step::Control(LaneControl::Edge(Edge::Following)) => next!(cx, r, following(ip), Carried::script_in(cx, view)),
            // A row that suspended or completed its activation ends the run.
            Step::Control(LaneControl::Edge(Edge::Leave)) => return exit(ip),
            // A transfer resumes the register file the frame switched to.
            Step::Control(LaneControl::Resumed(resumed)) => resume!(cx, resumed),
            Step::Control(_) | Step::Unresolved => sibling!(const { protocol_miss::<H>(MISS) }, r, ip, view),
        }
    }
}

/// The static form of the call row whose miss is `miss` (`CallForm`): a plain script call of a
/// `script_call_miss` instantiation (`protocol_misses::script_call`'s argument codes), or a leaf call
/// of a `script_call_leaf_miss` one (`protocol_misses::leaf_call::leaf_of`); `None` for any other
/// miss, whose record the lane frame does not answer.
pub(in crate::vm::dispatch_fast) const fn call_form(miss: u16) -> Option<CallForm> {
    use super::protocol_misses::script_call as code;
    let parameters = protocol_family::parameters(miss);
    match protocol_family::of(miss) {
        protocol_family::SCRIPT_CALL if parameters & !code::PLAIN <= code::ARGC_ANY => {
            let argc = parameters & !code::PLAIN;
            let argc = if argc == code::ARGC_ANY { ANY_ARGC } else { argc as u16 };
            Some(CallForm { leaf: NOT_LEAF, argc, plain: parameters & code::PLAIN != 0 })
        }
        protocol_family::LEAF_CALL => match super::protocol_misses::leaf_call::leaf_of(parameters) {
            Some((leaf, argc)) if leaf >= PLAIN_LEAF && leaf <= THIS_LEAF && argc <= FIXED_ARGUMENT_COPIES =>
                Some(CallForm { leaf, argc, plain: false }),
            _ => None,
        },
        _ => None,
    }
}

/// The hand-off of the call row whose miss is `miss` (`call_form`): its leaf form's run at its
/// argument count (`leaf_call_activation`), else the plain activation at its argument count
/// (`plain_call_activation`), each an instantiation whose count and form are constants.
const fn call_activation<H: Host>(miss: u16) -> CallActivation {
    let Some(form) = call_form(miss) else { return plain_call_activation::<H, ANY_ARGC> };
    macro_rules! leaf {
        ($form:ident) => {
            match form.argc {
                0 => leaf_call_activation::<H, $form, 0> as CallActivation,
                1 => leaf_call_activation::<H, $form, 1>,
                2 => leaf_call_activation::<H, $form, 2>,
                3 => leaf_call_activation::<H, $form, 3>,
                _ => leaf_call_activation::<H, $form, 4>,
            }
        };
    }
    match form.leaf {
        PLAIN_LEAF => leaf!(PLAIN_LEAF),
        CAPTURE_LEAF => leaf!(CAPTURE_LEAF),
        CONTEXT_LEAF => leaf!(CONTEXT_LEAF),
        THIS_LEAF => leaf!(THIS_LEAF),
        _ => match form.argc {
            0 => plain_call_activation::<H, 0> as CallActivation,
            1 => plain_call_activation::<H, 1>,
            2 => plain_call_activation::<H, 2>,
            3 => plain_call_activation::<H, 3>,
            4 => plain_call_activation::<H, 4>,
            _ => plain_call_activation::<H, ANY_ARGC>,
        },
    }
}

/// The plain activation of `target` past the lane frame's admission of the call record `record` at
/// `ip` (`CallActivation`): the proved arm of `lane_plain_activation!` from its window on, its
/// window filled from the record's (`fill_window_in_lane`) and its header pushed straight below it at
/// the admitted `depth` (`lane_enter_plain_window`). `ARGC` is the record's argument count
/// (`call_argument_count`).
///
/// # Safety
/// `context` is the running lane's context and `record` the call record at `ip` of the caller's
/// registers `r`; `target` is the entry the frame admitted for it (one admission compare, its
/// callee, its parameters taking the arguments, a plain activation's room at `depth`), of an
/// unpublished entry cell (its proof), and `this` the receiver it binds.
#[inline(always)]
unsafe fn plain_call_activation<H: Host, const ARGC: u16>(context: *mut (), r: Registers, ip: Ip, record: *const LaneInstruction,
    target: *const ScriptCallTarget, depth: usize, this: Value) -> (Registers, Ip) {
    let cx = context.cast::<LaneContext<H>>();
    // SAFETY: the function's contract.
    let (vm, i, target) = unsafe { (vm(cx), &*record, &*target) };
    debug_assert!(target.entry.activates() && !target.settles());
    debug_assert!(!unsafe { (*target.view).entry_published() }, "publishing unproves the view's entries");
    let argc = call_argument_count::<ARGC>(i);
    // SAFETY: the record's window of `r` holds its values.
    let passed = unsafe { r.0.add(usize::from(ImmediateLayout::call_window_base(i.imm()))).cast_const() };
    let callee_registers = fill_window_in_lane(vm, target, passed, argc);
    lane_enter_plain_window(cx, vm, r, ip, target, callee_registers, depth, argc, this)
}

/// The leaf call of `target` of form `LEAF` past the lane frame's admission of the call record
/// `record` at `ip` (`CallActivation`): `script_call_leaf_miss`'s proved arm from its run on
/// (`lane_run_leaf`). A leaf pushes no plain activation, so the depth is not read.
///
/// # Safety
/// As `plain_call_activation`, `target` an entry of leaf form `LEAF` whose parameters take `ARGC`
/// values.
#[inline(always)]
unsafe fn leaf_call_activation<H: Host, const LEAF: u8, const ARGC: u16>(context: *mut (), r: Registers, ip: Ip,
    record: *const LaneInstruction, target: *const ScriptCallTarget, _depth: usize, this: Value) -> (Registers, Ip) {
    // SAFETY: the function's contract.
    unsafe { super::leaf_calls::lane_run_leaf::<H, LEAF, ARGC>(context.cast(), r, ip, &*record, &*target, this) }
}

/// The frame a call row of `MISS` steps over (`protocol_row`): the register file and the carried view,
/// and, for a plain or leaf script call (`call_form`), the call record its call hooks read
/// (`LaneCall`), whose form and hand-off are constants of the instantiation.
#[inline(always)]
fn call_frame<H: Host, const OP: u16, const ACC: u8, const MISS: u16>(cx: *mut LaneContext<H>, r: Registers, ip: Ip,
    i: &LaneInstruction, view: Carried) -> LaneFrame<ACC> {
    let mut frame = row_frame::<H, OP, ACC, FLAG_UNREAD>(cx, r, ip, i, view);
    if let Some(form) = const { call_form(MISS) } {
        // SAFETY: the running lane's context; the addresses are taken, nothing is read.
        let vm = unsafe { vm(cx) };
        frame.call = Some(LaneCall {
            context: cx.cast(),
            stack: &raw const *vm.lane_stack,
            globals: &raw const vm.realm.globals,
            record: std::ptr::from_ref(i),
            ip,
            form,
            target: std::cell::Cell::new(std::ptr::null()),
            depth: 0,
            activate: const { call_activation::<H>(MISS) },
        });
    }
    frame
}

script_handler! {
    /// The lane handler of a returning record of opcode `OP` at leaf form `LEAF`, whose RETURN
    /// parameter is `MISS` (`protocol_misses::returning_code`). It is generated from its staged row as
    /// `protocol_row` is, but its value completes the activation (`lane_return_value!`) instead of
    /// writing a register. The record decodes with its result flags stripped (`decode_completed`), and
    /// only a record whose flags are exactly the return reaches its row. Any other record, and any step
    /// but a value, takes the record's protocol miss, which completes the returning value itself.
    pub(super) fn returning_row<OP: u16, LEAF: u8, MISS: u16>(cx, r, ip, i, view) {
        let fields = LaneFields::<OP>(i);
        // A superinstruction's pair decodes from its parts in the program's table (`super_parts_in_lane`).
        let decoded = if const { crate::staged::super_const_array_family(LaneFields::<OP>::OPCODE) } {
            Operation::decode_literal_completed_with(crate::staged::Fields::wide(fields), unsafe { super::super_parts_in_lane::<H>(cx, i.imm() as usize) }, ())
        } else {
            Operation::decode_completed(crate::staged::Fields::wide(fields), crate::staged::DecodeContext::EMPTY)
        };
        let Some((operation, _)) = decoded.filter(|(_, flags)| *flags == crate::bytecode::RETURN_REGISTER) else {
            sibling!(const { protocol_miss::<H>(MISS) }, r, ip, view)
        };
        let mut stage = Concrete::new(std::ptr::null());
        let mut frame = row_frame::<H, OP, ACC_NONE, FLAG_UNREAD>(cx, r, ip, i, view);
        match row_step!(LaneFields::<OP>::OPCODE, &mut stage, &mut frame, operation) {
            Step::Value(value) => {
                let value = Value::from_bits(value);
                if let Some(resumed) = by_leaf!(LEAF, |L| super::complete_plain_activation_in_lane::<H, L>(cx, value)) {
                    resume!(cx, resumed)
                }
                resume!(cx, super::return_value_general_in_lane(cx, ip, value));
            }
            Step::Control(_) | Step::Unresolved => sibling!(const { protocol_miss::<H>(MISS) }, r, ip, view),
        }
    }
}

script_handler! {
    /// The lane handler of a returning `Binary` of arithmetic operator `OPERATOR` (a `BinaryOperator`'s
    /// value) on a register and a constant, at leaf form `LEAF`, whose RETURN parameter is `MISS`
    /// (`protocol_misses::operator_return_code`). The operator is static, so the row builds the staged
    /// `Binary` operation directly (`numeric::int32_binary_operation` where the record's constant is an
    /// int32 `INT32`, else `numeric::binary_operation`): the left operand is the record's register, the
    /// right one its constant, which the record's immediate (the operator field) no longer holds, so the
    /// frame's reads answer it from the record's own constant word. It runs over the frame `returning_row`
    /// runs over, and completes the activation with its value. Any other step, and a record whose flags
    /// are not exactly the return, takes the record's protocol miss (`protocol_miss`), which completes the
    /// value itself.
    pub(super) fn returning_operator_row<OP: u16, OPERATOR: u8, LEAF: u8, INT32: bool, MISS: u16>(cx, r, ip, i, view) {
        if i.a & !crate::bytecode::REGISTER_MASK != crate::bytecode::RETURN_REGISTER {
            sibling!(const { protocol_miss::<H>(MISS) }, r, ip, view)
        }
        let destination = i.a & crate::bytecode::REGISTER_MASK;
        let (left, right) = (Operand::register(i.b), Operand(i.c));
        let number = if INT32 {
            crate::staged::numeric::int32_binary_operation(OPERATOR as u32, destination, left, right)
        } else {
            crate::staged::numeric::binary_operation(OPERATOR as u32, destination, left, right)
        };
        let Some(number) = number else {
            sibling!(const { protocol_miss::<H>(MISS) }, r, ip, view)
        };
        let mut stage = Concrete::new(std::ptr::null());
        let mut frame = LaneFrame::<ACC_NONE> { reads: Reads::of_jump::<H>(cx, Some(i.constant())),
            ..row_frame::<H, OP, ACC_NONE, FLAG_UNREAD>(cx, r, ip, i, view) };
        match crate::staged::step_number(&mut stage, &mut frame, Operation::Number(number)) {
            Step::Value(value) => {
                // An int32 row's result is an integer already; a Float64 one is made as its canonical Value
                // (`Value::number`), so an integral result is the integer it is.
                let value = Value::from_bits(value);
                let value = if INT32 { value } else { value.as_number().map_or(value, Value::number) };
                if let Some(resumed) = by_leaf!(LEAF, |L| super::complete_plain_activation_in_lane::<H, L>(cx, value)) {
                    resume!(cx, resumed)
                }
                resume!(cx, super::return_value_general_in_lane(cx, ip, value));
            }
            Step::Control(_) | Step::Unresolved => sibling!(const { protocol_miss::<H>(MISS) }, r, ip, view),
        }
    }
}

script_handler! {
    /// The lane handler of a plain `Binary` of arithmetic operator `OPERATOR` (a `BinaryOperator`'s
    /// value) whose accumulator field is `ACC`: `generated_row`'s row with the operator a parameter of the
    /// instantiation rather than a field the row decodes and dispatches on. Where `INT32` (an operand is an
    /// int32 constant) the operation is `numeric::int32_binary_operation`'s, else `numeric::binary_operation`'s,
    /// over the record's own operands. Any step but a value hands the record to its generated row for good
    /// (`requicken`), which decides every operand pair.
    pub(super) fn operator_row<OP: u16, ACC: u8, OPERATOR: u8, INT32: bool>(cx, r, ip, i, view) {
        let generic: Handler = generated_row!(H, OP, ACC, FLAG_UNREAD);
        let (left, right) = (Operand(i.b), Operand(i.c));
        let number = if INT32 {
            crate::staged::numeric::int32_binary_operation(OPERATOR as u32, i.a, left, right)
        } else {
            crate::staged::numeric::binary_operation(OPERATOR as u32, i.a, left, right)
        };
        let Some(number) = number else {
            requicken::<H>(ip, generic);
            sibling!(generic, r, ip, view)
        };
        let mut stage = Concrete::new(std::ptr::null());
        let mut frame = row_frame::<H, OP, ACC, FLAG_UNREAD>(cx, r, ip, i, view);
        match crate::staged::step_number(&mut stage, &mut frame, Operation::Number(number)) {
            Step::Value(value) => next!(cx, r, following(ip), Carried { acc: Slot::of(Value::from_bits(value)), ..view }),
            Step::Control(_) | Step::Unresolved => {
                requicken::<H>(ip, generic);
                sibling!(generic, r, ip, view)
            }
        }
    }
}

/// Whether `handler` is opcode `OP`'s plain row in any direction: its value row with no
/// accumulator field, or its jump row.
pub(in crate::vm::dispatch_fast) fn plain_row<H: Host, const OP: u16>(handler: *const ()) -> bool {
    if const { jumps_by_row(LaneFields::<OP>::OPCODE) } {
        return handler == jump_row::<H, OP>(false) as *const () || handler == jump_row::<H, OP>(true) as *const ();
    }
    is_row_flagged_plain::<H, OP>(handler)
}

/// Whether `handler` is opcode `OP`'s row with no accumulator field at any flag.
fn is_row_flagged_plain<H: Host, const OP: u16>(handler: *const ()) -> bool {
    [generated_row!(H, OP, ACC_NONE, FLAG_UNREAD) as Handler, generated_row!(H, OP, ACC_NONE, FLAG_CLEAR), generated_row!(H, OP, ACC_NONE, FLAG_SET)]
        .iter().any(|row| *row as *const () == handler)
}

/// Whether `handler` is opcode `OP`'s backward jump row.
fn jump_row_backward<H: Host, const OP: u16>(handler: *const ()) -> bool {
    if const { jumps_by_row(LaneFields::<OP>::OPCODE) } { handler == jump_row::<H, OP>(true) as *const () } else { false }
}

/// The value `allocation` makes in the lane context `cx` (a `*mut LaneContext<H>`): it
/// allocates and runs no guest code, so a frame's own run makes it as the general loop does;
/// `None` where the general path must.
///
/// # Safety
/// `cx` is the running lane's context and `r` its register file. Called through
/// `LaneHost::allocate`, a constant function pointer in the inlined frame a row builds, so the
/// call inlines and each handler keeps only its allocation's arm.
#[inline(always)]
pub(super) unsafe fn lane_allocation<H: Host>(cx: *mut (), r: Registers, allocation: Allocation) -> Option<Value> {
    let cx = cx.cast::<LaneContext<H>>();
    match allocation {
        Allocation::Empty => Some(new_object_in_lane(unsafe { vm(cx) })),
        Allocation::Array { length, numbers } => Some(new_hole_array_in_lane(unsafe { vm(cx) }, length as usize, numbers)),
        Allocation::ArrayOf { elements } => Some(literal_array_value_in_lane(unsafe { vm(cx) }, r, elements)),
        Allocation::RegExp { site, callee: None } => regexp_literal_value_in_lane(cx, site),
        Allocation::RegExp { site, callee: Some(callee) } => lane_regexp_argument_value(cx, r.get(callee), site),
        Allocation::ConstArray { constant, count } => const_array_value_in_lane(cx, constant, count),
        Allocation::ObjectPair { site, first, second } => Some(object_pair_in_lane(cx, r, site, [first, second])),
        Allocation::ObjectLiteral { site, window } => Some(object_literal_value_in_lane(cx, r, site, window)),
        Allocation::Closure { function } => closure_value_of_in_lane(cx, function),
        Allocation::Template { template } => template_value_in_lane(cx, template),
        Allocation::Concat { window } => concat_value_in_lane(cx, r, window),
    }
}

/// `DefineArrayElement` of `value` at `index` into the literal array `array` in the lane context `cx`'s
/// own run (`Vm::define_literal_element`); false where the general path must define it.
///
/// # Safety
/// `cx` is the running lane's context. Called through `LaneHost::define`, a constant function pointer.
#[inline(always)]
unsafe fn define_element_in_lane<H: Host>(cx: *mut (), array: u64, index: u32, value: u64) -> bool {
    let vm = unsafe { vm(cx.cast::<LaneContext<H>>()) };
    vm.define_literal_element(Value::from_bits(array), index as usize, Value::from_bits(value))
}

/// The count of the values the running keeping activation kept and the register holding the
/// first (`ScriptActivation::kept`); `None` in a frame or any other activation.
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
unsafe fn kept_values<H: Host>(cx: *mut ()) -> Option<(u16, u16)> {
    let cx = cx.cast::<LaneContext<H>>();
    if unsafe { (*cx).registers }.env().is_deleted() {
        return None;
    }
    let script = unsafe { vm(cx) }.lane_stack.top_frame()?.script().filter(|script| script.forwards)?;
    Some((script.argc, script.kept))
}

/// Whether `receiver` already holds its constructor's run of constant fields
/// (`Vm::constant_fields_held`): one cache entry of site `site` and the receiver's shape, read in
/// the lane context `cx`; it allocates nothing and runs no guest code.
///
/// # Safety
/// `cx` is the running lane's context. Called through `LaneHost::constant_fields`, a constant
/// function pointer in the frame a names row builds (`row_frame`), so the call inlines.
#[inline(always)]
unsafe fn constant_fields_value<H: Host>(cx: *mut (), receiver: Value, site: u16) -> bool {
    unsafe { vm(cx.cast::<LaneContext<H>>()) }.constant_fields_held(receiver, site)
}

/// The value of `atom` under `read` in the lane context `cx`, which allocates nothing and runs
/// no guest code; `None` where the binding does not answer it (a native no first run recorded,
/// an intrinsic name the VM cannot evaluate, a hint the global object's shape no longer fits).
///
/// # Safety
/// `cx` is the running lane's context. Called through `LaneHost::read_name`, a constant
/// function pointer in the inlined frame `row_frame` builds for a by-name load, so the call inlines.
#[inline(always)]
unsafe fn name_value_in_lane<H: Host>(cx: *mut (), read: NameRead, atom: u32) -> Option<Value> {
    let vm = unsafe { vm(cx.cast::<LaneContext<H>>()) };
    match read {
        NameRead::Native => {
            let native = vm.intrinsic_natives.get(atom as usize).copied().flatten()?;
            Some(vm.native_value(native))
        }
        NameRead::Intrinsic => vm.intrinsic_name_value(atom),
        NameRead::Field(hint) => vm.record_field_entry_hit(hint, vm.realm.globals),
        NameRead::Lexical(hint) => vm.global_lexical_at(atom, hint.slot),
    }
}

/// The `true` of `delete base[key]` of a string key on an ordinary object, which runs no guest
/// code. A refused deletion (a non-configurable property) has changed nothing and, like any
/// other base or key, is the general path's: strict code makes it an error.
///
/// # Safety
/// `cx` is the running lane's context.
unsafe fn delete_value_in_lane<H: Host>(cx: *mut (), base: Value, key: Value) -> Option<Value> {
    let cx = cx.cast::<LaneContext<H>>();
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let plain = base.direct_object_index().is_some()
        && vm.heap.string(key).is_some()
        && vm.object_data(base).is_some_and(|data| !data.is_module_namespace() && !data.is_arguments_object());
    (plain && vm.object_delete_property(code, &[base, key]).is_ok_and(|deleted| deleted == Value::TRUE)).then_some(Value::TRUE)
}

/// The fast answer of a `Names` row's iteration or for-in call-out over its operands (`[first, second]`,
/// the registers the call names in order): the VM's fast arm of the step, which runs no guest code and
/// allocates at most what the general step would. `None` where the general step decides it.
///
/// # Safety
/// `cx` is the running lane's context. Called through `LaneFrame::names`, a constant function pointer
/// in the inlined frame `row_frame` builds, so the call inlines.
#[inline(always)]
unsafe fn fast_names_step<H: Host>(cx: *mut (), call: CallOut, [first, second]: [Option<Value>; 2]) -> Option<Value> {
    use crate::staged::protocol_classes::ClassStep;
    let vm = unsafe { vm(cx.cast::<LaneContext<H>>()) };
    match call {
        // A built-in iterator whose chain lacks `return` closes with `undefined`.
        CallOut::Protocol { step: Protocol::IteratorClose, .. } => {
            vm.iterator_close_absent(second?).then_some(Value::UNDEFINED)
        }
        // The intrinsic values iterator of an array whose iteration is intact, made without looking
        // up or calling `@@iterator`.
        CallOut::Protocol { step: Protocol::GetIterator, .. } => {
            let source = second?;
            if !vm.array_iteration_intact(source) {
                return None;
            }
            let iterator = vm.array_iterator_of(crate::heap::IteratorKind::ArrayValues, source);
            note_allocation(vm);
            Some(iterator)
        }
        // The loop's state over the snapshot of an object whose keys the for-in memo lists.
        CallOut::Protocol { step: Protocol::ForInStart, .. } => {
            let source = second?;
            let keys = vm.listed_for_in_keys(source)?;
            let state = vm.for_in_state_in_lane(source, keys);
            // A state may be allocated; a due collection runs at the lane's next safepoint.
            note_allocation(vm);
            Some(state)
        }
        // The next key of a listed snapshot, `Value::DELETED` once none is left (which takes the exit).
        CallOut::ForInNext { .. } => vm.for_in_step_listed(second?).map(|key| key.unwrap_or(Value::DELETED)),
        // ToPropertyKey of a primitive: the VM's conversion, which runs no guest code for one. An object's
        // conversion calls its `toString` or `valueOf`, so the general step makes it.
        CallOut::PropertyKey { .. } => {
            let source = second?;
            if vm.is_object_like(source) {
                return None;
            }
            // SAFETY: the context's program outlives the lane run.
            let program = unsafe { &*(*cx.cast::<LaneContext<H>>()).code };
            let key = vm.to_property_key(program, source).ok()?;
            note_allocation(vm);
            Some(key)
        }
        // A template object's cache of the running frame (`Vm::cache_template_object`, `load_template_object`),
        // keyed as the VM keys it. A frameless activation beside the lane leaves the step to the general path.
        CallOut::Protocol { step: Protocol::Literal(crate::staged::protocol_literals::LiteralStep::CacheTemplate { site }), .. } => {
            let frame = unsafe { (*cx.cast::<LaneContext<H>>()).frame() };
            if vm.lane_stack.has_frames() || frame >= vm.frames.len() {
                return None;
            }
            Some(vm.cache_template_object(frame, site, first?))
        }
        CallOut::Protocol { step: Protocol::Literal(crate::staged::protocol_literals::LiteralStep::LoadTemplate { site }), .. } => {
            let frame = unsafe { (*cx.cast::<LaneContext<H>>()).frame() };
            if vm.lane_stack.has_frames() || frame >= vm.frames.len() {
                return None;
            }
            Some(vm.load_template_object(frame, site))
        }
        // `SetFunctionNameKey` (`Vm::set_function_name_key`): the function named by a key, which runs no guest
        // code (a string or symbol key names it; any other key names nothing, as the VM's helper does).
        CallOut::Protocol { step: Protocol::Class(ClassStep::SetFunctionNameKey { prefix }), .. } => {
            let (function, key) = (first?, second?);
            vm.set_function_name_key(function, key, prefix);
            note_allocation(vm);
            Some(Value::UNDEFINED)
        }
        // `DefineComputedField` (`Vm::define_class_field` of the key's conversion): a primitive key converts
        // without guest code, and an object key's `toString` or `valueOf` runs guest code, so it takes the
        // general step. A proxy target's define runs its trap, and a keeping activation beside the lane
        // holds registers that are not the frame's, so both leave to the general step too.
        CallOut::Protocol { step: Protocol::Class(ClassStep::DefineComputedField { key: key_register }), .. } => {
            if vm.lane_stack.has_frames() {
                return None;
            }
            let (value, object) = (first?, second?);
            if vm.proxy_parts(object).is_some() {
                return None;
            }
            // SAFETY: the context's program outlives the lane run, and its registers are the running frame's.
            let (program, source) = unsafe {
                let context = cx.cast::<LaneContext<H>>();
                (&*(*context).code, (*context).registers.get(key_register))
            };
            if vm.is_object_like(source) {
                return None;
            }
            let source = vm.to_property_key(program, source).ok()?;
            let key = if let Some(atom) = vm.string_key_atom(source) {
                crate::vm::PropertyKey::string(atom)
            } else if vm.heap.is_symbol(source) {
                crate::vm::PropertyKey::symbol(source)
            } else {
                return None;
            };
            vm.define_class_field(program, object, key, value).ok()?;
            note_allocation(vm);
            Some(Value::UNDEFINED)
        }
        // `CopyDataProperties` (`Vm::copy_plain_data_properties`): a plain copy runs no guest code and cannot
        // throw; a copy it declines leaves to the row's miss, which takes the general step. The exclusions are
        // the running frame's registers, so a keeping activation beside the lane leaves the copy to its miss.
        CallOut::Protocol { step: Protocol::Literal(crate::staged::protocol_literals::LiteralStep::CopyDataProperties { exclusions }), .. } => {
            if vm.lane_stack.has_frames() {
                return None;
            }
            let (target, source) = (first?, second?);
            // SAFETY: as for `DefineComputedField`.
            let exclusions = unsafe { (*cx.cast::<LaneContext<H>>()).registers }.get(exclusions);
            if !vm.copy_plain_data_properties(target, source, exclusions) {
                return None;
            }
            note_allocation(vm);
            Some(Value::UNDEFINED)
        }
        // A spread onto the literal's array with no elisions before it, from a source whose
        // iteration runs no guest code: the append defines no register, so its value is unused.
        CallOut::Protocol { step: Protocol::AppendArray { length: 0, spread: true }, .. } => {
            let (array, source) = (first?, second?);
            if !vm.spread_intact_in_place(array, source) {
                return None;
            }
            note_allocation(vm);
            Some(Value::UNDEFINED)
        }
        _ => None,
    }
}

/// The array of the `count` program constants from `constant` (`Vm::const_array_value`);
/// `None` where the range is outside the program's constants.
#[inline(never)]
fn const_array_value_in_lane<H: Host>(cx: *mut LaneContext<H>, constant: u32, count: u16) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let value = vm.const_array_value(unsafe { (*cx).program }, constant as usize, usize::from(count))?;
    note_allocation(vm);
    Some(value)
}

/// The value of literal template `template` in a frame's own lane run (`Vm::literal_template`);
/// `None` inside a lane activation, whose frame is not the context's, or where it fails.
#[inline(never)]
fn template_value_in_lane<H: Host>(cx: *mut LaneContext<H>, template: u32) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    if vm.lane_stack.has_frames() {
        return None;
    }
    // SAFETY: the context's program outlives the lane run.
    let (code, frame) = unsafe { (&*(*cx).code, (*cx).frame()) };
    let value = vm.literal_template(code, frame, template as usize).ok();
    note_allocation(vm);
    value
}

/// The string of the parts in `window`, each converted without guest code
/// (`Vm::pure_string_concat`); `None` where any part needs the general path.
#[inline(never)]
fn concat_value_in_lane<H: Host>(cx: *mut LaneContext<H>, r: Registers, window: RegisterWindow) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    let base = usize::from(window.base);
    // SAFETY: the context's program outlives the lane run.
    let value = vm.pure_string_concat(unsafe { &*(*cx).code }, usize::from(window.count),
        |_: &Vm<H>, part: usize| r.get((base + part) as Register))?;
    note_allocation(vm);
    Some(value)
}

/// `delete base[key]` into `destination` by the staged operation, in the lane context `cx`'s own
/// run; false, writing nothing, where the general path must make it.
#[inline(always)]
pub(super) fn staged_delete<H: Host>(cx: *mut LaneContext<H>, r: Registers, [destination, base, key]: [u16; 3], strict: bool) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::hosted::<H>(r, Carried::script(cx), cx);
    matches!(crate::staged::step(&mut stage, &mut frame, Operation::Delete { destination, base, key, strict }), Step::Value(_))
}

// The fusion handlers and their install passes (`fuse_rows`, `fuse_chains`), in their own module.
#[path = "lane_frame/fused.rs"]
mod fused;
pub(in crate::vm::dispatch_fast) use fused::{chain_row, fused_row, fused_rows, fuse_chains, fuse_rows, is_window_row};

/// The lane context's string-case index for a `StringCaseIndex` row (`case_index_in_lane`): the record's
/// filter, null where the record has none.
#[derive(Clone, Copy)]
struct StringCases {
    context: *mut (),
    filter: *const super::string_case_filter::StringCaseFilter,
    index: unsafe fn(*mut (), *const super::string_case_filter::StringCaseFilter, Value, usize, usize) -> u32,
}

/// The `typeof` kind of the heap value `value`, as the VM's `typeof` names it (`Vm::typeof_kind`: an
/// IsHTMLDDA object reads as undefined): the index of its name in `TYPEOF_NAMES`.
///
/// # Safety
/// `cx` is the running lane's context.
unsafe fn type_of_in_lane<H: Host>(cx: *mut (), value: Value) -> Option<crate::staged::value::TypeofKind> {
    let vm = unsafe { vm(cx.cast::<LaneContext<H>>()) };
    let index = crate::vm::coercion::typeof_name_index(vm.typeof_kind(value));
    Some(crate::staged::value::TypeofKind::of_index(index as u64))
}

/// The interned `typeof` result string of the name index `name` (`Vm::typeof_names`), which
/// `typeof` yields without allocating.
///
/// # Safety
/// `cx` is the running lane's context.
unsafe fn typeof_string_in_lane<H: Host>(cx: *mut (), name: u64) -> Option<Value> {
    let vm = unsafe { vm(cx.cast::<LaneContext<H>>()) };
    vm.typeof_names.get(usize::try_from(name).ok()?).copied()
}

/// SetFunctionName of `function` to the program atom `name` through cache `site`, by the VM's own naming
/// (`Vm::set_function_name`); `None` where it raises.
///
/// # Safety
/// `cx` is the running lane's context.
unsafe fn set_function_name_in_lane<H: Host>(cx: *mut (), function: Value, name: u32, site: u16) -> Option<()> {
    let cx = cx.cast::<LaneContext<H>>();
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    vm.set_function_name(code, function, (name, site)).ok()
}

/// Whether `value` is an object of any kind the VM admits as an iterator result (`Vm::is_object_like`),
/// for `RequireIteratorResult`'s object arm (`LaneFrame::object_like`).
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
unsafe fn object_like_in_lane<H: Host>(cx: *mut (), value: Value) -> bool {
    unsafe { vm(cx.cast::<LaneContext<H>>()) }.is_object_like(value)
}

/// `IteratorCleanupPush` in the running VM frame, as the general loop runs it: the iterator to close if
/// the loop exits abruptly. A frameless activation keeps it on the lane stack (`LaneStack::iterators`),
/// which its frame takes if it materializes.
///
/// # Safety
/// `cx` is the running lane's context.
unsafe fn iterator_push_in_lane<H: Host>(cx: *mut (), iterator: Register, done: Register) {
    let cx = cx.cast::<LaneContext<H>>();
    let vm = unsafe { vm(cx) };
    let iterator = crate::vm::ActiveIterator { iterator, done };
    if let Some(depth) = vm.lane_stack.activations().len().checked_sub(1) {
        vm.lane_stack.iterators.push((depth, iterator));
    } else {
        let frame = unsafe { (*cx).frame() };
        vm.frames[frame].active_iterators.push(iterator);
    }
}

/// `IteratorCleanupPop` in the running VM frame: the running activation's own entry, the last. `false`
/// where there is none, which the general path raises for.
///
/// # Safety
/// `cx` is the running lane's context.
unsafe fn iterator_pop_in_lane<H: Host>(cx: *mut ()) -> bool {
    let cx = cx.cast::<LaneContext<H>>();
    let vm = unsafe { vm(cx) };
    match vm.lane_stack.activations().len().checked_sub(1) {
        Some(depth) => vm.lane_stack.iterators.pop_if(|(owner, _)| *owner == depth).is_some(),
        None => {
            let frame = unsafe { (*cx).frame() };
            vm.frames[frame].active_iterators.pop().is_some()
        }
    }
}

// `generated_row`'s body, once per shard (`generated_row_shard`).
mod generated_rows_0 {
    use super::*;
    include!("generated_row.rs");
}
mod generated_rows_1 {
    use super::*;
    include!("generated_row.rs");
}
mod generated_rows_2 {
    use super::*;
    include!("generated_row.rs");
}
mod generated_rows_3 {
    use super::*;
    include!("generated_row.rs");
}
mod generated_rows_4 {
    use super::*;
    include!("generated_row.rs");
}
mod generated_rows_5 {
    use super::*;
    include!("generated_row.rs");
}
mod generated_rows_6 {
    use super::*;
    include!("generated_row.rs");
}
mod generated_rows_7 {
    use super::*;
    include!("generated_row.rs");
}
mod generated_rows_8 {
    use super::*;
    include!("generated_row.rs");
}
mod generated_rows_9 {
    use super::*;
    include!("generated_row.rs");
}
mod generated_rows_10 {
    use super::*;
    include!("generated_row.rs");
}
mod generated_rows_11 {
    use super::*;
    include!("generated_row.rs");
}
mod generated_rows_12 {
    use super::*;
    include!("generated_row.rs");
}
mod generated_rows_13 {
    use super::*;
    include!("generated_row.rs");
}
mod generated_rows_14 {
    use super::*;
    include!("generated_row.rs");
}
mod generated_rows_15 {
    use super::*;
    include!("generated_row.rs");
}
