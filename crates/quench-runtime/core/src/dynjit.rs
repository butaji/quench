use super::coverage::ExecutionMode;
use super::dynbytecode::{
    ARGUMENTS_BINDING_NAME, AccessorKind, CatchBinding, DynCode, DynOp, Literal, Register,
    THIS_BINDING_NAME, UnaryKind,
};
use super::region_plan::RegionPlan;
use super::*;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

#[allow(unexpected_cfgs)]
mod site_holes {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/stencil-aot/site_holes.rs"
    ));
}

mod rustc_stencils {
    include!(concat!(env!("OUT_DIR"), "/stencil_catalog.rs"));
}

impl rustc_stencils::RustcStencil {
    fn patch_offsets(
        &self,
        binding: patch_schema::PatchBinding,
    ) -> impl Iterator<Item = usize> + '_ {
        self.patch_sites
            .iter()
            .filter_map(move |site| (site.binding == binding).then_some(site.offset))
    }

    fn unique_patch_offset(&self, binding: patch_schema::PatchBinding) -> Option<usize> {
        let mut offsets = self.patch_offsets(binding);
        let offset = offsets.next();
        assert!(offsets.next().is_none(), "patch binding must be unique");
        offset
    }

    fn next_relocation(&self) -> usize {
        self.unique_patch_offset(patch_schema::PatchBinding::Next)
            .expect("stencil has a next edge")
    }

    fn slow_relocation(&self) -> Option<usize> {
        self.unique_patch_offset(patch_schema::PatchBinding::Slow)
    }

    fn branch_relocation(&self) -> Option<usize> {
        self.unique_patch_offset(patch_schema::PatchBinding::Taken)
    }

    fn operand_relocations(&self) -> Vec<(usize, operand_holes::OperandHoleKind)> {
        self.patch_sites
            .iter()
            .filter_map(|site| match site.binding {
                patch_schema::PatchBinding::Operand(kind) => Some((site.offset, kind)),
                _ => None,
            })
            .collect()
    }

    fn site_advance_relocations(&self) -> Vec<usize> {
        self.patch_offsets(patch_schema::PatchBinding::SiteAdvanceBytes)
            .collect()
    }
}

const FIRST_INSTRUCTION_PC: usize = 0;
const LEGACY_BLOCK_STENCIL_BYTES: usize = 32;
const DIRECT_BLOCK_STENCIL_BYTES: usize = 40;
const EFFECT_REENTRY_KERNEL_BYTES: usize = 32;
const EFFECT_REENTRY_ADAPTER_BYTES: usize = 16;
const HELPER_LITERAL_OFFSET: usize = 24;
const EFFECT_REENTRY_KERNEL_LITERAL_OFFSET: usize = 24;
const EFFECT_REENTRY_ADAPTER_LITERAL_OFFSET: usize = 8;
const BLOCK_HELPER_LITERAL_OFFSET: usize = 32;
const PC_INSTRUCTION_OFFSET: usize = 4;
const MAX_EMBEDDED_PC: usize = u16::MAX as usize;
const NEXT_INSTRUCTION_DISTANCE: usize = 1;
const UNBOUNDED_SEMANTIC_RANGE_END: usize = usize::MAX;
const CONDITION_INSTRUCTION_COUNT: usize = 4;
const CONDITION_COMPARE_PC_OFFSET: usize = 2;
const CONDITION_BRANCH_PC_OFFSET: usize = 3;
const NAME_OPERAND_PC_OFFSET: usize = 1;
const UPDATE_LITERAL_PC_OFFSET: usize = 1;
const UPDATE_BINARY_PC_OFFSET: usize = 2;
const UPDATE_STORE_PC_OFFSET: usize = 3;
const UPDATE_JUMP_PC_OFFSET: usize = 4;
const UPDATE_INSTRUCTION_COUNT: usize = 5;
const LOCAL_UPDATE_INSTRUCTION_COUNT: usize = 4;
const RECURRENCE_COMPARE_PC_OFFSET: usize = 5;
const RECURRENCE_BRANCH_PC_OFFSET: usize = 6;
const PROPERTY_EQUAL_FIRST_GET_PC_OFFSET: usize = 1;
const PROPERTY_EQUAL_SECOND_GET_PC_OFFSET: usize = 3;
const PROPERTY_EQUAL_COMPARE_PC_OFFSET: usize = 4;
const PROPERTY_EQUAL_BRANCH_PC_OFFSET: usize = 5;
const PROPERTY_LITERAL_GET_PC_OFFSET: usize = 1;
const PROPERTY_LITERAL_COMPARE_PC_OFFSET: usize = 3;
const PROPERTY_LITERAL_BRANCH_PC_OFFSET: usize = 4;
const PROPERTY_LOAD_GET_PC_OFFSET: usize = 1;
const PROPERTY_LOAD_STORE_PC_OFFSET: usize = 2;
const PROPERTY_LOAD_JUMP_PC_OFFSET: usize = 3;
const PROPERTY_LOAD_JUMP_INSTRUCTION_COUNT: usize = 4;
const TWO_PROPERTY_STORE_FIRST_RECEIVER_LOAD_PC_OFFSET: usize = 0;
const TWO_PROPERTY_STORE_FIRST_VALUE_LOAD_PC_OFFSET: usize = 1;
const TWO_PROPERTY_STORE_FIRST_SET_PC_OFFSET: usize = 2;
const TWO_PROPERTY_STORE_SECOND_RECEIVER_LOAD_PC_OFFSET: usize = 3;
const TWO_PROPERTY_STORE_SECOND_VALUE_LOAD_PC_OFFSET: usize = 4;
const TWO_PROPERTY_STORE_SECOND_SET_PC_OFFSET: usize = 5;
const TWO_PROPERTY_STORE_RETURN_PC_OFFSET: usize = 6;
const TWO_PROPERTY_STORE_INSTRUCTION_COUNT: usize = 7;
const _: () = assert!(
    TWO_PROPERTY_STORE_FIRST_RECEIVER_LOAD_PC_OFFSET + 1
        == TWO_PROPERTY_STORE_FIRST_VALUE_LOAD_PC_OFFSET
);
const _: () = assert!(
    TWO_PROPERTY_STORE_FIRST_VALUE_LOAD_PC_OFFSET + 1 == TWO_PROPERTY_STORE_FIRST_SET_PC_OFFSET
);
const _: () = assert!(
    TWO_PROPERTY_STORE_FIRST_SET_PC_OFFSET + 1 == TWO_PROPERTY_STORE_SECOND_RECEIVER_LOAD_PC_OFFSET
);
const _: () = assert!(
    TWO_PROPERTY_STORE_SECOND_RECEIVER_LOAD_PC_OFFSET + 1
        == TWO_PROPERTY_STORE_SECOND_VALUE_LOAD_PC_OFFSET
);
const _: () = assert!(
    TWO_PROPERTY_STORE_SECOND_VALUE_LOAD_PC_OFFSET + 1 == TWO_PROPERTY_STORE_SECOND_SET_PC_OFFSET
);
const _: () =
    assert!(TWO_PROPERTY_STORE_SECOND_SET_PC_OFFSET + 1 == TWO_PROPERTY_STORE_RETURN_PC_OFFSET);
const _: () =
    assert!(TWO_PROPERTY_STORE_RETURN_PC_OFFSET + 1 == TWO_PROPERTY_STORE_INSTRUCTION_COUNT);
const INSTANCEOF_LOAD_LOCAL_PC_OFFSET: usize = 0;
const INSTANCEOF_LOAD_NAME_PC_OFFSET: usize = 1;
const INSTANCEOF_OPERATION_PC_OFFSET: usize = 2;
const INSTANCEOF_BRANCH_PC_OFFSET: usize = 3;
const INSTANCEOF_INSTRUCTION_COUNT: usize = 4;
const INSTANCEOF_NOT_OPERATION_PC_OFFSET: usize = 3;
const INSTANCEOF_NOT_BRANCH_PC_OFFSET: usize = 4;
const INSTANCEOF_NOT_INSTRUCTION_COUNT: usize = 5;
const INSTANCEOF_CACHE_FALSE: usize = 0;
const INSTANCEOF_CACHE_TRUE: usize = 1;
const _: () = assert!(INSTANCEOF_LOAD_LOCAL_PC_OFFSET == 0);
const _: () = assert!(INSTANCEOF_LOAD_NAME_PC_OFFSET + 1 == INSTANCEOF_OPERATION_PC_OFFSET);
const _: () = assert!(INSTANCEOF_BRANCH_PC_OFFSET + 1 == INSTANCEOF_INSTRUCTION_COUNT);
const _: () = assert!(INSTANCEOF_NOT_OPERATION_PC_OFFSET + 1 == INSTANCEOF_NOT_BRANCH_PC_OFFSET);
const _: () = assert!(INSTANCEOF_NOT_BRANCH_PC_OFFSET + 1 == INSTANCEOF_NOT_INSTRUCTION_COUNT);
const ARRAY_LENGTH_PROPERTY_NAME: &str = "length";
const STENCIL_COVERAGE_ENV: &str = "QUENCH_STENCIL_COVERAGE";
const BLOCK_KERNEL_ONLY_ENV: &str = "QUENCH_BLOCK_KERNEL_ONLY";
const BLOCK_SHAPE_TRACE_ENV: &str = "QUENCH_BLOCK_SHAPE_TRACE";
const PROTOTYPE_IC_STATS_ENV: &str = "QUENCH_PROTOTYPE_IC_STATS";
const EFFECT_REENTRY_STATS_ENV: &str = "QUENCH_EFFECT_REENTRY_STATS";
const DIRECT_CALL_STATS_ENV: &str = "QUENCH_DIRECT_CALL_STATS";
const NATIVE_PATH_STATS_ENV: &str = "QUENCH_NATIVE_PATH_STATS";
const MAX_CALLS_PER_INITIAL_REGION: usize = 1;
const MIN_SURROUNDING_DIRECT_OPCODES_PER_CALL_REGION: usize = 2;
const GET_STATIC_CALL_INSTRUCTION_COUNT: usize = 2;
const GET_STATIC_LOAD_LOCAL_CALL_INSTRUCTION_COUNT: usize = 3;
const RESIDUAL_BLOCK_STATS_ENV: &str = "QUENCH_RESIDUAL_BLOCK_STATS";
const UNUSED_SITE_OPERAND: usize = usize::MAX;
const EMPTY_PROPERTY_SLOT: usize = usize::MAX;
const EMPTY_INHERITED_PROPERTY_SLOT: usize = 0;
const NO_ALLOCATION_SHAPE_POINTER: u64 = 0;
const NO_CALL_REGISTER: usize = usize::MAX;
const NO_CALL_SPAN_START: u32 = u32::MAX;
const CALL_RECIPE_FLAG_DISABLED: usize = 0;
const CALL_RECIPE_FLAG_ENABLED: usize = 1;
const GENERIC_BLOCK_VERSION_COUNT: usize = 1;
const SPECIALIZED_BLOCK_VERSION_COUNT: usize = 1;
const MAX_STATIC_BLOCK_VERSIONS: usize =
    GENERIC_BLOCK_VERSION_COUNT + SPECIALIZED_BLOCK_VERSION_COUNT;
const MAX_REPORTED_GUARD_FAILURE_SITES: usize = 512;

#[repr(usize)]
enum InlineOpcode {
    Unsupported,
    LoadLiteral,
    LoadLocal,
    StoreLocal,
    Move,
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Jump,
    JumpIfFalse,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct InlineSite {
    pc: usize,
    opcode: usize,
    run_end: usize,
    dst: usize,
    left: usize,
    right: usize,
    literal: u64,
}

impl InlineSite {
    fn unused(pc: usize) -> Self {
        Self {
            pc,
            opcode: InlineOpcode::Unsupported as usize,
            run_end: pc,
            dst: UNUSED_SITE_OPERAND,
            left: UNUSED_SITE_OPERAND,
            right: UNUSED_SITE_OPERAND,
            literal: raw_value::RawValue::UNDEFINED.bits(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
struct PropertyIc {
    receiver_shape: *const Shape,
    slot: usize,
}

impl PropertyIc {
    const EMPTY: Self = Self {
        receiver_shape: std::ptr::null(),
        slot: EMPTY_PROPERTY_SLOT,
    };

    fn populated(self) -> Option<Self> {
        (!self.receiver_shape.is_null()).then_some(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
struct InstanceOfIc {
    constructor_bits: u64,
    receiver_prototype_word: usize,
    epoch: u64,
    answer: usize,
}

impl InstanceOfIc {
    const EMPTY: Self = Self {
        constructor_bits: raw_value::UNDEFINED_TAG,
        receiver_prototype_word: 0,
        epoch: INITIAL_PROTOTYPE_EPOCH,
        answer: INSTANCEOF_CACHE_FALSE,
    };
}

#[repr(C)]
struct InstanceOfIcSite {
    entry: Cell<InstanceOfIc>,
}

impl InstanceOfIcSite {
    fn new() -> Self {
        Self {
            entry: Cell::new(InstanceOfIc::EMPTY),
        }
    }

    fn invalidate(&self) {
        self.entry.set(InstanceOfIc::EMPTY);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PrototypeGuard {
    identity: *const ObjectCell,
    shape: *const Shape,
}

#[derive(Debug)]
struct PrototypePropertyIc {
    receiver_shape: *const Shape,
    chain: Box<[PrototypeGuard]>,
    slot: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
struct PublishedInheritedPropertyIc {
    receiver_shape: *const Shape,
    holder_identity: *const ObjectCell,
    holder_shape: *const Shape,
    slot: usize,
}

impl PublishedInheritedPropertyIc {
    const EMPTY: Self = Self {
        receiver_shape: std::ptr::null(),
        holder_identity: std::ptr::null(),
        holder_shape: std::ptr::null(),
        slot: EMPTY_INHERITED_PROPERTY_SLOT,
    };

    fn from_chain(receiver_shape: *const Shape, chain: &[PrototypeGuard], slot: usize) -> Self {
        let [holder] = chain else {
            return Self::EMPTY;
        };
        Self {
            receiver_shape,
            holder_identity: holder.identity,
            holder_shape: holder.shape,
            slot,
        }
    }
}

#[repr(C)]
struct PropertyIcSite {
    own: Cell<PropertyIc>,
    published_inherited: Cell<PublishedInheritedPropertyIc>,
    inherited: RefCell<Option<PrototypePropertyIc>>,
    stats_enabled: bool,
}

const _: () = assert!(std::mem::offset_of!(PropertyIcSite, own) == 0);
const _: () = assert!(
    std::mem::offset_of!(PropertyIcSite, published_inherited) == std::mem::size_of::<PropertyIc>()
);
const _: () = assert!(std::mem::size_of::<Cell<PropertyIc>>() == std::mem::size_of::<PropertyIc>());
impl PropertyIcSite {
    fn new(stats_enabled: bool) -> Self {
        Self {
            own: Cell::new(PropertyIc::EMPTY),
            published_inherited: Cell::new(PublishedInheritedPropertyIc::EMPTY),
            inherited: RefCell::new(None),
            stats_enabled,
        }
    }

    fn inherited_value(&self, root: &ObjectCell) -> Option<Value> {
        let cache = self.inherited.borrow();
        let location = cache.as_ref()?;
        let receiver = root.borrow();
        if receiver.props.shape.0 != location.receiver_shape {
            self.record_inherited_result(false);
            return None;
        }
        let mut current = receiver.prototype.clone();
        drop(receiver);
        for (index, guard) in location.chain.iter().enumerate() {
            let Some(object) = current else {
                self.record_inherited_result(false);
                return None;
            };
            if object.as_ptr() != guard.identity {
                self.record_inherited_result(false);
                return None;
            }
            let object_ref = object.borrow();
            if object_ref.props.shape.0 != guard.shape {
                self.record_inherited_result(false);
                return None;
            }
            if index + 1 == location.chain.len() {
                let Some((_, value)) = object_ref.props.get_index(location.slot) else {
                    self.record_inherited_result(false);
                    return None;
                };
                let value = value.clone();
                self.record_inherited_result(true);
                return Some(value);
            }
            current = object_ref.prototype.clone();
        }
        self.record_inherited_result(false);
        None
    }

    fn record_inherited_result(&self, hit: bool) {
        if !self.stats_enabled {
            return;
        }
        let counter = if hit {
            &PROTOTYPE_IC_RUNTIME_STATS.hits
        } else {
            &PROTOTYPE_IC_RUNTIME_STATS.misses
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    fn clear_inherited(&self) {
        self.published_inherited
            .set(PublishedInheritedPropertyIc::EMPTY);
        self.inherited.replace(None);
    }

    fn record_inherited(&self, location: PrototypePropertyIc) {
        self.published_inherited
            .set(PublishedInheritedPropertyIc::from_chain(
                location.receiver_shape,
                &location.chain,
                location.slot,
            ));
        self.inherited.replace(Some(location));
        if self.stats_enabled {
            PROTOTYPE_IC_RUNTIME_STATS
                .fills
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    fn invalidate_unmarked_object_identities(&self) {
        let should_clear = self.inherited.borrow().as_ref().is_some_and(|location| {
            location
                .chain
                .iter()
                .any(|guard| !unsafe { guard.identity.as_ref() }.is_some_and(ObjectCell::is_marked))
        });
        if should_clear {
            self.clear_inherited();
        }
    }
}

#[repr(C)]
struct FunctionCallRecipe {
    entry: DynEntry,
    guest_entry: DynEntry,
    layout: CallFrameLayout,
    parameter_slots: *const usize,
    parameter_count: usize,
    this_slot: usize,
    arguments_slot: usize,
    uses_arguments: usize,
    captures_frame: usize,
}

impl FunctionCallRecipe {
    fn flag(value: bool) -> usize {
        if value {
            CALL_RECIPE_FLAG_ENABLED
        } else {
            CALL_RECIPE_FLAG_DISABLED
        }
    }

    fn captures_frame(&self) -> bool {
        self.captures_frame == CALL_RECIPE_FLAG_ENABLED
    }

    fn uses_arguments(&self) -> bool {
        self.uses_arguments == CALL_RECIPE_FLAG_ENABLED
    }

    fn parameter_slots(&self) -> &[usize] {
        if self.parameter_count == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(self.parameter_slots, self.parameter_count) }
        }
    }
}

#[repr(C)]
pub(super) struct InlineCallTarget {
    pc: usize,
    dst: usize,
    callee: usize,
    receiver: usize,
    arguments: *const Register,
    argument_count: usize,
    span_start: u32,
    identity_word: Cell<u64>,
    recipe: Cell<*const FunctionCallRecipe>,
    environment: Cell<*const RefCell<Environment>>,
    reusable_activation: Cell<*mut DynFrame>,
}

impl InlineCallTarget {
    fn unused(pc: usize) -> Self {
        Self {
            pc,
            dst: NO_CALL_REGISTER,
            callee: NO_CALL_REGISTER,
            receiver: NO_CALL_REGISTER,
            arguments: std::ptr::null(),
            argument_count: 0,
            span_start: NO_CALL_SPAN_START,
            identity_word: Cell::new(raw_value::RawValue::UNDEFINED.bits()),
            recipe: Cell::new(std::ptr::null()),
            environment: Cell::new(std::ptr::null()),
            reusable_activation: Cell::new(std::ptr::null_mut()),
        }
    }

    fn arguments(&self) -> &[Register] {
        if self.argument_count == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(self.arguments, self.argument_count) }
        }
    }
}

struct UserCallIc {
    _callee_owner: Value,
    code: Rc<DynJitCode>,
    environment: Env,
}

pub(crate) struct CallIcSite {
    target: InlineCallTarget,
    _arguments: Box<[Register]>,
    entry: std::cell::OnceCell<UserCallIc>,
    reusable_activation: RefCell<Option<Box<DynFrame>>>,
    activation_stats_enabled: bool,
    #[cfg(feature = "inline-census")]
    static_resolution: crate::static_call_census::StaticCallResolution,
}

impl CallIcSite {
    fn new(
        pc: usize,
        instruction: &super::dynbytecode::DynInstr,
        activation_stats_enabled: bool,
    ) -> Self {
        let DynOp::Call {
            dst,
            callee,
            receiver,
            args,
        } = &instruction.op
        else {
            return Self {
                target: InlineCallTarget::unused(pc),
                _arguments: Box::new([]),
                entry: std::cell::OnceCell::new(),
                reusable_activation: RefCell::new(None),
                activation_stats_enabled,
                #[cfg(feature = "inline-census")]
                static_resolution: crate::static_call_census::StaticCallResolution::Computed,
            };
        };
        let arguments = args.clone().into_boxed_slice();
        let target = InlineCallTarget {
            pc,
            dst: usize::from(*dst),
            callee: usize::from(*callee),
            receiver: usize::from(*receiver),
            arguments: arguments.as_ptr(),
            argument_count: arguments.len(),
            span_start: instruction.span.start,
            identity_word: Cell::new(raw_value::RawValue::UNDEFINED.bits()),
            recipe: Cell::new(std::ptr::null()),
            environment: Cell::new(std::ptr::null()),
            reusable_activation: Cell::new(std::ptr::null_mut()),
        };
        Self {
            target,
            _arguments: arguments,
            entry: std::cell::OnceCell::new(),
            reusable_activation: RefCell::new(None),
            activation_stats_enabled,
            #[cfg(feature = "inline-census")]
            static_resolution: crate::static_call_census::StaticCallResolution::Computed,
        }
    }

    #[cfg(feature = "inline-census")]
    fn new_with_source(
        pc: usize,
        instruction: &super::dynbytecode::DynInstr,
        activation_stats_enabled: bool,
        source_id: Option<usize>,
    ) -> Self {
        let mut site = Self::new(pc, instruction, activation_stats_enabled);
        site.static_resolution =
            crate::static_call_census::resolution(source_id, instruction.span.start);
        site
    }

    pub(crate) fn matches(&self, callee: &Value) -> bool {
        self.entry.get().is_some()
            && callee.as_borrowed_raw().bits() == self.target.identity_word.get()
    }

    pub(crate) fn call<A: CallArguments + ?Sized>(
        &self,
        vm: &mut Vm,
        receiver: Value,
        arguments: &A,
    ) -> JsResult<Value> {
        let entry = self.entry.get().expect("matching call IC is populated");
        if entry.code.call_recipe.captures_frame() {
            return entry
                .code
                .call(vm, entry.environment.clone(), receiver, arguments);
        }
        #[cfg(feature = "inline-census")]
        if self.activation_stats_enabled {
            let decision = record_initial_inline_candidate(&entry.code, self.target.argument_count);
            crate::static_call_census::record_user_execution(
                self.static_resolution,
                &entry._callee_owner,
                decision,
            );
        }
        let activation = self.take_reusable_activation();
        let (activation, outcome) = entry.code.call_with_reusable_activation(
            vm,
            &entry.environment,
            receiver,
            arguments,
            activation,
        );
        self.recycle_activation(activation);
        outcome
    }

    pub(crate) fn has_loop(&self) -> bool {
        self.entry.get().is_some_and(|entry| entry.code.has_loop())
    }

    pub(crate) fn fill(&self, callee: &Value, code: Rc<DynJitCode>, environment: Env) {
        if self
            .entry
            .set(UserCallIc {
                _callee_owner: callee.clone(),
                code: code.clone(),
                environment: environment.clone(),
            })
            .is_ok()
        {
            self.target
                .identity_word
                .set(callee.as_borrowed_raw().bits());
            self.target.environment.set(Rc::as_ptr(&environment));
            // A non-null recipe is the single publication/readiness fact. All
            // dependent raw pointers and identity words must be visible first.
            self.target
                .recipe
                .set(std::ptr::from_ref(&code.call_recipe));
        }
    }

    fn take_reusable_activation(&self) -> Option<Box<DynFrame>> {
        let activation = self.reusable_activation.borrow_mut().take();
        self.target.reusable_activation.set(std::ptr::null_mut());
        if self.activation_stats_enabled {
            let counter = if activation.is_some() {
                &DIRECT_CALL_RUNTIME_STATS.activation_reuses
            } else {
                &DIRECT_CALL_RUNTIME_STATS.activation_allocations
            };
            counter.fetch_add(1, Ordering::Relaxed);
        }
        activation
    }

    fn recycle_activation(&self, mut activation: Box<DynFrame>) {
        let mut reusable = self.reusable_activation.borrow_mut();
        if reusable.is_none() {
            self.target
                .reusable_activation
                .set(std::ptr::from_mut(&mut *activation));
            *reusable = Some(activation);
        }
    }
}

struct PrototypeIcRuntimeStats {
    hits: AtomicU64,
    misses: AtomicU64,
    fills: AtomicU64,
}

static PROTOTYPE_IC_RUNTIME_STATS: PrototypeIcRuntimeStats = PrototypeIcRuntimeStats {
    hits: AtomicU64::new(0),
    misses: AtomicU64::new(0),
    fills: AtomicU64::new(0),
};

struct EffectReentryRuntimeStats {
    linked_blocks: AtomicU64,
    entries: AtomicU64,
}

struct DirectCallRuntimeStats {
    linked_calls: AtomicU64,
    attempts: AtomicU64,
    hits: AtomicU64,
    misses: AtomicU64,
    exceptions: AtomicU64,
    activation_allocations: AtomicU64,
    activation_reuses: AtomicU64,
}

static DIRECT_CALL_RUNTIME_STATS: DirectCallRuntimeStats = DirectCallRuntimeStats {
    linked_calls: AtomicU64::new(0),
    attempts: AtomicU64::new(0),
    hits: AtomicU64::new(0),
    misses: AtomicU64::new(0),
    exceptions: AtomicU64::new(0),
    activation_allocations: AtomicU64::new(0),
    activation_reuses: AtomicU64::new(0),
};

#[cfg(feature = "inline-census")]
struct InlineCensusRuntimeStats {
    eligible_calls: AtomicU64,
    captures_frame_calls: AtomicU64,
    arguments_object_calls: AtomicU64,
    arity_mismatch_calls: AtomicU64,
    nested_call_calls: AtomicU64,
    control_flow_calls: AtomicU64,
    code_size_calls: AtomicU64,
    frame_size_calls: AtomicU64,
}

#[cfg(feature = "inline-census")]
static INLINE_CENSUS_RUNTIME_STATS: InlineCensusRuntimeStats = InlineCensusRuntimeStats {
    eligible_calls: AtomicU64::new(0),
    captures_frame_calls: AtomicU64::new(0),
    arguments_object_calls: AtomicU64::new(0),
    arity_mismatch_calls: AtomicU64::new(0),
    nested_call_calls: AtomicU64::new(0),
    control_flow_calls: AtomicU64::new(0),
    code_size_calls: AtomicU64::new(0),
    frame_size_calls: AtomicU64::new(0),
};

#[cfg(feature = "inline-census")]
#[cold]
#[inline(never)]
fn record_initial_inline_candidate(
    code: &DynJitCode,
    argument_count: usize,
) -> crate::inline_plan::InitialInlineDecision {
    use crate::inline_plan::InitialInlineDecision::*;
    let decision = code.initial_inline_decision(argument_count);
    let counter = match decision {
        Eligible => &INLINE_CENSUS_RUNTIME_STATS.eligible_calls,
        CapturesFrame => &INLINE_CENSUS_RUNTIME_STATS.captures_frame_calls,
        UsesArgumentsObject => &INLINE_CENSUS_RUNTIME_STATS.arguments_object_calls,
        ArityMismatch => &INLINE_CENSUS_RUNTIME_STATS.arity_mismatch_calls,
        NestedCall => &INLINE_CENSUS_RUNTIME_STATS.nested_call_calls,
        ControlFlow => &INLINE_CENSUS_RUNTIME_STATS.control_flow_calls,
        CodeSize => &INLINE_CENSUS_RUNTIME_STATS.code_size_calls,
        FrameSize => &INLINE_CENSUS_RUNTIME_STATS.frame_size_calls,
    };
    counter.fetch_add(1, Ordering::Relaxed);
    decision
}

#[cfg(feature = "inline-census")]
fn inline_census_stats_json() -> String {
    let load = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
    let dynamic_fields = format!(
        concat!(
            ",\"inline_eligible_calls\":{},\"inline_captures_frame_calls\":{},",
            "\"inline_arguments_object_calls\":{},\"inline_arity_mismatch_calls\":{},",
            "\"inline_nested_call_calls\":{},\"inline_control_flow_calls\":{},",
            "\"inline_code_size_calls\":{},\"inline_frame_size_calls\":{}"
        ),
        load(&INLINE_CENSUS_RUNTIME_STATS.eligible_calls),
        load(&INLINE_CENSUS_RUNTIME_STATS.captures_frame_calls),
        load(&INLINE_CENSUS_RUNTIME_STATS.arguments_object_calls),
        load(&INLINE_CENSUS_RUNTIME_STATS.arity_mismatch_calls),
        load(&INLINE_CENSUS_RUNTIME_STATS.nested_call_calls),
        load(&INLINE_CENSUS_RUNTIME_STATS.control_flow_calls),
        load(&INLINE_CENSUS_RUNTIME_STATS.code_size_calls),
        load(&INLINE_CENSUS_RUNTIME_STATS.frame_size_calls),
    );
    dynamic_fields + &crate::static_call_census::stats_json_fields()
}

static EFFECT_REENTRY_RUNTIME_STATS: EffectReentryRuntimeStats = EffectReentryRuntimeStats {
    linked_blocks: AtomicU64::new(0),
    entries: AtomicU64::new(0),
};

pub fn prototype_ic_stats_enabled() -> bool {
    std::env::var_os(PROTOTYPE_IC_STATS_ENV).is_some()
}

pub fn prototype_ic_stats_json() -> String {
    let load = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
    format!(
        "{{\"hits\":{},\"misses\":{},\"fills\":{}}}",
        load(&PROTOTYPE_IC_RUNTIME_STATS.hits),
        load(&PROTOTYPE_IC_RUNTIME_STATS.misses),
        load(&PROTOTYPE_IC_RUNTIME_STATS.fills),
    )
}

pub fn effect_reentry_stats_enabled() -> bool {
    std::env::var_os(EFFECT_REENTRY_STATS_ENV).is_some()
}

pub fn effect_reentry_stats_json() -> String {
    let load = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
    format!(
        "{{\"linked_blocks\":{},\"entries\":{}}}",
        load(&EFFECT_REENTRY_RUNTIME_STATS.linked_blocks),
        load(&EFFECT_REENTRY_RUNTIME_STATS.entries),
    )
}

pub fn direct_call_stats_enabled() -> bool {
    cfg!(test) || std::env::var_os(DIRECT_CALL_STATS_ENV).is_some()
}

#[cfg(not(feature = "inline-census"))]
pub fn direct_call_stats_json() -> String {
    let load = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
    format!(
        "{{\"linked_calls\":{},\"attempts\":{},\"hits\":{},\"misses\":{},\"exceptions\":{},\"activation_allocations\":{},\"activation_reuses\":{}}}",
        load(&DIRECT_CALL_RUNTIME_STATS.linked_calls),
        load(&DIRECT_CALL_RUNTIME_STATS.attempts),
        load(&DIRECT_CALL_RUNTIME_STATS.hits),
        load(&DIRECT_CALL_RUNTIME_STATS.misses),
        load(&DIRECT_CALL_RUNTIME_STATS.exceptions),
        load(&DIRECT_CALL_RUNTIME_STATS.activation_allocations),
        load(&DIRECT_CALL_RUNTIME_STATS.activation_reuses),
    )
}

#[cfg(test)]
pub fn direct_call_hit_count() -> u64 {
    DIRECT_CALL_RUNTIME_STATS.hits.load(Ordering::Relaxed)
}

#[cfg(feature = "inline-census")]
pub fn direct_call_stats_json() -> String {
    let load = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
    format!(
        concat!(
            "{{\"linked_calls\":{},\"attempts\":{},\"hits\":{},\"misses\":{},",
            "\"exceptions\":{},\"activation_allocations\":{},\"activation_reuses\":{}",
            "{}}}"
        ),
        load(&DIRECT_CALL_RUNTIME_STATS.linked_calls),
        load(&DIRECT_CALL_RUNTIME_STATS.attempts),
        load(&DIRECT_CALL_RUNTIME_STATS.hits),
        load(&DIRECT_CALL_RUNTIME_STATS.misses),
        load(&DIRECT_CALL_RUNTIME_STATS.exceptions),
        load(&DIRECT_CALL_RUNTIME_STATS.activation_allocations),
        load(&DIRECT_CALL_RUNTIME_STATS.activation_reuses),
        inline_census_stats_json(),
    )
}

#[cfg(target_arch = "aarch64")]
const MOVE_FRAME_TO_ARG: u32 = 0xaa1403e0;
#[cfg(target_arch = "aarch64")]
const MOVE_PC_TO_ARG_BASE: u32 = 0x52800001;
#[cfg(target_arch = "aarch64")]
const LOAD_BLOCK_HELPER_AT_LITERAL: u32 = 0x58000090;
#[cfg(target_arch = "aarch64")]
const BRANCH_HELPER_RESULT: u32 = 0xd61f0000;
#[cfg(target_arch = "aarch64")]
const ALIGN_POINTER_LITERAL: u32 = a64_abi::NOP;
#[cfg(target_arch = "aarch64")]
const LOAD_SITE_AT_LITERAL: u32 = 0x58000041;
#[cfg(target_arch = "aarch64")]
const BRANCH_OVER_SITE_LITERAL: u32 = 0x14000003;
#[cfg(target_arch = "aarch64")]
const PROLOGUE_SITE_LITERAL_OFFSET: usize = 24;
#[cfg(target_arch = "aarch64")]
const CURRENT_SITE_FRAME_SLOT: u32 = 3;
#[cfg(target_arch = "aarch64")]
const CONNECTOR_FRAME_REGISTER: u32 = 20;
#[cfg(target_arch = "aarch64")]
const SITE_ARGUMENT_REGISTER: u32 = 1;
#[cfg(target_arch = "aarch64")]
const BRANCH_TARGET_REGISTER_NUMBER: u32 = 16;
#[cfg(target_arch = "aarch64")]
const LOAD_SLOW_HELPER_AT_LITERAL: u32 = 0x580000d0;
#[cfg(target_arch = "aarch64")]
const MOVE_TARGET_TO_BRANCH_REGISTER: u32 = 0xaa0003f0;
#[cfg(target_arch = "aarch64")]
const LOAD_CURRENT_SITE: u32 = a64_abi::LDR_X_UNSIGNED_BASE
    | (CURRENT_SITE_FRAME_SLOT << 10)
    | (CONNECTOR_FRAME_REGISTER << 5)
    | SITE_ARGUMENT_REGISTER;
#[cfg(target_arch = "aarch64")]
const BRANCH_TARGET_REGISTER: u32 = 0xd61f0200;
const LOAD_RETURN_TARGET: u32 = a64_abi::LDR_X_UNSIGNED_BASE
    | ((RETURN_TARGET_FRAME_WORD_OFFSET as u32) << 10)
    | (CONNECTOR_FRAME_REGISTER << 5)
    | BRANCH_TARGET_REGISTER_NUMBER;
#[cfg(target_arch = "aarch64")]
const LOAD_RESUME_TARGET: u32 = a64_abi::LDR_X_UNSIGNED_BASE
    | ((RESUME_TARGET_FRAME_WORD_OFFSET as u32) << 10)
    | (CONNECTOR_FRAME_REGISTER << 5)
    | BRANCH_TARGET_REGISTER_NUMBER;
#[cfg(target_arch = "aarch64")]
const LOAD_EXIT_KERNEL_AT_LITERAL: u32 = 0x58000050;
#[cfg(target_arch = "aarch64")]
const SLOW_PATH_LABEL: LabelId = LabelId(u32::MAX);
#[cfg(target_arch = "aarch64")]
const FUNCTION_EXIT_LABEL: LabelId = LabelId(u32::MAX - 1);
#[cfg(target_arch = "aarch64")]
const EFFECT_REENTRY_LABEL: LabelId = LabelId(u32::MAX - 2);
#[cfg(target_arch = "aarch64")]
const GUEST_ENTRY_LABEL: LabelId = LabelId(u32::MAX - 3);
#[cfg(target_arch = "aarch64")]
const DIRECT_CALL_EXCEPTION_LABEL: LabelId = LabelId(u32::MAX - 4);
#[cfg(target_arch = "aarch64")]
#[repr(C)]
#[derive(Clone, Copy)]
struct RegionArrayView {
    elements: *mut Value,
    length: usize,
}

const PROPERTY_VIEW_UNUSED_LENGTH: usize = 0;

const DIRECT_CALL_SUCCESS: usize = 0;
const DIRECT_CALL_EXCEPTION: usize = 1;
const DIRECT_CALL_MISS: usize = 2;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CallFrameLayout {
    local_count: usize,
    register_base: usize,
    snapshot_base: usize,
    snapshot_count: usize,
    slot_count: usize,
}

impl CallFrameLayout {
    fn new(local_count: usize, register_count: usize, snapshot_count: usize) -> Self {
        let register_base = local_count;
        let snapshot_base = register_base
            .checked_add(register_count)
            .expect("call frame slot count does not overflow");
        let slot_count = snapshot_base
            .checked_add(snapshot_count)
            .expect("call frame snapshot extent does not overflow");
        Self {
            local_count,
            register_base,
            snapshot_base,
            snapshot_count,
            slot_count,
        }
    }
}

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/stencil-aot/guest_frame_schema.rs"
));

define_guest_frame_abi! {
    pub(super) struct GuestFrameHeader {
        slot_value: Value,
        result_value: raw_value::RawValue,
        site: InlineSite,
        region_array: RegionArrayView,
        callback_owner: DynFrame,
        name_ic: NameIcSite,
        environment_access: EnvironmentAccess,
    }
}

impl GuestFrameHeader {
    fn result_ref(&self) -> &Value {
        unsafe { Value::from_raw_ref(&self.result) }
    }

    fn replace_result(&mut self, value: Value) {
        let previous = std::mem::replace(&mut self.result, value.into_owned_raw());
        unsafe { drop(Value::from_owned_raw(previous)) };
    }

    fn take_result(&mut self) -> Value {
        let raw = std::mem::replace(&mut self.result, raw_value::RawValue::UNDEFINED);
        unsafe { Value::from_owned_raw(raw) }
    }
}

#[repr(C)]
pub(super) struct DynFrame {
    guest: GuestFrameHeader,
    sidecar: DynFrameSidecar,
}

pub(super) struct DynFrameSidecar {
    vm: *mut Vm,
    code: *const DynCode,
    environment: Env,
    environment_chain: [*const RefCell<Environment>; INLINE_ENVIRONMENT_CHAIN_CAPACITY],
    environment_chain_len: usize,
    environment_access_chain: [*const EnvironmentAccess; INLINE_ENVIRONMENT_CHAIN_CAPACITY],
    name_snapshot_pcs: *const usize,
    name_snapshot_count: usize,
    owned_values: Vec<Value>,
    iterators: HashMap<Register, (Vec<String>, usize)>,
    handlers: Vec<usize>,
    pending_throw: Option<Value>,
    targets: *const usize,
    target_count: usize,
    name_ics: *const NameIcSite,
    name_ic_count: usize,
    property_ics: *const PropertyIcSite,
    property_ic_count: usize,
    call_ics: *const CallIcSite,
    call_ic_count: usize,
    instanceof_ics: *const InstanceOfIcSite,
    instanceof_ic_count: usize,
    region_guards: *const Option<numeric_region::GuardPlan>,
    region_guard_count: usize,
    region_array_views: Vec<RegionArrayView>,
    region_array_owners: Vec<Value>,
    region_contexts: Vec<CachedRegionContext>,
    region_stats_enabled: bool,
    region_trace_enabled: bool,
    effect_reentry_stats_enabled: bool,
    direct_call_stats_enabled: bool,
    error: Option<JsError>,
    previous_active: *mut DynFrame,
}

struct CachedRegionContext {
    pc: usize,
    context: numeric_region::ValidatedRegionContext,
}

impl std::ops::Deref for DynFrame {
    type Target = DynFrameSidecar;

    fn deref(&self) -> &Self::Target {
        &self.sidecar
    }
}

impl std::ops::DerefMut for DynFrame {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.sidecar
    }
}

impl Drop for DynFrame {
    fn drop(&mut self) {
        drop(self.guest.take_result());
    }
}

const _: () = assert!(std::mem::offset_of!(DynFrame, guest) == 0);

type DynEntry = unsafe extern "C" fn(*mut DynFrame);

struct NativePathRuntimeStats {
    direct_block_entries: AtomicU64,
    direct_opcode_entries: AtomicU64,
    numeric_region_entries: AtomicU64,
    semantic_kernel_entries: AtomicU64,
    effect_reentry_entries: AtomicU64,
    direct_call_entries: AtomicU64,
}

static NATIVE_PATH_RUNTIME_STATS: NativePathRuntimeStats = NativePathRuntimeStats {
    direct_block_entries: AtomicU64::new(0),
    direct_opcode_entries: AtomicU64::new(0),
    numeric_region_entries: AtomicU64::new(0),
    semantic_kernel_entries: AtomicU64::new(0),
    effect_reentry_entries: AtomicU64::new(0),
    direct_call_entries: AtomicU64::new(0),
};

pub fn native_path_stats_enabled() -> bool {
    std::env::var_os(NATIVE_PATH_STATS_ENV).is_some()
}

pub fn native_path_stats_json() -> String {
    let load = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
    format!(
        concat!(
            "{{\"direct_block_entries\":{},\"direct_opcode_entries\":{},",
            "\"numeric_region_entries\":{},\"semantic_kernel_entries\":{},",
            "\"effect_reentry_entries\":{},\"direct_call_entries\":{}}}"
        ),
        load(&NATIVE_PATH_RUNTIME_STATS.direct_block_entries),
        load(&NATIVE_PATH_RUNTIME_STATS.direct_opcode_entries),
        load(&NATIVE_PATH_RUNTIME_STATS.numeric_region_entries),
        load(&NATIVE_PATH_RUNTIME_STATS.semantic_kernel_entries),
        load(&NATIVE_PATH_RUNTIME_STATS.effect_reentry_entries),
        load(&NATIVE_PATH_RUNTIME_STATS.direct_call_entries),
    )
}

struct NumericRegionRuntimeStats {
    linked_regions: AtomicU64,
    linked_register_regions: AtomicU64,
    linked_register_conversions: AtomicU64,
    linked_register_forwarded_local_loads: AtomicU64,
    linked_register_alias_updates: AtomicU64,
    linked_register_maximum_location_fanout: AtomicU64,
    register_plan_attempts: AtomicU64,
    register_reject_not_single_trace: AtomicU64,
    register_reject_too_little_numeric_work: AtomicU64,
    register_reject_unsupported_boolean_use: AtomicU64,
    register_reject_missing_numeric_value: AtomicU64,
    register_reject_register_pressure: AtomicU64,
    register_reject_live_values_at_backedge: AtomicU64,
    linked_register_word_literals: AtomicU64,
    linked_register_word_loads: AtomicU64,
    linked_register_spills: AtomicU64,
    linked_block_regions: AtomicU64,
    linked_adjacent_regions: AtomicU64,
    linked_condition_supernodes: AtomicU64,
    linked_update_supernodes: AtomicU64,
    linked_proven_index_stencils: AtomicU64,
    linked_literal_one_lane_stencils: AtomicU64,
    linked_literal_two_lane_stencils: AtomicU64,
    linked_literal_three_lane_stencils: AtomicU64,
    linked_literal_four_lane_stencils: AtomicU64,
    linked_forwarded_local_loads: AtomicU64,
    linked_reused_captured_loads: AtomicU64,
    linked_reused_literals: AtomicU64,
    linked_propagated_copies: AtomicU64,
    linked_reused_pure_expressions: AtomicU64,
    linked_reused_heap_loads: AtomicU64,
    linked_eliminated_local_stores: AtomicU64,
    guard_successes: AtomicU64,
    guard_failures: AtomicU64,
    guard_cache_hits: AtomicU64,
    guard_cache_misses: AtomicU64,
    iterations: AtomicU64,
}

static NUMERIC_REGION_RUNTIME_STATS: NumericRegionRuntimeStats = NumericRegionRuntimeStats {
    linked_regions: AtomicU64::new(0),
    linked_register_regions: AtomicU64::new(0),
    linked_register_conversions: AtomicU64::new(0),
    linked_register_forwarded_local_loads: AtomicU64::new(0),
    linked_register_alias_updates: AtomicU64::new(0),
    linked_register_maximum_location_fanout: AtomicU64::new(0),
    register_plan_attempts: AtomicU64::new(0),
    register_reject_not_single_trace: AtomicU64::new(0),
    register_reject_too_little_numeric_work: AtomicU64::new(0),
    register_reject_unsupported_boolean_use: AtomicU64::new(0),
    register_reject_missing_numeric_value: AtomicU64::new(0),
    register_reject_register_pressure: AtomicU64::new(0),
    register_reject_live_values_at_backedge: AtomicU64::new(0),
    linked_register_word_literals: AtomicU64::new(0),
    linked_register_word_loads: AtomicU64::new(0),
    linked_register_spills: AtomicU64::new(0),
    linked_block_regions: AtomicU64::new(0),
    linked_adjacent_regions: AtomicU64::new(0),
    linked_condition_supernodes: AtomicU64::new(0),
    linked_update_supernodes: AtomicU64::new(0),
    linked_proven_index_stencils: AtomicU64::new(0),
    linked_literal_one_lane_stencils: AtomicU64::new(0),
    linked_literal_two_lane_stencils: AtomicU64::new(0),
    linked_literal_three_lane_stencils: AtomicU64::new(0),
    linked_literal_four_lane_stencils: AtomicU64::new(0),
    linked_forwarded_local_loads: AtomicU64::new(0),
    linked_reused_captured_loads: AtomicU64::new(0),
    linked_reused_literals: AtomicU64::new(0),
    linked_propagated_copies: AtomicU64::new(0),
    linked_reused_pure_expressions: AtomicU64::new(0),
    linked_reused_heap_loads: AtomicU64::new(0),
    linked_eliminated_local_stores: AtomicU64::new(0),
    guard_successes: AtomicU64::new(0),
    guard_failures: AtomicU64::new(0),
    guard_cache_hits: AtomicU64::new(0),
    guard_cache_misses: AtomicU64::new(0),
    iterations: AtomicU64::new(0),
};

const REGISTER_PLAN_COUNT_INCREMENT: u64 = 1;

#[cfg(target_arch = "aarch64")]
fn register_region_plan_with_stats(
    quote: &numeric_region::QuotedLoop,
    stats_enabled: bool,
) -> Option<numeric_region::RegisterRegionPlan> {
    let result = numeric_region::plan_register_region(quote);
    if stats_enabled {
        NUMERIC_REGION_RUNTIME_STATS
            .register_plan_attempts
            .fetch_add(REGISTER_PLAN_COUNT_INCREMENT, Ordering::Relaxed);
        if let Err(reject) = &result {
            use numeric_region::RegisterRegionReject as Reject;
            let counter = match reject {
                Reject::NotSingleTrace => {
                    &NUMERIC_REGION_RUNTIME_STATS.register_reject_not_single_trace
                }
                Reject::TooLittleNumericWork => {
                    &NUMERIC_REGION_RUNTIME_STATS.register_reject_too_little_numeric_work
                }
                Reject::UnsupportedBooleanUse { .. } => {
                    &NUMERIC_REGION_RUNTIME_STATS.register_reject_unsupported_boolean_use
                }
                Reject::MissingNumericValue { .. } => {
                    &NUMERIC_REGION_RUNTIME_STATS.register_reject_missing_numeric_value
                }
                Reject::RegisterPressure { .. } => {
                    &NUMERIC_REGION_RUNTIME_STATS.register_reject_register_pressure
                }
                Reject::LiveValuesAtBackedge { .. } => {
                    &NUMERIC_REGION_RUNTIME_STATS.register_reject_live_values_at_backedge
                }
            };
            counter.fetch_add(REGISTER_PLAN_COUNT_INCREMENT, Ordering::Relaxed);
        }
    }
    result.ok()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum GuardFailureKind {
    Missing,
    NotNumber,
    NotArrayIndex,
    NotDenseArray,
    NotPackedNumber,
    PropertyNotObject,
    PropertyMissing,
    PropertyInheritedWrite,
    PropertyNotNumber,
    PropertyNotDenseArray,
    PropertyNotPackedNumber,
    PropertyNotTriviallyCopyable,
}

impl GuardFailureKind {
    const ALL: [Self; 12] = [
        Self::Missing,
        Self::NotNumber,
        Self::NotArrayIndex,
        Self::NotDenseArray,
        Self::NotPackedNumber,
        Self::PropertyNotObject,
        Self::PropertyMissing,
        Self::PropertyInheritedWrite,
        Self::PropertyNotNumber,
        Self::PropertyNotDenseArray,
        Self::PropertyNotPackedNumber,
        Self::PropertyNotTriviallyCopyable,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::NotNumber => "not_number",
            Self::NotArrayIndex => "not_array_index",
            Self::NotDenseArray => "not_dense_array",
            Self::NotPackedNumber => "not_packed_number",
            Self::PropertyNotObject => "property_not_object",
            Self::PropertyMissing => "property_missing",
            Self::PropertyInheritedWrite => "property_inherited_write",
            Self::PropertyNotNumber => "property_not_number",
            Self::PropertyNotDenseArray => "property_not_dense_array",
            Self::PropertyNotPackedNumber => "property_not_packed_number",
            Self::PropertyNotTriviallyCopyable => "property_not_trivially_copyable",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum GuardSourceKind {
    Local,
    Captured,
    LiveIn,
}

impl GuardSourceKind {
    const fn name(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Captured => "captured",
            Self::LiveIn => "live_in",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum GuardRequirementKind {
    Number,
    ArrayIndex,
    DenseArray,
    PropertyRead,
    PropertyWrite,
    Unknown,
}

impl GuardRequirementKind {
    const fn name(self) -> &'static str {
        match self {
            Self::Number => "number",
            Self::ArrayIndex => "array_index",
            Self::DenseArray => "dense_array",
            Self::PropertyRead => "property_read",
            Self::PropertyWrite => "property_write",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct GuardRegionKey {
    source_id: Option<usize>,
    start: usize,
    end: usize,
    byte_offset: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct GuardFailureSiteKey {
    region: GuardRegionKey,
    source_kind: GuardSourceKind,
    source_index: Option<usize>,
    requirement_kind: GuardRequirementKind,
    failure_kind: GuardFailureKind,
    property_pc: Option<usize>,
    failure_byte_offset: Option<u32>,
}

#[derive(Default)]
struct NumericGuardDiagnostics {
    failures_by_kind: BTreeMap<GuardFailureKind, u64>,
    failures_by_site: BTreeMap<GuardFailureSiteKey, u64>,
    successes_by_region: BTreeMap<GuardRegionKey, u64>,
    failure_site_events_dropped: u64,
    success_site_events_dropped: u64,
}

static NUMERIC_GUARD_DIAGNOSTICS: LazyLock<Mutex<NumericGuardDiagnostics>> =
    LazyLock::new(|| Mutex::new(NumericGuardDiagnostics::default()));

fn guard_source_key(source: &numeric_region::GuardSource) -> (GuardSourceKind, Option<usize>) {
    match source {
        numeric_region::GuardSource::Local(slot) => (GuardSourceKind::Local, Some(*slot)),
        numeric_region::GuardSource::Captured(_) => (GuardSourceKind::Captured, None),
        numeric_region::GuardSource::LiveIn(register) => {
            (GuardSourceKind::LiveIn, Some(usize::from(*register)))
        }
    }
}

const fn guard_kind_key(kind: numeric_region::GuardKind) -> GuardRequirementKind {
    match kind {
        numeric_region::GuardKind::Number => GuardRequirementKind::Number,
        numeric_region::GuardKind::ArrayIndex => GuardRequirementKind::ArrayIndex,
        numeric_region::GuardKind::DenseArray => GuardRequirementKind::DenseArray,
    }
}

const fn property_access_key(access: numeric_region::StaticPropertyAccess) -> GuardRequirementKind {
    match access {
        numeric_region::StaticPropertyAccess::Read => GuardRequirementKind::PropertyRead,
        numeric_region::StaticPropertyAccess::Write => GuardRequirementKind::PropertyWrite,
    }
}

fn missing_requirement_key(
    plan: &numeric_region::GuardPlan,
    source: &numeric_region::GuardSource,
) -> GuardRequirementKind {
    if let Some((_, kind)) = plan
        .requirements()
        .iter()
        .find(|(candidate, _)| candidate == source)
    {
        return guard_kind_key(*kind);
    }
    plan.property_requirements()
        .iter()
        .find(|requirement| &requirement.source == source)
        .map_or(GuardRequirementKind::Unknown, |requirement| {
            property_access_key(requirement.access)
        })
}

fn guard_failure_site_key(
    code: &DynCode,
    plan: &numeric_region::GuardPlan,
    failure: &numeric_region::GuardFailure,
) -> GuardFailureSiteKey {
    use numeric_region::{GuardFailure, PropertyGuardFailure};

    let region = GuardRegionKey {
        source_id: code.source_id,
        start: plan.start,
        end: plan.end,
        byte_offset: code
            .ops
            .get(plan.start)
            .map(|instruction| instruction.span.start),
    };
    let (source, requirement_kind, failure_kind, property_pc) = match failure {
        GuardFailure::Missing(source) => (
            source,
            missing_requirement_key(plan, source),
            GuardFailureKind::Missing,
            None,
        ),
        GuardFailure::NotNumber(source) => (
            source,
            GuardRequirementKind::Number,
            GuardFailureKind::NotNumber,
            None,
        ),
        GuardFailure::NotArrayIndex(source) => (
            source,
            GuardRequirementKind::ArrayIndex,
            GuardFailureKind::NotArrayIndex,
            None,
        ),
        GuardFailure::NotDenseArray(source) => (
            source,
            GuardRequirementKind::DenseArray,
            GuardFailureKind::NotDenseArray,
            None,
        ),
        GuardFailure::NotPackedNumber(source) => (
            source,
            GuardRequirementKind::DenseArray,
            GuardFailureKind::NotPackedNumber,
            None,
        ),
        GuardFailure::Property {
            pc,
            source,
            access,
            failure,
        } => {
            let failure_kind = match failure {
                PropertyGuardFailure::NotObject => GuardFailureKind::PropertyNotObject,
                PropertyGuardFailure::MissingProperty => GuardFailureKind::PropertyMissing,
                PropertyGuardFailure::InheritedWrite => GuardFailureKind::PropertyInheritedWrite,
                PropertyGuardFailure::NotNumber => GuardFailureKind::PropertyNotNumber,
                PropertyGuardFailure::NotDenseArray => GuardFailureKind::PropertyNotDenseArray,
                PropertyGuardFailure::NotPackedNumber => GuardFailureKind::PropertyNotPackedNumber,
                PropertyGuardFailure::NotTriviallyCopyable => {
                    GuardFailureKind::PropertyNotTriviallyCopyable
                }
            };
            (
                source,
                property_access_key(*access),
                failure_kind,
                Some(*pc),
            )
        }
    };
    let (source_kind, source_index) = guard_source_key(source);
    let failure_byte_offset =
        property_pc.and_then(|pc| code.ops.get(pc).map(|instruction| instruction.span.start));
    GuardFailureSiteKey {
        region,
        source_kind,
        source_index,
        requirement_kind,
        failure_kind,
        property_pc,
        failure_byte_offset,
    }
}

fn record_numeric_guard_failure(
    code: &DynCode,
    plan: &numeric_region::GuardPlan,
    failure: &numeric_region::GuardFailure,
) {
    let site = guard_failure_site_key(code, plan, failure);
    let mut diagnostics = NUMERIC_GUARD_DIAGNOSTICS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *diagnostics
        .failures_by_kind
        .entry(site.failure_kind)
        .or_default() += 1;
    if let Some(count) = diagnostics.failures_by_site.get_mut(&site) {
        *count += 1;
    } else if diagnostics.failures_by_site.len() < MAX_REPORTED_GUARD_FAILURE_SITES {
        diagnostics.failures_by_site.insert(site, 1);
    } else {
        diagnostics.failure_site_events_dropped += 1;
    }
}

fn record_numeric_guard_success(code: &DynCode, plan: &numeric_region::GuardPlan) {
    let region = GuardRegionKey {
        source_id: code.source_id,
        start: plan.start,
        end: plan.end,
        byte_offset: code
            .ops
            .get(plan.start)
            .map(|instruction| instruction.span.start),
    };
    let mut diagnostics = NUMERIC_GUARD_DIAGNOSTICS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(count) = diagnostics.successes_by_region.get_mut(&region) {
        *count += 1;
    } else if diagnostics.successes_by_region.len() < MAX_REPORTED_GUARD_FAILURE_SITES {
        diagnostics.successes_by_region.insert(region, 1);
    } else {
        diagnostics.success_site_events_dropped += 1;
    }
}

fn write_optional_usize(out: &mut String, value: Option<usize>) {
    match value {
        Some(value) => write!(out, "{value}").expect("writing to String cannot fail"),
        None => out.push_str("null"),
    }
}

fn write_optional_u32(out: &mut String, value: Option<u32>) {
    match value {
        Some(value) => write!(out, "{value}").expect("writing to String cannot fail"),
        None => out.push_str("null"),
    }
}

fn numeric_guard_diagnostics_json() -> String {
    let diagnostics = NUMERIC_GUARD_DIAGNOSTICS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut out = String::from("{\"by_kind\":{");
    for (index, kind) in GuardFailureKind::ALL.iter().copied().enumerate() {
        if index != 0 {
            out.push(',');
        }
        write!(
            out,
            "\"{}\":{}",
            kind.name(),
            diagnostics
                .failures_by_kind
                .get(&kind)
                .copied()
                .unwrap_or(0)
        )
        .expect("writing to String cannot fail");
    }
    out.push_str("},\"by_site\":[");
    for (index, (site, count)) in diagnostics.failures_by_site.iter().enumerate() {
        if index != 0 {
            out.push(',');
        }
        out.push_str("{\"source_id\":");
        write_optional_usize(&mut out, site.region.source_id);
        write!(
            out,
            concat!(
                ",\"region_start\":{},\"region_end\":{},",
                "\"region_byte_offset\":"
            ),
            site.region.start, site.region.end,
        )
        .expect("writing to String cannot fail");
        write_optional_u32(&mut out, site.region.byte_offset);
        write!(
            out,
            ",\"source_kind\":\"{}\",\"source_index\":",
            site.source_kind.name(),
        )
        .expect("writing to String cannot fail");
        write_optional_usize(&mut out, site.source_index);
        write!(
            out,
            concat!(
                ",\"requirement_kind\":\"{}\",\"failure_kind\":\"{}\",",
                "\"property_pc\":"
            ),
            site.requirement_kind.name(),
            site.failure_kind.name(),
        )
        .expect("writing to String cannot fail");
        write_optional_usize(&mut out, site.property_pc);
        out.push_str(",\"failure_byte_offset\":");
        write_optional_u32(&mut out, site.failure_byte_offset);
        write!(out, ",\"count\":{count}}}").expect("writing to String cannot fail");
    }
    write!(
        out,
        "],\"site_events_dropped\":{},\"success_by_region\":[",
        diagnostics.failure_site_events_dropped
    )
    .expect("writing to String cannot fail");
    for (index, (region, count)) in diagnostics.successes_by_region.iter().enumerate() {
        if index != 0 {
            out.push(',');
        }
        out.push_str("{\"source_id\":");
        write_optional_usize(&mut out, region.source_id);
        write!(
            out,
            ",\"region_start\":{},\"region_end\":{},\"region_byte_offset\":",
            region.start, region.end,
        )
        .expect("writing to String cannot fail");
        write_optional_u32(&mut out, region.byte_offset);
        write!(out, ",\"count\":{count}}}").expect("writing to String cannot fail");
    }
    write!(
        out,
        "],\"success_site_events_dropped\":{}}}",
        diagnostics.success_site_events_dropped
    )
    .expect("writing to String cannot fail");
    out
}

pub fn numeric_region_stats_json() -> String {
    let load = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
    let guard_diagnostics = numeric_guard_diagnostics_json();
    format!(
        "{{\"linked_regions\":{},\"linked_register_regions\":{},\"linked_register_conversions\":{},\"linked_register_forwarded_local_loads\":{},\"linked_register_alias_updates\":{},\"linked_register_maximum_location_fanout\":{},\"register_plan_attempts\":{},\"register_reject_not_single_trace\":{},\"register_reject_too_little_numeric_work\":{},\"register_reject_unsupported_boolean_use\":{},\"register_reject_missing_numeric_value\":{},\"register_reject_register_pressure\":{},\"register_reject_live_values_at_backedge\":{},\"linked_register_word_literals\":{},\"linked_register_word_loads\":{},\"linked_register_spills\":{},\"linked_block_regions\":{},\"linked_adjacent_regions\":{},\"linked_condition_supernodes\":{},\"linked_update_supernodes\":{},\"linked_proven_index_stencils\":{},\"linked_literal_one_lane_stencils\":{},\"linked_literal_two_lane_stencils\":{},\"linked_literal_three_lane_stencils\":{},\"linked_literal_four_lane_stencils\":{},\"linked_forwarded_local_loads\":{},\"linked_reused_captured_loads\":{},\"linked_reused_literals\":{},\"linked_propagated_copies\":{},\"linked_reused_pure_expressions\":{},\"linked_reused_heap_loads\":{},\"linked_eliminated_local_stores\":{},\"guard_successes\":{},\"guard_failures\":{},\"guard_cache_hits\":{},\"guard_cache_misses\":{},\"guard_diagnostics\":{},\"iterations\":{}}}",
        load(&NUMERIC_REGION_RUNTIME_STATS.linked_regions),
        load(&NUMERIC_REGION_RUNTIME_STATS.linked_register_regions),
        load(&NUMERIC_REGION_RUNTIME_STATS.linked_register_conversions),
        load(&NUMERIC_REGION_RUNTIME_STATS.linked_register_forwarded_local_loads),
        load(&NUMERIC_REGION_RUNTIME_STATS.linked_register_alias_updates),
        load(&NUMERIC_REGION_RUNTIME_STATS.linked_register_maximum_location_fanout),
        load(&NUMERIC_REGION_RUNTIME_STATS.register_plan_attempts),
        load(&NUMERIC_REGION_RUNTIME_STATS.register_reject_not_single_trace),
        load(&NUMERIC_REGION_RUNTIME_STATS.register_reject_too_little_numeric_work),
        load(&NUMERIC_REGION_RUNTIME_STATS.register_reject_unsupported_boolean_use),
        load(&NUMERIC_REGION_RUNTIME_STATS.register_reject_missing_numeric_value),
        load(&NUMERIC_REGION_RUNTIME_STATS.register_reject_register_pressure),
        load(&NUMERIC_REGION_RUNTIME_STATS.register_reject_live_values_at_backedge),
        load(&NUMERIC_REGION_RUNTIME_STATS.linked_register_word_literals),
        load(&NUMERIC_REGION_RUNTIME_STATS.linked_register_word_loads),
        load(&NUMERIC_REGION_RUNTIME_STATS.linked_register_spills),
        load(&NUMERIC_REGION_RUNTIME_STATS.linked_block_regions),
        load(&NUMERIC_REGION_RUNTIME_STATS.linked_adjacent_regions),
        load(&NUMERIC_REGION_RUNTIME_STATS.linked_condition_supernodes),
        load(&NUMERIC_REGION_RUNTIME_STATS.linked_update_supernodes),
        load(&NUMERIC_REGION_RUNTIME_STATS.linked_proven_index_stencils),
        load(&NUMERIC_REGION_RUNTIME_STATS.linked_literal_one_lane_stencils),
        load(&NUMERIC_REGION_RUNTIME_STATS.linked_literal_two_lane_stencils),
        load(&NUMERIC_REGION_RUNTIME_STATS.linked_literal_three_lane_stencils),
        load(&NUMERIC_REGION_RUNTIME_STATS.linked_literal_four_lane_stencils),
        load(&NUMERIC_REGION_RUNTIME_STATS.linked_forwarded_local_loads),
        load(&NUMERIC_REGION_RUNTIME_STATS.linked_reused_captured_loads),
        load(&NUMERIC_REGION_RUNTIME_STATS.linked_reused_literals),
        load(&NUMERIC_REGION_RUNTIME_STATS.linked_propagated_copies),
        load(&NUMERIC_REGION_RUNTIME_STATS.linked_reused_pure_expressions),
        load(&NUMERIC_REGION_RUNTIME_STATS.linked_reused_heap_loads),
        load(&NUMERIC_REGION_RUNTIME_STATS.linked_eliminated_local_stores),
        load(&NUMERIC_REGION_RUNTIME_STATS.guard_successes),
        load(&NUMERIC_REGION_RUNTIME_STATS.guard_failures),
        load(&NUMERIC_REGION_RUNTIME_STATS.guard_cache_hits),
        load(&NUMERIC_REGION_RUNTIME_STATS.guard_cache_misses),
        guard_diagnostics,
        load(&NUMERIC_REGION_RUNTIME_STATS.iterations),
    )
}

pub struct DynJitCode {
    call_recipe: FunctionCallRecipe,
    constructor_shape: Option<ShapeRef>,
    code: Rc<DynCode>,
    targets: Vec<usize>,
    name_ics: Box<[NameIcSite]>,
    property_ics: Box<[PropertyIcSite]>,
    _instanceof_ics: Box<[InstanceOfIcSite]>,
    call_ics: Box<[CallIcSite]>,
    region_guards: Box<[Option<numeric_region::GuardPlan>]>,
    sites: Box<[InlineSite]>,
    name_snapshot_pcs: Box<[usize]>,
    binding_layout: Rc<HashMap<String, usize>>,
    _parameter_slots: Box<[usize]>,
    code_bytes: usize,
    direct_blocks: usize,
    direct_opcodes: usize,
    has_loop: bool,
    region_stats_enabled: bool,
    region_trace_enabled: bool,
    effect_reentry_stats_enabled: bool,
    direct_call_stats_enabled: bool,
    _exit_kernel: Rc<Kernel<Connector, ReturnState>>,
    _effect_reentry_kernel: Option<Rc<Kernel<Connector, Connector>>>,
}

impl DynJitCode {
    pub fn build(code: DynCode, arena: &mut CodeArena, instrumented_kernels: bool) -> Option<Self> {
        #[cfg(target_arch = "aarch64")]
        {
            build_aarch64(code, arena, instrumented_kernels)
        }
        #[cfg(not(target_arch = "aarch64"))]
        {
            let _ = (code, arena, instrumented_kernels);
            None
        }
    }

    pub fn call<A: CallArguments + ?Sized>(
        &self,
        vm: &mut Vm,
        outer: Env,
        this: Value,
        args: &A,
    ) -> JsResult<Value> {
        if !self.call_recipe.captures_frame() {
            return self.call_with_stack_frame(vm, outer, this, args);
        }
        let environment = if self.call_recipe.captures_frame() {
            Environment::with_layout(Some(outer), self.binding_layout.clone())
        } else {
            vm.acquire_environment(Some(outer), self.binding_layout.clone())
        };
        let arguments = self
            .call_recipe
            .uses_arguments()
            .then(|| arguments_value(vm, args));
        {
            let mut frame = environment.borrow_mut();
            frame.declare(THIS_BINDING_NAME, this);
            if let Some(arguments) = arguments {
                frame.declare(ARGUMENTS_BINDING_NAME, arguments);
            }
            for (index, name) in self.code.params.iter().enumerate() {
                frame.declare(name, args.value(index).unwrap_or(Value::Undefined));
            }
        }
        self.install_hoisted(vm, &environment)?;
        let result = self.run(vm, environment.clone());
        if !self.call_recipe.captures_frame() {
            vm.release_environment(environment);
        }
        result
    }

    fn call_with_stack_frame<A: CallArguments + ?Sized>(
        &self,
        vm: &mut Vm,
        outer: Env,
        this: Value,
        args: &A,
    ) -> JsResult<Value> {
        let layout = self.call_recipe.layout;
        let mut values = vm.acquire_registers(layout.slot_count);
        self.initialize_bindings(vm, &mut values[..layout.local_count], this, args);
        let local_values = values.as_mut_ptr();
        self.run_with_owned_values(
            vm,
            outer,
            local_values,
            layout.local_count,
            values,
            layout.register_base,
            layout.snapshot_base,
            layout.snapshot_count,
        )
    }

    fn call_with_reusable_activation<A: CallArguments + ?Sized>(
        &self,
        vm: &mut Vm,
        outer: &Env,
        this: Value,
        args: &A,
        activation: Option<Box<DynFrame>>,
    ) -> (Box<DynFrame>, JsResult<Value>) {
        let layout = self.call_recipe.layout;
        let mut frame = activation.unwrap_or_else(|| {
            let mut values = vm.acquire_registers(layout.slot_count);
            let local_values = values.as_mut_ptr();
            Box::new(self.make_frame(
                vm,
                outer.clone(),
                local_values,
                layout.local_count,
                values,
                layout.register_base,
                layout.snapshot_base,
                layout.snapshot_count,
            ))
        });
        debug_assert_eq!(frame.owned_values.len(), layout.slot_count);
        debug_assert!(Rc::ptr_eq(&frame.environment, outer));
        debug_assert_eq!(frame.code, Rc::as_ptr(&self.code));
        self.initialize_bindings(
            vm,
            &mut frame.owned_values[..layout.local_count],
            this,
            args,
        );
        enter_dyn_frame(&mut frame);
        unsafe { (self.call_recipe.entry)(&mut *frame) };
        let outcome = complete_reusable_dyn_frame(&mut frame);
        (frame, outcome)
    }

    fn initialize_bindings<A: CallArguments + ?Sized>(
        &self,
        vm: &Vm,
        values: &mut [Value],
        this: Value,
        args: &A,
    ) {
        Value::overwrite(&mut values[self.call_recipe.this_slot], this);
        if self.call_recipe.uses_arguments() {
            Value::overwrite(
                &mut values[self.call_recipe.arguments_slot],
                arguments_value(vm, args),
            );
        }
        for (index, slot) in self
            .call_recipe
            .parameter_slots()
            .iter()
            .copied()
            .enumerate()
        {
            let value = args.value(index).unwrap_or(Value::Undefined);
            Value::overwrite(&mut values[slot], value);
        }
    }

    pub fn call_script(&self, vm: &mut Vm, environment: Env) -> JsResult<Value> {
        self.install_hoisted(vm, &environment)?;
        self.run(vm, environment)
    }

    fn install_hoisted(&self, vm: &mut Vm, environment: &Env) -> JsResult<()> {
        for (name, function) in &self.code.hoisted {
            let node = unsafe { &**function };
            let closure = vm.make_user(node, environment.clone());
            if let Some(function) = closure.as_function_ref() {
                vm.compile_user_function(function, node)?;
            }
            environment.borrow_mut().declare(name, closure);
        }
        Ok(())
    }

    fn run(&self, vm: &mut Vm, environment: Env) -> JsResult<Value> {
        let (local_values, local_count) = {
            let mut environment = environment.borrow_mut();
            (environment.values.as_mut_ptr(), environment.values.len())
        };
        self.run_with_locals(vm, environment, local_values, local_count)
    }

    fn run_with_locals(
        &self,
        vm: &mut Vm,
        environment: Env,
        local_values: *mut Value,
        local_count: usize,
    ) -> JsResult<Value> {
        let layout = CallFrameLayout::new(0, self.code.registers, self.name_snapshot_slot_count());
        let owned_values = vm.acquire_registers(layout.slot_count);
        self.run_with_owned_values(
            vm,
            environment,
            local_values,
            local_count,
            owned_values,
            layout.register_base,
            layout.snapshot_base,
            layout.snapshot_count,
        )
    }

    fn name_snapshot_slot_count(&self) -> usize {
        self.name_snapshot_pcs.len()
    }

    fn run_with_owned_values(
        &self,
        vm: &mut Vm,
        environment: Env,
        local_values: *mut Value,
        local_count: usize,
        owned_values: Vec<Value>,
        register_base: usize,
        snapshot_base: usize,
        snapshot_count: usize,
    ) -> JsResult<Value> {
        let mut frame = self.make_frame(
            vm,
            environment,
            local_values,
            local_count,
            owned_values,
            register_base,
            snapshot_base,
            snapshot_count,
        );
        enter_dyn_frame(&mut frame);
        unsafe { (self.call_recipe.entry)(&mut frame) };
        complete_dyn_frame(&mut frame)
    }

    #[allow(clippy::too_many_arguments)]
    fn make_frame(
        &self,
        vm: &mut Vm,
        environment: Env,
        local_values: *mut Value,
        local_count: usize,
        mut owned_values: Vec<Value>,
        register_base: usize,
        snapshot_base: usize,
        snapshot_count: usize,
    ) -> DynFrame {
        debug_assert!(
            register_base
                .checked_add(self.code.registers)
                .is_some_and(|required| required <= owned_values.len()),
            "owned frame covers every virtual register"
        );
        debug_assert!(
            snapshot_base
                .checked_add(snapshot_count)
                .is_some_and(|required| required <= owned_values.len()),
            "owned frame covers every name snapshot slot"
        );
        let register_values = unsafe { owned_values.as_mut_ptr().add(register_base) };
        let environment_chain = inline_environment_chain(&environment);
        let name_snapshots = if snapshot_count == 0 {
            std::ptr::null()
        } else {
            unsafe { owned_values.as_ptr().add(snapshot_base) }
        };
        DynFrame {
            guest: GuestFrameHeader {
                register_values,
                local_values,
                local_count,
                current_site: self.sites.as_ptr(),
                sites: self.sites.as_ptr(),
                name_snapshots,
                result: raw_value::RawValue::UNDEFINED,
                region_arrays: std::ptr::null(),
                region_guard: validate_numeric_region,
                region_iterations: 0,
                direct_call: execute_direct_call,
                return_target: self._exit_kernel.entry,
                resume_target: 0,
                instanceof_condition: execute_cached_instanceof_condition,
                environment_access_chain: std::ptr::null(),
                environment_access_chain_len: environment_chain.len,
                name_ics: self.name_ics.as_ptr(),
            },
            sidecar: DynFrameSidecar {
                vm,
                code: Rc::as_ptr(&self.code),
                environment,
                environment_chain: environment_chain.environments,
                environment_chain_len: environment_chain.len,
                environment_access_chain: environment_chain.accesses,
                name_snapshot_pcs: self.name_snapshot_pcs.as_ptr(),
                name_snapshot_count: self.name_snapshot_pcs.len(),
                owned_values,
                iterators: HashMap::new(),
                handlers: Vec::new(),
                pending_throw: None,
                targets: self.targets.as_ptr(),
                target_count: self.targets.len(),
                name_ics: self.name_ics.as_ptr(),
                name_ic_count: self.name_ics.len(),
                property_ics: self.property_ics.as_ptr(),
                property_ic_count: self.property_ics.len(),
                call_ics: self.call_ics.as_ptr(),
                call_ic_count: self.call_ics.len(),
                instanceof_ics: self._instanceof_ics.as_ptr(),
                instanceof_ic_count: self._instanceof_ics.len(),
                region_guards: self.region_guards.as_ptr(),
                region_guard_count: self.region_guards.len(),
                region_array_views: Vec::new(),
                region_array_owners: Vec::new(),
                region_contexts: Vec::new(),
                region_stats_enabled: self.region_stats_enabled,
                region_trace_enabled: self.region_trace_enabled,
                effect_reentry_stats_enabled: self.effect_reentry_stats_enabled,
                direct_call_stats_enabled: self.direct_call_stats_enabled,
                error: None,
                previous_active: std::ptr::null_mut(),
            },
        }
    }

    pub fn has_loop(&self) -> bool {
        self.has_loop
    }

    #[cfg(feature = "inline-census")]
    fn initial_inline_decision(
        &self,
        argument_count: usize,
    ) -> crate::inline_plan::InitialInlineDecision {
        crate::inline_plan::classify_initial_inline_candidate(
            crate::call_recipe::initial_inline_facts(
                &self.code,
                self.call_recipe.captures_frame(),
                self.call_recipe.uses_arguments(),
                self.call_recipe.parameter_count,
                argument_count,
                self.code_bytes,
                self.call_recipe.layout.slot_count,
            ),
        )
    }

    pub fn code_bytes(&self) -> usize {
        self.code_bytes
    }

    pub fn direct_selection(&self) -> (usize, usize) {
        (self.direct_blocks, self.direct_opcodes)
    }

    pub(super) fn constructor_shape(&self) -> Option<ShapeRef> {
        self.constructor_shape
    }

    pub(super) fn trace_object_roots(&self, tracer: &mut ObjectTracer<'_>) {
        for site in &self.call_ics {
            let Some(entry) = site.entry.get() else {
                continue;
            };
            tracer.value(&entry._callee_owner);
            tracer.environment(entry.environment.clone());
            if let Some(frame) = site.reusable_activation.borrow().as_deref() {
                frame.region_contexts.iter().for_each(|cached| {
                    cached.context.visit_roots(|value| tracer.value(value));
                });
            }
        }
    }

    pub(super) fn invalidate_unmarked_object_identities(&self) {
        self.property_ics
            .iter()
            .for_each(PropertyIcSite::invalidate_unmarked_object_identities);
        self._instanceof_ics
            .iter()
            .for_each(InstanceOfIcSite::invalidate);
    }

    #[cfg(test)]
    pub fn numeric_region_count(&self) -> usize {
        self.region_guards
            .iter()
            .filter(|guard| guard.is_some())
            .count()
    }
}

pub(super) unsafe fn trace_active_object_roots(
    mut frame: *mut DynFrame,
    tracer: &mut ObjectTracer<'_>,
) {
    while let Some(active) = unsafe { frame.as_ref() } {
        active
            .owned_values
            .iter()
            .for_each(|value| tracer.value(value));
        tracer.value(active.guest.result_ref());
        if let Some(value) = &active.pending_throw {
            tracer.value(value);
        }
        if let Some(JsError::Throw(value)) = &active.error {
            tracer.value(value);
        }
        active
            .region_array_owners
            .iter()
            .for_each(|value| tracer.value(value));
        active
            .region_contexts
            .iter()
            .for_each(|cached| cached.context.visit_roots(|value| tracer.value(value)));
        tracer.environment(active.environment.clone());
        frame = active.previous_active;
    }
}

pub(super) unsafe fn invalidate_active_unmarked_object_identities(mut frame: *mut DynFrame) {
    while let Some(active) = unsafe { frame.as_ref() } {
        let property_ics =
            unsafe { std::slice::from_raw_parts(active.property_ics, active.property_ic_count) };
        property_ics
            .iter()
            .for_each(PropertyIcSite::invalidate_unmarked_object_identities);
        let instanceof_ics = unsafe {
            std::slice::from_raw_parts(active.instanceof_ics, active.instanceof_ic_count)
        };
        instanceof_ics.iter().for_each(InstanceOfIcSite::invalidate);
        frame = active.previous_active;
    }
}

fn enter_dyn_frame(frame: &mut DynFrame) {
    frame.guest.environment_access_chain = frame.environment_access_chain.as_ptr();
    refresh_name_snapshots(frame);
    let frame_pointer = std::ptr::from_mut(frame);
    let vm = unsafe { &mut *frame.vm };
    frame.previous_active = vm.active_dyn_frame.replace(frame_pointer);
    let code = unsafe { &*frame.code };
    if let Some(source_id) = code.source_id {
        vm.source_ids.push(source_id);
    }
}

fn leave_dyn_frame(frame: &mut DynFrame) {
    let frame_pointer = std::ptr::from_mut(frame);
    let vm = unsafe { &mut *frame.vm };
    assert_eq!(
        vm.active_dyn_frame.get(),
        frame_pointer,
        "dynamic frames must leave in stack order"
    );
    vm.active_dyn_frame.set(frame.previous_active);
    frame.previous_active = std::ptr::null_mut();
}

fn complete_dyn_frame(frame: &mut DynFrame) -> JsResult<Value> {
    if frame.region_stats_enabled {
        NUMERIC_REGION_RUNTIME_STATS
            .iterations
            .fetch_add(frame.guest.region_iterations, Ordering::Relaxed);
    }
    let code = unsafe { &*frame.code };
    let vm = unsafe { &mut *frame.vm };
    if code.source_id.is_some() {
        vm.source_ids.pop();
    }
    vm.collect_objects_if_requested();
    let outcome = frame
        .error
        .take()
        .map_or_else(|| Ok(frame.guest.take_result()), Err);
    leave_dyn_frame(frame);
    let vm = unsafe { &mut *frame.vm };
    vm.release_registers(std::mem::take(&mut frame.owned_values));
    outcome
}

fn complete_reusable_dyn_frame(frame: &mut DynFrame) -> JsResult<Value> {
    if frame.region_stats_enabled {
        NUMERIC_REGION_RUNTIME_STATS
            .iterations
            .fetch_add(frame.guest.region_iterations, Ordering::Relaxed);
    }
    let code = unsafe { &*frame.code };
    let vm = unsafe { &mut *frame.vm };
    if code.source_id.is_some() {
        vm.source_ids.pop();
    }
    vm.collect_objects_if_requested();
    let outcome = frame
        .error
        .take()
        .map_or_else(|| Ok(frame.guest.take_result()), Err);
    leave_dyn_frame(frame);
    reset_reusable_dyn_frame(frame);
    outcome
}

fn reset_reusable_dyn_frame(frame: &mut DynFrame) {
    reset_value_slots(&mut frame.owned_values);
    frame.iterators.clear();
    frame.handlers.clear();
    frame.pending_throw = None;
    frame.region_array_views.clear();
    frame.region_array_owners.clear();
    frame.guest.region_arrays = std::ptr::null();
    frame.guest.region_iterations = 0;
    frame.guest.resume_target = 0;
    debug_assert!(frame.error.is_none());
    debug_assert!(frame.previous_active.is_null());
}

struct PreparedDirectCall {
    child: Box<DynFrame>,
    entry: DynEntry,
    target: *const InlineCallTarget,
    call_ic: *const CallIcSite,
}

unsafe extern "C" fn execute_direct_call(parent: *mut DynFrame, site: *const InlineSite) -> usize {
    let parent = unsafe { &mut *parent };
    if parent.direct_call_stats_enabled {
        DIRECT_CALL_RUNTIME_STATS
            .attempts
            .fetch_add(1, Ordering::Relaxed);
    }
    let Some(mut prepared) = (unsafe { prepare_direct_child(parent, site) }) else {
        return DIRECT_CALL_MISS;
    };
    if parent.direct_call_stats_enabled {
        DIRECT_CALL_RUNTIME_STATS
            .hits
            .fetch_add(1, Ordering::Relaxed);
    }
    let child = &mut *prepared.child;
    enter_dyn_frame(child);
    unsafe { (prepared.entry)(child) };
    let outcome = complete_reusable_dyn_frame(child);
    let continuation = finish_direct_call(parent, prepared.target, outcome);
    unsafe { &*prepared.call_ic }.recycle_activation(prepared.child);
    continuation
}

unsafe fn prepare_direct_child(
    parent: &mut DynFrame,
    site: *const InlineSite,
) -> Option<PreparedDirectCall> {
    let pc = unsafe { (*site).pc };
    if pc >= parent.call_ic_count {
        return direct_call_miss(parent);
    }
    let call_ic = unsafe { &*parent.call_ics.add(pc) };
    let target = &call_ic.target;
    if target.pc != pc || target.callee == NO_CALL_REGISTER {
        return direct_call_miss(parent);
    }
    let callee_value = unsafe { &*parent.guest.register_values.add(target.callee) };
    if !call_ic.matches(callee_value) {
        return direct_call_miss(parent);
    }
    let cached = call_ic.entry.get().expect("matching call IC is populated");
    let recipe = unsafe { target.recipe.get().as_ref() }
        .expect("a matching call target has a function recipe");
    if recipe.captures_frame() {
        return direct_call_miss(parent);
    }

    let receiver = unsafe { (&*parent.guest.register_values.add(target.receiver)).clone() };
    let code = unsafe { &*parent.code };
    let caller_registers =
        unsafe { std::slice::from_raw_parts(parent.guest.register_values, code.registers) };
    let arguments = RegisterArguments {
        values: caller_registers,
        registers: target.arguments(),
    };
    let callee = cached.code.as_ref();
    let layout = recipe.layout;
    let vm = unsafe { &mut *parent.vm };
    let mut child = call_ic.take_reusable_activation().unwrap_or_else(|| {
        let mut values = vm.acquire_registers(layout.slot_count);
        let local_values = values.as_mut_ptr();
        Box::new(callee.make_frame(
            vm,
            cached.environment.clone(),
            local_values,
            layout.local_count,
            values,
            layout.register_base,
            layout.snapshot_base,
            layout.snapshot_count,
        ))
    });
    let frame = &mut *child;
    debug_assert_eq!(frame.owned_values.len(), layout.slot_count);
    debug_assert!(Rc::ptr_eq(&frame.environment, &cached.environment));
    callee.initialize_bindings(
        vm,
        &mut frame.owned_values[..layout.local_count],
        receiver,
        &arguments,
    );
    Some(PreparedDirectCall {
        child,
        entry: recipe.entry,
        target,
        call_ic,
    })
}

fn direct_call_miss(parent: &DynFrame) -> Option<PreparedDirectCall> {
    if parent.direct_call_stats_enabled {
        DIRECT_CALL_RUNTIME_STATS
            .misses
            .fetch_add(1, Ordering::Relaxed);
    }
    None
}

fn finish_direct_call(
    parent: &mut DynFrame,
    target: *const InlineCallTarget,
    outcome: JsResult<Value>,
) -> usize {
    refresh_name_snapshots(parent);
    let target = unsafe { &*target };
    let pc = target.pc;
    let code = unsafe { &*parent.code };
    match outcome {
        Ok(value) => {
            put(parent, &(target.dst as Register), value);
            DIRECT_CALL_SUCCESS
        }
        Err(JsError::Throw(value)) => {
            if parent.direct_call_stats_enabled {
                DIRECT_CALL_RUNTIME_STATS
                    .exceptions
                    .fetch_add(1, Ordering::Relaxed);
            }
            let target_pc = exceptional_pc(parent, JsError::Throw(value), code.ops.len());
            parent.guest.resume_target = select_single_target(parent, target_pc);
            DIRECT_CALL_EXCEPTION
        }
        Err(error) => {
            if parent.direct_call_stats_enabled {
                DIRECT_CALL_RUNTIME_STATS
                    .exceptions
                    .fetch_add(1, Ordering::Relaxed);
            }
            let location = code
                .source_id
                .and_then(|source_id| {
                    unsafe { &*parent.vm }
                        .coverage
                        .location(source_id, target.span_start)
                })
                .map_or_else(
                    || format!("bytecode pc {pc}"),
                    |(path, line)| format!("{}:{}", path.display(), line),
                );
            let error = JsError::Message(format!("{error} in Call stencil at {location}"));
            let target_pc = exceptional_pc(parent, error, code.ops.len());
            parent.guest.resume_target = select_single_target(parent, target_pc);
            DIRECT_CALL_EXCEPTION
        }
    }
}

fn arguments_value<A: CallArguments + ?Sized>(vm: &Vm, args: &A) -> Value {
    let value = vm.object_value(Object::array(None, args.materialize()));
    vm.set_prop(&value, "\0wrapper", Value::string_value("Arguments"));
    value
}

unsafe extern "C" fn validate_numeric_region(frame: *mut DynFrame, pc: usize) -> usize {
    let frame = unsafe { &mut *frame };
    if pc >= frame.region_guard_count {
        return target(frame, pc);
    }
    let Some(plan) = (unsafe { &*frame.region_guards.add(pc) }) else {
        return target(frame, pc);
    };
    let region_end = plan.end;
    let context = reusable_region_context(frame, pc, plan);
    let context = match context {
        Ok(context) => context,
        Err(failure) => {
            if frame.region_stats_enabled {
                NUMERIC_REGION_RUNTIME_STATS
                    .guard_failures
                    .fetch_add(1, Ordering::Relaxed);
                record_numeric_guard_failure(unsafe { &*frame.code }, plan, &failure);
            }
            if frame.region_trace_enabled {
                eprintln!("NUMERIC_REGION:guard-failed start={pc} reason={failure:?}");
            }
            return unsafe { execute_region_fallback(frame, pc, region_end) };
        }
    };
    clear_region_clobbers(frame, plan.clobbers());
    clear_region_local_clobbers(frame, plan.local_clobbers());
    if frame.region_stats_enabled {
        NUMERIC_REGION_RUNTIME_STATS
            .guard_successes
            .fetch_add(1, Ordering::Relaxed);
        record_numeric_guard_success(unsafe { &*frame.code }, plan);
    }
    if frame.region_trace_enabled {
        eprintln!("NUMERIC_REGION:guard-succeeded start={pc}");
    }
    install_region_context(frame, &context);
    install_dense_registers(frame, plan);
    if plan.can_reuse_context() {
        frame
            .region_contexts
            .push(CachedRegionContext { pc, context });
    }
    0
}

fn reusable_region_context(
    frame: &mut DynFrame,
    pc: usize,
    plan: &numeric_region::GuardPlan,
) -> Result<numeric_region::ValidatedRegionContext, numeric_region::GuardFailure> {
    if !plan.can_reuse_context() {
        return plan.validate(|source| resolve_guard_source(frame, source));
    }
    let cached = frame
        .region_contexts
        .iter()
        .position(|cached| cached.pc == pc)
        .map(|index| frame.region_contexts.swap_remove(index).context);
    if let Some(context) = cached
        && plan.revalidate(&context, |source| resolve_guard_source(frame, source))
    {
        if frame.region_stats_enabled {
            NUMERIC_REGION_RUNTIME_STATS
                .guard_cache_hits
                .fetch_add(1, Ordering::Relaxed);
        }
        return Ok(context);
    }
    if frame.region_stats_enabled {
        NUMERIC_REGION_RUNTIME_STATS
            .guard_cache_misses
            .fetch_add(1, Ordering::Relaxed);
    }
    plan.validate(|source| resolve_guard_source(frame, source))
}

fn clear_region_local_clobbers(frame: &mut DynFrame, clobbers: &[usize]) {
    for slot in clobbers.iter().copied() {
        Value::overwrite(
            unsafe { &mut *frame.guest.local_values.add(slot) },
            Value::Undefined,
        );
    }
}

fn clear_region_clobbers(frame: &mut DynFrame, clobbers: &[Register]) {
    for register in clobbers.iter().copied() {
        Value::overwrite(
            unsafe { &mut *frame.guest.register_values.add(register as usize) },
            Value::Undefined,
        );
    }
}

unsafe fn execute_region_fallback(frame: &mut DynFrame, start: usize, end: usize) -> usize {
    let mut pc = start;
    loop {
        let continuation = unsafe {
            dyn_block_step_impl::<true, false, false>(
                frame,
                pc as u32,
                UNBOUNDED_SEMANTIC_RANGE_END,
            )
        };
        pc = unsafe { (*frame.guest.current_site).pc };
        if !(start..end).contains(&pc) {
            return continuation;
        }
    }
}

fn resolve_guard_source(frame: &DynFrame, source: &numeric_region::GuardSource) -> Option<Value> {
    match source {
        numeric_region::GuardSource::Local(slot) => (*slot < frame.guest.local_count)
            .then(|| unsafe { (&*frame.guest.local_values.add(*slot)).clone() }),
        numeric_region::GuardSource::Captured(name) => Environment::get(&frame.environment, name),
        numeric_region::GuardSource::LiveIn(register) => {
            Some(unsafe { (&*frame.guest.register_values.add(*register as usize)).clone() })
        }
    }
}

fn install_region_context(frame: &mut DynFrame, context: &numeric_region::ValidatedRegionContext) {
    frame.region_array_views.clear();
    frame.region_array_owners.clear();
    for binding in &context.arrays {
        let guarded = &context.unique_arrays[binding.array];
        frame.region_array_views.push(RegionArrayView {
            elements: guarded.elements,
            length: guarded.length,
        });
        frame.region_array_owners.push(guarded.receiver().clone());
    }
    // Property analysis accepts only frame/environment roots that cannot be
    // overwritten inside the region, so those roots own each borrowed slot.
    context.properties.visit(|property| {
        frame.region_array_views.push(RegionArrayView {
            elements: property.view.value,
            length: PROPERTY_VIEW_UNUSED_LENGTH,
        });
    });
    frame.guest.region_arrays = frame.region_array_views.as_ptr();
}

fn install_dense_registers(frame: &mut DynFrame, plan: &numeric_region::GuardPlan) {
    for binding in plan.dense_register_bindings() {
        let value = frame.region_array_owners[binding.array].clone();
        Value::overwrite(
            unsafe { &mut *frame.guest.register_values.add(binding.register as usize) },
            value,
        );
    }
}

unsafe extern "C" fn dyn_block_step(frame: *mut DynFrame, pc: u32) -> usize {
    unsafe { dyn_block_step_impl::<false, true, false>(frame, pc, UNBOUNDED_SEMANTIC_RANGE_END) }
}

unsafe extern "C" fn dyn_snapshot_block_step(frame: *mut DynFrame, pc: u32) -> usize {
    unsafe { dyn_block_step_impl::<true, true, false>(frame, pc, UNBOUNDED_SEMANTIC_RANGE_END) }
}

unsafe extern "C" fn dyn_fast_block_step(frame: *mut DynFrame, pc: u32) -> usize {
    unsafe { dyn_block_step_impl::<false, false, false>(frame, pc, UNBOUNDED_SEMANTIC_RANGE_END) }
}

unsafe extern "C" fn dyn_fast_snapshot_block_step(frame: *mut DynFrame, pc: u32) -> usize {
    unsafe { dyn_block_step_impl::<true, false, false>(frame, pc, UNBOUNDED_SEMANTIC_RANGE_END) }
}

unsafe extern "C" fn dyn_residual_block_step(frame: *mut DynFrame, pc: u32) -> usize {
    unsafe { dyn_block_step_impl::<false, false, true>(frame, pc, UNBOUNDED_SEMANTIC_RANGE_END) }
}

unsafe extern "C" fn dyn_residual_snapshot_block_step(frame: *mut DynFrame, pc: u32) -> usize {
    unsafe { dyn_block_step_impl::<true, false, true>(frame, pc, UNBOUNDED_SEMANTIC_RANGE_END) }
}

unsafe extern "C" fn dyn_fast_range_step(frame: *mut DynFrame, pc: u32) -> usize {
    let end = unsafe { (*(*frame).guest.sites.add(pc as usize)).run_end };
    unsafe { dyn_block_step_impl::<false, false, false>(frame, pc, end) }
}

unsafe extern "C" fn dyn_fast_snapshot_range_step(frame: *mut DynFrame, pc: u32) -> usize {
    let end = unsafe { (*(*frame).guest.sites.add(pc as usize)).run_end };
    unsafe { dyn_block_step_impl::<true, false, false>(frame, pc, end) }
}

unsafe fn dyn_block_step_impl<
    const REFRESH_NAME_SNAPSHOTS: bool,
    const INSTRUMENTED: bool,
    const RESIDUAL_STATS: bool,
>(
    frame: *mut DynFrame,
    pc: u32,
    range_end: usize,
) -> usize {
    let frame = unsafe { &mut *frame };
    let code = unsafe { &*frame.code };
    let mut pc = pc as usize;
    let vm = unsafe { &mut *frame.vm };
    if RESIDUAL_STATS || (INSTRUMENTED && vm.block_stats_enabled) {
        let block_pc = pc;
        vm.jit_stats
            .record_block_entry(frame.code as usize, block_pc, || {
                let mut names = Vec::new();
                for instruction in &code.ops[block_pc..] {
                    names.push(block_profile_op_name(&instruction.op));
                    if closes_block(&instruction.op) {
                        break;
                    }
                }
                names.join(",")
            });
    }
    loop {
        if pc == range_end {
            return select_single_target(frame, pc);
        }
        let Some(instruction) = code.ops.get(pc) else {
            frame.error = Some(JsError::Message(format!("invalid dynamic stencil pc {pc}")));
            return select_single_target(frame, code.ops.len());
        };
        let vm = unsafe { &mut *frame.vm };
        if INSTRUMENTED && !vm.consume_instruction_budget() {
            frame.error = Some(JsError::Message(format!(
                "instruction budget exhausted at dynamic stencil pc {pc}"
            )));
            return select_single_target(frame, code.ops.len());
        }
        if INSTRUMENTED && vm.jit_stats_enabled {
            vm.jit_stats.record_kernel_entry(instruction.op.opcode());
        }
        if INSTRUMENTED
            && vm.coverage.is_enabled()
            && let Some(source_id) = code.source_id
        {
            vm.coverage.mark(
                source_id,
                instruction.span.start,
                op_name(&instruction.op),
                ExecutionMode::KernelExit,
            );
        }
        let closes_block = closes_block(&instruction.op);
        let execution = if REFRESH_NAME_SNAPSHOTS {
            execute_with_name_snapshot_refresh(frame, &instruction.op, pc + 1)
        } else {
            execute(frame, &instruction.op, pc + 1)
        };
        match execution {
            Ok(next) if closes_block => return select_single_target(frame, next),
            Ok(next) => pc = next,
            Err(JsError::Throw(value)) => {
                let target_pc = exceptional_pc(frame, JsError::Throw(value), code.ops.len());
                return select_single_target(frame, target_pc);
            }
            Err(error) => {
                let operation = op_name(&instruction.op);
                let location = code
                    .source_id
                    .and_then(|source_id| vm.coverage.location(source_id, instruction.span.start))
                    .map_or_else(
                        || format!("{:?}", instruction.span),
                        |(path, line)| format!("{}:{}", path.display(), line),
                    );
                let error =
                    JsError::Message(format!("{error} in {operation} stencil at {location}"));
                let target_pc = exceptional_pc(frame, error, code.ops.len());
                return select_single_target(frame, target_pc);
            }
        }
    }
}

unsafe extern "C" fn dyn_single_step(frame: *mut DynFrame, site: *const InlineSite) -> usize {
    let frame = unsafe { &mut *frame };
    if frame.effect_reentry_stats_enabled {
        EFFECT_REENTRY_RUNTIME_STATS
            .entries
            .fetch_add(1, Ordering::Relaxed);
    }
    let code = unsafe { &*frame.code };
    let pc = unsafe { (*site).pc as usize };
    let Some(instruction) = code.ops.get(pc) else {
        frame.error = Some(JsError::Message(format!("invalid dynamic stencil pc {pc}")));
        return select_single_target(frame, code.ops.len());
    };
    let vm = unsafe { &mut *frame.vm };
    if vm.jit_stats_enabled {
        vm.jit_stats.record_kernel_entry(instruction.op.opcode());
    }
    if vm.coverage.is_enabled()
        && let Some(source_id) = code.source_id
    {
        vm.coverage.mark(
            source_id,
            instruction.span.start,
            op_name(&instruction.op),
            ExecutionMode::KernelExit,
        );
    }
    let execution = if frame.name_snapshot_count == 0 {
        execute(frame, &instruction.op, pc + NEXT_INSTRUCTION_DISTANCE)
    } else {
        execute_with_name_snapshot_refresh(frame, &instruction.op, pc + NEXT_INSTRUCTION_DISTANCE)
    };
    let next = match execution {
        Ok(next) => next,
        Err(JsError::Throw(value)) => {
            let target_pc = exceptional_pc(frame, JsError::Throw(value), code.ops.len());
            return select_single_target(frame, target_pc);
        }
        Err(error) => {
            let operation = op_name(&instruction.op);
            let location = code
                .source_id
                .and_then(|source_id| vm.coverage.location(source_id, instruction.span.start))
                .map_or_else(
                    || format!("{:?}", instruction.span),
                    |(path, line)| format!("{}:{}", path.display(), line),
                );
            let error = JsError::Message(format!("{error} in {operation} stencil at {location}"));
            let target_pc = exceptional_pc(frame, error, code.ops.len());
            return select_single_target(frame, target_pc);
        }
    };
    select_single_target(frame, next)
}

unsafe extern "C" fn dyn_block_from_site(frame: *mut DynFrame, site: *const InlineSite) -> usize {
    unsafe { dyn_block_from_site_impl::<true, false>(frame, site) }
}

unsafe extern "C" fn dyn_fast_block_from_site(
    frame: *mut DynFrame,
    site: *const InlineSite,
) -> usize {
    unsafe { dyn_block_from_site_impl::<false, false>(frame, site) }
}

unsafe extern "C" fn dyn_residual_block_from_site(
    frame: *mut DynFrame,
    site: *const InlineSite,
) -> usize {
    unsafe { dyn_block_from_site_impl::<false, true>(frame, site) }
}

unsafe fn dyn_block_from_site_impl<const INSTRUMENTED: bool, const RESIDUAL_STATS: bool>(
    frame: *mut DynFrame,
    site: *const InlineSite,
) -> usize {
    let pc = unsafe { (*site).pc as u32 };
    if unsafe { (*frame).sidecar.name_snapshot_count } == 0 {
        unsafe {
            dyn_block_step_impl::<false, INSTRUMENTED, RESIDUAL_STATS>(
                frame,
                pc,
                UNBOUNDED_SEMANTIC_RANGE_END,
            )
        }
    } else {
        unsafe {
            dyn_block_step_impl::<true, INSTRUMENTED, RESIDUAL_STATS>(
                frame,
                pc,
                UNBOUNDED_SEMANTIC_RANGE_END,
            )
        }
    }
}

fn exceptional_pc(frame: &mut DynFrame, error: JsError, exit_pc: usize) -> usize {
    if let JsError::Throw(ref value) = error
        && let Some(handler) = frame.handlers.pop()
    {
        frame.pending_throw = Some(value.clone());
        return handler;
    }
    frame.error = Some(error);
    exit_pc
}

fn select_single_target(frame: &mut DynFrame, pc: usize) -> usize {
    let bounded_pc = pc.min(unsafe { &*frame.code }.ops.len());
    frame.guest.current_site = unsafe { frame.guest.sites.add(bounded_pc) };
    target(frame, pc)
}

fn closes_block(op: &DynOp) -> bool {
    matches!(
        op,
        DynOp::Jump { .. }
            | DynOp::JumpIfFalse { .. }
            | DynOp::ForInNext { .. }
            | DynOp::Throw { .. }
            | DynOp::Rethrow
            | DynOp::Return { .. }
    )
}

fn target(frame: &DynFrame, pc: usize) -> usize {
    if pc >= frame.target_count {
        return unsafe { *frame.targets.add(frame.target_count - 1) };
    }
    unsafe { *frame.targets.add(pc) }
}

#[inline(always)]
fn execute(frame: &mut DynFrame, op: &DynOp, next: usize) -> JsResult<usize> {
    let name_ic = || {
        let pc = next.checked_sub(NEXT_INSTRUCTION_DISTANCE)?;
        (pc < frame.name_ic_count).then(|| unsafe { &*frame.name_ics.add(pc) })
    };
    let property_ic_count = frame.property_ic_count;
    let property_ics = frame.property_ics;
    let property_ic = move || {
        let pc = next.checked_sub(NEXT_INSTRUCTION_DISTANCE)?;
        (pc < property_ic_count).then(|| unsafe { &*property_ics.add(pc) })
    };
    let instanceof_ic = || {
        let pc = next.checked_sub(NEXT_INSTRUCTION_DISTANCE)?;
        let site = unsafe { &*frame.guest.sites.add(pc) };
        let cache = site.literal as usize as *const InstanceOfIcSite;
        (!cache.is_null()).then(|| unsafe { &*cache })
    };
    match op {
        DynOp::LoadLiteral { dst, value } => {
            let value = match value {
                Literal::Undefined => Value::Undefined,
                Literal::Null => Value::Null,
                Literal::Bool(value) => Value::Bool(*value),
                Literal::Number(value) => Value::Number(*value),
                Literal::String(value) => allocate_string_literal(value),
            };
            put(frame, dst, value);
        }
        DynOp::LoadName { dst, name } => {
            let value = name_ic().and_then(|cache| {
                Environment::get_cached(
                    &frame.environment,
                    &frame.environment_chain[..frame.environment_chain_len],
                    name,
                    cache,
                )
            });
            put(frame, dst, value.unwrap_or(Value::Undefined));
        }
        DynOp::LoadLocal { dst, slot } => {
            let value = get_local(frame, *slot);
            put(frame, dst, value);
        }
        DynOp::DeclareName { name, src } => {
            let value = get(frame, src);
            frame.environment.borrow_mut().declare(&name, value.clone());
            unsafe { &*frame.vm }.sync_global_binding(&frame.environment, &name, value);
        }
        DynOp::DeclareLocal { slot, src } => {
            let value = get(frame, src);
            set_local(frame, *slot, value);
        }
        DynOp::StoreName { name, src } => {
            let value = get(frame, src);
            if let Some(cache) = name_ic() {
                Environment::set_cached(
                    &frame.environment,
                    &frame.environment_chain[..frame.environment_chain_len],
                    name,
                    value,
                    cache,
                );
            } else {
                Environment::set(&frame.environment, name, value);
            }
        }
        DynOp::StoreLocal { slot, src } => {
            let value = get(frame, src);
            set_local(frame, *slot, value);
        }
        DynOp::LoadThis { dst } => {
            let value = name_ic().and_then(|cache| {
                Environment::get_cached(
                    &frame.environment,
                    &frame.environment_chain[..frame.environment_chain_len],
                    THIS_BINDING_NAME,
                    cache,
                )
            });
            // Script code executes directly in the global environment, whose
            // `this` binding is materialized as `globalThis`.  Keep the
            // fallback script-scoped: function frames must retain their own
            // `this` value (including `undefined` for strict calls).
            let value = value.or_else(|| {
                (unsafe { &*frame.code }.is_script)
                    .then(|| Environment::get(&vm(frame).global, "globalThis"))
                    .flatten()
            });
            put(frame, dst, value.unwrap_or(Value::Undefined));
        }
        DynOp::Move { dst, src } => put(frame, dst, get(frame, src)),
        DynOp::NewArray { dst } => {
            let value = vm(frame).array();
            put(frame, dst, value);
        }
        DynOp::NewArrayFromRegisters { dst, elements } => {
            let values = elements
                .iter()
                .map(|source| source.map_or(Value::Undefined, |source| get(frame, &source)))
                .collect();
            let value = vm(frame).array_from_values(values);
            put(frame, dst, value);
        }
        DynOp::NewObject { dst } => {
            let pc = next - NEXT_INSTRUCTION_DISTANCE;
            let shape_bits = unsafe { (*frame.guest.sites.add(pc)).literal };
            let value = if shape_bits == NO_ALLOCATION_SHAPE_POINTER {
                vm(frame).object(None)
            } else {
                let shape_pointer = shape_bits as usize as *const Shape;
                vm(frame).object_with_shape(None, ShapeRef(shape_pointer))
            };
            put(frame, dst, value);
        }
        DynOp::NewObjectFromRegisters { dst, values, .. } => {
            let pc = next - NEXT_INSTRUCTION_DISTANCE;
            let shape_bits = unsafe { (*frame.guest.sites.add(pc)).literal };
            debug_assert_ne!(shape_bits, NO_ALLOCATION_SHAPE_POINTER);
            let shape_pointer = shape_bits as usize as *const Shape;
            let values = values.iter().map(|source| get(frame, source)).collect();
            let value = vm(frame).object_with_shape_values(None, ShapeRef(shape_pointer), values);
            put(frame, dst, value);
        }
        DynOp::MakeClosure { dst, function } => {
            let environment = frame.environment.clone();
            let node = unsafe { &**function };
            let closure = vm(frame).make_user(node, environment);
            if let Some(function) = closure.as_function_ref() {
                vm(frame).compile_user_function(function, node)?;
            }
            put(frame, dst, closure);
        }
        DynOp::MakeArrow { dst, function } => {
            let environment = frame.environment.clone();
            let node = unsafe { &**function };
            let closure = vm(frame).make_arrow(node, environment);
            if let Some(function) = closure.as_function_ref() {
                vm(frame).compile_arrow_function(function, node)?;
            }
            put(frame, dst, closure);
        }
        DynOp::Unary { dst, src, kind } => {
            let operand = get_ref(frame, *src).clone();
            let value = unary(vm(frame), *kind, &operand)?;
            put(frame, dst, value);
        }
        DynOp::Binary {
            dst,
            left,
            right,
            kind,
        } => {
            let left = get_ref(frame, *left).clone();
            let right = get_ref(frame, *right).clone();
            let value = match (left.as_number(), right.as_number()) {
                (Some(left), Some(right)) => exec_numeric_op(*kind, left, right),
                _ => super::binary_with_vm(vm(frame), *kind, &left, &right)?,
            };
            put(frame, dst, value);
        }
        DynOp::Update {
            dst,
            src,
            increment,
        } => {
            let old = get_ref(frame, *src).clone();
            let primitive = if old.is_object() || old.is_function() {
                super::to_primitive_for_binary(vm(frame), &old, false)?
            } else {
                old
            };
            let value = if is_bigint_marker(&primitive) {
                let bigint = parse_bigint_text(primitive.as_string().map_or("", String::as_str))
                    .map_err(|_| {
                        JsError::Throw(super::type_error(vm(frame), "invalid BigInt value"))
                    })?;
                let next = if *increment {
                    bigint_marker(bigint + num_bigint::BigInt::from(1u8))
                } else {
                    bigint_marker(bigint - num_bigint::BigInt::from(1u8))
                };
                next
            } else {
                let number = primitive.number();
                Value::Number(if *increment {
                    number + 1.0
                } else {
                    number - 1.0
                })
            };
            put(frame, dst, value);
        }
        DynOp::InstanceOf { dst, left, right } => {
            let value = if let Some(cache) = instanceof_ic() {
                Value::Bool(instance_of_cached(
                    get_ref(frame, *left),
                    get_ref(frame, *right),
                    cache,
                    unsafe { (*frame.vm).prototype_epoch.get() },
                ))
            } else {
                Value::Bool(instance_of(get_ref(frame, *left), get_ref(frame, *right)))
            };
            put(frame, dst, value);
        }
        DynOp::In { dst, left, right } => {
            let value = Value::Bool(in_prop(get_ref(frame, *left), get_ref(frame, *right)));
            put(frame, dst, value);
        }
        DynOp::GetStatic { dst, object, key } => {
            let value = {
                let object = get_ref(frame, *object).clone();
                if object.is_null() || object.is_undefined() {
                    return Err(JsError::Message(format!(
                        "cannot read property {key} of {}",
                        object.display()
                    )));
                }
                if unsafe { &*frame.vm }.restricted_function_property(&object, key) {
                    return Err(JsError::Throw(super::type_error(
                        unsafe { &mut *frame.vm },
                        "'caller' and 'arguments' are unavailable on this function",
                    )));
                }
                let cache = property_ic();
                let vm = unsafe { &mut *frame.vm };
                let cached = vm
                    .find_accessor(&object, key)
                    .is_none()
                    .then(|| cache.and_then(|cache| get_static_cached(&object, key, cache)))
                    .flatten();
                cached.unwrap_or(vm.get_prop_with_accessors(&object, key)?)
            };
            put(frame, dst, value);
        }
        DynOp::GetComputed { dst, object, key } => {
            let value = {
                let object = get_ref(frame, *object).clone();
                if object.is_null() || object.is_undefined() {
                    return Err(JsError::Message(format!(
                        "cannot read computed property of {}",
                        object.display()
                    )));
                }
                let key = get_ref(frame, *key).clone();
                let vm = unsafe { &mut *frame.vm };
                let key_string = vm.to_property_key(key)?;
                if unsafe { &*frame.vm }.restricted_function_property(&object, &key_string) {
                    return Err(JsError::Throw(super::type_error(
                        unsafe { &mut *frame.vm },
                        "'caller' and 'arguments' are unavailable on this function",
                    )));
                }
                let cache = property_ic();
                let cached = vm
                    .find_accessor(&object, &key_string)
                    .is_none()
                    .then(|| cache.and_then(|cache| get_static_cached(&object, &key_string, cache)))
                    .flatten();
                cached.unwrap_or(vm.get_prop_with_accessors(&object, &key_string)?)
            };
            put(frame, dst, value);
        }
        DynOp::SetStatic { object, key, src } => {
            let value = get(frame, src);
            let object = get_ref(frame, *object).clone();
            if object.is_null() || object.is_undefined() {
                return Err(JsError::Message(format!(
                    "cannot write property {key} of {}",
                    object.display()
                )));
            }
            if super::accessor_key(key).is_some() {
                unsafe { &*frame.vm }.install_accessor_slot(&object, key, value);
                return Ok(next);
            }
            if key == "stack" && unsafe { &*frame.vm }.has_error_stack_accessor(&object) {
                super::native_error_stack_set(unsafe { &mut *frame.vm }, object.clone(), &[value])?;
                return Ok(next);
            }
            let cache = property_ic();
            let vm = unsafe { &mut *frame.vm };
            if vm.find_accessor(&object, key).is_none() {
                if let Some(cache) = cache {
                    if set_static_cached(&object, key, value.clone(), cache).is_ok() {
                        return Ok(next);
                    }
                }
            }
            vm.set_prop_with_accessors(&object, key, value)?;
        }
        DynOp::SetComputed {
            object,
            key,
            src,
            accessor,
        } => {
            let value = get(frame, src);
            let object = get_ref(frame, *object).clone();
            if object.is_null() || object.is_undefined() {
                return Err(JsError::Message(format!(
                    "cannot write computed property of {}",
                    object.display()
                )));
            }
            let key = get_ref(frame, *key).clone();
            let vm = unsafe { &mut *frame.vm };
            let key_string = vm.to_property_key(key)?;
            if let Some(accessor) = accessor {
                let slot = super::accessor_slot(
                    match accessor {
                        AccessorKind::Getter => "get",
                        AccessorKind::Setter => "set",
                    },
                    &key_string,
                );
                unsafe { &*frame.vm }.install_accessor_slot(&object, &slot, value);
                return Ok(next);
            }
            if key_string == "stack" && unsafe { &*frame.vm }.has_error_stack_accessor(&object) {
                super::native_error_stack_set(unsafe { &mut *frame.vm }, object, &[value])?;
                return Ok(next);
            }
            vm.set_prop_with_accessors(&object, &key_string, value)?;
        }
        DynOp::DeleteStatic {
            dst,
            object,
            key,
            strict,
        } => {
            let object = get(frame, object);
            let deleted = unsafe { &*frame.vm }.delete_prop(&object, &key);
            if *strict && !deleted {
                return Err(JsError::Throw(super::type_error(
                    unsafe { &mut *frame.vm },
                    "property is not configurable",
                )));
            }
            put(frame, dst, Value::Bool(deleted));
        }
        DynOp::DeleteComputed {
            dst,
            object,
            key,
            strict,
        } => {
            let object = get(frame, object);
            let key = get(frame, key).string();
            let deleted = unsafe { &*frame.vm }.delete_prop(&object, &key);
            if *strict && !deleted {
                return Err(JsError::Throw(super::type_error(
                    unsafe { &mut *frame.vm },
                    "property is not configurable",
                )));
            }
            put(frame, dst, Value::Bool(deleted));
        }
        DynOp::Call {
            dst,
            callee,
            receiver,
            args,
        } => {
            let register_count = unsafe { &*frame.code }.registers;
            let register_values =
                unsafe { std::slice::from_raw_parts(frame.guest.register_values, register_count) };
            let arguments = RegisterArguments {
                values: register_values,
                registers: args,
            };
            let receiver = get(frame, receiver);
            let callee = unsafe { &*frame.guest.register_values.add(*callee as usize) };
            let pc = next - NEXT_INSTRUCTION_DISTANCE;
            let cache = (pc < frame.call_ic_count).then(|| unsafe { &*frame.call_ics.add(pc) });
            let result = unsafe { &mut *frame.vm }
                .call_arguments_with_ic(callee, receiver, &arguments, cache)?;
            put(frame, dst, result);
        }
        DynOp::Construct { dst, callee, args } => {
            let pc = next - NEXT_INSTRUCTION_DISTANCE;
            let cache = (pc < frame.call_ic_count).then(|| unsafe { &*frame.call_ics.add(pc) });
            construct(frame, *dst, *callee, args, cache)?;
        }
        DynOp::RegExp {
            dst,
            global,
            kernel,
        } => {
            let regex = kernel.instantiate()?;
            put(
                frame,
                dst,
                Value::RegExp(Rc::new(RefCell::new(RegExpValue::new(regex, *global)))),
            );
        }
        DynOp::Jump { target } => return Ok(*target),
        DynOp::JumpIfFalse { test, target } if !get_ref(frame, *test).truthy() => {
            return Ok(*target);
        }
        DynOp::JumpIfFalse { .. } => {}
        DynOp::ForInInit { iterator, object } => {
            let keys = enumerable_keys(&get(frame, object));
            frame.iterators.insert(*iterator, (keys, 0));
        }
        DynOp::ForInNext {
            iterator,
            dst,
            done,
        } => {
            let Some((keys, index)) = frame.iterators.get_mut(&iterator) else {
                return Err(JsError::Message(
                    "missing for-in iterator stencil state".into(),
                ));
            };
            let Some(key) = keys.get(*index).cloned() else {
                return Ok(*done);
            };
            *index += 1;
            put(frame, dst, Value::String(Rc::new(key.into())));
        }
        DynOp::PushHandler { target } => frame.handlers.push(*target),
        DynOp::PopHandler => {
            frame.handlers.pop();
        }
        DynOp::Catch { binding } => {
            let value = frame.pending_throw.take().unwrap_or(Value::Undefined);
            match binding {
                Some(CatchBinding::Name(name)) => {
                    frame.environment.borrow_mut().declare(name, value);
                }
                Some(CatchBinding::Local(slot)) => set_local(frame, *slot, value),
                None => {}
            }
        }
        DynOp::Throw { src } => return Err(JsError::Throw(get(frame, src))),
        DynOp::Rethrow => {
            let value = frame.pending_throw.take().unwrap_or(Value::Undefined);
            return Err(JsError::Throw(value));
        }
        DynOp::Return { src } => {
            let result = src.map_or(Value::Undefined, |register| get(frame, register));
            frame.guest.replace_result(result);
            return Ok(unsafe { &*frame.code }.ops.len());
        }
    }
    Ok(next)
}

#[inline(always)]
fn execute_with_name_snapshot_refresh(
    frame: &mut DynFrame,
    op: &DynOp,
    next: usize,
) -> JsResult<usize> {
    let refreshes_names = matches!(
        op,
        DynOp::DeclareName { .. }
            | DynOp::StoreName { .. }
            | DynOp::Call { .. }
            | DynOp::Construct { .. }
            | DynOp::Catch {
                binding: Some(CatchBinding::Name(_))
            }
    );
    let outcome = execute(frame, op, next);
    if refreshes_names {
        refresh_name_snapshots(frame);
    }
    outcome
}

fn refresh_name_snapshots(frame: &mut DynFrame) {
    if frame.name_snapshot_count == 0 {
        return;
    }
    let code = unsafe { &*frame.code };
    for index in 0..frame.name_snapshot_count {
        let pc = unsafe { *frame.name_snapshot_pcs.add(index) };
        let DynOp::LoadName { name, .. } = &code.ops[pc].op else {
            unreachable!("name snapshot pc identifies LoadName")
        };
        let slot = unsafe { &mut *(frame.guest.name_snapshots as *mut Value).add(index) };
        let value = if pc < frame.name_ic_count {
            let cache = unsafe { &*frame.name_ics.add(pc) };
            Environment::get_cached(
                &frame.environment,
                &frame.environment_chain[..frame.environment_chain_len],
                name,
                cache,
            )
        } else {
            Environment::get(&frame.environment, name)
        };
        Value::overwrite(slot, value.unwrap_or(Value::Undefined));
    }
}

fn get<R: std::borrow::Borrow<Register>>(frame: &DynFrame, register: R) -> Value {
    get_ref(frame, *register.borrow()).clone()
}

fn get_ref(frame: &DynFrame, register: Register) -> &Value {
    unsafe { &*frame.guest.register_values.add(register as usize) }
}

struct RegisterArguments<'a> {
    values: &'a [Value],
    registers: &'a [Register],
}

impl CallArguments for RegisterArguments<'_> {
    fn value(&self, index: usize) -> Option<Value> {
        let register = *self.registers.get(index)? as usize;
        self.values.get(register).cloned()
    }

    fn len(&self) -> usize {
        self.registers.len()
    }
}

#[inline(always)]
fn put<R: std::borrow::Borrow<Register>>(frame: &mut DynFrame, register: R, value: Value) {
    Value::overwrite(
        unsafe { &mut *frame.guest.register_values.add(*register.borrow() as usize) },
        value,
    );
}

fn get_local(frame: &DynFrame, slot: usize) -> Value {
    if slot >= frame.guest.local_count {
        return Value::Undefined;
    }
    unsafe { (&*frame.guest.local_values.add(slot)).clone() }
}

fn set_local(frame: &mut DynFrame, slot: usize, value: Value) {
    if slot < frame.guest.local_count {
        unsafe { Value::overwrite(&mut *frame.guest.local_values.add(slot), value) };
    }
}

fn vm(frame: &mut DynFrame) -> &mut Vm {
    unsafe { &mut *frame.vm }
}

#[inline(never)]
fn allocate_string_literal(value: &str) -> Value {
    Value::String(Rc::new(value.to_owned().into()))
}

fn object_prototype_storage_word(object: &ObjectCell) -> usize {
    let object = object.borrow();
    unsafe { std::ptr::from_ref(&object.prototype).cast::<usize>().read() }
}

fn instance_of_cached(
    value: &Value,
    constructor: &Value,
    cache: &InstanceOfIcSite,
    epoch: u64,
) -> bool {
    if let Some(result) = instance_of_cache_hit(value, constructor, cache, epoch) {
        return result;
    }
    let result = instance_of(value, constructor);
    let (Some(object), Some(_)) = (value.as_object_ref(), constructor.as_function_ref()) else {
        return result;
    };
    cache.entry.set(InstanceOfIc {
        constructor_bits: constructor.0.bits(),
        receiver_prototype_word: object_prototype_storage_word(object),
        epoch,
        answer: if result {
            INSTANCEOF_CACHE_TRUE
        } else {
            INSTANCEOF_CACHE_FALSE
        },
    });
    result
}

fn instance_of_cache_hit(
    value: &Value,
    constructor: &Value,
    cache: &InstanceOfIcSite,
    epoch: u64,
) -> Option<bool> {
    let object = value.as_object_ref()?;
    constructor.as_function_ref()?;
    let entry = cache.entry.get();
    if entry.constructor_bits != constructor.0.bits()
        || entry.receiver_prototype_word != object_prototype_storage_word(object)
        || entry.epoch != epoch
    {
        return None;
    }
    match entry.answer {
        INSTANCEOF_CACHE_FALSE => Some(false),
        INSTANCEOF_CACHE_TRUE => Some(true),
        _ => None,
    }
}

unsafe extern "C" fn execute_cached_instanceof_condition(
    frame: *mut DynFrame,
    site: *const InlineSite,
) -> usize {
    let frame = unsafe { &mut *frame };
    let load = unsafe { &*site.add(INSTANCEOF_LOAD_LOCAL_PC_OFFSET) };
    if load.left >= frame.guest.local_count {
        return INSTANCEOF_CACHE_FALSE;
    }
    let receiver = unsafe { &*frame.guest.local_values.add(load.left) };
    let name_pc = unsafe { (*site.add(INSTANCEOF_LOAD_NAME_PC_OFFSET)).pc };
    let code = unsafe { &*frame.code };
    let DynOp::LoadName { name, .. } = &code.ops[name_pc].op else {
        return INSTANCEOF_CACHE_FALSE;
    };
    let constructor = if name_pc < frame.name_ic_count {
        let cache = unsafe { &*frame.name_ics.add(name_pc) };
        Environment::get_cached(
            &frame.environment,
            &frame.environment_chain[..frame.environment_chain_len],
            name,
            cache,
        )
    } else {
        Environment::get(&frame.environment, name)
    }
    .unwrap_or(Value::Undefined);
    let operation = unsafe { &*site.add(INSTANCEOF_OPERATION_PC_OFFSET) };
    let cache = operation.literal as usize as *const InstanceOfIcSite;
    if cache.is_null() {
        return INSTANCEOF_CACHE_FALSE;
    }
    let result = instance_of_cached(receiver, &constructor, unsafe { &*cache }, unsafe {
        (*frame.vm).prototype_epoch.get()
    });
    if result {
        INSTANCEOF_CACHE_TRUE
    } else {
        INSTANCEOF_CACHE_FALSE
    }
}

fn get_static_cached(value: &Value, key: &str, cache: &PropertyIcSite) -> Option<Value> {
    let Some(root) = value.as_object_ref() else {
        return None;
    };
    if let Some(location) = cache.own.get().populated() {
        let object = root.borrow();
        if object.props.shape.0 == location.receiver_shape
            && let Some((_, cached_value)) = object.props.get_index(location.slot)
        {
            return Some(cached_value.clone());
        }
    }
    if let Some(value) = cache.inherited_value(root) {
        return Some(value);
    }

    let receiver_shape;
    let mut chain = Vec::new();
    let mut object = {
        let root = root.borrow();
        receiver_shape = root.props.shape.0;
        if let Some((slot, _, value)) = root.props.get_full(key) {
            cache.own.set(PropertyIc {
                receiver_shape,
                slot,
            });
            cache.clear_inherited();
            return Some(value.clone());
        }
        root.prototype.clone()
    };
    while let Some(current) = object {
        let current_ref = current.borrow();
        chain.push(PrototypeGuard {
            identity: current.as_ptr(),
            shape: current_ref.props.shape.0,
        });
        if let Some((slot, _, value)) = current_ref.props.get_full(key) {
            let chain = chain.into_boxed_slice();
            cache.record_inherited(PrototypePropertyIc {
                receiver_shape,
                chain,
                slot,
            });
            return Some(value.clone());
        }
        object = current_ref.prototype.clone();
    }
    cache.clear_inherited();
    None
}

fn set_static_cached(
    value: &Value,
    key: &str,
    new_value: Value,
    cache: &PropertyIcSite,
) -> Result<(), Value> {
    let Some(object) = value.as_object_ref() else {
        return Err(new_value);
    };
    if object.borrow().array.is_some()
        && (key == ARRAY_LENGTH_PROPERTY_NAME || key.parse::<usize>().is_ok())
    {
        return Err(new_value);
    }
    let mut object = object.borrow_mut();
    let receiver_shape = object.props.shape.0;
    if let Some(location) = cache.own.get().populated()
        && location.receiver_shape == receiver_shape
        && let Some((_, cached_value)) = object.props.get_index_mut(location.slot)
    {
        Value::overwrite(cached_value, new_value);
        cache.clear_inherited();
        return Ok(());
    }
    if let Some((slot, _, old_value)) = object.props.get_full_mut(key) {
        Value::overwrite(old_value, new_value);
        cache.own.set(PropertyIc {
            receiver_shape,
            slot,
        });
        cache.clear_inherited();
        return Ok(());
    }
    Err(new_value)
}

fn unary(vm: &mut super::Vm, kind: UnaryKind, value: &Value) -> JsResult<Value> {
    let marker = if super::is_bigint_marker(value) {
        value.as_string().cloned()
    } else {
        value.as_object_ref().and_then(|object| {
            object
                .borrow()
                .props
                .get("\0primitive")
                .filter(|primitive| super::is_bigint_marker(primitive))
                .and_then(Value::as_string)
                .cloned()
        })
    };
    if let Some(marker) = marker {
        let bigint =
            super::parse_bigint_text(&marker).unwrap_or_else(|_| num_bigint::BigInt::from(0));
        return Ok(match kind {
            UnaryKind::Negate => super::bigint_marker(-bigint),
            UnaryKind::BitNot => super::bigint_marker(!bigint),
            UnaryKind::Typeof => Value::string_value("bigint"),
            UnaryKind::Plus => {
                return Err(JsError::Throw(super::type_error(
                    vm,
                    "cannot convert a BigInt value to a number",
                )));
            }
            UnaryKind::Not => Value::Bool(false),
            UnaryKind::Void => Value::Undefined,
        });
    }
    Ok(match kind {
        UnaryKind::Plus => Value::Number(value.number()),
        UnaryKind::Negate => Value::Number(-value.number()),
        UnaryKind::Not => Value::Bool(!value.truthy()),
        UnaryKind::BitNot => Value::Number(!i32_js(value.number()) as f64),
        UnaryKind::Typeof => Value::String(Rc::new(
            if value.is_undefined() {
                "undefined"
            } else if value.is_function() {
                "function"
            } else if value.as_bool().is_some() {
                "boolean"
            } else if value.as_number().is_some() {
                "number"
            } else if value.is_string() {
                "string"
            } else {
                "object"
            }
            .into(),
        )),
        UnaryKind::Void => Value::Undefined,
    })
}

fn construct(
    frame: &mut DynFrame,
    dst: Register,
    callee: Register,
    args: &[Register],
    call_ic: Option<&CallIcSite>,
) -> JsResult<()> {
    let callee = get(frame, callee);
    if !super::constructable(&callee) {
        return Err(JsError::Throw(super::type_error(
            unsafe { &mut *frame.vm },
            "not a constructor",
        )));
    }
    let native = callee.as_function_ref().is_some_and(|function| {
        matches!(
            function.kind,
            FunctionKind::Builtin(_) | FunctionKind::Native(_)
        )
    });
    let wrapper_constructor = callee.as_function_ref().is_some_and(|function| {
        matches!(
            function.kind,
            FunctionKind::Builtin(
                BuiltinId::BooleanConstructor
                    | BuiltinId::NumberConstructor
                    | BuiltinId::StringConstructor
            )
        )
    });
    // Native constructor semantics allocate their own result. The generic receiver
    // would be immediately discarded, so keep the construction kernel allocation-free.
    let constructor_shape = callee.as_function_ref().and_then(|function| {
        function
            .dyn_jit
            .borrow()
            .as_ref()
            .and_then(|code| code.constructor_shape())
    });
    let object = if wrapper_constructor {
        let prototype = callee
            .as_function_ref()
            .map(|function| Some(function.prototype))
            .unwrap_or(None);
        vm(frame).object(prototype)
    } else if native {
        let native_prototype = callee.as_function_ref().and_then(|function| {
            matches!(
                function.kind,
                FunctionKind::Builtin(BuiltinId::DateConstructor)
            )
            .then(|| function.prototype.clone())
        });
        let error_prototype = callee.as_function_ref().and_then(|function| {
            matches!(
                function.kind,
                FunctionKind::Builtin(
                    BuiltinId::ErrorConstructor
                        | BuiltinId::TypeErrorConstructor
                        | BuiltinId::RangeErrorConstructor
                        | BuiltinId::URIErrorConstructor
                        | BuiltinId::SyntaxErrorConstructor
                        | BuiltinId::ReferenceErrorConstructor
                        | BuiltinId::EvalErrorConstructor
                        | BuiltinId::AggregateErrorConstructor
                )
            )
            .then(|| function.prototype.clone())
        });
        error_prototype
            .or(native_prototype)
            .map_or(Value::Undefined, |prototype| {
                vm(frame).object(Some(prototype))
            })
    } else if let Some(function) = callee.as_function_ref() {
        let prototype = Some(function.prototype);
        match constructor_shape {
            Some(shape) => vm(frame).object_with_shape(prototype, shape),
            None => vm(frame).object(prototype),
        }
    } else {
        vm(frame).object(None)
    };
    let register_count = unsafe { &*frame.code }.registers;
    let register_values =
        unsafe { std::slice::from_raw_parts(frame.guest.register_values, register_count) };
    let arguments = RegisterArguments {
        values: register_values,
        registers: args,
    };
    let result = unsafe { &mut *frame.vm }.call_arguments_with_ic(
        &callee,
        object.clone(),
        &arguments,
        call_ic,
    )?;
    // Error constructors return ordinary objects, but those objects must retain
    // identity with the constructor used (`thrown.constructor === TypeError`).
    // Stamp the constructor metadata at the construction boundary so all error
    // kinds share the same native allocation path.
    let error_constructor = callee
        .as_function_ref()
        .and_then(|function| match function.kind {
            FunctionKind::Builtin(
                BuiltinId::ErrorConstructor
                | BuiltinId::TypeErrorConstructor
                | BuiltinId::RangeErrorConstructor
                | BuiltinId::URIErrorConstructor
                | BuiltinId::SyntaxErrorConstructor
                | BuiltinId::ReferenceErrorConstructor
                | BuiltinId::EvalErrorConstructor
                | BuiltinId::AggregateErrorConstructor,
            ) => Some(()),
            _ => None,
        });
    if error_constructor.is_some() && result.as_object_ref().is_some() {
        vm(frame).set_prop(&result, "constructor", callee.clone());
        let name = callee
            .as_function_ref()
            .and_then(|function| function.props.borrow().get("name").cloned())
            .unwrap_or_else(|| Value::string_value("Error"));
        vm(frame).set_prop(&result, "name", name);
    }
    if wrapper_constructor {
        vm(frame).set_prop(&object, "\0primitive", result.clone());
        let wrapper = match callee.as_function_ref().map(|function| &function.kind) {
            Some(FunctionKind::Builtin(BuiltinId::BooleanConstructor)) => "Boolean",
            Some(FunctionKind::Builtin(BuiltinId::NumberConstructor)) => "Number",
            Some(FunctionKind::Builtin(BuiltinId::StringConstructor)) => "String",
            _ => "Object",
        };
        vm(frame).set_prop(&object, "\0wrapper", Value::string_value(wrapper));
        if matches!(
            callee.as_function_ref().map(|function| &function.kind),
            Some(FunctionKind::Builtin(BuiltinId::StringConstructor))
        ) {
            super::initialize_string_wrapper(vm(frame), &object, &result);
        }
    }
    let returns_object = result.is_object() || result.is_function() || result.is_regexp();
    put(
        frame,
        dst,
        if wrapper_constructor {
            object
        } else if native || returns_object {
            result
        } else {
            object
        },
    );
    Ok(())
}

fn enumerable_keys(value: &Value) -> Vec<String> {
    super::object_own_enumerable_keys(value)
}

fn op_name(op: &DynOp) -> &'static str {
    op.name()
}

fn block_profile_op_name(op: &DynOp) -> String {
    match op {
        DynOp::LoadLiteral { value, .. } => {
            format!("LoadLiteral:{}", value.profile_kind())
        }
        DynOp::Unary { kind, .. } => format!("Unary:{}", kind.profile_name()),
        DynOp::Binary { kind, .. } => format!("Binary:{}", kind.profile_name()),
        DynOp::Update { increment, .. } => {
            format!("Update:{}", if *increment { "++" } else { "--" })
        }
        _ => op.name().to_owned(),
    }
}

fn function_binding_layout(code: &DynCode) -> HashMap<String, usize> {
    if !code.bindings.is_empty() {
        return code
            .bindings
            .iter()
            .enumerate()
            .map(|(slot, name)| (name.clone(), slot))
            .collect();
    }
    let mut layout = HashMap::new();
    let mut add_binding = |name: &str| {
        let next_slot = layout.len();
        layout.entry(name.to_owned()).or_insert(next_slot);
    };
    add_binding(THIS_BINDING_NAME);
    add_binding(ARGUMENTS_BINDING_NAME);
    for parameter in &code.params {
        add_binding(parameter);
    }
    for (name, _) in &code.hoisted {
        add_binding(name);
    }
    for instruction in &code.ops {
        match &instruction.op {
            DynOp::DeclareName { name, .. } => add_binding(name),
            DynOp::Catch {
                binding: Some(CatchBinding::Name(name)),
            } => add_binding(name),
            _ => {}
        }
    }
    layout
}

fn inline_literal_bits(literal: &Literal) -> Option<u64> {
    Some(match literal {
        Literal::Undefined => raw_value::RawValue::UNDEFINED.bits(),
        Literal::Null => raw_value::RawValue::NULL.bits(),
        Literal::Bool(value) => raw_value::RawValue::boolean(*value).bits(),
        Literal::Number(value) => raw_value::RawValue::number(*value).bits(),
        Literal::String(_) => return None,
    })
}

fn immediate_literal_truthiness(literal: &Literal) -> Option<bool> {
    match literal {
        Literal::Undefined | Literal::Null => Some(false),
        Literal::Bool(value) => Some(*value),
        Literal::Number(value) => Some(*value != 0.0 && !value.is_nan()),
        Literal::String(_) => None,
    }
}

#[cfg(target_arch = "aarch64")]
fn bytecode_target_label(code_len: usize, target: usize) -> LabelId {
    if target == code_len {
        FUNCTION_EXIT_LABEL
    } else {
        LabelId(target as u32)
    }
}

fn inline_site(pc: usize, op: &DynOp) -> InlineSite {
    let mut site = InlineSite::unused(pc);
    site.opcode = inline_opcode(op)
        .map(|opcode| opcode as usize)
        .unwrap_or(InlineOpcode::Unsupported as usize);
    match op {
        DynOp::LoadLiteral { dst, value } => {
            site.dst = usize::from(*dst);
            site.literal = inline_literal_bits(value).unwrap_or(site.literal);
        }
        DynOp::LoadLocal { dst, slot } => {
            site.dst = usize::from(*dst);
            site.left = *slot;
        }
        DynOp::LoadName { dst, .. } => {
            site.dst = usize::from(*dst);
        }
        DynOp::DeclareLocal { slot, src } | DynOp::StoreLocal { slot, src } => {
            site.dst = *slot;
            site.left = usize::from(*src);
        }
        DynOp::Move { dst, src } => {
            site.dst = usize::from(*dst);
            site.left = usize::from(*src);
        }
        DynOp::NewObject { .. } => {
            site.literal = NO_ALLOCATION_SHAPE_POINTER;
        }
        DynOp::Unary { dst, src, .. } => {
            site.dst = usize::from(*dst);
            site.left = usize::from(*src);
        }
        DynOp::Binary {
            dst, left, right, ..
        } => {
            site.dst = usize::from(*dst);
            site.left = usize::from(*left);
            site.right = usize::from(*right);
        }
        DynOp::Jump { target } => site.literal = *target as u64,
        DynOp::JumpIfFalse { test, target } => {
            site.left = usize::from(*test);
            site.literal = *target as u64;
        }
        DynOp::GetComputed { dst, object, key } => {
            site.dst = usize::from(*dst);
            site.left = usize::from(*object);
            site.right = usize::from(*key);
        }
        DynOp::GetStatic { dst, object, .. } => {
            site.dst = usize::from(*dst);
            site.left = usize::from(*object);
        }
        DynOp::SetComputed {
            object, key, src, ..
        } => {
            site.dst = usize::from(*src);
            site.left = usize::from(*object);
            site.right = usize::from(*key);
        }
        DynOp::SetStatic { object, src, .. } => {
            site.dst = usize::from(*src);
            site.left = usize::from(*object);
        }
        DynOp::Return { src } => {
            site.left = src.map_or(UNUSED_SITE_OPERAND, usize::from);
        }
        _ => {}
    }
    site
}

fn inline_opcode(op: &DynOp) -> Option<InlineOpcode> {
    match op {
        DynOp::LoadLiteral { value, .. } if inline_literal_bits(value).is_some() => {
            Some(InlineOpcode::LoadLiteral)
        }
        DynOp::LoadLocal { .. } => Some(InlineOpcode::LoadLocal),
        DynOp::DeclareLocal { .. } | DynOp::StoreLocal { .. } => Some(InlineOpcode::StoreLocal),
        DynOp::Move { .. } => Some(InlineOpcode::Move),
        DynOp::Binary { kind, .. } => match kind {
            Op::Add => Some(InlineOpcode::Add),
            Op::Sub => Some(InlineOpcode::Subtract),
            Op::Mul => Some(InlineOpcode::Multiply),
            Op::Div => Some(InlineOpcode::Divide),
            // AArch64 lowers f64 remainder to an external `fmod` call. It stays
            // at the canonical block slow path until external stencil calls
            // are represented as explicit patch obligations.
            Op::Rem => None,
            Op::Lt => Some(InlineOpcode::Less),
            Op::Le => Some(InlineOpcode::LessEqual),
            Op::Gt => Some(InlineOpcode::Greater),
            Op::Ge => Some(InlineOpcode::GreaterEqual),
            _ => None,
        },
        DynOp::Jump { .. } => Some(InlineOpcode::Jump),
        DynOp::JumpIfFalse { .. } => Some(InlineOpcode::JumpIfFalse),
        _ => None,
    }
}

fn trace_block_shapes(code: &DynCode, plan: &RegionPlan) {
    if std::env::var_os(BLOCK_SHAPE_TRACE_ENV).is_none() {
        return;
    }
    for block in plan.blocks() {
        let shape = (block.start..block.end)
            .map(|pc| match inline_opcode(&code.ops[pc].op) {
                Some(opcode) => match opcode {
                    InlineOpcode::Unsupported => "Unsupported",
                    InlineOpcode::LoadLiteral => "LoadLiteral",
                    InlineOpcode::LoadLocal => "LoadLocal",
                    InlineOpcode::StoreLocal => "StoreLocal",
                    InlineOpcode::Move => "Move",
                    InlineOpcode::Add => "Add",
                    InlineOpcode::Subtract => "Subtract",
                    InlineOpcode::Multiply => "Multiply",
                    InlineOpcode::Divide => "Divide",
                    InlineOpcode::Remainder => "Remainder",
                    InlineOpcode::Less => "Less",
                    InlineOpcode::LessEqual => "LessEqual",
                    InlineOpcode::Greater => "Greater",
                    InlineOpcode::GreaterEqual => "GreaterEqual",
                    InlineOpcode::Jump => "Jump",
                    InlineOpcode::JumpIfFalse => "JumpIfFalse",
                },
                None => code.ops[pc].op.name(),
            })
            .collect::<Vec<_>>()
            .join(",");
        eprintln!(
            "BLOCK_SHAPE:{}:{}",
            if block.loop_region { "loop" } else { "block" },
            shape
        );
    }
}

#[cfg(target_arch = "aarch64")]
enum DirectBlockTemplate {
    Terminal {
        stencil_name: &'static str,
    },
    Transfer {
        stencil_name: &'static str,
        target_label: LabelId,
    },
    UpdateLocalLiteralJump {
        stencil_name: &'static str,
        target: usize,
    },
    ConditionLocalLiteral {
        stencil_name: &'static str,
        target: usize,
    },
    ConditionLocalName {
        stencil_name: &'static str,
        target: usize,
    },
    InstanceOfCondition {
        stencil_name: &'static str,
        target: usize,
    },
    PropertyCondition {
        stencil_name: &'static str,
        target: usize,
    },
}

#[cfg(target_arch = "aarch64")]
#[derive(Clone, Copy)]
enum DirectOpcodeTemplate {
    Next(&'static str),
    Transfer {
        stencil_name: &'static str,
        target: usize,
    },
    Branch {
        stencil_name: &'static str,
        target: usize,
    },
    Return(&'static str),
}

#[cfg(target_arch = "aarch64")]
struct NameSnapshotPlan {
    representative_pcs: Vec<usize>,
    site_slots: Vec<(usize, usize)>,
}

#[cfg(target_arch = "aarch64")]
fn name_snapshot_plan(code: &DynCode, pcs: &[usize]) -> NameSnapshotPlan {
    let mut slots_by_name = HashMap::<&str, usize>::new();
    let mut representative_pcs = Vec::new();
    let mut site_slots = Vec::with_capacity(pcs.len());
    for pc in pcs.iter().copied() {
        let DynOp::LoadName { name, .. } = &code.ops[pc].op else {
            unreachable!("name snapshot pc identifies LoadName")
        };
        let slot = *slots_by_name.entry(name).or_insert_with(|| {
            let slot = representative_pcs.len();
            representative_pcs.push(pc);
            slot
        });
        site_slots.push((pc, slot));
    }
    NameSnapshotPlan {
        representative_pcs,
        site_slots,
    }
}

#[cfg(target_arch = "aarch64")]
fn select_direct_opcode_template(op: &DynOp) -> Option<DirectOpcodeTemplate> {
    let template = match op {
        DynOp::LoadLiteral { value, .. } if inline_literal_bits(value).is_some() => {
            DirectOpcodeTemplate::Next("quench_dyn_load_literal")
        }
        DynOp::LoadLocal { .. } => DirectOpcodeTemplate::Next("quench_dyn_load_local"),
        DynOp::LoadName { .. } => DirectOpcodeTemplate::Next("quench_dyn_load_name_cached"),
        DynOp::DeclareLocal { .. } | DynOp::StoreLocal { .. } => {
            DirectOpcodeTemplate::Next("quench_dyn_store_local")
        }
        DynOp::Move { .. } => DirectOpcodeTemplate::Next("quench_dyn_move"),
        DynOp::Unary {
            kind: UnaryKind::Not,
            ..
        } => DirectOpcodeTemplate::Next("quench_dyn_not"),
        DynOp::Binary { kind, .. } => DirectOpcodeTemplate::Next(match kind {
            Op::Add => "quench_dyn_add",
            Op::Sub => "quench_dyn_subtract",
            Op::Mul => "quench_dyn_multiply",
            Op::Div => "quench_dyn_divide",
            Op::Lt => "quench_dyn_less",
            Op::Le => "quench_dyn_less_equal",
            Op::Gt => "quench_dyn_greater",
            Op::Ge => "quench_dyn_greater_equal",
            Op::Eq => "quench_dyn_equal",
            Op::Ne => "quench_dyn_not_equal",
            Op::StrictEq => "quench_dyn_strict_equal",
            Op::StrictNe => "quench_dyn_strict_not_equal",
            Op::Shl => "quench_dyn_shift_left",
            Op::Shr => "quench_dyn_shift_right",
            Op::Ushr => "quench_dyn_shift_right_unsigned",
            Op::Or => "quench_dyn_bit_or",
            Op::Xor => "quench_dyn_bit_xor",
            Op::And => "quench_dyn_bit_and",
            _ => return None,
        }),
        DynOp::GetStatic { .. } => DirectOpcodeTemplate::Next("quench_dyn_get_static"),
        DynOp::GetComputed { .. } => DirectOpcodeTemplate::Next("quench_dyn_get_computed_dense"),
        DynOp::SetStatic { .. } => DirectOpcodeTemplate::Next("quench_dyn_set_static"),
        DynOp::SetComputed { .. } => DirectOpcodeTemplate::Next("quench_dyn_set_computed_dense"),
        DynOp::Jump { target } => DirectOpcodeTemplate::Transfer {
            stencil_name: "quench_dyn_jump",
            target: *target,
        },
        DynOp::JumpIfFalse { target, .. } => DirectOpcodeTemplate::Branch {
            stencil_name: "quench_dyn_jump_if_false",
            target: *target,
        },
        DynOp::Return { src } => DirectOpcodeTemplate::Return(if src.is_some() {
            "quench_dyn_return"
        } else {
            "quench_dyn_return_undefined"
        }),
        _ => return None,
    };
    Some(template)
}

#[cfg(target_arch = "aarch64")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CallRegion {
    start: usize,
    call_pc: usize,
    end: usize,
}

#[cfg(target_arch = "aarch64")]
fn select_call_region(code: &DynCode, start: usize, end: usize) -> Option<CallRegion> {
    let mut calls = (start..end).filter(|pc| matches!(code.ops[*pc].op, DynOp::Call { .. }));
    let call_pc = calls.next()?;
    if calls.next().is_some() {
        return None;
    }
    let surrounding_opcodes = end - start - MAX_CALLS_PER_INITIAL_REGION;
    if surrounding_opcodes < MIN_SURROUNDING_DIRECT_OPCODES_PER_CALL_REGION {
        return None;
    }
    code.ops[start..end]
        .iter()
        .enumerate()
        .all(|(offset, instruction)| {
            start + offset == call_pc || select_direct_opcode_template(&instruction.op).is_some()
        })
        .then_some(CallRegion {
            start,
            call_pc,
            end,
        })
}

#[cfg(target_arch = "aarch64")]
fn select_get_static_call_pair(code: &DynCode, call_pc: usize) -> Option<usize> {
    let get_pc = call_pc.checked_sub(NEXT_INSTRUCTION_DISTANCE)?;
    let DynOp::GetStatic { dst: loaded, .. } = &code.ops[get_pc].op else {
        return None;
    };
    let DynOp::Call {
        dst: result,
        callee,
        ..
    } = &code.ops[call_pc].op
    else {
        return None;
    };
    (*loaded == *callee
        && *loaded != *result
        && register_is_read_only_at(code, *loaded, &[call_pc]))
    .then_some(get_pc)
}

#[cfg(target_arch = "aarch64")]
fn select_get_static_load_local_call(code: &DynCode, call_pc: usize) -> Option<usize> {
    let load_pc = call_pc.checked_sub(NEXT_INSTRUCTION_DISTANCE)?;
    let get_pc = load_pc.checked_sub(NEXT_INSTRUCTION_DISTANCE)?;
    let DynOp::GetStatic { dst: callee, .. } = &code.ops[get_pc].op else {
        return None;
    };
    let DynOp::LoadLocal { dst: argument, .. } = &code.ops[load_pc].op else {
        return None;
    };
    let DynOp::Call {
        dst: result,
        callee: called,
        args,
        ..
    } = &code.ops[call_pc].op
    else {
        return None;
    };
    (*callee == *called
        && args.as_slice() == [*argument]
        && callee != argument
        && *result != *callee
        && *result != *argument
        && register_is_read_only_at(code, *callee, &[call_pc])
        && register_is_read_only_at(code, *argument, &[call_pc]))
    .then_some(get_pc)
}

#[cfg(target_arch = "aarch64")]
struct NumericRegionLink {
    quote: numeric_region::QuotedLoop,
    guard: numeric_region::GuardPlan,
    register_plan: Option<numeric_region::RegisterRegionPlan>,
    level: StencilLevel,
}

#[cfg(target_arch = "aarch64")]
struct PropertyLiteralCondition<'a> {
    receiver: Register,
    property: Register,
    literal_register: Register,
    comparison: Register,
    literal: &'a Literal,
    kind: Op,
    target: usize,
}

#[cfg(target_arch = "aarch64")]
fn parse_property_literal_condition(
    ops: &[super::dynbytecode::DynInstr],
) -> Option<PropertyLiteralCondition<'_>> {
    let [load, get, literal, compare, branch] = ops else {
        return None;
    };
    let DynOp::LoadLocal { dst: receiver, .. } = &load.op else {
        return None;
    };
    let DynOp::GetStatic {
        dst: property,
        object,
        ..
    } = &get.op
    else {
        return None;
    };
    let DynOp::LoadLiteral {
        dst: literal_register,
        value: literal,
    } = &literal.op
    else {
        return None;
    };
    let DynOp::Binary {
        dst: comparison,
        left,
        right,
        kind,
    } = compare.op
    else {
        return None;
    };
    let DynOp::JumpIfFalse { test, target } = branch.op else {
        return None;
    };
    (*receiver == *object && *property == left && *literal_register == right && comparison == test)
        .then_some(PropertyLiteralCondition {
            receiver: *receiver,
            property: *property,
            literal_register: *literal_register,
            comparison,
            literal,
            kind,
            target,
        })
}

#[cfg(target_arch = "aarch64")]
fn select_property_literal_condition(
    code: &DynCode,
    start: usize,
    ops: &[super::dynbytecode::DynInstr],
) -> Option<DirectBlockTemplate> {
    let pattern = parse_property_literal_condition(ops)?;
    let reads_match = register_is_read_only_at(
        code,
        pattern.receiver,
        &[start + PROPERTY_LITERAL_GET_PC_OFFSET],
    ) && register_is_read_only_at(
        code,
        pattern.property,
        &[start + PROPERTY_LITERAL_COMPARE_PC_OFFSET],
    ) && register_is_read_only_at(
        code,
        pattern.literal_register,
        &[start + PROPERTY_LITERAL_COMPARE_PC_OFFSET],
    ) && register_is_read_only_at(
        code,
        pattern.comparison,
        &[start + PROPERTY_LITERAL_BRANCH_PC_OFFSET],
    );
    if !reads_match {
        return None;
    }
    Some(DirectBlockTemplate::PropertyCondition {
        stencil_name: property_literal_condition_stencil(pattern.literal, pattern.kind)?,
        target: pattern.target,
    })
}

#[cfg(target_arch = "aarch64")]
fn property_literal_condition_stencil(literal: &Literal, kind: Op) -> Option<&'static str> {
    match (literal, kind) {
        (Literal::Null | Literal::Undefined, Op::Eq) => {
            Some("quench_dyn_dead_own_property_nullish_equal")
        }
        (Literal::Null | Literal::Undefined, Op::Ne) => {
            Some("quench_dyn_dead_own_property_nullish_not_equal")
        }
        (Literal::Null | Literal::Undefined, Op::StrictEq) => {
            Some("quench_dyn_dead_own_property_immediate_strict_equal")
        }
        (Literal::Null | Literal::Undefined, Op::StrictNe) => {
            Some("quench_dyn_dead_own_property_immediate_strict_not_equal")
        }
        _ => None,
    }
}

#[cfg(target_arch = "aarch64")]
fn select_direct_block_template(
    code: &DynCode,
    start: usize,
    end: usize,
) -> Option<DirectBlockTemplate> {
    let ops = &code.ops.get(start..end)?;
    if let [
        first_receiver_load,
        first_value_load,
        first_set,
        second_receiver_load,
        second_value_load,
        second_set,
        return_,
    ] = ops
        && let DynOp::LoadLocal {
            dst: first_receiver_register,
            ..
        } = first_receiver_load.op
        && let DynOp::LoadLocal {
            dst: first_value_register,
            slot: first_source_local,
        } = first_value_load.op
        && let DynOp::SetStatic {
            object: first_set_receiver,
            src: first_set_value,
            ..
        } = first_set.op
        && let DynOp::LoadLocal {
            dst: second_receiver_register,
            ..
        } = second_receiver_load.op
        && let DynOp::LoadLocal {
            dst: second_value_register,
            slot: second_source_local,
        } = second_value_load.op
        && let DynOp::SetStatic {
            object: second_set_receiver,
            src: second_set_value,
            ..
        } = second_set.op
        && let DynOp::Return { src: None } = return_.op
        && first_receiver_register == first_set_receiver
        && first_value_register == first_set_value
        && second_receiver_register == second_set_receiver
        && second_value_register == second_set_value
        && first_source_local != second_source_local
        && [
            first_receiver_register,
            first_value_register,
            second_receiver_register,
            second_value_register,
        ]
        .into_iter()
        .all(|register| {
            code.ops[end..]
                .iter()
                .all(|instruction| !op_reads_register(&instruction.op, register))
        })
    {
        return Some(DirectBlockTemplate::Terminal {
            stencil_name: "quench_dyn_take_two_own_properties_return_undefined",
        });
    }
    if let [return_] = ops
        && let DynOp::Return { src } = return_.op
    {
        return Some(DirectBlockTemplate::Terminal {
            stencil_name: if src.is_some() {
                "quench_dyn_return"
            } else {
                "quench_dyn_return_undefined"
            },
        });
    }
    if let [jump] = ops
        && let DynOp::Jump { target } = jump.op
    {
        return Some(DirectBlockTemplate::Transfer {
            stencil_name: "quench_dyn_jump",
            target_label: bytecode_target_label(code.ops.len(), target),
        });
    }
    if let [literal, branch] = ops
        && let DynOp::LoadLiteral { dst, value } = &literal.op
        && let Some(truthy) = immediate_literal_truthiness(value)
        && let DynOp::JumpIfFalse { test, target } = branch.op
        && *dst == test
        && register_is_read_only_at(code, *dst, &[start + NEXT_INSTRUCTION_DISTANCE])
    {
        let target = if truthy { end } else { target };
        return Some(DirectBlockTemplate::Transfer {
            stencil_name: if truthy {
                "quench_dyn_constant_truthy"
            } else {
                "quench_dyn_constant_falsey"
            },
            target_label: bytecode_target_label(code.ops.len(), target),
        });
    }
    if let [load, return_] = ops
        && let DynOp::LoadLocal { dst, .. } = &load.op
        && let DynOp::Return { src: Some(src) } = &return_.op
        && dst == src
        && register_is_read_only_at(code, *dst, &[start + NEXT_INSTRUCTION_DISTANCE])
    {
        return Some(DirectBlockTemplate::Terminal {
            stencil_name: "quench_dyn_return_local",
        });
    }
    if let [literal, return_] = ops
        && let DynOp::LoadLiteral { dst, value } = &literal.op
        && inline_literal_bits(value).is_some()
        && let DynOp::Return { src: Some(src) } = &return_.op
        && dst == src
        && register_is_read_only_at(code, *dst, &[start + NEXT_INSTRUCTION_DISTANCE])
    {
        return Some(DirectBlockTemplate::Terminal {
            stencil_name: "quench_dyn_return_literal",
        });
    }
    if end == start + PROPERTY_LOAD_JUMP_INSTRUCTION_COUNT
        && let [load, get, store, jump] = ops
        && let DynOp::LoadLocal { dst: receiver, .. } = &load.op
        && let DynOp::GetStatic {
            dst: property,
            object,
            ..
        } = &get.op
        && let DynOp::StoreLocal { src, .. } = &store.op
        && let DynOp::Jump { target } = &jump.op
        && receiver == object
        && property == src
        && register_is_read_only_at(code, *receiver, &[start + PROPERTY_LOAD_GET_PC_OFFSET])
        && register_is_read_only_at(code, *property, &[start + PROPERTY_LOAD_STORE_PC_OFFSET])
    {
        return Some(DirectBlockTemplate::Transfer {
            stencil_name: "quench_dyn_dead_own_property_store_local_jump",
            target_label: bytecode_target_label(code.ops.len(), *target),
        });
    }
    if let [
        first_load,
        first_get,
        second_load,
        second_get,
        compare,
        branch,
    ] = ops
        && let DynOp::LoadLocal {
            dst: first_receiver,
            ..
        } = &first_load.op
        && let DynOp::GetStatic {
            dst: first_value,
            object: first_object,
            ..
        } = &first_get.op
        && let DynOp::LoadLocal {
            dst: second_receiver,
            ..
        } = &second_load.op
        && let DynOp::GetStatic {
            dst: second_value,
            object: second_object,
            ..
        } = &second_get.op
        && let DynOp::Binary {
            dst: comparison,
            left,
            right,
            kind: Op::StrictEq,
        } = &compare.op
        && let DynOp::JumpIfFalse { test, target } = &branch.op
        && first_receiver == first_object
        && second_receiver == second_object
        && first_value == left
        && second_value == right
        && comparison == test
        && register_is_read_only_at(
            code,
            *first_receiver,
            &[start + PROPERTY_EQUAL_FIRST_GET_PC_OFFSET],
        )
        && register_is_read_only_at(
            code,
            *first_value,
            &[start + PROPERTY_EQUAL_COMPARE_PC_OFFSET],
        )
        && register_is_read_only_at(
            code,
            *second_receiver,
            &[start + PROPERTY_EQUAL_SECOND_GET_PC_OFFSET],
        )
        && register_is_read_only_at(
            code,
            *second_value,
            &[start + PROPERTY_EQUAL_COMPARE_PC_OFFSET],
        )
        && register_is_read_only_at(
            code,
            *comparison,
            &[start + PROPERTY_EQUAL_BRANCH_PC_OFFSET],
        )
    {
        return Some(DirectBlockTemplate::PropertyCondition {
            stencil_name: "quench_dyn_dead_own_property_strict_equal",
            target: *target,
        });
    }
    if let Some(template) = select_property_literal_condition(code, start, ops) {
        return Some(template);
    }
    if let [load, literal, binary, store, jump] = ops
        && let DynOp::LoadLocal {
            dst: load_dst,
            slot: load_slot,
        } = &load.op
        && let DynOp::LoadLiteral {
            dst: literal_dst,
            value: Literal::Number(_),
        } = &literal.op
        && let DynOp::Binary {
            dst: binary_dst,
            left,
            right,
            kind,
        } = &binary.op
        && let DynOp::StoreLocal {
            slot: store_slot,
            src,
        } = &store.op
        && let DynOp::Jump { target } = &jump.op
        && left == load_dst
        && right == literal_dst
        && src == binary_dst
        && store_slot == load_slot
    {
        let binary_pc = start + UPDATE_BINARY_PC_OFFSET;
        let store_pc = start + UPDATE_STORE_PC_OFFSET;
        let results_are_dead = register_is_read_only_at(code, *load_dst, &[binary_pc])
            && register_is_read_only_at(code, *literal_dst, &[binary_pc])
            && register_is_read_only_at(code, *binary_dst, &[store_pc]);
        let stencil_name = if results_are_dead {
            match kind {
                Op::Add => "quench_dyn_dead_update_local_number_add_jump",
                Op::Sub => "quench_dyn_dead_update_local_number_subtract_jump",
                _ => return None,
            }
        } else if matches!(kind, Op::Add) {
            "quench_dyn_loop_add_local_literal"
        } else {
            return None;
        };
        return Some(DirectBlockTemplate::UpdateLocalLiteralJump {
            stencil_name,
            target: *target,
        });
    }
    if let [
        load,
        update_literal,
        update,
        store,
        bound_literal,
        compare,
        branch,
    ] = ops
        && let DynOp::LoadLocal {
            dst: load_dst,
            slot: load_slot,
        } = &load.op
        && let DynOp::LoadLiteral {
            dst: update_literal_dst,
            value: Literal::Number(_),
        } = &update_literal.op
        && let DynOp::Binary {
            dst: update_dst,
            left: update_left,
            right: update_right,
            kind: update_kind,
        } = &update.op
        && let DynOp::StoreLocal {
            slot: store_slot,
            src: store_src,
        } = &store.op
        && let DynOp::LoadLiteral {
            dst: bound_literal_dst,
            value: Literal::Number(_),
        } = &bound_literal.op
        && let DynOp::Binary {
            dst: compare_dst,
            left: compare_left,
            right: compare_right,
            kind: compare_kind,
        } = &compare.op
        && let DynOp::JumpIfFalse { test, target } = &branch.op
        && update_left == load_dst
        && update_right == update_literal_dst
        && store_src == update_dst
        && store_slot == load_slot
        && compare_left == update_dst
        && compare_right == bound_literal_dst
        && test == compare_dst
    {
        let update_pc = start + UPDATE_BINARY_PC_OFFSET;
        let store_pc = start + UPDATE_STORE_PC_OFFSET;
        let compare_pc = start + RECURRENCE_COMPARE_PC_OFFSET;
        let branch_pc = start + RECURRENCE_BRANCH_PC_OFFSET;
        let results_are_dead = register_is_read_only_at(code, *load_dst, &[update_pc])
            && register_is_read_only_at(code, *update_literal_dst, &[update_pc])
            && register_is_read_only_at(code, *update_dst, &[store_pc, compare_pc])
            && register_is_read_only_at(code, *bound_literal_dst, &[compare_pc])
            && register_is_read_only_at(code, *compare_dst, &[branch_pc]);
        if results_are_dead {
            let stencil_name = select_dead_recurrence_stencil(*update_kind, *compare_kind)?;
            return Some(DirectBlockTemplate::ConditionLocalLiteral {
                stencil_name,
                target: *target,
            });
        }
    }
    if let [load, literal, compare, branch] = ops
        && let DynOp::LoadLocal { dst: load_dst, .. } = &load.op
        && let DynOp::LoadLiteral {
            dst: literal_dst,
            value,
        } = &literal.op
        && let DynOp::Binary {
            dst: compare_dst,
            left,
            right,
            kind,
        } = &compare.op
        && let DynOp::JumpIfFalse { test, target } = &branch.op
        && left == load_dst
        && right == literal_dst
        && test == compare_dst
    {
        let compare_pc = start + CONDITION_COMPARE_PC_OFFSET;
        let branch_pc = start + CONDITION_BRANCH_PC_OFFSET;
        let results_are_dead = register_is_read_only_at(code, *load_dst, &[compare_pc])
            && register_is_read_only_at(code, *literal_dst, &[compare_pc])
            && register_is_read_only_at(code, *compare_dst, &[branch_pc]);
        let stencil_name = if results_are_dead {
            select_dead_condition_stencil(value, *kind)?
        } else {
            match (value, kind) {
                (Literal::Number(_), Op::Lt) => "quench_dyn_condition_local_literal_less",
                (Literal::Number(_), Op::Le) => "quench_dyn_condition_local_literal_less_equal",
                (Literal::Number(_), Op::Gt) => "quench_dyn_condition_local_literal_greater",
                (Literal::Number(_), Op::Ge) => "quench_dyn_condition_local_literal_greater_equal",
                _ => return None,
            }
        };
        return Some(DirectBlockTemplate::ConditionLocalLiteral {
            stencil_name,
            target: *target,
        });
    }
    if let [load, name, operation, branch] = ops
        && let DynOp::LoadLocal { dst: receiver, .. } = &load.op
        && let DynOp::LoadName {
            dst: constructor, ..
        } = &name.op
        && let DynOp::InstanceOf {
            dst: result,
            left,
            right,
        } = &operation.op
        && let DynOp::JumpIfFalse { test, target } = &branch.op
        && receiver == left
        && constructor == right
        && result == test
        && register_is_read_only_at(code, *receiver, &[start + INSTANCEOF_OPERATION_PC_OFFSET])
        && register_is_read_only_at(
            code,
            *constructor,
            &[start + INSTANCEOF_OPERATION_PC_OFFSET],
        )
        && register_is_read_only_at(code, *result, &[start + INSTANCEOF_BRANCH_PC_OFFSET])
    {
        return Some(DirectBlockTemplate::InstanceOfCondition {
            stencil_name: "quench_dyn_dead_cached_instanceof_condition",
            target: *target,
        });
    }
    if let [load, name, operation, not, branch] = ops
        && let DynOp::LoadLocal { dst: receiver, .. } = &load.op
        && let DynOp::LoadName {
            dst: constructor, ..
        } = &name.op
        && let DynOp::InstanceOf {
            dst: membership,
            left,
            right,
        } = &operation.op
        && let DynOp::Unary {
            dst: inverted,
            src,
            kind: UnaryKind::Not,
        } = &not.op
        && let DynOp::JumpIfFalse { test, target } = &branch.op
        && receiver == left
        && constructor == right
        && membership == src
        && inverted == test
        && register_is_read_only_at(code, *receiver, &[start + INSTANCEOF_OPERATION_PC_OFFSET])
        && register_is_read_only_at(
            code,
            *constructor,
            &[start + INSTANCEOF_OPERATION_PC_OFFSET],
        )
        && register_is_read_only_at(
            code,
            *membership,
            &[start + INSTANCEOF_NOT_OPERATION_PC_OFFSET],
        )
        && register_is_read_only_at(code, *inverted, &[start + INSTANCEOF_NOT_BRANCH_PC_OFFSET])
    {
        return Some(DirectBlockTemplate::InstanceOfCondition {
            stencil_name: "quench_dyn_dead_cached_instanceof_not_condition",
            target: *target,
        });
    }
    if let [load, name, compare, branch] = ops
        && let DynOp::LoadLocal { dst: load_dst, .. } = &load.op
        && let DynOp::LoadName { dst: name_dst, .. } = &name.op
        && let DynOp::Binary {
            dst: compare_dst,
            left,
            right,
            kind,
        } = &compare.op
        && let DynOp::JumpIfFalse { test, target } = &branch.op
        && left == load_dst
        && right == name_dst
        && test == compare_dst
    {
        let compare_pc = start + CONDITION_COMPARE_PC_OFFSET;
        let branch_pc = start + CONDITION_BRANCH_PC_OFFSET;
        let results_are_dead = register_is_read_only_at(code, *load_dst, &[compare_pc])
            && register_is_read_only_at(code, *name_dst, &[compare_pc])
            && register_is_read_only_at(code, *compare_dst, &[branch_pc]);
        if results_are_dead {
            return Some(DirectBlockTemplate::ConditionLocalName {
                stencil_name: select_dead_name_condition_stencil(*kind)?,
                target: *target,
            });
        }
    }
    None
}

#[cfg(target_arch = "aarch64")]
fn select_dead_name_condition_stencil(kind: Op) -> Option<&'static str> {
    match kind {
        Op::Eq | Op::StrictEq => Some("quench_dyn_dead_condition_local_name_number_equal"),
        Op::Ne | Op::StrictNe => Some("quench_dyn_dead_condition_local_name_number_not_equal"),
        Op::Lt => Some("quench_dyn_dead_condition_local_name_number_less"),
        Op::Le => Some("quench_dyn_dead_condition_local_name_number_less_equal"),
        Op::Gt => Some("quench_dyn_dead_condition_local_name_number_greater"),
        Op::Ge => Some("quench_dyn_dead_condition_local_name_number_greater_equal"),
        _ => None,
    }
}

#[cfg(target_arch = "aarch64")]
fn select_dead_recurrence_stencil(update: Op, compare: Op) -> Option<&'static str> {
    match (update, compare) {
        (Op::Add, Op::Eq | Op::StrictEq) => Some("quench_dyn_dead_recurrence_add_equal"),
        (Op::Add, Op::Ne | Op::StrictNe) => Some("quench_dyn_dead_recurrence_add_not_equal"),
        (Op::Add, Op::Lt) => Some("quench_dyn_dead_recurrence_add_less"),
        (Op::Add, Op::Le) => Some("quench_dyn_dead_recurrence_add_less_equal"),
        (Op::Add, Op::Gt) => Some("quench_dyn_dead_recurrence_add_greater"),
        (Op::Add, Op::Ge) => Some("quench_dyn_dead_recurrence_add_greater_equal"),
        (Op::Sub, Op::Eq | Op::StrictEq) => Some("quench_dyn_dead_recurrence_subtract_equal"),
        (Op::Sub, Op::Ne | Op::StrictNe) => Some("quench_dyn_dead_recurrence_subtract_not_equal"),
        (Op::Sub, Op::Lt) => Some("quench_dyn_dead_recurrence_subtract_less"),
        (Op::Sub, Op::Le) => Some("quench_dyn_dead_recurrence_subtract_less_equal"),
        (Op::Sub, Op::Gt) => Some("quench_dyn_dead_recurrence_subtract_greater"),
        (Op::Sub, Op::Ge) => Some("quench_dyn_dead_recurrence_subtract_greater_equal"),
        _ => None,
    }
}

#[cfg(target_arch = "aarch64")]
fn select_dead_condition_stencil(literal: &Literal, kind: Op) -> Option<&'static str> {
    match (literal, kind) {
        (Literal::Number(_), Op::Lt) => Some("quench_dyn_dead_condition_local_number_less"),
        (Literal::Number(_), Op::Le) => Some("quench_dyn_dead_condition_local_number_less_equal"),
        (Literal::Number(_), Op::Gt) => Some("quench_dyn_dead_condition_local_number_greater"),
        (Literal::Number(_), Op::Ge) => {
            Some("quench_dyn_dead_condition_local_number_greater_equal")
        }
        (Literal::Number(_), Op::Eq | Op::StrictEq) => {
            Some("quench_dyn_dead_condition_local_number_equal")
        }
        (Literal::Number(_), Op::Ne | Op::StrictNe) => {
            Some("quench_dyn_dead_condition_local_number_not_equal")
        }
        (Literal::Null | Literal::Undefined, Op::Eq) => {
            Some("quench_dyn_dead_condition_local_nullish_equal")
        }
        (Literal::Null | Literal::Undefined, Op::Ne) => {
            Some("quench_dyn_dead_condition_local_nullish_not_equal")
        }
        (Literal::Null | Literal::Undefined | Literal::Bool(_), Op::StrictEq) => {
            Some("quench_dyn_dead_condition_local_immediate_strict_equal")
        }
        (Literal::Null | Literal::Undefined | Literal::Bool(_), Op::StrictNe) => {
            Some("quench_dyn_dead_condition_local_immediate_strict_not_equal")
        }
        _ => None,
    }
}

#[cfg(target_arch = "aarch64")]
fn register_is_read_only_at(code: &DynCode, register: Register, permitted_pcs: &[usize]) -> bool {
    code.ops.iter().enumerate().all(|(pc, instruction)| {
        !op_reads_register(&instruction.op, register) || permitted_pcs.contains(&pc)
    })
}

#[cfg(target_arch = "aarch64")]
fn op_reads_register(op: &DynOp, register: Register) -> bool {
    op.reads_register(register)
}

#[cfg(target_arch = "aarch64")]
fn op_writes_register(op: &DynOp, register: Register) -> bool {
    op.written_register() == Some(register)
}

#[cfg(target_arch = "aarch64")]
fn allocation_site_shapes(code: &DynCode, plan: &RegionPlan) -> Vec<Option<ShapeRef>> {
    let mut shapes = vec![None; code.ops.len()];
    for block in plan.blocks() {
        for pc in block.start..block.end {
            if let DynOp::NewObjectFromRegisters { keys, .. } = &code.ops[pc].op {
                shapes[pc] = Some(intern_shape(keys));
                continue;
            }
            let DynOp::NewObject { dst } = &code.ops[pc].op else {
                continue;
            };
            let dst = *dst;
            let mut keys = Vec::<String>::new();
            for instruction in &code.ops[pc + NEXT_INSTRUCTION_DISTANCE..block.end] {
                if let DynOp::SetStatic { object, key, src } = &instruction.op
                    && *object == dst
                    && *src != dst
                {
                    if !keys.iter().any(|existing| existing == key) {
                        keys.push(key.clone());
                    }
                    continue;
                }
                if op_reads_register(&instruction.op, dst)
                    || op_writes_register(&instruction.op, dst)
                {
                    break;
                }
            }
            if !keys.is_empty() {
                shapes[pc] = Some(intern_shape(&keys));
            }
        }
    }
    shapes
}

#[cfg(target_arch = "aarch64")]
fn terminal_constructor_shape(code: &DynCode, this_slot: usize) -> Option<ShapeRef> {
    let [
        first_receiver_load,
        first_value_load,
        first_set,
        second_receiver_load,
        second_value_load,
        second_set,
        return_,
    ] = code.ops.as_slice()
    else {
        return None;
    };
    let DynOp::LoadLocal {
        dst: first_receiver,
        slot: first_receiver_slot,
    } = first_receiver_load.op
    else {
        return None;
    };
    let DynOp::LoadLocal {
        dst: first_value, ..
    } = first_value_load.op
    else {
        return None;
    };
    let DynOp::SetStatic {
        object: first_object,
        key: ref first_key,
        src: first_source,
    } = first_set.op
    else {
        return None;
    };
    let DynOp::LoadLocal {
        dst: second_receiver,
        slot: second_receiver_slot,
    } = second_receiver_load.op
    else {
        return None;
    };
    let DynOp::LoadLocal {
        dst: second_value, ..
    } = second_value_load.op
    else {
        return None;
    };
    let DynOp::SetStatic {
        object: second_object,
        key: ref second_key,
        src: second_source,
    } = second_set.op
    else {
        return None;
    };
    if first_receiver_slot != this_slot
        || second_receiver_slot != this_slot
        || first_receiver != first_object
        || first_value != first_source
        || second_receiver != second_object
        || second_value != second_source
        || first_key == second_key
        || !matches!(return_.op, DynOp::Return { src: None })
    {
        return None;
    }
    Some(intern_shape(&[first_key.clone(), second_key.clone()]))
}

#[cfg(target_arch = "aarch64")]
fn build_aarch64(
    code: DynCode,
    arena: &mut CodeArena,
    instrumented_kernels: bool,
) -> Option<DynJitCode> {
    if code.ops.is_empty() || code.ops.len() > MAX_EMBEDDED_PC {
        return None;
    }
    let plan =
        RegionPlan::quote(&code).unwrap_or_else(|error| panic!("invalid RegionPlan: {error}"));
    trace_block_shapes(&code, &plan);
    let direct_enabled = direct_opcode_stencils_enabled(instrumented_kernels);
    let region_stats_enabled = numeric_region::stats_enabled();
    let region_trace_enabled = numeric_region::trace_enabled();
    let effect_reentry_stats_enabled = effect_reentry_stats_enabled();
    let direct_call_stats_enabled = direct_call_stats_enabled();
    let native_path_stats_enabled = native_path_stats_enabled();
    let residual_block_stats_enabled = std::env::var_os(RESIDUAL_BLOCK_STATS_ENV).is_some();
    let mut regions = if direct_enabled {
        numeric_region::analyze(&code)
            .loops
            .into_iter()
            .map(|loop_quote| {
                let quote = if numeric_region::plan_register_region(&loop_quote).is_ok() {
                    loop_quote
                } else {
                    let adjacent = plan
                        .blocks()
                        .iter()
                        .find(|block| block.end == loop_quote.start && block.start < block.end)
                        .filter(|block| {
                            numeric_region::quote_block(&code, block.start, block.end).is_ok()
                        })
                        .and_then(|block| {
                            numeric_region::quote_adjacent_loop(
                                &code,
                                block.start,
                                loop_quote.start,
                                loop_quote.end,
                            )
                            .ok()
                        });
                    adjacent.unwrap_or(loop_quote)
                };
                let register_plan = register_region_plan_with_stats(&quote, region_stats_enabled);
                NumericRegionLink {
                    guard: numeric_region::GuardPlan::from_region(&quote),
                    quote,
                    register_plan,
                    level: StencilLevel::Loop,
                }
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let block_region_count = if direct_enabled
        && MAX_STATIC_BLOCK_VERSIONS > GENERIC_BLOCK_VERSION_COUNT
    {
        let block_regions = plan
            .blocks()
            .iter()
            .filter(|block| {
                !regions
                    .iter()
                    .any(|region| block.start < region.quote.end && region.quote.start < block.end)
            })
            .filter_map(|block| numeric_region::quote_block(&code, block.start, block.end).ok())
            .map(|quote| {
                let register_plan = register_region_plan_with_stats(&quote, region_stats_enabled);
                NumericRegionLink {
                    guard: numeric_region::GuardPlan::from_region(&quote),
                    quote,
                    register_plan,
                    level: StencilLevel::Block,
                }
            })
            .collect::<Vec<_>>();
        let count = block_regions.len();
        regions.extend(block_regions);
        regions.sort_unstable_by_key(|region| region.quote.start);
        count
    } else {
        0
    };
    let prototype_ic_stats_enabled = prototype_ic_stats_enabled();
    let property_ics = (0..code.ops.len())
        .map(|_| PropertyIcSite::new(prototype_ic_stats_enabled))
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let instanceof_ics = (0..code.ops.len())
        .map(|_| InstanceOfIcSite::new())
        .collect::<Vec<_>>()
        .into_boxed_slice();
    #[cfg(not(feature = "inline-census"))]
    let call_ics = code
        .ops
        .iter()
        .enumerate()
        .map(|(pc, instruction)| CallIcSite::new(pc, instruction, direct_call_stats_enabled))
        .collect::<Vec<_>>()
        .into_boxed_slice();
    #[cfg(feature = "inline-census")]
    let call_ics = code
        .ops
        .iter()
        .enumerate()
        .map(|(pc, instruction)| {
            CallIcSite::new_with_source(pc, instruction, direct_call_stats_enabled, code.source_id)
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let mut sites = code
        .ops
        .iter()
        .enumerate()
        .map(|(pc, instruction)| inline_site(pc, &instruction.op))
        .chain(std::iter::once(InlineSite::unused(code.ops.len())))
        .collect::<Vec<_>>();
    for (pc, shape) in allocation_site_shapes(&code, &plan).into_iter().enumerate() {
        if let Some(shape) = shape {
            sites[pc].literal = shape.0 as usize as u64;
        }
    }
    for (pc, instruction) in code.ops.iter().enumerate() {
        if matches!(
            instruction.op,
            DynOp::GetStatic { .. } | DynOp::SetStatic { .. }
        ) {
            sites[pc].literal = std::ptr::from_ref(&property_ics[pc]) as usize as u64;
        }
        if matches!(instruction.op, DynOp::InstanceOf { .. }) {
            sites[pc].literal = std::ptr::from_ref(&instanceof_ics[pc]) as usize as u64;
        }
        if matches!(instruction.op, DynOp::Call { .. }) {
            sites[pc].literal = std::ptr::from_ref(&call_ics[pc].target) as usize as u64;
        }
    }
    for region in &regions {
        for dense in region.guard.dense_sites() {
            sites[dense.pc].literal = dense.array as u64;
        }
        for property in region.guard.property_sites() {
            sites[property.pc].literal =
                (region.guard.dense_view_count() + property.property) as u64;
        }
    }
    for block in plan.blocks() {
        let start = block.start;
        let end = block.end;
        let run_end = (start..end)
            .find(|pc| inline_opcode(&code.ops[*pc].op).is_none())
            .unwrap_or(end);
        sites[start].run_end = run_end;
    }
    let mut sites = sites.into_boxed_slice();
    let (direct_blocks, direct_opcodes) = direct_selection_stats(&code, &plan, direct_enabled);
    let mut name_snapshot_pcs = direct_name_snapshot_pcs(&code, &plan, direct_enabled);
    name_snapshot_pcs.extend(regions.iter().flat_map(|region| {
        region
            .quote
            .operations()
            .into_iter()
            .filter_map(|operation| match operation {
                numeric_region::RegionOp::ReadCaptured { pc, .. } => Some(pc),
                _ => None,
            })
    }));
    name_snapshot_pcs.sort_unstable();
    name_snapshot_pcs.dedup();
    let name_snapshot_plan = name_snapshot_plan(&code, &name_snapshot_pcs);
    for (pc, snapshot_slot) in name_snapshot_plan.site_slots {
        sites[pc].literal = snapshot_slot as u64;
    }
    let name_snapshot_pcs = name_snapshot_plan.representative_pcs.into_boxed_slice();
    let exit = exit_kernel();
    let effect_reentry_blocks = plan
        .blocks()
        .iter()
        .filter(|block| {
            !regions
                .iter()
                .any(|region| (region.quote.start..region.quote.end).contains(&block.start))
                && effect_reentry_block(&code, block.start, block.end, direct_enabled)
        })
        .count();
    if effect_reentry_stats_enabled {
        EFFECT_REENTRY_RUNTIME_STATS
            .linked_blocks
            .fetch_add(effect_reentry_blocks as u64, Ordering::Relaxed);
    }
    if direct_call_stats_enabled {
        let linked_calls = plan
            .blocks()
            .iter()
            .filter(|block| select_call_region(&code, block.start, block.end).is_some())
            .count();
        DIRECT_CALL_RUNTIME_STATS
            .linked_calls
            .fetch_add(linked_calls as u64, Ordering::Relaxed);
    }
    let effect_reentry = (effect_reentry_blocks != 0).then(effect_reentry_kernel);
    let graph = quote_function(
        &code,
        &plan,
        sites.as_ptr(),
        direct_enabled,
        !name_snapshot_pcs.is_empty(),
        instrumented_kernels,
        residual_block_stats_enabled,
        effect_reentry.as_ref().map(|kernel| kernel.entry),
        &regions,
        region_stats_enabled,
        native_path_stats_enabled,
    )
    .then_kernel(exit);
    let image = graph.prefix.image().clone();
    if region_trace_enabled {
        for region in &regions {
            let entry = LabelId(region.quote.start as u32);
            let body = LabelId(REGION_BODY_LABEL_BASE + region.quote.start as u32);
            let entry_offsets = image
                .labels
                .iter()
                .filter(|label| label.id == entry)
                .map(|label| label.offset)
                .collect::<Vec<_>>();
            let body_offsets = image
                .labels
                .iter()
                .filter(|label| label.id == body)
                .map(|label| label.offset)
                .collect::<Vec<_>>();
            eprintln!(
                "NUMERIC_REGION:linked level={:?} start={} end={} ops={} register_plan={:?} rewrites={:?} guards={:?} entry={entry_offsets:?} body={body_offsets:?}",
                region.level,
                region.quote.start,
                region.quote.end,
                region.quote.op_count(),
                region.register_plan.as_ref().map(|plan| (
                    plan.maximum_live_word_lanes,
                    plan.maximum_live_f64_lanes,
                    plan.numeric_operations,
                    plan.conversions,
                    plan.forwarded_local_loads,
                    plan.alias_updates,
                    plan.maximum_location_fanout,
                    plan.steps.len(),
                )),
                region.quote.rewrite_stats(),
                region.guard.requirements(),
            );
            if numeric_region::op_trace_enabled() {
                eprintln!(
                    "NUMERIC_REGION:linked-operations level={:?} start={} {:?}",
                    region.level,
                    region.quote.start,
                    region.quote.operations(),
                );
            }
        }
    }
    let linked = link_dynamic_holes(&image)?;
    let mapped = Rc::new(map_executable(&linked, linked.len())?);
    let base = mapped.ptr as usize;
    let labels = image
        .labels
        .iter()
        .map(|label| (label.id, base + label.offset))
        .collect::<HashMap<_, _>>();
    let guest_entry_label = if direct_enabled {
        GUEST_ENTRY_LABEL
    } else {
        LabelId(FIRST_INSTRUCTION_PC as u32)
    };
    let guest_entry = *labels.get(&guest_entry_label)?;
    let exit = graph.kernel.entry;
    let mut targets = vec![exit; code.ops.len() + 1];
    for (label, address) in labels {
        if (label.0 as usize) < code.ops.len() {
            targets[label.0 as usize] = address;
        }
    }
    let entry = unsafe { std::mem::transmute::<*mut u8, DynEntry>(mapped.ptr) };
    let name_ics = (0..code.ops.len())
        .map(|_| NameIcSite::new())
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let mut region_guards = (0..code.ops.len()).map(|_| None).collect::<Vec<_>>();
    for region in &regions {
        region_guards[region.guard.start] = Some(region.guard.clone());
    }
    let region_guards = region_guards.into_boxed_slice();
    let binding_layout = Rc::new(function_binding_layout(&code));
    let this_slot = binding_layout[THIS_BINDING_NAME];
    let constructor_shape = terminal_constructor_shape(&code, this_slot);
    let arguments_slot = binding_layout[ARGUMENTS_BINDING_NAME];
    let parameter_slots = code
        .params
        .iter()
        .map(|parameter| binding_layout[parameter])
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let uses_arguments = code.ops.iter().any(|instruction| {
        matches!(
            &instruction.op,
            DynOp::LoadName { name, .. } | DynOp::StoreName { name, .. }
                if name == ARGUMENTS_BINDING_NAME
        ) || matches!(
            &instruction.op,
            DynOp::LoadLocal { slot, .. } | DynOp::StoreLocal { slot, .. }
                if *slot == arguments_slot
        )
    });
    let captures_frame = !code.hoisted.is_empty()
        || code.ops.iter().any(|instruction| {
            matches!(
                instruction.op,
                DynOp::MakeClosure { .. } | DynOp::MakeArrow { .. }
            )
        });
    let call_recipe = FunctionCallRecipe {
        entry,
        guest_entry: unsafe { std::mem::transmute::<usize, DynEntry>(guest_entry) },
        layout: CallFrameLayout::new(
            binding_layout.len(),
            code.registers,
            name_snapshot_pcs.len(),
        ),
        parameter_slots: parameter_slots.as_ptr(),
        parameter_count: parameter_slots.len(),
        this_slot,
        arguments_slot,
        uses_arguments: FunctionCallRecipe::flag(uses_arguments),
        captures_frame: FunctionCallRecipe::flag(captures_frame),
    };
    if region_stats_enabled {
        NUMERIC_REGION_RUNTIME_STATS
            .linked_regions
            .fetch_add(regions.len() as u64, Ordering::Relaxed);
        NUMERIC_REGION_RUNTIME_STATS
            .linked_block_regions
            .fetch_add(block_region_count as u64, Ordering::Relaxed);
        let adjacent_regions = regions
            .iter()
            .filter(|region| {
                region
                    .quote
                    .trace_header()
                    .is_some_and(|header| header != region.quote.start)
            })
            .count();
        NUMERIC_REGION_RUNTIME_STATS
            .linked_adjacent_regions
            .fetch_add(adjacent_regions as u64, Ordering::Relaxed);
        for region in &regions {
            if let Some(plan) = &region.register_plan {
                use numeric_region::RegisterRegionStep as RegisterStep;
                NUMERIC_REGION_RUNTIME_STATS
                    .linked_register_regions
                    .fetch_add(1, Ordering::Relaxed);
                NUMERIC_REGION_RUNTIME_STATS
                    .linked_register_conversions
                    .fetch_add(plan.conversions as u64, Ordering::Relaxed);
                NUMERIC_REGION_RUNTIME_STATS
                    .linked_register_forwarded_local_loads
                    .fetch_add(plan.forwarded_local_loads as u64, Ordering::Relaxed);
                NUMERIC_REGION_RUNTIME_STATS
                    .linked_register_alias_updates
                    .fetch_add(plan.alias_updates as u64, Ordering::Relaxed);
                NUMERIC_REGION_RUNTIME_STATS
                    .linked_register_maximum_location_fanout
                    .fetch_max(plan.maximum_location_fanout as u64, Ordering::Relaxed);
                let word_literals = plan
                    .steps
                    .iter()
                    .filter(|step| matches!(step, RegisterStep::LoadWordLiteral { .. }))
                    .count();
                let word_loads = plan
                    .steps
                    .iter()
                    .filter(|step| {
                        matches!(
                            step,
                            RegisterStep::LoadWordLocal { .. } | RegisterStep::ReadDenseWord { .. }
                        )
                    })
                    .count();
                NUMERIC_REGION_RUNTIME_STATS
                    .linked_register_word_literals
                    .fetch_add(word_literals as u64, Ordering::Relaxed);
                NUMERIC_REGION_RUNTIME_STATS
                    .linked_register_word_loads
                    .fetch_add(word_loads as u64, Ordering::Relaxed);
            }
            let rewrites = region.quote.rewrite_stats();
            NUMERIC_REGION_RUNTIME_STATS
                .linked_forwarded_local_loads
                .fetch_add(rewrites.local_loads_forwarded as u64, Ordering::Relaxed);
            NUMERIC_REGION_RUNTIME_STATS
                .linked_reused_captured_loads
                .fetch_add(rewrites.captured_loads_reused as u64, Ordering::Relaxed);
            NUMERIC_REGION_RUNTIME_STATS
                .linked_reused_literals
                .fetch_add(rewrites.literals_reused as u64, Ordering::Relaxed);
            NUMERIC_REGION_RUNTIME_STATS
                .linked_propagated_copies
                .fetch_add(rewrites.copies_propagated as u64, Ordering::Relaxed);
            NUMERIC_REGION_RUNTIME_STATS
                .linked_reused_pure_expressions
                .fetch_add(rewrites.pure_expressions_reused as u64, Ordering::Relaxed);
            NUMERIC_REGION_RUNTIME_STATS
                .linked_reused_heap_loads
                .fetch_add(rewrites.heap_loads_reused as u64, Ordering::Relaxed);
            NUMERIC_REGION_RUNTIME_STATS
                .linked_eliminated_local_stores
                .fetch_add(rewrites.local_stores_eliminated as u64, Ordering::Relaxed);
        }
    }
    numeric_region::trace(&code);
    let has_loop = code.blocks.iter().any(|block| block.2);
    arena.adopt(mapped.clone());
    Some(DynJitCode {
        call_recipe,
        constructor_shape,
        code: Rc::new(code),
        targets,
        name_ics,
        property_ics,
        _instanceof_ics: instanceof_ics,
        call_ics,
        region_guards,
        sites,
        name_snapshot_pcs,
        binding_layout,
        _parameter_slots: parameter_slots,
        code_bytes: image.bytes.len(),
        direct_blocks,
        direct_opcodes,
        has_loop,
        region_stats_enabled,
        region_trace_enabled,
        effect_reentry_stats_enabled,
        direct_call_stats_enabled,
        _exit_kernel: graph.kernel,
        _effect_reentry_kernel: effect_reentry,
    })
}

#[cfg(target_arch = "aarch64")]
fn direct_opcode_stencils_enabled(instrumented_kernels: bool) -> bool {
    !instrumented_kernels
        && std::env::var_os(BLOCK_KERNEL_ONLY_ENV).is_none()
        && std::env::var_os(STENCIL_COVERAGE_ENV).is_none()
}

#[cfg(target_arch = "aarch64")]
fn direct_selection_stats(code: &DynCode, plan: &RegionPlan, enabled: bool) -> (usize, usize) {
    if !enabled {
        return (0, 0);
    }
    plan.blocks()
        .iter()
        .filter_map(|block| {
            (select_direct_block_template(code, block.start, block.end).is_some()
                || code.ops[block.start..block.end]
                    .iter()
                    .all(|instruction| select_direct_opcode_template(&instruction.op).is_some()))
            .then_some((1usize, block.end - block.start))
        })
        .fold((0, 0), |(blocks, opcodes), (block, opcode_count)| {
            (blocks + block, opcodes + opcode_count)
        })
}

#[cfg(target_arch = "aarch64")]
fn direct_name_snapshot_pcs(code: &DynCode, plan: &RegionPlan, enabled: bool) -> Vec<usize> {
    if !enabled {
        return Vec::new();
    }
    plan.blocks()
        .iter()
        .filter_map(|block| {
            matches!(
                select_direct_block_template(code, block.start, block.end),
                Some(DirectBlockTemplate::ConditionLocalName { .. })
            )
            .then_some(block.start + NAME_OPERAND_PC_OFFSET)
        })
        .collect()
}

#[cfg(target_arch = "aarch64")]
fn quote_function(
    code: &DynCode,
    plan: &RegionPlan,
    sites: *const InlineSite,
    direct_enabled: bool,
    refresh_name_snapshots: bool,
    instrumented_kernels: bool,
    residual_block_stats_enabled: bool,
    effect_reentry_entry: Option<usize>,
    regions: &[NumericRegionLink],
    region_stats_enabled: bool,
    native_path_stats_enabled: bool,
) -> Stencil<Connector, Connector> {
    let prologue = if direct_enabled {
        Stencil::instantiate(direct_prologue_template().instantiate(
            vec![CopyPatch::pointer(
                PROLOGUE_SITE_LITERAL_OFFSET,
                sites as usize,
            )],
            Vec::new(),
            Vec::new(),
        ))
    } else {
        Stencil::instantiate(prologue_template().instantiate(Vec::new(), Vec::new(), Vec::new()))
    };
    let body = quote_body(
        code,
        plan,
        regions,
        direct_enabled,
        refresh_name_snapshots,
        instrumented_kernels,
        residual_block_stats_enabled,
        region_stats_enabled,
        native_path_stats_enabled,
    );
    let function = if direct_enabled {
        let function = prologue
            + body
            + slow_adapter_leaf(instrumented_kernels, residual_block_stats_enabled)
                .labeled(SLOW_PATH_LABEL)
            + exit_adapter_leaf().labeled(FUNCTION_EXIT_LABEL)
            + direct_call_exception_adapter_leaf().labeled(DIRECT_CALL_EXCEPTION_LABEL)
            + numeric_region_fallbacks(
                code,
                regions,
                refresh_name_snapshots,
                instrumented_kernels,
                residual_block_stats_enabled,
                native_path_stats_enabled,
            );
        let function = function + guest_entry_adapter_leaf().labeled(GUEST_ENTRY_LABEL);
        effect_reentry_entry.map_or(function.clone(), |entry| {
            function + effect_reentry_adapter_leaf(entry).labeled(EFFECT_REENTRY_LABEL)
        })
    } else {
        prologue + body
    };
    let root_level = if code.is_script {
        StencilLevel::Program
    } else {
        StencilLevel::Function
    };
    function.region(
        root_level,
        StencilFamily::Branch,
        FIRST_INSTRUCTION_PC,
        code.ops.len(),
    )
}

#[cfg(target_arch = "aarch64")]
fn quote_body(
    code: &DynCode,
    plan: &RegionPlan,
    regions: &[NumericRegionLink],
    direct_enabled: bool,
    refresh_name_snapshots: bool,
    instrumented_kernels: bool,
    residual_block_stats_enabled: bool,
    region_stats_enabled: bool,
    native_path_stats_enabled: bool,
) -> Stencil<Connector, Connector> {
    let mut function = identity::<Connector>();
    let mut block_index = 0;
    while block_index < plan.blocks().len() {
        let start = plan.blocks()[block_index].start;
        if let Some(region) = regions.iter().find(|region| region.quote.start == start) {
            function = function
                + numeric_region_stencil(
                    code,
                    region,
                    region_stats_enabled,
                    native_path_stats_enabled,
                );
            while block_index < plan.blocks().len()
                && plan.blocks()[block_index].start < region.quote.end
            {
                block_index += 1;
            }
            continue;
        }
        let block = plan.blocks()[block_index].legacy_tuple();
        function = function
            + regular_block_stencil(
                code,
                block,
                direct_enabled,
                refresh_name_snapshots,
                instrumented_kernels,
                residual_block_stats_enabled,
                native_path_stats_enabled,
            );
        block_index += 1;
    }
    function
}

#[cfg(target_arch = "aarch64")]
fn regular_block_stencil(
    code: &DynCode,
    block: (usize, usize, bool),
    direct_enabled: bool,
    refresh_name_snapshots: bool,
    instrumented_kernels: bool,
    residual_block_stats_enabled: bool,
    native_path_stats_enabled: bool,
) -> Stencil<Connector, Connector> {
    let (start, end, loop_region) = block;
    let level = if loop_region {
        StencilLevel::Loop
    } else {
        StencilLevel::Block
    };
    let call_region = direct_enabled
        .then(|| select_call_region(code, start, end))
        .flatten();
    let stencil = call_region.map_or_else(
        || {
            non_call_block_stencil(
                code,
                start,
                end,
                direct_enabled,
                refresh_name_snapshots,
                instrumented_kernels,
                residual_block_stats_enabled,
                native_path_stats_enabled,
            )
        },
        |region| call_region_stencil(code, region, native_path_stats_enabled),
    );
    stencil.region(level, code.ops[start].op.stencil_family(), start, end)
}

#[cfg(target_arch = "aarch64")]
fn call_region_stencil(
    code: &DynCode,
    region: CallRegion,
    native_path_stats_enabled: bool,
) -> Stencil<Connector, Connector> {
    if let Some(get_pc) = select_get_static_load_local_call(code, region.call_pc)
        && get_pc >= region.start
    {
        let prefix = direct_opcode_sequence(code, region.start, get_pc, native_path_stats_enabled)
            .expect("selected one-argument method-call prefix has total direct cover");
        let continuation = direct_opcode_sequence(
            code,
            region.call_pc + NEXT_INSTRUCTION_DISTANCE,
            region.end,
            native_path_stats_enabled,
        )
        .expect("selected one-argument method-call continuation has total direct cover");
        return prefix
            + get_static_load_local_call_leaf(get_pc, native_path_stats_enabled)
            + continuation;
    }
    if let Some(get_pc) = select_get_static_call_pair(code, region.call_pc)
        && get_pc >= region.start
    {
        let prefix = direct_opcode_sequence(code, region.start, get_pc, native_path_stats_enabled)
            .expect("selected method-call prefix has total direct cover");
        let continuation = direct_opcode_sequence(
            code,
            region.call_pc + NEXT_INSTRUCTION_DISTANCE,
            region.end,
            native_path_stats_enabled,
        )
        .expect("selected method-call continuation has total direct cover");
        return prefix + get_static_call_leaf(get_pc, native_path_stats_enabled) + continuation;
    }
    let prefix = direct_opcode_sequence(
        code,
        region.start,
        region.call_pc,
        native_path_stats_enabled,
    )
    .expect("selected call-region prefix has total direct cover");
    let continuation = direct_opcode_sequence(
        code,
        region.call_pc + NEXT_INSTRUCTION_DISTANCE,
        region.end,
        native_path_stats_enabled,
    )
    .expect("selected call-region continuation has total direct cover");
    prefix + direct_call_leaf(region.call_pc, native_path_stats_enabled) + continuation
}

#[cfg(target_arch = "aarch64")]
fn direct_call_leaf(pc: usize, native_path_stats_enabled: bool) -> Stencil<Connector, Connector> {
    let leaf = rustc_dyn_stencil(
        "quench_dyn_direct_call",
        None,
        Some(DIRECT_CALL_EXCEPTION_LABEL),
    );
    observed_labeled_leaf(
        pc,
        native_path_stats_enabled,
        &NATIVE_PATH_RUNTIME_STATS.direct_call_entries,
        leaf,
    )
}

#[cfg(target_arch = "aarch64")]
fn get_static_call_leaf(
    pc: usize,
    native_path_stats_enabled: bool,
) -> Stencil<Connector, Connector> {
    let leaf = rustc_dyn_stencil_with_operands_and_site_advance(
        "quench_dyn_get_static_call",
        None,
        Some(DIRECT_CALL_EXCEPTION_LABEL),
        Vec::new(),
        Vec::new(),
        GET_STATIC_CALL_INSTRUCTION_COUNT * NEXT_INSTRUCTION_DISTANCE,
    );
    observed_labeled_leaf(
        pc,
        native_path_stats_enabled,
        &NATIVE_PATH_RUNTIME_STATS.direct_call_entries,
        leaf,
    )
}

#[cfg(target_arch = "aarch64")]
fn get_static_load_local_call_leaf(
    pc: usize,
    native_path_stats_enabled: bool,
) -> Stencil<Connector, Connector> {
    let leaf = rustc_dyn_stencil_with_operands_and_site_advance(
        "quench_dyn_get_static_load_local_call",
        None,
        Some(DIRECT_CALL_EXCEPTION_LABEL),
        Vec::new(),
        Vec::new(),
        GET_STATIC_LOAD_LOCAL_CALL_INSTRUCTION_COUNT * NEXT_INSTRUCTION_DISTANCE,
    );
    observed_labeled_leaf(
        pc,
        native_path_stats_enabled,
        &NATIVE_PATH_RUNTIME_STATS.direct_call_entries,
        leaf,
    )
}

#[cfg(target_arch = "aarch64")]
fn non_call_block_stencil(
    code: &DynCode,
    start: usize,
    end: usize,
    direct_enabled: bool,
    refresh_name_snapshots: bool,
    instrumented_kernels: bool,
    residual_block_stats_enabled: bool,
    native_path_stats_enabled: bool,
) -> Stencil<Connector, Connector> {
    let direct_template = direct_enabled
        .then(|| select_direct_block_template(code, start, end))
        .flatten();
    direct_template.map_or_else(
        || {
            if direct_enabled
                && let Some(stencil) =
                    direct_opcode_sequence(code, start, end, native_path_stats_enabled)
            {
                stencil
            } else if effect_reentry_block(code, start, end, direct_enabled) {
                effect_reentry_dispatch_leaf(start, native_path_stats_enabled)
            } else {
                block_leaf(
                    start,
                    end,
                    direct_enabled,
                    refresh_name_snapshots,
                    instrumented_kernels,
                    residual_block_stats_enabled,
                    native_path_stats_enabled,
                )
            }
        },
        |template| direct_block_leaf(start, template, native_path_stats_enabled),
    )
}

#[cfg(target_arch = "aarch64")]
fn direct_opcode_sequence(
    code: &DynCode,
    start: usize,
    end: usize,
    native_path_stats_enabled: bool,
) -> Option<Stencil<Connector, Connector>> {
    code.ops[start..end].iter().enumerate().try_fold(
        identity::<Connector>(),
        |sequence, (offset, instruction)| {
            let pc = start + offset;
            direct_opcode_leaf(
                code.ops.len(),
                pc,
                &instruction.op,
                native_path_stats_enabled,
            )
            .map(|leaf| sequence + leaf)
        },
    )
}

#[cfg(target_arch = "aarch64")]
fn direct_opcode_leaf(
    code_len: usize,
    pc: usize,
    op: &DynOp,
    native_path_stats_enabled: bool,
) -> Option<Stencil<Connector, Connector>> {
    let template = select_direct_opcode_template(op)?;
    let stencil = match template {
        DirectOpcodeTemplate::Next(stencil_name) => rustc_dyn_stencil(stencil_name, None, None),
        DirectOpcodeTemplate::Transfer {
            stencil_name,
            target,
        } => rustc_dyn_stencil(
            stencil_name,
            Some(bytecode_target_label(code_len, target)),
            None,
        ),
        DirectOpcodeTemplate::Branch {
            stencil_name,
            target,
        } => rustc_dyn_stencil(
            stencil_name,
            None,
            Some(bytecode_target_label(code_len, target)),
        ),
        DirectOpcodeTemplate::Return(stencil_name) => {
            rustc_dyn_stencil(stencil_name, Some(FUNCTION_EXIT_LABEL), None)
        }
    };
    Some(observed_labeled_leaf(
        pc,
        native_path_stats_enabled,
        &NATIVE_PATH_RUNTIME_STATS.direct_opcode_entries,
        stencil,
    ))
}

#[cfg(target_arch = "aarch64")]
fn effect_reentry_block(code: &DynCode, start: usize, end: usize, enabled: bool) -> bool {
    enabled
        && end == start + NEXT_INSTRUCTION_DISTANCE
        && select_direct_block_template(code, start, end).is_none()
}

#[cfg(target_arch = "aarch64")]
const REGION_BODY_LABEL_BASE: u32 = MAX_EMBEDDED_PC as u32 + 1;
#[cfg(target_arch = "aarch64")]
const REGISTER_REGION_EXIT_LABEL_BASE: u32 = REGION_BODY_LABEL_BASE + MAX_EMBEDDED_PC as u32 + 1;

#[cfg(target_arch = "aarch64")]
fn register_region_exit_label(target: usize) -> LabelId {
    LabelId(REGISTER_REGION_EXIT_LABEL_BASE + target as u32)
}

#[cfg(target_arch = "aarch64")]
fn numeric_region_stencil(
    code: &DynCode,
    region: &NumericRegionLink,
    region_stats_enabled: bool,
    native_path_stats_enabled: bool,
) -> Stencil<Connector, Connector> {
    if let Some(plan) = &region.register_plan {
        return register_numeric_region_stencil(
            code.ops.len(),
            region,
            plan,
            native_path_stats_enabled,
        );
    }
    let guard = observed_labeled_leaf(
        region.quote.start,
        native_path_stats_enabled,
        &NATIVE_PATH_RUNTIME_STATS.numeric_region_entries,
        rustc_dyn_stencil("quench_region_guard", None, None),
    );
    let operations = region.quote.operations();
    let mut body = identity::<Connector>();
    let mut operation_index = 0;
    while operation_index < operations.len() {
        if let Some(supernode) = select_numeric_region_condition_supernode(
            code,
            &operations,
            operation_index,
            region.quote.start,
            region.quote.end,
            &region.guard,
        ) {
            if region_stats_enabled {
                NUMERIC_REGION_RUNTIME_STATS
                    .linked_condition_supernodes
                    .fetch_add(1, Ordering::Relaxed);
            }
            let label = LabelId(REGION_BODY_LABEL_BASE + supernode.pc as u32);
            body = body
                + rustc_dyn_stencil(supernode.stencil_name, None, Some(supernode.target))
                    .labeled(label);
            operation_index += CONDITION_INSTRUCTION_COUNT;
            continue;
        }
        if let Some(supernode) = select_numeric_region_update_supernode(
            code,
            &operations,
            operation_index,
            region.quote.start,
            region.quote.end,
            &region.guard,
            region_stats_enabled,
        ) {
            if region_stats_enabled {
                NUMERIC_REGION_RUNTIME_STATS
                    .linked_update_supernodes
                    .fetch_add(1, Ordering::Relaxed);
            }
            let label = LabelId(REGION_BODY_LABEL_BASE + supernode.pc as u32);
            body = body
                + rustc_dyn_stencil(supernode.stencil_name, Some(supernode.target), None)
                    .labeled(label);
            operation_index += UPDATE_INSTRUCTION_COUNT;
            continue;
        }
        if let Some(supernode) = select_numeric_region_local_update_supernode(
            code,
            &operations,
            operation_index,
            &region.guard,
        ) {
            if region_stats_enabled {
                NUMERIC_REGION_RUNTIME_STATS
                    .linked_update_supernodes
                    .fetch_add(1, Ordering::Relaxed);
            }
            let label = LabelId(REGION_BODY_LABEL_BASE + supernode.pc as u32);
            body = body + rustc_dyn_stencil(supernode.stencil_name, None, None).labeled(label);
            operation_index += LOCAL_UPDATE_INSTRUCTION_COUNT;
            continue;
        }
        if region_stats_enabled
            && matches!(
                operations[operation_index],
                numeric_region::RegionOp::ReadDense { pc, .. }
                    | numeric_region::RegionOp::WriteDense { pc, .. }
                    if region.guard.has_proven_index(pc)
            )
        {
            NUMERIC_REGION_RUNTIME_STATS
                .linked_proven_index_stencils
                .fetch_add(1, Ordering::Relaxed);
        }
        let (next_operation_index, site_advance) =
            numeric_region_leaf_advance(&operations, operation_index);
        body = body
            + numeric_region_leaf(
                &operations[operation_index],
                region.quote.start,
                region.quote.end,
                &region.guard,
                region_stats_enabled,
                site_advance,
            );
        operation_index = next_operation_index;
    }
    (guard + body).region(
        region.level,
        StencilFamily::Element,
        region.quote.start,
        region.quote.end,
    )
}

#[cfg(target_arch = "aarch64")]
fn register_numeric_region_stencil(
    code_len: usize,
    region: &NumericRegionLink,
    plan: &numeric_region::RegisterRegionPlan,
    native_path_stats_enabled: bool,
) -> Stencil<Connector, Connector> {
    let guard = observed_labeled_leaf(
        region.quote.start,
        native_path_stats_enabled,
        &NATIVE_PATH_RUNTIME_STATS.numeric_region_entries,
        rustc_dyn_stencil("quench_region_guard", None, None),
    );
    let enter = register_region_enter_leaf();
    let mut body = identity::<RegisterRegionConnector>();
    let mut previous_pc = None;
    for step in plan.steps.iter() {
        let label_pc = previous_pc != Some(step.pc());
        body = body + register_region_step_leaf(step, plan, &region.guard, label_pc);
        previous_pc = Some(step.pc());
    }
    let leave = register_region_leave_leaf(bytecode_target_label(code_len, plan.exit))
        .labeled(register_region_exit_label(plan.exit));
    (guard + enter + body + leave).region(
        region.level,
        StencilFamily::Element,
        region.quote.start,
        region.quote.end,
    )
}

#[cfg(target_arch = "aarch64")]
fn register_region_enter_leaf() -> Stencil<Connector, RegisterRegionConnector> {
    rustc_typed_stencil_with_site_advance(
        "quench_register_region_enter",
        None,
        None,
        Vec::new(),
        Vec::new(),
        FIRST_INSTRUCTION_PC,
    )
}

#[cfg(target_arch = "aarch64")]
fn register_region_leave_leaf(target: LabelId) -> Stencil<RegisterRegionConnector, Connector> {
    rustc_typed_stencil_with_site_advance(
        "quench_register_region_leave",
        Some(target),
        None,
        Vec::new(),
        Vec::new(),
        FIRST_INSTRUCTION_PC,
    )
}

#[cfg(target_arch = "aarch64")]
fn register_region_step_leaf(
    step: &numeric_region::RegisterRegionStep,
    plan: &numeric_region::RegisterRegionPlan,
    guard: &numeric_region::GuardPlan,
    label_pc: bool,
) -> Stencil<RegisterRegionConnector, RegisterRegionConnector> {
    use numeric_region::RegisterConversion as Conversion;
    use numeric_region::RegisterLocation as Location;
    use numeric_region::RegisterRegionStep as Step;

    let (name, branch_target) = match step {
        Step::Nop { .. } => ("quench_register_region_nop".to_owned(), None),
        Step::CopyLoadLocal { .. } => ("quench_register_region_copy_load_local".to_owned(), None),
        Step::LoadLocal { destination, .. } => (
            format!("quench_register_region_load_local_d{destination}"),
            None,
        ),
        Step::LoadWordLocal { destination, .. } => (
            format!("quench_register_region_load_word_local_w{destination}"),
            None,
        ),
        Step::LoadLiteral { destination, .. } => (
            format!("quench_register_region_load_literal_d{destination}"),
            None,
        ),
        Step::LoadWordLiteral {
            destination, word, ..
        } => {
            let name = format!("quench_register_region_load_word_literal_w{destination}");
            let leaf = rustc_typed_stencil_with_site_advance(
                &name,
                None,
                None,
                Vec::new(),
                vec![RawValueBinding {
                    id: raw_value_holes::WORD32_LITERAL_HOLE_ID,
                    bits: u64::from(*word),
                }],
                step.bytecode_width(),
            );
            return if label_pc {
                leaf.labeled(LabelId(REGION_BODY_LABEL_BASE + step.pc() as u32))
            } else {
                leaf
            };
        }
        Step::LoadName { destination, .. } => (
            format!("quench_register_region_load_name_d{destination}"),
            None,
        ),
        Step::CopyLoadName { .. } => ("quench_register_region_copy_load_name".to_owned(), None),
        Step::StoreLocal { source, .. } => (
            format!("quench_register_region_store_local_s{source}"),
            None,
        ),
        Step::CopyStoreLocal { .. } => ("quench_register_region_copy_store_local".to_owned(), None),
        Step::Move {
            destination,
            source,
            ..
        } => match (*destination, *source) {
            (Location::F64(destination), Location::F64(source)) => (
                format!("quench_register_region_move_f64_d{destination}{source}"),
                None,
            ),
            (
                Location::Word32 {
                    lane: destination, ..
                },
                Location::Word32 { lane: source, .. },
            ) => (
                format!("quench_register_region_move_word_w{destination}{source}"),
                None,
            ),
            _ => unreachable!("register move must preserve its representation"),
        },
        Step::CopyMove { .. } => ("quench_register_region_copy_move".to_owned(), None),
        Step::Convert {
            destination,
            source,
            kind,
            ..
        } => (
            match kind {
                Conversion::F64ToWord32 => {
                    format!("quench_register_region_f64_to_word_w{destination}d{source}")
                }
                Conversion::SignedWord32ToF64 => {
                    format!("quench_register_region_signed_word_to_f64_d{destination}w{source}")
                }
                Conversion::UnsignedWord32ToF64 => {
                    format!("quench_register_region_unsigned_word_to_f64_d{destination}w{source}")
                }
            },
            None,
        ),
        Step::Unary {
            destination,
            source,
            kind,
            ..
        } => match (*destination, *source, *kind) {
            (Location::F64(destination), Location::F64(source), kind) => (
                format!(
                    "quench_register_region_{}_d{destination}{source}",
                    register_region_unary_name(kind)
                ),
                None,
            ),
            (
                Location::Word32 {
                    lane: destination, ..
                },
                Location::Word32 { lane: source, .. },
                numeric_region::NumericUnary::BitNot,
            ) => (
                format!("quench_register_region_bit_not_w{destination}{source}"),
                None,
            ),
            _ => unreachable!("register unary representation does not match its operation"),
        },
        Step::Binary {
            destination,
            left,
            right,
            kind,
            ..
        } => match (*destination, *left, *right) {
            (Location::F64(destination), Location::F64(left), Location::F64(right)) => (
                format!(
                    "quench_register_region_{}_d{destination}{left}{right}",
                    register_region_binary_name(*kind)
                ),
                None,
            ),
            (
                Location::Word32 {
                    lane: destination, ..
                },
                Location::Word32 { lane: left, .. },
                Location::Word32 { lane: right, .. },
            ) => (
                format!(
                    "quench_register_region_{}_w{destination}{left}{right}",
                    register_region_binary_name(*kind)
                ),
                None,
            ),
            _ => unreachable!("register binary operands must use one representation"),
        },
        Step::CompareBranch {
            left,
            right,
            kind,
            target,
            ..
        } => (
            format!(
                "quench_register_region_{}_l{left}{right}",
                register_region_binary_name(*kind)
            ),
            Some(register_region_target(*target, plan)),
        ),
        Step::ReadDense {
            pc,
            destination,
            index,
        } => (
            format!(
                "quench_register_region_read_dense{}_d{destination}{index}",
                if guard.has_proven_index(*pc) {
                    "_proven"
                } else {
                    ""
                }
            ),
            None,
        ),
        Step::ReadDenseWord {
            pc,
            destination,
            index,
        } => (
            format!(
                "quench_register_region_read_dense_word{}_w{destination}d{index}",
                if guard.has_proven_index(*pc) {
                    "_proven"
                } else {
                    ""
                }
            ),
            None,
        ),
        Step::WriteDense { pc, index, source } => (
            format!(
                "quench_register_region_write_dense{}_i{index}{source}",
                if guard.has_proven_index(*pc) {
                    "_proven"
                } else {
                    ""
                }
            ),
            None,
        ),
        Step::LoadStatic { destination, .. } => (
            format!("quench_register_region_load_static_d{destination}"),
            None,
        ),
        Step::CopyReadStatic { .. } => ("quench_register_region_copy_read_static".to_owned(), None),
        Step::WriteStatic { source, .. } => (
            format!("quench_register_region_write_static_s{source}"),
            None,
        ),
        Step::CopyWriteStatic { .. } => {
            ("quench_register_region_copy_write_static".to_owned(), None)
        }
        Step::Jump { target, .. } => (
            "quench_register_region_jump".to_owned(),
            Some(register_region_target(*target, plan)),
        ),
    };
    let next_label = matches!(step, Step::Jump { .. }).then(|| branch_target.unwrap());
    let branch_label = matches!(step, Step::CompareBranch { .. }).then(|| branch_target.unwrap());
    let leaf = rustc_typed_stencil_with_site_advance(
        &name,
        next_label,
        branch_label,
        Vec::new(),
        Vec::new(),
        step.bytecode_width(),
    );
    if label_pc {
        leaf.labeled(LabelId(REGION_BODY_LABEL_BASE + step.pc() as u32))
    } else {
        leaf
    }
}

#[cfg(target_arch = "aarch64")]
fn register_region_target(target: usize, plan: &numeric_region::RegisterRegionPlan) -> LabelId {
    if (plan.start..plan.end).contains(&target) {
        LabelId(REGION_BODY_LABEL_BASE + target as u32)
    } else {
        debug_assert_eq!(target, plan.exit);
        register_region_exit_label(target)
    }
}

#[cfg(target_arch = "aarch64")]
fn register_region_unary_name(kind: numeric_region::NumericUnary) -> &'static str {
    use numeric_region::NumericUnary::*;
    match kind {
        Plus => "unary_plus",
        Negate => "negate",
        BitNot => "bit_not",
    }
}

#[cfg(target_arch = "aarch64")]
fn register_region_binary_name(kind: numeric_region::NumericBinary) -> &'static str {
    use numeric_region::NumericBinary::*;
    match kind {
        Add => "add",
        Subtract => "subtract",
        Multiply => "multiply",
        Divide => "divide",
        Equal | StrictEqual => "equal",
        NotEqual | StrictNotEqual => "not_equal",
        Less => "less",
        LessEqual => "less_equal",
        Greater => "greater",
        GreaterEqual => "greater_equal",
        ShiftLeft => "shift_left",
        ShiftRight => "shift_right",
        ShiftRightUnsigned => "shift_right_unsigned",
        BitOr => "bit_or",
        BitXor => "bit_xor",
        BitAnd => "bit_and",
    }
}

#[cfg(target_arch = "aarch64")]
fn numeric_region_leaf_advance(
    operations: &[numeric_region::RegionOp],
    operation_index: usize,
) -> (usize, usize) {
    use numeric_region::RegionOp as Op;

    let current = &operations[operation_index];
    if matches!(current, Op::Jump { .. } | Op::JumpIfFalse { .. }) {
        return (
            operation_index + NEXT_INSTRUCTION_DISTANCE,
            NEXT_INSTRUCTION_DISTANCE,
        );
    }

    let mut next_index = operation_index + NEXT_INSTRUCTION_DISTANCE;
    while matches!(operations.get(next_index), Some(Op::Elided { .. })) {
        next_index += NEXT_INSTRUCTION_DISTANCE;
    }
    let current_pc = numeric_region_pc(current);
    let next_pc = operations
        .get(next_index)
        .map(numeric_region_pc)
        .unwrap_or_else(|| current_pc + next_index - operation_index);
    let site_advance = next_pc
        .checked_sub(current_pc)
        .expect("quoted numeric operations remain in bytecode order");
    let byte_advance = site_advance
        .checked_mul(std::mem::size_of::<InlineSite>())
        .expect("site advance does not overflow");
    if byte_advance > site_holes::AARCH64_ADD_IMMEDIATE_MAX {
        return (
            operation_index + NEXT_INSTRUCTION_DISTANCE,
            NEXT_INSTRUCTION_DISTANCE,
        );
    }
    (next_index, site_advance)
}

#[cfg(target_arch = "aarch64")]
#[derive(Clone, Copy)]
enum NumericRegionConditionSource {
    Local,
    Literal,
    Captured,
}

#[cfg(target_arch = "aarch64")]
struct NumericRegionConditionSupernode {
    pc: usize,
    target: LabelId,
    stencil_name: &'static str,
}

#[cfg(target_arch = "aarch64")]
fn select_numeric_region_condition_supernode(
    code: &DynCode,
    operations: &[numeric_region::RegionOp],
    operation_index: usize,
    region_start: usize,
    region_end: usize,
    guard: &numeric_region::GuardPlan,
) -> Option<NumericRegionConditionSupernode> {
    use numeric_region::RegionOp as Op;

    let end = operation_index.checked_add(CONDITION_INSTRUCTION_COUNT)?;
    let [
        Op::ReadLocal {
            pc,
            dst: left_dst,
            slot: left_slot,
        },
        right_operation,
        Op::Binary {
            pc: compare_pc,
            dst: compare_dst,
            left,
            right,
            kind,
        },
        Op::JumpIfFalse {
            pc: branch_pc,
            test,
            target,
        },
    ] = operations.get(operation_index..end)?
    else {
        return None;
    };

    let (right_pc, right_dst, source, right_is_proven) = match right_operation {
        Op::ReadLocal { pc, dst, slot } => (
            *pc,
            *dst,
            NumericRegionConditionSource::Local,
            guard.proves_local_load(*pc, *slot, numeric_region::GuardKind::Number),
        ),
        Op::NumberLiteral { pc, dst, .. } => {
            (*pc, *dst, NumericRegionConditionSource::Literal, true)
        }
        Op::ReadCaptured { pc, dst, name } => (
            *pc,
            *dst,
            NumericRegionConditionSource::Captured,
            guard.proves(
                &numeric_region::GuardSource::Captured(name.clone()),
                numeric_region::GuardKind::Number,
            ),
        ),
        _ => return None,
    };

    let pcs_are_consecutive = right_pc == pc.checked_add(NEXT_INSTRUCTION_DISTANCE)?
        && *compare_pc == pc.checked_add(CONDITION_COMPARE_PC_OFFSET)?
        && *branch_pc == pc.checked_add(CONDITION_BRANCH_PC_OFFSET)?;
    let operands_match = left == left_dst && *right == right_dst && test == compare_dst;
    if !pcs_are_consecutive || !operands_match {
        return None;
    }

    if !guard.proves_local_load(*pc, *left_slot, numeric_region::GuardKind::Number)
        || !right_is_proven
    {
        return None;
    }

    let interior_start = pc.checked_add(NEXT_INSTRUCTION_DISTANCE)?;
    let interior_end = pc.checked_add(CONDITION_INSTRUCTION_COUNT)?;
    let has_interior_entry = operations.iter().any(|operation| {
        let target = match operation {
            Op::Jump { target, .. } | Op::JumpIfFalse { target, .. } => *target,
            _ => return false,
        };
        (interior_start..interior_end).contains(&target)
    });
    let intermediates_are_dead = register_is_read_only_at(code, *left_dst, &[*compare_pc])
        && register_is_read_only_at(code, right_dst, &[*compare_pc])
        && register_is_read_only_at(code, *compare_dst, &[*branch_pc]);
    if has_interior_entry || !intermediates_are_dead {
        return None;
    }

    Some(NumericRegionConditionSupernode {
        pc: *pc,
        target: numeric_region_target(*target, region_start, region_end),
        stencil_name: numeric_region_condition_stencil_name(source, *kind)?,
    })
}

#[cfg(target_arch = "aarch64")]
fn numeric_region_condition_stencil_name(
    source: NumericRegionConditionSource,
    kind: numeric_region::NumericBinary,
) -> Option<&'static str> {
    use NumericRegionConditionSource as Source;
    use numeric_region::NumericBinary as Binary;

    Some(match (source, kind) {
        (Source::Local, Binary::Equal | Binary::StrictEqual) => {
            "quench_region_dead_condition_local_local_number_equal"
        }
        (Source::Local, Binary::NotEqual | Binary::StrictNotEqual) => {
            "quench_region_dead_condition_local_local_number_not_equal"
        }
        (Source::Local, Binary::Less) => "quench_region_dead_condition_local_local_number_less",
        (Source::Local, Binary::LessEqual) => {
            "quench_region_dead_condition_local_local_number_less_equal"
        }
        (Source::Local, Binary::Greater) => {
            "quench_region_dead_condition_local_local_number_greater"
        }
        (Source::Local, Binary::GreaterEqual) => {
            "quench_region_dead_condition_local_local_number_greater_equal"
        }
        (Source::Literal, Binary::Equal | Binary::StrictEqual) => {
            "quench_region_dead_condition_local_literal_number_equal"
        }
        (Source::Literal, Binary::NotEqual | Binary::StrictNotEqual) => {
            "quench_region_dead_condition_local_literal_number_not_equal"
        }
        (Source::Literal, Binary::Less) => "quench_region_dead_condition_local_literal_number_less",
        (Source::Literal, Binary::LessEqual) => {
            "quench_region_dead_condition_local_literal_number_less_equal"
        }
        (Source::Literal, Binary::Greater) => {
            "quench_region_dead_condition_local_literal_number_greater"
        }
        (Source::Literal, Binary::GreaterEqual) => {
            "quench_region_dead_condition_local_literal_number_greater_equal"
        }
        (Source::Captured, Binary::Equal | Binary::StrictEqual) => {
            "quench_region_dead_condition_local_name_number_equal"
        }
        (Source::Captured, Binary::NotEqual | Binary::StrictNotEqual) => {
            "quench_region_dead_condition_local_name_number_not_equal"
        }
        (Source::Captured, Binary::Less) => "quench_region_dead_condition_local_name_number_less",
        (Source::Captured, Binary::LessEqual) => {
            "quench_region_dead_condition_local_name_number_less_equal"
        }
        (Source::Captured, Binary::Greater) => {
            "quench_region_dead_condition_local_name_number_greater"
        }
        (Source::Captured, Binary::GreaterEqual) => {
            "quench_region_dead_condition_local_name_number_greater_equal"
        }
        _ => return None,
    })
}

#[cfg(target_arch = "aarch64")]
struct NumericRegionUpdateSupernode {
    pc: usize,
    target: LabelId,
    stencil_name: &'static str,
}

#[cfg(target_arch = "aarch64")]
struct NumericRegionLocalUpdateSupernode {
    pc: usize,
    stencil_name: &'static str,
}

#[cfg(target_arch = "aarch64")]
fn select_numeric_region_local_update_supernode(
    code: &DynCode,
    operations: &[numeric_region::RegionOp],
    operation_index: usize,
    guard: &numeric_region::GuardPlan,
) -> Option<NumericRegionLocalUpdateSupernode> {
    use numeric_region::RegionOp as Op;

    let end = operation_index.checked_add(LOCAL_UPDATE_INSTRUCTION_COUNT)?;
    let [
        Op::ReadLocal {
            pc,
            dst: load_dst,
            slot: load_slot,
        },
        Op::NumberLiteral {
            pc: literal_pc,
            dst: literal_dst,
            ..
        },
        Op::Binary {
            pc: binary_pc,
            dst: binary_dst,
            left,
            right,
            kind,
        },
        Op::WriteLocal {
            pc: store_pc,
            slot: _,
            src,
        },
    ] = operations.get(operation_index..end)?
    else {
        return None;
    };

    let pcs_are_consecutive = *literal_pc == pc.checked_add(UPDATE_LITERAL_PC_OFFSET)?
        && *binary_pc == pc.checked_add(UPDATE_BINARY_PC_OFFSET)?
        && *store_pc == pc.checked_add(UPDATE_STORE_PC_OFFSET)?;
    let operands_form_expression = left == load_dst && right == literal_dst && src == binary_dst;
    if !pcs_are_consecutive || !operands_form_expression {
        return None;
    }

    if !guard.proves_local_load(*pc, *load_slot, numeric_region::GuardKind::Number) {
        return None;
    }

    let interior_start = pc.checked_add(NEXT_INSTRUCTION_DISTANCE)?;
    let interior_end = pc.checked_add(LOCAL_UPDATE_INSTRUCTION_COUNT)?;
    let has_interior_entry = operations.iter().any(|operation| {
        let target = match operation {
            Op::Jump { target, .. } | Op::JumpIfFalse { target, .. } => *target,
            _ => return false,
        };
        (interior_start..interior_end).contains(&target)
    });
    let intermediates_are_dead = register_is_read_only_at(code, *load_dst, &[*binary_pc])
        && register_is_read_only_at(code, *literal_dst, &[*binary_pc])
        && register_is_read_only_at(code, *binary_dst, &[*store_pc]);
    if has_interior_entry || !intermediates_are_dead {
        return None;
    }

    let stencil_name = match kind {
        numeric_region::NumericBinary::Add => "quench_region_dead_local_number_add",
        numeric_region::NumericBinary::Subtract => "quench_region_dead_local_number_subtract",
        _ => return None,
    };
    Some(NumericRegionLocalUpdateSupernode {
        pc: *pc,
        stencil_name,
    })
}

#[cfg(target_arch = "aarch64")]
fn select_numeric_region_update_supernode(
    code: &DynCode,
    operations: &[numeric_region::RegionOp],
    operation_index: usize,
    region_start: usize,
    region_end: usize,
    guard: &numeric_region::GuardPlan,
    region_stats_enabled: bool,
) -> Option<NumericRegionUpdateSupernode> {
    use numeric_region::RegionOp as Op;

    let end = operation_index.checked_add(UPDATE_INSTRUCTION_COUNT)?;
    let [
        Op::ReadLocal {
            pc,
            dst: load_dst,
            slot: load_slot,
        },
        Op::NumberLiteral {
            pc: literal_pc,
            dst: literal_dst,
            ..
        },
        Op::Binary {
            pc: binary_pc,
            dst: binary_dst,
            left,
            right,
            kind,
        },
        Op::WriteLocal {
            pc: store_pc,
            slot: store_slot,
            src,
        },
        Op::Jump {
            pc: jump_pc,
            target,
        },
    ] = operations.get(operation_index..end)?
    else {
        return None;
    };

    let pcs_are_consecutive = *literal_pc == pc.checked_add(UPDATE_LITERAL_PC_OFFSET)?
        && *binary_pc == pc.checked_add(UPDATE_BINARY_PC_OFFSET)?
        && *store_pc == pc.checked_add(UPDATE_STORE_PC_OFFSET)?
        && *jump_pc == pc.checked_add(UPDATE_JUMP_PC_OFFSET)?;
    let operands_form_recurrence = load_slot == store_slot
        && left == load_dst
        && right == literal_dst
        && src == binary_dst
        && *target <= *jump_pc;
    if !pcs_are_consecutive || !operands_form_recurrence {
        return None;
    }

    if !guard.proves_local_load(*pc, *load_slot, numeric_region::GuardKind::Number) {
        return None;
    }

    let interior_start = pc.checked_add(NEXT_INSTRUCTION_DISTANCE)?;
    let interior_end = pc.checked_add(UPDATE_INSTRUCTION_COUNT)?;
    let has_interior_entry = operations.iter().any(|operation| {
        let target = match operation {
            Op::Jump { target, .. } | Op::JumpIfFalse { target, .. } => *target,
            _ => return false,
        };
        (interior_start..interior_end).contains(&target)
    });
    let local_written_elsewhere = operations.iter().enumerate().any(|(index, operation)| {
        index != operation_index + UPDATE_STORE_PC_OFFSET
            && matches!(operation, Op::WriteLocal { slot, .. } if slot == load_slot)
    });
    if has_interior_entry || local_written_elsewhere {
        return None;
    }

    let intermediates_are_dead = register_is_read_only_at(code, *load_dst, &[*binary_pc])
        && register_is_read_only_at(code, *literal_dst, &[*binary_pc])
        && register_is_read_only_at(code, *binary_dst, &[*store_pc]);
    if !intermediates_are_dead {
        return None;
    }

    let stencil_name = match (kind, region_stats_enabled) {
        (numeric_region::NumericBinary::Add, false) => {
            "quench_region_dead_update_local_number_add_jump"
        }
        (numeric_region::NumericBinary::Subtract, false) => {
            "quench_region_dead_update_local_number_subtract_jump"
        }
        (numeric_region::NumericBinary::Add, true) => {
            "quench_region_counted_dead_update_local_number_add_jump"
        }
        (numeric_region::NumericBinary::Subtract, true) => {
            "quench_region_counted_dead_update_local_number_subtract_jump"
        }
        _ => return None,
    };
    Some(NumericRegionUpdateSupernode {
        pc: *pc,
        target: numeric_region_target(*target, region_start, region_end),
        stencil_name,
    })
}

#[cfg(target_arch = "aarch64")]
fn numeric_region_leaf(
    operation: &numeric_region::RegionOp,
    region_start: usize,
    region_end: usize,
    guard: &numeric_region::GuardPlan,
    region_stats_enabled: bool,
    site_advance: usize,
) -> Stencil<Connector, Connector> {
    let (name, next, branch, burns_operands, burns_raw_value) = numeric_region_template(
        operation,
        region_start,
        region_end,
        guard,
        region_stats_enabled,
    );
    let pc = numeric_region_pc(operation);
    let label = LabelId(REGION_BODY_LABEL_BASE + pc as u32);
    if region_stats_enabled && burns_raw_value {
        record_linked_literal_variant(name);
    }
    rustc_dyn_stencil_with_operands_and_site_advance(
        name,
        next,
        branch,
        numeric_region_operand_bindings(operation, burns_operands),
        numeric_region_raw_value_bindings(operation, burns_raw_value),
        site_advance,
    )
    .labeled(label)
}

#[cfg(target_arch = "aarch64")]
fn record_linked_literal_variant(name: &str) {
    let counter = match name {
        "quench_region_burned_load_literal_one_lane" => {
            &NUMERIC_REGION_RUNTIME_STATS.linked_literal_one_lane_stencils
        }
        "quench_region_burned_load_literal_two_lanes" => {
            &NUMERIC_REGION_RUNTIME_STATS.linked_literal_two_lane_stencils
        }
        "quench_region_burned_load_literal_three_lanes" => {
            &NUMERIC_REGION_RUNTIME_STATS.linked_literal_three_lane_stencils
        }
        "quench_region_burned_load_literal_four_lanes" => {
            &NUMERIC_REGION_RUNTIME_STATS.linked_literal_four_lane_stencils
        }
        _ => panic!("unknown burned literal stencil {name}"),
    };
    counter.fetch_add(1, Ordering::Relaxed);
}

#[cfg(target_arch = "aarch64")]
fn numeric_region_template(
    operation: &numeric_region::RegionOp,
    region_start: usize,
    region_end: usize,
    guard: &numeric_region::GuardPlan,
    region_stats_enabled: bool,
) -> (&'static str, Option<LabelId>, Option<LabelId>, bool, bool) {
    use numeric_region::RegionOp as Op;
    let operands_fit = numeric_region_operands_fit(operation);
    match operation {
        Op::Elided { .. } => ("quench_region_nop", None, None, false, false),
        Op::NumberLiteral { .. } => ("quench_region_load_literal", None, None, false, false),
        Op::ReadLocal { dst, .. }
            if guard.is_dense_register(*dst) || guard.is_property_register(*dst) =>
        {
            ("quench_region_nop", None, None, false, false)
        }
        Op::ReadCaptured { dst, .. }
            if guard.is_dense_register(*dst) || guard.is_property_register(*dst) =>
        {
            ("quench_region_nop", None, None, false, false)
        }
        Op::Move { dst, .. }
            if guard.is_dense_register(*dst) || guard.is_property_register(*dst) =>
        {
            ("quench_region_nop", None, None, false, false)
        }
        Op::ReadLocal { .. } => (
            if operands_fit {
                "quench_region_burned_load_local"
            } else {
                "quench_region_load_local"
            },
            None,
            None,
            operands_fit,
            false,
        ),
        Op::ReadCaptured { .. } => ("quench_region_load_name", None, None, false, false),
        Op::WriteLocal { .. } => (
            if operands_fit {
                "quench_region_burned_store_local"
            } else {
                "quench_region_store_local"
            },
            None,
            None,
            operands_fit,
            false,
        ),
        Op::Move { .. } => (
            if operands_fit {
                "quench_region_burned_move"
            } else {
                "quench_region_move"
            },
            None,
            None,
            operands_fit,
            false,
        ),
        Op::Unary { kind, .. } => (
            numeric_unary_template(*kind, operands_fit),
            None,
            None,
            operands_fit,
            false,
        ),
        Op::Binary { kind, .. } => {
            let burns_operands = operands_fit && numeric_binary_has_burned_template(*kind);
            (
                numeric_binary_template(*kind, burns_operands),
                None,
                None,
                burns_operands,
                false,
            )
        }
        Op::ReadDense { pc, .. } => (
            if guard.has_proven_index(*pc) {
                "quench_region_read_dense_proven_index"
            } else {
                "quench_region_read_dense"
            },
            None,
            None,
            false,
            false,
        ),
        Op::WriteDense { pc, .. } => (
            if guard.has_proven_index(*pc) {
                "quench_region_write_dense_proven_index"
            } else {
                "quench_region_write_dense"
            },
            None,
            None,
            false,
            false,
        ),
        Op::ReadStatic { .. } => ("quench_region_read_static", None, None, false, false),
        Op::WriteStatic { .. } => ("quench_region_write_static", None, None, false, false),
        Op::Jump { pc, target } => (
            if region_stats_enabled && target <= pc {
                "quench_region_counted_jump"
            } else {
                "quench_dyn_jump"
            },
            Some(numeric_region_target(*target, region_start, region_end)),
            None,
            false,
            false,
        ),
        Op::JumpIfFalse { target, .. } => (
            "quench_dyn_jump_if_false",
            None,
            Some(numeric_region_target(*target, region_start, region_end)),
            false,
            false,
        ),
    }
}

#[cfg(target_arch = "aarch64")]
fn numeric_unary_template(kind: numeric_region::NumericUnary, burn_operands: bool) -> &'static str {
    use numeric_region::NumericUnary::*;
    match (kind, burn_operands) {
        (Plus, true) => "quench_region_burned_unary_plus",
        (Negate, true) => "quench_region_burned_negate",
        (BitNot, true) => "quench_region_burned_bit_not",
        (Plus, false) => "quench_region_unary_plus",
        (Negate, false) => "quench_region_negate",
        (BitNot, false) => "quench_region_bit_not",
    }
}

#[cfg(target_arch = "aarch64")]
fn numeric_binary_has_burned_template(kind: numeric_region::NumericBinary) -> bool {
    use numeric_region::NumericBinary::*;
    matches!(
        kind,
        Add | Subtract
            | Multiply
            | Divide
            | Equal
            | NotEqual
            | StrictEqual
            | StrictNotEqual
            | Less
            | LessEqual
            | Greater
            | GreaterEqual
    )
}

#[cfg(target_arch = "aarch64")]
fn numeric_binary_template(
    kind: numeric_region::NumericBinary,
    burn_operands: bool,
) -> &'static str {
    use numeric_region::NumericBinary::*;
    match (kind, burn_operands) {
        (Add, true) => "quench_region_burned_add",
        (Subtract, true) => "quench_region_burned_subtract",
        (Multiply, true) => "quench_region_burned_multiply",
        (Divide, true) => "quench_region_burned_divide",
        (Equal | StrictEqual, true) => "quench_region_burned_equal",
        (NotEqual | StrictNotEqual, true) => "quench_region_burned_not_equal",
        (Less, true) => "quench_region_burned_less",
        (LessEqual, true) => "quench_region_burned_less_equal",
        (Greater, true) => "quench_region_burned_greater",
        (GreaterEqual, true) => "quench_region_burned_greater_equal",
        (Add, false) => "quench_region_add",
        (Subtract, false) => "quench_region_subtract",
        (Multiply, false) => "quench_region_multiply",
        (Divide, false) => "quench_region_divide",
        (Equal | StrictEqual, false) => "quench_region_equal",
        (NotEqual | StrictNotEqual, false) => "quench_region_not_equal",
        (Less, false) => "quench_region_less",
        (LessEqual, false) => "quench_region_less_equal",
        (Greater, false) => "quench_region_greater",
        (GreaterEqual, false) => "quench_region_greater_equal",
        (ShiftLeft, _) => "quench_region_shift_left",
        (ShiftRight, _) => "quench_region_shift_right",
        (ShiftRightUnsigned, _) => "quench_region_shift_right_unsigned",
        (BitOr, _) => "quench_region_bit_or",
        (BitXor, _) => "quench_region_bit_xor",
        (BitAnd, _) => "quench_region_bit_and",
    }
}

#[cfg(target_arch = "aarch64")]
#[derive(Clone, Copy)]
struct OperandBinding {
    kind: operand_holes::OperandHoleKind,
    byte_offset: usize,
}

#[cfg(target_arch = "aarch64")]
#[derive(Clone, Copy)]
struct RawValueBinding {
    id: u8,
    bits: u64,
}

#[cfg(target_arch = "aarch64")]
fn register_operand_byte_offset(register: Register) -> Option<usize> {
    value_slot_byte_offset(usize::from(register))
}

#[cfg(target_arch = "aarch64")]
fn value_slot_byte_offset(slot: usize) -> Option<usize> {
    let byte_offset = slot.checked_mul(operand_holes::VALUE_BYTE_WIDTH)?;
    (byte_offset
        <= operand_holes::AARCH64_UNSIGNED_OFFSET_MAX_SCALED * operand_holes::VALUE_BYTE_WIDTH)
        .then_some(byte_offset)
}

#[cfg(target_arch = "aarch64")]
fn register_operand_offsets_fit<const COUNT: usize>(registers: [Register; COUNT]) -> bool {
    registers
        .into_iter()
        .all(|register| register_operand_byte_offset(register).is_some())
}

#[cfg(target_arch = "aarch64")]
fn numeric_region_operands_fit(operation: &numeric_region::RegionOp) -> bool {
    use numeric_region::RegionOp;
    match operation {
        RegionOp::NumberLiteral { dst, .. } => register_operand_byte_offset(*dst).is_some(),
        RegionOp::ReadLocal { dst, slot, .. } => {
            register_operand_byte_offset(*dst).is_some() && value_slot_byte_offset(*slot).is_some()
        }
        RegionOp::WriteLocal { slot, src, .. } => {
            value_slot_byte_offset(*slot).is_some() && register_operand_byte_offset(*src).is_some()
        }
        RegionOp::Move { dst, src, .. } | RegionOp::Unary { dst, src, .. } => {
            register_operand_offsets_fit([*dst, *src])
        }
        RegionOp::Binary {
            dst, left, right, ..
        } => register_operand_offsets_fit([*dst, *left, *right]),
        _ => false,
    }
}

#[cfg(target_arch = "aarch64")]
fn numeric_region_operand_bindings(
    operation: &numeric_region::RegionOp,
    burns_operands: bool,
) -> Vec<OperandBinding> {
    use numeric_region::RegionOp;
    if !burns_operands {
        return Vec::new();
    }
    let register_binding = |kind, register| OperandBinding {
        kind,
        byte_offset: register_operand_byte_offset(register)
            .expect("burned operand template selected only for encodable registers"),
    };
    let local_binding = |kind, slot| OperandBinding {
        kind,
        byte_offset: value_slot_byte_offset(slot)
            .expect("burned operand template selected only for encodable local slots"),
    };
    match operation {
        RegionOp::NumberLiteral { dst, .. } => vec![register_binding(
            operand_holes::OperandHoleKind::DestinationRegisterByteOffset,
            *dst,
        )],
        RegionOp::ReadLocal { dst, slot, .. } => vec![
            register_binding(
                operand_holes::OperandHoleKind::DestinationRegisterByteOffset,
                *dst,
            ),
            local_binding(operand_holes::OperandHoleKind::SourceLocalByteOffset, *slot),
        ],
        RegionOp::WriteLocal { slot, src, .. } => vec![
            local_binding(
                operand_holes::OperandHoleKind::DestinationLocalByteOffset,
                *slot,
            ),
            register_binding(
                operand_holes::OperandHoleKind::SourceRegisterByteOffset,
                *src,
            ),
        ],
        RegionOp::Move { dst, src, .. } | RegionOp::Unary { dst, src, .. } => vec![
            register_binding(
                operand_holes::OperandHoleKind::DestinationRegisterByteOffset,
                *dst,
            ),
            register_binding(
                operand_holes::OperandHoleKind::SourceRegisterByteOffset,
                *src,
            ),
        ],
        RegionOp::Binary {
            dst, left, right, ..
        } => vec![
            register_binding(
                operand_holes::OperandHoleKind::DestinationRegisterByteOffset,
                *dst,
            ),
            register_binding(
                operand_holes::OperandHoleKind::LeftRegisterByteOffset,
                *left,
            ),
            register_binding(
                operand_holes::OperandHoleKind::RightRegisterByteOffset,
                *right,
            ),
        ],
        _ => panic!("selected burned template has no operand binding recipe"),
    }
}

#[cfg(target_arch = "aarch64")]
fn numeric_region_raw_value_bindings(
    operation: &numeric_region::RegionOp,
    burns_raw_value: bool,
) -> Vec<RawValueBinding> {
    if !burns_raw_value {
        return Vec::new();
    }
    match operation {
        numeric_region::RegionOp::NumberLiteral { bits, .. } => vec![RawValueBinding {
            id: raw_value_holes::RAW_VALUE_HOLE_ID,
            bits: *bits,
        }],
        _ => panic!("selected raw-value template has no binding recipe"),
    }
}

#[cfg(target_arch = "aarch64")]
fn numeric_region_target(target: usize, region_start: usize, region_end: usize) -> LabelId {
    if (region_start..region_end).contains(&target) {
        LabelId(REGION_BODY_LABEL_BASE + target as u32)
    } else {
        LabelId(target as u32)
    }
}

#[cfg(target_arch = "aarch64")]
fn numeric_region_fallbacks(
    code: &DynCode,
    regions: &[NumericRegionLink],
    refresh_name_snapshots: bool,
    instrumented_kernels: bool,
    residual_block_stats_enabled: bool,
    native_path_stats_enabled: bool,
) -> Stencil<Connector, Connector> {
    code.blocks
        .iter()
        .filter(|(start, _, _)| {
            regions.iter().any(|region| {
                (region.quote.start + NEXT_INSTRUCTION_DISTANCE..region.quote.end).contains(start)
            })
        })
        .fold(identity::<Connector>(), |fallbacks, (start, end, _)| {
            fallbacks
                + block_leaf(
                    *start,
                    *end,
                    true,
                    refresh_name_snapshots,
                    instrumented_kernels,
                    residual_block_stats_enabled,
                    native_path_stats_enabled,
                )
        })
}

#[cfg(target_arch = "aarch64")]
fn numeric_region_pc(operation: &numeric_region::RegionOp) -> usize {
    use numeric_region::RegionOp::*;
    match operation {
        Elided { pc }
        | NumberLiteral { pc, .. }
        | ReadLocal { pc, .. }
        | ReadCaptured { pc, .. }
        | WriteLocal { pc, .. }
        | Move { pc, .. }
        | Unary { pc, .. }
        | Binary { pc, .. }
        | ReadDense { pc, .. }
        | WriteDense { pc, .. }
        | ReadStatic { pc, .. }
        | WriteStatic { pc, .. }
        | Jump { pc, .. }
        | JumpIfFalse { pc, .. } => *pc,
    }
}

#[cfg(target_arch = "aarch64")]
fn direct_block_leaf(
    pc: usize,
    template: DirectBlockTemplate,
    native_path_stats_enabled: bool,
) -> Stencil<Connector, Connector> {
    let (name, next_label, branch_label) = match template {
        DirectBlockTemplate::Terminal { stencil_name } => {
            (stencil_name, Some(FUNCTION_EXIT_LABEL), None)
        }
        DirectBlockTemplate::Transfer {
            stencil_name,
            target_label,
        } => (stencil_name, Some(target_label), None),
        DirectBlockTemplate::UpdateLocalLiteralJump {
            stencil_name,
            target,
        } => (stencil_name, Some(LabelId(target as u32)), None),
        DirectBlockTemplate::ConditionLocalLiteral {
            stencil_name,
            target,
        } => (stencil_name, None, Some(LabelId(target as u32))),
        DirectBlockTemplate::ConditionLocalName {
            stencil_name,
            target,
        } => (stencil_name, None, Some(LabelId(target as u32))),
        DirectBlockTemplate::InstanceOfCondition {
            stencil_name,
            target,
        } => (stencil_name, None, Some(LabelId(target as u32))),
        DirectBlockTemplate::PropertyCondition {
            stencil_name,
            target,
        } => (stencil_name, None, Some(LabelId(target as u32))),
    };
    observed_labeled_leaf(
        pc,
        native_path_stats_enabled,
        &NATIVE_PATH_RUNTIME_STATS.direct_block_entries,
        rustc_dyn_stencil(name, next_label, branch_label),
    )
}

#[cfg(target_arch = "aarch64")]
fn rustc_dyn_stencil(
    name: &str,
    next_label: Option<LabelId>,
    branch_label: Option<LabelId>,
) -> Stencil<Connector, Connector> {
    rustc_dyn_stencil_with_operands(name, next_label, branch_label, Vec::new())
}

#[cfg(target_arch = "aarch64")]
fn rustc_dyn_stencil_with_operands(
    name: &str,
    next_label: Option<LabelId>,
    branch_label: Option<LabelId>,
    operand_bindings: Vec<OperandBinding>,
) -> Stencil<Connector, Connector> {
    rustc_dyn_stencil_with_operands_and_site_advance(
        name,
        next_label,
        branch_label,
        operand_bindings,
        Vec::new(),
        NEXT_INSTRUCTION_DISTANCE,
    )
}

#[cfg(target_arch = "aarch64")]
fn rustc_dyn_stencil_with_operands_and_site_advance(
    name: &str,
    next_label: Option<LabelId>,
    branch_label: Option<LabelId>,
    operand_bindings: Vec<OperandBinding>,
    raw_value_bindings: Vec<RawValueBinding>,
    site_advance: usize,
) -> Stencil<Connector, Connector> {
    rustc_typed_stencil_with_site_advance(
        name,
        next_label,
        branch_label,
        operand_bindings,
        raw_value_bindings,
        site_advance,
    )
}

#[cfg(target_arch = "aarch64")]
fn rustc_typed_stencil_with_site_advance<In: StencilState, Out: StencilState>(
    name: &str,
    next_label: Option<LabelId>,
    branch_label: Option<LabelId>,
    operand_bindings: Vec<OperandBinding>,
    raw_value_bindings: Vec<RawValueBinding>,
    site_advance: usize,
) -> Stencil<In, Out> {
    let index = rustc_stencils::STENCILS
        .iter()
        .position(|stencil| stencil.name == name)
        .expect("direct stencil exists in rustc catalog");
    let descriptor = &rustc_stencils::STENCILS[index];
    thread_local! {
        static TEMPLATES: Vec<Rc<StencilTemplate>> = rustc_stencils::STENCILS
            .iter()
            .map(|stencil| Rc::new(StencilTemplate {
                level: StencilLevel::Opcode,
                bytes: Rc::from(stencil.bytes),
            }))
            .collect();
    }
    let template = TEMPLATES.with(|templates| templates[index].clone());
    #[cfg(debug_assertions)]
    validate_stencil_bindings(name, descriptor, &operand_bindings, &raw_value_bindings);
    let holes = stencil_holes(descriptor, next_label, branch_label);
    let site_advance_bytes = site_advance
        .checked_mul(std::mem::size_of::<InlineSite>())
        .expect("site advance does not overflow");
    let patches = stencil_copy_patches(
        name,
        descriptor,
        &operand_bindings,
        &raw_value_bindings,
        site_advance_bytes,
    );
    Stencil::<In, Out>::instantiate(template.instantiate(patches, holes, Vec::new()))
}

#[cfg(target_arch = "aarch64")]
fn observation_leaf(counter: &'static AtomicU64) -> Stencil<Connector, Connector> {
    rustc_dyn_stencil_with_operands_and_site_advance(
        "quench_observe_entry",
        None,
        None,
        Vec::new(),
        vec![RawValueBinding {
            id: raw_value_holes::OBSERVATION_COUNTER_HOLE_ID,
            bits: std::ptr::from_ref(counter) as usize as u64,
        }],
        FIRST_INSTRUCTION_PC,
    )
}

#[cfg(target_arch = "aarch64")]
fn observed_labeled_leaf(
    pc: usize,
    enabled: bool,
    counter: &'static AtomicU64,
    leaf: Stencil<Connector, Connector>,
) -> Stencil<Connector, Connector> {
    if enabled {
        (observation_leaf(counter) + leaf).labeled(LabelId(pc as u32))
    } else {
        leaf.labeled(LabelId(pc as u32))
    }
}

#[cfg(target_arch = "aarch64")]
#[cfg(any(debug_assertions, test))]
fn validate_stencil_bindings(
    name: &str,
    descriptor: &rustc_stencils::RustcStencil,
    operand_bindings: &[OperandBinding],
    raw_value_bindings: &[RawValueBinding],
) {
    let operand_relocation_count = descriptor
        .patch_sites
        .iter()
        .filter(|site| matches!(site.binding, patch_schema::PatchBinding::Operand(_)))
        .count();
    assert_eq!(
        operand_relocation_count,
        operand_bindings.len(),
        "stencil {name} must receive exactly its cooked operand bindings"
    );
    for binding in operand_bindings {
        assert_eq!(
            descriptor
                .patch_sites
                .iter()
                .filter(|site| {
                    site.binding == patch_schema::PatchBinding::Operand(binding.kind)
                })
                .count(),
            1,
            "stencil {name} must contain one relocation for {:?}",
            binding.kind
        );
        assert_eq!(
            operand_bindings
                .iter()
                .filter(|candidate| candidate.kind == binding.kind)
                .count(),
            1,
            "stencil {name} must receive one binding for {:?}",
            binding.kind
        );
    }
    for binding in raw_value_bindings {
        assert_eq!(
            raw_value_bindings
                .iter()
                .filter(|candidate| candidate.id == binding.id)
                .count(),
            1,
            "stencil {name} must receive one raw-value binding for id {}",
            binding.id
        );
        assert!(
            descriptor.patch_sites.iter().any(|site| {
                site.binding == patch_schema::PatchBinding::RawValue { id: binding.id }
            }),
            "stencil {name} does not contain raw-value id {}",
            binding.id
        );
    }
    for site in descriptor.patch_sites {
        if let patch_schema::PatchBinding::RawValue { id } = site.binding {
            assert!(
                raw_value_bindings.iter().any(|binding| binding.id == id),
                "stencil {name} is missing raw-value binding {id}"
            );
        }
    }
}

#[cfg(target_arch = "aarch64")]
fn stencil_holes(
    descriptor: &rustc_stencils::RustcStencil,
    next_label: Option<LabelId>,
    branch_label: Option<LabelId>,
) -> Vec<Hole> {
    let mut holes = vec![match next_label {
        Some(label) => Hole::Symbolic {
            offset: descriptor.next_relocation(),
            label,
        },
        None => Hole::Internal {
            offset: descriptor.next_relocation(),
            target: SymbolicTarget::Next,
        },
    }];
    if let Some(offset) = descriptor.slow_relocation() {
        holes.push(Hole::Symbolic {
            offset,
            label: SLOW_PATH_LABEL,
        });
    }
    if let Some(offset) = descriptor.branch_relocation() {
        holes.push(Hole::Symbolic {
            offset,
            label: branch_label.expect("taken branch stencil has a bytecode target"),
        });
    } else {
        debug_assert!(branch_label.is_none());
    }
    holes
}

#[cfg(target_arch = "aarch64")]
fn stencil_copy_patches(
    name: &str,
    descriptor: &rustc_stencils::RustcStencil,
    operand_bindings: &[OperandBinding],
    raw_value_bindings: &[RawValueBinding],
    site_advance_bytes: usize,
) -> Vec<CopyPatch> {
    descriptor
        .patch_sites
        .iter()
        .filter_map(|site| {
            use patch_schema::PatchBinding;
            let value = match site.binding {
                PatchBinding::Next | PatchBinding::Slow | PatchBinding::Taken => return None,
                PatchBinding::SiteAdvanceBytes => site_advance_bytes as u64,
                PatchBinding::Operand(kind) => operand_bindings
                    .iter()
                    .find_map(|binding| {
                        (binding.kind == kind).then_some(binding.byte_offset as u64)
                    })
                    .unwrap_or_else(|| panic!("missing {kind:?} binding for stencil {name}")),
                PatchBinding::RawValue { id } => raw_value_bindings
                    .iter()
                    .find_map(|binding| (binding.id == id).then_some(binding.bits))
                    .unwrap_or_else(|| panic!("missing raw-value binding {id} for stencil {name}")),
            };
            Some(CopyPatch::encoded(descriptor.bytes, *site, value))
        })
        .collect()
}

#[cfg(target_arch = "aarch64")]
const AARCH64_ADD_IMMEDIATE_FIELD_SHIFT: u32 = 10;
#[cfg(target_arch = "aarch64")]
const AARCH64_ADD_IMMEDIATE_FIELD_MASK: u32 =
    (1_u32 << site_holes::AARCH64_ADD_IMMEDIATE_FIELD_BITS) - 1;

#[cfg(target_arch = "aarch64")]
fn encoded_instruction_patch(
    stencil_bytes: &[u8],
    instruction_offset: usize,
    encoding: patch_schema::PatchEncoding,
    value: u64,
) -> Option<CopyPatch> {
    CopyPatch::try_with_encoding(stencil_bytes, instruction_offset, encoding, value)
}

#[cfg(target_arch = "aarch64")]
fn site_advance_patch(
    stencil_bytes: &[u8],
    instruction_offset: usize,
    byte_advance: usize,
) -> Option<CopyPatch> {
    encoded_instruction_patch(
        stencil_bytes,
        instruction_offset,
        patch_schema::PatchEncoding::AddImmediate12,
        byte_advance as u64,
    )
}

#[cfg(target_arch = "aarch64")]
fn burned_operand_patch(
    stencil_bytes: &[u8],
    instruction_offset: usize,
    byte_offset: usize,
) -> Option<CopyPatch> {
    encoded_instruction_patch(
        stencil_bytes,
        instruction_offset,
        patch_schema::PatchEncoding::LoadStoreUnsigned12,
        byte_offset as u64,
    )
}

#[cfg(target_arch = "aarch64")]
fn slow_adapter_leaf(
    instrumented_kernels: bool,
    residual_block_stats_enabled: bool,
) -> Stencil<Connector, Connector> {
    let template = slow_adapter_template();
    Stencil::instantiate(template.instantiate(
        vec![CopyPatch::pointer(
            HELPER_LITERAL_OFFSET,
            if instrumented_kernels {
                dyn_block_from_site as *const () as usize
            } else if residual_block_stats_enabled {
                dyn_residual_block_from_site as *const () as usize
            } else {
                dyn_fast_block_from_site as *const () as usize
            },
        )],
        Vec::new(),
        Vec::new(),
    ))
}

#[cfg(target_arch = "aarch64")]
fn effect_reentry_dispatch_leaf(
    pc: usize,
    native_path_stats_enabled: bool,
) -> Stencil<Connector, Connector> {
    let leaf = Stencil::leaf(LeafStencil {
        level: StencilLevel::Opcode,
        bytes: machine_bytes(&[a64_abi::BR_BASE], false).to_vec(),
        holes: vec![Hole::Symbolic {
            offset: 0,
            label: EFFECT_REENTRY_LABEL,
        }],
        labels: Vec::new(),
        fragments: Vec::new(),
    });
    observed_labeled_leaf(
        pc,
        native_path_stats_enabled,
        &NATIVE_PATH_RUNTIME_STATS.effect_reentry_entries,
        leaf,
    )
}

#[cfg(target_arch = "aarch64")]
fn effect_reentry_adapter_leaf(entry: usize) -> Stencil<Connector, Connector> {
    let template = effect_reentry_adapter_template();
    Stencil::instantiate(template.instantiate(
        vec![CopyPatch::pointer(
            EFFECT_REENTRY_ADAPTER_LITERAL_OFFSET,
            entry,
        )],
        Vec::new(),
        Vec::new(),
    ))
}

#[cfg(target_arch = "aarch64")]
fn guest_entry_adapter_leaf() -> Stencil<Connector, Connector> {
    Stencil::leaf(LeafStencil {
        level: StencilLevel::Function,
        bytes: machine_bytes(&[a64_abi::SET_FRAME, a64_abi::BR_BASE], false).to_vec(),
        holes: vec![Hole::Symbolic {
            offset: a64_abi::INSTRUCTION_BYTES,
            label: LabelId(FIRST_INSTRUCTION_PC as u32),
        }],
        labels: Vec::new(),
        fragments: Vec::new(),
    })
}

#[cfg(target_arch = "aarch64")]
fn exit_adapter_leaf() -> Stencil<Connector, Connector> {
    Stencil::instantiate(exit_adapter_template().instantiate(Vec::new(), Vec::new(), Vec::new()))
}

#[cfg(target_arch = "aarch64")]
fn direct_call_exception_adapter_leaf() -> Stencil<Connector, Connector> {
    Stencil::leaf(LeafStencil {
        level: StencilLevel::Function,
        bytes: machine_bytes(&[LOAD_RESUME_TARGET, BRANCH_TARGET_REGISTER], false).to_vec(),
        holes: Vec::new(),
        labels: Vec::new(),
        fragments: Vec::new(),
    })
}

#[cfg(target_arch = "aarch64")]
fn block_leaf(
    pc: usize,
    end: usize,
    direct_abi: bool,
    refresh_name_snapshots: bool,
    instrumented_kernels: bool,
    residual_block_stats_enabled: bool,
    native_path_stats_enabled: bool,
) -> Stencil<Connector, Connector> {
    let helper = block_step_entry(
        refresh_name_snapshots,
        instrumented_kernels,
        residual_block_stats_enabled,
    );
    semantic_helper_leaf(pc, end, direct_abi, helper, native_path_stats_enabled)
}

#[cfg(target_arch = "aarch64")]
fn bounded_range_leaf(
    start: usize,
    end: usize,
    refresh_name_snapshots: bool,
    native_path_stats_enabled: bool,
) -> Stencil<Connector, Connector> {
    let helper = if refresh_name_snapshots {
        dyn_fast_snapshot_range_step
    } else {
        dyn_fast_range_step
    };
    semantic_helper_leaf(start, end, true, helper, native_path_stats_enabled)
}

type BlockStepEntry = unsafe extern "C" fn(*mut DynFrame, u32) -> usize;

#[cfg(target_arch = "aarch64")]
fn block_step_entry(
    refresh_name_snapshots: bool,
    instrumented_kernels: bool,
    residual_block_stats_enabled: bool,
) -> BlockStepEntry {
    match (
        refresh_name_snapshots,
        instrumented_kernels,
        residual_block_stats_enabled,
    ) {
        (false, false, false) => dyn_fast_block_step,
        (false, false, true) => dyn_residual_block_step,
        (false, true, _) => dyn_block_step,
        (true, false, false) => dyn_fast_snapshot_block_step,
        (true, false, true) => dyn_residual_snapshot_block_step,
        (true, true, _) => dyn_snapshot_block_step,
    }
}

#[cfg(target_arch = "aarch64")]
fn semantic_helper_leaf(
    pc: usize,
    end: usize,
    direct_abi: bool,
    helper: BlockStepEntry,
    native_path_stats_enabled: bool,
) -> Stencil<Connector, Connector> {
    let pc_instruction = MOVE_PC_TO_ARG_BASE | ((pc as u32) << 5);
    let (template, helper_offset) = if direct_abi {
        (direct_block_template(), BLOCK_HELPER_LITERAL_OFFSET)
    } else {
        (block_template(), HELPER_LITERAL_OFFSET)
    };
    let instance = template.instantiate(
        vec![
            CopyPatch::word(PC_INSTRUCTION_OFFSET, pc_instruction),
            CopyPatch::pointer(helper_offset, helper as *const () as usize),
        ],
        Vec::new(),
        vec![StencilFragment {
            level: StencilLevel::Block,
            family: StencilFamily::Helper,
            bytes: Vec::new(),
            relocs: vec![RelocSpec {
                offset: helper_offset as u32,
                kind: RelocKind::Helper,
                width: a64_abi::POINTER_BYTES as u8,
            }],
            bytecode_start: pc,
            bytecode_end: end,
        }],
    );
    observed_labeled_leaf(
        pc,
        native_path_stats_enabled,
        &NATIVE_PATH_RUNTIME_STATS.semantic_kernel_entries,
        Stencil::instantiate(instance),
    )
}

#[cfg(target_arch = "aarch64")]
fn machine_bytes(words: &[u32], trailing_pointer: bool) -> Rc<[u8]> {
    let extra = usize::from(trailing_pointer) * a64_abi::POINTER_BYTES;
    let mut bytes = Vec::with_capacity(words.len() * a64_abi::INSTRUCTION_BYTES + extra);
    for word in words {
        a64_word(&mut bytes, *word);
    }
    if trailing_pointer {
        bytes.extend_from_slice(&a64_abi::EMPTY_POINTER_LITERAL);
    }
    bytes.into()
}

#[cfg(target_arch = "aarch64")]
macro_rules! define_kernel {
    ($name:ident, [$($word:expr),+ $(,)?]) => {
        fn $name() -> Rc<Kernel<Connector, ReturnState>> {
            thread_local! {
                static KERNEL: Rc<Kernel<Connector, ReturnState>> = {
                    let bytes = machine_bytes(&[$($word),+], false);
                    let memory = Rc::new(map_executable(&bytes, bytes.len())
                        .expect("map shared immutable kernel"));
                    Rc::new(Kernel {
                        entry: memory.ptr as usize,
                        _memory: memory,
                        state: PhantomData,
                    })
                };
            }
            KERNEL.with(Clone::clone)
        }
    };
}

#[cfg(target_arch = "aarch64")]
macro_rules! define_helper_kernel {
    ($name:ident, $helper:path, $literal_offset:expr, $byte_count:expr, [$($word:expr),+ $(,)?]) => {
        fn $name() -> Rc<Kernel<Connector, Connector>> {
            thread_local! {
                static KERNEL: Rc<Kernel<Connector, Connector>> = {
                    let mut bytes = machine_bytes(&[$($word),+], true).to_vec();
                    bytes[$literal_offset..$literal_offset + a64_abi::POINTER_BYTES]
                        .copy_from_slice(&($helper as *const () as usize).to_le_bytes());
                    debug_assert_eq!(bytes.len(), $byte_count);
                    let memory = Rc::new(map_executable(&bytes, bytes.len())
                        .expect("map shared immutable helper kernel"));
                    Rc::new(Kernel {
                        entry: memory.ptr as usize,
                        _memory: memory,
                        state: PhantomData,
                    })
                };
            }
            KERNEL.with(Clone::clone)
        }
    };
}

#[cfg(target_arch = "aarch64")]
define_kernel!(
    exit_kernel,
    [
        a64_abi::RESTORE_CONNECTORS,
        a64_abi::RESTORE_FRAME_AND_LINK,
        a64_abi::RETURN,
    ]
);

define_helper_kernel!(
    effect_reentry_kernel,
    dyn_single_step,
    EFFECT_REENTRY_KERNEL_LITERAL_OFFSET,
    EFFECT_REENTRY_KERNEL_BYTES,
    [
        LOAD_SLOW_HELPER_AT_LITERAL,
        a64_abi::CALL_HELPER,
        MOVE_TARGET_TO_BRANCH_REGISTER,
        MOVE_FRAME_TO_ARG,
        LOAD_CURRENT_SITE,
        BRANCH_TARGET_REGISTER,
    ]
);

fn block_template() -> Rc<StencilTemplate> {
    thread_local! {
        static TEMPLATE: Rc<StencilTemplate> = Rc::new(StencilTemplate {
            level: StencilLevel::Opcode,
            bytes: machine_bytes(&[
                MOVE_FRAME_TO_ARG,
                MOVE_PC_TO_ARG_BASE,
                LOAD_BLOCK_HELPER_AT_LITERAL,
                a64_abi::CALL_HELPER,
                BRANCH_HELPER_RESULT,
                ALIGN_POINTER_LITERAL,
            ], true),
        });
    }
    TEMPLATE.with(|template| {
        debug_assert_eq!(template.bytes.len(), LEGACY_BLOCK_STENCIL_BYTES);
        template.clone()
    })
}

#[cfg(target_arch = "aarch64")]
fn direct_block_template() -> Rc<StencilTemplate> {
    thread_local! {
        static TEMPLATE: Rc<StencilTemplate> = Rc::new(StencilTemplate {
            level: StencilLevel::Opcode,
            bytes: machine_bytes(&[
                MOVE_FRAME_TO_ARG,
                MOVE_PC_TO_ARG_BASE,
                LOAD_SLOW_HELPER_AT_LITERAL,
                a64_abi::CALL_HELPER,
                MOVE_TARGET_TO_BRANCH_REGISTER,
                MOVE_FRAME_TO_ARG,
                LOAD_CURRENT_SITE,
                BRANCH_TARGET_REGISTER,
            ], true),
        });
    }
    TEMPLATE.with(|template| {
        debug_assert_eq!(template.bytes.len(), DIRECT_BLOCK_STENCIL_BYTES);
        template.clone()
    })
}

#[cfg(target_arch = "aarch64")]
fn prologue_template() -> Rc<StencilTemplate> {
    thread_local! {
        static TEMPLATE: Rc<StencilTemplate> = Rc::new(StencilTemplate {
            level: StencilLevel::Function,
            bytes: machine_bytes(&[
                a64_abi::SAVE_FRAME_AND_LINK,
                a64_abi::SET_NATIVE_FRAME,
                a64_abi::SAVE_CONNECTORS,
                a64_abi::SET_FRAME,
            ], false),
        });
    }
    TEMPLATE.with(Clone::clone)
}

#[cfg(target_arch = "aarch64")]
fn direct_prologue_template() -> Rc<StencilTemplate> {
    thread_local! {
        static TEMPLATE: Rc<StencilTemplate> = Rc::new(StencilTemplate {
            level: StencilLevel::Function,
            bytes: machine_bytes(&[
                a64_abi::SAVE_FRAME_AND_LINK,
                a64_abi::SET_NATIVE_FRAME,
                a64_abi::SAVE_CONNECTORS,
                a64_abi::SET_FRAME,
                LOAD_SITE_AT_LITERAL,
                BRANCH_OVER_SITE_LITERAL,
            ], true),
        });
    }
    TEMPLATE.with(Clone::clone)
}

#[cfg(target_arch = "aarch64")]
fn slow_adapter_template() -> Rc<StencilTemplate> {
    thread_local! {
        static TEMPLATE: Rc<StencilTemplate> = Rc::new(StencilTemplate {
            level: StencilLevel::Function,
            bytes: machine_bytes(&[
                LOAD_SLOW_HELPER_AT_LITERAL,
                a64_abi::CALL_HELPER,
                MOVE_TARGET_TO_BRANCH_REGISTER,
                MOVE_FRAME_TO_ARG,
                LOAD_CURRENT_SITE,
                BRANCH_TARGET_REGISTER,
            ], true),
        });
    }
    TEMPLATE.with(Clone::clone)
}

#[cfg(target_arch = "aarch64")]
fn effect_reentry_adapter_template() -> Rc<StencilTemplate> {
    thread_local! {
        static TEMPLATE: Rc<StencilTemplate> = Rc::new(StencilTemplate {
            level: StencilLevel::Function,
            bytes: machine_bytes(&[
                LOAD_EXIT_KERNEL_AT_LITERAL,
                BRANCH_TARGET_REGISTER,
            ], true),
        });
    }
    TEMPLATE.with(|template| {
        debug_assert_eq!(template.bytes.len(), EFFECT_REENTRY_ADAPTER_BYTES);
        template.clone()
    })
}

#[cfg(target_arch = "aarch64")]
fn exit_adapter_template() -> Rc<StencilTemplate> {
    thread_local! {
        static TEMPLATE: Rc<StencilTemplate> = Rc::new(StencilTemplate {
            level: StencilLevel::Function,
            bytes: machine_bytes(&[LOAD_RETURN_TARGET, BRANCH_TARGET_REGISTER], false),
        });
    }
    TEMPLATE.with(Clone::clone)
}

#[cfg(target_arch = "aarch64")]
fn rustc_stencil(index: usize) -> Stencil<Connector, Connector> {
    thread_local! {
        static TEMPLATES: Vec<Rc<StencilTemplate>> = rustc_stencils::STENCILS
            .iter()
            .map(|stencil| Rc::new(StencilTemplate {
                level: StencilLevel::Opcode,
                bytes: Rc::from(stencil.bytes),
            }))
            .collect();
    }
    let descriptor = &rustc_stencils::STENCILS[index];
    let template = TEMPLATES.with(|templates| templates[index].clone());
    Stencil::instantiate(template.instantiate(
        Vec::new(),
        vec![Hole::Internal {
            offset: descriptor.next_relocation(),
            target: SymbolicTarget::Next,
        }],
        Vec::new(),
    ))
}

#[cfg(target_arch = "aarch64")]
fn link_rustc_chain(image: &MaterializedStencil) -> Option<Vec<u8>> {
    let mut bytes = image.bytes.clone();
    for hole in &image.holes {
        let Hole::Internal {
            offset,
            target: SymbolicTarget::Offset(target),
        } = *hole
        else {
            return None;
        };
        patch_tail_branch(&mut bytes, offset, target)?;
    }
    Some(bytes)
}

#[cfg(target_arch = "aarch64")]
fn patch_tail_branch(bytes: &mut [u8], branch_offset: usize, target_offset: usize) -> Option<()> {
    let instruction_bytes = a64_abi::INSTRUCTION_BYTES as isize;
    let displacement = target_offset as isize - branch_offset as isize;
    if displacement % instruction_bytes != 0 {
        return None;
    }
    let words = displacement / instruction_bytes;
    if !(-a64_abi::B_WORD_LIMIT..a64_abi::B_WORD_LIMIT).contains(&words) {
        return None;
    }
    let immediate = words as i32 as u32 & a64_abi::BR_IMM_MASK;
    let branch = a64_abi::BR_BASE | immediate;
    bytes
        .get_mut(branch_offset..branch_offset + a64_abi::INSTRUCTION_BYTES)?
        .copy_from_slice(&branch.to_le_bytes());
    Some(())
}

#[cfg(target_arch = "aarch64")]
fn link_dynamic_holes(image: &MaterializedStencil) -> Option<Vec<u8>> {
    let mut bytes = image.bytes.clone();
    for hole in &image.holes {
        match *hole {
            Hole::Internal {
                offset,
                target: SymbolicTarget::Offset(target),
            } => patch_tail_branch(&mut bytes, offset, target)?,
            Hole::Symbolic { offset, label } => {
                let target = image.labels.iter().find(|item| item.id == label)?.offset;
                patch_tail_branch(&mut bytes, offset, target)?;
            }
            _ => return None,
        }
    }
    Some(bytes)
}

#[cfg(all(test, target_arch = "aarch64"))]
mod tests {
    use super::*;

    const MOVE_ACCUMULATOR_TO_RETURN: u32 = 0xaa04_03e0;
    const TEST_CONDITION_START_PC: usize = 0;
    const TEST_CONDITION_END_PC: usize = 4;
    const TEST_CONSTANT_CONDITION_END_PC: usize = 2;
    const TEST_CONDITION_REGISTER_COUNT: usize = 3;
    const TEST_RECURRENCE_END_PC: usize = 7;
    const TEST_RECURRENCE_REGISTER_COUNT: usize = 5;
    const TEST_LOCAL_SLOT: usize = 0;
    const TEST_LOAD_REGISTER: Register = 0;
    const TEST_LITERAL_REGISTER: Register = 1;
    const TEST_RESULT_REGISTER: Register = 2;
    const TEST_BOUND_REGISTER: Register = 3;
    const TEST_COMPARISON_REGISTER: Register = 4;
    const AARCH64_BRANCH_LINK_MASK: u32 = 0xfc00_0000;
    const AARCH64_BRANCH_LINK: u32 = 0x9400_0000;
    const AARCH64_BRANCH_LINK_REGISTER_MASK: u32 = 0xffff_fc1f;
    const AARCH64_BRANCH_LINK_REGISTER: u32 = 0xd63f_0000;
    const AARCH64_MOVE_WIDE_ZERO_64_BASE: u32 = 0xd280_0000;
    const AARCH64_SECOND_REGISTER_FIELD_SHIFT: u32 = 5;
    const AARCH64_MOVE_WIDE_IMMEDIATE_FIELD_SHIFT: u32 = 5;
    const AARCH64_UNSIGNED_OFFSET_FIELD_SHIFT: u32 = 10;
    const FRAME_ARGUMENT_REGISTER: u32 = 0;
    const EXIT_MARKER_REGISTER: u32 = 2;
    const CONDITION_TRUE_EXIT_MARKER: u16 = 1;
    const CONDITION_FALSE_EXIT_MARKER: u16 = 2;
    const REUSABLE_TEST_CALL_PC: usize = 0;
    const REUSABLE_TEST_RESULT_REGISTER: Register = 0;
    const REUSABLE_TEST_RESULT: f64 = 42.0;

    type RawStencilEntry = unsafe extern "C" fn(
        *mut u8,
        *mut raw_value::RawValue,
        *const u8,
        *mut u8,
        raw_value::RawValue,
        u64,
        u64,
        u64,
    ) -> raw_value::RawValue;

    #[repr(C)]
    struct RawRegionTestFrame {
        registers: *mut Value,
        locals: *mut Value,
        local_count: usize,
        current_site: *const InlineSite,
        sites: *const InlineSite,
        name_snapshots: *const Value,
        result: Value,
        region_arrays: *const RegionArrayView,
        region_guard: unsafe extern "C" fn(*mut RawRegionTestFrame, usize) -> usize,
        region_iterations: u64,
    }

    type RawRegionEntry = unsafe extern "C" fn(*mut RawRegionTestFrame, *const InlineSite);
    unsafe fn enter_raw_region_stencil(
        entry: RawRegionEntry,
        frame: *mut RawRegionTestFrame,
        site: *const InlineSite,
    ) {
        unsafe { entry(frame, site) };
    }

    unsafe extern "C" fn test_region_guard(_: *mut RawRegionTestFrame, _: usize) -> usize {
        0
    }

    fn condition_code(literal: Literal, kind: Op) -> DynCode {
        let instruction = |op| super::super::dynbytecode::DynInstr {
            op,
            span: Span::default(),
        };
        DynCode {
            ops: vec![
                instruction(DynOp::LoadLocal {
                    dst: TEST_LOAD_REGISTER,
                    slot: TEST_LOCAL_SLOT,
                }),
                instruction(DynOp::LoadLiteral {
                    dst: TEST_LITERAL_REGISTER,
                    value: literal,
                }),
                instruction(DynOp::Binary {
                    dst: TEST_RESULT_REGISTER,
                    left: TEST_LOAD_REGISTER,
                    right: TEST_LITERAL_REGISTER,
                    kind,
                }),
                instruction(DynOp::JumpIfFalse {
                    test: TEST_RESULT_REGISTER,
                    target: TEST_CONDITION_START_PC,
                }),
            ],
            registers: TEST_CONDITION_REGISTER_COUNT,
            params: Vec::new(),
            hoisted: Vec::new(),
            source_id: None,
            blocks: vec![(TEST_CONDITION_START_PC, TEST_CONDITION_END_PC, false)],
            bindings: Vec::new(),
            is_script: false,
        }
    }

    #[test]
    fn noncapturing_call_frame_layout_is_one_contiguous_value_range() {
        const LOCAL_COUNT: usize = 3;
        const REGISTER_COUNT: usize = 5;
        const SNAPSHOT_COUNT: usize = 7;
        const EXPECTED_REGISTER_BASE: usize = LOCAL_COUNT;
        const EXPECTED_SNAPSHOT_BASE: usize = LOCAL_COUNT + REGISTER_COUNT;
        const EXPECTED_SLOT_COUNT: usize = EXPECTED_SNAPSHOT_BASE + SNAPSHOT_COUNT;

        let layout = CallFrameLayout::new(LOCAL_COUNT, REGISTER_COUNT, SNAPSHOT_COUNT);
        assert_eq!(layout.local_count, LOCAL_COUNT);
        assert_eq!(layout.register_base, EXPECTED_REGISTER_BASE);
        assert_eq!(layout.snapshot_base, EXPECTED_SNAPSHOT_BASE);
        assert_eq!(layout.snapshot_count, SNAPSHOT_COUNT);
        assert_eq!(layout.slot_count, EXPECTED_SLOT_COUNT);

        let empty = CallFrameLayout::new(0, 0, 0);
        assert_eq!(empty.register_base, 0);
        assert_eq!(empty.snapshot_base, 0);
        assert_eq!(empty.snapshot_count, 0);
        assert_eq!(empty.slot_count, 0);
    }

    #[test]
    fn call_site_owns_one_pod_operand_view() {
        const CALL_PC: usize = 7;
        const DESTINATION: Register = 4;
        const CALLEE: Register = 1;
        const RECEIVER: Register = 2;
        const FIRST_ARGUMENT: Register = 3;
        const SECOND_ARGUMENT: Register = 5;
        const ARGUMENTS: [Register; 2] = [FIRST_ARGUMENT, SECOND_ARGUMENT];

        let instruction = super::super::dynbytecode::DynInstr {
            op: DynOp::Call {
                dst: DESTINATION,
                callee: CALLEE,
                receiver: RECEIVER,
                args: ARGUMENTS.to_vec(),
            },
            span: Span::default(),
        };
        let site = CallIcSite::new(CALL_PC, &instruction, true);
        assert_eq!(site.target.pc, CALL_PC);
        assert_eq!(site.target.dst, usize::from(DESTINATION));
        assert_eq!(site.target.callee, usize::from(CALLEE));
        assert_eq!(site.target.receiver, usize::from(RECEIVER));
        assert_eq!(site.target.arguments(), ARGUMENTS);
        assert_eq!(site.target.arguments, site._arguments.as_ptr());
        assert_eq!(site.target.span_start, instruction.span.start);
        assert!(site.target.recipe.get().is_null());
        assert!(site.target.environment.get().is_null());
    }

    #[test]
    fn function_call_recipe_is_the_canonical_call_layout() {
        const FIRST_PARAMETER: &str = "left";
        const SECOND_PARAMETER: &str = "right";
        const PARAMETER_COUNT: usize = 2;

        let mut code = one_step_code(vec![DynOp::Return { src: None }]);
        code.params = vec![FIRST_PARAMETER.into(), SECOND_PARAMETER.into()];
        let mut arena = CodeArena::new();
        let jit = DynJitCode::build(code, &mut arena, false).expect("build call recipe image");
        let recipe = &jit.call_recipe;
        assert_ne!(recipe.entry as usize, recipe.guest_entry as usize);
        assert!(
            recipe.guest_entry as usize > recipe.entry as usize,
            "guest entry skips the host-only prologue"
        );
        assert_eq!(recipe.layout.local_count, jit.binding_layout.len());
        assert_eq!(recipe.parameter_count, PARAMETER_COUNT);
        assert_eq!(recipe.parameter_slots(), jit._parameter_slots.as_ref());
        assert_eq!(recipe.this_slot, jit.binding_layout[THIS_BINDING_NAME]);
        assert_eq!(
            recipe.arguments_slot,
            jit.binding_layout[ARGUMENTS_BINDING_NAME]
        );
        assert!(!recipe.uses_arguments());
        assert!(!recipe.captures_frame());
    }

    #[test]
    fn guest_return_continuation_is_named_in_the_canonical_frame_abi() {
        assert_eq!(
            core::mem::offset_of!(GuestFrameHeader, return_target),
            RETURN_TARGET_FRAME_WORD_OFFSET * core::mem::size_of::<usize>()
        );
        assert_eq!(
            core::mem::size_of::<GuestFrameHeader>(),
            GUEST_FRAME_HEADER_WORDS * core::mem::size_of::<usize>()
        );
        assert_eq!(
            exit_adapter_template().bytes.as_ref(),
            machine_bytes(&[LOAD_RETURN_TARGET, BRANCH_TARGET_REGISTER], false).as_ref()
        );
    }

    fn reusable_call_fixture() -> (CallIcSite, Vm, CodeArena) {
        let instruction = super::super::dynbytecode::DynInstr {
            op: DynOp::Call {
                dst: REUSABLE_TEST_RESULT_REGISTER,
                callee: REUSABLE_TEST_RESULT_REGISTER,
                receiver: REUSABLE_TEST_RESULT_REGISTER,
                args: Vec::new(),
            },
            span: Span::default(),
        };
        let site = CallIcSite::new(REUSABLE_TEST_CALL_PC, &instruction, false);
        let callee_code = one_step_code(vec![
            DynOp::LoadLiteral {
                dst: REUSABLE_TEST_RESULT_REGISTER,
                value: Literal::Number(REUSABLE_TEST_RESULT),
            },
            DynOp::Return {
                src: Some(REUSABLE_TEST_RESULT_REGISTER),
            },
        ]);
        let mut arena = CodeArena::new();
        let callee = Rc::new(
            DynJitCode::build(callee_code, &mut arena, false)
                .expect("build reusable activation callee"),
        );
        let vm = Vm::new();
        site.fill(&Value::Undefined, callee, vm.global.clone());
        (site, vm, arena)
    }

    #[test]
    fn monomorphic_call_site_reuses_one_cleared_activation() {
        let (site, mut vm, _arena) = reusable_call_fixture();
        let first = site
            .call(&mut vm, Value::Undefined, &[] as &[Value])
            .expect("first cached call succeeds");
        assert_eq!(first.as_number(), Some(REUSABLE_TEST_RESULT));
        let first_address = {
            let cached = site.reusable_activation.borrow();
            let frame = cached.as_deref().expect("first call caches its activation");
            assert!(frame.owned_values.iter().all(Value::is_undefined));
            std::ptr::from_ref(frame) as usize
        };

        let second = site
            .call(&mut vm, Value::Undefined, &[] as &[Value])
            .expect("second cached call succeeds");
        assert_eq!(second.as_number(), Some(REUSABLE_TEST_RESULT));
        let second_address = site
            .reusable_activation
            .borrow()
            .as_deref()
            .map(|frame| std::ptr::from_ref(frame) as usize)
            .expect("second call returns the activation to the cache");
        assert_eq!(first_address, second_address);
    }

    fn selected_direct_name(code: &DynCode, end: usize) -> Option<&'static str> {
        match select_direct_block_template(code, TEST_CONDITION_START_PC, end) {
            Some(DirectBlockTemplate::Terminal { stencil_name }) => Some(stencil_name),
            Some(DirectBlockTemplate::Transfer { stencil_name, .. }) => Some(stencil_name),
            Some(DirectBlockTemplate::ConditionLocalLiteral { stencil_name, .. }) => {
                Some(stencil_name)
            }
            Some(DirectBlockTemplate::UpdateLocalLiteralJump { stencil_name, .. }) => {
                Some(stencil_name)
            }
            Some(DirectBlockTemplate::ConditionLocalName { stencil_name, .. }) => {
                Some(stencil_name)
            }
            Some(DirectBlockTemplate::InstanceOfCondition { stencil_name, .. }) => {
                Some(stencil_name)
            }
            Some(DirectBlockTemplate::PropertyCondition { stencil_name, .. }) => Some(stencil_name),
            _ => None,
        }
    }

    fn selected_condition_name(code: &DynCode) -> Option<&'static str> {
        selected_direct_name(code, TEST_CONDITION_END_PC)
    }

    #[test]
    fn instanceof_condition_selection_requires_exact_dead_flow() {
        const RECEIVER_REGISTER: Register = 0;
        const CONSTRUCTOR_REGISTER: Register = 1;
        const MEMBERSHIP_REGISTER: Register = 2;
        const INVERTED_REGISTER: Register = 3;
        const RECEIVER_LOCAL: usize = 0;
        const CONSTRUCTOR_NAME: &str = "Constructor";
        const BRANCH_TARGET: usize = 0;

        let instruction = |op| super::super::dynbytecode::DynInstr {
            op,
            span: Span::default(),
        };
        let condition = DynCode {
            ops: vec![
                instruction(DynOp::LoadLocal {
                    dst: RECEIVER_REGISTER,
                    slot: RECEIVER_LOCAL,
                }),
                instruction(DynOp::LoadName {
                    dst: CONSTRUCTOR_REGISTER,
                    name: CONSTRUCTOR_NAME.into(),
                }),
                instruction(DynOp::InstanceOf {
                    dst: MEMBERSHIP_REGISTER,
                    left: RECEIVER_REGISTER,
                    right: CONSTRUCTOR_REGISTER,
                }),
                instruction(DynOp::JumpIfFalse {
                    test: MEMBERSHIP_REGISTER,
                    target: BRANCH_TARGET,
                }),
            ],
            registers: INVERTED_REGISTER as usize + 1,
            params: Vec::new(),
            hoisted: Vec::new(),
            source_id: None,
            blocks: vec![(
                INSTANCEOF_LOAD_LOCAL_PC_OFFSET,
                INSTANCEOF_INSTRUCTION_COUNT,
                false,
            )],
            bindings: Vec::new(),
            is_script: false,
        };
        assert_eq!(
            selected_direct_name(&condition, INSTANCEOF_INSTRUCTION_COUNT),
            Some("quench_dyn_dead_cached_instanceof_condition")
        );

        let mut inverted = DynCode {
            ops: vec![
                instruction(DynOp::LoadLocal {
                    dst: RECEIVER_REGISTER,
                    slot: RECEIVER_LOCAL,
                }),
                instruction(DynOp::LoadName {
                    dst: CONSTRUCTOR_REGISTER,
                    name: CONSTRUCTOR_NAME.into(),
                }),
                instruction(DynOp::InstanceOf {
                    dst: MEMBERSHIP_REGISTER,
                    left: RECEIVER_REGISTER,
                    right: CONSTRUCTOR_REGISTER,
                }),
                instruction(DynOp::Unary {
                    dst: INVERTED_REGISTER,
                    src: MEMBERSHIP_REGISTER,
                    kind: UnaryKind::Not,
                }),
                instruction(DynOp::JumpIfFalse {
                    test: INVERTED_REGISTER,
                    target: BRANCH_TARGET,
                }),
            ],
            registers: INVERTED_REGISTER as usize + 1,
            params: Vec::new(),
            hoisted: Vec::new(),
            source_id: None,
            blocks: vec![(
                INSTANCEOF_LOAD_LOCAL_PC_OFFSET,
                INSTANCEOF_NOT_INSTRUCTION_COUNT,
                false,
            )],
            bindings: Vec::new(),
            is_script: false,
        };
        assert_eq!(
            selected_direct_name(&inverted, INSTANCEOF_NOT_INSTRUCTION_COUNT),
            Some("quench_dyn_dead_cached_instanceof_not_condition")
        );

        inverted.ops.push(instruction(DynOp::Return {
            src: Some(MEMBERSHIP_REGISTER),
        }));
        assert_eq!(
            selected_direct_name(&inverted, INSTANCEOF_NOT_INSTRUCTION_COUNT),
            None
        );
    }

    #[test]
    fn instanceof_cache_records_complete_membership_and_observes_epoch() {
        let allocator = Allocator::default();
        let parsed = Parser::new(
            &allocator,
            "function Constructor() {}",
            SourceType::default(),
        )
        .parse();
        let function = match &parsed.program.body[0] {
            Statement::FunctionDeclaration(function) => function,
            _ => panic!("expected function"),
        };
        let function =
            unsafe { std::mem::transmute::<&Function<'_>, &Function<'static>>(function) };
        let vm = Vm::new();
        let constructor = vm.make_user(function, vm.global.clone());
        let prototype = constructor
            .as_function_ref()
            .expect("constructor function")
            .prototype
            .clone();
        let receiver = vm.object(Some(prototype));
        let cache = InstanceOfIcSite::new();
        let first_epoch = vm.prototype_epoch.get();

        assert_eq!(
            instance_of_cache_hit(&receiver, &constructor, &cache, first_epoch),
            None
        );
        assert!(instance_of_cached(
            &receiver,
            &constructor,
            &cache,
            first_epoch
        ));
        assert_eq!(
            instance_of_cache_hit(&receiver, &constructor, &cache, first_epoch),
            Some(true)
        );

        receiver
            .as_object_ref()
            .expect("receiver object")
            .borrow_mut()
            .prototype = Some(vm.allocate_object(Object::ordinary(None)));
        vm.invalidate_prototype_membership();
        let second_epoch = vm.prototype_epoch.get();
        assert_ne!(first_epoch, second_epoch);
        assert_eq!(
            instance_of_cache_hit(&receiver, &constructor, &cache, second_epoch),
            None
        );
        assert!(!instance_of_cached(
            &receiver,
            &constructor,
            &cache,
            second_epoch
        ));
        assert_eq!(
            instance_of_cache_hit(&receiver, &constructor, &cache, second_epoch),
            Some(false)
        );
    }

    fn terminal_code(ops: Vec<DynOp>) -> DynCode {
        let end = ops.len();
        DynCode {
            ops: ops
                .into_iter()
                .map(|op| super::super::dynbytecode::DynInstr {
                    op,
                    span: Span::default(),
                })
                .collect(),
            registers: TEST_CONDITION_REGISTER_COUNT,
            params: Vec::new(),
            hoisted: Vec::new(),
            source_id: None,
            blocks: vec![(TEST_CONDITION_START_PC, end, false)],
            bindings: Vec::new(),
            is_script: false,
        }
    }

    #[test]
    fn terminal_block_selection_is_structural_and_rejects_near_misses() {
        let undefined = terminal_code(vec![DynOp::Return { src: None }]);
        assert_eq!(
            selected_direct_name(&undefined, undefined.ops.len()),
            Some("quench_dyn_return_undefined")
        );

        let transfer = terminal_code(vec![DynOp::Jump {
            target: TEST_CONDITION_START_PC,
        }]);
        assert_eq!(
            selected_direct_name(&transfer, transfer.ops.len()),
            Some("quench_dyn_jump")
        );

        let local = terminal_code(vec![
            DynOp::LoadLocal {
                dst: TEST_LOAD_REGISTER,
                slot: TEST_LOCAL_SLOT,
            },
            DynOp::Return {
                src: Some(TEST_LOAD_REGISTER),
            },
        ]);
        assert_eq!(
            selected_direct_name(&local, local.ops.len()),
            Some("quench_dyn_return_local")
        );

        let literal = terminal_code(vec![
            DynOp::LoadLiteral {
                dst: TEST_LITERAL_REGISTER,
                value: Literal::Number(42.0),
            },
            DynOp::Return {
                src: Some(TEST_LITERAL_REGISTER),
            },
        ]);
        assert_eq!(
            selected_direct_name(&literal, literal.ops.len()),
            Some("quench_dyn_return_literal")
        );

        let allocating_literal = terminal_code(vec![
            DynOp::LoadLiteral {
                dst: TEST_LITERAL_REGISTER,
                value: Literal::String("not immediate".into()),
            },
            DynOp::Return {
                src: Some(TEST_LITERAL_REGISTER),
            },
        ]);
        assert_eq!(
            selected_direct_name(&allocating_literal, allocating_literal.ops.len()),
            None
        );

        let mismatched_register = terminal_code(vec![
            DynOp::LoadLocal {
                dst: TEST_LOAD_REGISTER,
                slot: TEST_LOCAL_SLOT,
            },
            DynOp::Return {
                src: Some(TEST_LITERAL_REGISTER),
            },
        ]);
        assert_eq!(
            selected_direct_name(&mismatched_register, mismatched_register.ops.len()),
            None
        );
    }

    #[test]
    fn own_property_equality_selection_is_structural_and_liveness_checked() {
        const FIRST_RECEIVER_REGISTER: Register = 0;
        const FIRST_VALUE_REGISTER: Register = 1;
        const SECOND_RECEIVER_REGISTER: Register = 2;
        const SECOND_VALUE_REGISTER: Register = 3;
        const COMPARISON_REGISTER: Register = 4;
        const FIRST_RECEIVER_LOCAL: usize = 0;
        const SECOND_RECEIVER_LOCAL: usize = 1;
        const PROPERTY_NAME: &str = "field";
        const BRANCH_TARGET: usize = 0;
        const PROPERTY_CONDITION_INSTRUCTION_COUNT: usize = 6;

        let condition = || {
            terminal_code(vec![
                DynOp::LoadLocal {
                    dst: FIRST_RECEIVER_REGISTER,
                    slot: FIRST_RECEIVER_LOCAL,
                },
                DynOp::GetStatic {
                    dst: FIRST_VALUE_REGISTER,
                    object: FIRST_RECEIVER_REGISTER,
                    key: PROPERTY_NAME.into(),
                },
                DynOp::LoadLocal {
                    dst: SECOND_RECEIVER_REGISTER,
                    slot: SECOND_RECEIVER_LOCAL,
                },
                DynOp::GetStatic {
                    dst: SECOND_VALUE_REGISTER,
                    object: SECOND_RECEIVER_REGISTER,
                    key: PROPERTY_NAME.into(),
                },
                DynOp::Binary {
                    dst: COMPARISON_REGISTER,
                    left: FIRST_VALUE_REGISTER,
                    right: SECOND_VALUE_REGISTER,
                    kind: Op::StrictEq,
                },
                DynOp::JumpIfFalse {
                    test: COMPARISON_REGISTER,
                    target: BRANCH_TARGET,
                },
            ])
        };

        let selected = condition();
        assert_eq!(
            selected_direct_name(&selected, selected.ops.len()),
            Some("quench_dyn_dead_own_property_strict_equal")
        );

        let mut coercing = condition();
        let DynOp::Binary { kind, .. } = &mut coercing.ops[PROPERTY_EQUAL_COMPARE_PC_OFFSET].op
        else {
            unreachable!()
        };
        *kind = Op::Eq;
        assert_eq!(selected_direct_name(&coercing, coercing.ops.len()), None);

        let mut live = condition();
        live.ops.push(super::super::dynbytecode::DynInstr {
            op: DynOp::Return {
                src: Some(FIRST_VALUE_REGISTER),
            },
            span: Span::default(),
        });
        assert_eq!(
            selected_direct_name(&live, PROPERTY_CONDITION_INSTRUCTION_COUNT),
            None
        );
    }

    #[test]
    fn own_property_load_store_jump_selection_requires_exact_dead_flow() {
        const RECEIVER_REGISTER: Register = 0;
        const PROPERTY_REGISTER: Register = 1;
        const RECEIVER_LOCAL: usize = 0;
        const DESTINATION_LOCAL: usize = 1;
        const PROPERTY_NAME: &str = "field";
        const JUMP_TARGET: usize = 0;

        let block = || {
            terminal_code(vec![
                DynOp::LoadLocal {
                    dst: RECEIVER_REGISTER,
                    slot: RECEIVER_LOCAL,
                },
                DynOp::GetStatic {
                    dst: PROPERTY_REGISTER,
                    object: RECEIVER_REGISTER,
                    key: PROPERTY_NAME.into(),
                },
                DynOp::StoreLocal {
                    slot: DESTINATION_LOCAL,
                    src: PROPERTY_REGISTER,
                },
                DynOp::Jump {
                    target: JUMP_TARGET,
                },
            ])
        };

        let selected = block();
        assert_eq!(
            selected_direct_name(&selected, selected.ops.len()),
            Some("quench_dyn_dead_own_property_store_local_jump")
        );

        let mut mismatched = block();
        let DynOp::GetStatic { object, .. } = &mut mismatched.ops[PROPERTY_LOAD_GET_PC_OFFSET].op
        else {
            unreachable!()
        };
        *object = PROPERTY_REGISTER;
        assert_eq!(
            selected_direct_name(&mismatched, PROPERTY_LOAD_JUMP_INSTRUCTION_COUNT),
            None
        );

        let mut live = block();
        live.ops.push(super::super::dynbytecode::DynInstr {
            op: DynOp::Return {
                src: Some(PROPERTY_REGISTER),
            },
            span: Span::default(),
        });
        assert_eq!(
            selected_direct_name(&live, PROPERTY_LOAD_JUMP_INSTRUCTION_COUNT),
            None
        );
    }

    #[test]
    fn ownership_taking_property_store_selection_requires_terminal_dead_sources() {
        const FIRST_RECEIVER_REGISTER: Register = 0;
        const FIRST_VALUE_REGISTER: Register = 1;
        const SECOND_RECEIVER_REGISTER: Register = 2;
        const SECOND_VALUE_REGISTER: Register = 3;
        const FIRST_RECEIVER_LOCAL: usize = 0;
        const FIRST_SOURCE_LOCAL: usize = 1;
        const SECOND_RECEIVER_LOCAL: usize = 2;
        const SECOND_SOURCE_LOCAL: usize = 3;
        const FIRST_PROPERTY_NAME: &str = "first";
        const SECOND_PROPERTY_NAME: &str = "second";

        let terminal_stores = || {
            terminal_code(vec![
                DynOp::LoadLocal {
                    dst: FIRST_RECEIVER_REGISTER,
                    slot: FIRST_RECEIVER_LOCAL,
                },
                DynOp::LoadLocal {
                    dst: FIRST_VALUE_REGISTER,
                    slot: FIRST_SOURCE_LOCAL,
                },
                DynOp::SetStatic {
                    object: FIRST_RECEIVER_REGISTER,
                    key: FIRST_PROPERTY_NAME.to_owned(),
                    src: FIRST_VALUE_REGISTER,
                },
                DynOp::LoadLocal {
                    dst: SECOND_RECEIVER_REGISTER,
                    slot: SECOND_RECEIVER_LOCAL,
                },
                DynOp::LoadLocal {
                    dst: SECOND_VALUE_REGISTER,
                    slot: SECOND_SOURCE_LOCAL,
                },
                DynOp::SetStatic {
                    object: SECOND_RECEIVER_REGISTER,
                    key: SECOND_PROPERTY_NAME.to_owned(),
                    src: SECOND_VALUE_REGISTER,
                },
                DynOp::Return { src: None },
            ])
        };

        let selected = terminal_stores();
        assert_eq!(
            selected_direct_name(&selected, selected.ops.len()),
            Some("quench_dyn_take_two_own_properties_return_undefined")
        );
        let this_slot = function_binding_layout(&selected)[THIS_BINDING_NAME];
        let mut constructor = terminal_stores();
        let DynOp::LoadLocal { slot, .. } =
            &mut constructor.ops[TWO_PROPERTY_STORE_FIRST_RECEIVER_LOAD_PC_OFFSET].op
        else {
            unreachable!()
        };
        *slot = this_slot;
        let DynOp::LoadLocal { slot, .. } =
            &mut constructor.ops[TWO_PROPERTY_STORE_SECOND_RECEIVER_LOAD_PC_OFFSET].op
        else {
            unreachable!()
        };
        *slot = this_slot;
        let constructor_shape = terminal_constructor_shape(&constructor, this_slot)
            .expect("pure terminal constructor shape is statically known");
        assert_eq!(constructor_shape.slot(FIRST_PROPERTY_NAME), Some(0));
        assert_eq!(constructor_shape.slot(SECOND_PROPERTY_NAME), Some(1));

        let mut shared_source = terminal_stores();
        let DynOp::LoadLocal { slot, .. } =
            &mut shared_source.ops[TWO_PROPERTY_STORE_SECOND_VALUE_LOAD_PC_OFFSET].op
        else {
            unreachable!()
        };
        *slot = FIRST_SOURCE_LOCAL;
        assert_eq!(
            selected_direct_name(&shared_source, shared_source.ops.len()),
            None
        );

        let mut observed_afterward = terminal_stores();
        observed_afterward
            .ops
            .push(super::super::dynbytecode::DynInstr {
                op: DynOp::Return {
                    src: Some(FIRST_VALUE_REGISTER),
                },
                span: Span::default(),
            });
        assert_eq!(
            selected_direct_name(&observed_afterward, TWO_PROPERTY_STORE_INSTRUCTION_COUNT),
            None
        );
    }

    #[test]
    fn own_property_nullish_selection_is_structural_and_liveness_checked() {
        const RECEIVER_REGISTER: Register = 0;
        const PROPERTY_REGISTER: Register = 1;
        const LITERAL_REGISTER: Register = 2;
        const COMPARISON_REGISTER: Register = 3;
        const PROPERTY_LOCAL: usize = 0;
        const PROPERTY_NAME: &str = "field";
        const BRANCH_TARGET: usize = 0;
        const INSTRUCTION_COUNT: usize = 5;

        let condition = |literal, kind| {
            terminal_code(vec![
                DynOp::LoadLocal {
                    dst: RECEIVER_REGISTER,
                    slot: PROPERTY_LOCAL,
                },
                DynOp::GetStatic {
                    dst: PROPERTY_REGISTER,
                    object: RECEIVER_REGISTER,
                    key: PROPERTY_NAME.into(),
                },
                DynOp::LoadLiteral {
                    dst: LITERAL_REGISTER,
                    value: literal,
                },
                DynOp::Binary {
                    dst: COMPARISON_REGISTER,
                    left: PROPERTY_REGISTER,
                    right: LITERAL_REGISTER,
                    kind,
                },
                DynOp::JumpIfFalse {
                    test: COMPARISON_REGISTER,
                    target: BRANCH_TARGET,
                },
            ])
        };
        let cases = [
            (
                Literal::Null,
                Op::Eq,
                "quench_dyn_dead_own_property_nullish_equal",
            ),
            (
                Literal::Undefined,
                Op::Ne,
                "quench_dyn_dead_own_property_nullish_not_equal",
            ),
            (
                Literal::Null,
                Op::StrictEq,
                "quench_dyn_dead_own_property_immediate_strict_equal",
            ),
            (
                Literal::Undefined,
                Op::StrictNe,
                "quench_dyn_dead_own_property_immediate_strict_not_equal",
            ),
        ];
        for (literal, kind, expected) in cases {
            let code = condition(literal, kind);
            assert_eq!(selected_direct_name(&code, code.ops.len()), Some(expected));
        }
        let number = condition(Literal::Number(0.0), Op::Eq);
        assert_eq!(selected_direct_name(&number, number.ops.len()), None);

        let mut live = condition(Literal::Null, Op::Eq);
        live.ops.push(super::super::dynbytecode::DynInstr {
            op: DynOp::Return {
                src: Some(PROPERTY_REGISTER),
            },
            span: Span::default(),
        });
        assert_eq!(selected_direct_name(&live, INSTRUCTION_COUNT), None);
    }

    #[test]
    fn constant_condition_selection_erases_only_dead_immediate_tests() {
        let condition = |value| {
            terminal_code(vec![
                DynOp::LoadLiteral {
                    dst: TEST_LITERAL_REGISTER,
                    value,
                },
                DynOp::JumpIfFalse {
                    test: TEST_LITERAL_REGISTER,
                    target: TEST_CONDITION_START_PC,
                },
            ])
        };
        for value in [Literal::Bool(true), Literal::Number(1.0)] {
            let code = condition(value);
            assert_eq!(
                selected_direct_name(&code, code.ops.len()),
                Some("quench_dyn_constant_truthy")
            );
        }
        for value in [
            Literal::Undefined,
            Literal::Null,
            Literal::Bool(false),
            Literal::Number(0.0),
            Literal::Number(-0.0),
            Literal::Number(f64::NAN),
        ] {
            let code = condition(value);
            assert_eq!(
                selected_direct_name(&code, code.ops.len()),
                Some("quench_dyn_constant_falsey")
            );
        }
        let string = condition(Literal::String("allocating".into()));
        assert_eq!(selected_direct_name(&string, string.ops.len()), None);

        let mut live = condition(Literal::Bool(true));
        live.ops.push(super::super::dynbytecode::DynInstr {
            op: DynOp::Return {
                src: Some(TEST_LITERAL_REGISTER),
            },
            span: Span::default(),
        });
        assert_eq!(
            selected_direct_name(&live, TEST_CONSTANT_CONDITION_END_PC),
            None
        );
        assert_eq!(
            bytecode_target_label(live.ops.len(), live.ops.len()),
            FUNCTION_EXIT_LABEL
        );
    }

    fn recurrence_code(update_kind: Op, compare_kind: Op) -> DynCode {
        let instruction = |op| super::super::dynbytecode::DynInstr {
            op,
            span: Span::default(),
        };
        DynCode {
            ops: vec![
                instruction(DynOp::LoadLocal {
                    dst: TEST_LOAD_REGISTER,
                    slot: TEST_LOCAL_SLOT,
                }),
                instruction(DynOp::LoadLiteral {
                    dst: TEST_LITERAL_REGISTER,
                    value: Literal::Number(1.0),
                }),
                instruction(DynOp::Binary {
                    dst: TEST_RESULT_REGISTER,
                    left: TEST_LOAD_REGISTER,
                    right: TEST_LITERAL_REGISTER,
                    kind: update_kind,
                }),
                instruction(DynOp::StoreLocal {
                    slot: TEST_LOCAL_SLOT,
                    src: TEST_RESULT_REGISTER,
                }),
                instruction(DynOp::LoadLiteral {
                    dst: TEST_BOUND_REGISTER,
                    value: Literal::Number(0.0),
                }),
                instruction(DynOp::Binary {
                    dst: TEST_COMPARISON_REGISTER,
                    left: TEST_RESULT_REGISTER,
                    right: TEST_BOUND_REGISTER,
                    kind: compare_kind,
                }),
                instruction(DynOp::JumpIfFalse {
                    test: TEST_COMPARISON_REGISTER,
                    target: TEST_CONDITION_START_PC,
                }),
            ],
            registers: TEST_RECURRENCE_REGISTER_COUNT,
            params: Vec::new(),
            hoisted: Vec::new(),
            source_id: None,
            blocks: vec![(TEST_CONDITION_START_PC, TEST_RECURRENCE_END_PC, false)],
            bindings: Vec::new(),
            is_script: false,
        }
    }

    fn name_condition_code(kind: Op) -> DynCode {
        let instruction = |op| super::super::dynbytecode::DynInstr {
            op,
            span: Span::default(),
        };
        DynCode {
            ops: vec![
                instruction(DynOp::LoadLocal {
                    dst: TEST_LOAD_REGISTER,
                    slot: TEST_LOCAL_SLOT,
                }),
                instruction(DynOp::LoadName {
                    dst: TEST_LITERAL_REGISTER,
                    name: "bound".into(),
                }),
                instruction(DynOp::Binary {
                    dst: TEST_RESULT_REGISTER,
                    left: TEST_LOAD_REGISTER,
                    right: TEST_LITERAL_REGISTER,
                    kind,
                }),
                instruction(DynOp::JumpIfFalse {
                    test: TEST_RESULT_REGISTER,
                    target: TEST_CONDITION_START_PC,
                }),
            ],
            registers: TEST_CONDITION_REGISTER_COUNT,
            params: Vec::new(),
            hoisted: Vec::new(),
            source_id: None,
            blocks: vec![(TEST_CONDITION_START_PC, TEST_CONDITION_END_PC, false)],
            bindings: Vec::new(),
            is_script: false,
        }
    }

    #[test]
    fn dead_condition_selection_is_semantic_and_literal_typed() {
        let cases = [
            (
                Literal::Number(f64::NAN),
                Op::Eq,
                "quench_dyn_dead_condition_local_number_equal",
            ),
            (
                Literal::Number(-0.0),
                Op::StrictEq,
                "quench_dyn_dead_condition_local_number_equal",
            ),
            (
                Literal::Null,
                Op::StrictEq,
                "quench_dyn_dead_condition_local_immediate_strict_equal",
            ),
            (
                Literal::Undefined,
                Op::Eq,
                "quench_dyn_dead_condition_local_nullish_equal",
            ),
            (
                Literal::Bool(true),
                Op::StrictNe,
                "quench_dyn_dead_condition_local_immediate_strict_not_equal",
            ),
        ];
        for (literal, kind, expected) in cases {
            assert_eq!(
                selected_condition_name(&condition_code(literal, kind)),
                Some(expected)
            );
        }
        assert_eq!(
            selected_condition_name(&condition_code(Literal::String("1".into()), Op::Eq)),
            None,
            "string coercion remains on the canonical slow path"
        );
    }

    #[test]
    fn dead_condition_selection_requires_all_intermediates_to_be_dead() {
        let mut code = condition_code(Literal::Number(1.0), Op::Eq);
        code.ops.push(super::super::dynbytecode::DynInstr {
            op: DynOp::Return {
                src: Some(TEST_LOAD_REGISTER),
            },
            span: Span::default(),
        });
        assert_eq!(selected_condition_name(&code), None);
    }

    #[test]
    fn dead_recurrence_selection_composes_update_compare_and_branch() {
        let code = recurrence_code(Op::Sub, Op::Ge);
        assert_eq!(
            selected_direct_name(&code, TEST_RECURRENCE_END_PC),
            Some("quench_dyn_dead_recurrence_subtract_greater_equal")
        );

        let mut live_code = recurrence_code(Op::Sub, Op::Ge);
        live_code.ops.push(super::super::dynbytecode::DynInstr {
            op: DynOp::Return {
                src: Some(TEST_RESULT_REGISTER),
            },
            span: Span::default(),
        });
        assert_eq!(
            selected_direct_name(&live_code, TEST_RECURRENCE_END_PC),
            None
        );
    }

    #[test]
    fn captured_name_condition_uses_an_effect_refreshed_snapshot() {
        let code = name_condition_code(Op::Le);
        assert_eq!(
            selected_condition_name(&code),
            Some("quench_dyn_dead_condition_local_name_number_less_equal")
        );
        assert_eq!(
            direct_name_snapshot_pcs(&code, &RegionPlan::quote(&code).unwrap(), true),
            vec![NAME_OPERAND_PC_OFFSET]
        );

        let mut script = name_condition_code(Op::Le);
        script.is_script = true;
        assert_eq!(
            selected_condition_name(&script),
            Some("quench_dyn_dead_condition_local_name_number_less_equal")
        );

        let mut effectful = name_condition_code(Op::Le);
        effectful.registers = TEST_RECURRENCE_REGISTER_COUNT;
        effectful.ops.push(super::super::dynbytecode::DynInstr {
            op: DynOp::Call {
                dst: TEST_COMPARISON_REGISTER,
                callee: TEST_BOUND_REGISTER,
                receiver: TEST_COMPARISON_REGISTER,
                args: Vec::new(),
            },
            span: Span::default(),
        });
        assert_eq!(
            selected_condition_name(&effectful),
            Some("quench_dyn_dead_condition_local_name_number_less_equal")
        );
    }

    #[test]
    fn ordinary_load_name_selects_the_lazy_cached_stencil() {
        const CAPTURED_NAME: &str = "captured";
        let op = DynOp::LoadName {
            dst: TEST_RESULT_REGISTER,
            name: CAPTURED_NAME.to_owned(),
        };
        assert!(matches!(
            select_direct_opcode_template(&op),
            Some(DirectOpcodeTemplate::Next("quench_dyn_load_name_cached"))
        ));
    }

    #[test]
    fn guarded_direct_bitwise_stencils_match_number_semantics() {
        const LEFT_REGISTER: Register = TEST_LOAD_REGISTER;
        const RIGHT_REGISTER: Register = TEST_LITERAL_REGISTER;
        let cases = [
            (Op::And, 6.0, 3.0, 2.0),
            (Op::Or, 6.0, 3.0, 7.0),
            (Op::Xor, 6.0, 3.0, 5.0),
            (Op::Shl, 3.0, 2.0, 12.0),
            (Op::Shr, -8.0, 2.0, -2.0),
            (Op::Ushr, -1.0, 1.0, 2_147_483_647.0),
        ];
        let mut vm = Vm::new();
        for (kind, left, right, expected) in cases {
            assert!(
                select_direct_opcode_template(&DynOp::Binary {
                    dst: TEST_RESULT_REGISTER,
                    left: LEFT_REGISTER,
                    right: RIGHT_REGISTER,
                    kind,
                })
                .is_some()
            );
            let code = single_block_code(vec![
                DynOp::LoadLiteral {
                    dst: LEFT_REGISTER,
                    value: Literal::Number(left),
                },
                DynOp::LoadLiteral {
                    dst: RIGHT_REGISTER,
                    value: Literal::Number(right),
                },
                DynOp::Binary {
                    dst: TEST_RESULT_REGISTER,
                    left: LEFT_REGISTER,
                    right: RIGHT_REGISTER,
                    kind,
                },
                DynOp::Return {
                    src: Some(TEST_RESULT_REGISTER),
                },
            ]);
            let result = call_one_step_code(code, &mut vm).expect("direct bitwise stencil runs");
            assert_eq!(result.as_number(), Some(expected));
        }
    }

    fn single_block_code(ops: Vec<DynOp>) -> DynCode {
        let end = ops.len();
        let mut code = one_step_code(ops);
        code.blocks = vec![(TEST_CONDITION_START_PC, end, false)];
        code
    }

    fn test_call_op() -> DynOp {
        DynOp::Call {
            dst: TEST_RESULT_REGISTER,
            callee: TEST_LOAD_REGISTER,
            receiver: TEST_LITERAL_REGISTER,
            args: Vec::new(),
        }
    }

    #[test]
    fn call_region_requires_one_call_and_total_surrounding_direct_cover() {
        let code = single_block_code(vec![
            DynOp::LoadLiteral {
                dst: TEST_LOAD_REGISTER,
                value: Literal::Undefined,
            },
            test_call_op(),
            DynOp::Return {
                src: Some(TEST_RESULT_REGISTER),
            },
        ]);
        assert_eq!(
            select_call_region(&code, TEST_CONDITION_START_PC, code.ops.len()),
            Some(CallRegion {
                start: TEST_CONDITION_START_PC,
                call_pc: TEST_CONDITION_START_PC + NEXT_INSTRUCTION_DISTANCE,
                end: code.ops.len(),
            })
        );

        let two_calls = single_block_code(vec![
            test_call_op(),
            test_call_op(),
            DynOp::Return {
                src: Some(TEST_RESULT_REGISTER),
            },
        ]);
        assert!(
            select_call_region(&two_calls, TEST_CONDITION_START_PC, two_calls.ops.len()).is_none()
        );

        let unsupported = single_block_code(vec![
            DynOp::NewObject {
                dst: TEST_LOAD_REGISTER,
            },
            test_call_op(),
            DynOp::Return {
                src: Some(TEST_RESULT_REGISTER),
            },
        ]);
        assert!(
            select_call_region(&unsupported, TEST_CONDITION_START_PC, unsupported.ops.len())
                .is_none()
        );
    }

    #[test]
    fn ordinary_name_loads_use_lazy_ic_without_snapshot_slots() {
        const CAPTURED_NAME: &str = "captured";
        const FIRST_LOAD_PC: usize = 4;
        const SECOND_LOAD_PC: usize = FIRST_LOAD_PC + NEXT_INSTRUCTION_DISTANCE;
        const EXPECTED_SNAPSHOT_COUNT: usize = 0;
        let mut ops = vec![
            DynOp::LoadLiteral {
                dst: TEST_RESULT_REGISTER,
                value: Literal::Undefined,
            };
            FIRST_LOAD_PC
        ];
        ops.push(DynOp::LoadName {
            dst: TEST_RESULT_REGISTER,
            name: CAPTURED_NAME.to_owned(),
        });
        ops.push(DynOp::LoadName {
            dst: TEST_LITERAL_REGISTER,
            name: CAPTURED_NAME.to_owned(),
        });
        ops.push(DynOp::Return {
            src: Some(TEST_RESULT_REGISTER),
        });
        let code = one_step_code(ops);
        assert_eq!(
            direct_name_snapshot_pcs(&code, &RegionPlan::quote(&code).unwrap(), true),
            Vec::<usize>::new()
        );

        let mut arena = CodeArena::new();
        let jit = DynJitCode::build(code, &mut arena, false).expect("build direct name load");
        assert_eq!(jit.name_snapshot_slot_count(), EXPECTED_SNAPSHOT_COUNT);
        assert_eq!(
            jit.call_recipe.layout.snapshot_count,
            EXPECTED_SNAPSHOT_COUNT
        );
        assert!(jit.name_ics[FIRST_LOAD_PC].get().is_none());
        assert!(jit.name_ics[SECOND_LOAD_PC].get().is_none());
    }

    #[test]
    fn store_name_effect_refreshes_a_later_direct_load() {
        const CAPTURED_NAME: &str = "captured";
        const INITIAL_VALUE: f64 = 3.0;
        const REPLACEMENT_VALUE: f64 = 11.0;
        let mut vm = Vm::new();
        Environment::set(&vm.global, CAPTURED_NAME, Value::Number(INITIAL_VALUE));
        let code = one_step_code(vec![
            DynOp::LoadLiteral {
                dst: TEST_LITERAL_REGISTER,
                value: Literal::Number(REPLACEMENT_VALUE),
            },
            DynOp::StoreName {
                name: CAPTURED_NAME.to_owned(),
                src: TEST_LITERAL_REGISTER,
            },
            DynOp::LoadName {
                dst: TEST_RESULT_REGISTER,
                name: CAPTURED_NAME.to_owned(),
            },
            DynOp::Return {
                src: Some(TEST_RESULT_REGISTER),
            },
        ]);
        let result = call_one_step_code(code, &mut vm).expect("execute refreshed name load");
        assert_eq!(result.as_number(), Some(REPLACEMENT_VALUE));
    }

    #[test]
    fn direct_name_ic_hits_and_refills_after_layout_change() {
        const CAPTURED_NAME: &str = "captured";
        const NEW_BINDING_NAME: &str = "newBinding";
        const INITIAL_VALUE: f64 = 5.0;
        const REPLACEMENT_VALUE: f64 = 13.0;
        const NEW_BINDING_VALUE: f64 = 21.0;
        const LOAD_PC: usize = 0;
        let code = one_step_code(vec![
            DynOp::LoadName {
                dst: TEST_RESULT_REGISTER,
                name: CAPTURED_NAME.to_owned(),
            },
            DynOp::Return {
                src: Some(TEST_RESULT_REGISTER),
            },
        ]);
        let mut arena = CodeArena::new();
        let jit = DynJitCode::build(code, &mut arena, false).expect("build direct name IC");
        let mut vm = Vm::new();
        Environment::set(&vm.global, CAPTURED_NAME, Value::Number(INITIAL_VALUE));

        let first_environment = vm.global.clone();
        let first = jit
            .call(
                &mut vm,
                first_environment,
                Value::Undefined,
                &[] as &[Value],
            )
            .expect("first access fills lexical-address IC");
        assert_eq!(first.as_number(), Some(INITIAL_VALUE));
        let first_layout = jit.name_ics[LOAD_PC]
            .get()
            .expect("first access publishes lexical address")
            .layout;

        Environment::set(
            &vm.global,
            NEW_BINDING_NAME,
            Value::Number(NEW_BINDING_VALUE),
        );
        Environment::set(&vm.global, CAPTURED_NAME, Value::Number(REPLACEMENT_VALUE));
        let second_environment = vm.global.clone();
        let second = jit
            .call(
                &mut vm,
                second_environment,
                Value::Undefined,
                &[] as &[Value],
            )
            .expect("layout miss refills lexical-address IC");
        assert_eq!(second.as_number(), Some(REPLACEMENT_VALUE));
        assert_ne!(
            jit.name_ics[LOAD_PC]
                .get()
                .expect("layout miss republishes lexical address")
                .layout,
            first_layout
        );
    }

    #[test]
    fn immutable_templates_and_kernels_are_shared() {
        fn accepts_exit_morphism<M: CategoryMorphism<Connector, ReturnState>>(_: &M) {}
        fn accepts_reentry_morphism<M: CategoryMorphism<Connector, Connector>>(_: &M) {}
        assert!(Rc::ptr_eq(&block_template(), &block_template()));
        assert!(Rc::ptr_eq(&prologue_template(), &prologue_template()));
        assert!(Rc::ptr_eq(
            &exit_adapter_template(),
            &exit_adapter_template()
        ));
        assert!(Rc::ptr_eq(
            &effect_reentry_adapter_template(),
            &effect_reentry_adapter_template()
        ));
        assert!(Rc::ptr_eq(&exit_kernel(), &exit_kernel()));
        assert_eq!(exit_kernel()._memory.ptr, exit_kernel()._memory.ptr);
        assert!(Rc::ptr_eq(
            &effect_reentry_kernel(),
            &effect_reentry_kernel()
        ));
        assert_eq!(
            effect_reentry_kernel()._memory.ptr,
            effect_reentry_kernel()._memory.ptr
        );
        accepts_exit_morphism(exit_kernel().as_ref());
        accepts_reentry_morphism(effect_reentry_kernel().as_ref());
    }

    fn one_step_code(ops: Vec<DynOp>) -> DynCode {
        let end = ops.len();
        DynCode {
            ops: ops
                .into_iter()
                .map(|op| super::super::dynbytecode::DynInstr {
                    op,
                    span: Span::default(),
                })
                .collect(),
            registers: TEST_CONDITION_REGISTER_COUNT,
            params: Vec::new(),
            hoisted: Vec::new(),
            source_id: None,
            blocks: (TEST_CONDITION_START_PC..end)
                .map(|pc| (pc, pc + NEXT_INSTRUCTION_DISTANCE, false))
                .collect(),
            bindings: Vec::new(),
            is_script: false,
        }
    }

    fn call_one_step_code(code: DynCode, vm: &mut Vm) -> JsResult<Value> {
        let mut arena = CodeArena::new();
        let jit = DynJitCode::build(code, &mut arena, false).expect("build one-step stencil graph");
        let outer = vm.global.clone();
        jit.call(vm, outer, Value::Undefined, &[] as &[Value])
    }

    #[test]
    fn guest_result_raw_word_transfers_and_drops_exactly_one_owner() {
        const EXTERNAL_OWNER_COUNT: usize = 1;
        const EXTERNAL_AND_FRAME_OWNER_COUNT: usize = 2;
        const RESULT_TEXT: &str = "owned guest result";

        let mut arena = CodeArena::new();
        let jit = DynJitCode::build(
            one_step_code(vec![DynOp::Return { src: None }]),
            &mut arena,
            false,
        )
        .expect("build result ownership test image");
        let mut vm = Vm::new();
        let environment = vm.global.clone();
        let layout = CallFrameLayout::new(0, jit.code.registers, jit.name_snapshot_slot_count());
        let owned_values = vm.acquire_registers(layout.slot_count);
        let mut frame = jit.make_frame(
            &mut vm,
            environment,
            std::ptr::null_mut(),
            0,
            owned_values,
            layout.register_base,
            layout.snapshot_base,
            layout.snapshot_count,
        );

        let owner = Rc::new(RESULT_TEXT.to_owned());
        frame.guest.replace_result(Value::String(Rc::clone(&owner)));
        assert_eq!(Rc::strong_count(&owner), EXTERNAL_AND_FRAME_OWNER_COUNT);
        frame.guest.replace_result(Value::Undefined);
        assert_eq!(Rc::strong_count(&owner), EXTERNAL_OWNER_COUNT);
        frame.guest.replace_result(Value::String(Rc::clone(&owner)));
        drop(frame);
        assert_eq!(Rc::strong_count(&owner), EXTERNAL_OWNER_COUNT);
    }

    #[test]
    fn effect_reentry_selection_is_structural() {
        let allocation = one_step_code(vec![DynOp::NewObject {
            dst: TEST_RESULT_REGISTER,
        }]);
        assert!(effect_reentry_block(
            &allocation,
            TEST_CONDITION_START_PC,
            TEST_CONDITION_START_PC + NEXT_INSTRUCTION_DISTANCE,
            true
        ));
        assert!(!effect_reentry_block(
            &allocation,
            TEST_CONDITION_START_PC,
            TEST_CONDITION_START_PC + NEXT_INSTRUCTION_DISTANCE,
            false
        ));

        let direct_return = one_step_code(vec![DynOp::Return { src: None }]);
        assert!(!effect_reentry_block(
            &direct_return,
            TEST_CONDITION_START_PC,
            TEST_CONDITION_START_PC + NEXT_INSTRUCTION_DISTANCE,
            true
        ));
    }

    #[test]
    fn fresh_object_allocation_preinstalls_only_unescaped_static_shape() {
        const FIRST_KEY: &str = "left";
        const SECOND_KEY: &str = "right";
        let mut folded = one_step_code(vec![
            DynOp::NewObject {
                dst: TEST_RESULT_REGISTER,
            },
            DynOp::LoadLiteral {
                dst: TEST_LITERAL_REGISTER,
                value: Literal::Number(1.0),
            },
            DynOp::SetStatic {
                object: TEST_RESULT_REGISTER,
                key: FIRST_KEY.to_owned(),
                src: TEST_LITERAL_REGISTER,
            },
            DynOp::SetStatic {
                object: TEST_RESULT_REGISTER,
                key: SECOND_KEY.to_owned(),
                src: TEST_LITERAL_REGISTER,
            },
            DynOp::Return {
                src: Some(TEST_RESULT_REGISTER),
            },
        ]);
        folded.blocks = vec![(0, folded.ops.len(), false)];
        let shapes = allocation_site_shapes(&folded, &RegionPlan::quote(&folded).unwrap());
        let shape = shapes[0].expect("fresh unescaped object has an allocation shape");
        assert_eq!(shape.slot(FIRST_KEY), Some(0));
        assert_eq!(shape.slot(SECOND_KEY), Some(1));

        let mut escaped = one_step_code(vec![
            DynOp::NewObject {
                dst: TEST_RESULT_REGISTER,
            },
            DynOp::StoreLocal {
                slot: TEST_LOCAL_SLOT,
                src: TEST_RESULT_REGISTER,
            },
            DynOp::SetStatic {
                object: TEST_RESULT_REGISTER,
                key: FIRST_KEY.to_owned(),
                src: TEST_LITERAL_REGISTER,
            },
            DynOp::Return {
                src: Some(TEST_RESULT_REGISTER),
            },
        ]);
        escaped.blocks = vec![(0, escaped.ops.len(), false)];
        assert!(
            allocation_site_shapes(&escaped, &RegionPlan::quote(&escaped).unwrap())[0].is_none()
        );

        let mut self_referential = one_step_code(vec![
            DynOp::NewObject {
                dst: TEST_RESULT_REGISTER,
            },
            DynOp::SetStatic {
                object: TEST_RESULT_REGISTER,
                key: FIRST_KEY.to_owned(),
                src: TEST_RESULT_REGISTER,
            },
            DynOp::Return {
                src: Some(TEST_RESULT_REGISTER),
            },
        ]);
        self_referential.blocks = vec![(0, self_referential.ops.len(), false)];
        assert!(
            allocation_site_shapes(
                &self_referential,
                &RegionPlan::quote(&self_referential).unwrap()
            )[0]
            .is_none()
        );
    }

    #[test]
    fn effect_kernel_reenters_after_allocation_and_call() {
        const CALLBACK_NAME: &str = "task131Callback";
        const CALLBACK_RESULT: f64 = 37.0;
        fn callback(_: &mut Vm, _: Value, _: &[Value]) -> JsResult<Value> {
            Ok(Value::Number(CALLBACK_RESULT))
        }

        let mut vm = Vm::new();
        let allocated = call_one_step_code(
            one_step_code(vec![
                DynOp::NewObject {
                    dst: TEST_RESULT_REGISTER,
                },
                DynOp::Return {
                    src: Some(TEST_RESULT_REGISTER),
                },
            ]),
            &mut vm,
        )
        .expect("allocation effect rejoins return stencil");
        assert!(allocated.as_object_ref().is_some());

        let callback = vm.native(callback);
        Environment::set(&vm.global, CALLBACK_NAME, callback);
        let called = call_one_step_code(
            one_step_code(vec![
                DynOp::LoadName {
                    dst: TEST_LOAD_REGISTER,
                    name: CALLBACK_NAME.to_owned(),
                },
                DynOp::LoadLiteral {
                    dst: TEST_LITERAL_REGISTER,
                    value: Literal::Undefined,
                },
                DynOp::Call {
                    dst: TEST_RESULT_REGISTER,
                    callee: TEST_LOAD_REGISTER,
                    receiver: TEST_LITERAL_REGISTER,
                    args: Vec::new(),
                },
                DynOp::Return {
                    src: Some(TEST_RESULT_REGISTER),
                },
            ]),
            &mut vm,
        )
        .expect("call effect rejoins return stencil");
        assert_eq!(called.as_number(), Some(CALLBACK_RESULT));
    }

    #[test]
    fn effect_kernel_preserves_exception_exit() {
        const THROWN_MESSAGE: &str = "task131 throw";
        let mut vm = Vm::new();
        let error = call_one_step_code(
            one_step_code(vec![
                DynOp::LoadLiteral {
                    dst: TEST_RESULT_REGISTER,
                    value: Literal::String(THROWN_MESSAGE.to_owned()),
                },
                DynOp::Throw {
                    src: TEST_RESULT_REGISTER,
                },
            ]),
            &mut vm,
        )
        .expect_err("uncaught one-step throw exits through the function kernel");
        assert!(error.to_string().contains(THROWN_MESSAGE));
    }

    #[test]
    fn regexp_literals_share_only_the_immutable_compiled_kernel() {
        const TEST_PATTERN: &str = "^[a-z]+$";
        let code = one_step_code(vec![
            DynOp::RegExp {
                dst: TEST_RESULT_REGISTER,
                global: true,
                kernel: RegExpLiteralKernel::compile(TEST_PATTERN, false),
            },
            DynOp::Return {
                src: Some(TEST_RESULT_REGISTER),
            },
        ]);
        let mut arena = CodeArena::new();
        let jit = DynJitCode::build(code, &mut arena, false).expect("build regexp stencil");
        let mut vm = Vm::new();
        let outer = vm.global.clone();
        let first = jit
            .call(&mut vm, outer.clone(), Value::Undefined, &[] as &[Value])
            .expect("first regexp literal evaluation");
        let second = jit
            .call(&mut vm, outer, Value::Undefined, &[] as &[Value])
            .expect("second regexp literal evaluation");
        let first_instance = first.as_regexp().expect("first regexp instance");
        let second_instance = second.as_regexp().expect("second regexp instance");
        assert!(!Rc::ptr_eq(&first_instance, &second_instance));
        assert!(Rc::ptr_eq(
            &first_instance.borrow().regex,
            &second_instance.borrow().regex
        ));
    }

    #[test]
    fn rustc_extracts_tail_composable_number_stencil() {
        assert!(!rustc_stencils::NUMBER_ADD_BYTES.is_empty());
        let number_add = rustc_stencils::STENCILS
            .iter()
            .find(|stencil| stencil.name == "quench_number_add")
            .expect("number-add stencil exists");
        assert!(number_add.next_relocation() < rustc_stencils::NUMBER_ADD_BYTES.len());
        assert!(rustc_stencils::STENCILS.len() >= 5);
        assert!(rustc_stencils::STENCILS.iter().all(|stencil| {
            !stencil.bytes.is_empty() && stencil.next_relocation() < stencil.bytes.len()
        }));
        assert!(
            rustc_stencils::STENCILS
                .iter()
                .filter(|stencil| {
                    stencil.name.starts_with("quench_dyn_")
                        && !matches!(
                            stencil.name,
                            "quench_dyn_jump"
                                | "quench_dyn_constant_truthy"
                                | "quench_dyn_constant_falsey"
                        )
                })
                .all(|stencil| stencil.slow_relocation().is_some())
        );
        let conditional = rustc_stencils::STENCILS
            .iter()
            .find(|stencil| stencil.name == "quench_dyn_jump_if_false")
            .expect("conditional branch stencil exists");
        assert!(conditional.branch_relocation().is_some());
    }

    #[test]
    fn rustc_extracts_complete_numeric_region_vocabulary() {
        const CONDITION_SOURCE_FAMILIES: usize = 3;
        const NUMERIC_COMPARISON_VARIANTS: usize = 6;
        const EXPECTED_CONDITION_STENCILS: usize =
            CONDITION_SOURCE_FAMILIES * NUMERIC_COMPARISON_VARIANTS;
        const REGION_STENCILS: &[&str] = &[
            "quench_region_load_literal",
            "quench_region_burned_load_literal_one_lane",
            "quench_region_burned_load_literal_two_lanes",
            "quench_region_burned_load_literal_three_lanes",
            "quench_region_burned_load_literal_four_lanes",
            "quench_region_load_local",
            "quench_region_burned_load_local",
            "quench_region_load_name",
            "quench_region_store_local",
            "quench_region_burned_store_local",
            "quench_region_move",
            "quench_region_burned_move",
            "quench_region_add",
            "quench_region_subtract",
            "quench_region_multiply",
            "quench_region_divide",
            "quench_region_equal",
            "quench_region_not_equal",
            "quench_region_less",
            "quench_region_less_equal",
            "quench_region_greater",
            "quench_region_greater_equal",
            "quench_region_bit_or",
            "quench_region_bit_xor",
            "quench_region_bit_and",
            "quench_region_shift_left",
            "quench_region_shift_right",
            "quench_region_shift_right_unsigned",
            "quench_region_unary_plus",
            "quench_region_burned_unary_plus",
            "quench_region_negate",
            "quench_region_burned_negate",
            "quench_region_bit_not",
            "quench_region_burned_bit_not",
            "quench_region_read_dense",
            "quench_region_read_dense_proven_index",
            "quench_region_write_dense",
            "quench_region_write_dense_proven_index",
            "quench_region_read_static",
            "quench_region_write_static",
            "quench_region_dead_update_local_number_add_jump",
            "quench_region_dead_update_local_number_subtract_jump",
            "quench_region_counted_dead_update_local_number_add_jump",
            "quench_region_counted_dead_update_local_number_subtract_jump",
            "quench_region_dead_local_number_add",
            "quench_region_dead_local_number_subtract",
        ];
        for name in REGION_STENCILS {
            let stencil = rustc_stencils::STENCILS
                .iter()
                .find(|stencil| stencil.name == *name)
                .unwrap_or_else(|| panic!("missing region stencil {name}"));
            assert!(stencil.next_relocation() < stencil.bytes.len());
            let is_dense = matches!(
                *name,
                "quench_region_read_dense"
                    | "quench_region_read_dense_proven_index"
                    | "quench_region_write_dense"
                    | "quench_region_write_dense_proven_index"
            );
            let needs_slow_path = is_dense || *name == "quench_region_load_name";
            assert_eq!(stencil.slow_relocation().is_some(), needs_slow_path);
            assert!(stencil.branch_relocation().is_none());
            assert!(
                stencil
                    .bytes
                    .chunks_exact(a64_abi::INSTRUCTION_BYTES)
                    .all(|bytes| {
                        let instruction = u32::from_le_bytes(bytes.try_into().unwrap());
                        instruction & AARCH64_BRANCH_LINK_MASK != AARCH64_BRANCH_LINK
                            && instruction & AARCH64_BRANCH_LINK_REGISTER_MASK
                                != AARCH64_BRANCH_LINK_REGISTER
                    })
            );
        }
        let condition_stencils = rustc_stencils::STENCILS
            .iter()
            .filter(|stencil| stencil.name.starts_with("quench_region_dead_condition_"))
            .collect::<Vec<_>>();
        assert_eq!(condition_stencils.len(), EXPECTED_CONDITION_STENCILS);
        assert!(condition_stencils.iter().all(|stencil| {
            stencil.next_relocation() < stencil.bytes.len()
                && stencil.branch_relocation().is_some()
                && stencil.slow_relocation().is_none()
        }));
    }

    #[test]
    fn cooker_extracts_and_linker_patches_site_advance_holes() {
        const ELIDED_SITE_COUNT: usize = 2;
        const EXPECTED_SITE_ADVANCE: usize = ELIDED_SITE_COUNT + NEXT_INSTRUCTION_DISTANCE;

        let stencil = rustc_stencils::STENCILS
            .iter()
            .find(|stencil| stencil.name == "quench_region_nop")
            .expect("region nop stencil exists");
        let site_advance_relocations = stencil.site_advance_relocations();
        let [instruction_offset] = site_advance_relocations.as_slice() else {
            panic!("region nop has exactly one site-advance relocation")
        };
        let byte_advance = EXPECTED_SITE_ADVANCE * std::mem::size_of::<InlineSite>();
        let patch = site_advance_patch(stencil.bytes, *instruction_offset, byte_advance)
            .expect("three-site advance is encodable");
        assert_eq!(patch.offset, *instruction_offset);
        let mut code = stencil.bytes.to_vec();
        patch.apply(&mut code);
        let value = u32::from_le_bytes(
            code[patch.offset..patch.offset + a64_abi::INSTRUCTION_BYTES]
                .try_into()
                .unwrap(),
        );
        let encoded_advance =
            (value >> AARCH64_ADD_IMMEDIATE_FIELD_SHIFT) & AARCH64_ADD_IMMEDIATE_FIELD_MASK;
        assert_eq!(encoded_advance as usize, byte_advance);
    }

    #[test]
    fn typed_patchers_reject_unencodable_or_misaligned_values() {
        let site_stencil = rustc_stencils::STENCILS
            .iter()
            .find(|stencil| stencil.name == "quench_region_nop")
            .expect("region nop stencil exists");
        let site_advance_relocations = site_stencil.site_advance_relocations();
        let [site_offset] = site_advance_relocations.as_slice() else {
            panic!("region nop has exactly one site-advance relocation")
        };
        assert!(
            site_advance_patch(
                site_stencil.bytes,
                *site_offset,
                site_holes::AARCH64_ADD_IMMEDIATE_MAX,
            )
            .is_some()
        );
        assert!(
            site_advance_patch(
                site_stencil.bytes,
                *site_offset,
                site_holes::AARCH64_ADD_IMMEDIATE_MAX + 1,
            )
            .is_none()
        );

        let operand_stencil = rustc_stencils::STENCILS
            .iter()
            .find(|stencil| stencil.name == "quench_region_burned_add")
            .expect("burned add stencil exists");
        let operand_relocations = operand_stencil.operand_relocations();
        let (operand_offset, _) = operand_relocations
            .first()
            .expect("burned add has an operand relocation");
        let maximum_operand_byte_offset =
            operand_holes::AARCH64_UNSIGNED_OFFSET_MAX_SCALED * operand_holes::VALUE_BYTE_WIDTH;
        assert!(
            burned_operand_patch(
                operand_stencil.bytes,
                *operand_offset,
                maximum_operand_byte_offset,
            )
            .is_some()
        );
        assert!(
            burned_operand_patch(
                operand_stencil.bytes,
                *operand_offset,
                maximum_operand_byte_offset + operand_holes::VALUE_BYTE_WIDTH,
            )
            .is_none()
        );
        assert!(
            burned_operand_patch(
                operand_stencil.bytes,
                *operand_offset,
                maximum_operand_byte_offset - 1,
            )
            .is_none()
        );
    }

    #[test]
    fn region_leaf_advance_folds_a_run_of_elided_forms() {
        const SOURCE_REGISTER: Register = 0;
        const DESTINATION_REGISTER: Register = 1;
        const ELIDED_REGISTER: Register = 2;
        let operations = [
            numeric_region::RegionOp::ReadLocal {
                pc: 0,
                dst: SOURCE_REGISTER,
                slot: 0,
            },
            numeric_region::RegionOp::Elided { pc: 1 },
            numeric_region::RegionOp::Elided { pc: 2 },
            numeric_region::RegionOp::Binary {
                pc: 3,
                dst: DESTINATION_REGISTER,
                left: SOURCE_REGISTER,
                right: ELIDED_REGISTER,
                kind: numeric_region::NumericBinary::Multiply,
            },
        ];
        assert_eq!(numeric_region_leaf_advance(&operations, 0), (3, 3));
    }

    #[test]
    fn cooker_extracts_and_linker_burns_typed_register_operands() {
        const LEFT_REGISTER: Register = 0;
        const RIGHT_REGISTER: Register = 1;
        const DESTINATION_REGISTER: Register = 2;
        const EXPECTED_OPERAND_KINDS: usize = 3;
        const LEFT_VALUE: f64 = 1.25;
        const RIGHT_VALUE: f64 = 2.75;
        const EXPECTED_VALUE: f64 = 4.0;

        let stencil = rustc_stencils::STENCILS
            .iter()
            .find(|stencil| stencil.name == "quench_region_burned_add")
            .expect("burned add stencil exists");
        assert_eq!(stencil.operand_relocations().len(), EXPECTED_OPERAND_KINDS);
        let byte_offset = |kind| match kind {
            operand_holes::OperandHoleKind::DestinationRegisterByteOffset => {
                usize::from(DESTINATION_REGISTER) * operand_holes::VALUE_BYTE_WIDTH
            }
            operand_holes::OperandHoleKind::SourceRegisterByteOffset
            | operand_holes::OperandHoleKind::DestinationLocalByteOffset
            | operand_holes::OperandHoleKind::SourceLocalByteOffset => {
                unreachable!("burned add uses only destination, left, and right registers")
            }
            operand_holes::OperandHoleKind::LeftRegisterByteOffset => {
                usize::from(LEFT_REGISTER) * operand_holes::VALUE_BYTE_WIDTH
            }
            operand_holes::OperandHoleKind::RightRegisterByteOffset => {
                usize::from(RIGHT_REGISTER) * operand_holes::VALUE_BYTE_WIDTH
            }
        };
        let mut code = stencil.bytes.to_vec();
        for (offset, kind) in stencil.operand_relocations() {
            let patch = burned_operand_patch(stencil.bytes, offset, byte_offset(kind))
                .expect("typed operand is encodable");
            patch.apply(&mut code);
        }
        let exit_offset = code.len();
        a64_word(&mut code, a64_abi::RETURN);
        patch_tail_branch(&mut code, stencil.next_relocation(), exit_offset)
            .expect("patch burned stencil continuation");
        let mut registers = vec![
            Value::Number(LEFT_VALUE),
            Value::Number(RIGHT_VALUE),
            Value::Undefined,
        ];
        let site = InlineSite::unused(FIRST_INSTRUCTION_PC);
        execute_region_test_code(
            &code,
            &mut registers,
            &mut [],
            std::slice::from_ref(&site),
            FIRST_INSTRUCTION_PC,
            &[],
            &[],
        );
        assert_eq!(
            registers[usize::from(DESTINATION_REGISTER)].as_number(),
            Some(EXPECTED_VALUE)
        );
    }

    #[test]
    fn cooker_extracts_and_linker_burns_raw_literal_value() {
        const DESTINATION_REGISTER: Register = 1;
        const EXPECTED_MOV_WIDE_SITES: usize = 4;
        const EXPECTED_VALUE: f64 = std::f64::consts::PI;

        let stencil = rustc_stencils::STENCILS
            .iter()
            .find(|stencil| stencil.name == "quench_region_burned_load_literal_four_lanes")
            .expect("burned literal stencil exists");
        let operand_bindings = [OperandBinding {
            kind: operand_holes::OperandHoleKind::DestinationRegisterByteOffset,
            byte_offset: register_operand_byte_offset(DESTINATION_REGISTER).unwrap(),
        }];
        let raw_value_bindings = [RawValueBinding {
            id: raw_value_holes::RAW_VALUE_HOLE_ID,
            bits: raw_value::RawValue::number(EXPECTED_VALUE).bits(),
        }];
        validate_stencil_bindings(
            stencil.name,
            stencil,
            &operand_bindings,
            &raw_value_bindings,
        );
        assert_eq!(
            stencil
                .patch_sites
                .iter()
                .filter(|site| site.encoding == patch_schema::PatchEncoding::MovWide16)
                .count(),
            EXPECTED_MOV_WIDE_SITES
        );
        let mut code = stencil.bytes.to_vec();
        for patch in stencil_copy_patches(
            stencil.name,
            stencil,
            &operand_bindings,
            &raw_value_bindings,
            std::mem::size_of::<InlineSite>(),
        ) {
            patch.apply(&mut code);
        }
        let exit_offset = code.len();
        a64_word(&mut code, a64_abi::RETURN);
        patch_tail_branch(&mut code, stencil.next_relocation(), exit_offset)
            .expect("patch burned literal continuation");
        let mut registers = vec![Value::Undefined, Value::Undefined];
        let site = InlineSite::unused(FIRST_INSTRUCTION_PC);
        execute_region_test_code(
            &code,
            &mut registers,
            &mut [],
            std::slice::from_ref(&site),
            FIRST_INSTRUCTION_PC,
            &[],
            &[],
        );
        assert_eq!(
            registers[usize::from(DESTINATION_REGISTER)].as_number(),
            Some(EXPECTED_VALUE)
        );
    }

    #[test]
    fn observation_stencil_is_patched_composable_and_effect_free_when_disabled() {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        const OBSERVED_PC: usize = 7;

        let payload = rustc_dyn_stencil("quench_region_nop", None, None);
        let plain = observed_labeled_leaf(OBSERVED_PC, false, &COUNTER, payload.clone());
        let expected = payload.labeled(LabelId(OBSERVED_PC as u32));
        assert_eq!(plain.code(), expected.code());

        let exit = Stencil::<Connector, Connector>::leaf(LeafStencil {
            level: StencilLevel::Opcode,
            bytes: a64_abi::RETURN.to_le_bytes().to_vec(),
            holes: Vec::new(),
            labels: Vec::new(),
            fragments: Vec::new(),
        });
        let chain = observation_leaf(&COUNTER) + exit;
        let code = link_rustc_chain(chain.image()).expect("observation tail-composes with exit");
        let before = COUNTER.load(Ordering::Relaxed);
        let mut registers = Vec::new();
        let mut locals = Vec::new();
        let site = InlineSite::unused(FIRST_INSTRUCTION_PC);
        execute_region_test_code(
            &code,
            &mut registers,
            &mut locals,
            std::slice::from_ref(&site),
            FIRST_INSTRUCTION_PC,
            &[],
            &[],
        );
        assert_eq!(COUNTER.load(Ordering::Relaxed), before + 1);
    }

    #[test]
    fn cooker_burns_local_copy_move_and_unary_operands() {
        const SOURCE_LOCAL: usize = 1;
        const DESTINATION_LOCAL: usize = 0;
        const LOAD_DESTINATION: Register = 2;
        const UNARY_DESTINATION: Register = 3;
        const MOVE_DESTINATION: Register = 1;
        const SOURCE_VALUE: f64 = 9.5;
        const EXPECTED_VALUE: f64 = -9.5;

        fn execute_burned(
            name: &str,
            bindings: &[(operand_holes::OperandHoleKind, usize)],
            registers: &mut [Value],
            locals: &mut [Value],
        ) {
            let stencil = rustc_stencils::STENCILS
                .iter()
                .find(|stencil| stencil.name == name)
                .unwrap_or_else(|| panic!("missing burned stencil {name}"));
            assert_eq!(stencil.operand_relocations().len(), bindings.len());
            let mut code = stencil.bytes.to_vec();
            for (offset, kind) in stencil.operand_relocations() {
                let byte_offset = bindings
                    .iter()
                    .find_map(|(binding_kind, byte_offset)| {
                        (*binding_kind == kind).then_some(*byte_offset)
                    })
                    .unwrap_or_else(|| panic!("missing {kind:?} test binding for {name}"));
                let patch = burned_operand_patch(stencil.bytes, offset, byte_offset)
                    .expect("burned copy operand is encodable");
                patch.apply(&mut code);
            }
            let exit_offset = code.len();
            a64_word(&mut code, a64_abi::RETURN);
            patch_tail_branch(&mut code, stencil.next_relocation(), exit_offset)
                .expect("patch burned copy continuation");
            let site = InlineSite::unused(FIRST_INSTRUCTION_PC);
            execute_region_test_code(
                &code,
                registers,
                locals,
                std::slice::from_ref(&site),
                FIRST_INSTRUCTION_PC,
                &[],
                &[],
            );
        }

        let register_offset =
            |register: Register| usize::from(register) * operand_holes::VALUE_BYTE_WIDTH;
        let local_offset = |slot: usize| slot * operand_holes::VALUE_BYTE_WIDTH;
        let mut registers = vec![Value::Undefined; usize::from(UNARY_DESTINATION) + 1];
        let mut locals = vec![Value::Undefined, Value::Number(SOURCE_VALUE)];

        execute_burned(
            "quench_region_burned_load_local",
            &[
                (
                    operand_holes::OperandHoleKind::DestinationRegisterByteOffset,
                    register_offset(LOAD_DESTINATION),
                ),
                (
                    operand_holes::OperandHoleKind::SourceLocalByteOffset,
                    local_offset(SOURCE_LOCAL),
                ),
            ],
            &mut registers,
            &mut locals,
        );
        execute_burned(
            "quench_region_burned_negate",
            &[
                (
                    operand_holes::OperandHoleKind::DestinationRegisterByteOffset,
                    register_offset(UNARY_DESTINATION),
                ),
                (
                    operand_holes::OperandHoleKind::SourceRegisterByteOffset,
                    register_offset(LOAD_DESTINATION),
                ),
            ],
            &mut registers,
            &mut locals,
        );
        execute_burned(
            "quench_region_burned_move",
            &[
                (
                    operand_holes::OperandHoleKind::DestinationRegisterByteOffset,
                    register_offset(MOVE_DESTINATION),
                ),
                (
                    operand_holes::OperandHoleKind::SourceRegisterByteOffset,
                    register_offset(UNARY_DESTINATION),
                ),
            ],
            &mut registers,
            &mut locals,
        );
        execute_burned(
            "quench_region_burned_store_local",
            &[
                (
                    operand_holes::OperandHoleKind::DestinationLocalByteOffset,
                    local_offset(DESTINATION_LOCAL),
                ),
                (
                    operand_holes::OperandHoleKind::SourceRegisterByteOffset,
                    register_offset(MOVE_DESTINATION),
                ),
            ],
            &mut registers,
            &mut locals,
        );

        assert_eq!(locals[DESTINATION_LOCAL].as_number(), Some(EXPECTED_VALUE));
    }

    #[test]
    fn cooked_region_guard_calls_frame_connector() {
        let mut registers = vec![Value::Undefined];
        let site = InlineSite::unused(0);
        run_region_stencil(
            "quench_region_guard",
            &mut registers,
            &mut [],
            &site,
            &[],
            &[],
        );
    }

    #[test]
    fn cooked_region_arithmetic_matches_javascript_numbers() {
        const LEFT_REGISTER: usize = 0;
        const RIGHT_REGISTER: usize = 1;
        const RESULT_REGISTER: usize = 2;
        let cases: [(&str, f64, f64, f64); 4] = [
            ("quench_region_add", 1.25, 2.75, 4.0),
            ("quench_region_divide", -0.0, 1.0, -0.0),
            ("quench_region_bit_or", 4_294_967_297.0, 2.0, 3.0),
            (
                "quench_region_shift_right_unsigned",
                -1.0,
                1.0,
                2_147_483_647.0,
            ),
        ];
        for (name, left, right, expected) in cases {
            let mut registers = vec![Value::Number(left), Value::Number(right), Value::Undefined];
            let site = InlineSite {
                pc: 0,
                opcode: 0,
                run_end: 1,
                dst: RESULT_REGISTER,
                left: LEFT_REGISTER,
                right: RIGHT_REGISTER,
                literal: 0,
            };
            run_region_stencil(name, &mut registers, &mut [], &site, &[], &[]);
            assert_eq!(
                registers[RESULT_REGISTER].as_number().unwrap().to_bits(),
                expected.to_bits(),
                "{name}"
            );
        }
    }

    #[test]
    fn cooked_region_update_supernodes_materialize_only_the_loop_local() {
        const LOCAL_SLOT: usize = 0;
        const LOAD_REGISTER: usize = 0;
        const LITERAL_REGISTER: usize = 1;
        const RESULT_REGISTER: usize = 2;
        const LOOP_HEADER_PC: usize = 0;
        const UPDATE_PC: usize = 3;
        const INITIAL_VALUE: f64 = 7.0;
        const UPDATE_VALUE: f64 = 2.0;

        let mut sites = (0..UPDATE_PC + UPDATE_INSTRUCTION_COUNT)
            .map(InlineSite::unused)
            .collect::<Vec<_>>();
        sites[UPDATE_PC].dst = LOAD_REGISTER;
        sites[UPDATE_PC].left = LOCAL_SLOT;
        sites[UPDATE_PC + UPDATE_LITERAL_PC_OFFSET].dst = LITERAL_REGISTER;
        sites[UPDATE_PC + UPDATE_LITERAL_PC_OFFSET].literal = UPDATE_VALUE.to_bits();
        sites[UPDATE_PC + UPDATE_BINARY_PC_OFFSET].dst = RESULT_REGISTER;
        sites[UPDATE_PC + UPDATE_BINARY_PC_OFFSET].left = LOAD_REGISTER;
        sites[UPDATE_PC + UPDATE_BINARY_PC_OFFSET].right = LITERAL_REGISTER;
        sites[UPDATE_PC + UPDATE_STORE_PC_OFFSET].dst = LOCAL_SLOT;
        sites[UPDATE_PC + UPDATE_STORE_PC_OFFSET].left = RESULT_REGISTER;
        sites[UPDATE_PC + UPDATE_JUMP_PC_OFFSET].literal = LOOP_HEADER_PC as u64;

        for (name, expected, counted) in [
            (
                "quench_region_dead_local_number_add",
                INITIAL_VALUE + UPDATE_VALUE,
                false,
            ),
            (
                "quench_region_dead_local_number_subtract",
                INITIAL_VALUE - UPDATE_VALUE,
                false,
            ),
            (
                "quench_region_dead_update_local_number_add_jump",
                INITIAL_VALUE + UPDATE_VALUE,
                false,
            ),
            (
                "quench_region_counted_dead_update_local_number_subtract_jump",
                INITIAL_VALUE - UPDATE_VALUE,
                true,
            ),
        ] {
            let mut registers = vec![Value::Undefined; RESULT_REGISTER + 1];
            let mut locals = vec![Value::Number(INITIAL_VALUE)];
            let iterations = run_region_stencil(
                name,
                &mut registers,
                &mut locals,
                &sites[UPDATE_PC],
                &[],
                &[],
            );
            assert_eq!(locals[LOCAL_SLOT].as_number(), Some(expected));
            assert!(registers.iter().all(Value::is_undefined));
            assert_eq!(iterations, u64::from(counted));
        }
    }

    #[test]
    fn numeric_region_update_supernode_selection_requires_a_closed_recurrence() {
        const OBJECT_LOCAL: usize = 0;
        const INDEX_LOCAL: usize = 1;
        const DIFFERENT_LOCAL: usize = 2;
        const OBJECT_REGISTER: Register = 0;
        const INDEX_REGISTER: Register = 1;
        const ELEMENT_REGISTER: Register = 2;
        const RELOAD_REGISTER: Register = 3;
        const UPDATE_REGISTER: Register = 4;
        const RESULT_REGISTER: Register = 5;
        const UPDATE_START_PC: usize = 3;
        const LOOP_END_PC: usize = 8;
        const PRE_UPDATE_ELEMENT_ACCESS_PC: usize = 2;
        const NONCONSECUTIVE_PC: usize = 99;
        const UPDATE_VALUE: f64 = 1.0;

        let instruction = |op| super::super::dynbytecode::DynInstr {
            op,
            span: Span::default(),
        };
        let mut code = DynCode {
            ops: vec![
                DynOp::LoadLocal {
                    dst: OBJECT_REGISTER,
                    slot: OBJECT_LOCAL,
                },
                DynOp::LoadLocal {
                    dst: INDEX_REGISTER,
                    slot: INDEX_LOCAL,
                },
                DynOp::GetComputed {
                    dst: ELEMENT_REGISTER,
                    object: OBJECT_REGISTER,
                    key: INDEX_REGISTER,
                },
                DynOp::LoadLocal {
                    dst: RELOAD_REGISTER,
                    slot: INDEX_LOCAL,
                },
                DynOp::LoadLiteral {
                    dst: UPDATE_REGISTER,
                    value: Literal::Number(UPDATE_VALUE),
                },
                DynOp::Binary {
                    dst: RESULT_REGISTER,
                    left: RELOAD_REGISTER,
                    right: UPDATE_REGISTER,
                    kind: Op::Add,
                },
                DynOp::StoreLocal {
                    slot: INDEX_LOCAL,
                    src: RESULT_REGISTER,
                },
                DynOp::Jump {
                    target: FIRST_INSTRUCTION_PC,
                },
                DynOp::Return { src: None },
            ]
            .into_iter()
            .map(instruction)
            .collect(),
            registers: usize::from(RESULT_REGISTER) + 1,
            params: Vec::new(),
            hoisted: Vec::new(),
            source_id: None,
            blocks: Vec::new(),
            bindings: Vec::new(),
            is_script: false,
        };
        let analysis = numeric_region::analyze(&code);
        assert!(analysis.rejected.is_empty(), "{:?}", analysis.rejected);
        let quote = &analysis.loops[0];
        let guard = numeric_region::GuardPlan::from_loop(quote);
        let operations = quote.operations();
        assert!(
            select_numeric_region_update_supernode(
                &code,
                &operations,
                UPDATE_START_PC,
                quote.start,
                quote.end,
                &guard,
                false,
            )
            .is_some()
        );
        assert!(
            select_numeric_region_local_update_supernode(
                &code,
                &operations,
                UPDATE_START_PC,
                &guard,
            )
            .is_some()
        );

        let mut different_destination = operations.clone();
        different_destination[UPDATE_START_PC + UPDATE_STORE_PC_OFFSET] =
            numeric_region::RegionOp::WriteLocal {
                pc: UPDATE_START_PC + UPDATE_STORE_PC_OFFSET,
                slot: DIFFERENT_LOCAL,
                src: RESULT_REGISTER,
            };
        assert!(
            select_numeric_region_local_update_supernode(
                &code,
                &different_destination,
                UPDATE_START_PC,
                &guard,
            )
            .is_some()
        );

        let mut mismatch = operations.clone();
        mismatch[UPDATE_START_PC + UPDATE_STORE_PC_OFFSET] = numeric_region::RegionOp::WriteLocal {
            pc: UPDATE_START_PC + UPDATE_STORE_PC_OFFSET,
            slot: INDEX_LOCAL,
            src: UPDATE_REGISTER,
        };
        assert!(
            select_numeric_region_update_supernode(
                &code,
                &mismatch,
                UPDATE_START_PC,
                quote.start,
                quote.end,
                &guard,
                false,
            )
            .is_none()
        );
        assert!(
            select_numeric_region_local_update_supernode(
                &code,
                &mismatch,
                UPDATE_START_PC,
                &guard,
            )
            .is_none()
        );

        let mut nonconsecutive = operations.clone();
        nonconsecutive[UPDATE_START_PC + UPDATE_LITERAL_PC_OFFSET] =
            numeric_region::RegionOp::NumberLiteral {
                pc: NONCONSECUTIVE_PC,
                dst: UPDATE_REGISTER,
                bits: UPDATE_VALUE.to_bits(),
            };
        assert!(
            select_numeric_region_update_supernode(
                &code,
                &nonconsecutive,
                UPDATE_START_PC,
                quote.start,
                quote.end,
                &guard,
                false,
            )
            .is_none()
        );
        assert!(
            select_numeric_region_local_update_supernode(
                &code,
                &nonconsecutive,
                UPDATE_START_PC,
                &guard,
            )
            .is_none()
        );

        let mut interior_entry = operations.clone();
        interior_entry[PRE_UPDATE_ELEMENT_ACCESS_PC] = numeric_region::RegionOp::JumpIfFalse {
            pc: PRE_UPDATE_ELEMENT_ACCESS_PC,
            test: ELEMENT_REGISTER,
            target: UPDATE_START_PC + UPDATE_LITERAL_PC_OFFSET,
        };
        assert!(
            select_numeric_region_update_supernode(
                &code,
                &interior_entry,
                UPDATE_START_PC,
                quote.start,
                quote.end,
                &guard,
                false,
            )
            .is_none()
        );
        assert!(
            select_numeric_region_local_update_supernode(
                &code,
                &interior_entry,
                UPDATE_START_PC,
                &guard,
            )
            .is_none()
        );

        code.ops[PRE_UPDATE_ELEMENT_ACCESS_PC].op = DynOp::Move {
            dst: ELEMENT_REGISTER,
            src: RELOAD_REGISTER,
        };
        assert!(
            select_numeric_region_update_supernode(
                &code,
                &operations,
                UPDATE_START_PC,
                quote.start,
                quote.end,
                &guard,
                false,
            )
            .is_none()
        );
        assert!(
            select_numeric_region_local_update_supernode(
                &code,
                &operations,
                UPDATE_START_PC,
                &guard,
            )
            .is_none()
        );

        assert_eq!(quote.end, LOOP_END_PC);
    }

    #[test]
    fn numeric_region_condition_supernode_selection_covers_three_source_families() {
        const INDEX_LOCAL: usize = 0;
        const OBJECT_LOCAL: usize = 1;
        const BOUND_LOCAL: usize = 2;
        const INDEX_REGISTER: Register = 0;
        const BOUND_REGISTER: Register = 1;
        const CONDITION_REGISTER: Register = 2;
        const OBJECT_REGISTER: Register = 3;
        const BODY_INDEX_REGISTER: Register = 4;
        const ELEMENT_REGISTER: Register = 5;
        const COPY_REGISTER: Register = 6;
        const BODY_COPY_PC: usize = 7;
        const LOOP_BACKEDGE_PC: usize = 8;
        const LOOP_EXIT_PC: usize = 9;
        const BOUND_NAME: &str = "numericRegionBound";
        const LITERAL_BOUND: f64 = 10.0;

        fn condition_code(right: DynOp) -> DynCode {
            let instruction = |op| super::super::dynbytecode::DynInstr {
                op,
                span: Span::default(),
            };
            DynCode {
                ops: vec![
                    DynOp::LoadLocal {
                        dst: INDEX_REGISTER,
                        slot: INDEX_LOCAL,
                    },
                    right,
                    DynOp::Binary {
                        dst: CONDITION_REGISTER,
                        left: INDEX_REGISTER,
                        right: BOUND_REGISTER,
                        kind: Op::Lt,
                    },
                    DynOp::JumpIfFalse {
                        test: CONDITION_REGISTER,
                        target: LOOP_EXIT_PC,
                    },
                    DynOp::LoadLocal {
                        dst: OBJECT_REGISTER,
                        slot: OBJECT_LOCAL,
                    },
                    DynOp::LoadLocal {
                        dst: BODY_INDEX_REGISTER,
                        slot: INDEX_LOCAL,
                    },
                    DynOp::GetComputed {
                        dst: ELEMENT_REGISTER,
                        object: OBJECT_REGISTER,
                        key: BODY_INDEX_REGISTER,
                    },
                    DynOp::Move {
                        dst: COPY_REGISTER,
                        src: ELEMENT_REGISTER,
                    },
                    DynOp::Jump {
                        target: FIRST_INSTRUCTION_PC,
                    },
                    DynOp::Return { src: None },
                ]
                .into_iter()
                .map(instruction)
                .collect(),
                registers: usize::from(COPY_REGISTER) + 1,
                params: Vec::new(),
                hoisted: Vec::new(),
                source_id: None,
                blocks: Vec::new(),
                bindings: Vec::new(),
                is_script: false,
            }
        }

        let cases = [
            (
                DynOp::LoadLocal {
                    dst: BOUND_REGISTER,
                    slot: BOUND_LOCAL,
                },
                "quench_region_dead_condition_local_local_number_less",
            ),
            (
                DynOp::LoadLiteral {
                    dst: BOUND_REGISTER,
                    value: Literal::Number(LITERAL_BOUND),
                },
                "quench_region_dead_condition_local_literal_number_less",
            ),
            (
                DynOp::LoadName {
                    dst: BOUND_REGISTER,
                    name: BOUND_NAME.to_owned(),
                },
                "quench_region_dead_condition_local_name_number_less",
            ),
        ];
        for (right, expected) in cases {
            let code = condition_code(right);
            let analysis = numeric_region::analyze(&code);
            assert!(analysis.rejected.is_empty(), "{:?}", analysis.rejected);
            let quote = &analysis.loops[0];
            let guard = numeric_region::GuardPlan::from_loop(quote);
            let operations = quote.operations();
            let selected = select_numeric_region_condition_supernode(
                &code,
                &operations,
                FIRST_INSTRUCTION_PC,
                quote.start,
                quote.end,
                &guard,
            )
            .expect("closed numeric condition selects a coarse stencil");
            assert_eq!(selected.stencil_name, expected);

            let mut interior_entry = operations.clone();
            interior_entry[LOOP_BACKEDGE_PC] = numeric_region::RegionOp::Jump {
                pc: LOOP_BACKEDGE_PC,
                target: NEXT_INSTRUCTION_DISTANCE,
            };
            assert!(
                select_numeric_region_condition_supernode(
                    &code,
                    &interior_entry,
                    FIRST_INSTRUCTION_PC,
                    quote.start,
                    quote.end,
                    &guard,
                )
                .is_none()
            );
        }

        let mut live = condition_code(DynOp::LoadLiteral {
            dst: BOUND_REGISTER,
            value: Literal::Number(LITERAL_BOUND),
        });
        let analysis = numeric_region::analyze(&live);
        let quote = &analysis.loops[0];
        let guard = numeric_region::GuardPlan::from_loop(quote);
        let operations = quote.operations();
        live.ops[BODY_COPY_PC].op = DynOp::Move {
            dst: COPY_REGISTER,
            src: INDEX_REGISTER,
        };
        assert!(
            select_numeric_region_condition_supernode(
                &live,
                &operations,
                FIRST_INSTRUCTION_PC,
                quote.start,
                quote.end,
                &guard,
            )
            .is_none()
        );
    }

    #[test]
    fn cooked_numeric_region_conditions_use_direct_true_and_false_exits() {
        const LEFT_VALUE: f64 = 1.0;
        const RIGHT_VALUE: f64 = 2.0;
        const OPERATOR_CASES: [(&str, bool); 6] = [
            ("equal", false),
            ("not_equal", true),
            ("less", true),
            ("less_equal", true),
            ("greater", false),
            ("greater_equal", false),
        ];
        const SOURCE_CASES: [TestConditionSource; 3] = [
            TestConditionSource::Local,
            TestConditionSource::Literal,
            TestConditionSource::Name,
        ];

        for source in SOURCE_CASES {
            for (operator, expected) in OPERATOR_CASES {
                let marker = run_cooked_region_condition(source, operator, LEFT_VALUE, RIGHT_VALUE);
                let expected_marker = if expected {
                    CONDITION_TRUE_EXIT_MARKER
                } else {
                    CONDITION_FALSE_EXIT_MARKER
                };
                assert_eq!(marker, expected_marker, "{} {operator}", source.name());
            }
        }

        const NAN_CASES: [(&str, bool); 3] =
            [("equal", false), ("not_equal", true), ("less", false)];
        for (operator, expected) in NAN_CASES {
            let marker = run_cooked_region_condition(
                TestConditionSource::Literal,
                operator,
                f64::NAN,
                RIGHT_VALUE,
            );
            let expected_marker = if expected {
                CONDITION_TRUE_EXIT_MARKER
            } else {
                CONDITION_FALSE_EXIT_MARKER
            };
            assert_eq!(marker, expected_marker, "NaN {operator}");
        }
    }

    #[test]
    fn cooked_return_stencil_moves_heap_register_ownership() {
        const RETURN_REGISTER: usize = 0;
        const RETURN_PC: usize = 0;
        const RETURN_RUN_END: usize = 1;
        const RETURN_TEXT: &str = "owned return";
        const EXTERNAL_AND_FRAME_OWNER_COUNT: usize = 2;

        let stencil = rustc_stencils::STENCILS
            .iter()
            .find(|stencil| stencil.name == "quench_dyn_return")
            .expect("cooked return stencil exists");
        let mut code = stencil.bytes.to_vec();
        let exit_offset = code.len();
        a64_word(&mut code, a64_abi::RETURN);
        patch_tail_branch(&mut code, stencil.next_relocation(), exit_offset)
            .expect("patch return continuation");
        if let Some(slow) = stencil.slow_relocation() {
            patch_tail_branch(&mut code, slow, exit_offset).expect("patch return slow exit");
        }

        let memory = map_executable(&code, code.len()).expect("map cooked return stencil");
        let entry = unsafe { std::mem::transmute::<*mut u8, RawRegionEntry>(memory.ptr) };
        let owner = Rc::new(RETURN_TEXT.to_owned());
        let mut registers = vec![Value::String(owner.clone())];
        let site = InlineSite {
            pc: RETURN_PC,
            opcode: InlineOpcode::Unsupported as usize,
            run_end: RETURN_RUN_END,
            dst: UNUSED_SITE_OPERAND,
            left: RETURN_REGISTER,
            right: UNUSED_SITE_OPERAND,
            literal: raw_value::RawValue::UNDEFINED.bits(),
        };
        let mut frame = RawRegionTestFrame {
            registers: registers.as_mut_ptr(),
            locals: std::ptr::null_mut(),
            local_count: 0,
            current_site: &site,
            sites: &site,
            name_snapshots: std::ptr::null(),
            result: Value::Undefined,
            region_arrays: std::ptr::null(),
            region_guard: test_region_guard,
            region_iterations: 0,
        };
        unsafe { enter_raw_region_stencil(entry, &mut frame, &site) };

        assert!(registers[RETURN_REGISTER].is_undefined());
        assert_eq!(
            frame.result.as_string().map(String::as_str),
            Some(RETURN_TEXT)
        );
        assert_eq!(Rc::strong_count(&owner), EXTERNAL_AND_FRAME_OWNER_COUNT);
    }

    #[test]
    fn cooked_value_move_keeps_traced_objects_on_the_native_edge() {
        const SOURCE_REGISTER: usize = 0;
        const DESTINATION_REGISTER: usize = 1;
        const MOVE_PC: usize = 0;
        const MOVE_RUN_END: usize = 1;

        let run = |source: Value| {
            let stencil = rustc_stencils::STENCILS
                .iter()
                .find(|stencil| stencil.name == "quench_dyn_move")
                .expect("cooked move stencil exists");
            let mut code = stencil.bytes.to_vec();
            let direct_exit = code.len();
            append_condition_exit_marker(&mut code, CONDITION_TRUE_EXIT_MARKER);
            let reference_counted_exit = code.len();
            append_condition_exit_marker(&mut code, CONDITION_FALSE_EXIT_MARKER);
            patch_tail_branch(&mut code, stencil.next_relocation(), direct_exit)
                .expect("patch direct move continuation");
            patch_tail_branch(
                &mut code,
                stencil
                    .slow_relocation()
                    .expect("move has an ownership slow exit"),
                reference_counted_exit,
            )
            .expect("patch reference-counted move continuation");

            let mut registers = vec![source, Value::Undefined];
            let site = InlineSite {
                pc: MOVE_PC,
                opcode: InlineOpcode::Move as usize,
                run_end: MOVE_RUN_END,
                dst: DESTINATION_REGISTER,
                left: SOURCE_REGISTER,
                right: UNUSED_SITE_OPERAND,
                literal: raw_value::RawValue::UNDEFINED.bits(),
            };
            let marker = execute_region_test_code(
                &code,
                &mut registers,
                &mut [],
                std::slice::from_ref(&site),
                MOVE_PC,
                &[],
                &[],
            )
            .1;
            (marker, registers)
        };

        let object = Value::Object(test_object(Object::ordinary(None)));
        let object_bits = object.as_borrowed_raw().bits();
        let (object_marker, object_registers) = run(object);
        assert_eq!(object_marker, u64::from(CONDITION_TRUE_EXIT_MARKER));
        assert_eq!(
            object_registers[DESTINATION_REGISTER]
                .as_borrowed_raw()
                .bits(),
            object_bits
        );

        let text = Rc::new("retained".to_owned());
        let (string_marker, string_registers) = run(Value::String(text.clone()));
        assert_eq!(string_marker, u64::from(CONDITION_FALSE_EXIT_MARKER));
        assert!(string_registers[DESTINATION_REGISTER].is_undefined());
        assert_eq!(Rc::strong_count(&text), 2);
    }

    #[test]
    fn maximal_direct_sequence_is_structural_and_closed() {
        const PROPERTY_KEY: &str = "directProperty";
        const RECEIVER_REGISTER: Register = 0;
        const VALUE_REGISTER: Register = 1;
        const RESULT_REGISTER: Register = 2;
        const DIRECT_BLOCK_END_PC: usize = 5;
        const PROPERTY_LITERAL_VALUE: f64 = 1.0;
        let instruction = |op| super::super::dynbytecode::DynInstr {
            op,
            span: Span::default(),
        };
        let code = DynCode {
            ops: vec![
                DynOp::LoadLocal {
                    dst: RECEIVER_REGISTER,
                    slot: TEST_LOCAL_SLOT,
                },
                DynOp::LoadLiteral {
                    dst: VALUE_REGISTER,
                    value: Literal::Number(PROPERTY_LITERAL_VALUE),
                },
                DynOp::SetStatic {
                    object: RECEIVER_REGISTER,
                    key: PROPERTY_KEY.to_owned(),
                    src: VALUE_REGISTER,
                },
                DynOp::GetStatic {
                    dst: RESULT_REGISTER,
                    object: RECEIVER_REGISTER,
                    key: PROPERTY_KEY.to_owned(),
                },
                DynOp::Return {
                    src: Some(RESULT_REGISTER),
                },
            ]
            .into_iter()
            .map(instruction)
            .collect(),
            registers: usize::from(RESULT_REGISTER) + 1,
            params: Vec::new(),
            hoisted: Vec::new(),
            source_id: None,
            blocks: vec![(TEST_CONDITION_START_PC, DIRECT_BLOCK_END_PC, false)],
            bindings: Vec::new(),
            is_script: false,
        };
        assert!(
            direct_opcode_sequence(&code, FIRST_INSTRUCTION_PC, code.ops.len(), false).is_some()
        );

        let mut computed = code;
        computed.ops[1].op = DynOp::GetComputed {
            dst: VALUE_REGISTER,
            object: RECEIVER_REGISTER,
            key: RESULT_REGISTER,
        };
        assert!(
            direct_opcode_sequence(&computed, FIRST_INSTRUCTION_PC, computed.ops.len(), false,)
                .is_some()
        );
    }

    #[test]
    fn cooked_static_property_stencils_take_guarded_own_and_one_level_inherited_hits() {
        const PROPERTY_KEY: &str = "value";
        const RECEIVER_REGISTER: usize = 0;
        const VALUE_REGISTER: usize = 1;
        const PROPERTY_PC: usize = 0;
        const PROPERTY_RUN_END: usize = 1;
        const INITIAL_VALUE: f64 = 3.0;
        const REPLACEMENT_VALUE: f64 = 9.0;

        fn run(name: &str, registers: &mut [Value], site: &InlineSite) -> u64 {
            let stencil = rustc_stencils::STENCILS
                .iter()
                .find(|stencil| stencil.name == name)
                .unwrap_or_else(|| panic!("missing property stencil {name}"));
            let mut code = stencil.bytes.to_vec();
            let direct_exit = code.len();
            append_condition_exit_marker(&mut code, CONDITION_TRUE_EXIT_MARKER);
            let slow_exit = code.len();
            append_condition_exit_marker(&mut code, CONDITION_FALSE_EXIT_MARKER);
            patch_tail_branch(&mut code, stencil.next_relocation(), direct_exit)
                .expect("patch property continuation");
            patch_tail_branch(
                &mut code,
                stencil
                    .slow_relocation()
                    .expect("property stencil has a guard-miss exit"),
                slow_exit,
            )
            .expect("patch property guard-miss continuation");
            execute_region_test_code(
                &code,
                registers,
                &mut [],
                std::slice::from_ref(site),
                PROPERTY_PC,
                &[],
                &[],
            )
            .1
        }

        let mut object = Object::ordinary(None);
        object
            .props
            .insert(PROPERTY_KEY, Value::Number(INITIAL_VALUE));
        let receiver = Value::Object(test_object(object));
        let cache = PropertyIcSite::new(false);
        drop(get_static_cached(&receiver, PROPERTY_KEY, &cache).unwrap());
        let mut registers = vec![receiver, Value::Undefined];
        let get_site = InlineSite {
            pc: PROPERTY_PC,
            opcode: InlineOpcode::Unsupported as usize,
            run_end: PROPERTY_RUN_END,
            dst: VALUE_REGISTER,
            left: RECEIVER_REGISTER,
            right: UNUSED_SITE_OPERAND,
            literal: std::ptr::from_ref(&cache) as usize as u64,
        };
        assert_eq!(
            run("quench_dyn_get_static", &mut registers, &get_site),
            u64::from(CONDITION_TRUE_EXIT_MARKER)
        );
        assert_eq!(registers[VALUE_REGISTER].as_number(), Some(INITIAL_VALUE));

        registers[VALUE_REGISTER] = Value::Number(REPLACEMENT_VALUE);
        let set_site = InlineSite {
            dst: VALUE_REGISTER,
            left: RECEIVER_REGISTER,
            ..get_site
        };
        assert_eq!(
            run("quench_dyn_set_static", &mut registers, &set_site),
            u64::from(CONDITION_TRUE_EXIT_MARKER)
        );
        assert_eq!(
            registers[RECEIVER_REGISTER]
                .as_object_ref()
                .unwrap()
                .borrow()
                .props
                .get(PROPERTY_KEY)
                .unwrap()
                .as_number(),
            Some(REPLACEMENT_VALUE)
        );

        let different_shape = Value::Object(test_object(Object::ordinary(None)));
        registers[RECEIVER_REGISTER] = different_shape;
        registers[VALUE_REGISTER] = Value::Undefined;
        assert_eq!(
            run("quench_dyn_get_static", &mut registers, &get_site),
            u64::from(CONDITION_FALSE_EXIT_MARKER)
        );
        assert!(registers[VALUE_REGISTER].is_undefined());

        let mut prototype_object = Object::ordinary(None);
        prototype_object
            .props
            .insert(PROPERTY_KEY, Value::Number(INITIAL_VALUE));
        let prototype = test_object(prototype_object);
        let inherited_receiver = Value::Object(test_object(Object::ordinary(Some(prototype))));
        let inherited_cache = PropertyIcSite::new(false);
        drop(get_static_cached(
            &inherited_receiver,
            PROPERTY_KEY,
            &inherited_cache,
        ));
        let inherited_site = InlineSite {
            literal: std::ptr::from_ref(&inherited_cache) as usize as u64,
            ..get_site
        };
        registers[RECEIVER_REGISTER] = inherited_receiver;
        assert_eq!(
            run("quench_dyn_get_static", &mut registers, &inherited_site),
            u64::from(CONDITION_TRUE_EXIT_MARKER)
        );
        assert_eq!(registers[VALUE_REGISTER].as_number(), Some(INITIAL_VALUE));

        prototype
            .borrow_mut()
            .props
            .insert("other".into(), Value::Number(REPLACEMENT_VALUE));
        registers[VALUE_REGISTER] = Value::Undefined;
        assert_eq!(
            run("quench_dyn_get_static", &mut registers, &inherited_site),
            u64::from(CONDITION_FALSE_EXIT_MARKER)
        );
        assert!(registers[VALUE_REGISTER].is_undefined());
    }

    #[test]
    fn cooked_computed_stencils_access_existing_dense_slots() {
        const RECEIVER_REGISTER: usize = 0;
        const KEY_REGISTER: usize = 1;
        const VALUE_REGISTER: usize = 2;
        const COMPUTED_PC: usize = 0;
        const COMPUTED_RUN_END: usize = 1;
        const ARRAY_INDEX: f64 = 1.0;
        const INITIAL_VALUE: f64 = 4.0;
        const REPLACEMENT_VALUE: f64 = 9.0;

        fn run(name: &str, registers: &mut [Value], site: &InlineSite) -> u64 {
            let stencil = rustc_stencils::STENCILS
                .iter()
                .find(|stencil| stencil.name == name)
                .unwrap_or_else(|| panic!("missing computed stencil {name}"));
            let mut code = stencil.bytes.to_vec();
            let direct_exit = code.len();
            append_condition_exit_marker(&mut code, CONDITION_TRUE_EXIT_MARKER);
            let slow_exit = code.len();
            append_condition_exit_marker(&mut code, CONDITION_FALSE_EXIT_MARKER);
            patch_tail_branch(&mut code, stencil.next_relocation(), direct_exit)
                .expect("patch computed continuation");
            patch_tail_branch(
                &mut code,
                stencil
                    .slow_relocation()
                    .expect("computed stencil slow exit"),
                slow_exit,
            )
            .expect("patch computed slow continuation");
            execute_region_test_code(
                &code,
                registers,
                &mut [],
                std::slice::from_ref(site),
                COMPUTED_PC,
                &[],
                &[],
            )
            .1
        }

        let receiver = Value::Object(test_object(Object::array(
            None,
            vec![Value::Number(0.0), Value::Number(INITIAL_VALUE)],
        )));
        let mut registers = vec![receiver, Value::Number(ARRAY_INDEX), Value::Undefined];
        let get_site = InlineSite {
            pc: COMPUTED_PC,
            opcode: InlineOpcode::Unsupported as usize,
            run_end: COMPUTED_RUN_END,
            dst: VALUE_REGISTER,
            left: RECEIVER_REGISTER,
            right: KEY_REGISTER,
            literal: raw_value::RawValue::UNDEFINED.bits(),
        };
        assert_eq!(
            run("quench_dyn_get_computed_dense", &mut registers, &get_site),
            u64::from(CONDITION_TRUE_EXIT_MARKER)
        );
        assert_eq!(registers[VALUE_REGISTER].as_number(), Some(INITIAL_VALUE));

        registers[VALUE_REGISTER] = Value::Number(REPLACEMENT_VALUE);
        assert_eq!(
            run("quench_dyn_set_computed_dense", &mut registers, &get_site),
            u64::from(CONDITION_TRUE_EXIT_MARKER)
        );
        assert_eq!(
            registers[RECEIVER_REGISTER]
                .as_object_ref()
                .unwrap()
                .borrow()
                .array
                .as_ref()
                .unwrap()
                .get(ARRAY_INDEX as usize)
                .and_then(Value::as_number),
            Some(REPLACEMENT_VALUE)
        );

        registers[KEY_REGISTER] = Value::Number(99.0);
        registers[VALUE_REGISTER] = Value::Undefined;
        assert_eq!(
            run("quench_dyn_get_computed_dense", &mut registers, &get_site),
            u64::from(CONDITION_FALSE_EXIT_MARKER)
        );
    }

    #[test]
    fn cooked_equality_stencils_match_fast_primitive_semantics() {
        const LEFT_REGISTER: usize = 0;
        const RIGHT_REGISTER: usize = 1;
        const RESULT_REGISTER: usize = 2;
        const EQUALITY_PC: usize = 0;
        const EQUALITY_RUN_END: usize = 1;
        const EQUAL_NUMBER: f64 = 7.0;
        const BOOLEAN_FALSE_NUMBER: f64 = 0.0;
        const COERCIVE_STRING: &str = "1";
        const COERCIVE_NUMBER: f64 = 1.0;

        fn run(name: &str, left: Value, right: Value) -> (u64, Option<bool>) {
            let stencil = rustc_stencils::STENCILS
                .iter()
                .find(|stencil| stencil.name == name)
                .unwrap_or_else(|| panic!("missing equality stencil {name}"));
            let mut code = stencil.bytes.to_vec();
            let direct_exit = code.len();
            append_condition_exit_marker(&mut code, CONDITION_TRUE_EXIT_MARKER);
            let slow_exit = code.len();
            append_condition_exit_marker(&mut code, CONDITION_FALSE_EXIT_MARKER);
            patch_tail_branch(&mut code, stencil.next_relocation(), direct_exit)
                .expect("patch equality continuation");
            patch_tail_branch(
                &mut code,
                stencil
                    .slow_relocation()
                    .expect("equality stencil has a coercion slow exit"),
                slow_exit,
            )
            .expect("patch equality slow continuation");
            let mut registers = vec![left, right, Value::Undefined];
            let site = InlineSite {
                pc: EQUALITY_PC,
                opcode: InlineOpcode::Unsupported as usize,
                run_end: EQUALITY_RUN_END,
                dst: RESULT_REGISTER,
                left: LEFT_REGISTER,
                right: RIGHT_REGISTER,
                literal: raw_value::RawValue::UNDEFINED.bits(),
            };
            let marker = execute_region_test_code(
                &code,
                &mut registers,
                &mut [],
                std::slice::from_ref(&site),
                EQUALITY_PC,
                &[],
                &[],
            )
            .1;
            (marker, registers[RESULT_REGISTER].as_bool())
        }

        let direct = u64::from(CONDITION_TRUE_EXIT_MARKER);
        let slow = u64::from(CONDITION_FALSE_EXIT_MARKER);
        assert_eq!(
            run(
                "quench_dyn_strict_equal",
                Value::Number(EQUAL_NUMBER),
                Value::Number(EQUAL_NUMBER)
            ),
            (direct, Some(true))
        );
        assert_eq!(
            run("quench_dyn_strict_equal", Value::Null, Value::Undefined),
            (direct, Some(false))
        );
        assert_eq!(
            run("quench_dyn_equal", Value::Null, Value::Undefined),
            (direct, Some(true))
        );
        assert_eq!(
            run(
                "quench_dyn_equal",
                Value::Bool(false),
                Value::Number(BOOLEAN_FALSE_NUMBER)
            ),
            (direct, Some(true))
        );
        assert_eq!(
            run(
                "quench_dyn_strict_not_equal",
                Value::Number(EQUAL_NUMBER),
                Value::Number(EQUAL_NUMBER)
            ),
            (direct, Some(false))
        );
        assert_eq!(
            run(
                "quench_dyn_equal",
                Value::String(Rc::new(COERCIVE_STRING.to_owned())),
                Value::Number(COERCIVE_NUMBER)
            ),
            (slow, None)
        );
    }

    #[test]
    fn cooked_dense_region_stencils_use_validated_view() {
        const ARRAY_INDEX: usize = 1;
        const INDEX_REGISTER: usize = 0;
        const VALUE_REGISTER: usize = 1;
        const RESULT_REGISTER: usize = 2;
        let mut elements = vec![Value::Number(2.0), Value::Number(4.0)];
        let views = [RegionArrayView {
            elements: elements.as_mut_ptr(),
            length: elements.len(),
        }];
        let mut registers = vec![
            Value::Number(ARRAY_INDEX as f64),
            Value::Number(7.0),
            Value::Undefined,
        ];
        let read = InlineSite {
            pc: 0,
            opcode: 0,
            run_end: 1,
            dst: RESULT_REGISTER,
            left: 0,
            right: INDEX_REGISTER,
            literal: 0,
        };
        run_region_stencil(
            "quench_region_read_dense",
            &mut registers,
            &mut [],
            &read,
            &views,
            &[],
        );
        assert_eq!(registers[RESULT_REGISTER].as_number(), Some(4.0));

        let write = InlineSite {
            dst: VALUE_REGISTER,
            ..read
        };
        run_region_stencil(
            "quench_region_write_dense",
            &mut registers,
            &mut [],
            &write,
            &views,
            &[],
        );
        assert_eq!(elements[ARRAY_INDEX].as_number(), Some(7.0));

        elements[ARRAY_INDEX] = Value::Number(4.0);
        registers[VALUE_REGISTER] = Value::Number(9.0);
        registers[RESULT_REGISTER] = Value::Undefined;
        run_region_stencil(
            "quench_region_read_dense_proven_index",
            &mut registers,
            &mut [],
            &read,
            &views,
            &[],
        );
        assert_eq!(registers[RESULT_REGISTER].as_number(), Some(4.0));
        run_region_stencil(
            "quench_region_write_dense_proven_index",
            &mut registers,
            &mut [],
            &write,
            &views,
            &[],
        );
        assert_eq!(elements[ARRAY_INDEX].as_number(), Some(9.0));
    }

    #[test]
    fn cooked_property_region_stencils_use_validated_view() {
        const RESULT_REGISTER: usize = 0;
        const VALUE_REGISTER: usize = 1;
        const PROPERTY_VIEW_INDEX: u64 = 0;
        let mut property = Value::Number(2.0);
        let views = [RegionArrayView {
            elements: &mut property,
            length: PROPERTY_VIEW_UNUSED_LENGTH,
        }];
        let mut registers = vec![Value::Undefined, Value::Number(7.0)];
        let read = InlineSite {
            pc: 0,
            opcode: 0,
            run_end: 1,
            dst: RESULT_REGISTER,
            left: 0,
            right: 0,
            literal: PROPERTY_VIEW_INDEX,
        };
        run_region_stencil(
            "quench_region_read_static",
            &mut registers,
            &mut [],
            &read,
            &[],
            &views,
        );
        assert_eq!(registers[RESULT_REGISTER].as_number(), Some(2.0));

        let write = InlineSite {
            dst: VALUE_REGISTER,
            ..read
        };
        run_region_stencil(
            "quench_region_write_static",
            &mut registers,
            &mut [],
            &write,
            &[],
            &views,
        );
        assert_eq!(property.as_number(), Some(7.0));
    }

    #[test]
    fn cooked_own_property_load_store_jump_reads_immediate_slot() {
        const PROPERTY_NAME: &str = "field";
        const RECEIVER_LOCAL: usize = 0;
        const DESTINATION_LOCAL: usize = 1;
        const PROPERTY_SLOT: usize = 0;
        const PROPERTY_VALUE: f64 = 7.0;
        const INITIAL_DESTINATION: f64 = 99.0;
        const JUMP_TARGET_PC: usize = 0;

        let stencil = rustc_stencils::STENCILS
            .iter()
            .find(|stencil| stencil.name == "quench_dyn_dead_own_property_store_local_jump")
            .expect("cooked own-property transfer stencil exists");
        let mut code = stencil.bytes.to_vec();
        let exit_offset = code.len();
        a64_word(&mut code, a64_abi::RETURN);
        patch_tail_branch(&mut code, stencil.next_relocation(), exit_offset)
            .expect("patch fast continuation");
        patch_tail_branch(
            &mut code,
            stencil
                .slow_relocation()
                .expect("property stencil has slow exit"),
            exit_offset,
        )
        .expect("patch slow continuation");

        let mut properties = PropertyStorage::new();
        properties.insert(PROPERTY_NAME, Value::Number(PROPERTY_VALUE));
        let object = test_object(Object {
            props: properties,
            prototype: None,
            dense_access: DenseArrayAccess::EMPTY,
            array: None,
            extensible: true,
            builtin_prototype: false,
            attributes: HashMap::new(),
        });
        let receiver_shape = object.borrow().props.shape.0;
        let property_ic = PropertyIc {
            receiver_shape,
            slot: PROPERTY_SLOT,
        };
        let mut locals = vec![Value::Object(object), Value::Number(INITIAL_DESTINATION)];
        let mut registers = vec![Value::Undefined; TEST_CONDITION_REGISTER_COUNT];
        let mut sites = [
            InlineSite::unused(0),
            InlineSite::unused(PROPERTY_LOAD_GET_PC_OFFSET),
            InlineSite::unused(PROPERTY_LOAD_STORE_PC_OFFSET),
            InlineSite::unused(PROPERTY_LOAD_JUMP_PC_OFFSET),
        ];
        sites[0].left = RECEIVER_LOCAL;
        sites[PROPERTY_LOAD_GET_PC_OFFSET].literal =
            std::ptr::from_ref(&property_ic) as usize as u64;
        sites[PROPERTY_LOAD_STORE_PC_OFFSET].dst = DESTINATION_LOCAL;
        sites[PROPERTY_LOAD_JUMP_PC_OFFSET].literal = JUMP_TARGET_PC as u64;

        let memory = map_executable(&code, code.len()).expect("map property stencil");
        let entry = unsafe { std::mem::transmute::<*mut u8, RawRegionEntry>(memory.ptr) };
        let mut frame = RawRegionTestFrame {
            registers: registers.as_mut_ptr(),
            locals: locals.as_mut_ptr(),
            local_count: locals.len(),
            current_site: sites.as_ptr(),
            sites: sites.as_ptr(),
            name_snapshots: std::ptr::null(),
            result: Value::Undefined,
            region_arrays: std::ptr::null(),
            region_guard: test_region_guard,
            region_iterations: 0,
        };
        unsafe { enter_raw_region_stencil(entry, &mut frame, sites.as_ptr()) };

        assert_eq!(locals[DESTINATION_LOCAL].as_number(), Some(PROPERTY_VALUE));
    }

    #[test]
    fn cooked_terminal_property_stencil_transfers_rc_owners_without_helpers() {
        const FIRST_PROPERTY_NAME: &str = "first";
        const SECOND_PROPERTY_NAME: &str = "second";
        const FIRST_RECEIVER_LOCAL: usize = 0;
        const FIRST_SOURCE_LOCAL: usize = 1;
        const SECOND_RECEIVER_LOCAL: usize = 2;
        const SECOND_SOURCE_LOCAL: usize = 3;
        const PROPERTY_SLOT: usize = 0;
        const FIRST_OLD_VALUE: &str = "first old owner";
        const FIRST_NEW_VALUE: &str = "first new owner";
        const SECOND_OLD_VALUE: &str = "second old owner";
        const SECOND_NEW_VALUE: &str = "second new owner";

        let stencil = rustc_stencils::STENCILS
            .iter()
            .find(|stencil| stencil.name == "quench_dyn_take_two_own_properties_return_undefined")
            .expect("cooked ownership-taking property stencil exists");
        let mut code = stencil.bytes.to_vec();
        let exit_offset = code.len();
        a64_word(&mut code, a64_abi::RETURN);
        patch_tail_branch(&mut code, stencil.next_relocation(), exit_offset)
            .expect("patch ownership-taking fast continuation");
        patch_tail_branch(
            &mut code,
            stencil
                .slow_relocation()
                .expect("ownership-taking property stencil has slow exit"),
            exit_offset,
        )
        .expect("patch ownership-taking slow continuation");

        let mut first_properties = PropertyStorage::new();
        first_properties.insert(
            FIRST_PROPERTY_NAME,
            Value::String(Rc::new(FIRST_OLD_VALUE.to_owned())),
        );
        let first_object = test_object(Object {
            props: first_properties,
            prototype: None,
            dense_access: DenseArrayAccess::EMPTY,
            array: None,
            extensible: true,
            builtin_prototype: false,
            attributes: HashMap::new(),
        });
        let first_ic = PropertyIc {
            receiver_shape: first_object.borrow().props.shape.0,
            slot: PROPERTY_SLOT,
        };

        let mut second_properties = PropertyStorage::new();
        second_properties.insert(
            SECOND_PROPERTY_NAME,
            Value::String(Rc::new(SECOND_OLD_VALUE.to_owned())),
        );
        let second_object = test_object(Object {
            props: second_properties,
            prototype: None,
            dense_access: DenseArrayAccess::EMPTY,
            array: None,
            extensible: true,
            builtin_prototype: false,
            attributes: HashMap::new(),
        });
        let second_ic = PropertyIc {
            receiver_shape: second_object.borrow().props.shape.0,
            slot: PROPERTY_SLOT,
        };

        let mut locals = vec![
            Value::Object(first_object),
            Value::String(Rc::new(FIRST_NEW_VALUE.to_owned())),
            Value::Object(second_object),
            Value::String(Rc::new(SECOND_NEW_VALUE.to_owned())),
        ];
        let mut registers = vec![Value::Undefined; TEST_CONDITION_REGISTER_COUNT];
        let mut sites: [InlineSite; TWO_PROPERTY_STORE_INSTRUCTION_COUNT] =
            std::array::from_fn(InlineSite::unused);
        sites[TWO_PROPERTY_STORE_FIRST_RECEIVER_LOAD_PC_OFFSET].left = FIRST_RECEIVER_LOCAL;
        sites[TWO_PROPERTY_STORE_FIRST_VALUE_LOAD_PC_OFFSET].left = FIRST_SOURCE_LOCAL;
        sites[TWO_PROPERTY_STORE_FIRST_SET_PC_OFFSET].literal =
            std::ptr::from_ref(&first_ic) as usize as u64;
        sites[TWO_PROPERTY_STORE_SECOND_RECEIVER_LOAD_PC_OFFSET].left = SECOND_RECEIVER_LOCAL;
        sites[TWO_PROPERTY_STORE_SECOND_VALUE_LOAD_PC_OFFSET].left = SECOND_SOURCE_LOCAL;
        sites[TWO_PROPERTY_STORE_SECOND_SET_PC_OFFSET].literal =
            std::ptr::from_ref(&second_ic) as usize as u64;

        let memory = map_executable(&code, code.len()).expect("map ownership-taking stencil");
        let entry = unsafe { std::mem::transmute::<*mut u8, RawRegionEntry>(memory.ptr) };
        let mut frame = RawRegionTestFrame {
            registers: registers.as_mut_ptr(),
            locals: locals.as_mut_ptr(),
            local_count: locals.len(),
            current_site: sites.as_ptr(),
            sites: sites.as_ptr(),
            name_snapshots: std::ptr::null(),
            result: Value::Undefined,
            region_arrays: std::ptr::null(),
            region_guard: test_region_guard,
            region_iterations: 0,
        };
        unsafe { enter_raw_region_stencil(entry, &mut frame, sites.as_ptr()) };

        assert_eq!(
            first_object
                .borrow()
                .props
                .get(FIRST_PROPERTY_NAME)
                .and_then(Value::as_string)
                .map(String::as_str),
            Some(FIRST_NEW_VALUE)
        );
        assert_eq!(
            second_object
                .borrow()
                .props
                .get(SECOND_PROPERTY_NAME)
                .and_then(Value::as_string)
                .map(String::as_str),
            Some(SECOND_NEW_VALUE)
        );
        assert_eq!(
            locals[FIRST_SOURCE_LOCAL].as_string().map(String::as_str),
            Some(FIRST_OLD_VALUE)
        );
        assert_eq!(
            locals[SECOND_SOURCE_LOCAL].as_string().map(String::as_str),
            Some(SECOND_OLD_VALUE)
        );
        assert!(frame.result.is_undefined());
    }

    #[test]
    fn rustc_stencil_catalog_copy_patches_and_executes() {
        const LEFT: f64 = 1.25;
        const RIGHT: f64 = 2.75;
        let cases = [
            ("quench_load_value", RIGHT),
            ("quench_number_add", LEFT + RIGHT),
            ("quench_number_divide", LEFT / RIGHT),
            ("quench_number_multiply", LEFT * RIGHT),
            ("quench_number_subtract", LEFT - RIGHT),
        ];
        for (expected_name, expected) in cases {
            let stencil = rustc_stencils::STENCILS
                .iter()
                .find(|stencil| stencil.name == expected_name)
                .expect("legacy raw stencil in catalog");
            assert_eq!(stencil.name, expected_name);
            assert_eq!(run_raw_stencil(stencil, LEFT, RIGHT), expected);
        }
    }

    #[test]
    fn rustc_stencils_compose_and_link_as_one_function() {
        const LEFT: f64 = 1.25;
        const RIGHT: f64 = 2.75;
        let find = |name| {
            rustc_stencils::STENCILS
                .iter()
                .position(|stencil| stencil.name == name)
                .expect("catalog stencil")
        };
        let terminal = Stencil::<Connector, Connector>::leaf(LeafStencil {
            level: StencilLevel::Function,
            bytes: [
                MOVE_ACCUMULATOR_TO_RETURN.to_le_bytes(),
                a64_abi::RETURN.to_le_bytes(),
            ]
            .concat(),
            holes: Vec::new(),
            labels: Vec::new(),
            fragments: Vec::new(),
        });
        let graph = rustc_stencil(find("quench_number_add"))
            + rustc_stencil(find("quench_number_multiply"))
            + terminal;
        let code = link_rustc_chain(graph.image()).expect("link composable rustc stencils");
        assert!(graph.holes().iter().all(|hole| matches!(
            hole,
            Hole::Internal {
                target: SymbolicTarget::Offset(_),
                ..
            }
        )));
        assert_eq!(run_raw_code(&code, LEFT, RIGHT), (LEFT + RIGHT) * RIGHT);
    }

    #[test]
    fn tail_connector_linking_preserves_explicit_edges() {
        let mut adjacent = a64_abi::BR_BASE.to_le_bytes().to_vec();
        patch_tail_branch(&mut adjacent, 0, a64_abi::INSTRUCTION_BYTES)
            .expect("patch adjacent connector");
        assert_eq!(
            u32::from_le_bytes(adjacent.try_into().expect("one instruction")),
            a64_abi::BR_BASE | 1
        );

        let mut non_adjacent = vec![0; a64_abi::INSTRUCTION_BYTES * 2];
        non_adjacent[..a64_abi::INSTRUCTION_BYTES].copy_from_slice(&a64_abi::BR_BASE.to_le_bytes());
        patch_tail_branch(&mut non_adjacent, 0, a64_abi::INSTRUCTION_BYTES * 2)
            .expect("patch non-adjacent connector");
        let linked = u32::from_le_bytes(
            non_adjacent[..a64_abi::INSTRUCTION_BYTES]
                .try_into()
                .expect("one instruction"),
        );
        assert_eq!(linked & a64_abi::BR_OPCODE_MASK, a64_abi::BR_BASE);
        assert_eq!(linked, a64_abi::BR_BASE | 2);
    }

    fn run_raw_stencil(stencil: &rustc_stencils::RustcStencil, left: f64, right: f64) -> f64 {
        let mut code = stencil.bytes.to_vec();
        let branch_offset = stencil.next_relocation();
        let exit_offset = code.len();
        a64_word(&mut code, MOVE_ACCUMULATOR_TO_RETURN);
        a64_word(&mut code, a64_abi::RETURN);

        let displacement = exit_offset as isize - branch_offset as isize;
        assert_eq!(displacement % a64_abi::INSTRUCTION_BYTES as isize, 0);
        let immediate = (displacement / a64_abi::INSTRUCTION_BYTES as isize) as u32;
        let branch = a64_abi::BR_BASE | (immediate & a64_abi::BR_IMM_MASK);
        code[branch_offset..branch_offset + a64_abi::INSTRUCTION_BYTES]
            .copy_from_slice(&branch.to_le_bytes());

        run_raw_code(&code, left, right)
    }

    #[derive(Clone, Copy)]
    enum TestConditionSource {
        Local,
        Literal,
        Name,
    }

    impl TestConditionSource {
        fn name(self) -> &'static str {
            match self {
                Self::Local => "local_local",
                Self::Literal => "local_literal",
                Self::Name => "local_name",
            }
        }
    }

    fn append_condition_exit_marker(code: &mut Vec<u8>, marker: u16) {
        let move_marker = AARCH64_MOVE_WIDE_ZERO_64_BASE
            | (u32::from(marker) << AARCH64_MOVE_WIDE_IMMEDIATE_FIELD_SHIFT)
            | EXIT_MARKER_REGISTER;
        let store_marker = a64_abi::STR_X_UNSIGNED_BASE
            | ((RESULT_FRAME_WORD_OFFSET as u32) << AARCH64_UNSIGNED_OFFSET_FIELD_SHIFT)
            | (FRAME_ARGUMENT_REGISTER << AARCH64_SECOND_REGISTER_FIELD_SHIFT)
            | EXIT_MARKER_REGISTER;
        a64_word(code, move_marker);
        a64_word(code, store_marker);
        a64_word(code, a64_abi::RETURN);
    }

    fn run_cooked_region_condition(
        source: TestConditionSource,
        operator: &str,
        left: f64,
        right: f64,
    ) -> u16 {
        const LEFT_LOCAL_SLOT: usize = 0;
        const RIGHT_LOCAL_SLOT: usize = 1;
        const LEFT_SITE_INDEX: usize = 0;
        const RIGHT_SITE_INDEX: usize = 1;
        const NAME_SNAPSHOT_SLOT: usize = 0;
        const BRANCH_SITE_INDEX: usize = 3;

        let name = format!(
            "quench_region_dead_condition_{}_number_{operator}",
            source.name()
        );
        let stencil = rustc_stencils::STENCILS
            .iter()
            .find(|stencil| stencil.name == name)
            .unwrap_or_else(|| panic!("missing condition stencil {name}"));
        let mut sites = (0..CONDITION_INSTRUCTION_COUNT)
            .map(InlineSite::unused)
            .collect::<Vec<_>>();
        sites[LEFT_SITE_INDEX].left = LEFT_LOCAL_SLOT;
        sites[RIGHT_SITE_INDEX].left = RIGHT_LOCAL_SLOT;
        sites[RIGHT_SITE_INDEX].literal = match source {
            TestConditionSource::Name => NAME_SNAPSHOT_SLOT as u64,
            TestConditionSource::Local | TestConditionSource::Literal => right.to_bits(),
        };
        sites[BRANCH_SITE_INDEX].literal = FIRST_INSTRUCTION_PC as u64;
        let mut locals = vec![Value::Number(left), Value::Number(right)];
        let mut registers = vec![Value::Undefined; TEST_CONDITION_REGISTER_COUNT];
        let snapshots = vec![Value::Number(right)];
        let marker =
            run_region_condition_code(stencil, &mut registers, &mut locals, &sites, &snapshots);
        assert!(registers.iter().all(Value::is_undefined));
        u16::try_from(marker).expect("condition exit marker fits u16")
    }

    fn run_region_condition_code(
        stencil: &rustc_stencils::RustcStencil,
        registers: &mut [Value],
        locals: &mut [Value],
        sites: &[InlineSite],
        name_snapshots: &[Value],
    ) -> u64 {
        let mut code = stencil.bytes.to_vec();
        let true_exit = code.len();
        append_condition_exit_marker(&mut code, CONDITION_TRUE_EXIT_MARKER);
        let false_exit = code.len();
        append_condition_exit_marker(&mut code, CONDITION_FALSE_EXIT_MARKER);
        patch_tail_branch(&mut code, stencil.next_relocation(), true_exit)
            .expect("patch condition true exit");
        patch_tail_branch(
            &mut code,
            stencil
                .branch_relocation()
                .expect("condition branch relocation"),
            false_exit,
        )
        .expect("patch condition false exit");
        execute_region_test_code(
            &code,
            registers,
            locals,
            sites,
            FIRST_INSTRUCTION_PC,
            name_snapshots,
            &[],
        )
        .1
    }

    fn run_region_stencil(
        name: &str,
        registers: &mut [Value],
        locals: &mut [Value],
        site: &InlineSite,
        region_arrays: &[RegionArrayView],
        region_properties: &[RegionArrayView],
    ) -> u64 {
        let stencil = rustc_stencils::STENCILS
            .iter()
            .find(|stencil| stencil.name == name)
            .unwrap_or_else(|| panic!("missing region stencil {name}"));
        let mut code = stencil.bytes.to_vec();
        let exit_offset = code.len();
        a64_word(&mut code, a64_abi::RETURN);
        patch_tail_branch(&mut code, stencil.next_relocation(), exit_offset)
            .expect("patch region continuation");
        if let Some(branch) = stencil.branch_relocation() {
            patch_tail_branch(&mut code, branch, exit_offset).expect("patch region branch exit");
        }
        if let Some(slow) = stencil.slow_relocation() {
            patch_tail_branch(&mut code, slow, exit_offset).expect("patch region slow exit");
        }
        execute_region_test_code(
            &code,
            registers,
            locals,
            std::slice::from_ref(site),
            FIRST_INSTRUCTION_PC,
            &[],
            &[region_arrays, region_properties],
        )
        .0
    }

    fn execute_region_test_code(
        code: &[u8],
        registers: &mut [Value],
        locals: &mut [Value],
        sites: &[InlineSite],
        entry_index: usize,
        name_snapshots: &[Value],
        view_groups: &[&[RegionArrayView]],
    ) -> (u64, u64) {
        let memory = map_executable(&code, code.len()).expect("map cooked region stencil");
        let entry = unsafe { std::mem::transmute::<*mut u8, RawRegionEntry>(memory.ptr) };
        let mut views = Vec::new();
        for group in view_groups {
            views.extend_from_slice(group);
        }
        let entry_site = &sites[entry_index];
        let mut frame = RawRegionTestFrame {
            registers: registers.as_mut_ptr(),
            locals: locals.as_mut_ptr(),
            local_count: locals.len(),
            current_site: entry_site,
            sites: sites.as_ptr(),
            name_snapshots: name_snapshots.as_ptr(),
            result: Value::Undefined,
            region_arrays: views.as_ptr(),
            region_guard: test_region_guard,
            region_iterations: 0,
        };
        unsafe { enter_raw_region_stencil(entry, &mut frame, entry_site) };
        (frame.region_iterations, frame.result.0.bits())
    }

    fn run_raw_code(code: &[u8], left: f64, right: f64) -> f64 {
        let memory = map_executable(code, code.len()).expect("map extracted stencil");
        let entry = unsafe { std::mem::transmute::<*mut u8, RawStencilEntry>(memory.ptr) };
        let left = raw_value::RawValue::number(left);
        let right = raw_value::RawValue::number(right);
        let result = unsafe {
            entry(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null_mut(),
                left,
                raw_value::TAG_MASK,
                raw_value::FIRST_TAG,
                right.bits(),
            )
        };
        result.as_number().expect("raw stencil returns a number")
    }

    #[test]
    fn property_cache_never_ignores_an_own_shadowing_property() {
        let prototype = test_object(Object::ordinary(None));
        prototype
            .borrow_mut()
            .props
            .insert("value".into(), Value::Number(1.0));
        let receiver = test_object(Object::ordinary(Some(prototype)));
        let receiver_value = Value::Object(receiver);
        let cache = PropertyIcSite::new(false);

        assert_eq!(
            get_static_cached(&receiver_value, "value", &cache)
                .unwrap()
                .number(),
            1.0
        );
        assert_eq!(
            cache.inherited.borrow().as_ref().map(|entry| entry.slot),
            Some(0),
            "prototype load records a guarded holder slot"
        );
        let published = cache.published_inherited.get();
        assert_eq!(published.receiver_shape, receiver.borrow().props.shape.0);
        assert_eq!(published.holder_identity, prototype.as_ptr());
        assert_eq!(published.holder_shape, prototype.borrow().props.shape.0);
        assert_eq!(published.slot, 0);
        receiver
            .borrow_mut()
            .props
            .insert("value".into(), Value::Number(2.0));
        assert_eq!(
            get_static_cached(&receiver_value, "value", &cache)
                .unwrap()
                .number(),
            2.0
        );
        assert_eq!(
            cache.published_inherited.get(),
            PublishedInheritedPropertyIc::EMPTY,
            "an own-property fill retires the inherited projection"
        );
    }

    #[test]
    fn property_cache_reuses_slots_only_for_the_guarded_shape() {
        let make = |first_key: &str, first_value: f64, second_key: &str| {
            let mut object = Object::ordinary(None);
            object
                .props
                .insert(first_key.into(), Value::Number(first_value));
            object
                .props
                .insert(second_key.into(), Value::Number(first_value + 1.0));
            Value::Object(test_object(object))
        };
        let first = make("value", 1.0, "other");
        let same_shape = make("value", 2.0, "other");
        let different_shape = make("other", 3.0, "value");
        let cache = PropertyIcSite::new(false);

        assert_eq!(
            get_static_cached(&first, "value", &cache).unwrap().number(),
            1.0
        );
        let guarded_shape = cache
            .own
            .get()
            .populated()
            .expect("own property populates cache");
        assert_eq!(
            get_static_cached(&same_shape, "value", &cache)
                .unwrap()
                .number(),
            2.0
        );
        assert_eq!(cache.own.get().populated(), Some(guarded_shape));
        assert_eq!(
            get_static_cached(&different_shape, "value", &cache)
                .unwrap()
                .number(),
            4.0
        );
        assert_ne!(cache.own.get().populated(), Some(guarded_shape));
    }

    #[test]
    fn inherited_property_projection_survives_gc_only_with_a_live_holder() {
        const PROPERTY_KEY: &str = "value";
        const PROPERTY_VALUE: f64 = 1.0;
        let fill = |prototype: ObjectHandle, cache: &PropertyIcSite| {
            prototype
                .borrow_mut()
                .props
                .insert(PROPERTY_KEY, Value::Number(PROPERTY_VALUE));
            let receiver = Value::Object(test_object(Object::ordinary(Some(prototype))));
            get_static_cached(&receiver, PROPERTY_KEY, cache).expect("inherited property fill");
        };

        let dead_holder = test_object(Object::ordinary(None));
        let dead_cache = PropertyIcSite::new(false);
        fill(dead_holder, &dead_cache);
        dead_cache.invalidate_unmarked_object_identities();
        assert_eq!(
            dead_cache.published_inherited.get(),
            PublishedInheritedPropertyIc::EMPTY
        );

        let live_holder = test_object(Object::ordinary(None));
        let live_cache = PropertyIcSite::new(false);
        fill(live_holder, &live_cache);
        assert!(live_holder.mark(), "the fixture holder starts unmarked");
        live_cache.invalidate_unmarked_object_identities();
        assert_eq!(
            live_cache.published_inherited.get().holder_identity,
            live_holder.as_ptr()
        );
    }

    #[test]
    fn prototype_cache_revalidates_shadowing_and_link_replacement() {
        const PROPERTY_KEY: &str = "value";
        const FIRST_VALUE: f64 = 1.0;
        const SHADOW_VALUE: f64 = 2.0;
        const REPLACEMENT_VALUE: f64 = 3.0;
        let first = test_object(Object::ordinary(None));
        first
            .borrow_mut()
            .props
            .insert(PROPERTY_KEY, Value::Number(FIRST_VALUE));
        let middle = test_object(Object::ordinary(Some(first)));
        let receiver = test_object(Object::ordinary(Some(middle)));
        let receiver_value = Value::Object(receiver);
        let cache = PropertyIcSite::new(false);

        assert_eq!(
            get_static_cached(&receiver_value, PROPERTY_KEY, &cache)
                .unwrap()
                .number(),
            FIRST_VALUE
        );
        assert_eq!(
            cache.published_inherited.get(),
            PublishedInheritedPropertyIc::EMPTY,
            "the first native projection is intentionally bounded to one prototype edge"
        );
        middle
            .borrow_mut()
            .props
            .insert(PROPERTY_KEY, Value::Number(SHADOW_VALUE));
        assert_eq!(
            get_static_cached(&receiver_value, PROPERTY_KEY, &cache)
                .unwrap()
                .number(),
            SHADOW_VALUE
        );

        let replacement = test_object(Object::ordinary(None));
        replacement
            .borrow_mut()
            .props
            .insert(PROPERTY_KEY, Value::Number(REPLACEMENT_VALUE));
        receiver.borrow_mut().prototype = Some(replacement);
        assert_eq!(
            get_static_cached(&receiver_value, PROPERTY_KEY, &cache)
                .unwrap()
                .number(),
            REPLACEMENT_VALUE
        );
    }

    #[test]
    fn static_property_cache_consumes_or_returns_one_owner() {
        const PROPERTY_KEY: &str = "value";
        const INITIAL_PROPERTY_VALUE: f64 = 1.0;

        let receiver = Value::Object(test_object(Object::ordinary(None)));
        receiver
            .as_object_ref()
            .unwrap()
            .borrow_mut()
            .props
            .insert(PROPERTY_KEY, Value::Number(INITIAL_PROPERTY_VALUE));
        let cache = PropertyIcSite::new(false);
        drop(get_static_cached(&receiver, PROPERTY_KEY, &cache).unwrap());

        let cached_owner = Rc::new("cached".to_owned());
        set_static_cached(
            &receiver,
            PROPERTY_KEY,
            Value::String(cached_owner.clone()),
            &cache,
        )
        .expect("shape hit consumes the supplied owner");
        assert_eq!(Rc::strong_count(&cached_owner), 2);

        let returned_owner = Rc::new("returned".to_owned());
        let returned = set_static_cached(
            &Value::Undefined,
            PROPERTY_KEY,
            Value::String(returned_owner.clone()),
            &PropertyIcSite::new(false),
        )
        .expect_err("cache miss returns the supplied owner");
        assert_eq!(Rc::strong_count(&returned_owner), 2);
        drop(returned);
        assert_eq!(Rc::strong_count(&returned_owner), 1);
    }
}
