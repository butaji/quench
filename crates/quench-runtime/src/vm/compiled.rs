//! Cranelift-backed compiled tiers for the shared VM.
//!
//! In Ershov's terms the lane's per-instruction semantics have two readings.
//! The lane handlers interpret them; this module is their generating
//! extension: for each instruction it emits the same meaning as Cranelift IR,
//! with every operand the residual already fixes (register numbers,
//! immediates, constants, jump targets, callees) folded in. Running it over a
//! function's residual bytecode is Futamura's first projection, the
//! interpreter specialized to that function; what specialization removes is
//! exactly what the program fixed: dispatch, operand decoding, value tags
//! (i32 and i64 stay unboxed), the register file (registers become SSA
//! variables) and the per-run memory-view resolution.
//!
//! Each operator keeps one semantic authority. Integer operators come from
//! the integer, float and conversion selectors of `crate::wasm` (an i32
//! operator is its i64 namesake at the narrower width), memory accesses from
//! `MemoryLoad`/`MemoryStore`, traps from `WasmTrap`. A function compiles only
//! when every instruction it can reach has a compiled denotation and every
//! function it calls compiles; anything else stays interpreted, so the mode
//! never changes behavior, only speed. Compiled code never allocates or
//! collects itself and holds no heap roots: values cross its boundary in
//! their Wasm encoding, a trap leaves as a status that becomes the
//! interpreter's `WasmTrap`, and an imported call is a callout through which
//! the VM makes the call exactly as the interpreter would (`import_callout`).

use super::*;
use crate::bytecode::{ControlFlowLayout, Function, WideInstruction};
use crate::wasm::conversion::ScalarConversionOperator;
use crate::wasm::float::{
    F32BinaryOperator, F32UnaryOperator, F64BinaryOperator, F64UnaryOperator,
};
use crate::wasm::integer::{
    I32BinaryOperator, I32UnaryOperator, I64BinaryOperator, I64UnaryOperator,
};
use crate::wasm::memory::{MemoryLoad, MemoryStore};
use cranelift_codegen::ir::{
    self, AbiParam, InstBuilder, MemFlagsData, Signature, UserFuncName,
    condcodes::{FloatCC, IntCC},
    immediates::{Ieee32, Ieee64},
    types,
};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use std::rc::{Rc, Weak};

/// How the shared VM executes JavaScript and Wasm functions. Compiled code is
/// lazy and falls back to the interpreter; both modes use the same VM, heap,
/// frames and host APIs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExecutionMode {
    /// The shared interpreter: general loop and fast lane.
    #[default]
    Interpreted,
    /// Hot supported JavaScript functions and Wasm functions run as native code.
    Compiled,
}

/// A value's width inside compiled code. Each register has an integer and a
/// float view per width (`View`); a write defines both, so the analysis
/// tracks only the width and a reinterpretation is free.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    I32,
    I64,
}

impl Kind {
    fn of(ty: crate::WasmType) -> Option<Self> {
        match ty {
            crate::WasmType::I32 | crate::WasmType::F32 => Some(Self::I32),
            crate::WasmType::I64 | crate::WasmType::F64 => Some(Self::I64),
            _ => None,
        }
    }

    fn ir(self) -> ir::Type {
        match self {
            Self::I32 => types::I32,
            Self::I64 => types::I64,
        }
    }

    fn integer(self) -> crate::WasmType {
        match self {
            Self::I32 => crate::WasmType::I32,
            Self::I64 => crate::WasmType::I64,
        }
    }

    fn of_ir(ty: ir::Type) -> Self {
        if ty.bits() == types::I32.bits() {
            Self::I32
        } else {
            Self::I64
        }
    }
}

/// The views of a register's value, integer and float per width, in the
/// order of a register's variables.
const VIEW_TYPES: [crate::WasmType; 4] = [
    crate::WasmType::I32,
    crate::WasmType::I64,
    crate::WasmType::F32,
    crate::WasmType::F64,
];

/// The variable index of a register's view as Wasm type `ty`.
fn view(ty: crate::WasmType) -> usize {
    VIEW_TYPES
        .iter()
        .position(|candidate| *candidate == ty)
        .expect("compiled values are numeric scalars")
}

/// The machine type of a scalar Wasm type.
fn scalar_ir(ty: crate::WasmType) -> ir::Type {
    match ty {
        crate::WasmType::I32 => types::I32,
        crate::WasmType::I64 => types::I64,
        crate::WasmType::F32 => types::F32,
        crate::WasmType::F64 => types::F64,
        _ => unreachable!("compiled values are numeric scalars"),
    }
}

/// The type of the other view of the same bits.
fn counterpart(ty: crate::WasmType) -> crate::WasmType {
    match ty {
        crate::WasmType::I32 => crate::WasmType::F32,
        crate::WasmType::I64 => crate::WasmType::F64,
        crate::WasmType::F32 => crate::WasmType::I32,
        crate::WasmType::F64 => crate::WasmType::I64,
        _ => unreachable!("compiled values are numeric scalars"),
    }
}

/// An i32 integer operator is its i64 namesake: the i64 selectors extend the
/// shared declaration (`crate::wasm::integer`), so tags agree.
fn widen_binary(operator: I32BinaryOperator) -> I64BinaryOperator {
    I64BinaryOperator::from_tag(operator as u32).expect("i64 selectors extend i32's")
}

fn widen_unary(operator: I32UnaryOperator) -> I64UnaryOperator {
    I64UnaryOperator::from_tag(operator as u32).expect("i64 selectors extend i32's")
}

/// A float operator and its operand type; both widths share one declaration
/// (`crate::wasm::float`), so tags agree.
fn float_binary_operator(op: Op, tag: u32) -> Option<(F64BinaryOperator, crate::WasmType)> {
    match op {
        Op::WasmF32Binary => Some((
            F64BinaryOperator::from_tag(F32BinaryOperator::from_tag(tag)? as u32)?,
            crate::WasmType::F32,
        )),
        Op::WasmF64Binary => Some((F64BinaryOperator::from_tag(tag)?, crate::WasmType::F64)),
        _ => None,
    }
}

fn float_unary_operator(op: Op, tag: u32) -> Option<(F64UnaryOperator, crate::WasmType)> {
    match op {
        Op::WasmF32Unary => Some((
            F64UnaryOperator::from_tag(F32UnaryOperator::from_tag(tag)? as u32)?,
            crate::WasmType::F32,
        )),
        Op::WasmF64Unary => Some((F64UnaryOperator::from_tag(tag)?, crate::WasmType::F64)),
        _ => None,
    }
}

/// What a register holds at an instruction, as the analysis derives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slot {
    Value(Kind),
    /// An instance binding: the environment slot it was loaded from.
    Binding(u32),
    /// The function's memory binding; only direct accesses read it.
    Memory,
    /// A reference to imported function `index`; only a call reads it.
    Import(u32),
    /// An indirect table call, with its table binding and expected type.
    IndirectReference {
        signature: u32,
        table: u32,
    },
    /// Paths disagree; no compiled instruction may read it.
    Conflict,
}

impl Slot {
    const DEFAULT: Self = Self::Value(Kind::I32);

    fn join(self, other: Self) -> Self {
        if self == other { self } else { Self::Conflict }
    }
}

/// Register facts at one bytecode position. Most Wasm temporaries have the
/// default i32 representation, so retaining a full `registers`-sized vector
/// at every reachable instruction wastes memory. This stores only deviations
/// from that default; joins still apply the same `Slot` lattice to every
/// register because two different deviations meet at `Conflict`.
#[derive(Clone, Debug)]
struct AnalysisState {
    slots: Vec<(u16, Slot)>,
}

impl AnalysisState {
    fn new() -> Self {
        Self { slots: Vec::new() }
    }

    fn get(&self, register: u16, registers: u16) -> Option<Slot> {
        if register >= registers {
            return None;
        }
        Some(
            self.slots
                .binary_search_by_key(&register, |(index, _)| *index)
                .map(|index| self.slots[index].1)
                .unwrap_or(Slot::DEFAULT),
        )
    }

    fn set(&mut self, register: u16, registers: u16, slot: Slot) -> Option<()> {
        if register >= registers {
            return None;
        }
        match self
            .slots
            .binary_search_by_key(&register, |(index, _)| *index)
        {
            Ok(index) if slot == Slot::DEFAULT => {
                self.slots.remove(index);
            }
            Ok(index) => self.slots[index].1 = slot,
            Err(index) if slot != Slot::DEFAULT => {
                self.slots.insert(index, (register, slot));
            }
            Err(_) => {}
        }
        Some(())
    }

    fn join_from(&mut self, incoming: &Self) -> bool {
        let mut joined = Vec::with_capacity(self.slots.len() + incoming.slots.len());
        let (mut left, mut right) = (0, 0);
        while left < self.slots.len() || right < incoming.slots.len() {
            let left_index = self.slots.get(left).map(|(index, _)| *index);
            let right_index = incoming.slots.get(right).map(|(index, _)| *index);
            let register = match (left_index, right_index) {
                (Some(a), Some(b)) => a.min(b),
                (Some(a), None) => a,
                (None, Some(b)) => b,
                (None, None) => unreachable!(),
            };
            let current = if left_index == Some(register) {
                let slot = self.slots[left].1;
                left += 1;
                slot
            } else {
                Slot::DEFAULT
            };
            let other = if right_index == Some(register) {
                let slot = incoming.slots[right].1;
                right += 1;
                slot
            } else {
                Slot::DEFAULT
            };
            let slot = current.join(other);
            if slot != Slot::DEFAULT {
                joined.push((register, slot));
            }
        }
        let changed = joined != self.slots;
        self.slots = joined;
        changed
    }
}

/// Traps compiled code can raise, by status (index + 1); status 0 is a
/// normal return.
const TRAPS: [crate::WasmTrap; 6] = [
    crate::WasmTrap::Unreachable,
    crate::WasmTrap::OutOfBoundsMemory,
    crate::WasmTrap::IntegerDivideByZero,
    crate::WasmTrap::IntegerOverflow,
    crate::WasmTrap::CallStackExhausted,
    crate::WasmTrap::InvalidConversionToInteger,
];

const JIT_ROOT_ENV: &str = "QUENCH_WASM_JIT_ROOT";
const JIT_ROOT_OPT_LEVEL_ENV: &str = "QUENCH_WASM_JIT_ROOT_OPT_LEVEL";
const JIT_WASM_SELECTIVE_ENV: &str = "QUENCH_WASM_JIT_SELECTIVE";
const MAX_JIT_ROOT_FUNCTIONS: usize = 128;
const MAX_JIT_ROOT_BYTECODES_PER_FUNCTION: usize = 80_000;
const MAX_JIT_ROOT_CODE_BYTES: usize = 2 * 1024 * 1024;
pub(super) const MAX_JIT_TIER_BYTECODES_PER_FUNCTION: usize = 2_048;

fn jit_root_override() -> Result<Option<usize>, ()> {
    let Some(root) = std::env::var_os(JIT_ROOT_ENV) else {
        return Ok(None);
    };
    root.to_str()
        .ok_or(())?
        .parse::<usize>()
        .map(Some)
        .map_err(|_| ())
}

fn jit_wasm_selective_enabled() -> bool {
    std::env::var(JIT_WASM_SELECTIVE_ENV).is_ok_and(|value| matches!(value.as_str(), "1" | "true"))
}

fn jit_root_opt_level(root: Option<usize>) -> Result<&'static str, ()> {
    if root.is_none() {
        return Ok("speed");
    }
    let Some(level) = std::env::var_os(JIT_ROOT_OPT_LEVEL_ENV) else {
        return Ok("speed");
    };
    match level.to_str().ok_or(())? {
        "none" => Ok("none"),
        "speed" => Ok("speed"),
        "speed_and_size" => Ok("speed_and_size"),
        _ => Err(()),
    }
}

/// Linux process RSS and high-water mark, in KiB, for opt-in JIT diagnostics.
#[cfg(target_os = "linux")]
fn jit_memory_snapshot() -> Option<(u64, u64)> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let mut rss = None;
    let mut high_water = None;
    for line in status.lines() {
        if let Some(value) = line.strip_prefix("VmRSS:") {
            rss = value.split_whitespace().next()?.parse().ok();
        } else if let Some(value) = line.strip_prefix("VmHWM:") {
            high_water = value.split_whitespace().next()?.parse().ok();
        }
        if rss.is_some() && high_water.is_some() {
            break;
        }
    }
    Some((rss?, high_water?))
}

#[cfg(not(target_os = "linux"))]
fn jit_memory_snapshot() -> Option<(u64, u64)> {
    None
}

/// The status of a callout that raised: the VM's error (or panic) waits in
/// the context.
const RAISED_STATUS: i64 = TRAPS.len() as i64 + 1;
const TERMINATED_STATUS: i64 = RAISED_STATUS + 1;

fn trap_status(trap: crate::WasmTrap) -> i64 {
    TRAPS
        .iter()
        .position(|candidate| *candidate == trap)
        .map(|index| index as i64 + 1)
        .expect("compiled code raises only listed traps")
}

/// Stack compiled code may use below its entry before a call traps as
/// exhausted; the interpreter's own guards bound the remainder.
const COMPILED_STACK_BUDGET: usize = 512 * 1024;

/// The state every compiled function receives. Field offsets are the ABI
/// compiled code loads from.
#[repr(C)]
struct CallContext {
    stack_limit: usize,
    interrupt: *const std::sync::atomic::AtomicBool,
    table: *const usize,
    vm: *mut (),
    env: Value,
    /// The entry trampoline's stack pointer after it saved the caller's
    /// registers; a guard fault resumes there.
    entry_stack: usize,
    /// The guarded memory reservation compiled code may fault in.
    guard_start: usize,
    guard_end: usize,
    /// Per global the program accesses (`CompiledProgram::globals`), the
    /// address of the global's value in this instance. Heap cells never move,
    /// so the addresses outlive callouts.
    globals: *const *mut Value,
    /// `import_callout` for the VM's host type.
    callout: usize,
    /// VM helper for indirect calls, including table checks and invocation.
    call_reference: usize,
    /// The artifact containing the currently executing call site. The active
    /// entry (or its caller) keeps this artifact strongly owned for the callout.
    compiled: *const CompiledProgram,
    // Fields below are the VM's, never read by compiled code.
    program: ProgramId,
    /// Number of Wasm-to-host callouts made by this native invocation.
    import_callouts: *mut u64,
    /// Number of direct known calls that had to use the VM fallback.
    known_callouts: *mut u64,
    /// Number of Wasm indirect-reference callouts made by this native invocation.
    reference_callouts: *mut u64,
    /// The bound linear memory whose base and size this call uses.
    memory: Value,
    /// Its exact current byte length, refreshed when a callout may grow it.
    memory_length: u64,
    /// What a callout that returned `RAISED_STATUS` raised.
    raised: Option<Raised>,
    // Diagnostic-only ABI fields are appended so existing repr(C) offsets
    // used by generated code stay fixed.
    call_reference_diagnostic: usize,
    reference_call_sites: *const ReferenceCallSiteDiagnostics,
    reference_call_site_count: usize,
    /// The VM's poll page (`ExecutionInterrupt::poll_page`), whose fault ends the call as
    /// terminated; empty when compiled code checks the flag instead.
    poll_start: usize,
    poll_end: usize,
}

impl CallContext {
    /// A nested native entry shares the caller's instance and callout state,
    /// but needs its own stack budget, guard-resume stack pointer and raised
    /// completion, as a nested `run_compiled` invocation would.
    fn nested_entry(&self) -> Self {
        let marker = 0u8;
        let stack_pointer = std::hint::black_box(&marker as *const u8 as usize);
        Self {
            stack_limit: stack_pointer.saturating_sub(COMPILED_STACK_BUDGET),
            interrupt: self.interrupt,
            table: self.table,
            vm: self.vm,
            env: self.env,
            entry_stack: 0,
            guard_start: self.guard_start,
            guard_end: self.guard_end,
            globals: self.globals,
            callout: self.callout,
            call_reference: self.call_reference,
            compiled: self.compiled,
            program: self.program,
            import_callouts: self.import_callouts,
            known_callouts: self.known_callouts,
            reference_callouts: self.reference_callouts,
            memory: self.memory,
            memory_length: self.memory_length,
            raised: None,
            call_reference_diagnostic: self.call_reference_diagnostic,
            reference_call_sites: self.reference_call_sites,
            reference_call_site_count: self.reference_call_site_count,
            poll_start: self.poll_start,
            poll_end: self.poll_end,
        }
    }
}

/// An error, or a panic to resume once compiled frames are gone.
enum Raised {
    Error(JsError),
    Panic(Box<dyn std::any::Any + Send>),
}

const CONTEXT_INTERRUPT: i32 = std::mem::offset_of!(CallContext, interrupt) as i32;
const CONTEXT_STACK_LIMIT: i32 = std::mem::offset_of!(CallContext, stack_limit) as i32;
const CONTEXT_TABLE: i32 = std::mem::offset_of!(CallContext, table) as i32;
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
const CONTEXT_ENTRY_STACK: u8 = std::mem::offset_of!(CallContext, entry_stack) as u8;
const CONTEXT_GLOBALS: i32 = std::mem::offset_of!(CallContext, globals) as i32;
const CONTEXT_CALLOUT: i32 = std::mem::offset_of!(CallContext, callout) as i32;
const CONTEXT_CALL_REFERENCE: i32 = std::mem::offset_of!(CallContext, call_reference) as i32;
const CONTEXT_MEMORY_LENGTH: i32 = std::mem::offset_of!(CallContext, memory_length) as i32;
const CONTEXT_CALL_REFERENCE_DIAGNOSTIC: i32 =
    std::mem::offset_of!(CallContext, call_reference_diagnostic) as i32;

/// The callout's signature: (context, function index, arguments, result) ->
/// status; arguments and result are Wasm bits, one word each.
type Callout = unsafe extern "C" fn(*mut CallContext, u64, *const u64, *mut u64) -> i64;
type ReferenceCallout =
    unsafe extern "C" fn(*mut CallContext, u64, u64, u64, u64, *const u64, *mut u64) -> i64;
type DiagnosticReferenceCallout =
    unsafe extern "C" fn(*mut CallContext, u64, u64, u64, u64, *const u64, *mut u64) -> i64;

/// Fill an already bounds-checked range in the instance's unshared memory.
/// The compiler checks against the exact current length before calling this
/// leaf, and no VM call can move the guarded memory during the operation.
unsafe extern "C" fn compiled_memory_fill(address: u64, byte: u64, length: u64) -> i64 {
    // Wasm32 operands are zero-extended before this call; they fit every
    // supported host's address size.
    unsafe {
        std::ptr::write_bytes(address as usize as *mut u8, byte as u8, length as usize);
    }
    0
}

fn note_compiled_callout(counter: *mut u64) {
    if !counter.is_null() {
        // SAFETY: only diagnostic-enabled calls provide this pointer, and it
        // points to a counter owned by the suspended native invocation.
        unsafe {
            *counter = (*counter).saturating_add(1);
        }
    }
}

#[derive(Clone, Copy, Default)]
struct ReferenceCallSiteCounts {
    total: u64,
    native: u64,
    fallback: u64,
}

struct ReferenceCallSiteDiagnostics {
    function: usize,
    pc: usize,
    counts: std::cell::Cell<ReferenceCallSiteCounts>,
}

fn note_reference_call_site(context: &CallContext, site: u64, native: bool) {
    let Ok(site) = usize::try_from(site) else {
        return;
    };
    if context.reference_call_sites.is_null() || site >= context.reference_call_site_count {
        return;
    }
    // SAFETY: diagnostic call sites are a stable vector owned by the compiled
    // program for the full native invocation, and generated ids are assigned
    // from that vector at compile time.
    let counts = unsafe { &*context.reference_call_sites.add(site) };
    let mut updated = counts.counts.get();
    updated.total = updated.total.saturating_add(1);
    if native {
        updated.native = updated.native.saturating_add(1);
    } else {
        updated.fallback = updated.fallback.saturating_add(1);
    }
    counts.counts.set(updated);
}

fn refresh_memory_length<H: Host>(context: &mut CallContext, vm: &Vm<H>) {
    if context.memory != Value::UNDEFINED
        && let Some((_, length)) = super::dispatch_fast::unshared_memory(vm, context.memory)
    {
        context.memory_length = length as u64;
    }
}

fn finish_callout(
    context: &mut CallContext,
    called: Result<Result<(), JsError>, Box<dyn std::any::Any + Send>>,
) -> i64 {
    match called {
        Ok(Ok(())) => 0,
        Ok(Err(error)) => {
            context.raised = Some(Raised::Error(error));
            RAISED_STATUS
        }
        Err(panic) => {
            context.raised = Some(Raised::Panic(panic));
            RAISED_STATUS
        }
    }
}

/// Resolve, check, and invoke an indirect table target in one VM callout.
unsafe extern "C" fn reference_callout<H: Host>(
    context: *mut CallContext,
    table_slot: u64,
    index: u64,
    signature_index: u64,
    site: u64,
    args: *const u64,
    out: *mut u64,
) -> i64 {
    // A stable call-site id is present even without diagnostics so the VM can
    // find the per-site monomorphic cache without a hash lookup.
    unsafe {
        reference_callout_inner::<H, false>(
            context,
            table_slot,
            index,
            signature_index,
            site,
            args,
            out,
        )
    }
}

/// Diagnostic-only reference callout. Its extra site id is emitted only when
/// Wasm JIT diagnostics were enabled at compilation time.
unsafe extern "C" fn diagnostic_reference_callout<H: Host>(
    context: *mut CallContext,
    table_slot: u64,
    index: u64,
    signature_index: u64,
    site: u64,
    args: *const u64,
    out: *mut u64,
) -> i64 {
    unsafe {
        reference_callout_inner::<H, true>(
            context,
            table_slot,
            index,
            signature_index,
            site,
            args,
            out,
        )
    }
}

/// Shared implementation, monomorphized so ordinary callouts have no
/// diagnostic counter branch or extra site lookup.
#[inline(always)]
unsafe fn reference_callout_inner<H: Host, const DIAGNOSTICS: bool>(
    context: *mut CallContext,
    table_slot: u64,
    index: u64,
    signature_index: u64,
    site: u64,
    args: *const u64,
    out: *mut u64,
) -> i64 {
    // SAFETY: generated code passes the context, table binding, table index,
    // signature index, argument words, and output storage for this call.
    let context = unsafe { &mut *context };
    note_compiled_callout(context.reference_callouts);
    let vm_pointer = context.vm.cast::<Vm<H>>();
    let active = ACTIVE_CONTEXT.with(|current| current.replace(std::ptr::null_mut()));
    let program = context.program;
    let mut native_target_hit = false;
    let mut direct_status = None;
    let called = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let (
            callee,
            params,
            result_type,
            same_program_function,
            direct_entry,
            cache_hit,
            table,
            table_slot,
            signature_index,
        ) = {
            // Do all heap/program inspection before entering native code. The
            // VM is re-entered through callouts, so no `&mut Vm` may live
            // across that boundary.
            let vm = unsafe { &mut *vm_pointer };
            let table_slot = u32::try_from(table_slot)
                .map_err(|_| JsError::validation("invalid Wasm table binding".into()))?;
            let signature_index = u32::try_from(signature_index)
                .map_err(|_| JsError::validation("invalid Wasm signature index".into()))?;
            let table = vm
                .heap
                .environment_slot(context.env, table_slot as usize)
                .ok_or_else(|| JsError::validation("invalid Wasm table binding".into()))?;
            let cached = usize::try_from(site).ok().and_then(|site| {
                cached_direct_reference(
                    vm,
                    context,
                    table_slot,
                    table,
                    index,
                    signature_index,
                    site,
                )
            });
            if let Some((callee, direct_entry)) = cached {
                (
                    callee,
                    smallvec::SmallVec::new(),
                    None,
                    None,
                    Some(direct_entry),
                    true,
                    table,
                    table_slot,
                    signature_index,
                )
            } else {
                let callee = vm.wasm_indirect_target(program, table, index, signature_index)?;
                let (params, result_type) = {
                    let signature = vm
                        .programs
                        .wasm_signatures(program)
                        .and_then(|signatures| signatures.type_signature(signature_index))
                        .ok_or_else(|| {
                            JsError::validation("invalid Wasm signature index".into())
                        })?;
                    (
                        signature
                            .params
                            .iter()
                            .copied()
                            .collect::<smallvec::SmallVec<[crate::WasmType; 8]>>(),
                        signature.results.first().copied(),
                    )
                };
                let same_program_function = match vm.heap.get(callee) {
                    Some(Cell::Function {
                        kind: crate::heap::FunctionKind::User(target_program, function),
                        env,
                        ..
                    }) if *target_program == program => Some((*target_program, *function, *env)),
                    _ => None,
                };
                let direct_entry = same_program_function
                    .filter(|(target_program, _, target_env)| {
                        *target_program == program && *target_env == context.env
                    })
                    .and_then(|(_, function, _)| {
                        let compiled = vm
                            .programs
                            .compiled(program)
                            .flatten()
                            .filter(|compiled| compiled.has_function(function as usize))
                            .or_else(|| vm.programs.wasm_tier_containing(program, function))?;
                        direct_reference_entry(
                            vm,
                            compiled,
                            function as usize,
                            context.env,
                            context.memory,
                            Some((&params, result_type)),
                        )
                    });
                (
                    callee,
                    params,
                    result_type,
                    same_program_function,
                    direct_entry,
                    false,
                    table,
                    table_slot,
                    signature_index,
                )
            }
        };
        if let Some((compiled, function, entry, (base, length), memory_binding, guard, globals)) =
            direct_entry
        {
            if !cache_hit
                && let Ok(site) = usize::try_from(site)
                && !context.compiled.is_null()
            {
                let target_weak = unsafe { &*vm_pointer }.heap.weak_handle(callee);
                if let Some(target_weak) = target_weak {
                    let caller = unsafe { &*context.compiled };
                    caller.cache_indirect_call(
                        site,
                        IndirectCallCacheEntry {
                            table_slot,
                            table,
                            index,
                            signature_index,
                            target: callee,
                            target_weak,
                            program,
                            function: function as u32,
                            env: context.env,
                            compiled: Rc::downgrade(&compiled),
                        },
                    );
                }
            }
            compiled.note_direct_reference_entry(function);
            let mut nested = context.nested_entry();
            nested.compiled = Rc::as_ptr(&compiled);
            nested.table = compiled.table.as_ptr();
            nested.globals = globals.as_ptr();
            nested.memory = memory_binding;
            nested.memory_length = length;
            let (guard_start, guard_end) = guard.map_or((0, 0), |(start, size)| {
                (start as usize, start as usize + size)
            });
            nested.guard_start = guard_start;
            nested.guard_end = guard_end;
            nested.call_reference_diagnostic = compiled.diagnostics.as_ref().map_or(0, |_| {
                diagnostic_reference_callout::<H> as DiagnosticReferenceCallout as usize
            });
            nested.reference_call_sites = compiled
                .diagnostics
                .as_ref()
                .map_or(std::ptr::null(), |stats| {
                    stats.reference_call_sites.as_ptr()
                });
            nested.reference_call_site_count = compiled
                .diagnostics
                .as_ref()
                .map_or(0, |stats| stats.reference_call_sites.len());
            // SAFETY: the table/signature resolver ran first; same program,
            // exact instance environment, and exact scalar signature were
            // checked above. The caller's raw argument/result buffers have
            // the Wasm ABI shape expected by this compiled entry, and `compiled`
            // keeps its executable mapping alive for the duration of the call.
            native_target_hit = true;
            let status = match guard_trampoline() {
                Some(trampoline) => {
                    let outer = ACTIVE_CONTEXT.with(|active| active.replace(&mut nested));
                    let status = unsafe { trampoline(&mut nested, base, length, args, out, entry) };
                    ACTIVE_CONTEXT.with(|active| active.set(outer));
                    status
                }
                None => unsafe { entry(&mut nested, base, length, args, out) },
            };
            if status == RAISED_STATUS as u32 {
                match nested.raised.take() {
                    Some(Raised::Error(error)) => return Err(error),
                    Some(Raised::Panic(panic)) => std::panic::resume_unwind(panic),
                    None => {
                        return Err(JsError::validation(
                            "compiled indirect callee raised without an error".into(),
                        ));
                    }
                }
            }
            direct_status = Some(status);
            return Ok(());
        }

        let vm = unsafe { &mut *vm_pointer };
        let mut values: smallvec::SmallVec<[Value; 8]> = smallvec::SmallVec::new();
        for (offset, ty) in params.iter().enumerate() {
            let kind = Kind::of(*ty)
                .ok_or_else(|| JsError::validation("unsupported Wasm call parameter".into()))?;
            // SAFETY: the generated call frame stores one word per parameter.
            let bits = unsafe { *args.add(offset) };
            values.push(vm.encode_wasm_value(crate::WasmValue::from_bits(
                *ty,
                match kind {
                    Kind::I32 => u64::from(bits as u32),
                    Kind::I64 => bits,
                },
            )));
        }
        let compiled_result = same_program_function.and_then(|(target_program, function, env)| {
            vm.run_compiled(target_program, env, function, &values)
        });
        native_target_hit = compiled_result.is_some();
        let result = match compiled_result {
            Some(result) => result?,
            None => {
                let code = vm
                    .programs
                    .get(program)
                    .ok_or_else(|| JsError::validation("missing Wasm program".into()))?;
                let receiver = vm
                    .programs
                    .constant(program, VOID_RESULT_CONSTANT)
                    .ok_or_else(|| JsError::validation("missing Wasm void constant".into()))?;
                vm.call_value(&code, callee, receiver, &values)?
            }
        };
        let bits = match result_type {
            Some(ty) => match vm
                .decode_wasm_value(result, ty)?
                .bits()
                .ok_or_else(|| JsError::validation("non-scalar Wasm call result".into()))?
            {
                crate::wasm::ScalarBits::Bits32(bits) => u64::from(bits),
                crate::wasm::ScalarBits::Bits64(bits) => bits,
                crate::wasm::ScalarBits::Bits128(_) => {
                    return Err(JsError::validation("non-scalar Wasm call result".into()));
                }
            },
            None => 0,
        };
        // SAFETY: `out` points into the result word emitted for this helper.
        unsafe { *out = bits };
        Ok(())
    }));
    ACTIVE_CONTEXT.with(|current| current.set(active));
    if DIAGNOSTICS {
        note_reference_call_site(context, site, native_target_hit);
    }
    let vm = unsafe { &mut *vm_pointer };
    refresh_memory_length(context, vm);
    direct_status.map_or_else(|| finish_callout(context, called), i64::from)
}

/// Make imported call `function` for compiled code, as the interpreter's
/// `Op::Call` does.
unsafe extern "C" fn import_callout<H: Host>(
    context: *mut CallContext,
    function: u64,
    args: *const u64,
    out: *mut u64,
) -> i64 {
    // SAFETY: compiled code passes the context `run_compiled` made, whose
    // VM is suspended in that call.
    let context = unsafe { &mut *context };
    let vm = unsafe { &mut *context.vm.cast::<Vm<H>>() };
    let defined = vm
        .programs
        .wasm_signatures(context.program)
        .and_then(|signatures| signatures.defined_index(function as u32))
        .is_some();
    note_compiled_callout(if defined {
        context.known_callouts
    } else {
        context.import_callouts
    });
    // The VM's own code runs outside compiled code: no guard fault is ours.
    let active = ACTIVE_CONTEXT.with(|current| current.replace(std::ptr::null_mut()));
    let program = context.program;
    let env = context.env;
    let called = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: compiled code passes one word per parameter and a result
        // word.
        unsafe { vm.call_import(program, env, function as u32, args, out) }
    }));
    ACTIVE_CONTEXT.with(|current| current.set(active));
    refresh_memory_length(context, vm);
    finish_callout(context, called)
}

/// A compiled function's entry: (context, memory base, memory length,
/// arguments, result) -> status.
type Entry = unsafe extern "C" fn(*mut CallContext, u64, u64, *const u64, *mut u64) -> u32;

enum CompiledArgs<'a> {
    Values(&'a [Value]),
    RegisterWindow {
        caller: usize,
        base: usize,
        count: usize,
    },
}

pub(super) struct CompiledFunction {
    entry: Entry,
    params: Vec<crate::WasmType>,
    result: Option<crate::WasmType>,
    #[cfg(test)]
    native_entries: std::cell::Cell<u64>,
    #[cfg(test)]
    direct_reference_entries: std::cell::Cell<u64>,
}

/// A program's compiled functions, sharing one code mapping and one call
/// table.
#[derive(Clone)]
struct IndirectCallCacheEntry {
    table_slot: u32,
    table: Value,
    index: u64,
    signature_index: u32,
    target: Value,
    target_weak: crate::heap::WeakHandle,
    program: ProgramId,
    function: u32,
    env: Value,
    compiled: Weak<CompiledProgram>,
}

type DirectReferenceEntry = (
    Rc<CompiledProgram>,
    usize,
    Entry,
    (u64, u64),
    Value,
    Option<(*mut u8, usize)>,
    smallvec::SmallVec<[*mut Value; 8]>,
);

/// Build the per-invocation activation for a same-instance compiled indirect
/// target. Even a monomorphic cache hit comes through here so memory, guards,
/// and mutable global addresses always reflect the current instance state.
fn direct_reference_entry<H: Host>(
    vm: &mut Vm<H>,
    compiled: Rc<CompiledProgram>,
    function: usize,
    env: Value,
    context_memory: Value,
    signature: Option<(&[crate::WasmType], Option<crate::WasmType>)>,
) -> Option<DirectReferenceEntry> {
    let native = compiled.functions.get(function)?.as_ref()?;
    if signature.is_some_and(|(params, result)| {
        native.params.as_slice() != params || native.result != result
    }) {
        return None;
    }
    let entry = native.entry;
    let (memory, memory_binding, guard) = match compiled.memory_slot {
        None => ((0, 0), Value::UNDEFINED, None),
        Some(slot) => {
            let binding = vm.heap.environment_slot(env, slot as usize)?;
            if binding != context_memory {
                return None;
            }
            let (base, length) = super::dispatch_fast::unshared_memory(vm, binding)?;
            let guard = guarded_memory(vm, binding);
            if compiled.guarded && guard.is_none()
                || compiled.wraps && !wrapping_memory(vm, binding)
                || compiled.fixed_base.is_some_and(|fixed| {
                    guard.map(|(base, _)| displacement_base(base)) != Some(Some(fixed))
                })
            {
                return None;
            }
            ((base as u64, length as u64), binding, guard)
        }
    };
    let mut globals: smallvec::SmallVec<[*mut Value; 8]> = smallvec::SmallVec::new();
    for slot in &compiled.globals {
        let binding = vm.heap.environment_slot(env, *slot as usize)?;
        match vm.heap.get_mut(binding)? {
            Cell::WasmGlobal { value, .. } => globals.push(value),
            _ => return None,
        }
    }
    Some((
        compiled,
        function,
        entry,
        memory,
        memory_binding,
        guard,
        globals,
    ))
}

/// Check the cache's guards without turning any miss into a Wasm trap. The
/// ordinary resolver below remains authoritative for OOB, null, and mismatch
/// cases, and for artifacts that cannot currently activate the cached target.
fn cached_direct_reference<H: Host>(
    vm: &mut Vm<H>,
    context: &CallContext,
    table_slot: u32,
    table: Value,
    index: u64,
    signature_index: u32,
    site: usize,
) -> Option<(Value, DirectReferenceEntry)> {
    if context.compiled.is_null() {
        return None;
    }
    let caller = unsafe { &*context.compiled };
    let cache = caller.indirect_call_cache(site)?;
    if cache.table_slot != table_slot
        || cache.table != table
        || cache.index != index
        || cache.signature_index != signature_index
        || cache.program != context.program
        || cache.env != context.env
    {
        return None;
    }
    let element = usize::try_from(index)
        .ok()
        .and_then(|index| vm.wasm_table_elements(table).ok()?.get(index).copied())?;
    if element != cache.target || vm.heap.weak_value(cache.target_weak) != Some(element) {
        return None;
    }
    match vm.heap.get(element) {
        Some(Cell::Function {
            kind: crate::heap::FunctionKind::User(program, function),
            env,
            ..
        }) if *program == cache.program && *function == cache.function && *env == cache.env => {}
        _ => return None,
    }
    let compiled = cache.compiled.upgrade()?;
    let direct = direct_reference_entry(
        vm,
        compiled,
        cache.function as usize,
        context.env,
        context.memory,
        None,
    )?;
    #[cfg(test)]
    caller
        .reference_cache_hits
        .set(caller.reference_cache_hits.get().saturating_add(1));
    Some((element, direct))
}

pub(super) struct CompiledProgram {
    _code: CodeMemory,
    table: Box<[usize]>,
    functions: Vec<Option<CompiledFunction>>,
    /// The environment slot of the memory compiled code accesses.
    memory_slot: Option<u32>,
    /// Memory accesses rely on the memory's guard reservation instead of
    /// bounds checks.
    guarded: bool,
    /// The guarded memory's base, when code addresses it as a constant
    /// displacement (see `displacement_base`); calls with another base fall
    /// back to the interpreter.
    fixed_base: Option<i32>,
    /// The guarded memory repeats itself above 2^32, so an i32 sum may be
    /// formed in 64 bits (see `WRAP_ALIAS`); calls with a memory that does
    /// not fall back to the interpreter.
    wraps: bool,
    /// The environment slots of the globals compiled code accesses, in the
    /// order of the context's global addresses.
    globals: Vec<u32>,
    diagnostics: Option<WasmJitDiagnostics>,
    /// One optional monomorphic cache per actual compiled `call_indirect`
    /// site. The vector is only pointers until a site becomes hot, and each
    /// cache weakly references its callee artifact to avoid Rc cycles.
    reference_call_caches: std::cell::RefCell<Vec<Option<Box<IndirectCallCacheEntry>>>>,
    #[cfg(test)]
    reference_cache_hits: std::cell::Cell<u64>,
}

/// Counters allocated only for opt-in benchmark diagnostics. These runs are
/// intended to explain coverage; their timings are not qualification scores.
struct WasmJitDiagnostics {
    program: u32,
    total_functions: usize,
    compiled_functions: usize,
    code_bytes: usize,
    mapped_bytes: usize,
    compile_ns: u128,
    native_entry_requests: Vec<std::cell::Cell<u64>>,
    interpreter_fallback_entries: Vec<std::cell::Cell<u64>>,
    compiled_to_vm_callouts: std::cell::Cell<u64>,
    compiled_import_callouts: std::cell::Cell<u64>,
    compiled_known_callouts: std::cell::Cell<u64>,
    compiled_reference_callouts: std::cell::Cell<u64>,
    reference_call_sites: Vec<ReferenceCallSiteDiagnostics>,
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
        eprintln!(
            "QUENCH_WASM_JIT_STATS {{\"program\":{},\"compile_attempts\":1,\"compiled_functions\":{},\"total_functions\":{},\"code_bytes\":{},\"mapped_bytes\":{},\"compile_ns\":{},\"native_entry_requests\":[{}],\"interpreter_fallback_entries\":[{}],\"compiled_to_vm_callouts\":{},\"compiled_import_callouts\":{},\"compiled_known_callouts\":{},\"compiled_reference_callouts\":{},\"reference_call_sites\":[{}]}}",
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
            reference_sites,
        );
    }
}

fn jit_diagnostics_enabled() -> bool {
    std::env::var_os("QUENCH_WASM_JIT_STATS").is_some_and(|value| value == "1")
}

impl CompiledProgram {
    fn indirect_call_cache(&self, site: usize) -> Option<IndirectCallCacheEntry> {
        self.reference_call_caches
            .borrow()
            .get(site)?
            .as_deref()
            .cloned()
    }

    fn cache_indirect_call(&self, site: usize, entry: IndirectCallCacheEntry) {
        if let Some(slot) = self.reference_call_caches.borrow_mut().get_mut(site) {
            *slot = Some(Box::new(entry));
        }
    }

    pub(super) fn clear_indirect_call_caches(&self) {
        self.reference_call_caches
            .borrow_mut()
            .iter_mut()
            .for_each(|slot| *slot = None);
    }

    #[cfg(test)]
    pub(super) fn reference_call_cache_target(&self, site: usize) -> Option<Value> {
        self.indirect_call_cache(site).map(|entry| entry.target)
    }

    #[cfg(test)]
    pub(super) fn reference_cache_hits(&self) -> u64 {
        self.reference_cache_hits.get()
    }

    fn note_direct_reference_entry(&self, function: usize) {
        self.note_native_entry(function);
        #[cfg(test)]
        if let Some(Some(function)) = self.functions.get(function) {
            function
                .direct_reference_entries
                .set(function.direct_reference_entries.get().saturating_add(1));
        }
    }

    fn note_native_entry(&self, function: usize) {
        if let Some(stats) = &self.diagnostics
            && let Some(entries) = stats.native_entry_requests.get(function)
        {
            entries.set(entries.get().saturating_add(1));
        }
        #[cfg(test)]
        if let Some(Some(function)) = self.functions.get(function) {
            function
                .native_entries
                .set(function.native_entries.get().saturating_add(1));
        }
    }

    pub(super) fn has_function(&self, function: usize) -> bool {
        self.functions.get(function).is_some_and(Option::is_some)
    }

    pub(super) fn mapped_bytes(&self) -> usize {
        self._code.length
    }

    #[cfg(test)]
    pub(super) fn reference_call_site_counts(&self) -> Option<Vec<(usize, usize, u64, u64, u64)>> {
        Some(
            self.diagnostics
                .as_ref()?
                .reference_call_sites
                .iter()
                .map(|site| {
                    let counts = site.counts.get();
                    (
                        site.function,
                        site.pc,
                        counts.total,
                        counts.native,
                        counts.fallback,
                    )
                })
                .collect(),
        )
    }

    pub(super) fn has_function_or_note_fallback(&self, function: usize) -> bool {
        let available = self.has_function(function);
        if !available
            && let Some(stats) = &self.diagnostics
            && let Some(fallbacks) = stats.interpreter_fallback_entries.get(function)
        {
            fallbacks.set(fallbacks.get().saturating_add(1));
        }
        available
    }

    #[cfg(test)]
    pub(super) fn native_entries(&self, function: usize) -> Option<u64> {
        self.functions
            .get(function)?
            .as_ref()
            .map(|function| function.native_entries.get())
    }

    #[cfg(test)]
    pub(super) fn direct_reference_entries(&self, function: usize) -> Option<u64> {
        self.functions
            .get(function)?
            .as_ref()
            .map(|function| function.direct_reference_entries.get())
    }
}

/// Executable memory holding a program's machine code.
struct CodeMemory {
    pointer: *mut u8,
    length: usize,
}

impl CodeMemory {
    fn new(bytes: &[u8]) -> Option<Self> {
        let length = bytes.len().max(1);
        // `mmap` rounds to whole pages. Store that rounded length explicitly
        // so the JS tier's budget reflects its executable mappings.
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        let page_size = usize::try_from(page_size).ok()?;
        let length = length
            .checked_add(page_size.checked_sub(1)?)?
            .checked_div(page_size)?
            .checked_mul(page_size)?;
        // SAFETY: an anonymous private mapping; the result is checked.
        let pointer = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                length,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if pointer == libc::MAP_FAILED {
            return None;
        }
        let pointer = pointer.cast::<u8>();
        // SAFETY: the mapping is `length` writable bytes.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer, bytes.len()) };
        // SAFETY: the same mapping, now read-only and executable.
        if unsafe { libc::mprotect(pointer.cast(), length, libc::PROT_READ | libc::PROT_EXEC) } != 0
        {
            // SAFETY: unmapping the mapping created above.
            unsafe { libc::munmap(pointer.cast(), length) };
            return None;
        }
        Some(Self { pointer, length })
    }
}

impl Drop for CodeMemory {
    fn drop(&mut self) {
        // SAFETY: the mapping this value owns.
        unsafe { libc::munmap(self.pointer.cast(), self.length) };
    }
}

/// A guarded JavaScript numeric function compiled from a narrow residual
/// bytecode subset. It owns one W^X executable mapping and has no heap roots.
pub(super) struct CompiledJsFunction {
    _code: CodeMemory,
    entry: CompiledJsEntry,
    params: usize,
    mapped_bytes: usize,
    return_constants: Vec<(u32, u32)>,
    #[cfg(test)]
    native_entries: std::cell::Cell<u64>,
}

#[derive(Clone, Copy)]
enum CompiledJsEntry {
    Number(unsafe extern "C" fn(*const f64) -> f64),
    StringReturnPc(unsafe extern "C" fn(*const f64) -> u32),
}

pub(super) enum CompiledJsResult {
    Number(f64),
    StringReturnPc(u32),
}

impl CompiledJsFunction {
    pub(super) fn from_artifact(artifact: super::js_compiled::JsJitArtifact) -> Option<Self> {
        let code = CodeMemory::new(&artifact.code)?;
        let mapped_bytes = code.length;
        let (entry, return_constants) = match artifact.result {
            super::js_compiled::JsJitResultKind::Number => {
                // SAFETY: the numeric JS compiler emits `fn(*const f64) -> f64`.
                let entry = unsafe {
                    std::mem::transmute::<*mut u8, unsafe extern "C" fn(*const f64) -> f64>(
                        code.pointer,
                    )
                };
                (CompiledJsEntry::Number(entry), Vec::new())
            }
            super::js_compiled::JsJitResultKind::StringReturnPc(return_constants) => {
                // SAFETY: the static-string compiler emits `fn(*const f64) -> u32`.
                let entry = unsafe {
                    std::mem::transmute::<*mut u8, unsafe extern "C" fn(*const f64) -> u32>(
                        code.pointer,
                    )
                };
                (CompiledJsEntry::StringReturnPc(entry), return_constants)
            }
        };
        Some(Self {
            _code: code,
            entry,
            params: artifact.params,
            mapped_bytes,
            return_constants,
            #[cfg(test)]
            native_entries: std::cell::Cell::new(0),
        })
    }

    pub(super) fn params(&self) -> usize {
        self.params
    }

    pub(super) fn call(&self, arguments: &[f64]) -> CompiledJsResult {
        #[cfg(test)]
        self.native_entries
            .set(self.native_entries.get().saturating_add(1));
        // SAFETY: the artifact result kind selects the matching ABI, and
        // `arguments` has the formal count encoded into the native function.
        unsafe {
            match self.entry {
                CompiledJsEntry::Number(entry) => {
                    CompiledJsResult::Number(entry(arguments.as_ptr()))
                }
                CompiledJsEntry::StringReturnPc(entry) => {
                    CompiledJsResult::StringReturnPc(entry(arguments.as_ptr()))
                }
            }
        }
    }

    pub(super) fn string_constant_for_return(&self, return_pc: u32) -> Option<u32> {
        self.return_constants
            .iter()
            .find_map(|(pc, constant)| (*pc == return_pc).then_some(*constant))
    }

    #[cfg(test)]
    pub(super) fn has_string_returns(&self) -> bool {
        !self.return_constants.is_empty()
    }

    #[cfg(test)]
    pub(super) fn native_entries(&self) -> u64 {
        self.native_entries.get()
    }

    pub(super) fn mapped_bytes(&self) -> usize {
        self.mapped_bytes
    }
}

/// A function's derived compilation facts.
struct Plan {
    params: Vec<crate::WasmType>,
    result: Option<crate::WasmType>,
    /// Per instruction, `None` when unreachable, else what its `b` register
    /// holds on entry: the one analysis fact a denotation reads (a moved or
    /// selected value's kind, a global's binding, an import reference). The
    /// full register states exist only while planning.
    operands: Vec<Option<Slot>>,
}

impl Plan {
    /// The plan as its callers see it: only the function being compiled
    /// holds its per-instruction facts.
    fn interface(self) -> Self {
        Self {
            operands: Vec::new(),
            ..self
        }
    }

    fn reachable(&self, pc: usize) -> bool {
        self.operands[pc].is_some()
    }

    /// The analysis fact for the `b` register of reachable instruction `pc`.
    fn operand(&self, pc: usize) -> Slot {
        self.operands[pc].expect("reachable")
    }
}

/// The register a direct access of `instruction` reads its memory through.
fn direct_access(op: Op) -> bool {
    MemoryLoad::from_direct_op(op).is_some() || MemoryStore::from_direct_op(op).is_some()
}

fn successors(function: &Function, pc: usize, instruction: WideInstruction) -> Vec<usize> {
    match instruction.op().control_flow_layout() {
        ControlFlowLayout::Jump => vec![instruction.jump_target() as usize],
        ControlFlowLayout::ConditionalJump => vec![instruction.jump_target() as usize, pc + 1],
        ControlFlowLayout::BranchTable => crate::bytecode::branch_table_entries(pc, instruction)
            .filter(|entry| *entry < function.code.len())
            .collect(),
        ControlFlowLayout::Terminal => vec![],
        ControlFlowLayout::Call | ControlFlowLayout::Fallthrough => {
            if instruction.returns_from_frame() {
                vec![]
            } else {
                vec![pc + 1]
            }
        }
    }
}

/// The analysis context shared by every function of a program.
struct Analysis<'a, H: Host> {
    vm: &'a Vm<H>,
    program: ProgramId,
    code: &'a ResidualProgram,
    env: Value,
    /// Whether imported calls may compile: a callee may grow memory, which
    /// moves an unguarded memory's bytes.
    callouts: bool,
    /// `unreachable` is enabled only for a bounded, explicitly selected root
    /// experiment until its compile-time memory cost is measured.
    experimental_root_compilation: bool,
    /// Rejection reasons exist only for explicit diagnostic runs.
    compile_rejections: Option<&'a std::cell::RefCell<Vec<Option<String>>>>,
}

impl<H: Host> Analysis<'_, H> {
    fn reject(&self, index: usize, reason: impl FnOnce() -> String) {
        let Some(rejections) = self.compile_rejections else {
            return;
        };
        let mut rejections = rejections.borrow_mut();
        if let Some(rejection) = rejections.get_mut(index)
            && rejection.is_none()
        {
            let reason = reason();
            *rejection = Some(reason.clone());
            eprintln!("QUENCH_WASM_JIT_REJECTION {index} {reason}");
        }
    }

    fn constant(&self, index: usize) -> Option<Value> {
        self.vm.programs.constant(self.program, index)
    }

    /// The machine kind of a constant: i32-encoded scalars, boxed 64-bit
    /// scalars, and any other constant (the void result) as an inert i32.
    fn constant_kind(&self, value: Value) -> Kind {
        match self.vm.decode_bits64(value) {
            Some(_) => Kind::I64,
            None => Kind::I32,
        }
    }

    fn constant_bits(&self, value: Value) -> i64 {
        match self.vm.decode_bits64(value) {
            Some(bits) => bits as i64,
            None => i64::from(value.wasm_bits32() as i32),
        }
    }

    /// The type of the global bound at environment slot `slot`.
    fn global_type(&self, slot: u32) -> Option<crate::WasmType> {
        let binding = self.vm.heap.environment_slot(self.env, slot as usize)?;
        match self.vm.heap.get(binding)? {
            Cell::WasmGlobal { ty, .. } => Some(*ty),
            _ => None,
        }
    }

    /// Whether `slot` binds an unshared memory compiled code can view.
    fn binds_unshared_memory(&self, slot: u32) -> bool {
        let Some(binding) = self.vm.heap.environment_slot(self.env, slot as usize) else {
            return false;
        };
        matches!(
            self.vm.heap.get(binding),
            Some(Cell::WasmMemory { bytes, .. })
                if matches!(**bytes, crate::wasm::memory::MemoryStorage::Unshared(_))
        )
    }

    /// Whether a bound memory has the representation supported by native
    /// linear-memory operations: unshared memory32 with ordinary Wasm pages.
    fn supports_compiled_memory(&self, slot: u32) -> bool {
        let Some(binding) = self.vm.heap.environment_slot(self.env, slot as usize) else {
            return false;
        };
        matches!(
            self.vm.heap.get(binding),
            Some(Cell::WasmMemory { bytes, header, .. })
                if matches!(**bytes, crate::wasm::memory::MemoryStorage::Unshared(_))
                    && !header.ty.memory64
                    && header.ty.page_size() as usize
                        == (1usize << crate::wasm::memory::WASM_PAGE_BITS)
        )
    }

    fn binds_table32(&self, slot: u32) -> bool {
        let Some(binding) = self.vm.heap.environment_slot(self.env, slot as usize) else {
            return false;
        };
        matches!(
            self.vm.heap.get(binding),
            Some(Cell::WasmTable { table64: false, .. })
        )
    }

    fn signature(&self, index: u32) -> Option<&crate::WasmSignature> {
        self.vm
            .programs
            .wasm_function_signature(self.program, index)
    }

    /// The signature of `function` when it is imported.
    fn import_signature(&self, function: u32) -> Option<&crate::WasmSignature> {
        let signatures = self.vm.programs.wasm_signatures(self.program)?;
        signatures
            .defined_index(function)
            .is_none()
            .then(|| signatures.get(function as usize))?
    }

    fn type_signature(&self, index: u32) -> Option<&crate::WasmSignature> {
        self.vm
            .programs
            .wasm_signatures(self.program)?
            .type_signature(index)
    }

    /// The function's plan, assuming every function in `callable` compiles.
    fn plan(&self, index: usize, callable: &[bool], memory_slot: &mut Option<u32>) -> Option<Plan> {
        let plan_started = self
            .compile_rejections
            .as_ref()
            .map(|_| std::time::Instant::now());
        let memory_before = plan_started.and_then(|_| jit_memory_snapshot());
        let function = &self.code.functions[index];
        let Some(signature) = self.signature(index as u32) else {
            self.reject(index, || "missing-signature".into());
            return None;
        };
        if function.locals != 0 {
            self.reject(index, || format!("frame-locals:{}", function.locals));
            return None;
        }
        if signature.results.len() > 1 {
            self.reject(index, || {
                format!("multi-value-results:{}", signature.results.len())
            });
            return None;
        }
        let params = signature.params.clone();
        let result = signature.results.first().copied();
        if let Some(ty) = params
            .iter()
            .chain(result.iter())
            .find(|ty| Kind::of(**ty).is_none())
        {
            self.reject(index, || format!("unsupported-signature-type:{ty:?}"));
            return None;
        }
        let Some(parameter_base) = function.parameter_registers.map(usize::from) else {
            self.reject(index, || "missing-parameter-register-base".into());
            return None;
        };
        let registers = usize::from(function.registers);
        if parameter_base + params.len() > registers {
            self.reject(index, || "parameters-outside-register-window".into());
            return None;
        }
        let memory_register = super::dispatch_fast::memory_register(function);
        let instructions: Vec<WideInstruction> = function
            .code
            .iter()
            .map(|packed| super::dispatch_fast::decoded_instruction(function, *packed))
            .collect();
        let mut entry = AnalysisState::new();
        for (offset, ty) in params.iter().enumerate() {
            entry.set(
                u16::try_from(parameter_base + offset).ok()?,
                function.registers,
                Slot::Value(Kind::of(*ty)?),
            )?;
        }
        let mut states: Vec<Option<AnalysisState>> = vec![None; instructions.len()];
        states[0] = Some(entry);
        let mut work = vec![0usize];
        while let Some(pc) = work.pop() {
            let instruction = instructions[pc];
            let mut state = states[pc]
                .clone()
                .expect("queued instructions have a state");
            if instruction.op() == Op::CallKnown
                && !callable
                    .get(usize::from(instruction.known_function_index()))
                    .copied()
                    .unwrap_or(false)
                && !self.experimental_root_compilation
            {
                let callee = instruction.known_function_index();
                self.reject(index, || format!("uncompiled-callee:{callee}@{pc}"));
                return None;
            }
            if self
                .transfer(
                    function,
                    instruction,
                    &mut state,
                    callable,
                    memory_register,
                    memory_slot,
                    result,
                )
                .is_none()
            {
                let op = instruction.op();
                self.reject(index, || format!("unsupported-instruction:{op:?}@{pc}"));
                return None;
            }
            for successor in successors(function, pc, instruction) {
                if successor >= instructions.len() {
                    self.reject(index, || format!("invalid-successor:{successor}@{pc}"));
                    return None;
                }
                let changed = match &mut states[successor] {
                    Some(existing) => existing.join_from(&state),
                    target @ None => {
                        *target = Some(state.clone());
                        true
                    }
                };
                if changed {
                    work.push(successor);
                }
            }
        }
        let operands = states
            .iter()
            .zip(&instructions)
            .map(|(state, instruction)| {
                let state = state.as_ref()?;
                Some(
                    state
                        .get(instruction.b(), function.registers)
                        .unwrap_or(Slot::Conflict),
                )
            })
            .collect();
        if let Some(started) = plan_started {
            let reachable = states.iter().flatten().count();
            let stored_slots: usize = states.iter().flatten().map(|state| state.slots.len()).sum();
            let slot_capacity: usize = states
                .iter()
                .flatten()
                .map(|state| state.slots.capacity())
                .sum();
            let dense_state_bytes = reachable
                .saturating_mul(registers)
                .saturating_mul(std::mem::size_of::<Slot>());
            let sparse_state_bytes = states
                .len()
                .saturating_mul(std::mem::size_of::<Option<AnalysisState>>())
                .saturating_add(slot_capacity.saturating_mul(std::mem::size_of::<(u16, Slot)>()));
            let memory_after = jit_memory_snapshot();
            eprintln!(
                "QUENCH_WASM_JIT_PLAN_MEMORY function={index} bytecodes={} registers={registers} reachable={reachable} stored_slots={stored_slots} dense_state_bytes={dense_state_bytes} sparse_state_bytes={sparse_state_bytes} elapsed_us={} rss_kib_before={:?} rss_kib_after={:?} hwm_kib_before={:?} hwm_kib_after={:?}",
                instructions.len(),
                started.elapsed().as_micros(),
                memory_before.map(|memory| memory.0),
                memory_after.map(|memory| memory.0),
                memory_before.map(|memory| memory.1),
                memory_after.map(|memory| memory.1),
            );
        }
        Some(Plan {
            params,
            result,
            operands,
        })
    }

    /// Check `instruction` has a compiled denotation in `state`, and apply its
    /// register writes.
    #[allow(clippy::too_many_arguments)]
    fn transfer(
        &self,
        function: &Function,
        instruction: WideInstruction,
        state: &mut AnalysisState,
        callable: &[bool],
        memory_register: Option<u16>,
        memory_slot: &mut Option<u32>,
        result: Option<crate::WasmType>,
    ) -> Option<()> {
        let op = instruction.op();
        if op != Op::Return && instruction.returns_from_frame() {
            return None;
        }
        if !super::dispatch_fast::lane_fields_fit(instruction, function) {
            return None;
        }
        let value =
            |state: &AnalysisState, register: u16| match state.get(register, function.registers)? {
                Slot::Value(kind) => Some(kind),
                _ => None,
            };
        let i32_value = |state: &AnalysisState, register: u16| {
            (value(state, register)? == Kind::I32).then_some(())
        };
        let set = |state: &mut AnalysisState, register: u16, slot: Slot| {
            state.set(register, function.registers, slot)
        };
        // The memory register is read only by direct accesses through it.
        if memory_register.is_some_and(|memory| {
            !direct_access(op)
                && !matches!(op, Op::WasmMemorySize | Op::WasmMemoryFill)
                && op != Op::WasmInstanceBinding
                && crate::bytecode::InstructionField::ALL.iter().any(|field| {
                    op.field_layout(*field).reads_register()
                        && instruction.field_value(*field) == memory
                })
        }) {
            return None;
        }
        if let Some(_operator) = I32BinaryOperator::from_register_op(op) {
            i32_value(state, instruction.b())?;
            i32_value(state, instruction.c())?;
            return set(state, instruction.a(), Slot::Value(Kind::I32));
        }
        if I32BinaryOperator::from_immediate_op(op).is_some() {
            i32_value(state, instruction.b())?;
            return set(state, instruction.a(), Slot::Value(Kind::I32));
        }
        if I32BinaryOperator::from_jump_op(op).is_some() {
            i32_value(state, instruction.a())?;
            return i32_value(state, instruction.b());
        }
        if I32BinaryOperator::from_immediate_jump_op(op).is_some() {
            return i32_value(state, instruction.a());
        }
        if let Some(load) = MemoryLoad::from_direct_op(op) {
            if Some(instruction.b()) != memory_register
                || state.get(instruction.b(), function.registers) != Some(Slot::Memory)
            {
                return None;
            }
            i32_value(state, instruction.c())?;
            return set(
                state,
                instruction.a(),
                Slot::Value(Kind::of(load_type(load)?)?),
            );
        }
        if let Some(store) = MemoryStore::from_direct_op(op) {
            if Some(instruction.b()) != memory_register
                || state.get(instruction.b(), function.registers) != Some(Slot::Memory)
            {
                return None;
            }
            i32_value(state, instruction.c())?;
            return (value(state, instruction.a())? == Kind::of(store.value_type())?).then_some(());
        }
        // Result widths are read off the operators' own semantics:
        // comparisons produce an i32.
        if let Some((operator, ty)) = float_binary_operator(op, instruction.imm()) {
            let kind = Kind::of(ty)?;
            (value(state, instruction.b())? == kind).then_some(())?;
            (value(state, instruction.c())? == kind).then_some(())?;
            let comparison = operator.apply(0.0, 0.0).ty() == crate::WasmType::I32;
            let result = if comparison { Kind::I32 } else { kind };
            return set(state, instruction.a(), Slot::Value(result));
        }
        if let Some((_, ty)) = float_unary_operator(op, instruction.imm()) {
            let kind = Kind::of(ty)?;
            (value(state, instruction.b())? == kind).then_some(())?;
            return set(state, instruction.a(), Slot::Value(kind));
        }
        match op {
            Op::WasmI64Binary => {
                let operator = I64BinaryOperator::from_tag(instruction.imm())?;
                (value(state, instruction.b())? == Kind::I64).then_some(())?;
                (value(state, instruction.c())? == Kind::I64).then_some(())?;
                let result = Kind::of(operator.apply(1, 1).ok()?.ty())?;
                set(state, instruction.a(), Slot::Value(result))
            }
            Op::WasmI64Unary => {
                let operator = I64UnaryOperator::from_tag(instruction.imm())?;
                (value(state, instruction.b())? == Kind::I64).then_some(())?;
                let result = Kind::of(operator.apply(0).ty())?;
                set(state, instruction.a(), Slot::Value(result))
            }
            Op::WasmScalarConvert => {
                let operator = ScalarConversionOperator::from_tag(instruction.imm())?;
                (value(state, instruction.b())? == Kind::of(operator.source_type())?)
                    .then_some(())?;
                let result = Kind::of(operator.result_type())?;
                set(state, instruction.a(), Slot::Value(result))
            }
            Op::Move => {
                let kind = value(state, instruction.b())?;
                set(state, instruction.a(), Slot::Value(kind))
            }
            Op::LoadConst => {
                let constant = self.constant(instruction.constant_index())?;
                set(
                    state,
                    instruction.a(),
                    Slot::Value(self.constant_kind(constant)),
                )
            }
            Op::WasmFillRegisters => {
                let constant = self.constant(instruction.constant_index())?;
                let kind = self.constant_kind(constant);
                let window = instruction.register_window();
                for offset in 0..window.count {
                    set(state, window.base + offset, Slot::Value(kind))?;
                }
                Some(())
            }
            Op::WasmI32Unary => {
                I32UnaryOperator::from_tag(instruction.imm())?;
                i32_value(state, instruction.b())?;
                set(state, instruction.a(), Slot::Value(Kind::I32))
            }
            Op::WasmI32ShiftRightUnsignedAndImmediate => {
                i32_value(state, instruction.b())?;
                set(state, instruction.a(), Slot::Value(Kind::I32))
            }
            Op::WasmI32AddImmediateJumpNonZero => {
                i32_value(state, instruction.b())?;
                set(state, instruction.a(), Slot::Value(Kind::I32))
            }
            Op::WasmJumpI32Zero | Op::WasmJumpI32NonZero => i32_value(state, instruction.a()),
            Op::Jump => Some(()),
            Op::WasmBranchTable => {
                i32_value(state, instruction.a())?;
                let pc_entries: Vec<_> =
                    crate::bytecode::branch_table_entries(0, instruction).collect();
                // Entries are `Jump`s; validation guarantees it for
                // reachable tables.
                (!pc_entries.is_empty()).then_some(())
            }
            Op::WasmSelect => {
                let (condition, _) = instruction.register_pair();
                i32_value(state, condition)?;
                let kind = value(state, instruction.b())?;
                (value(state, instruction.c())? == kind).then_some(())?;
                set(state, instruction.a(), Slot::Value(kind))
            }
            Op::WasmMemorySize => {
                let memory = instruction.b();
                if Some(memory) != memory_register
                    || state.get(memory, function.registers) != Some(Slot::Memory)
                    || !memory_slot.is_some_and(|slot| self.supports_compiled_memory(slot))
                {
                    return None;
                }
                set(state, instruction.a(), Slot::Value(Kind::I32))
            }
            Op::WasmMemoryFill => {
                let memory = instruction.a();
                let (destination, _) = instruction.register_pair();
                if Some(memory) != memory_register
                    || state.get(memory, function.registers) != Some(Slot::Memory)
                    || !memory_slot.is_some_and(|slot| self.supports_compiled_memory(slot))
                {
                    return None;
                }
                i32_value(state, instruction.b())?;
                i32_value(state, instruction.c())?;
                i32_value(state, destination)
            }
            Op::WasmIndirectTarget if self.callouts => {
                let Slot::Binding(table) = state.get(instruction.b(), function.registers)? else {
                    return None;
                };
                if !self.binds_table32(table) {
                    return None;
                }
                i32_value(state, instruction.c())?;
                self.type_signature(instruction.imm())?;
                set(
                    state,
                    instruction.a(),
                    Slot::IndirectReference {
                        signature: instruction.imm(),
                        table,
                    },
                )
            }
            Op::CallKnown => {
                let callee = u32::from(instruction.known_function_index());
                if !callable.get(callee as usize).copied().unwrap_or(false)
                    && !self.experimental_root_compilation
                {
                    return None;
                }
                let signature = self.signature(callee)?;
                if signature.results.len() > 1
                    && !callable.get(callee as usize).copied().unwrap_or(false)
                {
                    // The VM callout ABI currently carries at most one result.
                    // Do not admit a residual fallback that would silently
                    // discard the remaining Wasm results.
                    return None;
                }
                let window = instruction.call_window();
                if usize::from(window.count) != signature.params.len() {
                    return None;
                }
                for (offset, ty) in signature.params.iter().enumerate() {
                    (value(state, window.base + offset as u16)? == Kind::of(*ty)?).then_some(())?;
                }
                let kind = match signature.results.first() {
                    Some(ty) => Kind::of(*ty)?,
                    None => Kind::I32,
                };
                set(state, instruction.result_register(), Slot::Value(kind))
            }
            Op::WasmRefFunc if self.callouts => {
                let function = instruction.imm();
                self.import_signature(function)?;
                set(state, instruction.a(), Slot::Import(function))
            }
            Op::Call => {
                let slot = state.get(instruction.b(), function.registers)?;
                let signature = match slot {
                    Slot::Import(function) => self.import_signature(function)?,
                    Slot::IndirectReference { signature, .. } if self.callouts => {
                        self.type_signature(signature)?
                    }
                    _ => return None,
                };
                let window = instruction.call_window();
                if usize::from(window.count) != signature.params.len()
                    || signature.results.len() > 1
                {
                    return None;
                }
                for (offset, ty) in signature.params.iter().enumerate() {
                    (value(state, window.base + offset as u16)? == Kind::of(*ty)?).then_some(())?;
                }
                let kind = match signature.results.first() {
                    Some(ty) => Kind::of(*ty)?,
                    None => Kind::I32,
                };
                set(state, instruction.result_register(), Slot::Value(kind))
            }
            Op::WasmUnreachable if self.experimental_root_compilation => Some(()),
            Op::Return => match result {
                Some(ty) => (value(state, instruction.a())? == Kind::of(ty)?).then_some(()),
                None => Some(()),
            },
            Op::WasmInstanceBinding => {
                let slot = instruction.imm();
                if Some(instruction.a()) == memory_register {
                    if !self.binds_unshared_memory(slot) || memory_slot.is_some_and(|s| s != slot) {
                        return None;
                    }
                    *memory_slot = Some(slot);
                    return set(state, instruction.a(), Slot::Memory);
                }
                set(state, instruction.a(), Slot::Binding(slot))
            }
            // Compiled code reads and writes a 32-bit global's encoded value
            // in place; a 64-bit global's value is a box, so it stays
            // interpreted.
            Op::WasmGlobalGet => {
                let Slot::Binding(slot) = state.get(instruction.b(), function.registers)? else {
                    return None;
                };
                (Kind::of(self.global_type(slot)?)? == Kind::I32).then_some(())?;
                set(state, instruction.a(), Slot::Value(Kind::I32))
            }
            Op::WasmGlobalSet => {
                let Slot::Binding(slot) = state.get(instruction.b(), function.registers)? else {
                    return None;
                };
                (Kind::of(self.global_type(slot)?)? == Kind::I32).then_some(())?;
                (value(state, instruction.a())? == Kind::I32).then_some(())
            }
            _ => None,
        }
    }
}

/// The Wasm type a direct load produces.
fn load_type(load: MemoryLoad) -> Option<crate::WasmType> {
    Some(match load {
        MemoryLoad::I32Load
        | MemoryLoad::I32Load8S
        | MemoryLoad::I32Load8U
        | MemoryLoad::I32Load16S
        | MemoryLoad::I32Load16U => crate::WasmType::I32,
        MemoryLoad::F32Load => crate::WasmType::F32,
        MemoryLoad::I64Load
        | MemoryLoad::I64Load8S
        | MemoryLoad::I64Load8U
        | MemoryLoad::I64Load16S
        | MemoryLoad::I64Load16U
        | MemoryLoad::I64Load32S
        | MemoryLoad::I64Load32U => crate::WasmType::I64,
        MemoryLoad::F64Load => crate::WasmType::F64,
        _ => return None,
    })
}

/// Whether a direct load sign-extends its narrow word.
fn load_signed(load: MemoryLoad) -> bool {
    matches!(
        load,
        MemoryLoad::I32Load8S
            | MemoryLoad::I32Load16S
            | MemoryLoad::I64Load8S
            | MemoryLoad::I64Load16S
            | MemoryLoad::I64Load32S
    )
}

/// The width in bytes a direct store writes.
fn store_width(store: MemoryStore) -> usize {
    match store {
        MemoryStore::I32Store8 | MemoryStore::I64Store8 => 1,
        MemoryStore::I32Store16 | MemoryStore::I64Store16 => 2,
        MemoryStore::I32Store | MemoryStore::F32Store | MemoryStore::I64Store32 => 4,
        _ => 8,
    }
}

/// Disjoint state compiled code accesses: an access to one never aliases an
/// access to another, so loads of one survive stores to the other.
#[derive(Clone, Copy, Debug)]
enum Region {
    /// The instance's linear memory.
    LinearMemory,
    /// Global values, which live in heap cells.
    Globals,
}

impl Region {
    const ALL: [Self; 2] = [Self::LinearMemory, Self::Globals];
    const COUNT: usize = Self::ALL.len();
}

/// Flags of a load of context state fixed for the whole call (the stack
/// limit, the callout, global addresses): it may be reused and hoisted.
fn invariant() -> MemFlagsData {
    MemFlagsData::trusted().with_readonly().with_can_move()
}

/// The memory facts a compiled call passes after the context: the base
/// unless the program fixes it as a constant, and the length unless the
/// guard bounds accesses. One program's functions share them.
#[derive(Clone, Copy)]
struct MemoryArguments {
    base: bool,
    length: bool,
}

impl MemoryArguments {
    fn of(fixed_base: Option<i32>, guarded: bool) -> Self {
        Self {
            base: fixed_base.is_none(),
            length: !guarded,
        }
    }

    /// The passed ones of `base` and `length`, in parameter order.
    fn select(self, base: ir::Value, length: ir::Value) -> impl Iterator<Item = ir::Value> {
        [self.base.then_some(base), self.length.then_some(length)]
            .into_iter()
            .flatten()
    }

    fn count(self) -> usize {
        usize::from(self.base) + usize::from(self.length)
    }
}

/// The internal signature of a compiled function: the context, the memory
/// arguments, then the Wasm parameters.
fn internal_signature(
    memory: MemoryArguments,
    plan_params: &[crate::WasmType],
    result: Option<crate::WasmType>,
    call_conv: cranelift_codegen::isa::CallConv,
) -> Signature {
    let mut signature = Signature::new(call_conv);
    for _ in 0..1 + memory.count() {
        signature.params.push(AbiParam::new(types::I64));
    }
    for ty in plan_params {
        signature
            .params
            .push(AbiParam::new(Kind::of(*ty).expect("planned").ir()));
    }
    signature.returns.push(AbiParam::new(types::I64));
    if let Some(ty) = result {
        signature
            .returns
            .push(AbiParam::new(Kind::of(ty).expect("planned").ir()));
    }
    signature
}

/// Instructions a threaded copy may repeat between a constant's jump and the
/// branch table the constant decides; bounds the code each copy adds.
const THREAD_BUDGET: usize = 16;

/// Where a threaded copy continues once its path is done.
#[derive(Clone, Copy)]
enum ThreadEnd {
    /// The branch table's case for the constant.
    Case(usize),
    /// The successor the constant decided a branch to.
    Exit(usize),
    /// The table at the path's end, run for real: a jump that knows no
    /// selector gets its own copy of the dispatch (`dispatch_path`), so each
    /// case's back edge predicts its own successor.
    Dispatch,
}

/// A copy of the instructions from a jump target to a branch table, for a
/// jump that leaves the table's selector holding a known constant: along
/// the copy the selector cannot change, so branches on it are decided and
/// the table becomes a direct jump (jump threading; a state machine's
/// dispatch on the state it just set disappears). The copy repeats each
/// instruction at its own PC and exits only to block leaders.
struct Thread {
    selector: u16,
    value: i32,
    path: Vec<usize>,
    end: ThreadEnd,
}

/// Whether `instruction` may write `register`; windows and other register
/// groups count as writing every register.
fn writes_register(instruction: WideInstruction, register: u16) -> bool {
    use crate::bytecode::{FieldLayout, InstructionField};
    InstructionField::ALL.iter().any(|field| {
        let operand = match field {
            InstructionField::A => instruction.a(),
            InstructionField::B => instruction.b(),
            InstructionField::C => instruction.c(),
        };
        match instruction.op().field_layout(*field) {
            FieldLayout::ResultRegister
            | FieldLayout::WriteRegister
            | FieldLayout::ReadWriteRegister => operand == register,
            FieldLayout::RegisterWindowBase
            | FieldLayout::RegisterCount
            | FieldLayout::ConstructArguments
            | FieldLayout::NumericLocalTarget => true,
            _ => false,
        }
    })
}

/// Whether conditional jump `instruction` is taken when `register` holds
/// `value`, if that alone decides it.
fn decided_branch(instruction: WideInstruction, register: u16, value: i32) -> Option<bool> {
    let op = instruction.op();
    if instruction.a() != register {
        return None;
    }
    if let Some(operator) = I32BinaryOperator::from_immediate_jump_op(op) {
        let right = i32::from(instruction.b() as i16);
        return operator.evaluate(value, right).ok().map(|holds| holds != 0);
    }
    match op {
        Op::WasmJumpI32Zero => Some(value == 0),
        Op::WasmJumpI32NonZero => Some(value != 0),
        _ => None,
    }
}

/// The threaded copy from `start` for `selector` holding `value`, when one
/// reaches a branch table on the selector, or a branch the value decides,
/// within the budget.
fn thread_path(
    instructions: &[WideInstruction],
    plan: &Plan,
    start: usize,
    selector: u16,
    value: i32,
) -> Option<Thread> {
    fn walk(
        instructions: &[WideInstruction],
        plan: &Plan,
        pc: usize,
        selector: u16,
        value: i32,
        path: &mut Vec<usize>,
    ) -> Option<ThreadEnd> {
        if path.len() == THREAD_BUDGET || !plan.reachable(pc) {
            return None;
        }
        let instruction = *instructions.get(pc)?;
        if writes_register(instruction, selector) {
            return None;
        }
        path.push(pc);
        let end = match instruction.op().control_flow_layout() {
            ControlFlowLayout::BranchTable if instruction.a() == selector => {
                let entries: Vec<usize> = crate::bytecode::branch_table_entries(pc, instruction)
                    .map(|entry| instructions[entry].jump_target() as usize)
                    .collect();
                let (default, cases) = entries.split_last()?;
                let case = usize::try_from(value as u32).ok();
                Some(ThreadEnd::Case(
                    *case.and_then(|case| cases.get(case)).unwrap_or(default),
                ))
            }
            ControlFlowLayout::Fallthrough => {
                walk(instructions, plan, pc + 1, selector, value, path)
            }
            ControlFlowLayout::Jump => walk(
                instructions,
                plan,
                instruction.jump_target() as usize,
                selector,
                value,
                path,
            ),
            ControlFlowLayout::ConditionalJump => {
                let target = instruction.jump_target() as usize;
                match decided_branch(instruction, selector, value) {
                    Some(taken) => {
                        let next = if taken { target } else { pc + 1 };
                        let length = path.len();
                        walk(instructions, plan, next, selector, value, path).or_else(|| {
                            path.truncate(length);
                            Some(ThreadEnd::Exit(next))
                        })
                    }
                    None => [target, pc + 1].into_iter().find_map(|next| {
                        let length = path.len();
                        walk(instructions, plan, next, selector, value, path).or_else(|| {
                            path.truncate(length);
                            None
                        })
                    }),
                }
            }
            _ => None,
        };
        if end.is_none() {
            path.pop();
        }
        end
    }
    let mut path = Vec::new();
    let end = walk(instructions, plan, start, selector, value, &mut path)?;
    Some(Thread {
        selector,
        value,
        path,
        end,
    })
}

/// The straight-line copy from jump target `start` to the branch table it
/// reaches, within the threading budget; the table is the path's last
/// instruction.
fn dispatch_path(instructions: &[WideInstruction], plan: &Plan, start: usize) -> Option<Thread> {
    let mut path = Vec::new();
    let mut pc = start;
    loop {
        if path.len() == THREAD_BUDGET || !plan.reachable(pc) {
            return None;
        }
        let instruction = *instructions.get(pc)?;
        path.push(pc);
        pc = match instruction.op().control_flow_layout() {
            ControlFlowLayout::Fallthrough => pc + 1,
            ControlFlowLayout::Jump => instruction.jump_target() as usize,
            ControlFlowLayout::BranchTable => {
                return Some(Thread {
                    selector: instruction.a(),
                    value: 0,
                    path,
                    end: ThreadEnd::Dispatch,
                });
            }
            _ => return None,
        };
    }
}

struct Emitter<'a, 'b, H: Host> {
    guarded: bool,
    /// The guarded memory repeats above 2^32 (`CompiledProgram::wraps`).
    wraps: bool,
    /// Per `Region`, its alias region in this function.
    regions: [ir::AliasRegion; Region::COUNT],
    globals: &'a [u32],
    analysis: &'a Analysis<'a, H>,
    builder: FunctionBuilder<'b>,
    plan: &'a Plan,
    variables: Vec<[Variable; VIEW_TYPES.len()]>,
    blocks: Vec<Option<ir::Block>>,
    context: ir::Value,
    /// Dense ids for the compiled artifact's actual indirect call sites.
    reference_site_ids: &'a [(usize, usize)],
    /// Selects the diagnostics ABI at compile time; both ABIs carry site ids.
    diagnostics_enabled: bool,
    base: ir::Value,
    /// The memory's exact length for checked accesses and bulk operations.
    length: Option<ir::Value>,
    memory: MemoryArguments,
    trap_blocks: [Option<ir::Block>; TRAPS.len()],
    termination_block: Option<ir::Block>,
    /// Per callee, its imported name in this function.
    callees: Vec<Option<ir::FuncRef>>,
    callout_signature: Option<ir::SigRef>,
    plans: &'a [Option<Plan>],
    call_conv: cranelift_codegen::isa::CallConv,
    instructions: &'a [WideInstruction],
    /// Registers some branch table dispatches on.
    selectors: Vec<u16>,
    /// Selectors known to hold an i32 constant since the current block began.
    known: Vec<(u16, i32)>,
    /// Threaded copies by (target, selector, value); `None` where none fits.
    threads: Vec<((usize, u16, i32), Option<ir::Block>)>,
    /// Copies to emit once the function body is done.
    pending: Vec<(ir::Block, Thread)>,
    /// While a copy emits a branch, the block standing in for one target.
    redirect: Option<(usize, ir::Block)>,
    /// Whether jumps may enter threaded copies (not from within one).
    threading: bool,
    /// The VM's poll page when every entry runs through the fault-resuming trampoline: an
    /// interrupt check is then one read of it.
    poll: Option<i64>,
    /// Indices the current block, or a block it alone leads to, has checked against a limit of
    /// the wrapped reservation (`address`): a later access through one of them needs no check
    /// at the same or a higher limit.
    checked: Vec<(ir::Value, i64)>,
}

impl<H: Host> Emitter<'_, '_, H> {
    /// The flags of an access to `region`.
    fn flags(&self, region: Region) -> MemFlagsData {
        MemFlagsData::trusted()
            .with_endianness(ir::Endianness::Little)
            .with_alias_region(Some(self.regions[region as usize]))
    }

    fn read(&mut self, register: u16, kind: Kind) -> ir::Value {
        self.read_typed(register, kind.integer())
    }

    fn write(&mut self, register: u16, kind: Kind, value: ir::Value) {
        self.write_typed(register, kind.integer(), value);
    }

    /// The register's value viewed as `ty`.
    fn read_typed(&mut self, register: u16, ty: crate::WasmType) -> ir::Value {
        self.builder
            .use_var(self.variables[usize::from(register)][view(ty)])
    }

    /// Define the register as `value` of type `ty`, in both views; a view no
    /// instruction reads costs nothing, since its conversion is dead.
    fn write_typed(&mut self, register: u16, ty: crate::WasmType, value: ir::Value) {
        self.known.retain(|(known, _)| *known != register);
        let other = counterpart(ty);
        let converted = self
            .builder
            .ins()
            .bitcast(scalar_ir(other), MemFlagsData::new(), value);
        let variables = self.variables[usize::from(register)];
        self.builder.def_var(variables[view(ty)], value);
        self.builder.def_var(variables[view(other)], converted);
    }

    /// Define the register as the constant `bits` of width `kind`.
    fn write_constant(&mut self, register: u16, kind: Kind, bits: i64) {
        self.known.retain(|(known, _)| *known != register);
        if kind == Kind::I32 && self.selectors.contains(&register) {
            self.known.push((register, bits as i32));
        }
        let (integer, float) = match kind {
            Kind::I32 => (
                self.builder.ins().iconst(types::I32, bits),
                self.builder.ins().f32const(Ieee32::with_bits(bits as u32)),
            ),
            Kind::I64 => (
                self.builder.ins().iconst(types::I64, bits),
                self.builder.ins().f64const(Ieee64::with_bits(bits as u64)),
            ),
        };
        let float_type = counterpart(kind.integer());
        let variables = self.variables[usize::from(register)];
        self.builder
            .def_var(variables[view(kind.integer())], integer);
        self.builder.def_var(variables[view(float_type)], float);
    }

    /// The kind of the `b` register of instruction `pc`.
    fn operand_kind(&self, pc: usize) -> Kind {
        match self.plan.operand(pc) {
            Slot::Value(kind) => kind,
            _ => Kind::I32,
        }
    }

    fn block(&mut self, pc: usize) -> ir::Block {
        *self.blocks[pc].get_or_insert_with(|| self.builder.create_block())
    }

    fn trap_block(&mut self, trap: crate::WasmTrap) -> ir::Block {
        let index = (trap_status(trap) - 1) as usize;
        if let Some(block) = self.trap_blocks[index] {
            return block;
        }
        let block = self.builder.create_block();
        self.builder.set_cold_block(block);
        self.trap_blocks[index] = Some(block);
        block
    }

    /// Branch to the trap when `condition` is nonzero; continue otherwise.
    fn trap_if(&mut self, condition: ir::Value, trap: crate::WasmTrap) {
        let trap = self.trap_block(trap);
        let next = self.builder.create_block();
        self.builder.ins().brif(condition, trap, &[], next, &[]);
        self.builder.switch_to_block(next);
        self.builder.seal_block(next);
    }

    fn interrupt_safepoint(&mut self) {
        if let Some(poll) = self.poll {
            // The read faults once termination is requested, and the fault resumes as the
            // termination (`guard_fault`). Atomic, so it is neither merged nor dropped.
            let page = self.builder.ins().iconst(types::I64, poll);
            self.builder
                .ins()
                .atomic_load(types::I8, MemFlagsData::trusted(), page);
            return;
        }
        // The flag's address is fixed for the call; only the flag changes.
        let pointer =
            self.builder
                .ins()
                .load(types::I64, invariant(), self.context, CONTEXT_INTERRUPT);
        let requested = self
            .builder
            .ins()
            // Atomic, so no check reuses an earlier one's load: Cranelift's alias analysis
            // would merge plain loads of the flag across the loop.
            .atomic_load(types::I8, MemFlagsData::trusted(), pointer);
        let stop = self.builder.ins().icmp_imm_u(IntCC::NotEqual, requested, 0);
        let termination = match self.termination_block {
            Some(block) => block,
            None => {
                let block = self.builder.create_block();
                self.termination_block = Some(block);
                block
            }
        };
        let next = self.builder.create_block();
        self.builder.ins().brif(stop, termination, &[], next, &[]);
        self.builder.switch_to_block(next);
        self.builder.seal_block(next);
    }
    fn finish_traps(&mut self) {
        if let Some(block) = self.termination_block {
            self.builder.switch_to_block(block);
            let status = self.builder.ins().iconst(types::I64, TERMINATED_STATUS);
            let mut values = vec![status];
            if let Some(ty) = self.plan.result {
                let kind = Kind::of(ty).expect("planned");
                values.push(self.builder.ins().iconst(kind.ir(), 0));
            }
            self.builder.ins().return_(&values);
        }
        for (index, block) in self.trap_blocks.into_iter().enumerate() {
            let Some(block) = block else { continue };
            self.builder.switch_to_block(block);
            let status = self.builder.ins().iconst(types::I64, index as i64 + 1);
            let mut values = vec![status];
            if let Some(ty) = self.plan.result {
                let kind = Kind::of(ty).expect("planned");
                values.push(self.builder.ins().iconst(kind.ir(), 0));
            }
            self.builder.ins().return_(&values);
        }
    }

    /// An integer operator at the operands' width.
    fn integer_binary(
        &mut self,
        operator: I64BinaryOperator,
        left: ir::Value,
        right: ir::Value,
    ) -> ir::Value {
        use I64BinaryOperator as B;
        let ty = self.builder.func.dfg.value_type(left);
        let compare = |this: &mut Self, cc: IntCC| {
            let flag = this.builder.ins().icmp(cc, left, right);
            this.builder.ins().uextend(types::I32, flag)
        };
        match operator {
            B::Add => self.builder.ins().iadd(left, right),
            B::Subtract => self.builder.ins().isub(left, right),
            B::Multiply => self.builder.ins().imul(left, right),
            B::And => self.builder.ins().band(left, right),
            B::Or => self.builder.ins().bor(left, right),
            B::Xor => self.builder.ins().bxor(left, right),
            B::ShiftLeft => self.builder.ins().ishl(left, right),
            B::ShiftRightSigned => self.builder.ins().sshr(left, right),
            B::ShiftRightUnsigned => self.builder.ins().ushr(left, right),
            B::RotateLeft => self.builder.ins().rotl(left, right),
            B::RotateRight => self.builder.ins().rotr(left, right),
            B::MultiplyHighSigned => self.builder.ins().smulhi(left, right),
            B::MultiplyHighUnsigned => self.builder.ins().umulhi(left, right),
            B::Equal => compare(self, IntCC::Equal),
            B::NotEqual => compare(self, IntCC::NotEqual),
            B::LessSigned => compare(self, IntCC::SignedLessThan),
            B::LessUnsigned => compare(self, IntCC::UnsignedLessThan),
            B::GreaterSigned => compare(self, IntCC::SignedGreaterThan),
            B::GreaterUnsigned => compare(self, IntCC::UnsignedGreaterThan),
            B::LessEqualSigned => compare(self, IntCC::SignedLessThanOrEqual),
            B::LessEqualUnsigned => compare(self, IntCC::UnsignedLessThanOrEqual),
            B::GreaterEqualSigned => compare(self, IntCC::SignedGreaterThanOrEqual),
            B::GreaterEqualUnsigned => compare(self, IntCC::UnsignedGreaterThanOrEqual),
            B::DivideSigned | B::RemainderSigned | B::DivideUnsigned | B::RemainderUnsigned => {
                let zero = self.builder.ins().icmp_imm_s(IntCC::Equal, right, 0);
                self.trap_if(zero, crate::WasmTrap::IntegerDivideByZero);
                match operator {
                    B::DivideUnsigned => self.builder.ins().udiv(left, right),
                    B::RemainderUnsigned => self.builder.ins().urem(left, right),
                    B::DivideSigned => {
                        let minimum = if ty == types::I32 {
                            i64::from(i32::MIN)
                        } else {
                            i64::MIN
                        };
                        let minimum = self.builder.ins().icmp_imm_s(IntCC::Equal, left, minimum);
                        let negative_one = self.builder.ins().icmp_imm_s(IntCC::Equal, right, -1);
                        let overflow = self.builder.ins().band(minimum, negative_one);
                        self.trap_if(overflow, crate::WasmTrap::IntegerOverflow);
                        self.builder.ins().sdiv(left, right)
                    }
                    _ => {
                        // `MIN % -1` is 0; the machine division would fault.
                        let negative_one = self.builder.ins().icmp_imm_s(IntCC::Equal, right, -1);
                        let one = self.builder.ins().iconst(ty, 1);
                        let divisor = self.builder.ins().select(negative_one, one, right);
                        self.builder.ins().srem(left, divisor)
                    }
                }
            }
        }
    }

    /// An integer unary operator at the operand's width.
    fn integer_unary(&mut self, operator: I64UnaryOperator, value: ir::Value) -> ir::Value {
        use I64UnaryOperator as U;
        let ty = self.builder.func.dfg.value_type(value);
        let mut extend = |narrow_type| {
            let narrow = self.builder.ins().ireduce(narrow_type, value);
            self.builder.ins().sextend(ty, narrow)
        };
        match operator {
            U::EqualZero => {
                let flag = self.builder.ins().icmp_imm_s(IntCC::Equal, value, 0);
                self.builder.ins().uextend(types::I32, flag)
            }
            U::LeadingZeros => self.builder.ins().clz(value),
            U::TrailingZeros => self.builder.ins().ctz(value),
            U::PopulationCount => self.builder.ins().popcnt(value),
            U::ExtendSigned8 => extend(types::I8),
            U::ExtendSigned16 => extend(types::I16),
            U::ExtendSigned32 => extend(types::I32),
        }
    }

    /// The canonical NaN of float type `ty`, as `crate::wasm::float` produces.
    fn canonical_nan(&mut self, ty: ir::Type) -> ir::Value {
        use crate::wasm::numeric::canonical_nan_bits;
        if ty == types::F32 {
            let bits = canonical_nan_bits(f32::MANTISSA_DIGITS, u64::from(f32::INFINITY.to_bits()));
            self.builder.ins().f32const(Ieee32::with_bits(bits as u32))
        } else {
            let bits = canonical_nan_bits(f64::MANTISSA_DIGITS, f64::INFINITY.to_bits());
            self.builder.ins().f64const(Ieee64::with_bits(bits))
        }
    }

    /// A float operator at the operands' width.
    fn float_binary(
        &mut self,
        operator: F64BinaryOperator,
        left: ir::Value,
        right: ir::Value,
    ) -> ir::Value {
        use F64BinaryOperator as F;
        let compare = |this: &mut Self, cc: FloatCC| {
            let flag = this.builder.ins().fcmp(cc, left, right);
            this.builder.ins().uextend(types::I32, flag)
        };
        match operator {
            F::Add => self.builder.ins().fadd(left, right),
            F::Subtract => self.builder.ins().fsub(left, right),
            F::Multiply => self.builder.ins().fmul(left, right),
            F::Divide => self.builder.ins().fdiv(left, right),
            F::Minimum | F::Maximum => {
                // Signed zeros order as Wasm requires; a NaN operand gives
                // the canonical NaN.
                let extremum = if operator == F::Minimum {
                    self.builder.ins().fmin(left, right)
                } else {
                    self.builder.ins().fmax(left, right)
                };
                let unordered = self.builder.ins().fcmp(FloatCC::Unordered, left, right);
                let ty = self.builder.func.dfg.value_type(left);
                let nan = self.canonical_nan(ty);
                self.builder.ins().select(unordered, nan, extremum)
            }
            F::CopySign => self.builder.ins().fcopysign(left, right),
            F::Equal => compare(self, FloatCC::Equal),
            F::NotEqual => compare(self, FloatCC::NotEqual),
            F::Less => compare(self, FloatCC::LessThan),
            F::Greater => compare(self, FloatCC::GreaterThan),
            F::LessEqual => compare(self, FloatCC::LessThanOrEqual),
            F::GreaterEqual => compare(self, FloatCC::GreaterThanOrEqual),
        }
    }

    /// A float unary operator at the operand's width. The machine rounding
    /// and square root quiet a signaling NaN, as the operators do.
    fn float_unary(&mut self, operator: F64UnaryOperator, value: ir::Value) -> ir::Value {
        use F64UnaryOperator as U;
        match operator {
            U::Absolute => self.builder.ins().fabs(value),
            U::Negate => self.builder.ins().fneg(value),
            U::Ceiling => self.builder.ins().ceil(value),
            U::Floor => self.builder.ins().floor(value),
            U::Truncate => self.builder.ins().trunc(value),
            U::Nearest => self.builder.ins().nearest(value),
            U::SquareRoot => self.builder.ins().sqrt(value),
        }
    }

    /// A scalar conversion of `value`, already viewed as the source type.
    fn convert(&mut self, operator: ScalarConversionOperator, value: ir::Value) -> ir::Value {
        use ScalarConversionOperator as C;
        let result = scalar_ir(operator.result_type());
        let source = operator.source_type();
        match operator {
            C::WrapI64 => self.builder.ins().ireduce(result, value),
            C::ExtendI32Signed => self.builder.ins().sextend(result, value),
            C::ExtendI32Unsigned => self.builder.ins().uextend(result, value),
            C::TruncateF32SignedI32
            | C::TruncateF64SignedI32
            | C::TruncateF32SignedI64
            | C::TruncateF64SignedI64 => self.truncate(value, source, result, true),
            C::TruncateF32UnsignedI32
            | C::TruncateF64UnsignedI32
            | C::TruncateF32UnsignedI64
            | C::TruncateF64UnsignedI64 => self.truncate(value, source, result, false),
            C::SaturateF32SignedI32
            | C::SaturateF64SignedI32
            | C::SaturateF32SignedI64
            | C::SaturateF64SignedI64 => self.builder.ins().fcvt_to_sint_sat(result, value),
            C::SaturateF32UnsignedI32
            | C::SaturateF64UnsignedI32
            | C::SaturateF32UnsignedI64
            | C::SaturateF64UnsignedI64 => self.builder.ins().fcvt_to_uint_sat(result, value),
            C::ConvertI32SignedF32
            | C::ConvertI64SignedF32
            | C::ConvertI32SignedF64
            | C::ConvertI64SignedF64 => self.builder.ins().fcvt_from_sint(result, value),
            C::ConvertI32UnsignedF32
            | C::ConvertI64UnsignedF32
            | C::ConvertI32UnsignedF64
            | C::ConvertI64UnsignedF64 => self.builder.ins().fcvt_from_uint(result, value),
            C::PromoteF32 => self.builder.ins().fpromote(result, value),
            C::DemoteF64 => self.builder.ins().fdemote(result, value),
            C::ReinterpretF32I32
            | C::ReinterpretF64I64
            | C::ReinterpretI32F32
            | C::ReinterpretI64F64 => {
                self.builder
                    .ins()
                    .bitcast(result, MemFlagsData::new(), value)
            }
        }
    }

    /// A trapping truncation: NaN and out-of-range inputs trap, in the
    /// order and bounds of `crate::wasm::conversion`; the machine conversion
    /// then sees only representable values.
    fn truncate(
        &mut self,
        value: ir::Value,
        source: crate::WasmType,
        result: ir::Type,
        signed: bool,
    ) -> ir::Value {
        let wide = if source == crate::WasmType::F32 {
            self.builder.ins().fpromote(types::F64, value)
        } else {
            value
        };
        let nan = self.builder.ins().fcmp(FloatCC::Unordered, wide, wide);
        self.trap_if(nan, crate::WasmTrap::InvalidConversionToInteger);
        let truncated = self.builder.ins().trunc(wide);
        let (lower, limit) = crate::wasm::conversion::truncation_bounds(result.bits(), signed);
        let lower = self.builder.ins().f64const(lower);
        let limit = self.builder.ins().f64const(limit);
        let below = self.builder.ins().fcmp(FloatCC::LessThan, truncated, lower);
        let above = self
            .builder
            .ins()
            .fcmp(FloatCC::GreaterThanOrEqual, truncated, limit);
        let overflow = self.builder.ins().bor(below, above);
        self.trap_if(overflow, crate::WasmTrap::IntegerOverflow);
        if signed {
            self.builder.ins().fcvt_to_sint_sat(result, truncated)
        } else {
            self.builder.ins().fcvt_to_uint_sat(result, truncated)
        }
    }

    /// The host address of a checked `width`-byte access at `address + offset`.
    fn address(&mut self, address: ir::Value, offset: u32, width: usize) -> ir::Value {
        if self.wraps && offset == 0 {
            // The Wasm effective address is already an i32 bit pattern, so
            // preserve its modulo-2^32 result before extending it. This also
            // lets x64 lower an i32 base-plus-scaled-index to a 32-bit LEA;
            // the repeated mapping makes that result equivalent to the old
            // widened sum when the i32 addition carried past 2^32.
            let address = self.builder.ins().uextend(types::I64, address);
            return self.builder.ins().iadd(self.base, address);
        }
        if self.wraps && offset != 0 {
            // An index whose offset carries it past 2^32 would meet the
            // repeated memory instead of the guard: it traps here. The
            // memory ends below `RESERVED_LIMIT`, so for offsets up to a Wasm
            // page that limit bounds the index alone, and accesses through
            // one index share one check.
            use crate::wasm::memory::{RESERVED_LIMIT, WASM_PAGE, WRAP_ALIAS};
            let limit = if offset as usize <= WASM_PAGE {
                RESERVED_LIMIT
            } else {
                WRAP_ALIAS - offset as usize
            } as i64;
            let known = self
                .checked
                .iter()
                .any(|&(index, checked)| index == address && checked <= limit);
            if !known {
                let past = self.builder.ins().icmp_imm_u(
                    IntCC::UnsignedGreaterThanOrEqual,
                    address,
                    limit,
                );
                self.trap_if(past, crate::WasmTrap::OutOfBoundsMemory);
                self.checked.push((address, limit));
            }
        }
        let address = self.builder.ins().uextend(types::I64, address);
        if self.guarded {
            // Index plus offset plus width stays inside the reservation; past
            // the accessible prefix the access faults, and the fault handler
            // raises the out-of-bounds trap (see `guard_fault`).
            let host = self.builder.ins().iadd(self.base, address);
            return self.builder.ins().iadd_imm_s(host, i64::from(offset));
        }
        let end = self
            .builder
            .ins()
            .iadd_imm_s(address, i64::from(offset) + width as i64);
        let out_of_bounds = self.builder.ins().icmp(
            IntCC::UnsignedGreaterThan,
            end,
            self.length.expect("unguarded accesses receive the length"),
        );
        self.trap_if(out_of_bounds, crate::WasmTrap::OutOfBoundsMemory);
        let host = self.builder.ins().iadd(self.base, address);
        self.builder.ins().iadd_imm_s(host, i64::from(offset))
    }

    /// The address of the value of the global bound at `slot`.
    fn global_address(&mut self, slot: u32) -> ir::Value {
        let position = self
            .globals
            .iter()
            .position(|candidate| *candidate == slot)
            .expect("planned globals are collected");
        let table = self
            .builder
            .ins()
            .load(types::I64, invariant(), self.context, CONTEXT_GLOBALS);
        self.builder
            .ins()
            .load(types::I64, invariant(), table, (position * 8) as i32)
    }

    /// The binding the `b` register of instruction `pc` holds.
    fn binding_slot(&self, pc: usize) -> u32 {
        match self.plan.operand(pc) {
            Slot::Binding(slot) => slot,
            _ => unreachable!("planned global accesses read a binding"),
        }
    }

    /// A nonzero status from a call (a trap or a raised callout) unwinds
    /// this activation too.
    fn propagate(&mut self, status: ir::Value) {
        let unwound = self.builder.create_block();
        self.builder.set_cold_block(unwound);
        let next = self.builder.create_block();
        self.builder.ins().brif(status, unwound, &[], next, &[]);
        self.builder.switch_to_block(unwound);
        self.builder.seal_block(unwound);
        let mut values = vec![status];
        if let Some(ty) = self.plan.result {
            values.push(
                self.builder
                    .ins()
                    .iconst(Kind::of(ty).expect("planned").ir(), 0),
            );
        }
        self.builder.ins().return_(&values);
        self.builder.switch_to_block(next);
        self.builder.seal_block(next);
    }

    /// An imported call: arguments go to the VM in a stack array, through
    /// the context's callout.
    fn callout(&mut self, pc: usize, instruction: WideInstruction) {
        let Slot::Import(function) = self.plan.operand(pc) else {
            unreachable!("planned imported calls read an import reference");
        };
        let signature = self
            .analysis
            .import_signature(function)
            .expect("planned")
            .clone();
        self.callout_function(function, &signature, instruction);
    }

    /// A statically known Wasm callee that did not pass native planning still
    /// crosses the ordinary VM boundary. Its body can run interpreted (and
    /// can itself enter any compatible compiled function).
    fn callout_known_fallback(&mut self, function: u32, instruction: WideInstruction) {
        let signature = self
            .analysis
            .signature(function)
            .expect("known signature")
            .clone();
        // `CallKnown` stores the defined-function-local index, while the
        // ordinary VM callout and function-reference lookup use Wasm's
        // absolute function index (imports first).
        let absolute_function = self
            .analysis
            .vm
            .programs
            .wasm_signatures(self.analysis.program)
            .and_then(|signatures| {
                u32::try_from(signatures.imports().len())
                    .ok()?
                    .checked_add(function)
            })
            .expect("validated known function index");
        self.callout_function(absolute_function, &signature, instruction);
    }

    fn callout_function(
        &mut self,
        function: u32,
        signature: &crate::WasmSignature,
        instruction: WideInstruction,
    ) {
        let word = types::I64.bytes();
        let signature = signature.clone();
        let words = signature.params.len() + 1;
        let slot = self.builder.create_sized_stack_slot(ir::StackSlotData::new(
            ir::StackSlotKind::ExplicitSlot,
            word * words as u32,
            word.trailing_zeros() as u8,
        ));
        let window = instruction.call_window();
        for (offset, ty) in signature.params.iter().enumerate() {
            let kind = Kind::of(*ty).expect("planned");
            let value = self.read(window.base + offset as u16, kind);
            let value = match kind {
                Kind::I32 => self.builder.ins().uextend(types::I64, value),
                Kind::I64 => value,
            };
            let address =
                self.builder
                    .ins()
                    .stack_addr(types::I64, slot, (offset as u32 * word) as i32);
            self.builder
                .ins()
                .store(MemFlagsData::trusted(), value, address, 0);
        }
        let result_offset = (signature.params.len() as u32 * word) as i32;
        let args = self.builder.ins().stack_addr(types::I64, slot, 0);
        let out = self
            .builder
            .ins()
            .stack_addr(types::I64, slot, result_offset);
        let callout_signature = match self.callout_signature {
            Some(signature) => signature,
            None => {
                let mut callout = Signature::new(self.call_conv);
                for _ in 0..4 {
                    callout.params.push(AbiParam::new(types::I64));
                }
                callout.returns.push(AbiParam::new(types::I64));
                let callout = self.builder.import_signature(callout);
                self.callout_signature = Some(callout);
                callout
            }
        };
        let target =
            self.builder
                .ins()
                .load(types::I64, invariant(), self.context, CONTEXT_CALLOUT);
        let function = self.builder.ins().iconst(types::I64, i64::from(function));
        let call = self.builder.ins().call_indirect(
            callout_signature,
            target,
            &[self.context, function, args, out],
        );
        let status = self.builder.inst_results(call)[0];
        self.propagate(status);
        let destination = instruction.result_register();
        match signature.results.first() {
            Some(ty) => {
                let kind = Kind::of(*ty).expect("planned");
                let bits = self
                    .builder
                    .ins()
                    .load(types::I64, MemFlagsData::trusted(), out, 0);
                let value = match kind {
                    Kind::I32 => self.builder.ins().ireduce(types::I32, bits),
                    Kind::I64 => bits,
                };
                self.write(destination, kind, value);
            }
            None => self.write_constant(destination, Kind::I32, 0),
        }
    }

    /// Keep the table index in native code until the combined VM callout.
    fn indirect_target(&mut self, _pc: usize, instruction: WideInstruction) {
        let index = self.read(instruction.c(), Kind::I32);
        let index = self.builder.ins().uextend(types::I64, index);
        self.write(instruction.a(), Kind::I64, index);
    }

    /// Resolve and call a table reference through one ordinary VM boundary.
    fn call_reference(&mut self, pc: usize, instruction: WideInstruction) {
        let Slot::IndirectReference {
            signature: signature_index,
            table,
        } = self.plan.operand(pc)
        else {
            unreachable!("planned indirect calls retain their table binding");
        };
        let wasm_signature = self
            .analysis
            .type_signature(signature_index)
            .expect("planned")
            .clone();
        let word = types::I64.bytes();
        let words = wasm_signature.params.len() + 1;
        let slot = self.builder.create_sized_stack_slot(ir::StackSlotData::new(
            ir::StackSlotKind::ExplicitSlot,
            word * words as u32,
            word.trailing_zeros() as u8,
        ));
        let window = instruction.call_window();
        for (offset, ty) in wasm_signature.params.iter().enumerate() {
            let kind = Kind::of(*ty).expect("planned");
            let value = self.read(window.base + offset as u16, kind);
            let value = match kind {
                Kind::I32 => self.builder.ins().uextend(types::I64, value),
                Kind::I64 => value,
            };
            let address =
                self.builder
                    .ins()
                    .stack_addr(types::I64, slot, (offset as u32 * word) as i32);
            self.builder
                .ins()
                .store(MemFlagsData::trusted(), value, address, 0);
        }
        let result_offset = (wasm_signature.params.len() as u32 * word) as i32;
        let args = self.builder.ins().stack_addr(types::I64, slot, 0);
        let out = self
            .builder
            .ins()
            .stack_addr(types::I64, slot, result_offset);
        let index = self.read(instruction.b(), Kind::I64);
        let table_slot = self.builder.ins().iconst(types::I64, i64::from(table));
        let signature_index = self
            .builder
            .ins()
            .iconst(types::I64, i64::from(signature_index));
        let site_id = self
            .reference_site_ids
            .binary_search_by_key(&pc, |(site_pc, _)| *site_pc)
            .ok()
            .map(|index| self.reference_site_ids[index].1)
            .expect("compiled indirect call has a cache site id");
        let target_offset = if self.diagnostics_enabled {
            CONTEXT_CALL_REFERENCE_DIAGNOSTIC
        } else {
            CONTEXT_CALL_REFERENCE
        };
        let target = self
            .builder
            .ins()
            .load(types::I64, invariant(), self.context, target_offset);
        let mut call_signature = Signature::new(self.call_conv);
        for _ in 0..7 {
            call_signature.params.push(AbiParam::new(types::I64));
        }
        call_signature.returns.push(AbiParam::new(types::I64));
        let call_signature = self.builder.import_signature(call_signature);
        let site_id = self.builder.ins().iconst(types::I64, site_id as i64);
        let call = self.builder.ins().call_indirect(
            call_signature,
            target,
            &[
                self.context,
                table_slot,
                index,
                signature_index,
                site_id,
                args,
                out,
            ],
        );
        let status = self.builder.inst_results(call)[0];
        self.propagate(status);
        let destination = instruction.result_register();
        match wasm_signature.results.first() {
            Some(ty) => {
                let kind = Kind::of(*ty).expect("planned");
                let bits = self
                    .builder
                    .ins()
                    .load(types::I64, MemFlagsData::trusted(), out, 0);
                let value = match kind {
                    Kind::I32 => self.builder.ins().ireduce(types::I32, bits),
                    Kind::I64 => bits,
                };
                self.write(destination, kind, value);
            }
            None => self.write_constant(destination, Kind::I32, 0),
        }
    }

    /// Emit a jump, polling only for the entry transfer or a loop backedge.
    /// Every bytecode control-flow cycle has a backedge, so acyclic edges do
    /// not need an atomic load to preserve cancellation responsiveness.
    fn jump(&mut self, target: usize, interrupt_check: bool) {
        if interrupt_check {
            self.interrupt_safepoint();
        }
        let mut block = self.edge(target);
        if self.threading
            && Some(block) == self.blocks[target]
            && let Some(thread) = dispatch_path(self.instructions, self.plan, target)
        {
            block = self.builder.create_block();
            self.pending.push((block, thread));
        }
        self.builder.ins().jump(block, &[]);
    }

    /// A conditional transfer from `pc`. It checks for an interrupt when
    /// either edge leads to `pc` or before: a loop repeats through one of
    /// them (`JumpI32Zero` passes its targets swapped, so either may be the
    /// one behind). The check sits before the branch so the backedge stays
    /// direct: routing it through a block of its own measured 10% more
    /// CoreMark instructions. A loop latch (a taken target behind the
    /// fallthrough) leaves the loop through a block of its own, so values
    /// only the exit passes (merge arguments) are computed after the loop,
    /// not on every iteration.
    fn branch(&mut self, pc: usize, condition: ir::Value, target: usize, fallthrough: usize) {
        if target <= pc || fallthrough <= pc {
            self.interrupt_safepoint();
        }
        let taken = self.edge(target);
        let next = self.edge(fallthrough);
        if target >= fallthrough {
            self.builder.ins().brif(condition, taken, &[], next, &[]);
            return;
        }
        let exit = self.builder.create_block();
        self.builder.ins().brif(condition, taken, &[], exit, &[]);
        self.builder.switch_to_block(exit);
        self.builder.ins().jump(next, &[]);
    }

    /// The block a jump from here to `target` enters: a threaded copy when
    /// a known selector decides a branch table ahead, else the target's.
    fn edge(&mut self, target: usize) -> ir::Block {
        if let Some((redirected, block)) = self.redirect
            && redirected == target
        {
            return block;
        }
        if self.threading {
            for (selector, value) in self.known.clone() {
                if let Some(block) = self.thread(target, selector, value) {
                    return block;
                }
            }
        }
        self.block(target)
    }

    fn thread(&mut self, target: usize, selector: u16, value: i32) -> Option<ir::Block> {
        let key = (target, selector, value);
        if let Some((_, block)) = self.threads.iter().find(|(known, _)| *known == key) {
            return *block;
        }
        let thread = thread_path(self.instructions, self.plan, target, selector, value);
        let block = thread.map(|thread| {
            let block = self.builder.create_block();
            self.pending.push((block, thread));
            block
        });
        self.threads.push((key, block));
        block
    }

    /// Emit the threaded copy `thread` into `block`.
    fn emit_thread(&mut self, function: &Function, block: ir::Block, thread: &Thread) {
        self.builder.switch_to_block(block);
        self.checked.clear();
        self.interrupt_safepoint();
        for (index, &pc) in thread.path.iter().enumerate() {
            let instruction = self.instructions[pc];
            match instruction.op().control_flow_layout() {
                // A dispatch copy ends in its table, run for real.
                ControlFlowLayout::BranchTable if matches!(thread.end, ThreadEnd::Dispatch) => {
                    self.instruction(function, pc, instruction);
                }
                // The path continues at the jump's target, or the table's
                // case: nothing to emit.
                ControlFlowLayout::Jump | ControlFlowLayout::BranchTable => {}
                ControlFlowLayout::ConditionalJump => {
                    if decided_branch(instruction, thread.selector, thread.value).is_some() {
                        continue;
                    }
                    let next = thread.path[index + 1];
                    let continuation = self.builder.create_block();
                    self.redirect = Some((next, continuation));
                    self.instruction(function, pc, instruction);
                    self.redirect = None;
                    self.builder.switch_to_block(continuation);
                    self.checked.clear();
                }
                _ => {
                    self.instruction(function, pc, instruction);
                }
            }
        }
        let (ThreadEnd::Case(target) | ThreadEnd::Exit(target)) = thread.end else {
            return;
        };
        let target = self.block(target);
        self.builder.ins().jump(target, &[]);
    }

    /// Emit the instruction at `pc`; returns whether control falls through.
    fn instruction(
        &mut self,
        function: &Function,
        pc: usize,
        instruction: WideInstruction,
    ) -> bool {
        let op = instruction.op();
        let (a, b, c) = (instruction.a(), instruction.b(), instruction.c());
        if let Some(operator) = I32BinaryOperator::from_register_op(op) {
            let left = self.read(b, Kind::I32);
            let right = self.read(c, Kind::I32);
            let value = self.integer_binary(widen_binary(operator), left, right);
            self.write(a, Kind::I32, value);
            return true;
        }
        if let Some(operator) = I32BinaryOperator::from_immediate_op(op) {
            let left = self.read(b, Kind::I32);
            let right = self
                .builder
                .ins()
                .iconst(types::I32, i64::from(instruction.imm() as i32));
            let value = self.integer_binary(widen_binary(operator), left, right);
            self.write(a, Kind::I32, value);
            return true;
        }
        if let Some(operator) = I32BinaryOperator::from_jump_op(op) {
            let left = self.read(a, Kind::I32);
            let right = self.read(b, Kind::I32);
            let holds = self.integer_binary(widen_binary(operator), left, right);
            self.branch(pc, holds, instruction.jump_target() as usize, pc + 1);
            return false;
        }
        if let Some(operator) = I32BinaryOperator::from_immediate_jump_op(op) {
            let left = self.read(a, Kind::I32);
            let right = self.builder.ins().iconst(types::I32, i64::from(b as i16));
            let holds = self.integer_binary(widen_binary(operator), left, right);
            self.branch(pc, holds, instruction.jump_target() as usize, pc + 1);
            return false;
        }
        if let Some(load) = MemoryLoad::from_direct_op(op) {
            let result = load_type(load).expect("planned");
            let kind = Kind::of(result).expect("planned");
            let address = self.read(c, Kind::I32);
            let host = self.address(address, instruction.imm(), load.width());
            let flags = self.flags(Region::LinearMemory);
            let ty = kind.ir();
            let value = match (load.width(), load_signed(load)) {
                // A float loads straight into its view.
                _ if result != kind.integer() => {
                    self.builder.ins().load(scalar_ir(result), flags, host, 0)
                }
                (1, false) => self.builder.ins().uload8(ty, flags, host, 0),
                (1, true) => self.builder.ins().sload8(ty, flags, host, 0),
                (2, false) => self.builder.ins().uload16(ty, flags, host, 0),
                (2, true) => self.builder.ins().sload16(ty, flags, host, 0),
                (4, false) if kind == Kind::I64 => self.builder.ins().uload32(flags, host, 0),
                (4, true) if kind == Kind::I64 => self.builder.ins().sload32(flags, host, 0),
                _ => self.builder.ins().load(ty, flags, host, 0),
            };
            self.write_typed(a, result, value);
            return true;
        }
        if let Some(store) = MemoryStore::from_direct_op(op) {
            let kind = Kind::of(store.value_type()).expect("planned");
            let value = self.read_typed(a, store.value_type());
            let address = self.read(c, Kind::I32);
            let width = store_width(store);
            let host = self.address(address, instruction.imm(), width);
            let flags = self.flags(Region::LinearMemory);
            match width {
                1 => self.builder.ins().istore8(flags, value, host, 0),
                2 => self.builder.ins().istore16(flags, value, host, 0),
                4 if kind == Kind::I64 => self.builder.ins().istore32(flags, value, host, 0),
                _ => self.builder.ins().store(flags, value, host, 0),
            };
            return true;
        }
        if op == Op::WasmMemorySize {
            let bytes = self.builder.ins().load(
                types::I64,
                MemFlagsData::trusted(),
                self.context,
                CONTEXT_MEMORY_LENGTH,
            );
            let pages = self.builder.ins().ushr_imm_u(bytes, 16);
            let pages = self.builder.ins().ireduce(types::I32, pages);
            self.write(a, Kind::I32, pages);
            return true;
        }
        if op == Op::WasmMemoryFill {
            let (destination, _) = instruction.register_pair();
            let destination = self.read(destination, Kind::I32);
            let destination = self.builder.ins().uextend(types::I64, destination);
            let raw_length = self.read(c, Kind::I32);
            let length = self.builder.ins().uextend(types::I64, raw_length);
            let end = self.builder.ins().iadd(destination, length);
            let memory_length = self.builder.ins().load(
                types::I64,
                MemFlagsData::trusted(),
                self.context,
                CONTEXT_MEMORY_LENGTH,
            );
            let out_of_bounds =
                self.builder
                    .ins()
                    .icmp(IntCC::UnsignedGreaterThan, end, memory_length);
            self.trap_if(out_of_bounds, crate::WasmTrap::OutOfBoundsMemory);
            let address = self.builder.ins().iadd(self.base, destination);
            let raw_byte = self.read(b, Kind::I32);
            let byte = self.builder.ins().uextend(types::I64, raw_byte);
            let mut signature = Signature::new(self.call_conv);
            for _ in 0..3 {
                signature.params.push(AbiParam::new(types::I64));
            }
            signature.returns.push(AbiParam::new(types::I64));
            let signature = self.builder.import_signature(signature);
            let target = self.builder.ins().iconst(
                types::I64,
                compiled_memory_fill as *const () as usize as i64,
            );
            self.builder
                .ins()
                .call_indirect(signature, target, &[address, byte, length]);
            return true;
        }
        if let Some((operator, ty)) = float_binary_operator(op, instruction.imm()) {
            let left = self.read_typed(b, ty);
            let right = self.read_typed(c, ty);
            let value = self.float_binary(operator, left, right);
            let result = if self.builder.func.dfg.value_type(value) == types::I32 {
                crate::WasmType::I32
            } else {
                ty
            };
            self.write_typed(a, result, value);
            return true;
        }
        if let Some((operator, ty)) = float_unary_operator(op, instruction.imm()) {
            let input = self.read_typed(b, ty);
            let value = self.float_unary(operator, input);
            self.write_typed(a, ty, value);
            return true;
        }
        match op {
            Op::WasmI64Binary => {
                let operator = I64BinaryOperator::from_tag(instruction.imm()).expect("planned");
                let left = self.read(b, Kind::I64);
                let right = self.read(c, Kind::I64);
                let value = self.integer_binary(operator, left, right);
                let kind = Kind::of_ir(self.builder.func.dfg.value_type(value));
                self.write(a, kind, value);
            }
            Op::WasmI64Unary => {
                let operator = I64UnaryOperator::from_tag(instruction.imm()).expect("planned");
                let input = self.read(b, Kind::I64);
                let value = self.integer_unary(operator, input);
                let kind = Kind::of_ir(self.builder.func.dfg.value_type(value));
                self.write(a, kind, value);
            }
            Op::WasmScalarConvert => {
                let operator =
                    ScalarConversionOperator::from_tag(instruction.imm()).expect("planned");
                let input = self.read_typed(b, operator.source_type());
                let value = self.convert(operator, input);
                self.write_typed(a, operator.result_type(), value);
            }
            Op::Move => {
                let kind = self.operand_kind(pc);
                let value = self.read(b, kind);
                self.write(a, kind, value);
            }
            Op::LoadConst | Op::WasmFillRegisters => {
                let constant = self
                    .analysis
                    .constant(instruction.constant_index())
                    .expect("planned");
                let kind = self.analysis.constant_kind(constant);
                let bits = self.analysis.constant_bits(constant);
                if op == Op::LoadConst {
                    self.write_constant(a, kind, bits);
                } else {
                    let window = instruction.register_window();
                    for offset in 0..window.count {
                        self.write_constant(window.base + offset, kind, bits);
                    }
                }
            }
            Op::WasmI32Unary => {
                let operator = I32UnaryOperator::from_tag(instruction.imm()).expect("planned");
                let input = self.read(b, Kind::I32);
                let value = self.integer_unary(widen_unary(operator), input);
                self.write(a, Kind::I32, value);
            }
            Op::WasmI32ShiftRightUnsignedAndImmediate => {
                let input = self.read(b, Kind::I32);
                let shift = self.builder.ins().iconst(types::I32, i64::from(c as i16));
                let shifted =
                    self.integer_binary(I64BinaryOperator::ShiftRightUnsigned, input, shift);
                let mask = self
                    .builder
                    .ins()
                    .iconst(types::I32, i64::from(instruction.imm() as i32));
                let value = self.integer_binary(I64BinaryOperator::And, shifted, mask);
                self.write(a, Kind::I32, value);
            }
            Op::WasmI32AddImmediateJumpNonZero => {
                let input = self.read(b, Kind::I32);
                let addend = self.builder.ins().iconst(types::I32, i64::from(c as i16));
                let sum = self.builder.ins().iadd(input, addend);
                self.write(a, Kind::I32, sum);
                self.branch(pc, sum, instruction.jump_target() as usize, pc + 1);
                return false;
            }
            Op::WasmJumpI32Zero | Op::WasmJumpI32NonZero => {
                let value = self.read(a, Kind::I32);
                let (taken, other) = (instruction.jump_target() as usize, pc + 1);
                if op == Op::WasmJumpI32NonZero {
                    self.branch(pc, value, taken, other);
                } else {
                    self.branch(pc, value, other, taken);
                }
                return false;
            }
            Op::Jump => {
                let target = instruction.jump_target() as usize;
                self.jump(target, target <= pc);
                return false;
            }
            Op::WasmBranchTable => {
                let selector = self.read(a, Kind::I32);
                let entries: Vec<usize> = crate::bytecode::branch_table_entries(pc, instruction)
                    .map(|entry| {
                        super::dispatch_fast::decoded_instruction(function, function.code[entry])
                            .jump_target() as usize
                    })
                    .collect();
                // A table entry at or before `pc` is a backedge; a loop may
                // repeat through the table alone, so it polls like a jump.
                if entries.iter().any(|&target| target <= pc) {
                    self.interrupt_safepoint();
                }
                let (default, cases) = entries.split_last().expect("a default entry");
                let default = self.block(*default);
                let default = self.builder.func.dfg.block_call(default, &[]);
                let cases: Vec<_> = cases
                    .iter()
                    .map(|target| {
                        let block = self.block(*target);
                        self.builder.func.dfg.block_call(block, &[])
                    })
                    .collect();
                let table = ir::JumpTableData::new(default, &cases);
                let table = self.builder.create_jump_table(table);
                self.builder.ins().br_table(selector, table);
                return false;
            }
            Op::WasmSelect => {
                let (condition, _) = instruction.register_pair();
                let kind = self.operand_kind(pc);
                let condition = self.read(condition, Kind::I32);
                let taken = self.read(b, kind);
                let other = self.read(c, kind);
                let value = self.builder.ins().select(condition, taken, other);
                self.write(a, kind, value);
            }
            Op::CallKnown => {
                let callee = usize::from(instruction.known_function_index());
                let Some(callee_plan) = self.plans[callee].as_ref() else {
                    self.callout_known_fallback(callee as u32, instruction);
                    return true;
                };
                let window = instruction.call_window();
                let mut args = vec![self.context];
                if self.memory.base {
                    args.push(self.base);
                }
                args.extend(self.length);
                for (offset, ty) in callee_plan.params.iter().enumerate() {
                    let kind = Kind::of(*ty).expect("planned");
                    args.push(self.read(window.base + offset as u16, kind));
                }
                // Compiled functions share one code buffer: a direct call,
                // linked at placement (`link`).
                let target = match self.callees[callee] {
                    Some(target) => target,
                    None => {
                        let signature = internal_signature(
                            self.memory,
                            &callee_plan.params,
                            callee_plan.result,
                            self.call_conv,
                        );
                        let signature = self.builder.import_signature(signature);
                        let name = self
                            .builder
                            .func
                            .declare_imported_user_function(function_name(callee));
                        let target = self.builder.import_function(ir::ExtFuncData {
                            name: ir::ExternalName::User(name),
                            signature,
                            colocated: true,
                            patchable: false,
                        });
                        self.callees[callee] = Some(target);
                        target
                    }
                };
                let call = self.builder.ins().call(target, &args);
                let results = self.builder.inst_results(call).to_vec();
                self.propagate(results[0]);
                let destination = instruction.result_register();
                match callee_plan.result {
                    Some(ty) => self.write(destination, Kind::of(ty).expect("planned"), results[1]),
                    None => {
                        let zero = self.builder.ins().iconst(types::I32, 0);
                        self.write(destination, Kind::I32, zero);
                    }
                }
            }
            Op::WasmRefFunc => {}
            Op::WasmIndirectTarget => self.indirect_target(pc, instruction),
            Op::Call => match self.plan.operand(pc) {
                Slot::Import(_) => self.callout(pc, instruction),
                Slot::IndirectReference { .. } => self.call_reference(pc, instruction),
                _ => unreachable!("planned calls hold a supported callable"),
            },
            Op::WasmUnreachable => {
                let trap = self.trap_block(crate::WasmTrap::Unreachable);
                self.builder.ins().jump(trap, &[]);
                return false;
            }
            Op::Return => {
                let status = self.builder.ins().iconst(types::I64, 0);
                let mut values = vec![status];
                if let Some(ty) = self.plan.result {
                    values.push(self.read(a, Kind::of(ty).expect("planned")));
                }
                self.builder.ins().return_(&values);
                return false;
            }
            // A binding is known statically per instruction; the globals it
            // names are reached through the context.
            Op::WasmInstanceBinding => {}
            Op::WasmGlobalGet => {
                let slot = self.binding_slot(pc);
                let address = self.global_address(slot);
                let flags = self.flags(Region::Globals);
                let value = self.builder.ins().load(types::I32, flags, address, 0);
                self.write(a, Kind::I32, value);
            }
            Op::WasmGlobalSet => {
                let slot = self.binding_slot(pc);
                let address = self.global_address(slot);
                let value = self.read(a, Kind::I32);
                let wide = self.builder.ins().uextend(types::I64, value);
                let encoded = self.builder.ins().bor_imm_u(wide, INTEGER_TAG as i64);
                let flags = self.flags(Region::Globals);
                self.builder.ins().store(flags, encoded, address, 0);
            }
            _ => unreachable!("planned instructions have a denotation"),
        }
        true
    }
}

impl<'a, H: Host> Analysis<'a, H> {
    /// Compile `index` with `plan`; `None` when Cranelift refuses it or its
    /// code needs a relocation other than a call to a compiled function.
    fn compile_function(
        &self,
        isa: &dyn cranelift_codegen::isa::TargetIsa,
        index: usize,
        plan: &Plan,
        plans: &[Option<Plan>],
        guarded: bool,
        wraps: bool,
        fixed_base: Option<i32>,
        globals: &[u32],
        reference_site_ids: &'a [(usize, usize)],
        diagnostics_enabled: bool,
    ) -> Option<(Vec<u8>, Vec<Call>)> {
        let function = &self.code.functions[index];
        let call_conv = isa.default_call_conv();
        let memory = MemoryArguments::of(fixed_base, guarded);
        let mut func = ir::Function::with_name_signature(
            UserFuncName::User(function_name(index)),
            internal_signature(memory, &plan.params, plan.result, call_conv),
        );
        let mut builder_context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_context);
        let variables: Vec<[Variable; VIEW_TYPES.len()]> = (0..function.registers)
            .map(|_| VIEW_TYPES.map(|ty| builder.declare_var(scalar_ir(ty))))
            .collect();
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        let parameters = builder.block_params(entry).to_vec();
        let context = parameters[0];
        let mut passed = parameters[1..].iter().copied();
        // A constant base folds into each access's addressing mode and
        // frees the register a parameter would occupy.
        let base = match fixed_base {
            Some(fixed) => builder.ins().iconst(types::I64, i64::from(fixed)),
            None => passed.next().expect("a passed base"),
        };
        let length = memory
            .length
            .then(|| passed.next().expect("a passed length"));
        let wasm_parameters = &parameters[1 + memory.count()..];
        let regions = Region::ALL.map(|region| {
            builder.func.dfg.alias_regions.insert(ir::AliasRegionData {
                user_id: region as u32,
                description: format!("{region:?}").into(),
            })
        });
        let instructions: Vec<WideInstruction> = function
            .code
            .iter()
            .map(|packed| super::dispatch_fast::decoded_instruction(function, *packed))
            .collect();
        let mut selectors: Vec<u16> = instructions
            .iter()
            .enumerate()
            .filter(|(pc, instruction)| {
                plan.reachable(*pc) && instruction.op() == Op::WasmBranchTable
            })
            .map(|(_, instruction)| instruction.a())
            .collect();
        selectors.sort_unstable();
        selectors.dedup();
        let mut emitter = Emitter {
            guarded,
            wraps,
            regions,
            globals,
            analysis: self,
            builder,
            plan,
            variables,
            blocks: vec![None; function.code.len()],
            context,
            reference_site_ids,
            diagnostics_enabled,
            base,
            length,
            memory,
            trap_blocks: [None; TRAPS.len()],
            termination_block: None,
            callees: vec![None; plans.len()],
            callout_signature: None,
            plans,
            call_conv,
            instructions: &instructions,
            selectors,
            known: Vec::new(),
            threads: Vec::new(),
            pending: Vec::new(),
            redirect: None,
            threading: true,
            poll: guard_support()
                .and_then(|_| self.vm.execution_interrupt.poll_page())
                .map(|(start, _)| start as i64),
            checked: Vec::new(),
        };
        // Registers start at i32 zero, as an activation's register file does.
        for register in 0..function.registers {
            emitter.write_constant(register, Kind::I32, 0);
            emitter.write_constant(register, Kind::I64, 0);
        }
        let parameter_base = function.parameter_registers.expect("planned");
        for (offset, ty) in plan.params.iter().enumerate() {
            let kind = Kind::of(*ty).expect("planned");
            emitter.write(
                parameter_base + offset as u16,
                kind,
                wasm_parameters[offset],
            );
        }
        // Each activation checks the stack it may still use.
        let stack_pointer = emitter.builder.ins().get_stack_pointer(types::I64);
        let limit =
            emitter
                .builder
                .ins()
                .load(types::I64, invariant(), context, CONTEXT_STACK_LIMIT);
        let exhausted = emitter
            .builder
            .ins()
            .icmp(IntCC::UnsignedLessThan, stack_pointer, limit);
        emitter.trap_if(exhausted, crate::WasmTrap::CallStackExhausted);
        emitter.known.clear();
        emitter.checked.clear();
        emitter.jump(0, true);
        // Every jump target starts a block, created before emission so a
        // backward jump finds its target's block already placed.
        for (pc, instruction) in instructions.iter().copied().enumerate() {
            if !plan.reachable(pc) {
                continue;
            }
            let targets = match instruction.op().control_flow_layout() {
                ControlFlowLayout::BranchTable => {
                    crate::bytecode::branch_table_entries(pc, instruction)
                        .map(|entry| instructions[entry].jump_target() as usize)
                        .collect()
                }
                ControlFlowLayout::Jump | ControlFlowLayout::ConditionalJump => {
                    vec![instruction.jump_target() as usize, pc + 1]
                }
                _ => vec![],
            };
            for target in targets {
                if target < instructions.len() && plan.reachable(target) {
                    emitter.block(target);
                }
            }
        }
        let mut open = false;
        for (pc, instruction) in instructions.iter().copied().enumerate() {
            if !plan.reachable(pc) {
                open = false;
                continue;
            }
            let is_leader = emitter.blocks[pc].is_some() || !open;
            if is_leader {
                emitter.known.clear();
                emitter.checked.clear();
                let block = emitter.block(pc);
                if open {
                    emitter.builder.ins().jump(block, &[]);
                }
                emitter.builder.switch_to_block(block);
            }
            open = emitter.instruction(function, pc, instruction);
            // A later jump may target the next instruction; give it a block
            // now so falling into it stays explicit.
            if open && emitter.blocks.get(pc + 1).is_some_and(Option::is_some) {
                let next = emitter.block(pc + 1);
                emitter.builder.ins().jump(next, &[]);
                open = false;
            }
        }
        if open {
            return None;
        }
        emitter.threading = false;
        for (block, thread) in std::mem::take(&mut emitter.pending) {
            emitter.emit_thread(function, block, &thread);
        }
        emitter.finish_traps();
        emitter.builder.seal_all_blocks();
        emitter.builder.finalize(isa.frontend_config());
        let mut context = cranelift_codegen::Context::for_function(func);
        let compiled = context
            .compile(
                isa,
                &mut cranelift_codegen::control::ControlPlane::default(),
            )
            .ok()?;
        let code = compiled.code_buffer().to_vec();
        let relocations = compiled.buffer.relocs().to_vec();
        let names = context.func.params.user_named_funcs();
        let calls = relocations
            .iter()
            .map(|relocation| {
                let cranelift_codegen::FinalizedRelocTarget::ExternalName(ir::ExternalName::User(
                    name,
                )) = relocation.target
                else {
                    return None;
                };
                let name = names.get(name)?;
                (name.namespace == FUNCTION_NAMESPACE).then_some(Call {
                    offset: relocation.offset as usize,
                    kind: relocation.kind,
                    addend: relocation.addend,
                    callee: name.index as usize,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some((code, calls))
    }

    /// The C entry of `index`: unpack arguments, call through the table and
    /// store the result.
    fn compile_entry(
        &self,
        isa: &dyn cranelift_codegen::isa::TargetIsa,
        index: usize,
        plan: &Plan,
        memory: MemoryArguments,
    ) -> Option<Vec<u8>> {
        let call_conv = isa.default_call_conv();
        let mut signature = Signature::new(call_conv);
        for _ in 0..5 {
            signature.params.push(AbiParam::new(types::I64));
        }
        signature.returns.push(AbiParam::new(types::I32));
        let mut func = ir::Function::with_name_signature(
            UserFuncName::user(ENTRY_NAMESPACE, index as u32),
            signature,
        );
        let mut builder_context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_context);
        let block = builder.create_block();
        builder.append_block_params_for_function_params(block);
        builder.switch_to_block(block);
        builder.seal_block(block);
        let parameters = builder.block_params(block).to_vec();
        let (context, base, length, args, out) = (
            parameters[0],
            parameters[1],
            parameters[2],
            parameters[3],
            parameters[4],
        );
        let mut call_args = vec![context];
        call_args.extend(memory.select(base, length));
        for (offset, ty) in plan.params.iter().enumerate() {
            let raw = builder.ins().load(
                types::I64,
                MemFlagsData::trusted(),
                args,
                (offset * 8) as i32,
            );
            call_args.push(match Kind::of(*ty).expect("planned") {
                Kind::I32 => builder.ins().ireduce(types::I32, raw),
                Kind::I64 => raw,
            });
        }
        let target_signature = builder.import_signature(internal_signature(
            memory,
            &plan.params,
            plan.result,
            call_conv,
        ));
        let table = builder
            .ins()
            .load(types::I64, MemFlagsData::trusted(), context, CONTEXT_TABLE);
        let target = builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            table,
            (index * 8) as i32,
        );
        let call = builder
            .ins()
            .call_indirect(target_signature, target, &call_args);
        let results = builder.inst_results(call).to_vec();
        if let Some(ty) = plan.result {
            let value = match Kind::of(ty).expect("planned") {
                Kind::I32 => builder.ins().uextend(types::I64, results[1]),
                Kind::I64 => results[1],
            };
            builder.ins().store(MemFlagsData::trusted(), value, out, 0);
        }
        let status = builder.ins().ireduce(types::I32, results[0]);
        builder.ins().return_(&[status]);
        builder.finalize(isa.frontend_config());
        let mut context = cranelift_codegen::Context::for_function(func);
        let compiled = context
            .compile(
                isa,
                &mut cranelift_codegen::control::ControlPlane::default(),
            )
            .ok()?;
        if !compiled.buffer.relocs().is_empty() {
            return None;
        }
        Some(compiled.code_buffer().to_vec())
    }
}

/// The guard reservation of an unshared memory binding.
/// A memory base an x86-64 addressing mode can carry as its signed 32-bit
/// displacement.
fn displacement_base(base: *mut u8) -> Option<i32> {
    i32::try_from(base as usize).ok()
}

fn guarded_memory<H: Host>(vm: &Vm<H>, binding: Value) -> Option<(*mut u8, usize)> {
    memory_bytes(vm, binding, crate::wasm::memory::LinearBytes::guard)
}

/// Whether the guarded memory `binding` names repeats above 2^32.
fn wrapping_memory<H: Host>(vm: &Vm<H>, binding: Value) -> bool {
    memory_bytes(vm, binding, |bytes| Some(bytes.wraps())) == Some(true)
}

/// `query` of the bytes of the unshared memory `binding` names.
fn memory_bytes<H: Host, T>(
    vm: &Vm<H>,
    binding: Value,
    query: impl FnOnce(&crate::wasm::memory::LinearBytes) -> Option<T>,
) -> Option<T> {
    let Some(Cell::WasmMemory { bytes, .. }) = vm.heap.get(binding) else {
        return None;
    };
    let crate::wasm::memory::MemoryStorage::Unshared(state) = &**bytes else {
        return None;
    };
    query(&state.try_borrow().ok()?.bytes)
}

thread_local! {
    /// The context of the compiled call running on this thread, which a
    /// guard fault resumes from.
    static ACTIVE_CONTEXT: std::cell::Cell<*mut CallContext> =
        const { std::cell::Cell::new(std::ptr::null_mut()) };
}

/// Enters compiled code: (context, base, length, arguments, result, entry)
/// -> status. It saves the caller's callee-saved registers, records its
/// stack pointer in the context, and calls `entry`; a guard fault resumes at
/// its landing, which returns the out-of-bounds status through the same
/// epilogue.
type Trampoline =
    unsafe extern "C" fn(*mut CallContext, u64, u64, *const u64, *mut u64, Entry) -> u32;

/// The trampoline and its landing, once the fault handler is installed.
struct GuardSupport {
    _code: CodeMemory,
    trampoline: Trampoline,
    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    landing: usize,
    /// Where a fault on the poll page resumes: the terminated status.
    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    termination: usize,
}

// SAFETY: immutable executable code, shared by every thread.
unsafe impl Send for GuardSupport {}
unsafe impl Sync for GuardSupport {}

/// The x86-64 System V trampoline, as machine code. Offsets of `RESUME` and
/// `LANDING` are into this array.
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
const TRAMPOLINE_CODE: [u8; 60] = [
    0x55, // push rbp
    0x48,
    0x89,
    0xe5, // mov rbp, rsp
    0x53, // push rbx
    0x41,
    0x54, // push r12
    0x41,
    0x55, // push r13
    0x41,
    0x56, // push r14
    0x41,
    0x57, // push r15
    0x48,
    0x83,
    0xec,
    0x08, // sub rsp, 8 (keeps the call aligned)
    0x48,
    0x89,
    0x67,
    CONTEXT_ENTRY_STACK, // mov [rdi + entry_stack], rsp
    0x41,
    0xff,
    0xd1, // call r9
    // RESUME (24):
    0x48,
    0x83,
    0xc4,
    0x08, // add rsp, 8
    0x41,
    0x5f, // pop r15
    0x41,
    0x5e, // pop r14
    0x41,
    0x5d, // pop r13
    0x41,
    0x5c, // pop r12
    0x5b, // pop rbx
    0x5d, // pop rbp
    0xc3, // ret
    // LANDING (39):
    0xb8,
    0,
    0,
    0,
    0, // mov eax, status (patched)
    0xe9,
    0,
    0,
    0,
    0, // jmp RESUME (patched)
    // TERMINATION (49):
    0xb8,
    0,
    0,
    0,
    0, // mov eax, terminated status (patched)
    0xe9,
    0,
    0,
    0,
    0,    // jmp RESUME (patched)
    0xcc, // int3
];

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
const TRAMPOLINE_RESUME: usize = 24;
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
const TRAMPOLINE_LANDING: usize = 39;
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
const TRAMPOLINE_TERMINATION: usize = 49;

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
static PREVIOUS_SEGV: std::sync::OnceLock<libc::sigaction> = std::sync::OnceLock::new();

/// The guard support of this host, installing the fault handler on first
/// use; `None` where guard faults cannot be resumed.
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
fn guard_support() -> Option<&'static GuardSupport> {
    static SUPPORT: std::sync::OnceLock<Option<GuardSupport>> = std::sync::OnceLock::new();
    SUPPORT
        .get_or_init(|| {
            let mut code = TRAMPOLINE_CODE;
            // Each landing loads its status and jumps back to the epilogue.
            for (landing, status) in [
                (
                    TRAMPOLINE_LANDING,
                    trap_status(crate::WasmTrap::OutOfBoundsMemory) as u32,
                ),
                (TRAMPOLINE_TERMINATION, TERMINATED_STATUS as u32),
            ] {
                code[landing + 1..landing + 5].copy_from_slice(&status.to_le_bytes());
                let after_jump = landing + 10;
                let displacement = TRAMPOLINE_RESUME as i32 - after_jump as i32;
                code[landing + 6..landing + 10].copy_from_slice(&displacement.to_le_bytes());
            }
            let memory = CodeMemory::new(&code)?;
            // SAFETY: the code implements `Trampoline` for this ABI.
            let trampoline = unsafe { std::mem::transmute::<*mut u8, Trampoline>(memory.pointer) };
            let landing = memory.pointer as usize + TRAMPOLINE_LANDING;
            let termination = memory.pointer as usize + TRAMPOLINE_TERMINATION;
            // SAFETY: installs a handler that only redirects faults inside an
            // active compiled call's guard and forwards every other fault to
            // the previous handler.
            unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = guard_fault as *const () as usize;
                action.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK | libc::SA_NODEFER;
                libc::sigemptyset(&mut action.sa_mask);
                let mut previous: libc::sigaction = std::mem::zeroed();
                if libc::sigaction(libc::SIGSEGV, &action, &mut previous) != 0 {
                    return None;
                }
                let _ = PREVIOUS_SEGV.set(previous);
            }
            Some(GuardSupport {
                _code: memory,
                trampoline,
                landing,
                termination,
            })
        })
        .as_ref()
}

#[cfg(not(all(target_arch = "x86_64", target_os = "linux")))]
fn guard_support() -> Option<&'static GuardSupport> {
    None
}

fn guard_trampoline() -> Option<Trampoline> {
    guard_support().map(|support| support.trampoline)
}

/// A fault inside the running compiled call's guard reservation resumes at
/// the trampoline's landing with the trampoline's saved stack; anything else
/// belongs to the previous handler.
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
extern "C" fn guard_fault(signal: i32, info: *mut libc::siginfo_t, context: *mut libc::c_void) {
    let active = ACTIVE_CONTEXT.with(std::cell::Cell::get);
    // SAFETY: the kernel passes valid signal information and context.
    unsafe {
        let address = (*info).si_addr() as usize;
        if !active.is_null()
            && let Some(support) = guard_support()
        {
            let landing = if (*active).guard_start <= address && address < (*active).guard_end {
                Some(support.landing)
            } else if (*active).poll_start <= address && address < (*active).poll_end {
                Some(support.termination)
            } else {
                None
            };
            if let Some(landing) = landing {
                let context = &mut *context.cast::<libc::ucontext_t>();
                context.uc_mcontext.gregs[libc::REG_RSP as usize] = (*active).entry_stack as i64;
                context.uc_mcontext.gregs[libc::REG_RIP as usize] = landing as i64;
                return;
            }
        }
        let Some(previous) = PREVIOUS_SEGV.get() else {
            return;
        };
        match previous.sa_sigaction {
            libc::SIG_DFL | libc::SIG_IGN => {
                // Restore the previous disposition; the fault repeats on return.
                libc::sigaction(libc::SIGSEGV, previous, std::ptr::null_mut());
            }
            handler if previous.sa_flags & libc::SA_SIGINFO != 0 => {
                let handler = std::mem::transmute::<
                    usize,
                    extern "C" fn(i32, *mut libc::siginfo_t, *mut libc::c_void),
                >(handler);
                handler(signal, info, context);
            }
            handler => {
                let handler = std::mem::transmute::<usize, extern "C" fn(i32)>(handler);
                handler(signal);
            }
        }
    }
}

/// The name namespace of compiled function bodies; their C entries use
/// `ENTRY_NAMESPACE`.
const FUNCTION_NAMESPACE: u32 = 0;
const ENTRY_NAMESPACE: u32 = 1;

fn function_name(index: usize) -> ir::UserExternalName {
    ir::UserExternalName::new(FUNCTION_NAMESPACE, index as u32)
}

/// A direct call in a function body, linked once bodies are placed.
struct Call {
    /// The relocated field, from the start of the body.
    offset: usize,
    kind: cranelift_codegen::binemit::Reloc,
    addend: i64,
    callee: usize,
}

/// Point `call` in the body at `body` to the callee placed at `callee`; all
/// are offsets in one buffer, so the relative displacement is final.
fn link(bytes: &mut [u8], body: usize, call: &Call, callee: usize) -> Option<()> {
    use cranelift_codegen::binemit::Reloc;
    let site = body + call.offset;
    let displacement = callee as i64 - site as i64 + call.addend;
    match call.kind {
        Reloc::X86CallPCRel4 => {
            let field = i32::try_from(displacement).ok()?;
            bytes
                .get_mut(site..site + size_of::<i32>())?
                .copy_from_slice(&field.to_le_bytes());
        }
        Reloc::Arm64Call => {
            /// A BL immediate: a signed word displacement in the low bits.
            const IMMEDIATE_BITS: u32 = 26;
            const WORD_SHIFT: u32 = 2;
            let words = displacement >> WORD_SHIFT;
            let range = 1i64 << (IMMEDIATE_BITS - 1);
            if displacement & ((1 << WORD_SHIFT) - 1) != 0 || !(-range..range).contains(&words) {
                return None;
            }
            let field = bytes.get_mut(site..site + size_of::<u32>())?;
            let mask = (1u32 << IMMEDIATE_BITS) - 1;
            let instruction = u32::from_le_bytes(field.try_into().ok()?);
            let instruction = (instruction & !mask) | (words as u32 & mask);
            field.copy_from_slice(&instruction.to_le_bytes());
        }
        _ => return None,
    }
    Some(())
}

/// Code alignment for each compiled function.
const FUNCTION_ALIGNMENT: usize = 64;

fn target_isa(opt_level: &str) -> Option<std::sync::Arc<dyn cranelift_codegen::isa::TargetIsa>> {
    let mut flags = settings::builder();
    flags.set("opt_level", opt_level).ok()?;
    flags.set("enable_verifier", "false").ok()?;
    let builder = cranelift_native::builder().ok()?;
    builder.finish(settings::Flags::new(flags)).ok()
}

impl<H: Host> Vm<H> {
    pub(crate) fn set_execution_mode(&mut self, mode: ExecutionMode) {
        self.execution_mode = mode;
    }

    pub(crate) fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }

    /// Try the per-function JavaScript tier at an already-initialized frame's
    /// entry. A guard miss leaves the frame untouched so the interpreter can
    /// run from the same bytecode PC.
    pub(super) fn js_jit_at_entry(
        &mut self,
        program: &ResidualProgram,
        frame: usize,
    ) -> Option<(usize, crate::bytecode::Register, Value)> {
        if cfg!(any(
            feature = "profile-aggregate",
            feature = "profile-trace",
            feature = "profile-memory",
            feature = "profile-gc-time"
        )) || self.execution_mode != ExecutionMode::Compiled
            || program.kind == crate::bytecode::ProgramKind::Wasm
            || self.frames.get(frame)?.pc != 0
        {
            return None;
        }
        let program_id = self.frames[frame].program;
        let function_id = self.frames[frame].function;
        let function = program.functions.get(function_id as usize)?;
        let tier = self.programs.js_tier(program_id, function_id);
        if matches!(
            tier.as_ref(),
            Some(super::program_store::JsTierState::Unsupported)
        ) {
            return None;
        }
        if tier.is_none() && !super::js_compiled::function_candidate(function, program) {
            return None;
        }

        let params = usize::from(function.params);
        let registers = self.frames[frame].registers.get(..params)?;
        let mut arguments: smallvec::SmallVec<[f64; 8]> = smallvec::SmallVec::new();
        let mut numeric = true;
        for value in registers {
            match value.as_number() {
                Some(value) => arguments.push(value),
                None => {
                    numeric = false;
                    break;
                }
            }
        }

        let compiled = match tier {
            Some(super::program_store::JsTierState::Compiled(compiled)) => {
                if !numeric {
                    return None;
                }
                compiled
            }
            Some(super::program_store::JsTierState::Unsupported) => return None,
            Some(super::program_store::JsTierState::Hot { .. }) | None => {
                if !self
                    .programs
                    .note_js_function_entry(program_id, function_id, numeric)
                    || !numeric
                {
                    return None;
                }
                let residual = self.programs.get(program_id)?;
                let compiled = super::js_compiled::compile(&residual, function_id)
                    .and_then(CompiledJsFunction::from_artifact)
                    .map(Rc::new);
                self.programs
                    .install_js_tier(program_id, function_id, compiled.clone());
                compiled?
            }
        };

        if compiled.params() != params {
            return None;
        }

        match compiled.call(&arguments) {
            CompiledJsResult::Number(value) => {
                let last_pc = function.code.len().checked_sub(1)?;
                let terminal = function.code.get(last_pc)?.as_wide();
                if terminal.op() != Op::Return {
                    return None;
                }
                Some((last_pc, terminal.register_a(), Value::number(value)))
            }
            CompiledJsResult::StringReturnPc(return_pc) => {
                let return_pc = usize::try_from(return_pc).ok()?;
                let instruction = function.code.get(return_pc)?.as_wide();
                if instruction.op() != Op::Return {
                    return None;
                }
                let constant = compiled.string_constant_for_return(return_pc as u32)?;
                let value = self.programs.constant(program_id, constant as usize)?;
                Some((return_pc, instruction.register_a(), value))
            }
        }
    }

    /// Compile every function of `program` that has a compiled denotation,
    /// with `env` as the instance whose bindings fix global types and memory.
    fn compile_program(&self, program: ProgramId, env: Value) -> Option<CompiledProgram> {
        self.compile_program_with_diagnostics(program, env, jit_diagnostics_enabled())
    }

    fn compile_program_with_diagnostics(
        &self,
        program: ProgramId,
        env: Value,
        diagnostics_enabled: bool,
    ) -> Option<CompiledProgram> {
        let root = jit_root_override().ok()?;
        self.compile_program_with_diagnostics_and_root(program, env, diagnostics_enabled, root)
    }

    fn compile_program_with_diagnostics_and_root(
        &self,
        program: ProgramId,
        env: Value,
        diagnostics_enabled: bool,
        root: Option<usize>,
    ) -> Option<CompiledProgram> {
        let code = self.programs.get(program)?;
        if code.kind != crate::bytecode::ProgramKind::Wasm {
            return None;
        }
        let compile_started = diagnostics_enabled.then(std::time::Instant::now);
        let compile_rejections =
            diagnostics_enabled.then(|| std::cell::RefCell::new(vec![None; code.functions.len()]));
        let count = code.functions.len();
        let allowed = if let Some(root) = root {
            let mut selected = vec![false; count];
            let mut pending = vec![root];
            while let Some(index) = pending.pop() {
                let Some(selected_function) = selected.get_mut(index) else {
                    if diagnostics_enabled {
                        eprintln!(
                            "QUENCH_WASM_JIT_ROOT_REJECTION root={root} invalid-function={index}"
                        );
                    }
                    return None;
                };
                if *selected_function {
                    continue;
                }
                let function = code.functions.get(index)?;
                if function.code.len() > MAX_JIT_ROOT_BYTECODES_PER_FUNCTION {
                    if diagnostics_enabled {
                        eprintln!(
                            "QUENCH_WASM_JIT_ROOT_REJECTION root={root} function={index} bytecodes={} limit={MAX_JIT_ROOT_BYTECODES_PER_FUNCTION}",
                            function.code.len()
                        );
                    }
                    return None;
                }
                *selected_function = true;
                for packed in &function.code {
                    let instruction = super::dispatch_fast::decoded_instruction(function, *packed);
                    if instruction.op() == Op::CallKnown {
                        pending.push(usize::from(instruction.known_function_index()));
                    }
                }
            }
            let selected_count = selected.iter().filter(|selected| **selected).count();
            if selected_count > MAX_JIT_ROOT_FUNCTIONS {
                if diagnostics_enabled {
                    eprintln!(
                        "QUENCH_WASM_JIT_ROOT_REJECTION root={root} functions={selected_count} limit={MAX_JIT_ROOT_FUNCTIONS}"
                    );
                }
                return None;
            }
            selected
        } else {
            vec![true; count]
        };
        let mut analysis = Analysis {
            vm: self,
            program,
            code: &code,
            env,
            callouts: true,
            experimental_root_compilation: root.is_some(),
            compile_rejections: compile_rejections.as_ref(),
        };
        // The callable set is the greatest fixpoint: assume every function
        // compiles, drop those that cannot, until nothing changes.
        let mut callable;
        let mut plans: Vec<Option<Plan>>;
        let mut memory_slot;
        let mut guarded;
        loop {
            callable = allowed.clone();
            loop {
                memory_slot = None;
                plans = (0..count)
                    .map(|index| {
                        if callable[index] {
                            analysis
                                .plan(index, &callable, &mut memory_slot)
                                .map(Plan::interface)
                        } else {
                            None
                        }
                    })
                    .collect();
                let next: Vec<bool> = plans.iter().map(Option::is_some).collect();
                if next == callable {
                    break;
                }
                callable = next;
            }
            // Accesses may rely on the guard when the memory has one and
            // this host resumes guard faults.
            guarded = guard_trampoline().is_some()
                && memory_slot.is_some_and(|slot| {
                    self.heap
                        .environment_slot(env, slot as usize)
                        .and_then(|binding| guarded_memory(self, binding))
                        .is_some()
                });
            // Callouts need a memory whose bytes stay put.
            if !analysis.callouts || guarded || memory_slot.is_none() {
                break;
            }
            analysis.callouts = false;
        }
        if plans.iter().all(Option::is_none) {
            return None;
        }
        if root.is_some_and(|root| plans.get(root).is_none_or(Option::is_none)) {
            return None;
        }
        let fixed_base = guarded
            .then(|| {
                let binding = self.heap.environment_slot(env, memory_slot? as usize)?;
                displacement_base(guarded_memory(self, binding)?.0)
            })
            .flatten();
        let wraps = guarded
            && memory_slot
                .and_then(|slot| self.heap.environment_slot(env, slot as usize))
                .is_some_and(|binding| wrapping_memory(self, binding));
        let opt_level = jit_root_opt_level(root).ok()?;
        if diagnostics_enabled {
            eprintln!("QUENCH_WASM_JIT_OPT_LEVEL {opt_level}");
        }
        let isa = target_isa(opt_level)?;
        // The full plan of a callable function, derived again where needed.
        let plan = |index: usize| analysis.plan(index, &callable, &mut memory_slot.clone());
        let mut reference_site_ids = vec![Vec::new(); count];
        let mut reference_call_sites = Vec::new();
        let mut reference_cache_count = 0usize;
        for index in 0..count {
            if plans[index].is_none() {
                continue;
            }
            let full_plan = plan(index)?;
            let function = &code.functions[index];
            for (pc, packed) in function.code.iter().enumerate() {
                let instruction = super::dispatch_fast::decoded_instruction(function, *packed);
                if instruction.op() == Op::Call
                    && matches!(full_plan.operand(pc), Slot::IndirectReference { .. })
                {
                    let site_id = reference_cache_count;
                    reference_cache_count += 1;
                    reference_site_ids[index].push((pc, site_id));
                    if diagnostics_enabled {
                        reference_call_sites.push(ReferenceCallSiteDiagnostics {
                            function: index,
                            pc,
                            counts: std::cell::Cell::new(ReferenceCallSiteCounts::default()),
                        });
                    }
                }
            }
        }
        // The globals compiled code accesses, each once.
        let mut globals: Vec<u32> = Vec::new();
        for index in 0..count {
            if plans[index].is_none() {
                continue;
            }
            let plan = plan(index)?;
            let function = &code.functions[index];
            for (pc, packed) in function.code.iter().enumerate() {
                let instruction = super::dispatch_fast::decoded_instruction(function, *packed);
                if !matches!(instruction.op(), Op::WasmGlobalGet | Op::WasmGlobalSet) {
                    continue;
                }
                if let Some(Slot::Binding(slot)) = plan.operands[pc]
                    && !globals.contains(&slot)
                {
                    globals.push(slot);
                }
            }
        }
        let mut bytes = Vec::new();
        let mut offsets = vec![None; count];
        let mut entries = vec![None; count];
        let mut calls = Vec::new();
        let place = |bytes: &mut Vec<u8>, code: Vec<u8>| {
            bytes.resize(bytes.len().next_multiple_of(FUNCTION_ALIGNMENT), 0);
            let offset = bytes.len();
            bytes.extend_from_slice(&code);
            offset
        };
        let projected_code_len = |current: usize, appended: usize| {
            current
                .checked_add(FUNCTION_ALIGNMENT - 1)
                .map(|aligned| aligned / FUNCTION_ALIGNMENT * FUNCTION_ALIGNMENT)?
                .checked_add(appended)
        };
        for index in 0..count {
            if plans[index].is_none() {
                continue;
            }
            let full_plan = plan(index)?;
            let body_compile_started = diagnostics_enabled.then(std::time::Instant::now);
            let body_memory_before = body_compile_started.and_then(|_| jit_memory_snapshot());
            let body_result = analysis.compile_function(
                &*isa,
                index,
                &full_plan,
                &plans,
                guarded,
                wraps,
                fixed_base,
                &globals,
                &reference_site_ids[index],
                diagnostics_enabled,
            );
            if let Some(started) = body_compile_started {
                let memory_after = jit_memory_snapshot();
                eprintln!(
                    "QUENCH_WASM_JIT_BODY_MEMORY function={index} elapsed_us={} rss_kib_before={:?} rss_kib_after={:?} hwm_kib_before={:?} hwm_kib_after={:?}",
                    started.elapsed().as_micros(),
                    body_memory_before.map(|memory| memory.0),
                    memory_after.map(|memory| memory.0),
                    body_memory_before.map(|memory| memory.1),
                    memory_after.map(|memory| memory.1),
                );
            }
            let (body, function_calls) = body_result?;
            let body_projected_len = projected_code_len(bytes.len(), body.len())?;
            if root.is_some() && body_projected_len > MAX_JIT_ROOT_CODE_BYTES {
                if diagnostics_enabled {
                    eprintln!(
                        "QUENCH_WASM_JIT_ROOT_REJECTION root={} native-code-bytes={body_projected_len} limit={MAX_JIT_ROOT_CODE_BYTES}",
                        root.expect("root mode checked")
                    );
                }
                return None;
            }
            let offset = place(&mut bytes, body);
            offsets[index] = Some(offset);
            calls.extend(function_calls.into_iter().map(|call| (offset, call)));
            let memory = MemoryArguments::of(fixed_base, guarded);
            let entry = analysis.compile_entry(&*isa, index, plans[index].as_ref()?, memory)?;
            let entry_projected_len = projected_code_len(bytes.len(), entry.len())?;
            if root.is_some() && entry_projected_len > MAX_JIT_ROOT_CODE_BYTES {
                if diagnostics_enabled {
                    eprintln!(
                        "QUENCH_WASM_JIT_ROOT_REJECTION root={} native-code-bytes={entry_projected_len} limit={MAX_JIT_ROOT_CODE_BYTES}",
                        root.expect("root mode checked")
                    );
                }
                return None;
            }
            entries[index] = Some(place(&mut bytes, entry));
        }
        for (offset, call) in calls {
            link(&mut bytes, offset, &call, offsets[call.callee]?)?;
        }
        let memory = CodeMemory::new(&bytes)?;
        let mapped_bytes = memory.length;
        let base = memory.pointer as usize;
        let table: Box<[usize]> = offsets
            .iter()
            .map(|offset| offset.map_or(0, |offset| base + offset))
            .collect();
        let functions: Vec<Option<CompiledFunction>> = plans
            .into_iter()
            .zip(entries)
            .map(|(plan, entry)| {
                let plan = plan?;
                let entry = entry?;
                // SAFETY: the entry was compiled with the `Entry` signature in
                // the host's default calling convention.
                let entry = unsafe { std::mem::transmute::<usize, Entry>(base + entry) };
                Some(CompiledFunction {
                    entry,
                    params: plan.params,
                    result: plan.result,
                    #[cfg(test)]
                    native_entries: std::cell::Cell::new(0),
                    #[cfg(test)]
                    direct_reference_entries: std::cell::Cell::new(0),
                })
            })
            .collect();
        let compiled_functions = functions
            .iter()
            .filter(|function| function.is_some())
            .count();
        let diagnostics = diagnostics_enabled.then(|| WasmJitDiagnostics {
            program: program.raw(),
            total_functions: count,
            compiled_functions,
            code_bytes: bytes.len(),
            mapped_bytes,
            compile_ns: compile_started.map_or(0, |started| started.elapsed().as_nanos()),
            native_entry_requests: (0..count).map(|_| std::cell::Cell::new(0)).collect(),
            interpreter_fallback_entries: (0..count).map(|_| std::cell::Cell::new(0)).collect(),
            compiled_to_vm_callouts: std::cell::Cell::new(0),
            compiled_import_callouts: std::cell::Cell::new(0),
            compiled_known_callouts: std::cell::Cell::new(0),
            compiled_reference_callouts: std::cell::Cell::new(0),
            reference_call_sites,
        });
        Some(CompiledProgram {
            _code: memory,
            table,
            functions,
            memory_slot,
            guarded,
            fixed_base,
            wraps,
            globals,
            diagnostics,
            reference_call_caches: std::cell::RefCell::new(
                std::iter::repeat_with(|| None)
                    .take(reference_cache_count)
                    .collect(),
            ),
            #[cfg(test)]
            reference_cache_hits: std::cell::Cell::new(0),
        })
    }

    #[cfg(test)]
    pub(super) fn install_compiled_diagnostics_for_test(
        &mut self,
        program: ProgramId,
        env: Value,
    ) -> bool {
        let Some(compiled) = self
            .compile_program_with_diagnostics(program, env, true)
            .map(Rc::new)
        else {
            return false;
        };
        self.programs.set_compiled(program, Some(compiled));
        true
    }

    #[cfg(test)]
    pub(super) fn install_compiled_root_for_test(
        &mut self,
        program: ProgramId,
        env: Value,
        root: usize,
    ) -> bool {
        let Some(compiled) = self
            .compile_program_with_diagnostics_and_root(program, env, true, Some(root))
            .map(Rc::new)
        else {
            return false;
        };
        self.programs.set_compiled(program, Some(compiled));
        true
    }

    /// Run known function `id` of the caller's program as native code, with
    /// arguments from the caller's register `window`. `None` leaves the call
    /// to the interpreter.
    pub(super) fn call_compiled(
        &mut self,
        caller: usize,
        id: u32,
        window: crate::bytecode::RegisterWindow,
    ) -> Option<Result<Value, JsError>> {
        if self.execution_mode != ExecutionMode::Compiled {
            return None;
        }
        let (program, env) = {
            let frame = self.frames.get(caller)?;
            (frame.program, frame.env)
        };
        self.run_compiled_inner(
            program,
            env,
            id,
            CompiledArgs::RegisterWindow {
                caller,
                base: usize::from(window.base),
                count: usize::from(window.count),
            },
        )
    }

    /// Run function `id` of `program` in instance `env` as native code, with
    /// encoded Wasm `args`. `None` leaves the call to the interpreter, which
    /// is then exactly where the call would otherwise have gone.
    pub(super) fn run_compiled(
        &mut self,
        program: ProgramId,
        env: Value,
        id: u32,
        args: &[Value],
    ) -> Option<Result<Value, JsError>> {
        self.run_compiled_inner(program, env, id, CompiledArgs::Values(args))
    }

    /// Convert an owned argument window or a public Wasm value slice into the
    /// raw scalar words expected by a compiled entry. The register borrow is
    /// confined to this conversion and ends before native code can call back
    /// into the VM.
    fn compiled_raw_args(
        &self,
        args: CompiledArgs<'_>,
        params: &[crate::WasmType],
    ) -> Option<smallvec::SmallVec<[u64; 8]>> {
        let values = match args {
            CompiledArgs::Values(values) => values,
            CompiledArgs::RegisterWindow {
                caller,
                base,
                count,
            } => self
                .frames
                .get(caller)?
                .registers
                .get(base..base.checked_add(count)?)?,
        };
        if values.len() != params.len() {
            return None;
        }
        let mut raw = smallvec::SmallVec::new();
        for (value, ty) in values.iter().zip(params) {
            raw.push(match Kind::of(*ty)? {
                Kind::I32 => u64::from(value.wasm_bits32()),
                Kind::I64 => self.decode_bits64(*value)?,
            });
        }
        Some(raw)
    }

    fn run_compiled_inner(
        &mut self,
        program: ProgramId,
        env: Value,
        id: u32,
        args: CompiledArgs<'_>,
    ) -> Option<Result<Value, JsError>> {
        if self.execution_mode != ExecutionMode::Compiled {
            return None;
        }
        let baseline = match self.programs.compiled(program) {
            Some(compiled) => compiled,
            None => {
                if jit_wasm_selective_enabled()
                    && self
                        .programs
                        .residual(program)
                        .is_some_and(|code| code.kind == crate::bytecode::ProgramKind::Wasm)
                {
                    // Selective mode starts cold Wasm functions in the
                    // interpreter. Hot roots are promoted after the entry
                    // gate below, avoiding module-wide cold compilation.
                    self.programs.set_compiled(program, None);
                    None
                } else {
                    let diagnose = jit_diagnostics_enabled()
                        && self
                            .programs
                            .get(program)
                            .is_some_and(|code| code.kind == crate::bytecode::ProgramKind::Wasm);
                    let started = diagnose.then(std::time::Instant::now);
                    let compiled = self.compile_program(program, env).map(Rc::new);
                    if diagnose && compiled.is_none() {
                        let total_functions = self
                            .programs
                            .get(program)
                            .map_or(0, |code| code.functions.len());
                        eprintln!(
                            "QUENCH_WASM_JIT_STATS {{\"program\":{},\"compile_attempts\":1,\"compile_failed\":true,\"total_functions\":{},\"compile_ns\":{}}}",
                            program.raw(),
                            total_functions,
                            started
                                .expect("diagnostic timer started")
                                .elapsed()
                                .as_nanos(),
                        );
                    }
                    self.programs.set_compiled(program, compiled.clone());
                    compiled
                }
            }
        };
        let mut compiled = baseline
            .filter(|compiled| compiled.has_function(id as usize))
            .or_else(|| self.programs.wasm_tier_containing(program, id));
        if compiled.is_none()
            && self.programs.residual(program).is_some_and(|code| {
                code.kind == crate::bytecode::ProgramKind::Wasm
                    && code.functions.get(id as usize).is_some_and(|function| {
                        function.code.len() <= MAX_JIT_TIER_BYTECODES_PER_FUNCTION
                    })
            })
            && self.programs.note_wasm_tier_entry(program, id)
        {
            let tier = self
                .compile_program_with_diagnostics_and_root(
                    program,
                    env,
                    jit_diagnostics_enabled(),
                    Some(id as usize),
                )
                .map(Rc::new);
            if let Some(tier) = tier
                && self.programs.install_wasm_tier(program, id, tier)
            {
                compiled = self.programs.wasm_tier(program, id);
            }
        }
        let Some(compiled) = compiled else {
            self.programs
                .has_compiled_function_or_note_fallback(program, id as usize);
            return None;
        };
        let result = (|| {
            let function = compiled.functions.get(id as usize)?.as_ref()?;
            let (base, length, guard, memory_binding) = match compiled.memory_slot {
                Some(slot) => {
                    let binding = self.heap.environment_slot(env, slot as usize)?;
                    let view = super::dispatch_fast::unshared_memory(self, binding)?;
                    let guard = guarded_memory(self, binding);
                    // Code compiled without bounds checks needs this memory's guard.
                    if compiled.guarded && guard.is_none() {
                        return None;
                    }
                    if compiled.wraps && !wrapping_memory(self, binding) {
                        return None;
                    }
                    if compiled.fixed_base.is_some_and(|fixed| {
                        guard.map(|(base, _)| displacement_base(base)) != Some(Some(fixed))
                    }) {
                        return None;
                    }
                    (view.0 as u64, view.1 as u64, guard, binding)
                }
                None => (0, 0, None, Value::UNDEFINED),
            };
            let raw = self.compiled_raw_args(args, &function.params)?;
            let marker = 0u8;
            let stack_pointer = std::hint::black_box(&marker as *const u8 as usize);
            let mut globals: smallvec::SmallVec<[*mut Value; 8]> = smallvec::SmallVec::new();
            for slot in &compiled.globals {
                let binding = self.heap.environment_slot(env, *slot as usize)?;
                match self.heap.get_mut(binding)? {
                    Cell::WasmGlobal { value, .. } => globals.push(value),
                    _ => return None,
                }
            }
            let (guard_start, guard_end) = guard.map_or((0, 0), |(start, size)| {
                (start as usize, start as usize + size)
            });
            let mut import_callouts = 0u64;
            let mut known_callouts = 0u64;
            let mut reference_callouts = 0u64;
            let (import_callouts_ptr, known_callouts_ptr, reference_callouts_ptr): (
                *mut u64,
                *mut u64,
                *mut u64,
            ) = if compiled.diagnostics.is_some() {
                (
                    &mut import_callouts as *mut u64,
                    &mut known_callouts as *mut u64,
                    &mut reference_callouts as *mut u64,
                )
            } else {
                (
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            let poll = self.execution_interrupt.poll_page();
            let mut context = CallContext {
                stack_limit: stack_pointer.saturating_sub(COMPILED_STACK_BUDGET),
                interrupt: self.execution_interrupt.atomic_flag(),
                table: compiled.table.as_ptr(),
                vm: (self as *mut Self).cast(),
                env,
                entry_stack: 0,
                guard_start,
                guard_end,
                globals: globals.as_ptr(),
                callout: import_callout::<H> as Callout as usize,
                call_reference: reference_callout::<H> as ReferenceCallout as usize,
                compiled: Rc::as_ptr(&compiled),
                program,
                import_callouts: import_callouts_ptr,
                known_callouts: known_callouts_ptr,
                reference_callouts: reference_callouts_ptr,
                memory: memory_binding,
                memory_length: length,
                raised: None,
                call_reference_diagnostic: compiled.diagnostics.as_ref().map_or(0, |_| {
                    diagnostic_reference_callout::<H> as DiagnosticReferenceCallout as usize
                }),
                reference_call_sites: compiled
                    .diagnostics
                    .as_ref()
                    .map_or(std::ptr::null(), |stats| {
                        stats.reference_call_sites.as_ptr()
                    }),
                reference_call_site_count: compiled
                    .diagnostics
                    .as_ref()
                    .map_or(0, |stats| stats.reference_call_sites.len()),
                poll_start: poll.map_or(0, |(start, _)| start),
                poll_end: poll.map_or(0, |(start, bytes)| start + bytes),
            };
            let mut out = 0u64;
            compiled.note_native_entry(id as usize);
            let status = match guard_trampoline() {
                Some(trampoline) => {
                    // The trampoline gives this entry its own resumable stack
                    // point, including when a compiled indirect call nests it.
                    let outer = ACTIVE_CONTEXT.with(|active| active.replace(&mut context));
                    // SAFETY: as below; the trampoline saves the caller's registers
                    // and records where a guard fault resumes.
                    let status = unsafe {
                        trampoline(
                            &mut context,
                            base,
                            length,
                            raw.as_ptr(),
                            &mut out,
                            function.entry,
                        )
                    };
                    ACTIVE_CONTEXT.with(|active| active.set(outer));
                    status
                }
                // SAFETY: the entry is compiled code for this function's
                // signature; the context, arguments and result outlive the call,
                // and the memory view stays valid because compiled code never
                // grows memory.
                None => unsafe {
                    (function.entry)(&mut context, base, length, raw.as_ptr(), &mut out)
                },
            };
            if let Some(stats) = &compiled.diagnostics {
                stats.compiled_to_vm_callouts.set(
                    stats
                        .compiled_to_vm_callouts
                        .get()
                        .saturating_add(import_callouts)
                        .saturating_add(known_callouts)
                        .saturating_add(reference_callouts),
                );
                stats.compiled_import_callouts.set(
                    stats
                        .compiled_import_callouts
                        .get()
                        .saturating_add(import_callouts),
                );
                stats.compiled_known_callouts.set(
                    stats
                        .compiled_known_callouts
                        .get()
                        .saturating_add(known_callouts),
                );
                stats.compiled_reference_callouts.set(
                    stats
                        .compiled_reference_callouts
                        .get()
                        .saturating_add(reference_callouts),
                );
            }
            if i64::from(status) == TERMINATED_STATUS {
                return Some(Err(JsError::execution_terminated()));
            }
            if i64::from(status) == RAISED_STATUS {
                return match context.raised.take()? {
                    Raised::Error(error) => Some(Err(error)),
                    Raised::Panic(panic) => std::panic::resume_unwind(panic),
                };
            }
            if status != 0 {
                let trap = TRAPS[status as usize - 1];
                return Some(Err(JsError::wasm_trap_error(trap)));
            }
            Some(Ok(match function.result {
                Some(ty) => match Kind::of(ty)? {
                    Kind::I32 => Value::integer(out as u32 as i32),
                    Kind::I64 => self.encode_bits64(out),
                },
                None => self.programs.constant(program, VOID_RESULT_CONSTANT)?,
            }))
        })();
        if let Some(stats) = &compiled.diagnostics
            && result.is_none()
            && let Some(fallbacks) = stats.interpreter_fallback_entries.get(id as usize)
        {
            fallbacks.set(fallbacks.get().saturating_add(1));
        }
        result
    }
}

impl<H: Host> Vm<H> {
    /// Call imported `function` of `program` in instance `env` with the Wasm
    /// bits at `args`, storing its result's bits at `out`.
    ///
    /// # Safety
    /// `args` holds a word per parameter of the import and `out` one word.
    unsafe fn call_import(
        &mut self,
        program: ProgramId,
        env: Value,
        function: u32,
        args: *const u64,
        out: *mut u64,
    ) -> Result<(), JsError> {
        let code = self
            .programs
            .get(program)
            .ok_or_else(|| JsError::validation("missing compiled Wasm program".into()))?;
        let (params, result_type) = {
            let signature = self
                .programs
                .wasm_signatures(program)
                .and_then(|signatures| signatures.get(function as usize))
                .ok_or_else(|| JsError::validation("missing Wasm import signature".into()))?;
            (
                signature
                    .params
                    .iter()
                    .copied()
                    .collect::<smallvec::SmallVec<[crate::WasmType; 8]>>(),
                signature.results.first().copied(),
            )
        };
        let active_program = self.replace_active_program(program);
        let callee = self.wasm_function_reference(&code, function, env);
        self.set_active_program(active_program);
        let callee = callee?;
        // Allocation never collects, so the encoded arguments stay live until
        // the call roots them.
        let mut values: smallvec::SmallVec<[Value; 8]> = smallvec::SmallVec::new();
        for (offset, ty) in params.iter().enumerate() {
            // SAFETY: the caller passes a word per parameter.
            let bits = unsafe { *args.add(offset) };
            values.push(self.encode_wasm_value(crate::WasmValue::from_bits(*ty, bits)));
        }
        let receiver = self
            .programs
            .constant(program, VOID_RESULT_CONSTANT)
            .ok_or_else(|| JsError::validation("missing Wasm void constant".into()))?;
        let defined = self
            .programs
            .wasm_signatures(program)
            .and_then(|signatures| signatures.defined_index(function));
        if let Some(defined) = defined {
            let active_program = self.replace_active_program(program);
            let compiled = self.run_compiled(program, env, defined, &values);
            self.set_active_program(active_program);
            if let Some(compiled) = compiled {
                let result = compiled?;
                let bits = if let Some(ty) = result_type {
                    match self.decode_wasm_value(result, ty)?.bits() {
                        Some(crate::wasm::ScalarBits::Bits32(bits)) => u64::from(bits),
                        Some(crate::wasm::ScalarBits::Bits64(bits)) => bits,
                        _ => {
                            return Err(JsError::validation(
                                "non-scalar Wasm import result".into(),
                            ));
                        }
                    }
                } else {
                    0
                };
                // SAFETY: the caller passes a result word.
                unsafe { *out = bits };
                return Ok(());
            }
        }
        let result = self.call_value(&code, callee, receiver, &values)?;
        if let Some(ty) = result_type {
            let bits = match self.decode_wasm_value(result, ty)?.bits() {
                Some(crate::wasm::ScalarBits::Bits32(bits)) => u64::from(bits),
                Some(crate::wasm::ScalarBits::Bits64(bits)) => bits,
                _ => return Err(JsError::validation("non-scalar Wasm import result".into())),
            };
            // SAFETY: the caller passes a result word.
            unsafe { *out = bits };
        }
        Ok(())
    }
}

/// The tag bits of an encoded i32, which a global's value carries.
const INTEGER_TAG: u64 = crate::value::INTEGER_TAG_BITS;

/// The constant a void Wasm function returns (see the Wasm lowering).
const VOID_RESULT_CONSTANT: usize = 1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_analysis_states_preserve_register_joins() {
        const FACTS: [Slot; 7] = [
            Slot::DEFAULT,
            Slot::Value(Kind::I64),
            Slot::Binding(7),
            Slot::Memory,
            Slot::Import(9),
            Slot::IndirectReference {
                signature: 11,
                table: 13,
            },
            Slot::Conflict,
        ];
        let states = FACTS
            .iter()
            .flat_map(|first| {
                FACTS.iter().flat_map(move |second| {
                    FACTS.iter().map(move |third| vec![*first, *second, *third])
                })
            })
            .collect::<Vec<_>>();
        let sparse = |dense: &[Slot]| {
            let registers = u16::try_from(dense.len()).expect("small test state");
            let mut state = AnalysisState::new();
            for (index, slot) in dense.iter().copied().enumerate() {
                state
                    .set(
                        u16::try_from(index).expect("small test register"),
                        registers,
                        slot,
                    )
                    .expect("register in range");
            }
            state
        };

        for left in &states {
            for right in &states {
                let mut expected = left.clone();
                let mut changed = false;
                for (slot, incoming) in expected.iter_mut().zip(right) {
                    let joined = slot.join(*incoming);
                    changed |= joined != *slot;
                    *slot = joined;
                }

                let mut actual = sparse(left);
                assert_eq!(actual.join_from(&sparse(right)), changed);
                for (index, expected) in expected.iter().copied().enumerate() {
                    assert_eq!(actual.get(index as u16, 3), Some(expected));
                }
            }
        }

        // Once a loop backedge turns a fact into Conflict, later iterations
        // must not make it precise again.
        let mut loop_state = AnalysisState::new();
        let mut incoming = AnalysisState::new();
        incoming
            .set(0, 1, Slot::Value(Kind::I64))
            .expect("register in range");
        assert!(loop_state.join_from(&incoming));
        assert_eq!(loop_state.get(0, 1), Some(Slot::Conflict));
        assert!(!loop_state.join_from(&AnalysisState::new()));
        assert!(!loop_state.join_from(&incoming));
        assert_eq!(loop_state.get(0, 1), Some(Slot::Conflict));
        assert_eq!(loop_state.get(1, 1), None);

        loop_state
            .set(0, 1, Slot::DEFAULT)
            .expect("register in range");
        assert_eq!(loop_state.get(0, 1), Some(Slot::DEFAULT));
    }

    /// Each i32 integer operator widens to its namesake, and each f32 float
    /// operator to its f64 namesake.
    #[test]
    fn narrow_selectors_name_their_wide_namesakes() {
        let mut tag = 0;
        while let Some(operator) = I32BinaryOperator::from_tag(tag) {
            assert_eq!(
                format!("{operator:?}"),
                format!("{:?}", widen_binary(operator))
            );
            tag += 1;
        }
        let mut tag = 0;
        while let Some(operator) = I32UnaryOperator::from_tag(tag) {
            assert_eq!(
                format!("{operator:?}"),
                format!("{:?}", widen_unary(operator))
            );
            tag += 1;
        }
        let mut tag = 0;
        while let Some(operator) = F32BinaryOperator::from_tag(tag) {
            let (wide, _) = float_binary_operator(Op::WasmF32Binary, tag).expect("f32 selector");
            assert_eq!(format!("{operator:?}"), format!("{wide:?}"));
            tag += 1;
        }
        let mut tag = 0;
        while let Some(operator) = F32UnaryOperator::from_tag(tag) {
            let (wide, _) = float_unary_operator(Op::WasmF32Unary, tag).expect("f32 selector");
            assert_eq!(format!("{operator:?}"), format!("{wide:?}"));
            tag += 1;
        }
    }
}
