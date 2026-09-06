//! Stackless execution state.
//!
//! `Machine::frames` is the sole owner of JavaScript call continuations.
//! A frame owns its return metadata and register-window bounds; frames are
//! pushed and popped only at VM transition boundaries. An empty stack is the
//! only completed state. `FrameStack::limit` is a hard bound: failed pushes
//! leave the stack unchanged and report the rejected frame to the caller.
//! Host callbacks may re-enter the VM, but must do so through a new machine
//! transition rather than Rust recursion.

pub use crate::identity::{CodeId, CodeRange, EnvironmentRef, FrameId, PackedCompletion};
use crate::stencil_admission::{AdmissionBuilder, AdmissionEntry, AdmissionStorage};
use crate::{
    completion::Completion,
    ir::ConstantKey,
    ops::{Constant, Op},
    value::Value,
};
use std::{cell::RefCell, collections::BTreeSet, rc::Rc, sync::OnceLock};

use crate::stencil_cfg::ControlFlowFacts;

#[cfg(target_arch = "aarch64")]
#[inline(always)]
unsafe fn invoke_f64x2_entry(entry: extern "C" fn(f64, f64) -> f64, lhs: f64, rhs: f64) -> f64 {
    // The generated leaf follows the platform C FP-register ABI.  Keep this
    // wrapper typed and let LLVM materialize the indirect call so arm64e
    // pointer-authentication and the return-address signing sequence remain
    // correct; hand-written BLR would need target-specific PAC handling.
    entry(lhs, rhs)
}

#[cfg(not(target_arch = "aarch64"))]
#[inline(always)]
fn invoke_f64x2_entry(entry: extern "C" fn(f64, f64) -> f64, lhs: f64, rhs: f64) -> f64 {
    entry(lhs, rhs)
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
unsafe fn invoke_f64x3_entry(
    entry: extern "C" fn(f64, f64, f64) -> f64,
    lhs: f64,
    rhs: f64,
    third: f64,
) -> f64 {
    // Keep the typed platform call so arm64e pointer authentication and the
    // return-address signing sequence remain correct.  A handwritten BLR
    // here would require target-specific PAC handling at every call site.
    entry(lhs, rhs, third)
}

#[cfg(not(target_arch = "aarch64"))]
#[inline(always)]
fn invoke_f64x3_entry(
    entry: extern "C" fn(f64, f64, f64) -> f64,
    lhs: f64,
    rhs: f64,
    third: f64,
) -> f64 {
    entry(lhs, rhs, third)
}

const OPTIMIZATION_WARMUP_MULTIPLIER: u32 = 8;

// Code stores are isolate-local and never shared across runtime threads. The
// OnceLock is retained only for the construction cycle: nested FunctionCode
// values must point at their eventual immutable store before that store exists.
// Replacing it with a lock-free RefCell would either expose a borrow across
// `ops()`'s returned slice or require a second representation of the store.

/// Immutable metadata kept beside executable instructions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstructionMeta {
    pub source: Option<u32>,
    pub name: Option<Rc<str>>,
    pub flags: u16,
    pub operand_window: u32,
    /// Index into the code-range quickening table, or `u32::MAX` when the
    /// opcode has no generated physical guard site.
    pub quickening_site: u32,
    pub named_cache: std::cell::Cell<u64>,
    /// Logical instruction-stream rewrite state.  The canonical instruction
    /// remains immutable; `CodeView::instruction` overlays this bounded cell
    /// so all readers observe the quickened opcode while the generic opcode
    /// remains available for dequickening/fallback.
    pub quickened_opcode: std::cell::Cell<Option<crate::ir::Opcode>>,
    pub quickened_shape: std::cell::Cell<u32>,
    pub quickened_property: std::cell::Cell<u32>,
    pub quickened_slot: std::cell::Cell<u32>,
}

impl InstructionMeta {
    pub const fn empty() -> Self {
        Self {
            source: None,
            name: None,
            flags: 0,
            operand_window: u32::MAX,
            quickening_site: u32::MAX,
            named_cache: std::cell::Cell::new(0),
            quickened_opcode: std::cell::Cell::new(None),
            quickened_shape: std::cell::Cell::new(u32::MAX),
            quickened_property: std::cell::Cell::new(u32::MAX),
            quickened_slot: std::cell::Cell::new(u32::MAX),
        }
    }
}

#[inline]
pub(crate) fn pack_named_cache(layout: u32, slot: u32) -> u64 {
    (u64::from(layout) << 32) | u64::from(slot.saturating_add(1))
}

#[inline]
pub(crate) fn unpack_named_cache(cache: u64) -> Option<(u32, u32)> {
    let layout = (cache >> 32) as u32;
    let slot = (cache as u32).checked_sub(1)?;
    (layout != 0).then_some((layout, slot))
}

/// Per-code canonical constant table.
///
/// `values` is the sole runtime fact and owns one entry per constant. IDs are
/// assigned in first-use order and are never truncated:
/// construction fails if the table cannot be represented by `u16` IDs.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstantPool {
    values: Rc<[Constant]>,
}
impl ConstantPool {
    pub fn try_new(values: Vec<Constant>) -> Result<Self, &'static str> {
        let mut canonical = Vec::with_capacity(values.len());
        let mut keys = Vec::with_capacity(values.len());
        for value in values {
            let key = ConstantKey::from(&value);
            if keys.contains(&key) {
                continue;
            }
            u16::try_from(canonical.len()).map_err(|_| "constant pool exceeds u16 IDs")?;
            keys.push(key);
            canonical.push(value);
        }
        Ok(Self {
            values: canonical.into(),
        })
    }

    pub fn new(values: Vec<Constant>) -> Self {
        Self::try_new(values).expect("constant pool exceeds u16 IDs")
    }

    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    pub fn id(&self, value: &Constant) -> Option<u16> {
        let key = ConstantKey::from(value);
        self.values
            .iter()
            .position(|candidate| ConstantKey::from(candidate) == key)
            .and_then(|id| u16::try_from(id).ok())
    }

    pub fn get(&self, id: u16) -> Option<&Constant> {
        self.values.get(usize::from(id))
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CatchRange {
    pub start: u16,
    pub end: u16,
    pub handler: u16,
    pub catch_slot: Option<u16>,
}

#[derive(Debug, Default)]
pub struct CodeArena {
    instructions: Vec<crate::ir::Instruction>,
    cold: Vec<Op>,
    ranges: Vec<(u32, u32)>,
    parameter_ends: Vec<Option<u32>>,
    constants: Vec<ConstantPool>,
    metadata: Vec<Vec<InstructionMeta>>,
    register_counts: Vec<u16>,
    quickening_sites: Vec<Vec<crate::quickening::QuickeningSite<4>>>,
    operand_windows: Vec<Vec<Rc<[u16]>>>,
    catch_ranges: Vec<Vec<CatchRange>>,
    pending_catches: Vec<CatchRange>,
}

fn metadata_for(op: &Op, source: Option<u32>) -> InstructionMeta {
    let name = match op {
        Op::CheckInitialized { name, .. }
        | Op::DeclareEvalBinding { name, .. }
        | Op::DeclareGlobalLexicalBinding { name, .. }
        | Op::DeleteEvalBinding { name, .. }
        | Op::DeleteName { name, .. }
        | Op::CheckGlobalFunction { name }
        | Op::CheckGlobalVar { name, .. }
        | Op::CreateGlobalFunction { name, .. }
        | Op::ResolveBindingTarget { name, .. }
        | Op::ResolveActiveBindingTarget { name, .. }
        | Op::InitializeResolvedBinding { name, .. }
        | Op::SetResolvedLocalBinding { name, .. }
        | Op::LoadResolvedLocalBinding { name, .. }
        | Op::LoadBinding { name, .. }
        | Op::LoadResolvedBinding { name, .. }
        | Op::ResolveStrictName { key: name, .. }
        | Op::ResolveName { key: name, .. }
        | Op::GetProperty { key: name, .. }
        | Op::SetProperty { key: name, .. }
        | Op::CallMethod { key: name, .. } => Some(Rc::<str>::from(name.as_str())),
        _ => None,
    };
    InstructionMeta {
        source,
        name,
        flags: u16::from(matches!(op, Op::CheckInitialized { .. })),
        operand_window: u32::MAX,
        quickening_site: u32::MAX,
        named_cache: std::cell::Cell::new(0),
        quickened_opcode: std::cell::Cell::new(None),
        quickened_shape: std::cell::Cell::new(u32::MAX),
        quickened_property: std::cell::Cell::new(u32::MAX),
        quickened_slot: std::cell::Cell::new(u32::MAX),
    }
}

/// Lower the common constant-specialized addition variants from the canonical
/// operation sequence. The write performed by `Const` is immediately consumed
/// by `Binary(Add)` at the same register, so the compact form can carry the
/// immutable pool ID directly.  The operand-order bit is retained because the
/// fallback may observe coercion order for strings, objects, and user code.
fn lower_const_add(ops: &[Op], constants: &ConstantPool) -> Option<crate::ir::Instruction> {
    let [Op::Const {
        dst: constant_dst,
        value,
    }, Op::Binary {
        dst,
        operator: crate::ops::BinaryOp::Add,
        lhs,
        rhs,
    }, ..] = ops
    else {
        return None;
    };
    let constant = constants.id(value)?;
    if constant_dst == lhs {
        Some(crate::ir::Instruction::add_const_left(*dst, *rhs, constant))
    } else if constant_dst == rhs {
        Some(crate::ir::Instruction::add_const(*dst, *lhs, constant))
    } else {
        None
    }
}

/// Derive one disposable physical site for each guarded instruction. The
/// instruction stream and operation catalog remain canonical; this table is
/// only mutable state for quickening decisions.
fn attach_quickening_sites(
    instructions: &[crate::ir::Instruction],
    metadata: &mut [InstructionMeta],
) -> Vec<crate::quickening::QuickeningSite<4>> {
    let mut sites = Vec::new();
    for (meta, instruction) in metadata.iter_mut().zip(instructions.iter()) {
        if instruction.opcode.is_quickenable()
            && (instruction
                .opcode
                .has_guard(crate::facts::OperationGuard::Shape)
                || instruction
                    .opcode
                    .has_guard(crate::facts::OperationGuard::Callable))
        {
            meta.quickening_site = sites.len() as u32;
            sites.push(crate::quickening::QuickeningSite::new(instruction.opcode));
        }
    }
    sites
}

/// Canonical register storage for the active frame.
///
/// `count` is the declared register width and `values.len()` is always exactly
/// that width. `base` is the frame-local offset and is never used as a second
/// copy of register contents.
#[derive(Debug, Clone, PartialEq)]
pub struct RegisterWindow {
    pub base: u32,
    pub count: u16,
    pub values: crate::register_file::RegisterFile,
}

impl RegisterWindow {
    pub fn new() -> Self {
        Self {
            base: 0,
            count: 0,
            values: crate::register_file::RegisterFile::new(),
        }
    }

    pub fn with_count(count: u16) -> Self {
        Self {
            base: 0,
            count,
            values: crate::register_file::RegisterFile::from_values(vec![
                Value::Undefined;
                usize::from(count)
            ]),
        }
    }

    pub fn is_valid(&self) -> bool {
        self.values.len() == usize::from(self.count)
    }
}

impl Default for RegisterWindow {
    fn default() -> Self {
        Self::new()
    }
}

/// Explicit VM frame storage.
///
/// `frames` is the canonical contiguous allocation. `base` identifies the
/// owning VM stack segment; frame offsets are valid only while the referenced
/// frame remains in this stack and are invalid after a pop or stack reset.
/// Push may relocate the allocation, so callers retain offsets, not references,
/// across transitions. The hard limit bounds depth and allocation growth.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameStack {
    pub base: u32,
    pub frames: Vec<Frame>,
    limit: u16,
}

impl FrameStack {
    const DEFAULT_CAPACITY: usize = 64;
    pub const DEFAULT_LIMIT: u16 = u16::MAX;

    pub fn new() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY as u16)
    }

    /// Creates a stack with an initial allocation and an explicit hard limit.
    /// Growth is geometric and happens before pushing, so execution never
    /// allocates beyond the configured limit.
    pub fn with_capacity(capacity: u16) -> Self {
        Self::with_capacity_and_limit(capacity, Self::DEFAULT_LIMIT)
    }

    pub fn with_capacity_and_limit(capacity: u16, limit: u16) -> Self {
        let capacity = usize::from(capacity.min(limit));
        Self {
            base: 0,
            frames: Vec::with_capacity(capacity),
            limit,
        }
    }

    pub fn capacity(&self) -> u16 {
        self.frames.capacity().min(usize::from(u16::MAX)) as u16
    }

    /// Preallocate room for `additional` frames without exceeding the limit.
    pub fn try_reserve_for(&mut self, additional: u16) -> bool {
        let target = self
            .frames
            .len()
            .checked_add(usize::from(additional))
            .filter(|target| *target <= usize::from(self.limit));
        let Some(target) = target else {
            return false;
        };
        if target > self.frames.capacity() {
            self.frames.reserve(target - self.frames.len());
        }
        true
    }

    /// Preallocate room, ignoring requests that exceed the hard limit.
    pub fn reserve_for(&mut self, additional: u16) {
        let _ = self.try_reserve_for(additional);
    }

    /// Returns the number of additional frames accepted before the hard limit.
    pub fn remaining(&self) -> u16 {
        self.limit
            .saturating_sub(self.frames.len().min(usize::from(u16::MAX)) as u16)
    }
}

impl Default for FrameStack {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameStack {
    pub fn try_push(&mut self, frame: Frame) -> Result<(), Frame> {
        if self.frames.len() >= usize::from(self.limit) {
            return Err(frame);
        }
        if self.frames.len() == self.frames.capacity() {
            let next = self
                .frames
                .capacity()
                .max(1)
                .saturating_mul(2)
                .min(usize::from(self.limit));
            self.frames.reserve(next.saturating_sub(self.frames.len()));
        }
        self.frames.push(frame);
        Ok(())
    }

    pub fn pop(&mut self) -> Option<Frame> {
        self.frames.pop()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn depth(&self) -> u16 {
        self.frames.len() as u16
    }

    pub fn as_slice(&self) -> &[Frame] {
        &self.frames
    }
    /// Check the canonical contiguous-storage invariant without exposing the
    /// backing allocation or introducing a second frame representation.
    pub fn invariant_holds(&self) -> bool {
        self.frames.len() <= usize::from(self.limit)
            && self
                .top_offset()
                .is_none_or(|offset| usize::from(offset) + 1 == self.frames.len())
    }
    /// Return the offset of the top frame in the contiguous frame storage.
    #[inline]
    pub fn top_offset(&self) -> Option<u16> {
        self.frames.len().checked_sub(1).map(|index| index as u16)
    }

    /// Read a frame by its stable stack offset.
    #[inline]
    pub fn frame_at(&self, offset: u16) -> Option<&Frame> {
        self.frames.get(usize::from(offset))
    }
    /// Mutably access a frame by its stable stack offset.
    #[inline]
    pub fn frame_at_mut(&mut self, offset: u16) -> Option<&mut Frame> {
        self.frames.get_mut(usize::from(offset))
    }
}

impl CodeArena {
    pub fn new() -> Self {
        Self::default()
    }
    fn append_tree(&mut self, mut body: Vec<Op>, store: &Rc<OnceLock<Rc<CodeStore>>>) -> CodeRange {
        for op in &mut body {
            op.rehome_bodies(self, store);
        }
        self.append(body)
    }
    pub fn append_slice(&mut self, body: &[Op]) -> CodeRange {
        let nested = body.iter().map(Op::body_count).sum::<usize>();
        self.ranges.reserve(nested.saturating_add(1));
        let code = CodeId(self.ranges.len() as u32);
        let start = self.instructions.len() as u32;
        let mut const_values = Vec::new();
        collect_op_constants(body, &mut const_values);
        let constants = ConstantPool::new(const_values);
        let mut parameter_end = None;
        let mut metadata = Vec::with_capacity(body.len());
        let mut operand_windows = Vec::new();
        let mut cursor = 0;
        let mut source = None;
        while cursor < body.len() {
            if let Some(next_source) = trace_source(&body[cursor]) {
                source = Some(next_source);
                cursor += 1;
                continue;
            }
            if matches!(body[cursor], Op::ParameterEnd) {
                parameter_end.get_or_insert(self.instructions.len() as u32 - start);
                cursor += 1;
                continue;
            }
            if let Some(instruction) = lower_checked_local_load(&body[cursor..]) {
                self.instructions.push(instruction);
                metadata.push(metadata_for(&body[cursor], source));
                cursor += 2;
                continue;
            }
            if let Some(instruction) = lower_checked_local_store(&body[cursor..]) {
                self.instructions.push(instruction);
                metadata.push(metadata_for(&body[cursor], source));
                cursor += 2;
                continue;
            }
            if let Some(instruction) = lower_local_initialization(&body[cursor..]) {
                self.instructions.push(instruction);
                metadata.push(metadata_for(&body[cursor + 1], source));
                cursor += 2;
                continue;
            }
            if let Some(instruction) = lower_proven_local_move(&body[cursor..]) {
                self.instructions.push(instruction);
                metadata.push(metadata_for(&body[cursor], source));
                cursor += 3;
                continue;
            }
            if let Some(instruction) = lower_named_call(&body[cursor..]) {
                self.instructions.push(instruction);
                metadata.push(metadata_for(&body[cursor], source));
                cursor += 2;
                continue;
            }
            if let Some(instruction) = lower_proven_local_postfix_update(&body[cursor..]) {
                self.instructions.push(instruction);
                metadata.push(metadata_for(&body[cursor], source));
                cursor += 5;
                continue;
            }
            if let Some(instruction) = lower_proven_local_update(&body[cursor..]) {
                self.instructions.push(instruction);
                metadata.push(metadata_for(&body[cursor], source));
                cursor += 4;
                continue;
            }
            if let Some(instruction) = lower_local_update(&body[cursor..]) {
                self.instructions.push(instruction);
                metadata.push(metadata_for(&body[cursor + 3], source));
                cursor += 5;
                continue;
            }
            if let Some(next) = self.try_encode_control(
                body,
                cursor,
                start,
                &constants,
                &mut metadata,
                &mut operand_windows,
                source,
                None,
            ) {
                cursor = next;
                continue;
            }
            if let Some(instruction) = lower_const_add(&body[cursor..], &constants) {
                self.instructions.push(instruction);
                metadata.push(metadata_for(&body[cursor + 1], source));
                cursor += 2;
                continue;
            }
            let op = &body[cursor];
            let mut meta = metadata_for(op, source);
            let instruction = match op {
                Op::Const { dst, value } => crate::ir::Instruction::load_const(
                    *dst,
                    constants.id(value).expect("constant was collected"),
                ),
                Op::CallMethod {
                    dst,
                    object,
                    callee: Some(callee),
                    args,
                    spreads,
                    ..
                } if args.len() == 6 && spreads.iter().all(|spread| !spread) => {
                    meta.operand_window = operand_windows.len() as u32;
                    operand_windows.push(Rc::from(args.as_slice()));
                    crate::ir::Instruction::call_registered_window(
                        *dst,
                        *object,
                        *callee,
                        args.len() as u8,
                    )
                }
                _ => crate::ir::lower_compact(op).unwrap_or_else(|| {
                    let index = self.cold.len() as u32;
                    self.cold.push(op.clone());
                    crate::ir::Instruction::slow_at(index)
                }),
            };
            self.instructions.push(instruction);
            metadata.push(meta);
            cursor += 1;
        }
        let instruction_end = start as usize + metadata.len();
        let quickening_sites = attach_quickening_sites(
            &self.instructions[start as usize..instruction_end],
            &mut metadata,
        );
        let register_count = register_count_for(
            &self.instructions[start as usize..instruction_end],
            &metadata,
            &operand_windows,
        );
        self.metadata.push(metadata);
        self.quickening_sites.push(quickening_sites);
        self.operand_windows.push(operand_windows);
        self.catch_ranges
            .push(std::mem::take(&mut self.pending_catches));
        let end = self.instructions.len() as u32;
        self.ranges.push((start, end));
        self.parameter_ends.push(parameter_end);
        self.constants.push(constants);
        self.register_counts.push(register_count);
        CodeRange { code, start, end }
    }

    fn emit_conditional(
        &mut self,
        dst: Option<u16>,
        condition: u16,
        then_ops: &[Op],
        else_ops: &[Op],
        range_start: u32,
        constants: &ConstantPool,
        metadata: &mut Vec<InstructionMeta>,
        operand_windows: &mut Vec<Rc<[u16]>>,
        source: Option<u32>,
    ) {
        let jif = self.instructions.len();
        self.instructions
            .push(crate::ir::Instruction::jump_if_false(condition, 0));
        metadata.push(InstructionMeta::empty());
        self.encode_linear(
            then_ops,
            range_start,
            constants,
            metadata,
            operand_windows,
            source,
            dst,
        );
        let jump = self.instructions.len();
        self.instructions.push(crate::ir::Instruction::jump(0));
        metadata.push(InstructionMeta::empty());
        let else_pc =
            u16::try_from(self.instructions.len() as u32 - range_start).unwrap_or(u16::MAX);
        self.instructions[jif].b = else_pc;
        self.encode_linear(
            else_ops,
            range_start,
            constants,
            metadata,
            operand_windows,
            source,
            dst,
        );
        let end_pc =
            u16::try_from(self.instructions.len() as u32 - range_start).unwrap_or(u16::MAX);
        self.instructions[jump].a = end_pc;
    }

    fn encode_linear(
        &mut self,
        body: &[Op],
        range_start: u32,
        constants: &ConstantPool,
        metadata: &mut Vec<InstructionMeta>,
        operand_windows: &mut Vec<Rc<[u16]>>,
        source: Option<u32>,
        ternary_dst: Option<u16>,
    ) {
        let mut cursor = 0;
        while cursor < body.len() {
            if let Some(next) = self.try_encode_control(
                body,
                cursor,
                range_start,
                constants,
                metadata,
                operand_windows,
                source,
                ternary_dst,
            ) {
                cursor = next;
                continue;
            }
            if let Some(instruction) = lower_const_add(&body[cursor..], constants) {
                self.instructions.push(instruction);
                metadata.push(metadata_for(&body[cursor + 1], source));
                cursor += 2;
                continue;
            }
            if let (Some(dst), Op::Return { src }) = (ternary_dst, &body[cursor]) {
                self.instructions
                    .push(crate::ir::Instruction::move_(dst, *src));
                metadata.push(InstructionMeta::empty());
                cursor += 1;
                continue;
            }
            if let Some(instruction) = lower_named_call(&body[cursor..]) {
                self.instructions.push(instruction);
                metadata.push(metadata_for(&body[cursor], source));
                cursor += 2;
                continue;
            }
            let op = &body[cursor];
            let mut meta = metadata_for(op, source);
            let instruction = match op {
                Op::Const { dst, value } => crate::ir::Instruction::load_const(
                    *dst,
                    constants.id(value).expect("constant was collected"),
                ),
                Op::CallMethod {
                    dst,
                    object,
                    callee: Some(callee),
                    args,
                    spreads,
                    ..
                } if args.len() == 6 && spreads.iter().all(|spread| !spread) => {
                    meta.operand_window = operand_windows.len() as u32;
                    operand_windows.push(Rc::from(args.as_slice()));
                    crate::ir::Instruction::call_registered_window(
                        *dst,
                        *object,
                        *callee,
                        args.len() as u8,
                    )
                }
                _ => crate::ir::lower_compact(op).unwrap_or_else(|| {
                    let index = self.cold.len() as u32;
                    self.cold.push(op.clone());
                    crate::ir::Instruction::slow_at(index)
                }),
            };
            self.instructions.push(instruction);
            metadata.push(meta);
            cursor += 1;
        }
    }

    fn relative_pc(&self, range_start: u32) -> u16 {
        u16::try_from(self.instructions.len() as u32 - range_start).unwrap_or(u16::MAX)
    }

    fn try_encode_control(
        &mut self,
        body: &[Op],
        cursor: usize,
        range_start: u32,
        constants: &ConstantPool,
        metadata: &mut Vec<InstructionMeta>,
        operand_windows: &mut Vec<Rc<[u16]>>,
        source: Option<u32>,
        ternary_dst: Option<u16>,
    ) -> Option<usize> {
        match &body[cursor] {
            Op::Conditional {
                dst,
                condition,
                consequent,
                alternate,
            } => {
                let then_ops = consequent.source_ops()?;
                let else_ops = alternate.source_ops()?;
                self.emit_conditional(
                    Some(*dst),
                    *condition,
                    then_ops,
                    else_ops,
                    range_start,
                    constants,
                    metadata,
                    operand_windows,
                    source,
                );
                Some(cursor + 1)
            }
            Op::Branch {
                condition,
                then_ops,
                else_ops,
            } => {
                let then_ops = then_ops.source_ops()?;
                let else_ops = else_ops.source_ops()?;
                self.emit_conditional(
                    None,
                    *condition,
                    then_ops,
                    else_ops,
                    range_start,
                    constants,
                    metadata,
                    operand_windows,
                    source,
                );
                Some(cursor + 1)
            }
            Op::Loop {
                init,
                test,
                body: loop_body,
                update,
                post_test,
                label,
                per_iteration,
                ..
            } if label.is_none() && per_iteration.is_empty() => {
                let init = init.source_ops()?;
                let test = test.source_ops()?;
                let loop_body = loop_body.source_ops()?;
                let update = update.source_ops()?;
                if update.is_empty() && init.is_empty() {
                    return None;
                }
                if ops_contain_call(init)
                    || ops_contain_call(test)
                    || ops_contain_call(loop_body)
                    || ops_contain_call(update)
                    || ops_contain_short_circuit(test)
                    || test_always_true(test)
                    || ops_use_arguments(init)
                    || ops_use_arguments(test)
                    || ops_use_arguments(loop_body)
                    || ops_use_arguments(update)
                    || !ops_are_stitchable_numeric(init)
                    || !ops_are_stitchable_numeric(test)
                    || !ops_are_stitchable_numeric(loop_body)
                    || !ops_are_stitchable_numeric(update)
                {
                    return None;
                }
                self.emit_loop(
                    init,
                    test,
                    loop_body,
                    update,
                    *post_test,
                    range_start,
                    constants,
                    metadata,
                    operand_windows,
                    source,
                );
                Some(cursor + 1)
            }
            _ => None,
        }
    }

    fn emit_loop(
        &mut self,
        init: &[Op],
        test: &[Op],
        body: &[Op],
        update: &[Op],
        post_test: bool,
        range_start: u32,
        constants: &ConstantPool,
        metadata: &mut Vec<InstructionMeta>,
        operand_windows: &mut Vec<Rc<[u16]>>,
        source: Option<u32>,
    ) {
        self.encode_fragment(
            init,
            range_start,
            constants,
            metadata,
            operand_windows,
            source,
        );
        if test_always_true(test) {
            let body_pc = self.relative_pc(range_start);
            self.encode_linear(
                body,
                range_start,
                constants,
                metadata,
                operand_windows,
                source,
                None,
            );
            self.encode_fragment(
                update,
                range_start,
                constants,
                metadata,
                operand_windows,
                source,
            );
            self.instructions
                .push(crate::ir::Instruction::jump(body_pc));
            metadata.push(InstructionMeta::empty());
            return;
        }
        if post_test {
            let body_pc = self.relative_pc(range_start);
            self.encode_linear(
                body,
                range_start,
                constants,
                metadata,
                operand_windows,
                source,
                None,
            );
            self.encode_fragment(
                update,
                range_start,
                constants,
                metadata,
                operand_windows,
                source,
            );
            if let Some(condition) = self.encode_test(
                test,
                range_start,
                constants,
                metadata,
                operand_windows,
                source,
            ) {
                let jif = self.instructions.len();
                self.instructions
                    .push(crate::ir::Instruction::jump_if_false(condition, 0));
                metadata.push(InstructionMeta::empty());
                self.instructions
                    .push(crate::ir::Instruction::jump(body_pc));
                metadata.push(InstructionMeta::empty());
                let end = self.relative_pc(range_start);
                self.instructions[jif].b = end;
            } else {
                self.instructions
                    .push(crate::ir::Instruction::jump(body_pc));
                metadata.push(InstructionMeta::empty());
            }
            return;
        }
        let test_pc = self.relative_pc(range_start);
        let jif = if let Some(condition) = self.encode_test(
            test,
            range_start,
            constants,
            metadata,
            operand_windows,
            source,
        ) {
            let jif = self.instructions.len();
            self.instructions
                .push(crate::ir::Instruction::jump_if_false(condition, 0));
            metadata.push(InstructionMeta::empty());
            Some(jif)
        } else {
            None
        };
        self.encode_linear(
            body,
            range_start,
            constants,
            metadata,
            operand_windows,
            source,
            None,
        );
        self.encode_fragment(
            update,
            range_start,
            constants,
            metadata,
            operand_windows,
            source,
        );
        self.instructions
            .push(crate::ir::Instruction::jump(test_pc));
        metadata.push(InstructionMeta::empty());
        if let Some(jif) = jif {
            let end = self.relative_pc(range_start);
            self.instructions[jif].b = end;
        }
    }

    fn emit_try(
        &mut self,
        body: &[Op],
        handler: Option<&[Op]>,
        finalizer: Option<&[Op]>,
        catch_slot: Option<u16>,
        range_start: u32,
        constants: &ConstantPool,
        metadata: &mut Vec<InstructionMeta>,
        operand_windows: &mut Vec<Rc<[u16]>>,
        source: Option<u32>,
        ternary_dst: Option<u16>,
    ) {
        let start = self.relative_pc(range_start);
        self.encode_linear(
            body,
            range_start,
            constants,
            metadata,
            operand_windows,
            source,
            ternary_dst,
        );
        let jump = self.instructions.len();
        self.instructions.push(crate::ir::Instruction::jump(0));
        metadata.push(InstructionMeta::empty());
        let handler_pc = self.relative_pc(range_start);
        self.pending_catches.push(CatchRange {
            start,
            end: handler_pc,
            handler: handler_pc,
            catch_slot,
        });
        if let Some(handler) = handler {
            self.encode_linear(
                handler,
                range_start,
                constants,
                metadata,
                operand_windows,
                source,
                ternary_dst,
            );
        }
        let after = self.relative_pc(range_start);
        self.instructions[jump].a = after;
        if let Some(finalizer) = finalizer {
            self.encode_linear(
                finalizer,
                range_start,
                constants,
                metadata,
                operand_windows,
                source,
                ternary_dst,
            );
        }
    }

    fn encode_fragment(
        &mut self,
        body: &[Op],
        range_start: u32,
        constants: &ConstantPool,
        metadata: &mut Vec<InstructionMeta>,
        operand_windows: &mut Vec<Rc<[u16]>>,
        source: Option<u32>,
    ) {
        let body = match body.last() {
            Some(Op::Return { .. }) => &body[..body.len() - 1],
            _ => body,
        };
        self.encode_linear(
            body,
            range_start,
            constants,
            metadata,
            operand_windows,
            source,
            None,
        );
    }

    fn encode_test(
        &mut self,
        body: &[Op],
        range_start: u32,
        constants: &ConstantPool,
        metadata: &mut Vec<InstructionMeta>,
        operand_windows: &mut Vec<Rc<[u16]>>,
        source: Option<u32>,
    ) -> Option<u16> {
        let condition = match body.last() {
            Some(Op::Return { src }) => Some(*src),
            _ => None,
        };
        self.encode_fragment(
            body,
            range_start,
            constants,
            metadata,
            operand_windows,
            source,
        );
        condition
    }

    pub fn append(&mut self, body: Vec<Op>) -> CodeRange {
        self.append_slice(&body)
    }

    pub fn append_function(&mut self, function: &FunctionCode) -> Option<CodeRange> {
        let body = function.source_ops()?;
        Some(self.append_slice(body))
    }

    pub fn constant_pool(&self, range: CodeRange) -> ConstantPool {
        self.constants
            .get(range.code.0 as usize)
            .cloned()
            .unwrap_or_else(ConstantPool::empty)
    }

    pub fn len(&self) -> usize {
        self.ranges.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    pub fn freeze(self) -> Rc<CodeStore> {
        let frame_register_counts = derive_frame_register_counts(
            &self.instructions,
            &self.cold,
            &self.ranges,
            &self.register_counts,
        );
        let mut store = Rc::new(CodeStore {
            instructions: self.instructions.into_boxed_slice().into(),
            cold: self.cold.into_boxed_slice().into(),
            ranges: self.ranges.into_boxed_slice().into(),
            parameter_ends: self.parameter_ends.into_boxed_slice().into(),
            constants: self.constants.into_boxed_slice().into(),
            metadata: self.metadata.into_boxed_slice().into(),
            register_counts: self.register_counts.into_boxed_slice().into(),
            frame_register_counts: frame_register_counts.into_boxed_slice().into(),
            quickening_sites: self
                .quickening_sites
                .into_iter()
                .map(|sites| {
                    sites
                        .into_iter()
                        .map(std::cell::RefCell::new)
                        .collect::<Vec<_>>()
                        .into_boxed_slice()
                })
                .collect::<Vec<_>>()
                .into_boxed_slice()
                .into(),
            operand_windows: self.operand_windows.into_boxed_slice().into(),
            catch_ranges: self.catch_ranges.into_boxed_slice().into(),
        });
        let weak = Rc::new(OnceLock::new());
        if let Some(store) = Rc::get_mut(&mut store) {
            let cold = Rc::get_mut(&mut store.cold).expect("cold store is uniquely owned");
            for op in cold {
                op.detach_store_links(&weak);
            }
        }
        let _ = weak.set(Rc::downgrade(&store));
        store
    }
}

/// Only residual operations with a fixed, allocation-free register contract
/// may be stitched into the current loop CFG.  Anything that can allocate,
/// call JS, suspend, or mutate shape/prototype state stays on the complete
/// ordinary fragment path until it has an explicit region declaration.
fn ops_are_stitchable_numeric(ops: &[Op]) -> bool {
    ops.iter().all(|op| {
        matches!(
            op,
            Op::Const { .. }
                | Op::StoreLocal { .. }
                | Op::Move { .. }
                | Op::LoadLocal { .. }
                | Op::LoadParameter { .. }
                | Op::LoadBinding { .. }
                | Op::LoadResolvedBinding { .. }
                | Op::LoadResolvedLocalBinding { .. }
                | Op::Binary { .. }
                | Op::Unary { .. }
                | Op::CheckInitialized { .. }
                | Op::RequireObjectCoercible { .. }
                | Op::GetPropertyDynamic { .. }
                | Op::SetPropertyDynamic { .. }
                | Op::Return { .. }
        )
    })
}

#[cfg(feature = "execution-trace")]
fn trace_source(op: &Op) -> Option<u32> {
    match op {
        Op::TraceSite { source } => Some(*source),
        _ => None,
    }
}

#[cfg(not(feature = "execution-trace"))]
#[inline(always)]
fn trace_source(_: &Op) -> Option<u32> {
    None
}

fn lower_checked_local_load(ops: &[Op]) -> Option<crate::ir::Instruction> {
    let [Op::CheckInitialized { slot: checked, .. }, Op::LoadLocal { dst, slot }, ..] = ops else {
        return None;
    };
    (checked == slot).then(|| crate::ir::Instruction::load_local_checked(*dst, *slot))
}

fn lower_checked_local_store(ops: &[Op]) -> Option<crate::ir::Instruction> {
    let [Op::CheckInitialized { slot: checked, .. }, Op::StoreLocal { slot, src }, ..] = ops else {
        return None;
    };
    (checked == slot).then(|| crate::ir::Instruction::store_local_checked(*slot, *src))
}

fn lower_local_initialization(ops: &[Op]) -> Option<crate::ir::Instruction> {
    let [Op::StoreLocal { slot, src }, Op::InitializeLocal { slot: initialized }, ..] = ops else {
        return None;
    };
    (slot == initialized).then(|| crate::ir::Instruction::init_local(*slot, *src))
}

fn lower_proven_local_move(ops: &[Op]) -> Option<crate::ir::Instruction> {
    let [Op::LoadLocal {
        dst: loaded,
        slot: source,
    }, Op::StoreLocal {
        slot: target,
        src: stored,
    }, Op::Move { dst, src }, ..] = ops
    else {
        return None;
    };
    (*stored == *loaded && *src == *loaded)
        .then(|| crate::ir::Instruction::move_local(*dst, *source, *target))
}

fn lower_named_call(ops: &[Op]) -> Option<crate::ir::Instruction> {
    let [Op::GetProperty {
        dst: callee,
        object,
        key,
    }, Op::CallMethod {
        dst,
        object: receiver,
        key: called_key,
        callee: Some(called),
        args,
        spreads,
    }, ..] = ops
    else {
        return None;
    };
    (*object == *receiver
        && *callee == *called
        && key == called_key
        && args.len() <= 1
        && spreads.iter().all(|spread| !spread))
    .then(|| crate::ir::Instruction::call_named(*dst, *object, args.first().copied()))
}

fn lower_local_update(ops: &[Op]) -> Option<crate::ir::Instruction> {
    let [Op::LoadLocal { dst: old, slot }, Op::Const {
        dst: one,
        value: Constant::Number(value),
    }, Op::Binary {
        dst: updated,
        operator,
        lhs,
        rhs,
    }, Op::CheckInitialized { slot: checked, .. }, Op::StoreLocal { slot: stored, src }, ..] = ops
    else {
        return None;
    };
    let decrement = match operator {
        crate::ops::BinaryOp::NumericAdd => false,
        crate::ops::BinaryOp::NumericSubtract => true,
        _ => return None,
    };
    (*value == 1.0
        && old != updated
        && lhs == old
        && rhs == one
        && slot == checked
        && slot == stored
        && updated == src)
        .then(|| crate::ir::Instruction::update_local(*old, *updated, *slot, decrement))
}

fn lower_proven_local_postfix_update(ops: &[Op]) -> Option<crate::ir::Instruction> {
    let [Op::LoadLocal { dst: old, slot }, Op::Const {
        dst: one,
        value: Constant::Number(value),
    }, Op::Binary {
        dst: updated,
        operator,
        lhs,
        rhs,
    }, Op::StoreLocal { slot: stored, src }, Op::Unary {
        dst: numeric_old,
        operator: crate::ops::UnaryOp::ToNumeric,
        src: unary_src,
    }, ..] = ops
    else {
        return None;
    };
    let decrement = match operator {
        crate::ops::BinaryOp::NumericAdd => false,
        crate::ops::BinaryOp::NumericSubtract => true,
        _ => return None,
    };
    (*value == 1.0
        && old != updated
        && lhs == old
        && rhs == one
        && slot == stored
        && updated == src
        && unary_src == old)
        .then(|| crate::ir::Instruction::update_local(*numeric_old, *updated, *slot, decrement))
}

fn lower_proven_local_update(ops: &[Op]) -> Option<crate::ir::Instruction> {
    let [Op::LoadLocal { dst: old, slot }, Op::Const {
        dst: one,
        value: Constant::Number(value),
    }, Op::Binary {
        dst: updated,
        operator,
        lhs,
        rhs,
    }, Op::StoreLocal { slot: stored, src }, ..] = ops
    else {
        return None;
    };
    let decrement = match operator {
        crate::ops::BinaryOp::NumericAdd => false,
        crate::ops::BinaryOp::NumericSubtract => true,
        _ => return None,
    };
    (*value == 1.0
        && old != updated
        && lhs == old
        && rhs == one
        && slot == stored
        && updated == src)
        .then(|| crate::ir::Instruction::update_local(*old, *updated, *slot, decrement))
}

fn register_count_for(
    instructions: &[crate::ir::Instruction],
    metadata: &[InstructionMeta],
    operand_windows: &[Rc<[u16]>],
) -> u16 {
    let mut count = 0usize;
    for (pc, instruction) in instructions.iter().enumerate() {
        let flow = instruction.register_flow();
        let window = metadata
            .get(pc)
            .and_then(|meta| operand_windows.get(meta.operand_window as usize))
            .map(AsRef::as_ref);
        let candidate = highest_register(flow, window).map_or_else(
            || {
                // Structured residuals are not yet physically composed;
                // retain their compact words until their handler contract is
                // available.
                if flow.complete {
                    0
                } else {
                    usize::from(instruction.a.max(instruction.b).max(instruction.c))
                }
            },
            usize::from,
        );
        count = count.max(candidate.saturating_add(1));
    }
    u16::try_from(count).unwrap_or(u16::MAX)
}

fn highest_register(flow: crate::ir::RegisterFlow, window: Option<&[u16]>) -> Option<u16> {
    flow.highest_register()
        .into_iter()
        .chain(window.into_iter().flatten().copied())
        .max()
}

fn derive_frame_register_counts(
    instructions: &[crate::ir::Instruction],
    cold: &[Op],
    ranges: &[(u32, u32)],
    register_counts: &[u16],
) -> Vec<u16> {
    let mut widths = register_counts.to_vec();
    for _ in 0..ranges.len() {
        let previous = widths.clone();
        for (code, &(start, end)) in ranges.iter().enumerate() {
            let mut width = previous.get(code).copied().unwrap_or(0);
            for instruction in &instructions[start as usize..end as usize] {
                let Some(op) = instruction
                    .cold_index()
                    .and_then(|index| cold.get(index as usize))
                else {
                    continue;
                };
                op.visit_frame_fragments(&mut |body| {
                    width = width.max(
                        previous
                            .get(body.code_id().0 as usize)
                            .copied()
                            .unwrap_or(0),
                    );
                });
            }
            widths[code] = width;
        }
        if widths == previous {
            break;
        }
    }
    widths
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodeStore {
    instructions: Rc<[crate::ir::Instruction]>,
    cold: Rc<[Op]>,
    ranges: Rc<[(u32, u32)]>,
    parameter_ends: Rc<[Option<u32>]>,
    constants: Rc<[ConstantPool]>,
    metadata: Rc<[Vec<InstructionMeta>]>,
    register_counts: Rc<[u16]>,
    frame_register_counts: Rc<[u16]>,
    quickening_sites: Rc<[Box<[std::cell::RefCell<crate::quickening::QuickeningSite<4>>]>]>,
    operand_windows: Rc<[Vec<Rc<[u16]>>]>,
    catch_ranges: Rc<[Vec<CatchRange>]>,
}

impl CodeStore {
    pub fn code(&self, range: CodeRange) -> Option<CodeView<'_>> {
        let (start, end) = self.ranges.get(range.code.0 as usize).copied()?;
        (range.start >= start && range.end <= end).then_some(CodeView { store: self, range })
    }
    /// Rare metadata lives out of line from hot instructions.
    pub fn metadata(&self, range: CodeRange) -> Option<&[InstructionMeta]> {
        self.metadata.get(range.code.0 as usize).map(Vec::as_slice)
    }
    /// Return the immutable pool used by execution and diagnostics.
    pub fn constant_pool(&self, range: CodeRange) -> ConstantPool {
        self.constants
            .get(range.code.0 as usize)
            .cloned()
            .unwrap_or_else(ConstantPool::empty)
    }

    pub fn range_len(&self, code: CodeId) -> Option<u32> {
        let (start, end) = self.ranges.get(code.0 as usize).copied()?;
        Some(end.saturating_sub(start))
    }

    pub fn register_count(&self, code: CodeId) -> Option<u16> {
        self.register_counts.get(code.0 as usize).copied()
    }

    pub fn frame_register_count(&self, code: CodeId) -> Option<u16> {
        self.frame_register_counts.get(code.0 as usize).copied()
    }
}

#[derive(Clone, Copy)]
pub struct CodeView<'a> {
    store: &'a CodeStore,
    range: CodeRange,
}

/// Execution tier selected for one immutable function body.
///
/// The tier is mutable metadata, not a second semantic representation: every
/// baseline entry still points at the canonical compact instruction and uses
/// the ordinary handler on a miss or unsupported operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionTier {
    Interpreter,
    Baseline,
    /// Quench-specific extra promotion layer. It re-wraps already compiled
    /// baseline entries as a physical execution view, not a second semantic
    /// IR and not the (nonexistent) optimizing JIT in Deegen's two-tier paper.
    Optimizing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TierTransition {
    Cold,
    CompileBaseline,
    Baseline,
    CompileOptimizing,
    Optimizing,
}

/// Observable, bounded profile for one function body.  This is a snapshot of
/// admission facts, not a second execution representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TierProfile {
    pub tier: ExecutionTier,
    pub invocations: u32,
    pub retired: u64,
    pub baseline_instructions: usize,
    pub optimizing_instructions: usize,
    pub osr_entries: usize,
    /// Number of interpreter back-edges that transferred the live frame into
    /// an already compiled baseline plan.  This is bounded per function and
    /// makes OSR admission observable in profiling/tests without adding a
    /// second execution representation.
    pub osr_transfers: u64,
}

/// Optional machine-code leaf for generated Number binary-operation stencils.
/// It is deliberately narrow: any non-number input or stencil failure returns
/// to the ordinary handler, so this cannot create an alternate JS semantics.
macro_rules! reset_installed {
    ($plan:expr, $empty:expr) => {{
        $plan.installed = $empty;
        $plan.physical.clear();
    }};
}

macro_rules! invoke_shared_entry {
    ($shared:expr, $owned:expr, $invoke:expr) => {{
        crate::stencil_arena::SharedStencilSlab::acquire_owned(&$shared, $owned)
            .and_then(|lease| lease.invoke($invoke))
    }};
}

#[derive(Clone, Copy)]
enum InstalledBinaryEntry {
    Unpublished,
    F64Local(usize),
    F64Shared(crate::stencil_arena::EntryToken<extern "C" fn(f64, f64) -> f64>),
    BoolLocal(usize),
    BoolShared(crate::stencil_arena::EntryToken<extern "C" fn(f64, f64) -> u64>),
    I32Local(usize),
    I32Shared(crate::stencil_arena::EntryToken<extern "C" fn(i32, i32) -> i32>),
    U32Local(usize),
    U32Shared(crate::stencil_arena::EntryToken<extern "C" fn(u32, u32) -> u32>),
    TaggedLocal(usize),
    TaggedShared(crate::stencil_arena::EntryToken<extern "C" fn(u64, u64) -> u64>),
    CompareBranchShared(
        crate::stencil_arena::EntryToken<
            extern "C" fn(*mut crate::native_control::NativeCompareBranchContext) -> u32,
        >,
    ),
}

#[derive(Clone, Copy, Debug)]
enum BinarySemantic {
    Numeric {
        returns_boolean: bool,
    },
    Integer {
        operator: crate::ops::BinaryOp,
        unsigned: bool,
    },
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CompareBranch {
    control: crate::stencil_cfg::RegionControlPlan,
    physical_key: Option<crate::stencil_fact::RegionKey>,
}

pub(crate) struct NativeBinaryPlan {
    // Mapping is lazy: compiling a baseline plan only records the admitted
    // leaf.  The disposable executable arena is created on first proven
    // numeric execution, so a cold function cannot allocate native code for
    // every arithmetic instruction it happens to contain.
    arena: Option<crate::stencil_arena::StencilArena>,
    shared_arena: Option<std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>>,
    physical: PhysicalState,
    // Numeric leaves currently have no dynamic holes, but retaining one site
    // keeps the patch-value view stable and avoids constructing a fresh cache
    // object on every native execution.
    site: crate::quickening::QuickeningSite<4>,
    opcode: crate::ir::Opcode,
    key: crate::stencil_fact::RegionKey,
    tagged_key: Option<crate::stencil_fact::RegionKey>,
    semantic: BinarySemantic,
    compare_branch: Option<CompareBranch>,
    /// Once the first render has passed all arena and fact checks, retain the
    /// typed entry pointer. Numeric stencil bytes have no mutable VM state;
    /// re-running lifecycle, cache, mprotect, and address checks on every
    /// iteration otherwise costs more than the floating-point instruction.
    installed: InstalledBinaryEntry,
    #[cfg(test)]
    native_entry_count: u64,
    #[cfg(test)]
    last_native_view: Option<crate::stencil_select::PhysicalStencilView>,
}

#[inline]
fn number_to_int32(value: f64) -> i32 {
    // The canonical ToInt32 conversion is shared with the ordinary handler;
    // native bitwise entries therefore preserve NaN/infinity, truncation and
    // modulo-2^32 behavior instead of rejecting those Number values.
    crate::intl::tolocale::value::to_int32(value)
}

impl NativeBinaryPlan {
    pub(crate) fn new_with_shared(
        instruction: crate::ir::Instruction,
        policy: crate::stencil_policy::ExecutionPolicy,
        shared_arena: std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        let mut plan = Self::new(instruction, policy)?;
        plan.shared_arena = Some(shared_arena);
        Some(plan)
    }

    fn new(
        instruction: crate::ir::Instruction,
        policy: crate::stencil_policy::ExecutionPolicy,
    ) -> Option<Self> {
        if !policy.native_leaves {
            return None;
        }
        let opcode = instruction.opcode;
        let (key, semantic) = if opcode.numeric_operator().is_some() {
            (
                crate::stencil_select::numeric_region_key(opcode)?,
                BinarySemantic::Numeric {
                    returns_boolean: false,
                },
            )
        } else if opcode == crate::ir::Opcode::IncI {
            (
                crate::stencil_select::increment_region_key(),
                BinarySemantic::Numeric {
                    returns_boolean: false,
                },
            )
        } else if opcode == crate::ir::Opcode::Binary {
            let key = match crate::ir::compact_binary_operator(instruction.flags) {
                Some(crate::ops::BinaryOp::Equal | crate::ops::BinaryOp::StrictEqual) => {
                    crate::stencil_select::compare_equal_region_key()
                }
                Some(crate::ops::BinaryOp::NotEqual | crate::ops::BinaryOp::StrictNotEqual) => {
                    crate::stencil_select::compare_not_equal_region_key()
                }
                Some(crate::ops::BinaryOp::BitwiseAnd) => {
                    return Self::new_integer(
                        instruction,
                        policy,
                        crate::stencil_select::bitwise_and_region_key(),
                        crate::ops::BinaryOp::BitwiseAnd,
                        false,
                    )
                }
                Some(crate::ops::BinaryOp::BitwiseOr) => {
                    return Self::new_integer(
                        instruction,
                        policy,
                        crate::stencil_select::bitwise_or_region_key(),
                        crate::ops::BinaryOp::BitwiseOr,
                        false,
                    )
                }
                Some(crate::ops::BinaryOp::BitwiseXor) => {
                    return Self::new_integer(
                        instruction,
                        policy,
                        crate::stencil_select::bitwise_xor_region_key(),
                        crate::ops::BinaryOp::BitwiseXor,
                        false,
                    )
                }
                Some(crate::ops::BinaryOp::ShiftLeft) => {
                    return Self::new_integer(
                        instruction,
                        policy,
                        crate::stencil_select::shift_left_region_key(),
                        crate::ops::BinaryOp::ShiftLeft,
                        false,
                    )
                }
                Some(crate::ops::BinaryOp::ShiftRight) => {
                    return Self::new_integer(
                        instruction,
                        policy,
                        crate::stencil_select::shift_right_region_key(),
                        crate::ops::BinaryOp::ShiftRight,
                        false,
                    )
                }
                Some(crate::ops::BinaryOp::ShiftRightZeroFill) => {
                    return Self::new_integer(
                        instruction,
                        policy,
                        crate::stencil_select::shift_right_zero_region_key(),
                        crate::ops::BinaryOp::ShiftRightZeroFill,
                        true,
                    )
                }
                Some(crate::ops::BinaryOp::LessThan) => {
                    crate::stencil_select::compare_less_region_key()
                }
                Some(crate::ops::BinaryOp::LessEqual) => {
                    crate::stencil_select::compare_less_equal_region_key()
                }
                Some(crate::ops::BinaryOp::GreaterThan) => {
                    crate::stencil_select::compare_greater_region_key()
                }
                Some(crate::ops::BinaryOp::GreaterEqual) => {
                    crate::stencil_select::compare_greater_equal_region_key()
                }
                _ => return None,
            };
            (
                key,
                BinarySemantic::Numeric {
                    returns_boolean: true,
                },
            )
        } else {
            return None;
        };
        let tagged_key = match instruction.flags {
            flag if flag == crate::ir::compact_binary_id(crate::ops::BinaryOp::StrictEqual) => {
                Some(crate::stencil_select::compare_equal_word_region_key())
            }
            flag if flag == crate::ir::compact_binary_id(crate::ops::BinaryOp::StrictNotEqual) => {
                Some(crate::stencil_select::compare_not_equal_word_region_key())
            }
            _ => None,
        };
        // The AddConst stencil has a fixed machine operand order (source in
        // xmm0, embedded constant in xmm1).  Constant-left addition is
        // observationally distinct for signed zero, so keep that variant on
        // the canonical Rust arithmetic handler instead of silently swapping
        // the operands in native code.
        if opcode == crate::ir::Opcode::AddConst && instruction.add_const_is_left() {
            return None;
        }
        // Build-generated rows are the sole executable admission fact.  ARM64
        // rows are real but opt-in until their measured call overhead is
        // recovered; the ordinary baseline remains the default fallback.
        crate::stencil_select::select_region(key)
            .filter(|record| record.executable && validate_physical_template(record).is_ok())?;
        if let Some(tagged_key) = tagged_key {
            crate::stencil_select::select_region(tagged_key)
                .filter(|record| record.executable && validate_physical_template(record).is_ok())?;
        }
        Some(Self {
            arena: None,
            shared_arena: None,
            physical: PhysicalState::new(),
            site: crate::quickening::QuickeningSite::new(opcode),
            opcode,
            key,
            tagged_key,
            semantic,
            compare_branch: None,
            installed: InstalledBinaryEntry::Unpublished,
            #[cfg(test)]
            native_entry_count: 0,
            #[cfg(test)]
            last_native_view: None,
        })
    }

    fn new_integer(
        instruction: crate::ir::Instruction,
        policy: crate::stencil_policy::ExecutionPolicy,
        key: crate::stencil_fact::RegionKey,
        operator: crate::ops::BinaryOp,
        unsigned: bool,
    ) -> Option<Self> {
        if !policy.native_leaves
            || instruction.opcode != crate::ir::Opcode::Binary
            || !crate::stencil_select::select_region(key).is_some_and(|record| {
                record.executable
                    && record.abi
                        == if unsigned {
                            crate::stencil_select::RegionAbi::ScalarU32
                        } else {
                            crate::stencil_select::RegionAbi::ScalarI32
                        }
                    && validate_physical_template(record).is_ok()
            })
        {
            return None;
        }
        Some(Self {
            arena: None,
            shared_arena: None,
            physical: PhysicalState::new(),
            site: crate::quickening::QuickeningSite::new(instruction.opcode),
            opcode: instruction.opcode,
            key,
            tagged_key: None,
            semantic: BinarySemantic::Integer { operator, unsigned },
            compare_branch: None,
            installed: InstalledBinaryEntry::Unpublished,
            #[cfg(test)]
            native_entry_count: 0,
            #[cfg(test)]
            last_native_view: None,
        })
    }

    #[inline]
    fn note_native_entry(&mut self) {
        self.note_native_entry_for(self.key);
    }

    #[inline]
    fn note_native_entry_for(&mut self, key: crate::stencil_fact::RegionKey) {
        #[cfg(test)]
        {
            self.native_entry_count = self.native_entry_count.saturating_add(1);
            self.last_native_view = crate::stencil_select::select_physical(key);
        }
        #[cfg(not(test))]
        let _ = key;
    }

    #[cfg(test)]
    pub(crate) fn native_entry_count(&self) -> u64 {
        self.native_entry_count
    }

    #[cfg(test)]
    pub(crate) fn last_native_view(&self) -> Option<crate::stencil_select::PhysicalStencilView> {
        self.last_native_view
    }

    #[inline]
    fn clear_physical_capabilities(&mut self) {
        reset_installed!(self, InstalledBinaryEntry::Unpublished);
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn execute_tagged(
        &mut self,
        lhs: u64,
        rhs: u64,
    ) -> Result<bool, crate::stencil_arena::ArenaError> {
        let key = self
            .tagged_key
            .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
        if self.shared_arena.is_none() {
            if let InstalledBinaryEntry::TaggedLocal(address) = self.installed {
                if let Some(arena) = self.arena.as_ref() {
                    if let Ok(entry) = arena.word_pair_bool_entry(address) {
                        self.note_native_entry();
                        return Ok(entry(lhs, rhs) != 0);
                    }
                }
                self.installed = InstalledBinaryEntry::Unpublished;
            }
        }
        if let (Some(shared), InstalledBinaryEntry::TaggedShared(owned)) =
            (self.shared_arena.clone(), self.installed)
        {
            if let Ok(result) = invoke_shared_entry!(shared, owned, |entry| entry(lhs, rhs)) {
                self.note_native_entry();
                return Ok(result != 0);
            }
            self.installed = InstalledBinaryEntry::Unpublished;
        }
        if let Some(shared) = self.shared_arena.clone() {
            let values = crate::stencil_fact::PatchValues::from_site(&self.site);
            let address = {
                let mut slab = shared.borrow_mut();
                let view = crate::stencil_select::select_physical_for_abi(
                    key,
                    crate::stencil_select::RegionAbi::ScalarWordPairBool,
                )
                .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
                let address =
                    slab.render_physical_view_or_get(&mut self.physical.cache, view, &values)?;
                slab.make_executable(address)?;
                address
            };
            drop(values);
            let owned = shared.borrow().owned_word_pair_bool_entry(address)?;
            self.installed = InstalledBinaryEntry::TaggedShared(owned);
            return match invoke_shared_entry!(shared, owned, |entry| entry(lhs, rhs)) {
                Ok(result) => {
                    self.note_native_entry();
                    Ok(result != 0)
                }
                Err(error) => {
                    self.clear_physical_capabilities();
                    Err(error)
                }
            };
        }
        if self.arena.is_none() {
            self.arena = Some(crate::stencil_arena::StencilArena::new(4096)?);
        }
        let values = crate::stencil_fact::PatchValues::from_site(&self.site);
        let arena = self
            .arena
            .as_mut()
            .ok_or(crate::stencil_arena::ArenaError::MappingFailed)?;
        let view = crate::stencil_select::select_physical_for_abi(
            key,
            crate::stencil_select::RegionAbi::ScalarWordPairBool,
        )
        .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
        let address = arena.render_physical_view_or_get(&mut self.physical.cache, view, &values)?;
        arena.make_executable()?;
        let entry = arena.word_pair_bool_entry(address)?;
        self.installed = InstalledBinaryEntry::TaggedLocal(address);
        self.note_native_entry();
        Ok(entry(lhs, rhs) != 0)
    }

    #[inline]
    pub(crate) fn execute(
        &mut self,
        lhs: f64,
        rhs: f64,
    ) -> Result<f64, crate::stencil_arena::ArenaError> {
        if self.integer_operation().is_some() {
            let left = number_to_int32(lhs);
            let right = number_to_int32(rhs);
            #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
            if self.shared_arena.is_none() {
                if let InstalledBinaryEntry::I32Local(address) = self.installed {
                    if let Some(arena) = self.arena.as_ref() {
                        if let Ok(entry) = arena.i32_entry(address) {
                            self.note_native_entry();
                            return Ok(f64::from(entry(left, right)));
                        }
                    }
                    self.installed = InstalledBinaryEntry::Unpublished;
                }
                if let InstalledBinaryEntry::U32Local(address) = self.installed {
                    if let Some(arena) = self.arena.as_ref() {
                        if let Ok(entry) = arena.u32_entry(address) {
                            self.note_native_entry();
                            return Ok(f64::from(entry(left as u32, right as u32)));
                        }
                    }
                    self.installed = InstalledBinaryEntry::Unpublished;
                }
            }
            #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
            if let Some(shared) = self.shared_arena.clone() {
                if self.is_unsigned_integer() {
                    if let InstalledBinaryEntry::U32Shared(owned) = self.installed {
                        match invoke_shared_entry!(shared, owned, |entry| entry(
                            left as u32,
                            right as u32
                        )) {
                            Ok(result) => {
                                self.note_native_entry();
                                return Ok(f64::from(result));
                            }
                            Err(_) => {
                                self.clear_physical_capabilities();
                            }
                        }
                    }
                } else if let InstalledBinaryEntry::I32Shared(owned) = self.installed {
                    match invoke_shared_entry!(shared, owned, |entry| entry(left, right)) {
                        Ok(result) => {
                            self.note_native_entry();
                            return Ok(f64::from(result));
                        }
                        Err(_) => {
                            self.clear_physical_capabilities();
                        }
                    }
                }
            }
            #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
            if self
                .physical
                .lifecycle
                .observe_site(&self.site, self.key, true)
                == crate::stencil_lifecycle::StencilState::Retired
            {
                return Err(crate::stencil_arena::ArenaError::ProtectionFailed);
            }
            let values = crate::stencil_fact::PatchValues::from_site(&self.site);
            #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
            if let Some(shared) = self.shared_arena.clone() {
                if self.is_unsigned_integer() {
                    let rendered = (|| -> Result<usize, crate::stencil_arena::ArenaError> {
                        let mut slab = shared.borrow_mut();
                        let view = crate::stencil_select::select_physical_for_abi(
                            self.key,
                            crate::stencil_select::RegionAbi::ScalarU32,
                        )
                        .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
                        let address = slab.render_physical_view_or_get(
                            &mut self.physical.cache,
                            view,
                            &values,
                        )?;
                        slab.make_executable(address)?;
                        Ok(address)
                    })();
                    let address = match rendered {
                        Ok(rendered) => rendered,
                        Err(error) => {
                            self.physical.clear();
                            return Err(error);
                        }
                    };
                    let owned = shared.borrow().owned_u32_entry(address)?;
                    let result = match invoke_shared_entry!(shared, owned, |entry| entry(
                        left as u32,
                        right as u32
                    )) {
                        Ok(result) => result,
                        Err(error) => {
                            self.clear_physical_capabilities();
                            return Err(error);
                        }
                    };
                    self.installed = InstalledBinaryEntry::U32Shared(owned);
                    self.note_native_entry();
                    return Ok(f64::from(result));
                }
                let rendered = (|| -> Result<usize, crate::stencil_arena::ArenaError> {
                    let mut slab = shared.borrow_mut();
                    let view = crate::stencil_select::select_physical_for_abi(
                        self.key,
                        crate::stencil_select::RegionAbi::ScalarI32,
                    )
                    .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
                    let address =
                        slab.render_physical_view_or_get(&mut self.physical.cache, view, &values)?;
                    slab.make_executable(address)?;
                    Ok(address)
                })();
                let address = match rendered {
                    Ok(rendered) => rendered,
                    Err(error) => {
                        self.physical.clear();
                        return Err(error);
                    }
                };
                let owned = shared.borrow().owned_i32_entry(address)?;
                let result = match invoke_shared_entry!(shared, owned, |entry| entry(left, right)) {
                    Ok(result) => result,
                    Err(error) => {
                        self.clear_physical_capabilities();
                        return Err(error);
                    }
                };
                self.installed = InstalledBinaryEntry::I32Shared(owned);
                self.note_native_entry();
                return Ok(f64::from(result));
            }
            if self.arena.is_none() {
                self.arena = Some(crate::stencil_arena::StencilArena::new(4096)?);
            }
            let unsigned = self.is_unsigned_integer();
            let arena = self
                .arena
                .as_mut()
                .ok_or(crate::stencil_arena::ArenaError::MappingFailed)?;
            let result = if unsigned {
                arena
                    .render_selected_u32(
                        &mut self.physical.cache,
                        self.key,
                        &values,
                        left as u32,
                        right as u32,
                    )
                    .map(|value| f64::from(value))
            } else {
                arena
                    .render_selected_i32(&mut self.physical.cache, self.key, &values, left, right)
                    .map(|value| f64::from(value))
            };
            #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
            if result.is_ok() {
                let abi = if self.is_unsigned_integer() {
                    crate::stencil_select::RegionAbi::ScalarU32
                } else {
                    crate::stencil_select::RegionAbi::ScalarI32
                };
                let signature = crate::stencil_select::select_physical_for_abi(self.key, abi)
                    .map(|view| crate::stencil_arena::physical_cache_signature(view, &values));
                self.note_native_entry();
                if let Some(arena) = self.arena.as_ref() {
                    if let Some(address) = signature.and_then(|signature| {
                        self.physical
                            .cache
                            .get_owned(self.key, signature, arena.id())
                    }) {
                        if self.is_unsigned_integer() {
                            self.installed = arena
                                .u32_entry(address)
                                .ok()
                                .map(|_| InstalledBinaryEntry::U32Local(address))
                                .unwrap_or(InstalledBinaryEntry::Unpublished);
                        } else {
                            self.installed = arena
                                .i32_entry(address)
                                .ok()
                                .map(|_| InstalledBinaryEntry::I32Local(address))
                                .unwrap_or(InstalledBinaryEntry::Unpublished);
                        }
                    }
                }
            }
            if result.is_err() {
                self.arena.take();
                self.physical.clear();
                #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
                {
                    self.installed = InstalledBinaryEntry::Unpublished;
                }
            }
            return result;
        }
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        if self.shared_arena.is_none() && !self.returns_boolean() {
            if let InstalledBinaryEntry::F64Local(address) = self.installed {
                if let Some(arena) = self.arena.as_ref() {
                    if let Ok(entry) = arena.f64_entry(address) {
                        self.note_native_entry();
                        return Ok(unsafe { invoke_f64x2_entry(entry, lhs, rhs) });
                    }
                }
                self.installed = InstalledBinaryEntry::Unpublished;
            }
        }
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        if !self.returns_boolean() {
            if let (Some(shared), InstalledBinaryEntry::F64Shared(owned)) =
                (self.shared_arena.clone(), self.installed)
            {
                match invoke_shared_entry!(shared, owned, |entry| unsafe {
                    invoke_f64x2_entry(entry, lhs, rhs)
                }) {
                    Ok(result) => {
                        self.note_native_entry();
                        return Ok(result);
                    }
                    Err(_) => {
                        self.clear_physical_capabilities();
                    }
                }
            }
        }
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        if self.returns_boolean() {
            if let (Some(shared), InstalledBinaryEntry::BoolShared(owned)) =
                (self.shared_arena.clone(), self.installed)
            {
                match invoke_shared_entry!(shared, owned, |entry| entry(lhs, rhs)) {
                    Ok(result) => {
                        self.note_native_entry();
                        return Ok(if result != 0 { 1.0 } else { 0.0 });
                    }
                    Err(_) => self.clear_physical_capabilities(),
                }
            }
        }
        let values = crate::stencil_fact::PatchValues::from_site(&self.site);
        let values = (self.opcode == crate::ir::Opcode::AddConst
            || self.opcode == crate::ir::Opcode::IncI)
            .then(|| values.with_constant_bits(rhs.to_bits()))
            .unwrap_or(values);
        let key = self.key;
        // Check the generated admission row before mapping any pages.  This
        // makes the ARM/non-executable path allocation-free and leaves the
        // canonical Rust handler as the only semantic implementation.
        if !crate::stencil_select::select_region(key).is_some_and(|record| record.executable) {
            return Err(crate::stencil_arena::ArenaError::ProtectionFailed);
        }
        if self.physical.lifecycle.observe_site(&self.site, key, true)
            == crate::stencil_lifecycle::StencilState::Retired
        {
            return Err(crate::stencil_arena::ArenaError::ProtectionFailed);
        }
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        if let Some(shared) = self.shared_arena.clone() {
            if self.returns_boolean() {
                let rendered = (|| {
                    let mut slab = shared.borrow_mut();
                    let view = crate::stencil_select::select_physical_for_abi(
                        key,
                        crate::stencil_select::RegionAbi::ScalarBool,
                    )
                    .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
                    let address =
                        slab.render_physical_view_or_get(&mut self.physical.cache, view, &values)?;
                    slab.make_executable(address)?;
                    Ok(address)
                })();
                return match rendered {
                    Ok(address) => {
                        let owned = shared.borrow().owned_bool_entry(address)?;
                        self.installed = InstalledBinaryEntry::BoolShared(owned);
                        match invoke_shared_entry!(shared, owned, |entry| entry(lhs, rhs) != 0) {
                            Ok(value) => {
                                self.note_native_entry();
                                Ok(if value { 1.0 } else { 0.0 })
                            }
                            Err(error) => {
                                self.clear_physical_capabilities();
                                Err(error)
                            }
                        }
                    }
                    Err(error) => {
                        self.physical.clear();
                        Err(error)
                    }
                };
            }
            let rendered = (|| {
                let mut slab = shared.borrow_mut();
                let view = crate::stencil_select::select_physical_for_abi(
                    key,
                    crate::stencil_select::RegionAbi::ScalarF64Binary,
                )
                .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
                let address =
                    slab.render_physical_view_or_get(&mut self.physical.cache, view, &values)?;
                slab.make_executable(address)?;
                Ok(address)
            })();
            let address = match rendered {
                Ok(rendered) => rendered,
                Err(error) => {
                    self.physical.clear();
                    return Err(error);
                }
            };
            let owned = shared.borrow().owned_f64_entry(address)?;
            self.installed = InstalledBinaryEntry::F64Shared(owned);
            return match invoke_shared_entry!(shared, owned, |entry| unsafe {
                invoke_f64x2_entry(entry, lhs, rhs)
            }) {
                Ok(result) => {
                    self.note_native_entry();
                    Ok(result)
                }
                Err(error) => {
                    self.clear_physical_capabilities();
                    Err(error)
                }
            };
        }
        if self.arena.is_none() {
            match crate::stencil_arena::StencilArena::new(4096) {
                Ok(arena) => self.arena = Some(arena),
                Err(error) => {
                    self.physical.lifecycle.reset();
                    return Err(error);
                }
            }
        }
        // The allocation above is fallible and has been stored only after it
        // succeeds, so the executable leaf always retains the ordinary
        // fallback on mapping failure instead of panicking.
        let result = if self.returns_boolean() {
            self.arena
                .as_mut()
                .ok_or(crate::stencil_arena::ArenaError::MappingFailed)?
                .render_selected_bool(&mut self.physical.cache, key, &values, lhs, rhs)
                .map(|value| if value { 1.0 } else { 0.0 })
        } else {
            let arena = self
                .arena
                .as_mut()
                .ok_or(crate::stencil_arena::ArenaError::MappingFailed)?;
            arena.render_selected_f64(&mut self.physical.cache, key, &values, lhs, rhs, || {
                Err(crate::stencil_arena::ArenaError::ProtectionFailed)
            })
        };
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        if result.is_ok() && !self.returns_boolean() {
            if let Some(view) = crate::stencil_select::select_physical(key) {
                // The arena deliberately canonicalizes hole-free stencils to
                // signature zero.  Use the same rule here; otherwise the
                // cached pointer would never be found for the common Add,
                // Sub, Mul, and Div leaves and the boundary tax would return
                // on every iteration.
                let signature = crate::stencil_arena::physical_cache_signature(view, &values);
                if let Some(arena) = self.arena.as_ref() {
                    if let Some(address) = self.physical.cache.get_owned(key, signature, arena.id())
                    {
                        self.installed = arena
                            .f64_entry(address)
                            .ok()
                            .map(|_| InstalledBinaryEntry::F64Local(address))
                            .unwrap_or(InstalledBinaryEntry::Unpublished);
                    }
                }
            }
        }
        if result.is_err() {
            // Any failed render/protection/patch leaves no installed physical
            // view. Drop the disposable mapping, cache, and lifecycle state
            // instead of retrying into stale writable/exhausted storage; the
            // caller then takes the complete Rust semantic fallback.
            self.arena.take();
            self.physical.clear();
            #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
            {
                self.installed = InstalledBinaryEntry::Unpublished;
            }
        }
        result
    }

    fn integer_operation(&self) -> Option<(crate::ops::BinaryOp, bool)> {
        match self.semantic {
            BinarySemantic::Integer { operator, unsigned } => Some((operator, unsigned)),
            BinarySemantic::Numeric { .. } => None,
        }
    }

    fn is_unsigned_integer(&self) -> bool {
        self.integer_operation()
            .is_some_and(|(_, unsigned)| unsigned)
    }

    pub(crate) fn returns_boolean(&self) -> bool {
        matches!(
            self.semantic,
            BinarySemantic::Numeric {
                returns_boolean: true
            }
        )
    }

    fn install_compare_branch(&mut self, branch: CompareBranch) {
        if self.returns_boolean() {
            self.compare_branch = Some(branch);
        }
    }

    #[cfg(target_arch = "aarch64")]
    pub(crate) fn execute_compare_branch(
        &mut self,
        pc: usize,
        lhs: f64,
        rhs: f64,
    ) -> Result<crate::native_control::NativeCompareBranchOutcome, crate::stencil_arena::ArenaError>
    {
        let branch = self
            .compare_branch
            .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
        let key = branch
            .physical_key
            .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
        (pc == branch.control.start())
            .then_some(())
            .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
        let (false_pc, true_pc) = branch
            .control
            .terminal_conditional_exits()
            .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
        self.execute_compare_branch_with_key(key, lhs, rhs, true_pc, false_pc)
    }

    #[cfg(target_arch = "aarch64")]
    fn execute_compare_branch_with_key(
        &mut self,
        key: crate::stencil_fact::RegionKey,
        lhs: f64,
        rhs: f64,
        true_pc: usize,
        false_pc: usize,
    ) -> Result<crate::native_control::NativeCompareBranchOutcome, crate::stencil_arena::ArenaError>
    {
        let shared = self
            .shared_arena
            .clone()
            .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
        if let InstalledBinaryEntry::CompareBranchShared(token) = self.installed {
            let result = invoke_compare_branch(&shared, token, lhs, rhs, true_pc, false_pc)?;
            self.note_native_entry_for(key);
            return Ok(result);
        }
        let token = self.publish_compare_branch(&shared, key)?;
        let result = invoke_compare_branch(&shared, token, lhs, rhs, true_pc, false_pc)?;
        self.installed = InstalledBinaryEntry::CompareBranchShared(token);
        self.note_native_entry_for(key);
        Ok(result)
    }

    #[cfg(target_arch = "aarch64")]
    fn publish_compare_branch(
        &mut self,
        shared: &std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>,
        key: crate::stencil_fact::RegionKey,
    ) -> Result<
        crate::stencil_arena::EntryToken<
            extern "C" fn(*mut crate::native_control::NativeCompareBranchContext) -> u32,
        >,
        crate::stencil_arena::ArenaError,
    > {
        let values = crate::stencil_fact::PatchValues::from_site(&self.site);
        let address = {
            let mut slab = shared.borrow_mut();
            let view = crate::stencil_select::select_physical_for_abi(
                key,
                crate::stencil_select::RegionAbi::CompareBranch,
            )
            .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
            crate::stencil_region_layout::validate_compare_branch_control(
                view,
                &self
                    .compare_branch
                    .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?
                    .control,
            )
            .map_err(|_| crate::stencil_arena::ArenaError::ProtectionFailed)?;
            let address =
                slab.render_physical_view_or_get(&mut self.physical.cache, view, &values)?;
            slab.make_executable(address)?;
            address
        };
        shared.borrow().owned_compare_branch_entry(address)
    }

    pub(crate) fn compare_branch_next(&self, pc: usize, value: bool) -> Option<usize> {
        let branch = self.compare_branch?;
        (pc == branch.control.start()).then_some(())?;
        let (false_pc, true_pc) = branch.control.terminal_conditional_exits()?;
        Some(if value { true_pc } else { false_pc })
    }

    #[cfg(test)]
    pub(crate) fn has_compare_branch(&self) -> bool {
        self.compare_branch.is_some()
    }

    #[cfg(test)]
    pub(crate) fn compare_branch_span(&self) -> Option<usize> {
        self.compare_branch.map(|branch| branch.control.span_len())
    }
}

impl std::fmt::Debug for NativeBinaryPlan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeBinaryPlan")
            .field("opcode", &self.opcode)
            .field("semantic", &self.semantic)
            .field(
                "used_bytes",
                &self
                    .shared_arena
                    .as_ref()
                    .map(|arena| arena.borrow().used())
                    .or_else(|| self.arena.as_ref().map(|arena| arena.used()))
                    .unwrap_or(0),
            )
            .field("cache_len", &self.physical.cache.len())
            .finish()
    }
}

#[cfg(target_arch = "aarch64")]
fn invoke_compare_branch(
    shared: &std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>,
    token: crate::stencil_arena::EntryToken<
        extern "C" fn(*mut crate::native_control::NativeCompareBranchContext) -> u32,
    >,
    lhs: f64,
    rhs: f64,
    true_pc: usize,
    false_pc: usize,
) -> Result<crate::native_control::NativeCompareBranchOutcome, crate::stencil_arena::ArenaError> {
    let mut context =
        crate::native_control::NativeCompareBranchContext::new(lhs, rhs, true_pc, false_pc);
    let status = crate::stencil_arena::SharedStencilSlab::acquire_owned(shared, token)?
        .invoke(|entry| entry(&mut context))?;
    context
        .finish(status)
        .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)
}

fn constant_word_bits(constant: &crate::ops::Constant) -> Option<u64> {
    let value = match constant {
        crate::ops::Constant::Number(value) => crate::value::Value::Number(*value),
        crate::ops::Constant::Boolean(value) => crate::value::Value::Boolean(*value),
        crate::ops::Constant::Null => crate::value::Value::Null,
        crate::ops::Constant::Undefined => crate::value::Value::Undefined,
        crate::ops::Constant::String(_)
        | crate::ops::Constant::StringUnits(_)
        | crate::ops::Constant::BigInt(_) => return None,
    };
    value.to_tagged().map(|tagged| tagged.bits())
}

/// Numeric ToBoolean leaf. Objects, strings and symbols retain the complete
/// coercion gateway; this body only handles Number values with no re-entry.
#[derive(Clone, Copy)]
enum InstalledTruthinessEntry {
    Unpublished,
    NumberLocal(usize),
    NumberShared(crate::stencil_arena::EntryToken<extern "C" fn(f64) -> u64>),
    WordLocal(usize),
    WordShared(crate::stencil_arena::EntryToken<extern "C" fn(u64) -> u64>),
    PointerLocal(usize),
    PointerShared(crate::stencil_arena::EntryToken<extern "C" fn(u64) -> u64>),
}

pub(crate) struct NativeTruthinessPlan {
    key: crate::stencil_fact::RegionKey,
    word_key: crate::stencil_fact::RegionKey,
    pointer_key: crate::stencil_fact::RegionKey,
    arena: Option<crate::stencil_arena::StencilArena>,
    shared_arena: Option<std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>>,
    physical: PhysicalState,
    site: crate::quickening::QuickeningSite<2>,
    installed: InstalledTruthinessEntry,
    #[cfg(test)]
    native_entry_count: u64,
}

impl NativeTruthinessPlan {
    #[inline]
    fn clear_shared_capabilities(&mut self) {
        reset_installed!(self, InstalledTruthinessEntry::Unpublished);
    }

    #[inline]
    fn note_entry(&mut self) {
        #[cfg(test)]
        {
            self.native_entry_count = self.native_entry_count.saturating_add(1);
        }
    }

    fn new_with_shared(
        instruction: crate::ir::Instruction,
        policy: crate::stencil_policy::ExecutionPolicy,
        shared: std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        let mut plan = Self::new(instruction, policy)?;
        plan.shared_arena = Some(shared);
        Some(plan)
    }

    fn new(
        instruction: crate::ir::Instruction,
        policy: crate::stencil_policy::ExecutionPolicy,
    ) -> Option<Self> {
        let valid_instruction = instruction.opcode == crate::ir::Opcode::JumpIfFalse
            || (instruction.opcode == crate::ir::Opcode::Unary
                && instruction.flags == crate::ir::compact_unary_id(crate::ops::UnaryOp::Not));
        let key = crate::stencil_select::truthy_number_region_key();
        let word_key = crate::stencil_select::truthy_word_region_key();
        let pointer_key = crate::stencil_select::truthy_pointer_word_region_key();
        (policy.native_leaves
            && valid_instruction
            && crate::stencil_select::select_region(key).is_some_and(|record| {
                record.executable
                    && record.abi == crate::stencil_select::RegionAbi::ScalarBool
                    && validate_physical_template(record).is_ok()
            })
            && crate::stencil_select::select_region(word_key).is_some_and(|record| {
                record.executable
                    && record.abi == crate::stencil_select::RegionAbi::ScalarWordBool
                    && validate_physical_template(record).is_ok()
            })
            && crate::stencil_select::select_region(pointer_key).is_some_and(|record| {
                record.executable
                    && record.abi == crate::stencil_select::RegionAbi::ScalarWordBool
                    && validate_physical_template(record).is_ok()
            }))
        .then_some(Self {
            key,
            word_key,
            pointer_key,
            arena: None,
            shared_arena: None,
            physical: PhysicalState::new(),
            site: crate::quickening::QuickeningSite::new(instruction.opcode),
            installed: InstalledTruthinessEntry::Unpublished,
            #[cfg(test)]
            native_entry_count: 0,
        })
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn execute(&mut self, value: f64) -> Result<bool, crate::stencil_arena::ArenaError> {
        let values = crate::stencil_fact::PatchValues::from_site(&self.site);
        if let Some(shared) = self.shared_arena.clone() {
            if let InstalledTruthinessEntry::NumberShared(owned) = self.installed {
                if let Ok(result) = invoke_shared_entry!(shared, owned, |entry| entry(value)) {
                    #[cfg(test)]
                    {
                        self.native_entry_count = self.native_entry_count.saturating_add(1);
                    }
                    return Ok(result != 0);
                }
                self.clear_shared_capabilities();
            }
            let address = {
                let values = crate::stencil_fact::PatchValues::from_site(&self.site);
                let mut slab = shared.borrow_mut();
                let view = crate::stencil_select::select_physical_for_abi(
                    self.key,
                    crate::stencil_select::RegionAbi::ScalarBool,
                )
                .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
                let address =
                    slab.render_physical_view_or_get(&mut self.physical.cache, view, &values)?;
                slab.make_executable(address)?;
                address
            };
            let owned = shared.borrow().owned_bool_unary_entry(address)?;
            self.installed = InstalledTruthinessEntry::NumberShared(owned);
            return match invoke_shared_entry!(shared, owned, |entry| entry(value)) {
                Ok(result) => {
                    #[cfg(test)]
                    {
                        self.native_entry_count = self.native_entry_count.saturating_add(1);
                    }
                    Ok(result != 0)
                }
                Err(error) => {
                    self.clear_shared_capabilities();
                    Err(error)
                }
            };
        }
        if let InstalledTruthinessEntry::NumberLocal(address) = self.installed {
            if let Some(arena) = self.arena.as_ref() {
                if let Ok(entry) = arena.bool_unary_entry(address) {
                    #[cfg(test)]
                    {
                        self.native_entry_count = self.native_entry_count.saturating_add(1);
                    }
                    return Ok(entry(value) != 0);
                }
            }
            self.installed = InstalledTruthinessEntry::Unpublished;
        }
        if self.arena.is_none() {
            self.arena = Some(crate::stencil_arena::StencilArena::new(4096)?);
        }
        let values = crate::stencil_fact::PatchValues::from_site(&self.site);
        let arena = self
            .arena
            .as_mut()
            .ok_or(crate::stencil_arena::ArenaError::MappingFailed)?;
        let view = crate::stencil_select::select_physical_for_abi(
            self.key,
            crate::stencil_select::RegionAbi::ScalarBool,
        )
        .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
        let address = arena.render_physical_view_or_get(&mut self.physical.cache, view, &values)?;
        arena.make_executable()?;
        let entry = arena.bool_unary_entry(address)?;
        self.installed = InstalledTruthinessEntry::NumberLocal(address);
        #[cfg(test)]
        {
            self.native_entry_count = self.native_entry_count.saturating_add(1);
        }
        Ok(entry(value) != 0)
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn execute_word(
        &mut self,
        value: u64,
    ) -> Result<bool, crate::stencil_arena::ArenaError> {
        let values = crate::stencil_fact::PatchValues::from_site(&self.site)
            .with_constant_bits(crate::tagged_value::TaggedValue::bool(true).bits());
        if let Some(shared) = self.shared_arena.clone() {
            if let InstalledTruthinessEntry::WordShared(owned) = self.installed {
                if let Ok(result) = invoke_shared_entry!(shared, owned, |entry| entry(value)) {
                    #[cfg(test)]
                    {
                        self.native_entry_count = self.native_entry_count.saturating_add(1);
                    }
                    return Ok(result != 0);
                }
                self.installed = InstalledTruthinessEntry::Unpublished;
            }
            let mut slab = shared.borrow_mut();
            let view = crate::stencil_select::select_physical_for_abi(
                self.word_key,
                crate::stencil_select::RegionAbi::ScalarWordBool,
            )
            .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
            let address =
                slab.render_physical_view_or_get(&mut self.physical.cache, view, &values)?;
            slab.make_executable(address)?;
            let entry = slab.word_bool_entry(address)?;
            drop(slab);
            let owned = shared.borrow().owned_word_bool_entry(address)?;
            self.installed = InstalledTruthinessEntry::WordShared(owned);
            let result = match invoke_shared_entry!(shared, owned, |entry| entry(value)) {
                Ok(result) => result,
                Err(error) => {
                    self.clear_shared_capabilities();
                    return Err(error);
                }
            };
            #[cfg(test)]
            {
                self.native_entry_count = self.native_entry_count.saturating_add(1);
            }
            return Ok(result != 0);
        }
        if let InstalledTruthinessEntry::WordLocal(address) = self.installed {
            if let Some(arena) = self.arena.as_ref() {
                if let Ok(entry) = arena.word_bool_entry(address) {
                    #[cfg(test)]
                    {
                        self.native_entry_count = self.native_entry_count.saturating_add(1);
                    }
                    return Ok(entry(value) != 0);
                }
            }
            self.installed = InstalledTruthinessEntry::Unpublished;
        }
        if self.arena.is_none() {
            self.arena = Some(crate::stencil_arena::StencilArena::new(4096)?);
        }
        let arena = self
            .arena
            .as_mut()
            .ok_or(crate::stencil_arena::ArenaError::MappingFailed)?;
        let view = crate::stencil_select::select_physical_for_abi(
            self.word_key,
            crate::stencil_select::RegionAbi::ScalarWordBool,
        )
        .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
        let address = arena.render_physical_view_or_get(&mut self.physical.cache, view, &values)?;
        arena.make_executable()?;
        let entry = arena.word_bool_entry(address)?;
        self.installed = InstalledTruthinessEntry::WordLocal(address);
        #[cfg(test)]
        {
            self.native_entry_count = self.native_entry_count.saturating_add(1);
        }
        Ok(entry(value) != 0)
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn execute_pointer(
        &mut self,
        value: u64,
    ) -> Result<bool, crate::stencil_arena::ArenaError> {
        let values = crate::stencil_fact::PatchValues::from_site(&self.site);
        if let Some(shared) = self.shared_arena.clone() {
            if let InstalledTruthinessEntry::PointerShared(owned) = self.installed {
                if let Ok(result) = invoke_shared_entry!(shared, owned, |entry| entry(value)) {
                    self.note_entry();
                    return Ok(result != 0);
                }
                self.installed = InstalledTruthinessEntry::Unpublished;
            }
            let mut slab = shared.borrow_mut();
            let view = crate::stencil_select::select_physical_for_abi(
                self.pointer_key,
                crate::stencil_select::RegionAbi::ScalarWordBool,
            )
            .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
            let address =
                slab.render_physical_view_or_get(&mut self.physical.cache, view, &values)?;
            slab.make_executable(address)?;
            let entry = slab.word_bool_entry(address)?;
            drop(slab);
            let owned = shared.borrow().owned_word_bool_entry(address)?;
            self.installed = InstalledTruthinessEntry::PointerShared(owned);
            let result = match invoke_shared_entry!(shared, owned, |entry| entry(value)) {
                Ok(result) => result,
                Err(error) => {
                    self.clear_shared_capabilities();
                    return Err(error);
                }
            };
            self.note_entry();
            return Ok(result != 0);
        }
        if let InstalledTruthinessEntry::PointerLocal(address) = self.installed {
            if let Some(arena) = self.arena.as_ref() {
                if let Ok(entry) = arena.word_bool_entry(address) {
                    self.note_entry();
                    return Ok(entry(value) != 0);
                }
            }
            self.installed = InstalledTruthinessEntry::Unpublished;
        }
        if self.arena.is_none() {
            self.arena = Some(crate::stencil_arena::StencilArena::new(4096)?);
        }
        let arena = self
            .arena
            .as_mut()
            .ok_or(crate::stencil_arena::ArenaError::MappingFailed)?;
        let view = crate::stencil_select::select_physical_for_abi(
            self.pointer_key,
            crate::stencil_select::RegionAbi::ScalarWordBool,
        )
        .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
        let address = arena.render_physical_view_or_get(&mut self.physical.cache, view, &values)?;
        arena.make_executable()?;
        let entry = arena.word_bool_entry(address)?;
        self.installed = InstalledTruthinessEntry::PointerLocal(address);
        self.note_entry();
        Ok(entry(value) != 0)
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    pub(crate) fn execute(
        &mut self,
        _value: f64,
    ) -> Result<bool, crate::stencil_arena::ArenaError> {
        Err(crate::stencil_arena::ArenaError::ProtectionFailed)
    }

    #[cfg(test)]
    pub(crate) fn native_entry_count(&self) -> u64 {
        self.native_entry_count
    }
}

impl std::fmt::Debug for NativeTruthinessPlan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeTruthinessPlan")
            .field("key", &self.key)
            .field("cache_len", &self.physical.cache.len())
            .finish()
    }
}

/// Tagged-word nullish predicate. The byte template compares the raw execute
/// word against the canonical Null/Undefined payloads; all other unary
/// coercions remain on the ordinary semantic handler.
#[derive(Clone, Copy)]
enum InstalledNullishEntry {
    Unpublished,
    Local(usize),
    Shared(crate::stencil_arena::EntryToken<extern "C" fn(u64) -> u64>),
}

pub(crate) struct NativeNullishPlan {
    key: crate::stencil_fact::RegionKey,
    arena: Option<crate::stencil_arena::StencilArena>,
    shared_arena: Option<std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>>,
    physical: PhysicalState,
    site: crate::quickening::QuickeningSite<4>,
    installed: InstalledNullishEntry,
    #[cfg(test)]
    native_entry_count: u64,
}

impl NativeNullishPlan {
    #[inline]
    fn clear_shared_capabilities(&mut self) {
        reset_installed!(self, InstalledNullishEntry::Unpublished);
    }

    fn new_with_shared(
        instruction: crate::ir::Instruction,
        policy: crate::stencil_policy::ExecutionPolicy,
        shared: std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        let mut plan = Self::new(instruction, policy)?;
        plan.shared_arena = Some(shared);
        Some(plan)
    }

    fn new(
        instruction: crate::ir::Instruction,
        policy: crate::stencil_policy::ExecutionPolicy,
    ) -> Option<Self> {
        let key = crate::stencil_select::nullish_word_region_key();
        (policy.native_leaves
            && instruction.opcode == crate::ir::Opcode::Unary
            && instruction.flags == crate::ir::compact_unary_id(crate::ops::UnaryOp::IsNullish)
            && crate::stencil_select::select_region(key).is_some_and(|record| {
                record.executable
                    && record.abi == crate::stencil_select::RegionAbi::ScalarWordBool
                    && validate_physical_template(record).is_ok()
            }))
        .then_some(Self {
            key,
            arena: None,
            shared_arena: None,
            physical: PhysicalState::new(),
            site: crate::quickening::QuickeningSite::new(crate::ir::Opcode::Unary),
            installed: InstalledNullishEntry::Unpublished,
            #[cfg(test)]
            native_entry_count: 0,
        })
    }

    #[cfg(test)]
    pub(crate) fn native_entry_count(&self) -> u64 {
        self.native_entry_count
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn execute(&mut self, bits: u64) -> Result<bool, crate::stencil_arena::ArenaError> {
        if let Some(shared) = self.shared_arena.clone() {
            if let InstalledNullishEntry::Shared(owned) = self.installed {
                if let Ok(result) = invoke_shared_entry!(shared, owned, |entry| entry(bits)) {
                    #[cfg(test)]
                    {
                        self.native_entry_count = self.native_entry_count.saturating_add(1);
                    }
                    return Ok(result != 0);
                }
                self.clear_shared_capabilities();
            }
            let (address, entry) = {
                let values = crate::stencil_fact::PatchValues::from_site(&self.site)
                    .with_constant_bits(0x7ff8_4000_0000_0003);
                let mut slab = shared.borrow_mut();
                let view = crate::stencil_select::select_physical_for_abi(
                    self.key,
                    crate::stencil_select::RegionAbi::ScalarWordBool,
                )
                .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
                let address =
                    slab.render_physical_view_or_get(&mut self.physical.cache, view, &values)?;
                slab.make_executable(address)?;
                (address, slab.word_bool_entry(address)?)
            };
            let owned = shared.borrow().owned_word_bool_entry(address)?;
            self.installed = InstalledNullishEntry::Shared(owned);
            let result =
                invoke_shared_entry!(shared, owned, |entry| entry(bits)).map(|result| result != 0);
            if result.is_ok() {
                #[cfg(test)]
                {
                    self.native_entry_count = self.native_entry_count.saturating_add(1);
                }
            } else {
                self.clear_shared_capabilities();
            }
            return result;
        }
        if let InstalledNullishEntry::Local(address) = self.installed {
            if let Some(arena) = self.arena.as_ref() {
                if let Ok(entry) = arena.word_bool_entry(address) {
                    #[cfg(test)]
                    {
                        self.native_entry_count = self.native_entry_count.saturating_add(1);
                    }
                    return Ok(entry(bits) != 0);
                }
            }
            self.installed = InstalledNullishEntry::Unpublished;
        }
        if self.arena.is_none() {
            self.arena = Some(crate::stencil_arena::StencilArena::new(4096)?);
        }
        let values = crate::stencil_fact::PatchValues::from_site(&self.site)
            .with_constant_bits(0x7ff8_4000_0000_0003);
        let arena = self
            .arena
            .as_mut()
            .ok_or(crate::stencil_arena::ArenaError::MappingFailed)?;
        let view = crate::stencil_select::select_physical_for_abi(
            self.key,
            crate::stencil_select::RegionAbi::ScalarWordBool,
        )
        .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
        let address = arena.render_physical_view_or_get(&mut self.physical.cache, view, &values)?;
        arena.make_executable()?;
        let entry = arena.word_bool_entry(address)?;
        self.installed = InstalledNullishEntry::Local(address);
        #[cfg(test)]
        {
            self.native_entry_count = self.native_entry_count.saturating_add(1);
        }
        Ok(entry(bits) != 0)
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    pub(crate) fn execute(&mut self, _bits: u64) -> Result<bool, crate::stencil_arena::ArenaError> {
        Err(crate::stencil_arena::ArenaError::ProtectionFailed)
    }
}

impl std::fmt::Debug for NativeNullishPlan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeNullishPlan")
            .field("key", &self.key)
            .field("cache_len", &self.physical.cache.len())
            .finish()
    }
}

/// Native primitive constant leaf. Heap-owning constants stay on the
/// canonical loader; this body only publishes an immutable tagged word.
#[derive(Clone, Copy)]
enum InstalledConstantEntry {
    Unpublished,
    Local(usize),
    Shared(crate::stencil_arena::EntryToken<extern "C" fn() -> u64>),
}

/// Shared physical installation bookkeeping. The cache is only a disposable
/// index; lifecycle remains the authority for admission and retirement.
struct PhysicalState {
    cache: crate::stencil_select::RenderedRegionCache,
    lifecycle: crate::stencil_lifecycle::StencilLifecycle,
}

impl PhysicalState {
    fn new() -> Self {
        Self {
            cache: crate::stencil_select::RenderedRegionCache::new(),
            lifecycle: crate::stencil_lifecycle::StencilLifecycle::new(),
        }
    }

    fn clear(&mut self) {
        self.cache.clear();
        self.lifecycle.reset();
    }

    fn retire(&mut self) {
        self.cache.clear();
        self.lifecycle.retire();
    }

    fn apply_dispatch_outcome<T>(
        &mut self,
        result: &Result<T, NativeDispatchError>,
        published: Option<(&Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>, usize)>,
    ) {
        match result {
            Err(NativeDispatchError::Physical(_)) => self.clear(),
            Err(NativeDispatchError::Committed { .. }) => {
                if let Some((arena, address)) = published {
                    let _ = arena
                        .borrow_mut()
                        .retire_allocation(address, &mut self.cache);
                }
                self.retire();
            }
            _ => {}
        }
    }
}

pub(crate) struct NativeLoadConstPlan {
    bits: u64,
    key: crate::stencil_fact::RegionKey,
    arena: Option<crate::stencil_arena::StencilArena>,
    shared_arena: Option<std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>>,
    physical: PhysicalState,
    site: crate::quickening::QuickeningSite<2>,
    installed: InstalledConstantEntry,
    #[cfg(test)]
    native_entry_count: u64,
}

impl std::fmt::Debug for NativeLoadConstPlan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeLoadConstPlan")
            .field("bits", &self.bits)
            .field("key", &self.key)
            .field("cache_len", &self.physical.cache.len())
            .finish()
    }
}

impl NativeLoadConstPlan {
    #[inline]
    fn clear_shared_capabilities(&mut self) {
        reset_installed!(self, InstalledConstantEntry::Unpublished);
    }

    fn new_with_shared(
        bits: u64,
        policy: crate::stencil_policy::ExecutionPolicy,
        shared: std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        let mut plan = Self::new(bits, policy)?;
        plan.shared_arena = Some(shared);
        Some(plan)
    }

    fn new(bits: u64, policy: crate::stencil_policy::ExecutionPolicy) -> Option<Self> {
        let key = crate::stencil_select::load_const_region_key();
        (policy.native_leaves
            && crate::stencil_select::select_region(key).is_some_and(|record| {
                record.executable
                    && record.abi == crate::stencil_select::RegionAbi::ConstantWord
                    && validate_physical_template(record).is_ok()
            }))
        .then_some(Self {
            bits,
            key,
            arena: None,
            shared_arena: None,
            physical: PhysicalState::new(),
            site: crate::quickening::QuickeningSite::new(crate::ir::Opcode::LoadConst),
            #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
            installed: InstalledConstantEntry::Unpublished,
            #[cfg(test)]
            native_entry_count: 0,
        })
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn execute(&mut self) -> Result<u64, crate::stencil_arena::ArenaError> {
        if self
            .physical
            .lifecycle
            .observe_site(&self.site, self.key, true)
            == crate::stencil_lifecycle::StencilState::Retired
        {
            return Err(crate::stencil_arena::ArenaError::ProtectionFailed);
        }
        if let Some(shared) = self.shared_arena.clone() {
            if let InstalledConstantEntry::Shared(owned) = self.installed {
                if let Ok(result) = invoke_shared_entry!(shared, owned, |entry| entry()) {
                    #[cfg(test)]
                    {
                        self.native_entry_count = self.native_entry_count.saturating_add(1);
                    }
                    return Ok(result);
                }
                self.clear_shared_capabilities();
            }
            let values = crate::stencil_fact::PatchValues::from_site(&self.site)
                .with_constant_bits(self.bits);
            let address = {
                let mut slab = shared.borrow_mut();
                let view = crate::stencil_select::select_physical_for_abi(
                    self.key,
                    crate::stencil_select::RegionAbi::ConstantWord,
                )
                .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
                let address =
                    slab.render_physical_view_or_get(&mut self.physical.cache, view, &values)?;
                slab.make_executable(address)?;
                address
            };
            let owned = shared.borrow().owned_constant_word_entry(address)?;
            self.installed = InstalledConstantEntry::Shared(owned);
            return match invoke_shared_entry!(shared, owned, |entry| entry()) {
                Ok(result) => {
                    #[cfg(test)]
                    {
                        self.native_entry_count = self.native_entry_count.saturating_add(1);
                    }
                    Ok(result)
                }
                Err(error) => {
                    self.clear_shared_capabilities();
                    Err(error)
                }
            };
        }
        if let InstalledConstantEntry::Local(address) = self.installed {
            if let Some(arena) = self.arena.as_ref() {
                if let Ok(entry) = arena.constant_word_entry(address) {
                    #[cfg(test)]
                    {
                        self.native_entry_count = self.native_entry_count.saturating_add(1);
                    }
                    return Ok(entry());
                }
            }
            self.installed = InstalledConstantEntry::Unpublished;
        }
        if self.arena.is_none() {
            self.arena = Some(crate::stencil_arena::StencilArena::new(4096)?);
        }
        let values =
            crate::stencil_fact::PatchValues::from_site(&self.site).with_constant_bits(self.bits);
        let arena = self
            .arena
            .as_mut()
            .ok_or(crate::stencil_arena::ArenaError::MappingFailed)?;
        let view = crate::stencil_select::select_physical_for_abi(
            self.key,
            crate::stencil_select::RegionAbi::ConstantWord,
        )
        .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
        let address = arena.render_physical_view_or_get(&mut self.physical.cache, view, &values)?;
        arena.make_executable()?;
        arena.constant_word_entry(address)?;
        self.installed = InstalledConstantEntry::Local(address);
        #[cfg(test)]
        {
            self.native_entry_count = self.native_entry_count.saturating_add(1);
        }
        Ok(arena.constant_word_entry(address)?())
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    pub(crate) fn execute(&mut self) -> Result<u64, crate::stencil_arena::ArenaError> {
        Err(crate::stencil_arena::ArenaError::ProtectionFailed)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeUnaryKind {
    BitwiseNot,
    Negate,
}

/// Typed unary leaves for exact numeric subsets. Other unary operators retain
/// the canonical residual handler and coercion order.
#[derive(Clone, Copy)]
enum InstalledUnaryEntry {
    Unpublished,
    IntegerLocal(usize),
    IntegerShared(crate::stencil_arena::EntryToken<extern "C" fn(i32) -> i32>),
    NumberLocal(usize),
    NumberShared(crate::stencil_arena::EntryToken<extern "C" fn(f64) -> f64>),
}

pub(crate) struct NativeUnaryPlan {
    arena: Option<crate::stencil_arena::StencilArena>,
    shared_arena: Option<std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>>,
    physical: PhysicalState,
    site: crate::quickening::QuickeningSite<4>,
    key: crate::stencil_fact::RegionKey,
    kind: NativeUnaryKind,
    installed: InstalledUnaryEntry,
    #[cfg(test)]
    native_entry_count: u64,
}

impl std::fmt::Debug for NativeUnaryPlan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeUnaryPlan")
            .field("key", &self.key)
            .field("cache_len", &self.physical.cache.len())
            .finish()
    }
}

impl NativeUnaryPlan {
    #[inline]
    fn clear_shared_capabilities(&mut self) {
        reset_installed!(self, InstalledUnaryEntry::Unpublished);
    }

    fn new_with_shared(
        instruction: crate::ir::Instruction,
        policy: crate::stencil_policy::ExecutionPolicy,
        shared: std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        let mut plan = Self::new(instruction, policy)?;
        plan.shared_arena = Some(shared);
        Some(plan)
    }

    fn new(
        instruction: crate::ir::Instruction,
        policy: crate::stencil_policy::ExecutionPolicy,
    ) -> Option<Self> {
        let (key, kind, abi) = match crate::ir::compact_unary_operator(instruction.flags) {
            Some(crate::ops::UnaryOp::BitwiseNot) => (
                crate::stencil_select::bitwise_not_region_key(),
                NativeUnaryKind::BitwiseNot,
                crate::stencil_select::RegionAbi::ScalarI32,
            ),
            Some(crate::ops::UnaryOp::Minus) => (
                crate::stencil_select::negate_region_key(),
                NativeUnaryKind::Negate,
                crate::stencil_select::RegionAbi::ScalarF64Unary,
            ),
            _ => return None,
        };
        (policy.native_leaves
            && instruction.opcode == crate::ir::Opcode::Unary
            && crate::stencil_select::select_region(key).is_some_and(|record| {
                record.executable && record.abi == abi && validate_physical_template(record).is_ok()
            }))
        .then_some(Self {
            arena: None,
            shared_arena: None,
            physical: PhysicalState::new(),
            site: crate::quickening::QuickeningSite::new(instruction.opcode),
            key,
            kind,
            #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
            installed: InstalledUnaryEntry::Unpublished,
            #[cfg(test)]
            native_entry_count: 0,
        })
    }

    #[inline]
    fn note_entry(&mut self) {
        #[cfg(test)]
        {
            self.native_entry_count = self.native_entry_count.saturating_add(1);
        }
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    fn execute_number_shared(
        &mut self,
        shared: std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>,
        value: f64,
    ) -> Result<f64, crate::stencil_arena::ArenaError> {
        if let InstalledUnaryEntry::NumberShared(owned) = self.installed {
            if let Ok(result) = invoke_shared_entry!(shared, owned, |entry| entry(value)) {
                self.note_entry();
                return Ok(result);
            }
            self.clear_shared_capabilities();
        }
        let values = crate::stencil_fact::PatchValues::from_site(&self.site)
            .with_constant_bits(0x8000_0000_0000_0000);
        let address = {
            let mut slab = shared.borrow_mut();
            let view = crate::stencil_select::select_physical_for_abi(
                self.key,
                crate::stencil_select::RegionAbi::ScalarF64Unary,
            )
            .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
            let address =
                slab.render_physical_view_or_get(&mut self.physical.cache, view, &values)?;
            slab.make_executable(address)?;
            address
        };
        let owned = shared.borrow().owned_f64_unary_entry(address)?;
        self.installed = InstalledUnaryEntry::NumberShared(owned);
        match invoke_shared_entry!(shared, owned, |entry| entry(value)) {
            Ok(result) => {
                self.note_entry();
                Ok(result)
            }
            Err(error) => {
                self.clear_shared_capabilities();
                Err(error)
            }
        }
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    fn execute_number_local(
        &mut self,
        value: f64,
    ) -> Result<f64, crate::stencil_arena::ArenaError> {
        let values = crate::stencil_fact::PatchValues::from_site(&self.site)
            .with_constant_bits(0x8000_0000_0000_0000);
        if let InstalledUnaryEntry::NumberLocal(address) = self.installed {
            if let Some(arena) = self.arena.as_ref() {
                if let Ok(entry) = arena.f64_unary_entry(address) {
                    self.note_entry();
                    return Ok(entry(value));
                }
            }
            self.installed = InstalledUnaryEntry::Unpublished;
        }
        if self.arena.is_none() {
            self.arena = Some(crate::stencil_arena::StencilArena::new(4096)?);
        }
        let arena = self
            .arena
            .as_mut()
            .ok_or(crate::stencil_arena::ArenaError::MappingFailed)?;
        let view = crate::stencil_select::select_physical_for_abi(
            self.key,
            crate::stencil_select::RegionAbi::ScalarF64Unary,
        )
        .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
        let address = arena.render_physical_view_or_get(&mut self.physical.cache, view, &values)?;
        arena.make_executable()?;
        let entry = arena.f64_unary_entry(address)?;
        self.installed = InstalledUnaryEntry::NumberLocal(address);
        self.note_entry();
        Ok(entry(value))
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    fn execute_number(&mut self, value: f64) -> Result<f64, crate::stencil_arena::ArenaError> {
        if let Some(shared) = self.shared_arena.clone() {
            return self.execute_number_shared(shared, value);
        }
        self.execute_number_local(value)
    }

    pub(crate) fn execute(&mut self, value: f64) -> Result<f64, crate::stencil_arena::ArenaError> {
        if self.kind == NativeUnaryKind::Negate {
            #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
            return self.execute_number(value);
            #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
            return Err(crate::stencil_arena::ArenaError::ProtectionFailed);
        }
        let operand = number_to_int32(value);
        if let Some(shared) = self.shared_arena.clone() {
            #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
            if let InstalledUnaryEntry::IntegerShared(owned) = self.installed {
                match invoke_shared_entry!(shared, owned, |entry| entry(operand)) {
                    Ok(result) => {
                        self.note_entry();
                        return Ok(f64::from(result));
                    }
                    Err(_) => self.clear_shared_capabilities(),
                }
            }
            let values = crate::stencil_fact::PatchValues::from_site(&self.site);
            let rendered = (|| -> Result<usize, crate::stencil_arena::ArenaError> {
                let mut slab = shared.borrow_mut();
                let view = crate::stencil_select::select_physical_for_abi(
                    self.key,
                    crate::stencil_select::RegionAbi::ScalarI32,
                )
                .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
                let address =
                    slab.render_physical_view_or_get(&mut self.physical.cache, view, &values)?;
                slab.make_executable(address)?;
                Ok(address)
            })();
            let address = rendered.map_err(|error| {
                self.physical.clear();
                error
            })?;
            let owned = shared.borrow().owned_i32_unary_entry(address)?;
            let result = match invoke_shared_entry!(shared, owned, |entry| entry(operand)) {
                Ok(result) => result,
                Err(error) => {
                    self.clear_shared_capabilities();
                    return Err(error);
                }
            };
            #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
            {
                self.installed = InstalledUnaryEntry::IntegerShared(owned);
            }
            self.note_entry();
            return Ok(f64::from(result));
        }
        if self.arena.is_none() {
            self.arena = Some(crate::stencil_arena::StencilArena::new(4096)?);
        }
        let values = crate::stencil_fact::PatchValues::from_site(&self.site);
        let arena = self
            .arena
            .as_mut()
            .ok_or(crate::stencil_arena::ArenaError::MappingFailed)?;
        let view = crate::stencil_select::select_physical_for_abi(
            self.key,
            crate::stencil_select::RegionAbi::ScalarI32,
        )
        .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
        let address = arena.render_physical_view_or_get(&mut self.physical.cache, view, &values)?;
        arena.make_executable()?;
        let entry = arena.i32_unary_entry(address)?;
        self.installed = InstalledUnaryEntry::IntegerLocal(address);
        let result = entry(operand);
        self.note_entry();
        Ok(f64::from(result))
    }
}

/// Fused numeric region.  The machine fragment receives the proven numeric
/// inputs in FP argument registers, performs the admitted sequence, and
/// returns once at the region boundary. Register writes and all
/// non-numeric/aliasing cases stay on the canonical path.
#[derive(Clone, Copy)]
enum InstalledF64x3Entry {
    Unpublished,
    Local(usize),
    Shared(crate::stencil_arena::EntryToken<extern "C" fn(f64, f64, f64) -> f64>),
}

pub(crate) struct NativeAddChainPlan {
    arena: Option<crate::stencil_arena::StencilArena>,
    shared_arena: Option<std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>>,
    physical: PhysicalState,
    bindings: crate::stencil_plan::F64x3Bindings,
    control: crate::stencil_cfg::RegionControlPlan,
    site: crate::quickening::QuickeningSite<4>,
    installed: InstalledF64x3Entry,
    #[cfg(test)]
    last_native_view: Option<crate::stencil_select::PhysicalStencilView>,
    #[cfg(test)]
    native_entry_count: u64,
}

impl NativeAddChainPlan {
    #[inline]
    fn clear_shared_capabilities(&mut self) {
        reset_installed!(self, InstalledF64x3Entry::Unpublished);
        #[cfg(test)]
        {
            self.last_native_view = None;
        }
    }

    pub(crate) fn new_with_arena(
        policy: crate::stencil_policy::ExecutionPolicy,
        shared_arena: std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>,
        bindings: crate::stencil_plan::F64x3Bindings,
        control: crate::stencil_cfg::RegionControlPlan,
    ) -> Option<Self> {
        let mut plan = Self::new(policy, bindings, control)?;
        plan.shared_arena = Some(shared_arena);
        Some(plan)
    }

    pub(crate) fn new_embedded_with_arena(
        policy: crate::stencil_policy::ExecutionPolicy,
        shared_arena: std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>,
        bindings: crate::stencil_plan::F64x3Bindings,
    ) -> Option<Self> {
        let control = crate::stencil_cfg::RegionControlPlan::linear(0, 2)?;
        Self::new_with_arena(policy, shared_arena, bindings, control)
    }

    fn new(
        policy: crate::stencil_policy::ExecutionPolicy,
        bindings: crate::stencil_plan::F64x3Bindings,
        control: crate::stencil_cfg::RegionControlPlan,
    ) -> Option<Self> {
        if !policy.native_leaves {
            return None;
        }
        let key = crate::stencil_select::add_chain_region_key();
        let view = crate::stencil_select::select_physical(key)?;
        (view.executable
            && validate_physical_template(view.record).is_ok()
            && crate::stencil_region_layout::validate_selected_control(view, &control).is_ok())
        .then_some(())?;
        Some(Self {
            arena: None,
            shared_arena: None,
            physical: PhysicalState::new(),
            bindings,
            control,
            site: crate::quickening::QuickeningSite::new(crate::ir::Opcode::Add),
            installed: InstalledF64x3Entry::Unpublished,
            #[cfg(test)]
            last_native_view: None,
            #[cfg(test)]
            native_entry_count: 0,
        })
    }

    #[cfg(test)]
    pub(crate) fn native_entry_count(&self) -> u64 {
        self.native_entry_count
    }

    #[cfg(test)]
    pub(crate) fn last_native_view(&self) -> Option<crate::stencil_select::PhysicalStencilView> {
        self.last_native_view
    }

    pub(crate) const fn bindings(&self) -> crate::stencil_plan::F64x3Bindings {
        self.bindings
    }

    #[inline]
    fn note_entry(&mut self) {
        #[cfg(test)]
        {
            self.native_entry_count = self.native_entry_count.saturating_add(1);
        }
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    fn execute_shared_cached(
        &mut self,
        lhs: f64,
        rhs: f64,
        third: f64,
    ) -> Result<Option<f64>, crate::stencil_arena::ArenaError> {
        let (Some(shared), InstalledF64x3Entry::Shared(owned)) =
            (self.shared_arena.clone(), self.installed)
        else {
            return Ok(None);
        };
        let result = invoke_shared_entry!(shared, owned, |entry| unsafe {
            invoke_f64x3_entry(entry, lhs, rhs, third)
        });
        match result {
            Ok(value) => {
                self.note_entry();
                Ok(Some(value))
            }
            Err(_) => {
                self.clear_shared_capabilities();
                Ok(None)
            }
        }
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    fn publish_shared(
        &mut self,
        key: crate::stencil_fact::RegionKey,
        values: &crate::stencil_fact::PatchValues,
    ) -> Result<
        crate::stencil_arena::EntryToken<extern "C" fn(f64, f64, f64) -> f64>,
        crate::stencil_arena::ArenaError,
    > {
        let shared = self
            .shared_arena
            .clone()
            .ok_or(crate::stencil_arena::ArenaError::MappingFailed)?;
        let rendered = (|| {
            let mut slab = shared.borrow_mut();
            let view = crate::stencil_select::select_physical_for_abi(
                key,
                crate::stencil_select::RegionAbi::ScalarF64x3,
            )
            .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
            crate::stencil_region_layout::validate_selected_control(view, &self.control)
                .map_err(|_| crate::stencil_arena::ArenaError::ProtectionFailed)?;
            let address =
                slab.render_physical_view_or_get(&mut self.physical.cache, view, values)?;
            slab.make_executable(address)?;
            Ok::<_, crate::stencil_arena::ArenaError>((address, view))
        })();
        let (address, _view) = match rendered {
            Ok(rendered) => rendered,
            Err(error) => {
                self.physical.clear();
                return Err(error);
            }
        };
        let owned = shared.borrow().owned_f64x3_entry(address)?;
        self.installed = InstalledF64x3Entry::Shared(owned);
        #[cfg(test)]
        {
            self.last_native_view = Some(_view);
        }
        Ok(owned)
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    fn render_shared(
        &mut self,
        key: crate::stencil_fact::RegionKey,
        values: &crate::stencil_fact::PatchValues,
        lhs: f64,
        rhs: f64,
        third: f64,
    ) -> Result<f64, crate::stencil_arena::ArenaError> {
        let shared = self
            .shared_arena
            .clone()
            .ok_or(crate::stencil_arena::ArenaError::MappingFailed)?;
        let owned = self.publish_shared(key, values)?;
        let result = invoke_shared_entry!(shared, owned, |entry| unsafe {
            invoke_f64x3_entry(entry, lhs, rhs, third)
        });
        match result {
            Ok(value) => {
                self.note_entry();
                Ok(value)
            }
            Err(error) => {
                // Publication succeeded, but the owner may have been retired
                // before invocation. Drop every capability derived from that
                // generation so the next attempt must revalidate and render;
                // never retain a callable pointer after an ownership miss.
                self.clear_shared_capabilities();
                Err(error)
            }
        }
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    fn render_local(
        &mut self,
        key: crate::stencil_fact::RegionKey,
        values: &crate::stencil_fact::PatchValues,
        lhs: f64,
        rhs: f64,
        third: f64,
    ) -> Result<f64, crate::stencil_arena::ArenaError> {
        let view = crate::stencil_select::select_physical_for_abi(
            key,
            crate::stencil_select::RegionAbi::ScalarF64x3,
        )
        .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
        crate::stencil_region_layout::validate_selected_control(view, &self.control)
            .map_err(|_| crate::stencil_arena::ArenaError::ProtectionFailed)?;
        if self.arena.is_none() {
            self.arena = Some(crate::stencil_arena::StencilArena::new(4096)?);
        }
        let result = self
            .arena
            .as_mut()
            .ok_or(crate::stencil_arena::ArenaError::MappingFailed)?
            .render_selected_f64x3(&mut self.physical.cache, key, values, lhs, rhs, third);
        if result.is_ok() {
            #[cfg(test)]
            {
                self.last_native_view = crate::stencil_select::select_physical_for_abi(
                    key,
                    crate::stencil_select::RegionAbi::ScalarF64x3,
                );
            }
            self.note_entry();
            if let Some(arena) = self.arena.as_ref() {
                let signature = crate::stencil_arena::physical_cache_signature(
                    crate::stencil_select::select_physical(key).expect("installed view"),
                    &values,
                );
                if let Some(address) = self.physical.cache.get_owned(key, signature, arena.id()) {
                    self.installed = arena
                        .f64x3_entry(address)
                        .ok()
                        .map(|_| InstalledF64x3Entry::Local(address))
                        .unwrap_or(InstalledF64x3Entry::Unpublished);
                }
            }
        } else {
            self.arena.take();
            self.physical.clear();
        }
        result
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[inline]
    pub(crate) fn execute(
        &mut self,
        lhs: f64,
        rhs: f64,
        third: f64,
    ) -> Result<f64, crate::stencil_arena::ArenaError> {
        if self.shared_arena.is_none() {
            if let InstalledF64x3Entry::Local(address) = self.installed {
                if let Some(arena) = self.arena.as_ref() {
                    if let Ok(entry) = arena.f64x3_entry(address) {
                        let result = unsafe { invoke_f64x3_entry(entry, lhs, rhs, third) };
                        self.note_entry();
                        return Ok(result);
                    }
                }
                self.installed = InstalledF64x3Entry::Unpublished;
            }
        }
        let key = crate::stencil_select::add_chain_region_key();
        if self.physical.lifecycle.observe_site(&self.site, key, true)
            == crate::stencil_lifecycle::StencilState::Retired
        {
            return Err(crate::stencil_arena::ArenaError::ProtectionFailed);
        }
        if let Some(value) = self.execute_shared_cached(lhs, rhs, third)? {
            return Ok(value);
        }
        let site = self.site.clone();
        let values = crate::stencil_fact::PatchValues::from_site(&site);
        if self.shared_arena.is_some() {
            return self.render_shared(key, &values, lhs, rhs, third);
        }
        self.render_local(key, &values, lhs, rhs, third)
    }
}

impl std::fmt::Debug for NativeAddChainPlan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeAddChainPlan")
            .field(
                "used_bytes",
                &self
                    .shared_arena
                    .as_ref()
                    .map(|arena| arena.borrow().used())
                    .or_else(|| self.arena.as_ref().map(|arena| arena.used()))
                    .unwrap_or(0),
            )
            .field("cache_len", &self.physical.cache.len())
            .finish()
    }
}

/// Optional native leaf for a pure register-word move. The machine code only
/// copies the canonical eight-byte word; the Rust destination write performs
/// the retain/release edge, so pointer-backed values remain ownership-safe.
#[derive(Clone, Copy)]
enum InstalledWordEntry {
    Unpublished,
    Local(usize),
    Shared(
        crate::stencil_arena::EntryToken<
            extern "C" fn(*const crate::tagged_value::TaggedValue) -> u64,
        >,
    ),
}

pub(crate) struct NativeMovePlan {
    arena: Option<crate::stencil_arena::StencilArena>,
    shared_arena: Option<std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>>,
    physical: PhysicalState,
    site: crate::quickening::QuickeningSite<4>,
    opcode: crate::ir::Opcode,
    installed: InstalledWordEntry,
    #[cfg(test)]
    native_entry_count: u64,
    #[cfg(test)]
    last_native_view: Option<crate::stencil_select::PhysicalStencilView>,
}

impl NativeMovePlan {
    #[inline]
    fn clear_shared_capabilities(&mut self) {
        reset_installed!(self, InstalledWordEntry::Unpublished);
    }

    fn new_with_arena(
        instruction: crate::ir::Instruction,
        policy: crate::stencil_policy::ExecutionPolicy,
        shared_arena: std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        let mut plan = Self::new(instruction, policy)?;
        plan.shared_arena = Some(shared_arena);
        Some(plan)
    }

    fn new(
        instruction: crate::ir::Instruction,
        policy: crate::stencil_policy::ExecutionPolicy,
    ) -> Option<Self> {
        if !policy.native_leaves {
            return None;
        }
        if !matches!(
            instruction.opcode,
            crate::ir::Opcode::Move
                | crate::ir::Opcode::LoadLocal
                | crate::ir::Opcode::StoreLocal
                | crate::ir::Opcode::SetN
        ) || instruction.flags != 0
        {
            return None;
        }
        let key = match instruction.opcode {
            crate::ir::Opcode::LoadLocal => crate::stencil_select::load_local_region_key(),
            crate::ir::Opcode::StoreLocal => crate::stencil_select::store_local_region_key(),
            crate::ir::Opcode::SetN => crate::stencil_select::store_property_region_key(),
            _ => crate::stencil_select::move_region_key(),
        };
        crate::stencil_select::select_region(key).filter(|record| {
            record.executable
                && record.abi == crate::stencil_select::RegionAbi::TaggedWord
                && validate_physical_template(record).is_ok()
        })?;
        Some(Self {
            arena: None,
            shared_arena: None,
            physical: PhysicalState::new(),
            site: crate::quickening::QuickeningSite::new(instruction.opcode),
            opcode: instruction.opcode,
            #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
            installed: InstalledWordEntry::Unpublished,
            #[cfg(test)]
            native_entry_count: 0,
            #[cfg(test)]
            last_native_view: None,
        })
    }

    #[inline]
    fn note_entry(&mut self) {
        #[cfg(test)]
        {
            self.native_entry_count = self.native_entry_count.saturating_add(1);
        }
    }

    fn note_view(&mut self, view: crate::stencil_select::PhysicalStencilView) {
        #[cfg(test)]
        {
            self.last_native_view = Some(view);
        }
        #[cfg(not(test))]
        let _ = view;
    }

    #[cfg(test)]
    pub(crate) fn native_entry_count(&self) -> u64 {
        self.native_entry_count
    }

    #[cfg(test)]
    pub(crate) fn last_native_view(&self) -> Option<crate::stencil_select::PhysicalStencilView> {
        self.last_native_view
    }

    #[inline]
    pub(crate) fn execute(
        &mut self,
        source: *const crate::tagged_value::TaggedValue,
    ) -> Result<u64, crate::stencil_arena::ArenaError> {
        if source.is_null() {
            return Err(crate::stencil_arena::ArenaError::ProtectionFailed);
        }
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        if self.shared_arena.is_none() {
            if let InstalledWordEntry::Local(address) = self.installed {
                if let Some(arena) = self.arena.as_ref() {
                    if let Ok(entry) = arena.tagged_word_entry(address) {
                        let value = entry(source);
                        self.note_entry();
                        return Ok(value);
                    }
                }
                self.installed = InstalledWordEntry::Unpublished;
            }
        }
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        if let (Some(shared), InstalledWordEntry::Shared(owned)) =
            (self.shared_arena.clone(), self.installed)
        {
            match invoke_shared_entry!(shared, owned, |entry| entry(source)) {
                Ok(value) => {
                    self.note_entry();
                    return Ok(value);
                }
                Err(_) => self.clear_shared_capabilities(),
            }
        }
        let key = match self.opcode {
            crate::ir::Opcode::LoadLocal => crate::stencil_select::load_local_region_key(),
            crate::ir::Opcode::StoreLocal => crate::stencil_select::store_local_region_key(),
            crate::ir::Opcode::SetN => crate::stencil_select::store_property_region_key(),
            _ => crate::stencil_select::move_region_key(),
        };
        let values = crate::stencil_fact::PatchValues::from_site(&self.site);
        if !crate::stencil_select::select_region(key).is_some_and(|record| record.executable)
            || self.physical.lifecycle.observe(key, true)
                == crate::stencil_lifecycle::StencilState::Retired
        {
            return Err(crate::stencil_arena::ArenaError::ProtectionFailed);
        }
        if let Some(shared) = self.shared_arena.clone() {
            let rendered = (|| -> Result<_, crate::stencil_arena::ArenaError> {
                let mut slab = shared.borrow_mut();
                let view = crate::stencil_select::select_physical_for_abi(
                    key,
                    crate::stencil_select::RegionAbi::TaggedWord,
                )
                .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
                let address =
                    slab.render_physical_view_or_get(&mut self.physical.cache, view, &values)?;
                slab.make_executable(address)?;
                Ok((address, view))
            })();
            let (address, view) = match rendered {
                Ok(rendered) => rendered,
                Err(error) => {
                    self.physical.clear();
                    return Err(error);
                }
            };
            let owned = shared.borrow().owned_tagged_word_entry(address)?;
            self.installed = InstalledWordEntry::Shared(owned);
            let result = invoke_shared_entry!(shared, owned, |entry| entry(source));
            return match result {
                Ok(value) => {
                    self.note_entry();
                    self.note_view(view);
                    Ok(value)
                }
                Err(error) => {
                    self.clear_shared_capabilities();
                    Err(error)
                }
            };
        }
        if self.arena.is_none() {
            match crate::stencil_arena::StencilArena::new(4096) {
                Ok(arena) => self.arena = Some(arena),
                Err(error) => {
                    self.physical.lifecycle.reset();
                    return Err(error);
                }
            }
        }
        let result = (|| {
            let arena = self
                .arena
                .as_mut()
                .ok_or(crate::stencil_arena::ArenaError::MappingFailed)?;
            let view = crate::stencil_select::select_physical_for_abi(
                key,
                crate::stencil_select::RegionAbi::TaggedWord,
            )
            .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
            let address =
                arena.render_physical_view_or_get(&mut self.physical.cache, view, &values)?;
            arena.make_executable()?;
            arena
                .execute_tagged_word(address, source)
                .map(|value| (value, view))
        })();
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        if let Ok((_, view)) = &result {
            let signature = crate::stencil_select::select_physical(key)
                .map(|view| crate::stencil_arena::physical_cache_signature(view, &values));
            self.note_view(*view);
            self.note_entry();
            if let Some(arena) = self.arena.as_ref() {
                if let Some(address) = signature
                    .and_then(|signature| self.physical.cache.get_owned(key, signature, arena.id()))
                {
                    self.installed = arena
                        .tagged_word_entry(address)
                        .map(|_| InstalledWordEntry::Local(address))
                        .unwrap_or(InstalledWordEntry::Unpublished);
                }
            }
        }
        if result.is_err() {
            self.arena.take();
            self.physical.clear();
            #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
            {
                self.installed = InstalledWordEntry::Unpublished;
            }
        }
        result.map(|(value, _)| value)
    }
}

impl std::fmt::Debug for NativeMovePlan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeMovePlan")
            .field("opcode", &self.opcode)
            .field(
                "used_bytes",
                &self
                    .shared_arena
                    .as_ref()
                    .map(|arena| arena.borrow().used())
                    .or_else(|| self.arena.as_ref().map(|arena| arena.used()))
                    .unwrap_or(0),
            )
            .field("cache_len", &self.physical.cache.len())
            .finish()
    }
}

/// Optional native leaf for a guarded plain-data property read. The quickening
/// site proves the slot; the physical entry rechecks layout and metadata before
/// loading the word. `RegisterFile` owns the retain/release edge afterward.
#[derive(Clone, Copy)]
enum InstalledPropertyEntry {
    Unpublished,
    ReadLocal {
        key: crate::stencil_fact::RegionKey,
        address: usize,
    },
    ReadShared {
        key: crate::stencil_fact::RegionKey,
        entry: crate::stencil_arena::EntryToken<
            extern "C" fn(*mut crate::native_property::NativePropertyReadContext) -> u32,
        >,
    },
    WriteLocal(usize),
    WriteShared(
        crate::stencil_arena::EntryToken<
            extern "C" fn(*mut crate::native_property::NativePropertyWriteContext) -> u32,
        >,
    ),
}

pub(crate) struct NativePropertyPlan {
    arena: Option<crate::stencil_arena::StencilArena>,
    shared_arena: Option<std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>>,
    physical: PhysicalState,
    opcode: crate::ir::Opcode,
    installed: InstalledPropertyEntry,
    #[cfg(test)]
    native_entry_count: u64,
    #[cfg(test)]
    last_native_view: Option<crate::stencil_select::PhysicalStencilView>,
}

impl NativePropertyPlan {
    #[inline]
    fn clear_shared_capabilities(&mut self) {
        reset_installed!(self, InstalledPropertyEntry::Unpublished);
    }

    pub(crate) fn new_with_arena(
        instruction: crate::ir::Instruction,
        policy: crate::stencil_policy::ExecutionPolicy,
        shared_arena: std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        let mut plan = Self::new(instruction, policy)?;
        plan.shared_arena = Some(shared_arena);
        Some(plan)
    }

    fn new(
        instruction: crate::ir::Instruction,
        policy: crate::stencil_policy::ExecutionPolicy,
    ) -> Option<Self> {
        if !policy.native_leaves {
            return None;
        }
        let opcode = instruction.opcode;
        let (key, abi) = match opcode {
            crate::ir::Opcode::GetN => (
                crate::stencil_select::property_region_key(),
                crate::stencil_select::RegionAbi::PropertyGuard,
            ),
            crate::ir::Opcode::SetN => (
                crate::stencil_select::store_property_region_key(),
                crate::stencil_select::RegionAbi::PropertyWriteGuard,
            ),
            _ => return None,
        };
        crate::stencil_select::select_region(key).filter(|record| {
            record.executable && record.abi == abi && validate_physical_template(record).is_ok()
        })?;
        Some(Self {
            arena: None,
            shared_arena: None,
            physical: PhysicalState::new(),
            opcode,
            installed: InstalledPropertyEntry::Unpublished,
            #[cfg(test)]
            native_entry_count: 0,
            #[cfg(test)]
            last_native_view: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn native_entry_count(&self) -> u64 {
        self.native_entry_count
    }

    #[cfg(test)]
    pub(crate) fn last_native_view(&self) -> Option<crate::stencil_select::PhysicalStencilView> {
        self.last_native_view
    }

    #[inline]
    pub(crate) fn execute(
        &mut self,
        access: crate::native_property::GuardedPropertySlot,
        site: &crate::quickening::QuickeningSite<4>,
    ) -> Result<u64, crate::stencil_arena::ArenaError> {
        if self.opcode != crate::ir::Opcode::GetN {
            return Err(crate::stencil_arena::ArenaError::ProtectionFailed);
        }
        let key = access.region_key();
        let mut context = crate::native_property::NativePropertyReadContext::new(access);
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        if self.shared_arena.is_none() {
            if let InstalledPropertyEntry::ReadLocal {
                key: installed_key,
                address,
            } = self.installed
            {
                if installed_key == key {
                    if let Some(arena) = self.arena.as_ref() {
                        if let Ok(entry) = arena.property_guard_entry(address) {
                            #[cfg(test)]
                            {
                                self.native_entry_count = self.native_entry_count.saturating_add(1);
                            }
                            let status = entry(&mut context);
                            return context
                                .result(status)
                                .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed);
                        }
                    }
                }
                self.installed = InstalledPropertyEntry::Unpublished;
            }
        }
        let values = crate::stencil_fact::PatchValues::from_site(site);
        if !crate::stencil_select::select_region(key).is_some_and(|record| record.executable)
            || self.physical.lifecycle.observe_site(site, key, true)
                == crate::stencil_lifecycle::StencilState::Retired
        {
            return Err(crate::stencil_arena::ArenaError::ProtectionFailed);
        }
        if let Some(shared) = self.shared_arena.clone() {
            #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
            if let InstalledPropertyEntry::ReadShared {
                key: installed_key,
                entry: owned,
            } = self.installed
            {
                if installed_key == key {
                    match invoke_shared_entry!(shared, owned, |entry| entry(&mut context)) {
                        Ok(status) => {
                            #[cfg(test)]
                            {
                                self.native_entry_count = self.native_entry_count.saturating_add(1);
                            }
                            return context
                                .result(status)
                                .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed);
                        }
                        Err(_) => self.clear_shared_capabilities(),
                    }
                }
            }
            let rendered = (|| {
                let mut slab = shared.borrow_mut();
                let view = crate::stencil_select::select_physical_for_abi(
                    key,
                    crate::stencil_select::RegionAbi::PropertyGuard,
                )
                .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
                let address =
                    slab.render_physical_view_or_get(&mut self.physical.cache, view, &values)?;
                slab.make_executable(address)?;
                Ok::<_, crate::stencil_arena::ArenaError>((address, view))
            })();
            let (address, view) = match rendered {
                Ok(rendered) => rendered,
                Err(error) => {
                    self.physical.clear();
                    return Err(error);
                }
            };
            #[cfg(not(test))]
            let _ = view;
            let owned = shared.borrow().owned_property_guard_entry(address)?;
            let status = match invoke_shared_entry!(shared, owned, |entry| entry(&mut context)) {
                Ok(status) => status,
                Err(error) => {
                    self.physical.clear();
                    return Err(error);
                }
            };
            #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
            {
                self.installed = InstalledPropertyEntry::ReadShared { key, entry: owned };
            }
            #[cfg(test)]
            {
                self.native_entry_count = self.native_entry_count.saturating_add(1);
                self.last_native_view = Some(view);
            }
            return context
                .result(status)
                .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed);
        }
        if self.arena.is_none() {
            match crate::stencil_arena::StencilArena::new(4096) {
                Ok(arena) => self.arena = Some(arena),
                Err(error) => {
                    self.physical.lifecycle.reset();
                    return Err(error);
                }
            }
        }
        let mut rendered_view = None;
        let result = (|| {
            let arena = self
                .arena
                .as_mut()
                .ok_or(crate::stencil_arena::ArenaError::MappingFailed)?;
            let view = crate::stencil_select::select_physical_for_abi(
                key,
                crate::stencil_select::RegionAbi::PropertyGuard,
            )
            .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
            let address =
                arena.render_physical_view_or_get(&mut self.physical.cache, view, &values)?;
            arena.make_executable()?;
            rendered_view = Some(view);
            let status = arena.execute_dispatch_with_abi(
                address,
                (&mut context as *mut crate::native_property::NativePropertyReadContext).cast(),
                crate::stencil_select::RegionAbi::PropertyGuard,
            )?;
            context
                .result(status as u32)
                .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)
        })();
        #[cfg(not(test))]
        let _ = rendered_view;
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        if result.is_ok() {
            #[cfg(test)]
            {
                self.native_entry_count = self.native_entry_count.saturating_add(1);
                self.last_native_view = rendered_view;
            }
            if let Some(arena) = self.arena.as_ref() {
                let signature = crate::stencil_arena::physical_cache_signature(
                    crate::stencil_select::select_physical(key).expect("installed view"),
                    &values,
                );
                if let Some(address) = self.physical.cache.get_owned(key, signature, arena.id()) {
                    self.installed = arena
                        .property_guard_entry(address)
                        .ok()
                        .map(|_| InstalledPropertyEntry::ReadLocal { key, address })
                        .unwrap_or(InstalledPropertyEntry::Unpublished);
                }
            }
        }
        if result.is_err() {
            self.arena.take();
            self.physical.clear();
            #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
            {
                self.installed = InstalledPropertyEntry::Unpublished;
            }
        }
        result
    }
    pub(crate) fn execute_write(
        &mut self,
        access: crate::native_property::GuardedPropertySlot,
        value: u64,
        site: &crate::quickening::QuickeningSite<4>,
    ) -> Result<(), crate::stencil_arena::ArenaError> {
        if self.opcode != crate::ir::Opcode::SetN
            || !access.accepts_non_owning_store()
            || crate::tagged_value::TaggedValue::from_bits(value).owns_rc()
        {
            return Err(crate::stencil_arena::ArenaError::ProtectionFailed);
        }
        let mut context = crate::native_property::NativePropertyWriteContext::new(access, value);
        if let Some(result) = self.execute_installed_write(&mut context) {
            return result;
        }
        let key = crate::stencil_select::store_property_region_key();
        let values = crate::stencil_fact::PatchValues::from_site(site);
        if self.physical.lifecycle.observe_site(site, key, true)
            == crate::stencil_lifecycle::StencilState::Retired
        {
            return Err(crate::stencil_arena::ArenaError::ProtectionFailed);
        }
        if self.shared_arena.is_some() {
            return self.render_shared_write(key, &values, &mut context);
        }
        self.render_local_write(key, &values, &mut context)
    }

    fn execute_installed_write(
        &mut self,
        context: &mut crate::native_property::NativePropertyWriteContext,
    ) -> Option<Result<(), crate::stencil_arena::ArenaError>> {
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        if let Some(shared) = self.shared_arena.clone() {
            if let InstalledPropertyEntry::WriteShared(owned) = self.installed {
                let result = invoke_shared_entry!(shared, owned, |entry| entry(context));
                return Some(self.finish_property_write(result));
            }
        } else if let InstalledPropertyEntry::WriteLocal(address) = self.installed {
            let result = self.arena.as_ref().and_then(|arena| {
                arena
                    .property_write_guard_entry(address)
                    .ok()
                    .map(|entry| entry(context))
            });
            return Some(self.finish_property_write(
                result.ok_or(crate::stencil_arena::ArenaError::ProtectionFailed),
            ));
        }
        None
    }

    fn finish_property_write(
        &mut self,
        result: Result<u32, crate::stencil_arena::ArenaError>,
    ) -> Result<(), crate::stencil_arena::ArenaError> {
        match result {
            Ok(1) => {
                #[cfg(test)]
                {
                    self.native_entry_count = self.native_entry_count.saturating_add(1);
                }
                Ok(())
            }
            Ok(_) => Err(crate::stencil_arena::ArenaError::ProtectionFailed),
            Err(error) => {
                self.clear_shared_capabilities();
                Err(error)
            }
        }
    }

    fn render_shared_write(
        &mut self,
        key: crate::stencil_fact::RegionKey,
        values: &crate::stencil_fact::PatchValues<'_>,
        context: &mut crate::native_property::NativePropertyWriteContext,
    ) -> Result<(), crate::stencil_arena::ArenaError> {
        let shared = self
            .shared_arena
            .clone()
            .ok_or(crate::stencil_arena::ArenaError::MappingFailed)?;
        let (address, view) = {
            let mut slab = shared.borrow_mut();
            let view = crate::stencil_select::select_physical_for_abi(
                key,
                crate::stencil_select::RegionAbi::PropertyWriteGuard,
            )
            .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
            let address =
                slab.render_physical_view_or_get(&mut self.physical.cache, view, values)?;
            slab.make_executable(address)?;
            (address, view)
        };
        let owned = shared.borrow().owned_property_write_guard_entry(address)?;
        let result = invoke_shared_entry!(shared, owned, |entry| entry(context));
        self.installed = InstalledPropertyEntry::WriteShared(owned);
        #[cfg(not(test))]
        let _ = view;
        #[cfg(test)]
        if result.is_ok() {
            self.last_native_view = Some(view);
        }
        self.finish_property_write(result)
    }

    fn render_local_write(
        &mut self,
        key: crate::stencil_fact::RegionKey,
        values: &crate::stencil_fact::PatchValues<'_>,
        context: &mut crate::native_property::NativePropertyWriteContext,
    ) -> Result<(), crate::stencil_arena::ArenaError> {
        let arena = self
            .arena
            .get_or_insert(crate::stencil_arena::StencilArena::new(4096)?);
        let view = crate::stencil_select::select_physical_for_abi(
            key,
            crate::stencil_select::RegionAbi::PropertyWriteGuard,
        )
        .ok_or(crate::stencil_arena::ArenaError::ProtectionFailed)?;
        let address = arena.render_physical_view_or_get(&mut self.physical.cache, view, values)?;
        arena.make_executable()?;
        let entry = arena.property_write_guard_entry(address)?;
        self.installed = InstalledPropertyEntry::WriteLocal(address);
        let result = entry(context);
        #[cfg(not(test))]
        let _ = view;
        #[cfg(test)]
        {
            self.last_native_view = Some(view);
        }
        self.finish_property_write(Ok(result))
    }
}

impl std::fmt::Debug for NativePropertyPlan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativePropertyPlan")
            .field("opcode", &self.opcode)
            .field(
                "used_bytes",
                &self
                    .shared_arena
                    .as_ref()
                    .map(|arena| arena.borrow().used())
                    .or_else(|| self.arena.as_ref().map(|arena| arena.used()))
                    .unwrap_or(0),
            )
            .field("cache_len", &self.physical.cache.len())
            .finish()
    }
}

/// Executable baseline entry for every compact opcode.  This is deliberately
/// a trampoline rather than a second implementation of an operation: the
/// generated bytes receive an opaque context and tail-call the canonical Rust
/// handler bridge. Specialized leaves above can still bypass this gateway;
/// every miss, throw, call, and control transition remains authoritative in
/// `run_baseline_instruction`.
pub(crate) struct NativeDispatchPlan {
    arena: Option<crate::stencil_arena::StencilArena>,
    shared_arena: Option<Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>>,
    physical: PhysicalState,
    site: crate::quickening::QuickeningSite<4>,
    opcode: crate::ir::Opcode,
}

#[derive(Debug)]
pub(crate) enum NativeDispatchError {
    /// The physical trampoline could not be selected, rendered, protected,
    /// or entered. The caller may retry the canonical Rust handler.
    Physical(String),
    /// The physical entry was already called and cannot be retried without
    /// risking duplicated effects. This is an invariant failure, not an
    /// admission miss; callers surface it as an internal VM error.
    Committed {
        pc: usize,
        message: String,
    },
    /// A canonical handler failed after region entry. The operation PC is
    /// retained so completion/exception machinery resumes after the exact
    /// failing residual operation rather than at the region start.
    SemanticAt {
        pc: usize,
        error: crate::vm::VmError,
    },
}

impl NativeDispatchError {
    pub(crate) fn committed(pc: usize, message: impl Into<String>) -> Self {
        Self::Committed {
            pc,
            message: message.into(),
        }
    }
}

impl NativeDispatchPlan {
    fn new_with_arena(
        instruction: crate::ir::Instruction,
        policy: crate::stencil_policy::ExecutionPolicy,
        shared_arena: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        let mut plan = Self::new(instruction, policy)?;
        plan.shared_arena = Some(shared_arena);
        Some(plan)
    }

    fn new(
        instruction: crate::ir::Instruction,
        policy: crate::stencil_policy::ExecutionPolicy,
    ) -> Option<Self> {
        if !policy.native_dispatch {
            return None;
        }
        let key = crate::stencil_select::dispatch_region_key();
        crate::stencil_select::select_region(key)
            .filter(|record| record.executable && validate_physical_template(record).is_ok())?;
        Some(Self {
            arena: None,
            shared_arena: None,
            physical: PhysicalState::new(),
            site: crate::quickening::QuickeningSite::new(instruction.opcode),
            opcode: instruction.opcode,
        })
    }

    pub(crate) fn execute(
        &mut self,
        code: CodeView<'_>,
        pc: usize,
        entry: BaselineEntry,
        registers: &mut crate::register_file::RegisterFile,
        context: &crate::vm::VmContext,
    ) -> Result<crate::vm::DispatchTransition, NativeDispatchError> {
        let key = crate::stencil_select::dispatch_region_key();
        let values = crate::stencil_fact::PatchValues::from_site(&self.site)
            .with_pointer_bits(crate::vm::native_dispatch_bridge as *const () as usize);
        if !crate::stencil_select::select_region(key).is_some_and(|record| record.executable)
            || self.physical.lifecycle.observe_site(&self.site, key, true)
                == crate::stencil_lifecycle::StencilState::Retired
        {
            return Err(NativeDispatchError::Physical(
                "native baseline entry unavailable".into(),
            ));
        }
        if let Some(shared) = self.shared_arena.clone() {
            let rendered = (|| {
                let mut slab = shared.borrow_mut();
                let view = crate::stencil_select::select_physical_for_abi(
                    key,
                    crate::stencil_select::RegionAbi::Bridge,
                )
                .ok_or_else(|| {
                    NativeDispatchError::Physical("native baseline stencil missing".into())
                })?;
                let address = slab
                    .render_physical_view_or_get(&mut self.physical.cache, view, &values)
                    .map_err(|error| {
                        NativeDispatchError::Physical(format!(
                            "native baseline render failed: {error:?}"
                        ))
                    })?;
                slab.make_executable(address).map_err(|error| {
                    NativeDispatchError::Physical(format!(
                        "native baseline protection failed: {error:?}"
                    ))
                })?;
                Ok::<_, NativeDispatchError>(address)
            })();
            let mut published_address = None;
            let result = rendered.and_then(|address| {
                published_address = Some(address);
                let mut dispatch =
                    crate::vm::NativeDispatchContext::new(code, pc, entry, registers, context);
                let raw = (&mut dispatch as *mut crate::vm::NativeDispatchContext<'_>)
                    .cast::<std::ffi::c_void>();
                let lease = crate::stencil_arena::SharedStencilSlab::acquire_address_lease(
                    &shared,
                    address,
                    crate::stencil_select::RegionAbi::Bridge,
                )
                .map_err(|error| {
                    NativeDispatchError::Physical(format!(
                        "native baseline lease failed: {error:?}"
                    ))
                })?;
                let status = lease.invoke_dispatch(raw).map_err(|error| {
                    NativeDispatchError::Physical(format!(
                        "native baseline execution failed: {error:?}"
                    ))
                })?;
                dispatch.finish(status)
            });
            self.physical.apply_dispatch_outcome(
                &result,
                published_address.map(|address| (&shared, address)),
            );
            return result;
        }
        if self.arena.is_none() {
            match crate::stencil_arena::StencilArena::new(4096) {
                Ok(arena) => self.arena = Some(arena),
                Err(error) => {
                    self.physical.lifecycle.reset();
                    return Err(NativeDispatchError::Physical(format!(
                        "native baseline mapping failed: {error:?}"
                    )));
                }
            }
        }
        let result = (|| {
            let arena = self.arena.as_mut().ok_or_else(|| {
                NativeDispatchError::Physical("native baseline arena missing".into())
            })?;
            let view = crate::stencil_select::select_physical_for_abi(
                key,
                crate::stencil_select::RegionAbi::Bridge,
            )
            .ok_or_else(|| {
                NativeDispatchError::Physical("native baseline stencil missing".into())
            })?;
            let address = arena
                .render_physical_view_or_get(&mut self.physical.cache, view, &values)
                .map_err(|error| {
                    NativeDispatchError::Physical(format!(
                        "native baseline render failed: {error:?}"
                    ))
                })?;
            arena.make_executable().map_err(|error| {
                NativeDispatchError::Physical(format!(
                    "native baseline protection failed: {error:?}"
                ))
            })?;
            let mut dispatch =
                crate::vm::NativeDispatchContext::new(code, pc, entry, registers, context);
            let status = arena
                .execute_dispatch(
                    address,
                    (&mut dispatch as *mut crate::vm::NativeDispatchContext<'_>)
                        .cast::<std::ffi::c_void>(),
                )
                .map_err(|error| {
                    NativeDispatchError::Physical(format!(
                        "native baseline execution failed: {error:?}"
                    ))
                })?;
            dispatch.finish(status)
        })();
        if matches!(
            result,
            Err(NativeDispatchError::Physical(_) | NativeDispatchError::Committed { .. })
        ) {
            // The trampoline carries no persistent semantic state. If mapping,
            // protection, or the bridge fails, discard the physical view and
            // make the caller use the complete ordinary path next time.
            self.arena.take();
        }
        self.physical.apply_dispatch_outcome(&result, None);
        result
    }
}

impl std::fmt::Debug for NativeDispatchPlan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeDispatchPlan")
            .field("opcode", &self.opcode)
            .field(
                "used_bytes",
                &self
                    .shared_arena
                    .as_ref()
                    .map(|arena| arena.borrow().used())
                    .or_else(|| self.arena.as_ref().map(|arena| arena.used()))
                    .unwrap_or(0),
            )
            .field("cache_len", &self.physical.cache.len())
            .finish()
    }
}

/// Bounded copy-and-patch entry for a measured straight-line region.  The
/// generated bytes only transfer control to the canonical Rust region bridge;
/// the bridge validates every opcode and transition before executing it.  No
/// region owns alternate JavaScript semantics, and any mismatch takes the
/// ordinary per-instruction path.
pub(crate) struct NativeRegionPlan {
    arena: Option<std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>>,
    physical: PhysicalState,
    site: crate::quickening::QuickeningSite<4>,
    key: crate::stencil_fact::RegionKey,
    operations: &'static [crate::ir::Opcode],
    admitted_control: Option<crate::stencil_cfg::RegionControlPlan>,
    /// Diagnostic witness set only by a direct machine-code entry. A region
    /// selected through the canonical Rust bridge is deliberately not counted
    /// as native execution.
    last_native_execution: bool,
    #[cfg(test)]
    last_native_view: Option<crate::stencil_select::PhysicalStencilView>,
    #[cfg(test)]
    physical_entry_count: u64,
}

/// Validate the complete residual window before a physical entry is
/// published.  Opcode identity alone is insufficient: operand encodings and
/// control successors are part of the canonical operation contract.  A
/// failure here is a pre-entry rejection, so callers may safely execute the
/// ordinary residual operation sequence without replaying effects.
fn validate_region_window(
    code: CodeView<'_>,
    pc: usize,
    record: &crate::stencil_select::RegionRecord,
    stencil: &crate::stencil_fact::Stencil,
) -> Result<(), NativeDispatchError> {
    let contract = record.contract();
    if !contract.executable
        || !contract.has_single_entry()
        || !contract.legal_external_entry(0)
        || contract.operations.is_empty()
    {
        return Err(NativeDispatchError::Physical(
            "native region contract has no legal entry".into(),
        ));
    }
    validate_physical_view(record, stencil).map_err(NativeDispatchError::Physical)?;
    let end = pc
        .checked_add(contract.operations.len())
        .ok_or_else(|| NativeDispatchError::Physical("native region pc overflow".into()))?;
    for (offset, expected) in contract.operations.iter().copied().enumerate() {
        let window_pc = pc
            .checked_add(offset)
            .ok_or_else(|| NativeDispatchError::Physical("native region pc overflow".into()))?;
        let instruction = code.instruction(window_pc).ok_or_else(|| {
            NativeDispatchError::Physical("native region window is incomplete".into())
        })?;
        if instruction.opcode != expected
            || !expected.operands_are_canonical([instruction.a, instruction.b, instruction.c])
        {
            return Err(NativeDispatchError::Physical(
                "native region operation contract changed before publication".into(),
            ));
        }
        match expected.control_operands(instruction) {
            crate::ir::ControlOperands::Next => {}
            crate::ir::ControlOperands::Return { .. } => {
                if window_pc + 1 != end {
                    return Err(NativeDispatchError::Physical(
                        "native region returns before its declared boundary".into(),
                    ));
                }
            }
            crate::ir::ControlOperands::Branch { target, .. }
            | crate::ir::ControlOperands::Jump { target } => {
                let target = usize::from(target);
                if target < pc || target > end {
                    return Err(NativeDispatchError::Physical(
                        "native region successor leaves its verified boundary".into(),
                    ));
                }
            }
            crate::ir::ControlOperands::Loop { .. } => {
                // Structured loop operations are not currently emitted by a
                // raw stencil.  They remain complete ordinary fallback.
                return Err(NativeDispatchError::Physical(
                    "structured loop opcode requires ordinary execution".into(),
                ));
            }
        }
    }
    Ok(())
}

fn validate_admitted_region_control(
    record: &crate::stencil_select::RegionRecord,
    pc: usize,
    control: &crate::stencil_cfg::RegionControlPlan,
) -> Result<(), NativeDispatchError> {
    let expected_end = pc
        .checked_add(record.operations.len())
        .ok_or_else(|| NativeDispatchError::Physical("native region pc overflow".into()))?;
    if control.start() != pc || control.end() != expected_end {
        return Err(NativeDispatchError::Physical(
            "native region control contract changed before entry".into(),
        ));
    }
    Ok(())
}

/// Validate the emitted template independently of opcode identity.  Every
/// physical entry is selected through a typed ABI, so calls, pointer holes,
/// checkpoints, and allocation effects must agree with that declaration.
/// Constructors for scalar/tagged leaves use this same proof before retaining
/// an entry pointer; region execution repeats it at the full-window boundary.
fn validate_physical_template(record: &crate::stencil_select::RegionRecord) -> Result<(), String> {
    validate_physical_view(record, &record.stencil)
}

fn validate_physical_view(
    record: &crate::stencil_select::RegionRecord,
    stencil: &crate::stencil_fact::Stencil,
) -> Result<(), String> {
    let contract = record.contract();
    let abi = contract.abi_contract();
    if !contract.abi_is_well_formed() {
        return Err("region ABI has inconsistent clobber/live-out/root contract".into());
    }
    if !stencil.validate() {
        return Err("native region stencil layout or relocation is invalid".into());
    }
    let physical_calls_helper = crate::stencil_physical::contains_call(stencil.bytes);
    let declared_boundary = contract.requires_semantic_boundary()
        || contract.has_effect(crate::facts::OperationEffect::Control)
        || contract.has_effect(crate::facts::OperationEffect::ReadHeap)
        || contract.has_effect(crate::facts::OperationEffect::WriteHeap);
    if contract.template_calls_helper
        && (!abi.may_call_helper || !abi.root_materialization_required || !declared_boundary)
    {
        return Err("declared helper boundary lacks ABI/effect/root contract".into());
    }
    if physical_calls_helper {
        if !abi.may_call_helper {
            return Err("native stencil contains a call outside its ABI contract".into());
        }
        // A helper-capable entry must also have a semantic boundary in the
        // canonical operation facts.  The call bit alone is not proof that
        // roots, exceptions, re-entry, or observable effects were discharged.
        // Control transitions are boundaries too: a bridge may call the
        // canonical handler for a Return/Jump even when that operation is not
        // allocating or throwing.
        if !declared_boundary || !abi.root_materialization_required {
            return Err("helper call lacks a declared semantic boundary and root contract".into());
        }
    }
    if abi.interruptible_backedge
        && !crate::stencil_physical::contains_interrupt_checkpoint(stencil.bytes)
    {
        return Err("interruptible region has no verified native checkpoint".into());
    }
    let pointer_holes = stencil
        .holes
        .iter()
        .filter(|hole| matches!(hole.kind, crate::stencil_fact::HoleKind::Ptr64))
        .count();
    let pointer_contract = abi_pointer_hole_contract(contract.abi);
    if pointer_holes != pointer_contract {
        return Err(format!(
            "ABI permits {pointer_contract} external pointer relocations, template has {pointer_holes}"
        ));
    }
    if raw_region_declares_allocation(contract) {
        return Err("raw array region declares an allocating operation".into());
    }
    if cfg!(target_arch = "aarch64") && !stencil_has_data_holes(stencil) {
        crate::stencil_physical::validate_aarch64_instruction_stream(stencil.bytes)?;
    }
    if matches!(
        contract.abi,
        crate::stencil_select::RegionAbi::ArrayKernel
            | crate::stencil_select::RegionAbi::ArrayNumericLoop
    ) {
        crate::stencil_physical::validate_raw_instruction_stream(stencil.bytes)?;
        let actual = crate::stencil_physical::simd_clobber_mask(stencil.bytes);
        if actual & !abi.hardware_clobber_mask != 0 {
            return Err(format!(
                "raw ABI declares clobber mask {:04x}, template uses undeclared {:04x}",
                abi.hardware_clobber_mask,
                actual & !abi.hardware_clobber_mask
            ));
        }
        let actual_gpr = crate::stencil_physical::gpr_clobber_mask(stencil.bytes);
        if actual_gpr & !abi.hardware_gpr_clobber_mask != 0 {
            return Err(format!(
                "raw ABI declares GPR clobber mask {:04x}, template uses undeclared {:04x}",
                abi.hardware_gpr_clobber_mask,
                actual_gpr & !abi.hardware_gpr_clobber_mask
            ));
        }
    }
    Ok(())
}

fn stencil_has_data_holes(stencil: &crate::stencil_fact::Stencil) -> bool {
    stencil.holes.iter().any(|hole| {
        matches!(
            hole.kind,
            crate::stencil_fact::HoleKind::Literal64 | crate::stencil_fact::HoleKind::Ptr64
        )
    })
}

fn abi_pointer_hole_contract(abi: crate::stencil_select::RegionAbi) -> usize {
    matches!(abi, crate::stencil_select::RegionAbi::Bridge) as usize
}

fn raw_region_declares_allocation(contract: crate::stencil_select::RegionContract) -> bool {
    matches!(
        contract.abi,
        crate::stencil_select::RegionAbi::ArrayKernel
            | crate::stencil_select::RegionAbi::ArrayNumericLoop
    ) && contract.has_effect(crate::facts::OperationEffect::Allocate)
}

impl NativeRegionPlan {
    fn new_with_arena(
        key: crate::stencil_fact::RegionKey,
        policy: crate::stencil_policy::ExecutionPolicy,
        arena: std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>,
        control: crate::stencil_cfg::RegionControlPlan,
    ) -> Option<Self> {
        let composed = crate::stencil_select::select_region(key).is_some_and(|record| {
            matches!(
                record.abi,
                crate::stencil_select::RegionAbi::ArrayKernel
                    | crate::stencil_select::RegionAbi::ArrayNumericLoop
            )
        });
        Self::new_inner(
            key,
            policy.fused_regions || (policy.composed_regions && composed),
            arena,
            Some(control),
        )
    }

    fn new_inner(
        key: crate::stencil_fact::RegionKey,
        enabled: bool,
        arena: std::rc::Rc<std::cell::RefCell<crate::stencil_arena::SharedStencilSlab>>,
        admitted_control: Option<crate::stencil_cfg::RegionControlPlan>,
    ) -> Option<Self> {
        if !enabled {
            return None;
        }
        let view = crate::stencil_select::select_physical(key)?;
        let record = view.record;
        if !record.executable
            || record.operations.is_empty()
            || validate_physical_view(record, view.stencil).is_err()
            || !record.abi.accepts_region_context()
            || !view.executable
            || !view.stencil.validate()
            || view.abi != record.abi
        {
            return None;
        }
        Some(Self {
            arena: Some(arena),
            physical: PhysicalState::new(),
            site: crate::quickening::QuickeningSite::new(record.operations[0]),
            key,
            operations: record.operations,
            admitted_control,
            last_native_execution: false,
            #[cfg(test)]
            last_native_view: None,
            #[cfg(test)]
            physical_entry_count: 0,
        })
    }

    pub(crate) fn last_native_execution(&self) -> bool {
        self.last_native_execution
    }

    #[cfg(test)]
    pub(crate) fn last_native_view_for_test(
        &self,
    ) -> Option<crate::stencil_select::PhysicalStencilView> {
        self.last_native_view
    }

    #[cfg(test)]
    pub(crate) fn key_for_test(&self) -> crate::stencil_fact::RegionKey {
        self.key
    }

    #[cfg(test)]
    pub(crate) fn admitted_control_for_test(
        &self,
    ) -> Option<crate::stencil_cfg::RegionControlPlan> {
        self.admitted_control
    }

    #[cfg(test)]
    pub(crate) fn physical_entry_count_for_test(&self) -> u64 {
        self.physical_entry_count
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(key: crate::stencil_fact::RegionKey) -> Option<Self> {
        let arena = std::rc::Rc::new(std::cell::RefCell::new(
            crate::stencil_arena::SharedStencilSlab::new(4096).ok()?,
        ));
        Self::new_inner(key, true, arena, None)
    }

    pub(crate) fn execute(
        &mut self,
        code: CodeView<'_>,
        pc: usize,
        registers: &mut crate::register_file::RegisterFile,
        context: &crate::vm::VmContext,
    ) -> Result<crate::vm::DispatchTransition, NativeDispatchError> {
        self.last_native_execution = false;
        #[cfg(test)]
        {
            self.last_native_view = None;
        }
        let values = crate::stencil_fact::PatchValues::from_site(&self.site)
            .with_pointer_bits(crate::vm::native_region_bridge as *const () as usize);
        let key = self.key;
        let operations = self.operations;
        // Verify the complete immutable residual window before rendering or
        // publishing executable bytes.  A stale/quickened opcode is therefore
        // a cheap RejectBeforeEntry and cannot consume slab capacity or leave
        // a physical mapping behind for a region that was never legal.
        let Some(record) = crate::stencil_select::select_region(key) else {
            return Err(NativeDispatchError::Physical(
                "native fused region stencil missing".into(),
            ));
        };
        let Some(view) = crate::stencil_select::select_physical(key) else {
            return Err(NativeDispatchError::Physical(
                "native fused region physical view rejected".into(),
            ));
        };
        if record.operations != operations {
            return Err(NativeDispatchError::Physical(
                "native fused region operation contract changed".into(),
            ));
        }
        if let Some(control) = self.admitted_control {
            validate_admitted_region_control(record, pc, &control)?;
        }
        validate_region_window(code, pc, record, view.stencil)?;
        if !record.executable
            || self.physical.lifecycle.observe_site(&self.site, key, true)
                == crate::stencil_lifecycle::StencilState::Retired
        {
            return Err(NativeDispatchError::Physical(
                "native fused region unavailable".into(),
            ));
        }
        let mut published_address = None;
        let result = (|| {
            let arena = self.arena.as_ref().ok_or_else(|| {
                NativeDispatchError::Physical("native fused region arena missing".into())
            })?;
            let record = view.record;
            let contract = record.contract();
            if !contract.legal_external_entry(0) {
                return Err(NativeDispatchError::Physical(
                    "native fused region has no legal external entry".into(),
                ));
            }
            // The generated declaration carries the physical ABI.  Fail
            // closed if metadata and the selected invocation path disagree;
            // an opcode-prefix match is never sufficient to pass a scalar
            // or raw-array entry a NativeRegionContext pointer.
            if !contract.abi.accepts_region_context() {
                return Err(NativeDispatchError::Physical(
                    "native fused region ABI metadata mismatch".into(),
                ));
            }
            let address = arena
                .borrow_mut()
                .render_physical_view_or_get(&mut self.physical.cache, view, &values)
                .map_err(|error| {
                    NativeDispatchError::Physical(format!(
                        "native fused region render failed: {error:?}"
                    ))
                })?;
            arena
                .borrow_mut()
                .make_executable(address)
                .map_err(|error| {
                    NativeDispatchError::Physical(format!(
                        "native fused region protection failed: {error:?}"
                    ))
                })?;
            published_address = Some(address);
            let storage_kind = record.name;
            let (used_bytes, capacity_bytes) = {
                let slab = arena.borrow();
                (slab.used(), slab.capacity())
            };
            crate::execution_trace::stencil_storage(
                code,
                pc,
                storage_kind,
                used_bytes,
                capacity_bytes,
            );
            let mut region = crate::vm::NativeRegionContext::new_with_abi(
                code, pc, operations, view.abi, registers, context,
            );
            #[cfg(not(target_arch = "aarch64"))]
            if matches!(
                record.abi,
                crate::stencil_select::RegionAbi::ArrayKernel
                    | crate::stencil_select::RegionAbi::ArrayNumericLoop
            ) {
                return crate::vm::execute_region_fallback(&mut region);
            }
            // The array block has a direct raw numeric entry on AArch64. Its
            // ABI context contains only proven backing words and scalar
            // operands; no VM object or Rust reference crosses the boundary.
            // Other rows retain the canonical bridge until they have an
            // equally complete physical implementation.
            #[cfg(target_arch = "aarch64")]
            match view.abi {
                crate::stencil_select::RegionAbi::ArrayKernel => {
                    let physical = crate::vm::execute_composed_array_kernel(&mut region, |raw| {
                        let lease = crate::stencil_arena::SharedStencilSlab::acquire_address_lease(
                            &arena,
                            address,
                            crate::stencil_select::RegionAbi::ArrayKernel,
                        )?;
                        lease.invoke_dispatch(raw)
                    });
                    self.last_native_execution |= region.native_entered;
                    #[cfg(test)]
                    if region.native_entered {
                        self.last_native_view = Some(view);
                    }
                    if let Some(result) = physical? {
                        return Ok(result);
                    }
                    // A failed admission has not entered the raw bytes. Reuse
                    // the complete semantic bridge so Unknown cases retain
                    // ordinary behavior.
                    return crate::vm::execute_region_fallback(&mut region);
                }
                crate::stencil_select::RegionAbi::ArrayNumericLoop => {
                    let physical =
                        crate::vm::execute_composed_array_numeric_loop(&mut region, |raw| {
                            let lease =
                                crate::stencil_arena::SharedStencilSlab::acquire_address_lease(
                                    &arena,
                                    address,
                                    crate::stencil_select::RegionAbi::ArrayNumericLoop,
                                )?;
                            lease.invoke_dispatch(raw)
                        });
                    self.last_native_execution |= region.native_entered;
                    #[cfg(test)]
                    if region.native_entered {
                        self.last_native_view = Some(view);
                    }
                    if let Some(result) = physical? {
                        return Ok(result);
                    }
                    return crate::vm::execute_region_fallback(&mut region);
                }
                crate::stencil_select::RegionAbi::Bridge => {}
                crate::stencil_select::RegionAbi::ScalarF64Binary
                | crate::stencil_select::RegionAbi::ScalarF64Unary
                | crate::stencil_select::RegionAbi::ScalarF64x3 => {
                    return Err(NativeDispatchError::Physical(
                        "scalar ABI cannot enter a region context".into(),
                    ));
                }
                crate::stencil_select::RegionAbi::TaggedWord => {
                    return Err(NativeDispatchError::Physical(
                        "tagged-word ABI cannot enter a region context".into(),
                    ));
                }
                crate::stencil_select::RegionAbi::PropertyGuard => {
                    return Err(NativeDispatchError::Physical(
                        "property-guard ABI cannot enter a region context".into(),
                    ));
                }
                crate::stencil_select::RegionAbi::PropertyWriteGuard => {
                    return Err(NativeDispatchError::Physical(
                        "property-write ABI cannot enter a region context".into(),
                    ));
                }
                crate::stencil_select::RegionAbi::CompareBranch => {
                    return Err(NativeDispatchError::Physical(
                        "compare-branch ABI requires its typed entry".into(),
                    ));
                }
                crate::stencil_select::RegionAbi::ConstantWord => {
                    return Err(NativeDispatchError::Physical(
                        "constant-word ABI cannot enter a region context".into(),
                    ));
                }
                crate::stencil_select::RegionAbi::ScalarBool => {
                    return Err(NativeDispatchError::Physical(
                        "scalar-bool ABI cannot enter a region context".into(),
                    ));
                }
                crate::stencil_select::RegionAbi::ScalarWordBool => {
                    return Err(NativeDispatchError::Physical(
                        "scalar-word-bool ABI cannot enter a region context".into(),
                    ));
                }
                crate::stencil_select::RegionAbi::ScalarWordPairBool => {
                    return Err(NativeDispatchError::Physical(
                        "scalar-word-pair-bool ABI cannot enter a region context".into(),
                    ));
                }
                crate::stencil_select::RegionAbi::ScalarI32 => {
                    return Err(NativeDispatchError::Physical(
                        "scalar i32 ABI cannot enter a region context".into(),
                    ));
                }
                crate::stencil_select::RegionAbi::ScalarU32 => {
                    return Err(NativeDispatchError::Physical(
                        "scalar u32 ABI cannot enter a region context".into(),
                    ));
                }
            }
            let raw =
                (&mut region as *mut crate::vm::NativeRegionContext<'_>).cast::<std::ffi::c_void>();
            let lease = crate::stencil_arena::SharedStencilSlab::acquire_address_lease(
                &arena, address, view.abi,
            )
            .map_err(|error| {
                NativeDispatchError::Physical(format!(
                    "native fused region lease failed: {error:?}"
                ))
            })?;
            let status = lease.invoke_dispatch(raw).map_err(|error| {
                NativeDispatchError::Physical(format!(
                    "native fused region execution failed: {error:?}"
                ))
            })?;
            #[cfg(test)]
            {
                self.physical_entry_count = self.physical_entry_count.saturating_add(1);
                self.last_native_view = Some(view);
            }
            region.finish(status)
        })();
        let published = self
            .arena
            .as_ref()
            .and_then(|arena| published_address.map(|address| (arena, address)));
        self.physical.apply_dispatch_outcome(&result, published);
        result
    }
}

impl std::fmt::Debug for NativeRegionPlan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeRegionPlan")
            .field("key", &self.key)
            .field("operations", &self.operations)
            .field(
                "used_bytes",
                &self.arena.as_ref().map_or(0, |arena| arena.borrow().used()),
            )
            .finish()
    }
}

/// Build-time/runtime boundary for the baseline tier.  Decoding and control
/// facts are computed once when a function becomes hot; values, effects, and
/// exception behavior remain owned by the canonical VM handlers.
#[derive(Debug, Clone)]
pub(crate) struct BaselinePlan {
    entries: Rc<[BaselineEntry]>,
    osr_entries: Rc<[u32]>,
    /// Sparse physical admissions.  The fixed-width span index is the only
    /// per-PC storage; alternatives are retained as typed records in one
    /// compact flat array, so polymorphic sites do not lose valid choices.
    admission: Option<Rc<AdmissionStorage<NativeAdmission>>>,
    /// All composed entries in one baseline view share a bounded slab owner.
    /// Scalar leaves retain their narrower per-plan arenas until they acquire
    /// an equally typed shared physical contract.
    shared_region_arena: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
}

impl PartialEq for BaselinePlan {
    fn eq(&self, other: &Self) -> bool {
        self.entries == other.entries && self.osr_entries == other.osr_entries
    }
}

impl Eq for BaselinePlan {}

/// One build-time-lowered baseline entry.  The instruction remains canonical;
/// handler/control facts are the mechanical consequences cached beside it.
#[derive(Clone, Copy)]
pub(crate) struct BaselineEntry {
    pub(crate) instruction: crate::ir::Instruction,
    pub(crate) handler: crate::ir::CompactHandler,
    pub(crate) control: crate::ir::ControlOperands,
}

impl std::fmt::Debug for BaselineEntry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BaselineEntry")
            .field("instruction", &self.instruction)
            .field("control", &self.control)
            .finish()
    }
}

#[derive(Clone)]
enum NativeAdmission {
    Binary(Rc<RefCell<NativeBinaryPlan>>),
    LoadConst(Rc<RefCell<NativeLoadConstPlan>>),
    Truthiness(Rc<RefCell<NativeTruthinessPlan>>),
    Nullish(Rc<RefCell<NativeNullishPlan>>),
    Unary(Rc<RefCell<NativeUnaryPlan>>),
    AddChain(Rc<RefCell<NativeAddChainPlan>>),
    LocalBinary(Rc<RefCell<crate::stencil_fusion::NativeLocalBinaryPlan>>),
    LocalProperty(Rc<RefCell<crate::stencil_fusion::NativeLocalPropertyPlan>>),
    Move(Rc<RefCell<NativeMovePlan>>),
    LoadLocal(Rc<RefCell<NativeMovePlan>>),
    StoreLocal(Rc<RefCell<NativeMovePlan>>),
    StoreProperty(Rc<RefCell<NativePropertyPlan>>),
    Property(Rc<RefCell<NativePropertyPlan>>),
    Dispatch(Rc<RefCell<NativeDispatchPlan>>),
    Region(Rc<RefCell<NativeRegionPlan>>),
}

impl AdmissionEntry for NativeAdmission {
    fn retained_metadata_bytes(&self) -> usize {
        use crate::stencil_admission_budget::shared_value_bytes;
        match self {
            Self::Binary(_) => shared_value_bytes::<RefCell<NativeBinaryPlan>>(),
            Self::LoadConst(_) => shared_value_bytes::<RefCell<NativeLoadConstPlan>>(),
            Self::Truthiness(_) => shared_value_bytes::<RefCell<NativeTruthinessPlan>>(),
            Self::Nullish(_) => shared_value_bytes::<RefCell<NativeNullishPlan>>(),
            Self::Unary(_) => shared_value_bytes::<RefCell<NativeUnaryPlan>>(),
            Self::AddChain(_) => shared_value_bytes::<RefCell<NativeAddChainPlan>>(),
            Self::LocalBinary(_) => {
                shared_value_bytes::<RefCell<crate::stencil_fusion::NativeLocalBinaryPlan>>()
            }
            Self::LocalProperty(_) => {
                shared_value_bytes::<RefCell<crate::stencil_fusion::NativeLocalPropertyPlan>>()
            }
            Self::Move(_) | Self::LoadLocal(_) | Self::StoreLocal(_) => {
                shared_value_bytes::<RefCell<NativeMovePlan>>()
            }
            Self::StoreProperty(_) | Self::Property(_) => {
                shared_value_bytes::<RefCell<NativePropertyPlan>>()
            }
            Self::Dispatch(_) => shared_value_bytes::<RefCell<NativeDispatchPlan>>(),
            Self::Region(_) => shared_value_bytes::<RefCell<NativeRegionPlan>>(),
        }
    }
}

impl std::fmt::Debug for NativeAdmission {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Binary(_) => "binary",
            Self::LoadConst(_) => "load_const",
            Self::Truthiness(_) => "truthiness",
            Self::Nullish(_) => "nullish",
            Self::Unary(_) => "unary",
            Self::AddChain(_) => "add_chain",
            Self::LocalBinary(_) => "local_binary",
            Self::LocalProperty(_) => "local_property",
            Self::Move(_) => "move",
            Self::LoadLocal(_) => "load_local",
            Self::StoreLocal(_) => "store_local",
            Self::StoreProperty(_) => "store_property",
            Self::Property(_) => "property",
            Self::Dispatch(_) => "dispatch",
            Self::Region(_) => "region",
        };
        formatter.write_str(name)
    }
}

macro_rules! native_admission {
    ($variant:ident, $plan:expr) => {
        $plan.map(|plan| NativeAdmission::$variant(Rc::new(RefCell::new(plan))))
    };
}

macro_rules! typed_admission_accessors {
    ($handle:ident, $public:ident, $variant:ident, $ty:ty) => {
        fn $handle(&self, pc: usize) -> Option<&Rc<RefCell<$ty>>> {
            self.native_handle(pc, |admission| match admission {
                NativeAdmission::$variant(plan) => Some(plan),
                _ => None,
            })
        }

        pub(crate) fn $public(&self, pc: usize) -> Option<&RefCell<$ty>> {
            self.$handle(pc).map(Rc::as_ref)
        }
    };
}

macro_rules! optimizing_admission_accessors {
    ($name:ident, $variant:ident, $ty:ty) => {
        pub(crate) fn $name(&self) -> Option<&RefCell<$ty>> {
            self.native_handle(|admission| match admission {
                NativeAdmission::$variant(plan) => Some(plan),
                _ => None,
            })
        }
    };
}

impl PartialEq for BaselineEntry {
    fn eq(&self, other: &Self) -> bool {
        self.instruction == other.instruction && self.control == other.control
    }
}

impl Eq for BaselineEntry {}

/// Admission-time view of the generated region contract.  This keeps static
/// shape checks out of the hot driver: execution still revalidates the live
/// code window, but malformed operands or successors are rejected before a
/// plan can render or publish executable bytes.
fn region_admission_control(
    entries: &[BaselineEntry],
    cfg: &ControlFlowFacts,
    start: usize,
    record: &crate::stencil_select::RegionRecord,
) -> Option<crate::stencil_cfg::RegionControlPlan> {
    let contract = record.contract();
    if !contract.executable || !contract.has_single_entry() || !contract.legal_external_entry(0) {
        return None;
    }
    let control = cfg.region_plan(entries, start, contract.operations)?;
    (record.bindings_match_entries(entries, start)
        && region_outputs_cover_exit(entries, cfg, start, record))
    .then_some(control)
}

#[cfg(test)]
fn region_admission_matches(
    entries: &[BaselineEntry],
    cfg: &ControlFlowFacts,
    start: usize,
    record: &crate::stencil_select::RegionRecord,
) -> bool {
    region_admission_control(entries, cfg, start, record).is_some()
}

fn region_outputs_cover_exit(
    entries: &[BaselineEntry],
    cfg: &ControlFlowFacts,
    start: usize,
    record: &crate::stencil_select::RegionRecord,
) -> bool {
    if record.outputs.is_empty() {
        let abi = record.abi.contract();
        if abi.preserves_vm_registers || abi.may_call_helper {
            return true;
        }
    }
    let Some(end) = start.checked_add(record.operations.len()) else {
        return false;
    };
    if end == entries.len() {
        return true;
    }
    cfg.live_in_at(end)
        .is_some_and(|live| record.outputs_cover_live_definitions(entries, start, live))
}

type SharedStencilPool = Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>;

fn collect_numeric_admissions(
    builder: &mut AdmissionBuilder<NativeAdmission>,
    entries: &[BaselineEntry],
    cfg: &ControlFlowFacts,
    pc: usize,
    entry: BaselineEntry,
    code: CodeView<'_>,
    policy: crate::stencil_policy::ExecutionPolicy,
    arena: &SharedStencilPool,
) {
    let instruction = entry.instruction;
    let binary =
        NativeBinaryPlan::new_with_shared(instruction, policy, Rc::clone(arena)).map(|mut plan| {
            if plan.returns_boolean() {
                if let Some(branch) = compare_branch(entries, cfg, pc, instruction) {
                    plan.install_compare_branch(branch);
                }
            }
            plan
        });
    builder.push_optional(pc, native_admission!(Binary, binary));
    let constant = (instruction.opcode == crate::ir::Opcode::LoadConst)
        .then(|| code.constant(instruction.b).and_then(constant_word_bits))
        .flatten();
    builder.push_optional(
        pc,
        native_admission!(
            LoadConst,
            constant.and_then(|bits| NativeLoadConstPlan::new_with_shared(
                bits,
                policy,
                Rc::clone(arena)
            ))
        ),
    );
    builder.push_optional(
        pc,
        native_admission!(
            Truthiness,
            NativeTruthinessPlan::new_with_shared(instruction, policy, Rc::clone(arena))
        ),
    );
    builder.push_optional(
        pc,
        native_admission!(
            Nullish,
            NativeNullishPlan::new_with_shared(instruction, policy, Rc::clone(arena))
        ),
    );
    builder.push_optional(
        pc,
        native_admission!(
            Unary,
            NativeUnaryPlan::new_with_shared(instruction, policy, Rc::clone(arena))
        ),
    );
}

pub(crate) fn compare_branch(
    entries: &[BaselineEntry],
    cfg: &ControlFlowFacts,
    pc: usize,
    comparison: crate::ir::Instruction,
) -> Option<CompareBranch> {
    for offset in 1..crate::stencil_plan::MAX_BLOCK_VALUES {
        let branch_pc = pc.checked_add(offset)?;
        let entry = entries.get(branch_pc)?;
        if entry.instruction.opcode == crate::ir::Opcode::JumpIfFalse {
            return compare_branch_at(entries, cfg, pc, branch_pc, comparison, entry.instruction);
        }
        if !dead_pure_definition(entry, cfg.live_out().get(branch_pc)?) {
            return None;
        }
    }
    None
}

fn compare_branch_at(
    entries: &[BaselineEntry],
    cfg: &ControlFlowFacts,
    start: usize,
    branch_pc: usize,
    comparison: crate::ir::Instruction,
    branch: crate::ir::Instruction,
) -> Option<CompareBranch> {
    let false_target = usize::from(branch.b);
    if branch.a != comparison.a
        || !branch
            .opcode
            .operands_are_canonical([branch.a, branch.b, branch.c])
        || false_target >= entries.len()
    {
        return None;
    }
    let end = branch_pc.checked_add(1)?;
    let physical_key = comparison_branch_key(comparison.flags);
    let control = cfg.region_control(start, end)?;
    let (planned_false, planned_true) = control.terminal_conditional_exits()?;
    (planned_false == false_target && planned_true == end).then_some(CompareBranch {
        control,
        physical_key,
    })
}

fn comparison_branch_key(flags: u8) -> Option<crate::stencil_fact::RegionKey> {
    use crate::ops::BinaryOp::*;
    match crate::ir::compact_binary_operator(flags)? {
        Equal | StrictEqual => Some(crate::stencil_select::compare_equal_branch_region_key()),
        NotEqual | StrictNotEqual => {
            Some(crate::stencil_select::compare_not_equal_branch_region_key())
        }
        LessThan => Some(crate::stencil_select::compare_less_branch_region_key()),
        LessEqual => Some(crate::stencil_select::compare_less_equal_branch_region_key()),
        GreaterThan => Some(crate::stencil_select::compare_greater_branch_region_key()),
        GreaterEqual => Some(crate::stencil_select::compare_greater_equal_branch_region_key()),
        _ => None,
    }
}

fn dead_pure_definition(entry: &BaselineEntry, live_after: &BTreeSet<u16>) -> bool {
    let flow = entry.instruction.register_flow();
    entry.control == crate::ir::ControlOperands::Next
        && entry.instruction.opcode.effects() == &[crate::facts::OperationEffect::Pure]
        && flow.complete
        && flow
            .definition
            .is_some_and(|definition| !live_after.contains(&definition))
}

fn move_admission(
    instruction: crate::ir::Instruction,
    policy: crate::stencil_policy::ExecutionPolicy,
    arena: &SharedStencilPool,
) -> Option<NativeAdmission> {
    let plan = NativeMovePlan::new_with_arena(instruction, policy, Rc::clone(arena))?;
    let plan = Rc::new(RefCell::new(plan));
    match instruction.opcode {
        crate::ir::Opcode::Move => Some(NativeAdmission::Move(plan)),
        crate::ir::Opcode::LoadLocal => Some(NativeAdmission::LoadLocal(plan)),
        crate::ir::Opcode::StoreLocal => Some(NativeAdmission::StoreLocal(plan)),
        _ => None,
    }
}

fn property_admission(
    instruction: crate::ir::Instruction,
    policy: crate::stencil_policy::ExecutionPolicy,
    arena: &SharedStencilPool,
) -> Option<NativeAdmission> {
    let plan = NativePropertyPlan::new_with_arena(instruction, policy, Rc::clone(arena))?;
    let plan = Rc::new(RefCell::new(plan));
    match instruction.opcode {
        crate::ir::Opcode::GetN => Some(NativeAdmission::Property(plan)),
        crate::ir::Opcode::SetN => Some(NativeAdmission::StoreProperty(plan)),
        _ => None,
    }
}

fn collect_memory_admissions(
    builder: &mut AdmissionBuilder<NativeAdmission>,
    pc: usize,
    instruction: crate::ir::Instruction,
    policy: crate::stencil_policy::ExecutionPolicy,
    arena: &SharedStencilPool,
) {
    builder.push_optional(pc, move_admission(instruction, policy, arena));
    builder.push_optional(pc, property_admission(instruction, policy, arena));
    builder.push_optional(
        pc,
        native_admission!(
            Dispatch,
            NativeDispatchPlan::new_with_arena(instruction, policy, Rc::clone(arena))
        ),
    );
}

fn add_chain_admission(
    entries: &[BaselineEntry],
    cfg: &ControlFlowFacts,
    pc: usize,
    policy: crate::stencil_policy::ExecutionPolicy,
    arena: &SharedStencilPool,
) -> Option<NativeAdmission> {
    let control = cfg.region_control(pc, pc.checked_add(2)?)?;
    let entry = entries.get(pc)?;
    let next = entries.get(pc + 1)?;
    let live_after = cfg.live_out().get(pc + 1)?;
    let selection =
        crate::stencil_plan::select_add_chain(entry.instruction, next.instruction, live_after)?;
    NativeAddChainPlan::new_with_arena(policy, Rc::clone(arena), selection.bindings, control)
        .map(|plan| NativeAdmission::AddChain(Rc::new(RefCell::new(plan))))
}

fn local_binary_admission(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &ControlFlowFacts,
    pc: usize,
    policy: crate::stencil_policy::ExecutionPolicy,
    arena: &SharedStencilPool,
) -> Option<NativeAdmission> {
    let selection = select_local_numeric(code, entries, cfg, pc)?;
    let plan =
        crate::stencil_fusion::NativeLocalBinaryPlan::new(selection, policy, Rc::clone(arena))?;
    Some(NativeAdmission::LocalBinary(Rc::new(RefCell::new(plan))))
}

fn local_property_admission(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &ControlFlowFacts,
    pc: usize,
    policy: crate::stencil_policy::ExecutionPolicy,
    arena: &SharedStencilPool,
) -> Option<NativeAdmission> {
    let selection = select_local_property(code, entries, cfg, pc)?;
    let plan =
        crate::stencil_fusion::NativeLocalPropertyPlan::new(selection, policy, Rc::clone(arena))?;
    Some(NativeAdmission::LocalProperty(Rc::new(RefCell::new(plan))))
}

fn select_local_numeric(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &ControlFlowFacts,
    pc: usize,
) -> Option<crate::stencil_plan::LocalBinarySelection> {
    select_value_window(code, entries, cfg, pc, |code, graph, operation, live| {
        if operation.opcode == crate::ir::Opcode::AddConst {
            let crate::ops::Constant::Number(value) = code.constant(operation.c)? else {
                return None;
            };
            if graph.len() == 1 {
                select_graph_add_const(code, graph.first()?, operation, live)
            } else {
                graph.select_add_const(operation, value.to_bits(), live)
            }
        } else {
            graph.select(operation, live)
        }
    })
}

fn select_local_property(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &ControlFlowFacts,
    pc: usize,
) -> Option<crate::stencil_plan::LocalPropertySelection> {
    select_value_window(code, entries, cfg, pc, |_, graph, operation, live| {
        graph.select_property(operation, live)
    })
}

fn select_value_window<T: crate::stencil_plan::RankedSelection>(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &ControlFlowFacts,
    pc: usize,
    mut select: impl FnMut(
        CodeView<'_>,
        crate::stencil_plan::BlockValueGraph,
        crate::ir::Instruction,
        &BTreeSet<u16>,
    ) -> Option<T>,
) -> Option<T> {
    let mut graph = crate::stencil_plan::BlockValueGraph::new();
    let mut best: Option<T> = None;
    for offset in 0..crate::stencil_plan::MAX_BLOCK_VALUES {
        let Some((operation_pc, operation, end)) =
            extend_value_window(code, entries, pc, offset, &mut graph)
        else {
            break;
        };
        let Some(live_after) = cfg.live_out().get(operation_pc) else {
            break;
        };
        if cfg.region_control(pc, end).is_none() {
            continue;
        }
        let Some(selection) = select(code, graph, operation, live_after) else {
            continue;
        };
        if best
            .as_ref()
            .is_none_or(|current| selection.rank() > current.rank())
        {
            best = Some(selection);
        }
    }
    best
}

fn extend_value_window(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    pc: usize,
    offset: usize,
    graph: &mut crate::stencil_plan::BlockValueGraph,
) -> Option<(usize, crate::ir::Instruction, usize)> {
    let instruction = entries.get(pc.checked_add(offset)?)?.instruction;
    graph
        .push(instruction, |constant| {
            let crate::ops::Constant::Number(value) = code.constant(constant)? else {
                return None;
            };
            Some(value.to_bits())
        })
        .then_some(())?;
    let operation_pc = pc.checked_add(graph.len())?;
    let operation = entries.get(operation_pc)?.instruction;
    Some((operation_pc, operation, operation_pc.checked_add(1)?))
}

fn select_graph_add_const(
    code: CodeView<'_>,
    producer: crate::stencil_plan::NumericProducer,
    operation: crate::ir::Instruction,
    live_after: &BTreeSet<u16>,
) -> Option<crate::stencil_plan::LocalBinarySelection> {
    if operation.opcode != crate::ir::Opcode::AddConst {
        return None;
    }
    let bits = match code.constant(operation.c)? {
        crate::ops::Constant::Number(value) => value.to_bits(),
        _ => return None,
    };
    crate::stencil_plan::select_source_add_const(producer, operation, bits, live_after)
}

fn region_admission(
    entries: &[BaselineEntry],
    cfg: &ControlFlowFacts,
    pc: usize,
    policy: crate::stencil_policy::ExecutionPolicy,
    arena: &SharedStencilPool,
) -> Option<NativeAdmission> {
    let (record, control) = crate::stencil_select::region_records()
        .iter()
        .filter_map(|record| {
            (record.executable && record.abi.accepts_region_context()).then_some(())?;
            let control = region_admission_control(entries, cfg, pc, record)?;
            Some((record, control))
        })
        .max_by_key(|(record, _)| crate::stencil_select::admission_rank(record))?;
    NativeRegionPlan::new_with_arena(record.key, policy, Rc::clone(arena), control)
        .map(|plan| NativeAdmission::Region(Rc::new(RefCell::new(plan))))
}

fn baseline_entries(code: CodeView<'_>) -> Rc<[BaselineEntry]> {
    (0..code.len())
        .filter_map(|pc| {
            let instruction = code.instruction(pc)?;
            Some(BaselineEntry {
                instruction,
                handler: instruction.opcode.handler(),
                control: instruction.opcode.control_operands(instruction),
            })
        })
        .collect::<Vec<_>>()
        .into()
}

fn baseline_osr_entries(code: CodeView<'_>) -> Rc<[u32]> {
    (0..code.len())
        .filter_map(|pc| {
            let instruction = code.instruction(pc)?;
            is_osr_candidate(pc, instruction).then_some(pc as u32)
        })
        .collect::<Vec<_>>()
        .into()
}

fn collect_admissions_at(
    builder: &mut AdmissionBuilder<NativeAdmission>,
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &ControlFlowFacts,
    pc: usize,
    policy: crate::stencil_policy::ExecutionPolicy,
    arena: &SharedStencilPool,
) {
    let entry = entries[pc];
    collect_numeric_admissions(builder, entries, cfg, pc, entry, code, policy, arena);
    builder.push_optional(pc, add_chain_admission(entries, cfg, pc, policy, arena));
    builder.push_optional(
        pc,
        local_binary_admission(code, entries, cfg, pc, policy, arena),
    );
    builder.push_optional(
        pc,
        local_property_admission(code, entries, cfg, pc, policy, arena),
    );
    collect_memory_admissions(builder, pc, entry.instruction, policy, arena);
    builder.push_optional(pc, region_admission(entries, cfg, pc, policy, arena));
}

fn build_admissions(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    policy: crate::stencil_policy::ExecutionPolicy,
) -> (
    Option<Rc<AdmissionStorage<NativeAdmission>>>,
    SharedStencilPool,
) {
    let arena = Rc::new(RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096)
            .expect("compile-time region slab capacity is valid"),
    ));
    if !policy.allows_admission() {
        return (None, arena);
    }
    let operand_windows = (0..entries.len())
        .map(|pc| code.operand_window_at(pc))
        .collect::<Vec<_>>();
    let cfg = ControlFlowFacts::new(entries, &operand_windows);
    let mut builder = AdmissionBuilder::new(entries.len());
    for pc in 0..entries.len() {
        if builder.exhausted() {
            break;
        }
        collect_admissions_at(&mut builder, code, entries, &cfg, pc, policy, &arena);
    }
    (builder.finish().map(Rc::new), arena)
}

impl BaselinePlan {
    #[cfg(test)]
    pub(crate) fn compile_for_test(
        code: CodeView<'_>,
        policy: crate::stencil_policy::ExecutionPolicy,
    ) -> Self {
        Self::compile(code, policy)
    }

    #[cfg(test)]
    pub(crate) fn native_storage_for_test(&self) -> (usize, usize, usize) {
        let arena = self.shared_region_arena.borrow();
        (arena.used(), arena.capacity(), arena.slab_count())
    }

    #[cfg(test)]
    pub(crate) fn shared_stencil_pool_for_test(&self) -> SharedStencilPool {
        Rc::clone(&self.shared_region_arena)
    }

    fn compile(code: CodeView<'_>, policy: crate::stencil_policy::ExecutionPolicy) -> Self {
        let entries = baseline_entries(code);
        let osr_entries = baseline_osr_entries(code);
        let (admission, shared_region_arena) = build_admissions(code, &entries, policy);
        Self {
            entries,
            osr_entries,
            admission,
            shared_region_arena,
        }
    }

    pub(crate) fn instruction(&self, pc: usize) -> Option<crate::ir::Instruction> {
        self.entries.get(pc).map(|entry| entry.instruction)
    }

    pub(crate) fn entry(&self, pc: usize) -> Option<BaselineEntry> {
        self.entries.get(pc).copied()
    }

    fn admissions_at(&self, pc: usize) -> &[NativeAdmission] {
        self.admission
            .as_deref()
            .map_or(&[], |storage| storage.entries_at(pc))
    }

    fn native_handle<T>(
        &self,
        pc: usize,
        select: impl Fn(&NativeAdmission) -> Option<&Rc<RefCell<T>>>,
    ) -> Option<&Rc<RefCell<T>>> {
        self.admissions_at(pc).iter().find_map(select)
    }

    typed_admission_accessors!(binary_handle_at, native_binary_at, Binary, NativeBinaryPlan);
    typed_admission_accessors!(
        load_const_handle_at,
        native_load_const_at,
        LoadConst,
        NativeLoadConstPlan
    );
    typed_admission_accessors!(
        local_binary_handle_at,
        native_local_binary_at,
        LocalBinary,
        crate::stencil_fusion::NativeLocalBinaryPlan
    );
    typed_admission_accessors!(
        local_property_handle_at,
        native_local_property_at,
        LocalProperty,
        crate::stencil_fusion::NativeLocalPropertyPlan
    );
    typed_admission_accessors!(
        truthiness_handle_at,
        native_truthiness_at,
        Truthiness,
        NativeTruthinessPlan
    );
    typed_admission_accessors!(
        nullish_handle_at,
        native_nullish_at,
        Nullish,
        NativeNullishPlan
    );
    typed_admission_accessors!(unary_handle_at, native_unary_at, Unary, NativeUnaryPlan);
    typed_admission_accessors!(
        add_chain_handle_at,
        native_add_chain_at,
        AddChain,
        NativeAddChainPlan
    );
    typed_admission_accessors!(move_handle_at, native_move_at, Move, NativeMovePlan);
    typed_admission_accessors!(
        load_local_handle_at,
        native_load_local_at,
        LoadLocal,
        NativeMovePlan
    );
    typed_admission_accessors!(
        store_local_handle_at,
        native_store_local_at,
        StoreLocal,
        NativeMovePlan
    );
    typed_admission_accessors!(
        store_property_handle_at,
        native_store_property_at,
        StoreProperty,
        NativePropertyPlan
    );
    typed_admission_accessors!(
        property_handle_at,
        native_property_at,
        Property,
        NativePropertyPlan
    );
    typed_admission_accessors!(
        dispatch_handle_at,
        native_dispatch_at,
        Dispatch,
        NativeDispatchPlan
    );
    typed_admission_accessors!(region_handle_at, native_region_at, Region, NativeRegionPlan);

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn is_osr_entry(&self, pc: usize) -> bool {
        self.osr_entries.binary_search(&(pc as u32)).is_ok()
    }
}

/// Rust-native optimizing dispatch plan. This is a physical execution view,
/// not a second semantic IR: every entry retains the canonical instruction,
/// handler, and control facts while caching already-admitted leaves. Any
/// unsupported operation still goes through the complete baseline handler.
#[derive(Clone)]
pub(crate) struct OptimizingPlan {
    entries: Rc<[BaselineEntry]>,
    admission: Option<Rc<AdmissionStorage<NativeAdmission>>>,
}

#[derive(Clone, Copy)]
pub(crate) struct OptimizingEntry<'a> {
    pub(crate) baseline: BaselineEntry,
    admissions: &'a [NativeAdmission],
}

impl OptimizingEntry<'_> {
    fn native_handle<T>(
        &self,
        select: impl Fn(&NativeAdmission) -> Option<&Rc<RefCell<T>>>,
    ) -> Option<&RefCell<T>> {
        self.admissions.iter().find_map(select).map(Rc::as_ref)
    }

    optimizing_admission_accessors!(native_binary, Binary, NativeBinaryPlan);
    optimizing_admission_accessors!(native_load_const, LoadConst, NativeLoadConstPlan);
    optimizing_admission_accessors!(native_truthiness, Truthiness, NativeTruthinessPlan);
    optimizing_admission_accessors!(native_nullish, Nullish, NativeNullishPlan);
    optimizing_admission_accessors!(native_unary, Unary, NativeUnaryPlan);
    optimizing_admission_accessors!(
        native_local_binary,
        LocalBinary,
        crate::stencil_fusion::NativeLocalBinaryPlan
    );
    optimizing_admission_accessors!(
        native_local_property,
        LocalProperty,
        crate::stencil_fusion::NativeLocalPropertyPlan
    );
    optimizing_admission_accessors!(native_move, Move, NativeMovePlan);
    optimizing_admission_accessors!(native_load_local, LoadLocal, NativeMovePlan);
    optimizing_admission_accessors!(native_store_local, StoreLocal, NativeMovePlan);
    optimizing_admission_accessors!(native_store_property, StoreProperty, NativePropertyPlan);
    optimizing_admission_accessors!(native_property, Property, NativePropertyPlan);
    optimizing_admission_accessors!(native_dispatch, Dispatch, NativeDispatchPlan);
    optimizing_admission_accessors!(native_region, Region, NativeRegionPlan);
}

impl std::fmt::Debug for OptimizingPlan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OptimizingPlan")
            .field("instructions", &self.entries.len())
            .finish()
    }
}

impl OptimizingPlan {
    #[cfg(test)]
    pub(crate) fn compile_for_test(
        baseline: &BaselinePlan,
        policy: crate::stencil_policy::ExecutionPolicy,
    ) -> Self {
        Self::compile(baseline, policy)
    }

    fn compile(baseline: &BaselinePlan, _policy: crate::stencil_policy::ExecutionPolicy) -> Self {
        Self {
            entries: Rc::clone(&baseline.entries),
            admission: baseline.admission.clone(),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn entry(&self, pc: usize) -> Option<OptimizingEntry<'_>> {
        Some(OptimizingEntry {
            baseline: *self.entries.get(pc)?,
            admissions: self
                .admission
                .as_deref()
                .map_or(&[], |storage| storage.entries_at(pc)),
        })
    }
}

/// OSR is an admission edge for a hot loop, not a generic branch/return hook.
/// The target is already a canonical compact operand, so the baseline plan can
/// classify back-edges without retaining another control-flow representation.
fn is_osr_candidate(pc: usize, instruction: crate::ir::Instruction) -> bool {
    match instruction.opcode.control_operands(instruction) {
        crate::ir::ControlOperands::Branch { target, .. }
        | crate::ir::ControlOperands::Jump { target } => usize::from(target) <= pc,
        // `ForI` is a structured-loop residual whose handler executes the
        // complete canonical loop gateway. It has no bytecode back-edge to
        // resume from, so treating it as an OSR entry would skip the loop by
        // transferring to `pc + 1` after compilation.
        crate::ir::ControlOperands::Loop { .. } => false,
        _ => false,
    }
}

#[derive(Debug)]
struct TierState {
    invocations: u32,
    retired: u64,
    osr_transfers: u64,
    threshold: u32,
    tier: ExecutionTier,
    plan: Option<Rc<BaselinePlan>>,
    optimizing: Option<Rc<OptimizingPlan>>,
}

impl TierState {
    fn new() -> Self {
        Self {
            invocations: 0,
            retired: 0,
            osr_transfers: 0,
            threshold: 32,
            tier: ExecutionTier::Interpreter,
            plan: None,
            optimizing: None,
        }
    }
}

impl<'a> CodeView<'a> {
    #[inline]
    pub(crate) fn range(self) -> CodeRange {
        self.range
    }

    #[inline]
    #[cfg(feature = "execution-trace")]
    pub(crate) fn trace_identity(self) -> (usize, u32) {
        (self.store as *const CodeStore as usize, self.range.code.0)
    }

    pub fn len(self) -> usize {
        self.range.end.saturating_sub(self.range.start) as usize
    }

    /// Number of register slots referenced by this lowered range. The count
    /// is derived once while freezing the immutable code store, so call entry
    /// does not scan instructions or size frames from bytecode length.
    pub fn register_count(self) -> u16 {
        self.store.register_count(self.range.code).unwrap_or(0)
    }

    /// Register width of the logical activation, including structured
    /// fragments that execute in this frame but excluding nested functions.
    pub fn frame_register_count(self) -> u16 {
        self.store
            .frame_register_count(self.range.code)
            .unwrap_or_else(|| self.register_count())
    }

    pub fn is_empty(self) -> bool {
        self.range.start == self.range.end
    }

    pub(crate) fn catch_at(self, pc: usize) -> Option<(usize, Option<u16>)> {
        let pc = u16::try_from(pc).ok()?;
        let ranges = self.store.catch_ranges.get(self.range.code.0 as usize)?;
        ranges.iter().rev().find_map(|range| {
            (pc >= range.start && pc < range.end)
                .then_some((usize::from(range.handler), range.catch_slot))
        })
    }

    pub fn parameter_end(self) -> Option<usize> {
        let (code_start, _) = self.store.ranges.get(self.range.code.0 as usize)?;
        let absolute = code_start.checked_add(
            self.store
                .parameter_ends
                .get(self.range.code.0 as usize)?
                .as_ref()
                .copied()?,
        )?;
        (absolute >= self.range.start && absolute <= self.range.end)
            .then(|| absolute.saturating_sub(self.range.start) as usize)
    }

    #[inline]
    pub fn instruction(self, pc: usize) -> Option<crate::ir::Instruction> {
        if pc >= self.len() {
            return None;
        }
        let mut instruction = self.store.instructions[(self.range.start as usize) + pc];
        if let Some(metadata) = self.metadata_at(pc) {
            if let Some(opcode) = metadata.quickened_opcode.get() {
                instruction.opcode = opcode;
            }
        }
        Some(instruction)
    }

    /// Rewrite one bounded instruction view after a confirmed generic-IC hit.
    /// The canonical bytes and complete fallback remain intact; a guard
    /// mismatch can clear this cell and expose the original opcode again.
    pub(crate) fn quicken_instruction(
        self,
        pc: usize,
        opcode: crate::ir::Opcode,
        shape: u32,
        property: u32,
        slot: u32,
    ) {
        if let Some(metadata) = self.metadata_at(pc) {
            metadata.quickened_shape.set(shape);
            metadata.quickened_property.set(property);
            metadata.quickened_slot.set(slot);
            metadata.quickened_opcode.set(Some(opcode));
        }
    }

    pub(crate) fn quickened_state(self, pc: usize) -> Option<(crate::ir::Opcode, u32, u32, u32)> {
        let metadata = self.metadata_at(pc)?;
        Some((
            metadata.quickened_opcode.get()?,
            metadata.quickened_shape.get(),
            metadata.quickened_property.get(),
            metadata.quickened_slot.get(),
        ))
    }

    pub(crate) fn dequicken_instruction(self, pc: usize) {
        if let Some(metadata) = self.metadata_at(pc) {
            metadata.quickened_opcode.set(None);
            metadata.quickened_shape.set(u32::MAX);
            metadata.quickened_property.set(u32::MAX);
            metadata.quickened_slot.set(u32::MAX);
        }
    }

    #[inline]
    pub fn cold(self, instruction: crate::ir::Instruction) -> Option<&'a Op> {
        let index = instruction.cold_index()? as usize;
        self.store.cold.get(index)
    }

    pub fn cold_at(self, pc: usize) -> Option<&'a Op> {
        self.instruction(pc)
            .and_then(|instruction| self.cold(instruction))
    }

    #[inline]
    pub fn constant_at(self, pc: usize) -> Option<(u16, &'a Constant)> {
        let instruction = self.instruction(pc)?;
        if instruction.opcode != crate::ir::Opcode::LoadConst {
            return None;
        }
        self.store
            .constants
            .get(self.range.code.0 as usize)?
            .get(instruction.b)
            .map(|value| (instruction.a, value))
    }

    /// Read a constant from this code range's canonical pool.
    #[inline]
    pub fn constant(self, id: u16) -> Option<&'a Constant> {
        self.store
            .constants
            .get(self.range.code.0 as usize)
            .and_then(|pool| pool.get(id))
    }

    #[inline]
    pub fn binary_at(self, pc: usize) -> Option<(u16, crate::ops::BinaryOp, u16, u16)> {
        let instruction = self.instruction(pc)?;
        let operator = match instruction.opcode {
            crate::ir::Opcode::Add => crate::ops::BinaryOp::Add,
            crate::ir::Opcode::Sub => crate::ops::BinaryOp::Subtract,
            crate::ir::Opcode::Mul => crate::ops::BinaryOp::Multiply,
            crate::ir::Opcode::Div => crate::ops::BinaryOp::Divide,
            crate::ir::Opcode::Binary => crate::ir::compact_binary_operator(instruction.flags)?,
            _ => return None,
        };
        Some((instruction.a, operator, instruction.b, instruction.c))
    }

    #[inline]
    pub fn metadata_at(self, pc: usize) -> Option<&'a InstructionMeta> {
        let offset = (self.range.start as usize).checked_add(pc)?;
        let range_start = self.store.ranges.get(self.range.code.0 as usize)?.0 as usize;
        self.store
            .metadata
            .get(self.range.code.0 as usize)?
            .get(offset.checked_sub(range_start)?)
    }

    /// Access the generated quickening state for one guarded instruction.
    #[inline]
    pub(crate) fn quickening_site(
        self,
        pc: usize,
    ) -> Option<&'a std::cell::RefCell<crate::quickening::QuickeningSite<4>>> {
        let metadata = self.metadata_at(pc)?;
        (metadata.quickening_site != u32::MAX).then_some(())?;
        self.store
            .quickening_sites
            .get(self.range.code.0 as usize)?
            .get(metadata.quickening_site as usize)
    }

    #[inline]
    pub fn operand_window_at(self, pc: usize) -> Option<&'a [u16]> {
        let meta = self.metadata_at(pc)?;
        self.store
            .operand_windows
            .get(self.range.code.0 as usize)?
            .get(meta.operand_window as usize)
            .map(AsRef::as_ref)
    }

    pub fn slice(self, start: usize, end: usize) -> Option<Self> {
        (start <= end && end <= self.len()).then(|| Self {
            store: self.store,
            range: CodeRange {
                code: self.range.code,
                start: self.range.start + start as u32,
                end: self.range.start + end as u32,
            },
        })
    }

    pub fn position_cold(self, mut predicate: impl FnMut(&Op) -> bool) -> Option<usize> {
        (0..self.len()).find(|&pc| self.cold_at(pc).is_some_and(&mut predicate))
    }

    pub fn find_cold(self, mut predicate: impl FnMut(&Op) -> bool) -> Option<(usize, &'a Op)> {
        (0..self.len()).find_map(|pc| {
            let op = self.cold_at(pc)?;
            predicate(op).then_some((pc, op))
        })
    }

    pub fn cold_ops(self) -> impl Iterator<Item = (usize, &'a Op)> + 'a {
        (0..self.len()).filter_map(move |pc| self.cold_at(pc).map(|op| (pc, op)))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExecutableCode {
    store: Rc<CodeStore>,
    entry: CodeRange,
}

impl ExecutableCode {
    pub fn from_ops(body: Vec<Op>) -> Self {
        let (store, entry, _) = freeze_tree(body);
        Self { store, entry }
    }

    pub fn store(&self) -> Rc<CodeStore> {
        self.store.clone()
    }

    pub fn entry(&self) -> CodeRange {
        self.entry
    }

    pub fn code(&self) -> CodeView<'_> {
        self.store.code(self.entry).expect("executable range")
    }
}

#[derive(Debug)]
enum CodeStoreLink {
    Strong(Rc<OnceLock<Rc<CodeStore>>>),
    Deferred {
        weak: Rc<OnceLock<std::rc::Weak<CodeStore>>>,
        promoted: OnceLock<Rc<CodeStore>>,
    },
}

impl CodeStoreLink {
    fn resolve_ref(&self) -> Option<&CodeStore> {
        match self {
            Self::Strong(link) => link.get().map(Rc::as_ref),
            Self::Deferred { weak, promoted } => {
                if promoted.get().is_none() {
                    let _ = promoted.set(weak.get()?.upgrade()?);
                }
                promoted.get().map(Rc::as_ref)
            }
        }
    }

    fn resolve(&self) -> Option<Rc<CodeStore>> {
        match self {
            Self::Strong(link) => link.get().cloned(),
            Self::Deferred { weak, promoted } => promoted
                .get()
                .cloned()
                .or_else(|| weak.get().and_then(std::rc::Weak::upgrade)),
        }
    }

    fn promoted(&self) -> Self {
        match self.resolve() {
            Some(store) => {
                let link = Rc::new(OnceLock::new());
                let _ = link.set(store);
                Self::Strong(link)
            }
            None => self.clone(),
        }
    }

    fn same_store(&self, other: &Self) -> bool {
        self.resolve()
            .zip(other.resolve())
            .is_some_and(|(left, right)| Rc::ptr_eq(&left, &right))
    }

    fn detach(&mut self, store: &Rc<OnceLock<std::rc::Weak<CodeStore>>>) {
        *self = Self::Deferred {
            weak: store.clone(),
            promoted: OnceLock::new(),
        };
    }
}

impl Clone for CodeStoreLink {
    fn clone(&self) -> Self {
        match self {
            Self::Strong(link) => Self::Strong(link.clone()),
            Self::Deferred { weak, promoted } => {
                let next = OnceLock::new();
                if let Some(store) = promoted.get() {
                    let _ = next.set(store.clone());
                }
                Self::Deferred {
                    weak: weak.clone(),
                    promoted: next,
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct FunctionCode {
    store: CodeStoreLink,
    pub range: CodeRange,
    source: Option<Rc<[Op]>>,
    capture_slots: Rc<[u16]>,
    facts: Rc<crate::facts::FunctionFacts>,
    tier: Rc<RefCell<TierState>>,
}

impl Clone for FunctionCode {
    fn clone(&self) -> Self {
        Self {
            store: self.store.promoted(),
            range: self.range,
            source: self.source.clone(),
            capture_slots: self.capture_slots.clone(),
            facts: self.facts.clone(),
            tier: self.tier.clone(),
        }
    }
}

impl FunctionCode {
    pub fn from_ops(body: Vec<Op>) -> Self {
        let capture_slots = collect_capture_slots(&body);
        let (_, range, store) = freeze_tree(body);
        Self {
            store: CodeStoreLink::Strong(store),
            range,
            source: None,
            capture_slots,
            facts: Rc::default(),
            tier: Rc::new(RefCell::new(TierState::new())),
        }
    }

    pub fn pending(body: Vec<Op>) -> Self {
        let capture_slots = collect_capture_slots(&body);
        Self {
            store: CodeStoreLink::Strong(Rc::new(OnceLock::new())),
            range: CodeRange {
                code: CodeId(0),
                start: 0,
                end: body.len() as u32,
            },
            source: Some(body.into_boxed_slice().into()),
            capture_slots,
            facts: Rc::default(),
            tier: Rc::new(RefCell::new(TierState::new())),
        }
    }

    pub fn pending_many(bodies: Vec<Vec<Op>>) -> Vec<Self> {
        bodies.into_iter().map(Self::pending).collect()
    }

    /// Materialize related nested bodies in one immutable store.
    pub fn from_ops_many(bodies: Vec<Vec<Op>>) -> Vec<Self> {
        let capture_slots = bodies
            .iter()
            .map(|body| collect_capture_slots(body))
            .collect::<Vec<_>>();
        let mut arena = CodeArena::new();
        let store = Rc::new(OnceLock::new());
        let ranges = bodies
            .into_iter()
            .map(|body| arena.append_tree(body, &store))
            .collect::<Vec<_>>();
        let _ = store.set(arena.freeze());
        ranges
            .into_iter()
            .zip(capture_slots)
            .map(|(range, capture_slots)| Self {
                store: CodeStoreLink::Strong(store.clone()),
                range,
                source: None,
                capture_slots,
                facts: Rc::default(),
                tier: Rc::new(RefCell::new(TierState::new())),
            })
            .collect()
    }

    pub fn new(store: Rc<CodeStore>, range: CodeRange) -> Self {
        let linked = Rc::new(OnceLock::new());
        let _ = linked.set(store);
        Self {
            store: CodeStoreLink::Strong(linked),
            range,
            source: None,
            capture_slots: Rc::from([u16::MAX]),
            facts: Rc::default(),
            tier: Rc::new(RefCell::new(TierState::new())),
        }
    }

    pub(crate) fn source_ops(&self) -> Option<&[Op]> {
        self.source.as_deref()
    }

    pub(crate) fn with_facts(mut self, facts: crate::facts::FunctionFacts) -> Self {
        self.facts = Rc::new(facts);
        self
    }

    pub(crate) fn facts(&self) -> &crate::facts::FunctionFacts {
        &self.facts
    }

    /// Account one function entry and compile the baseline plan when prior
    /// execution has crossed the bytecode-retirement threshold.  The paper's
    /// profiler measures executed bytecodes rather than call frequency, so a
    /// function is not promoted merely because it was invoked repeatedly.
    /// Compilation is disposable metadata; the canonical CodeView and handler
    /// remain the only semantic source of truth.
    pub(crate) fn enter_invocation(&self) -> TierTransition {
        let mut state = self.tier.borrow_mut();
        state.invocations = state.invocations.saturating_add(1);
        if state.tier == ExecutionTier::Optimizing {
            return TierTransition::Optimizing;
        }
        if state.tier == ExecutionTier::Baseline {
            // Admit optimization only after a bounded warmup beyond baseline.
            // The existing profile is the sole admission source; the plan is
            // disposable metadata over canonical instructions.
            let optimization_threshold = state
                .threshold
                .saturating_mul(OPTIMIZATION_WARMUP_MULTIPLIER)
                .max(1);
            if state.invocations < optimization_threshold {
                return TierTransition::Baseline;
            }
            let Some(plan) = state.plan.as_ref() else {
                return TierTransition::Baseline;
            };
            state.optimizing = Some(Rc::new(OptimizingPlan::compile(
                plan,
                crate::stencil_policy::current(),
            )));
            state.tier = ExecutionTier::Optimizing;
            return TierTransition::CompileOptimizing;
        }
        if state.retired < u64::from(state.threshold) {
            return TierTransition::Cold;
        }
        let Some(code) = self.code() else {
            return TierTransition::Cold;
        };
        state.plan = Some(Rc::new(BaselinePlan::compile(
            code,
            crate::stencil_policy::current(),
        )));
        state.tier = ExecutionTier::Baseline;
        TierTransition::CompileBaseline
    }

    pub(crate) fn tier(&self) -> ExecutionTier {
        self.tier.borrow().tier
    }

    pub(crate) fn baseline_plan(&self) -> Option<Rc<BaselinePlan>> {
        self.tier.borrow().plan.clone()
    }

    pub(crate) fn optimizing_plan(&self) -> Option<Rc<OptimizingPlan>> {
        self.tier.borrow().optimizing.clone()
    }

    /// The optimizing view follows the host's derived execution policy. ARM
    /// remains gated until a composed native region replaces the per-op
    /// bridge; scalar ARM leaves are independently opt-in.
    pub(crate) fn executable_optimizing_plan(&self) -> Option<Rc<OptimizingPlan>> {
        crate::stencil_policy::current()
            .optimizing_view
            .then(|| self.optimizing_plan())
            .flatten()
    }

    pub fn tier_profile(&self) -> TierProfile {
        let state = self.tier.borrow();
        TierProfile {
            tier: state.tier,
            invocations: state.invocations,
            retired: state.retired,
            baseline_instructions: state.plan.as_ref().map_or(0, |plan| plan.len()),
            optimizing_instructions: state.optimizing.as_ref().map_or(0, |plan| plan.len()),
            osr_entries: state.plan.as_ref().map_or(0, |plan| plan.osr_entries.len()),
            osr_transfers: state.osr_transfers,
        }
    }

    pub(crate) fn retire(&self, count: u64) {
        let mut state = self.tier.borrow_mut();
        state.retired = state.retired.saturating_add(count);
        if state.tier != ExecutionTier::Baseline
            || state.optimizing.is_some()
            || state.retired
                < u64::from(
                    state
                        .threshold
                        .saturating_mul(OPTIMIZATION_WARMUP_MULTIPLIER)
                        .max(1),
                )
        {
            return;
        }
        let Some(plan) = state.plan.clone() else {
            return;
        };
        state.optimizing = Some(Rc::new(OptimizingPlan::compile(
            &plan,
            crate::stencil_policy::current(),
        )));
        state.tier = ExecutionTier::Optimizing;
    }

    /// Retire one interpreter operation and compile at a hot back-edge. This
    /// is the OSR admission edge: it only installs a plan, while the next
    /// dispatch transfers to the same body with the current registers intact.
    pub(crate) fn retire_at(&self, pc: usize) -> TierTransition {
        let should_compile = {
            let mut state = self.tier.borrow_mut();
            state.retired = state.retired.saturating_add(1);
            state.tier == ExecutionTier::Interpreter
                && state.retired >= u64::from(state.threshold)
                && self
                    .code()
                    .and_then(|code| code.instruction(pc))
                    .is_some_and(|instruction| is_osr_candidate(pc, instruction))
        };
        if !should_compile {
            return if self.tier() == ExecutionTier::Baseline {
                TierTransition::Baseline
            } else {
                TierTransition::Cold
            };
        }
        let Some(code) = self.code() else {
            return TierTransition::Cold;
        };
        let mut state = self.tier.borrow_mut();
        if state.tier == ExecutionTier::Interpreter {
            state.plan = Some(Rc::new(BaselinePlan::compile(
                code,
                crate::stencil_policy::current(),
            )));
            state.tier = ExecutionTier::Baseline;
            TierTransition::CompileBaseline
        } else {
            TierTransition::Baseline
        }
    }

    pub(crate) fn is_osr_entry(&self, pc: usize) -> bool {
        self.baseline_plan()
            .is_some_and(|plan| plan.is_osr_entry(pc))
    }

    pub(crate) fn record_osr_transfer(&self) {
        let mut state = self.tier.borrow_mut();
        state.osr_transfers = state.osr_transfers.saturating_add(1);
    }

    #[cfg(test)]
    pub(crate) fn tier_counts(&self) -> (u32, u64) {
        let state = self.tier.borrow();
        (state.invocations, state.retired)
    }

    #[cfg(test)]
    pub(crate) fn set_tier_threshold_for_test(&self, threshold: u32) {
        self.tier.borrow_mut().threshold = threshold.max(1);
    }

    pub fn code_id(&self) -> CodeId {
        self.range.code
    }

    pub fn len(&self) -> usize {
        self.range.end.saturating_sub(self.range.start) as usize
    }

    /// Return the immutable width of this logical activation. Structured
    /// fragments share it; nested function literals own independent frames.
    pub(crate) fn required_register_count(&self) -> u16 {
        self.code().map(CodeView::frame_register_count).unwrap_or(0)
    }

    pub(crate) fn code(&self) -> Option<CodeView<'_>> {
        self.store.resolve_ref()?.code(self.range)
    }

    pub(crate) fn store(&self) -> Option<Rc<CodeStore>> {
        self.store.resolve()
    }

    pub(crate) fn capture_slots(&self) -> &[u16] {
        &self.capture_slots
    }

    pub(crate) fn uses_slot(&self, slot: u16) -> bool {
        self.capture_slots.binary_search(&u16::MAX).is_ok()
            || self.capture_slots.binary_search(&slot).is_ok()
    }

    pub(crate) fn rehome(&mut self, arena: &mut CodeArena, store: &Rc<OnceLock<Rc<CodeStore>>>) {
        // Link the canonical source before both flattening and range emission.
        // Keeping the pre-link clone here leaves nested functions with their
        // original OnceLock and lets structured branch encoding observe an
        // owner-less body after a conditional has been flattened.
        let Some(body) = self.source.take() else {
            return;
        };
        let mut body = body.to_vec();
        for op in &mut body {
            op.rehome_bodies(arena, store);
        }
        self.source = Some(body.clone().into_boxed_slice().into());
        self.range = arena.append(body);
        self.store = CodeStoreLink::Strong(store.clone());
    }

    pub(crate) fn rehome_contents(
        &mut self,
        arena: &mut CodeArena,
        store: &Rc<OnceLock<Rc<CodeStore>>>,
    ) {
        let Some(body) = self.source.take() else {
            return;
        };
        let mut body = body.to_vec();
        for op in &mut body {
            op.rehome_bodies(arena, store);
        }
        self.source = Some(body.into_boxed_slice().into());
        self.store = CodeStoreLink::Strong(store.clone());
    }

    pub(crate) fn detach_internal_store(&mut self, store: &Rc<OnceLock<std::rc::Weak<CodeStore>>>) {
        self.store.detach(store);
        if let Some(source) = &mut self.source {
            for op in Rc::make_mut(source) {
                op.detach_store_links(store);
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn has_internal_store_link(&self) -> bool {
        matches!(self.store, CodeStoreLink::Deferred { .. })
    }
}

pub(crate) fn test_always_true(ops: &[Op]) -> bool {
    let mut saw_true = false;
    for op in ops {
        match op {
            Op::Const {
                value: crate::ops::Constant::Boolean(true),
                ..
            } => saw_true = true,
            Op::Const {
                value: crate::ops::Constant::Number(value),
                ..
            } if *value != 0.0 && !value.is_nan() => saw_true = true,
            Op::Return { .. } | Op::Move { .. } => {}
            _ => return false,
        }
    }
    saw_true
}

pub(crate) fn ops_use_arguments(ops: &[Op]) -> bool {
    ops.iter().any(|op| match op {
        Op::ResolveName { key, .. } | Op::SetName { key, .. } if key == "arguments" => true,
        Op::LoadBinding { name, .. } if name == "arguments" => true,
        _ => false,
    })
}

pub(crate) fn ops_contain_short_circuit(ops: &[Op]) -> bool {
    ops.iter().any(|op| match op {
        Op::Conditional { .. } | Op::Branch { .. } => true,
        Op::Loop { test, .. } => test.source_ops().is_some_and(ops_contain_short_circuit),
        _ => false,
    })
}

pub(crate) fn ops_contain_call(ops: &[Op]) -> bool {
    ops.iter().any(|op| match op {
        Op::Call { .. } | Op::CallMethod { .. } | Op::Construct { .. } => true,
        Op::Conditional {
            consequent,
            alternate,
            ..
        }
        | Op::Branch {
            then_ops: consequent,
            else_ops: alternate,
            ..
        } => {
            consequent.source_ops().is_some_and(ops_contain_call)
                || alternate.source_ops().is_some_and(ops_contain_call)
        }
        Op::Loop {
            init,
            test,
            body,
            update,
            ..
        } => [init, test, body, update]
            .into_iter()
            .any(|part| part.source_ops().is_some_and(ops_contain_call)),
        Op::Try {
            body,
            handler,
            finalizer,
            ..
        } => {
            body.source_ops().is_some_and(ops_contain_call)
                || handler
                    .as_ref()
                    .and_then(FunctionCode::source_ops)
                    .is_some_and(ops_contain_call)
                || finalizer
                    .as_ref()
                    .and_then(FunctionCode::source_ops)
                    .is_some_and(ops_contain_call)
        }
        _ => false,
    })
}

fn collect_op_constants(ops: &[Op], out: &mut Vec<crate::ops::Constant>) {
    for op in ops {
        match op {
            Op::Const { value, .. } => out.push(value.clone()),
            Op::Conditional {
                consequent,
                alternate,
                ..
            }
            | Op::Branch {
                then_ops: consequent,
                else_ops: alternate,
                ..
            } => {
                if let Some(body) = consequent.source_ops() {
                    collect_op_constants(body, out);
                }
                if let Some(body) = alternate.source_ops() {
                    collect_op_constants(body, out);
                }
            }
            Op::Loop {
                init,
                test,
                body,
                update,
                ..
            } => {
                for part in [init, test, body, update] {
                    if let Some(body) = part.source_ops() {
                        collect_op_constants(body, out);
                    }
                }
            }
            Op::Try {
                body,
                handler,
                finalizer,
                ..
            } => {
                if let Some(body) = body.source_ops() {
                    collect_op_constants(body, out);
                }
                if let Some(handler) = handler.as_ref().and_then(FunctionCode::source_ops) {
                    collect_op_constants(handler, out);
                }
                if let Some(finalizer) = finalizer.as_ref().and_then(FunctionCode::source_ops) {
                    collect_op_constants(finalizer, out);
                }
            }
            _ => {}
        }
    }
}

fn collect_capture_slots(body: &[Op]) -> Rc<[u16]> {
    let mut slots = Vec::new();
    for op in body {
        op.capture_slots(&mut slots);
    }
    slots.sort_unstable();
    slots.dedup();
    slots.into()
}

fn freeze_tree(body: Vec<Op>) -> (Rc<CodeStore>, CodeRange, Rc<OnceLock<Rc<CodeStore>>>) {
    let mut arena = CodeArena::new();
    let linked = Rc::new(OnceLock::new());
    let range = arena.append_tree(body, &linked);
    let store = arena.freeze();
    let _ = linked.set(store.clone());
    (store, range, linked)
}

impl PartialEq for FunctionCode {
    fn eq(&self, other: &Self) -> bool {
        self.range == other.range
            && self.source == other.source
            && self.facts == other.facts
            && (self.source.is_some() || self.store.same_store(&other.store))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum IteratorPhase {
    Fetch,
    Bind,
    Body,
    Continue,
    Close,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BranchPhase {
    Body,
    Resume,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TryPhase {
    Body,
    Catch,
    Finally,
    Resume,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PrivatePhase {
    Body,
    Resume,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    Try {
        phase: TryPhase,
        body: CodeRange,
        handler: Option<CodeRange>,
        finalizer: Option<CodeRange>,
        body_resume: CodeRange,
        resume: CodeRange,
        yield_dst: u16,
        catch_slot: Option<u16>,
    },
    Iterator {
        phase: IteratorPhase,
        iterator: Value,
        binding: u16,
        body: CodeRange,
        body_resume: CodeRange,
        resume: CodeRange,
        yield_dst: u16,
        close_normal: bool,
        repeat: bool,
        slot: u16,
    },
    Await {
        phase: u8,
        resume: CodeRange,
        destination: u16,
    },
    Delegate {
        phase: u8,
        iterator: Value,
        destination: u16,
    },
    Dispose {
        body_resume: CodeRange,
        resume: CodeRange,
        stack: u16,
        await_using: bool,
        yield_dst: u16,
    },
    Branch {
        phase: BranchPhase,
        branch_resume: CodeRange,
        resume: CodeRange,
        dst: Option<u16>,
        yield_dst: u16,
    },
    Private {
        phase: PrivatePhase,
        environment: crate::private_environment::PrivateEnvironment,
        body_resume: CodeRange,
        resume: CodeRange,
        yield_dst: u16,
    },
    Loop {
        label: Option<String>,
        body: CodeRange,
        test: CodeRange,
        update: CodeRange,
        phase: crate::continuation::LoopPhase,
        phase_resume: CodeRange,
        resume: CodeRange,
        dst: u16,
        yield_dst: u16,
        post_test: bool,
        per_iteration: Rc<[u16]>,
    },
}

include!("frame_resume.rs");

#[derive(Debug, Clone, PartialEq)]
pub struct Machine {
    pub(crate) store: Option<Rc<CodeStore>>,
    pub(crate) code: CodeId,
    pub(crate) pc: u32,
    pub(crate) registers: RegisterWindow,
    pub(crate) environment: EnvironmentRef,
    environment_data: Option<Rc<crate::environment::Environment>>,
    pub(crate) completion: Completion,
    pub(crate) frames: FrameStack,
    /// Suspended callers waiting for a non-tail call to complete.
    pub(crate) call_frames: Vec<crate::completion::CallContinuation>,
}

impl Machine {
    pub fn new(code: CodeId, environment: EnvironmentRef) -> Self {
        Self {
            store: None,
            code,
            pc: 0,
            registers: RegisterWindow::new(),
            environment,
            environment_data: None,
            completion: Completion::Normal,
            frames: FrameStack::new(),
            call_frames: Vec::new(),
        }
    }
    pub fn with_function(
        function: &FunctionCode,
        environment: EnvironmentRef,
        register_count: u16,
    ) -> Self {
        let mut machine =
            Self::with_register_count(function.code_id(), environment, register_count);
        machine.store = function.store();
        machine
    }

    pub fn with_register_count(
        code: CodeId,
        environment: EnvironmentRef,
        register_count: u16,
    ) -> Self {
        let mut machine = Self::new(code, environment);
        machine.registers = RegisterWindow::with_count(register_count);
        machine
    }

    pub fn step<F, E>(&mut self, input: Completion, execute: F) -> Result<Completion, E>
    where
        F: FnOnce(&mut crate::register_file::RegisterFile) -> Result<Completion, E>,
    {
        self.completion = input;
        let completion = execute(&mut self.registers.values)?;
        self.completion = completion.clone();
        Ok(completion)
    }
    /// Drive dispatch transitions without growing the Rust call stack.
    ///
    /// The callback performs one resumable VM slice and returns the next
    /// completion.  Normal completion terminates; tail calls are fed back
    /// into the same machine so callers do not recursively re-enter Rust.
    pub fn run_until_complete<F, E>(
        &mut self,
        mut input: Completion,
        mut execute: F,
    ) -> Result<Completion, E>
    where
        F: FnMut(&mut crate::register_file::RegisterFile, Completion) -> Result<Completion, E>,
    {
        loop {
            self.completion = input.clone();
            let completion = execute(&mut self.registers.values, input)?;
            self.completion = completion.clone();
            if !matches!(completion, Completion::TailCall(_)) {
                return Ok(completion);
            }
            input = completion;
        }
    }

    pub fn record_completion(&mut self, completion: Completion) {
        self.completion = completion;
    }

    pub fn try_push_frame(&mut self, frame: Frame) -> Result<(), Frame> {
        let valid = self
            .store
            .as_ref()
            .map(|store| {
                frame
                    .ranges()
                    .into_iter()
                    .all(|range| store.code(range).is_some())
            })
            .unwrap_or(false);
        if !valid {
            return Err(frame);
        }
        self.frames.try_push(frame)
    }

    pub fn pop_frame(&mut self) -> Option<Frame> {
        self.frames.pop()
    }

    pub fn pop_await_frame(&mut self) -> bool {
        let Some(offset) = self.frames.top_offset() else {
            return false;
        };
        if !matches!(self.frames.frame_at(offset), Some(Frame::Await { .. })) {
            return false;
        }
        self.frames.pop();
        true
    }

    /// Capture the active caller state at an ordinary call boundary.
    ///
    /// The register window is moved (rather than cloned) so a callee can use
    /// the machine's storage directly.  The immutable caller code is retained
    /// by the continuation for the dispatch loop to resume.
    pub(crate) fn suspend_call(
        &mut self,
        callee: Value,
        receiver: Value,
        arguments: Vec<Value>,
        destination: u16,
        guards: crate::completion::ContinuationGuards,
    ) {
        let continuation = crate::completion::CallContinuation {
            callee,
            receiver,
            arguments: arguments.into(),
            caller_code: self.code,
            caller_pc: self.pc,
            caller_registers: self.take_registers(),
            caller_environment: self.environment,
            destination,
            guards,
        };
        self.push_call_frame(continuation);
        self.environment_data = None;
    }

    /// Restore the most recently suspended caller and deliver its result.
    ///
    /// A continuation is only executable when its return address belongs to
    /// this machine's immutable code store. Keep that check at the resume
    /// boundary so stale continuations cannot turn an integer into an
    /// instruction pointer.
    pub(crate) fn resume_call(
        &mut self,
        value: Value,
    ) -> Option<crate::completion::CallContinuation> {
        let continuation = self.pop_call_frame()?;
        let valid_source = self
            .store
            .as_ref()
            .and_then(|store| store.range_len(continuation.caller_code))
            .is_some_and(|len| continuation.caller_pc < len);
        if !valid_source {
            return None;
        }
        self.restore_registers(continuation.caller_registers.clone());
        self.environment = continuation.caller_environment;
        self.code = continuation.caller_code;
        self.pc = continuation.caller_pc;
        self.environment_data = None;
        crate::execute::write_value(&mut self.registers.values, continuation.destination, value);
        Some(continuation)
    }
    /// Save a caller continuation while a non-tail call executes.
    #[inline]
    pub(crate) fn push_call_frame(&mut self, frame: crate::completion::CallContinuation) {
        self.call_frames.push(frame);
    }

    /// Resume the most recently suspended caller.
    #[inline]
    pub(crate) fn pop_call_frame(&mut self) -> Option<crate::completion::CallContinuation> {
        self.call_frames.pop()
    }

    pub fn frame_count(&self) -> u16 {
        self.frames.depth()
    }

    pub fn code_id(&self) -> CodeId {
        self.code
    }

    pub fn program_counter(&self) -> u32 {
        self.pc
    }

    pub fn register_count(&self) -> u16 {
        self.registers.count
    }

    pub(crate) fn install_environment(&mut self, environment: Rc<crate::environment::Environment>) {
        self.environment_data = Some(environment);
    }

    pub(crate) fn environment(&self) -> Option<Rc<crate::environment::Environment>> {
        self.environment_data.clone()
    }

    pub(crate) fn take_registers(&mut self) -> crate::register_file::RegisterFile {
        std::mem::take(&mut self.registers.values)
    }

    pub(crate) fn restore_registers(&mut self, registers: crate::register_file::RegisterFile) {
        let count = u16::try_from(registers.len()).expect("register window exceeds u16 capacity");
        self.registers.count = count;
        self.registers.values = registers;
    }

    pub fn iterator_phase(&self) -> Option<&IteratorPhase> {
        let offset = self.frames.top_offset()?;
        let Some(Frame::Iterator { phase, .. }) = self.frames.frame_at(offset) else {
            return None;
        };
        Some(phase)
    }
    /// The active frame is represented only by the top frame-stack offset.
    /// `None` is the canonical no-frame state; callers must not retain a
    /// frame reference across a push or pop.
    pub fn current_frame(&self) -> Option<&Frame> {
        self.frames
            .top_offset()
            .and_then(|offset| self.frames.frame_at(offset))
    }

    /// Constants are canonicalized from the immutable code store; no mutable
    /// per-machine copy exists.
    pub fn constants(&self) -> Option<ConstantPool> {
        self.store
            .as_ref()
            .map(|store| store.constant_pool(self.code_range()))
    }

    fn code_range(&self) -> CodeRange {
        CodeRange::new(
            self.code,
            0,
            self.store
                .as_ref()
                .and_then(|s| s.range_len(self.code))
                .unwrap_or(0),
        )
        .expect("machine code range must be ordered")
    }

    /// Checks the state invariants at VM transition boundaries.
    pub fn state_is_valid(&self) -> bool {
        self.registers.is_valid()
            && self.current_frame().is_some() == (self.frame_count() != 0)
            && self.pc
                <= self
                    .store
                    .as_ref()
                    .and_then(|s| s.range_len(self.code))
                    .unwrap_or(u32::MAX)
    }
    /// Borrow the canonical register storage without exposing the window
    /// bookkeeping.  The immutable view keeps readers on the same storage
    /// used by execution and avoids materializing a duplicate register list.
    #[inline]
    pub fn registers(&self) -> &crate::register_file::RegisterFile {
        &self.registers.values
    }

    /// Return the current completion/exception state owned by this machine.
    #[inline]
    pub fn completion(&self) -> &Completion {
        &self.completion
    }

    /// Borrow the contiguous register storage used by the active frame.
    #[inline]
    pub fn registers_mut(&mut self) -> &mut crate::register_file::RegisterFile {
        &mut self.registers.values
    }

    /// Update the direct program-counter field after a resumable slice.
    #[inline]
    pub fn set_program_counter(&mut self, pc: u32) {
        self.pc = pc;
    }

    pub(crate) fn set_iterator_phase(&mut self, next: IteratorPhase) -> bool {
        let Some(offset) = self.frames.top_offset() else {
            return false;
        };
        let Some(Frame::Iterator { phase, .. }) = self.frames.frame_at_mut(offset) else {
            return false;
        };
        *phase = next;
        true
    }

    pub(crate) fn advance_frame_resume(&mut self, resume: CodeRange, yield_dst: u16) -> bool {
        let Some(offset) = self.frames.top_offset() else {
            return false;
        };
        let Some(frame) = self.frames.frame_at_mut(offset) else {
            return false;
        };
        frame.advance_resume(resume, yield_dst)
    }

    pub(crate) fn set_try_finally_resume(&mut self, resume: CodeRange, yield_dst: u16) -> bool {
        let Some(offset) = self.frames.top_offset() else {
            return false;
        };
        let Some(frame) = self.frames.frame_at_mut(offset) else {
            return false;
        };
        frame.set_finally_resume(resume, yield_dst)
    }

    pub(crate) fn set_try_catch_resume(&mut self, resume: CodeRange, yield_dst: u16) -> bool {
        let Some(offset) = self.frames.top_offset() else {
            return false;
        };
        let Some(frame) = self.frames.frame_at_mut(offset) else {
            return false;
        };
        frame.set_catch_resume(resume, yield_dst)
    }
}

#[cfg(test)]
mod tests {
    use super::{EnvironmentRef, FrameStack, Machine, RegisterWindow};
    use crate::completion::{Completion, TailCallRequest};
    use crate::value::Value;
    use std::rc::Rc;

    #[test]
    fn code_ranges_validate_and_measure_offsets() {
        let range = super::CodeRange::new(super::CodeId(2), 3, 7).unwrap();
        assert_eq!(range.len(), 4);
        assert!(range.contains(3));
        assert!(range.contains(6));
        assert!(!range.contains(7));
        assert!(super::CodeRange::new(super::CodeId(2), 8, 7).is_none());
    }

    #[test]
    fn zero_argument_named_call_does_not_use_sentinel_as_register() {
        let zero = crate::ir::Instruction::call_named(1, 2, None);
        let one = crate::ir::Instruction::call_named(1, 2, Some(9));
        assert_eq!(super::register_count_for(&[zero], &[], &[]), 3);
        assert_eq!(super::register_count_for(&[one], &[], &[]), 10);
        assert_eq!(zero.register_flow().highest_register(), Some(2));
        assert_eq!(one.register_flow().highest_register(), Some(9));
    }

    #[test]
    fn frame_width_uses_operand_roles_not_constant_pool_words() {
        let add_const = crate::ir::Instruction::add_const(4, 1, u16::MAX);
        let load = crate::ir::Instruction::load_const(3, u16::MAX);
        let local = crate::ir::Instruction::load_local(3, u16::MAX);
        let checked = crate::ir::Instruction::load_local_checked(3, u16::MAX);
        let local_move = crate::ir::Instruction::move_local(3, u16::MAX, u16::MAX - 1);
        let call = crate::ir::Instruction::call_registered_window(12, 2, 7, 4);
        assert_eq!(super::register_count_for(&[add_const], &[], &[]), 5);
        assert_eq!(super::register_count_for(&[load], &[], &[]), 4);
        assert_eq!(super::register_count_for(&[local], &[], &[]), 4);
        assert_eq!(super::register_count_for(&[checked], &[], &[]), 4);
        assert_eq!(super::register_count_for(&[local_move], &[], &[]), 4);
        assert_eq!(super::register_count_for(&[call], &[], &[]), 13);
    }

    #[test]
    fn liveness_excludes_local_slot_identifiers() {
        let entries = [
            super::BaselineEntry {
                instruction: crate::ir::Instruction::load_local(3, u16::MAX),
                handler: crate::ir::Opcode::LoadLocal.handler(),
                control: crate::ir::ControlOperands::Next,
            },
            super::BaselineEntry {
                instruction: crate::ir::Instruction::ret(4),
                handler: crate::ir::Opcode::Return.handler(),
                control: crate::ir::ControlOperands::Return { source: 4 },
            },
        ];
        let cfg = super::ControlFlowFacts::new(&entries, &[None, None]);
        assert_eq!(cfg.live_out()[0], std::collections::BTreeSet::from([4]));
        assert!(!cfg.live_out()[0].contains(&u16::MAX));
    }

    #[test]
    fn value_window_keeps_best_candidate_at_code_boundary() {
        let function = super::FunctionCode::from_ops(vec![
            super::Op::LoadLocal { dst: 4, slot: 0 },
            super::Op::LoadLocal { dst: 7, slot: 1 },
            super::Op::Binary {
                dst: 1,
                operator: crate::ops::BinaryOp::Add,
                lhs: 4,
                rhs: 7,
            },
        ]);
        let code = function.code().expect("compact code");
        let entries = super::baseline_entries(code);
        let windows = vec![None; entries.len()];
        let cfg = super::ControlFlowFacts::new(&entries, &windows);
        let selected = super::select_local_numeric(code, &entries, &cfg, 0)
            .expect("earlier profitable candidate survives exhausted lookahead");
        assert_eq!(selected.span, 3);
    }

    #[test]
    fn out_of_line_call_arguments_size_and_remain_live_in_the_frame() {
        let mut arena = super::CodeArena::new();
        let range = arena.append_slice(&[
            super::Op::Move { dst: 0, src: 1 },
            super::Op::CallMethod {
                dst: 20,
                object: 2,
                key: "method".into(),
                callee: Some(4),
                args: vec![5, 7, 9, 11, 13, 27],
                spreads: vec![false; 6],
            },
        ]);
        let store = arena.freeze();
        let code = store.code(range).expect("compact code");
        let entries = super::baseline_entries(code);
        let windows = (0..entries.len())
            .map(|pc| code.operand_window_at(pc))
            .collect::<Vec<_>>();
        let cfg = super::ControlFlowFacts::new(&entries, &windows);
        assert_eq!(code.frame_register_count(), 28);
        assert!(cfg.live_out()[0].contains(&27));
    }

    #[test]
    fn interruptible_template_requires_a_physical_checkpoint() {
        let absent = [0u8; 12];
        assert!(!crate::stencil_physical::contains_interrupt_checkpoint(
            &absent
        ));
        #[cfg(target_arch = "aarch64")]
        {
            let words = [0xF940_1805u32, 0x3940_00A6, 0x3500_0006];
            let bytes: Vec<u8> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
            assert!(crate::stencil_physical::contains_interrupt_checkpoint(
                &bytes
            ));
        }
    }

    #[test]
    fn physical_templates_match_declared_abi_before_entry() {
        for record in crate::stencil_select::region_records() {
            if record.executable {
                let validation = super::validate_physical_template(record);
                assert!(
                    validation.is_ok(),
                    "generated ABI/template mismatch for {:?} abi={:?} ops={:?} holes={:?} error={:?}",
                    record.key,
                    record.abi,
                    record.operations,
                    record.stencil.holes,
                    validation.err()
                );
            }
        }
        static BYTES: [u8; 8] = [0; 8];
        static HOLES: [crate::stencil_fact::Hole; 1] = [crate::stencil_fact::Hole {
            offset: 0,
            kind: crate::stencil_fact::HoleKind::Ptr64,
        }];
        static OPS: [crate::ir::Opcode; 1] = [crate::ir::Opcode::Add];
        static ENTRIES: [u16; 1] = [0];
        let scalar_with_pointer = crate::stencil_select::RegionRecord {
            name: "test_scalar_pointer",
            key: crate::stencil_fact::RegionKey(0),
            stencil: crate::stencil_fact::Stencil {
                bytes: &BYTES,
                holes: &HOLES,
            },
            operations: &OPS,
            bindings: &[],
            outputs: &[],
            entry: 0,
            external_entries: &ENTRIES,
            fallthrough: None,
            abi: crate::stencil_select::RegionAbi::ScalarF64Binary,
            template_calls_helper: false,
            executable: true,
        };
        assert!(super::validate_physical_template(&scalar_with_pointer).is_err());
    }

    #[test]
    fn generated_template_call_effects_match_target_decoder() {
        for record in crate::stencil_select::region_records() {
            if crate::stencil_physical::contains_call(record.stencil.bytes) {
                assert!(
                    record.abi.contract().may_call_helper,
                    "generated direct call escaped ABI for {}",
                    record.name
                );
            }
        }
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn raw_template_rejects_undeclared_simd_clobber() {
        static BYTES: [u8; 4] = 0xFD40_0003u32.to_le_bytes(); // ldr d3, [x0]
        static OPS: [crate::ir::Opcode; 1] = [crate::ir::Opcode::AGetI];
        static ENTRIES: [u16; 1] = [0];
        let record = crate::stencil_select::RegionRecord {
            name: "test_raw_clobber",
            key: crate::stencil_fact::RegionKey(3),
            stencil: crate::stencil_fact::Stencil {
                bytes: &BYTES,
                holes: &[],
            },
            operations: &OPS,
            bindings: &[],
            outputs: &[],
            entry: 0,
            external_entries: &ENTRIES,
            fallthrough: None,
            abi: crate::stencil_select::RegionAbi::ArrayKernel,
            template_calls_helper: false,
            executable: true,
        };
        assert!(super::validate_physical_template(&record)
            .expect_err("d3 is outside the ArrayKernel scratch contract")
            .contains("undeclared"));
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn raw_template_rejects_undeclared_gpr_clobber() {
        static BYTES: [u8; 4] = 0xF940_0007u32.to_le_bytes(); // ldr x7, [x0]
        static OPS: [crate::ir::Opcode; 1] = [crate::ir::Opcode::AGetI];
        static ENTRIES: [u16; 1] = [0];
        let record = crate::stencil_select::RegionRecord {
            name: "test_raw_gpr_clobber",
            key: crate::stencil_fact::RegionKey(4),
            stencil: crate::stencil_fact::Stencil {
                bytes: &BYTES,
                holes: &[],
            },
            operations: &OPS,
            bindings: &[],
            outputs: &[],
            entry: 0,
            external_entries: &ENTRIES,
            fallthrough: None,
            abi: crate::stencil_select::RegionAbi::ArrayKernel,
            template_calls_helper: false,
            executable: true,
        };
        assert!(super::validate_physical_template(&record)
            .expect_err("x7 is outside the ArrayKernel GPR scratch contract")
            .contains("GPR clobber"));
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn raw_template_rejects_unknown_instruction_before_entry() {
        static BYTES: [u8; 4] = 0xFFFF_FFFFu32.to_le_bytes();
        static OPS: [crate::ir::Opcode; 1] = [crate::ir::Opcode::AGetI];
        static ENTRIES: [u16; 1] = [0];
        let record = crate::stencil_select::RegionRecord {
            name: "test_raw_unknown_instruction",
            key: crate::stencil_fact::RegionKey(6),
            stencil: crate::stencil_fact::Stencil {
                bytes: &BYTES,
                holes: &[],
            },
            operations: &OPS,
            bindings: &[],
            outputs: &[],
            entry: 0,
            external_entries: &ENTRIES,
            fallthrough: None,
            abi: crate::stencil_select::RegionAbi::ArrayKernel,
            template_calls_helper: false,
            executable: true,
        };
        assert!(super::validate_physical_template(&record)
            .expect_err("unknown instructions must fail closed")
            .contains("unknown instruction"));
    }

    #[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
    #[test]
    fn physical_helper_call_is_rejected_for_scalar_abi() {
        #[cfg(target_arch = "aarch64")]
        static CALL_BYTES: [u8; 4] = 0x9400_0000u32.to_le_bytes();
        #[cfg(target_arch = "x86_64")]
        static CALL_BYTES: [u8; 1] = [0xE8];
        static OPS: [crate::ir::Opcode; 1] = [crate::ir::Opcode::Add];
        static ENTRIES: [u16; 1] = [0];
        let scalar_call = crate::stencil_select::RegionRecord {
            name: "test_scalar_call",
            key: crate::stencil_fact::RegionKey(1),
            stencil: crate::stencil_fact::Stencil {
                bytes: &CALL_BYTES,
                holes: &[],
            },
            operations: &OPS,
            bindings: &[],
            outputs: &[],
            entry: 0,
            external_entries: &ENTRIES,
            fallthrough: None,
            abi: crate::stencil_select::RegionAbi::ScalarF64Binary,
            template_calls_helper: true,
            executable: true,
        };
        assert!(super::validate_physical_template(&scalar_call).is_err());
    }

    #[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
    #[test]
    fn physical_helper_call_requires_semantic_boundary_and_roots() {
        let bridge =
            crate::stencil_select::select_region(crate::stencil_select::dispatch_region_key())
                .expect("generated bridge declaration");
        static OPS: [crate::ir::Opcode; 1] = [crate::ir::Opcode::Move];
        static ENTRIES: [u16; 1] = [0];
        let pure_bridge = crate::stencil_select::RegionRecord {
            name: "test_pure_bridge",
            key: crate::stencil_fact::RegionKey(2),
            stencil: bridge.stencil,
            operations: &OPS,
            bindings: &[],
            outputs: &[],
            entry: 0,
            external_entries: &ENTRIES,
            fallthrough: None,
            abi: crate::stencil_select::RegionAbi::Bridge,
            template_calls_helper: true,
            executable: true,
        };
        assert!(
            super::validate_physical_template(&pure_bridge).is_err(),
            "a physical helper call must not be admitted for a pure operation"
        );
    }

    #[test]
    fn physical_effect_mismatch_rejects_before_entry() {
        static BYTES: [u8; 1] = [0xC3];
        static OPS: [crate::ir::Opcode; 1] = [crate::ir::Opcode::Move];
        static ENTRIES: [u16; 1] = [0];
        let record = crate::stencil_select::RegionRecord {
            name: "test_effect_mismatch",
            key: crate::stencil_fact::RegionKey(5),
            stencil: crate::stencil_fact::Stencil {
                bytes: &BYTES,
                holes: &[],
            },
            operations: &OPS,
            bindings: &[],
            outputs: &[],
            entry: 0,
            external_entries: &ENTRIES,
            fallthrough: None,
            abi: crate::stencil_select::RegionAbi::Bridge,
            template_calls_helper: true,
            executable: true,
        };
        let error = super::validate_physical_template(&record)
            .expect_err("a drifted physical effect must fail closed");
        assert!(error.contains("declared helper boundary"));
    }

    #[test]
    fn raw_region_admission_requires_every_live_definition_output() {
        static OPS: [crate::ir::Opcode; 1] = [crate::ir::Opcode::Move];
        static OUTPUTS: [crate::stencil_select::PhysicalOutput; 1] =
            [crate::stencil_select::PhysicalOutput {
                value: crate::stencil_select::PhysicalOutputValue::Result,
                destination: crate::stencil_select::PhysicalOutputDestination::Register(
                    crate::stencil_select::PhysicalOperand {
                        operation: 0,
                        field: crate::stencil_select::PhysicalOperandField::A,
                    },
                ),
            }];
        let entry = |instruction: crate::ir::Instruction| super::BaselineEntry {
            instruction,
            handler: instruction.opcode.handler(),
            control: instruction.opcode.control_operands(instruction),
        };
        let entries = [
            entry(crate::ir::Instruction::move_(2, 1)),
            entry(crate::ir::Instruction::ret(2)),
        ];
        let cfg = crate::stencil_cfg::ControlFlowFacts::new(&entries, &[None, None]);
        let mut record = crate::stencil_select::RegionRecord {
            name: "live_output_test",
            key: crate::stencil_fact::RegionKey(23),
            stencil: crate::stencil_fact::Stencil {
                bytes: &[],
                holes: &[],
            },
            operations: &OPS,
            bindings: &[],
            outputs: &OUTPUTS,
            entry: 0,
            external_entries: &[0],
            fallthrough: None,
            abi: crate::stencil_select::RegionAbi::ArrayNumericLoop,
            template_calls_helper: false,
            executable: true,
        };
        assert!(super::region_admission_matches(&entries, &cfg, 0, &record));
        record.outputs = &[];
        assert!(!super::region_admission_matches(&entries, &cfg, 0, &record));
    }

    #[test]
    fn code_store_exposes_only_checked_ranges() {
        let mut arena = super::CodeArena::new();
        let range = arena.append_slice(&[super::Op::ParameterEnd]);
        let store = arena.freeze();
        assert_eq!(store.range_len(range.code), Some(0));
        let code = store.code(range).expect("compact code range");
        assert!(code.is_empty());
        assert_eq!(code.parameter_end(), Some(0));
        assert!(code.cold_at(0).is_none());
        let invalid = super::CodeRange::new(range.code, 0, 1).unwrap();
        assert!(store.code(invalid).is_none());
    }

    #[test]
    fn fast_instruction_has_no_duplicate_cold_operation() {
        let mut arena = super::CodeArena::new();
        let range = arena.append_slice(&[super::Op::Move { dst: 1, src: 2 }]);
        let store = arena.freeze();
        let code = store.code(range).expect("compact code range");
        assert_eq!(
            code.instruction(0),
            Some(crate::ir::Instruction::move_(1, 2))
        );
        assert!(code.cold_at(0).is_none());
    }

    #[test]
    fn guarded_catalog_rows_receive_one_disposable_quickening_site() {
        let mut arena = super::CodeArena::new();
        let range = arena.append_slice(&[super::Op::GetPropertyDynamic {
            dst: 1,
            object: 2,
            key: 3,
        }]);
        let store = arena.freeze();
        let code = store.code(range).expect("compact code range");
        assert_eq!(
            code.instruction(0).map(|i| i.opcode),
            Some(crate::ir::Opcode::AGetI)
        );
        assert!(code.quickening_site(0).is_some());

        let (store, range) = {
            let mut arena = super::CodeArena::new();
            let range = arena.append_slice(&[super::Op::Move { dst: 1, src: 2 }]);
            (arena.freeze(), range)
        };
        assert!(store.code(range).unwrap().quickening_site(0).is_none());
    }

    #[test]
    fn callable_catalog_rows_receive_a_quickening_site() {
        let mut arena = super::CodeArena::new();
        let range = arena.append_slice(&[super::Op::Call {
            dst: 0,
            callee: 1,
            receiver: None,
            args: Vec::new(),
            spreads: Vec::new(),
        }]);
        let store = arena.freeze();
        let code = store.code(range).expect("compact code range");
        assert_eq!(
            code.instruction(0).map(|instruction| instruction.opcode),
            Some(crate::ir::Opcode::Call)
        );
        assert!(code.quickening_site(0).is_some());
    }

    #[cfg(feature = "execution-trace")]
    #[test]
    fn trace_site_is_metadata_not_an_instruction() {
        let mut arena = super::CodeArena::new();
        let range = arena.append_slice(&[
            super::Op::TraceSite { source: 41 },
            super::Op::Move { dst: 1, src: 2 },
            super::Op::Move { dst: 3, src: 4 },
        ]);
        let store = arena.freeze();
        let code = store.code(range).expect("compact code range");
        assert_eq!(code.len(), 2);
        assert_eq!(code.metadata_at(0).and_then(|meta| meta.source), Some(41));
        assert_eq!(code.metadata_at(1).and_then(|meta| meta.source), Some(41));
    }
    #[test]
    fn constant_instruction_uses_canonical_pool_without_cold_operation() {
        let mut arena = super::CodeArena::new();
        let value = super::Constant::String("constant".into());
        let range = arena.append_slice(&[
            super::Op::Const {
                dst: 3,
                value: value.clone(),
            },
            super::Op::Const {
                dst: 4,
                value: value.clone(),
            },
        ]);
        let store = arena.freeze();
        let code = store.code(range).expect("compact code range");
        assert_eq!(
            code.instruction(0),
            Some(crate::ir::Instruction::load_const(3, 0))
        );
        assert_eq!(
            code.instruction(1),
            Some(crate::ir::Instruction::load_const(4, 0))
        );
        assert_eq!(code.constant_at(0), Some((3, &value)));
        assert!(code.cold_at(0).is_none());
        assert_eq!(store.constant_pool(range).len(), 1);
    }
    #[test]
    fn local_numeric_update_lowers_as_one_physical_instruction() {
        let mut arena = super::CodeArena::new();
        let range = arena.append_slice(&[
            super::Op::LoadLocal { dst: 2, slot: 7 },
            super::Op::Const {
                dst: 3,
                value: super::Constant::Number(1.0),
            },
            super::Op::Binary {
                dst: 4,
                operator: crate::ops::BinaryOp::NumericAdd,
                lhs: 2,
                rhs: 3,
            },
            super::Op::CheckInitialized {
                slot: 7,
                name: "local_7".into(),
            },
            super::Op::StoreLocal { slot: 7, src: 4 },
        ]);
        let store = arena.freeze();
        let code = store.code(range).expect("compact code range");
        assert_eq!(code.len(), 1);
        assert_eq!(
            code.instruction(0),
            Some(crate::ir::Instruction::update_local(2, 4, 7, false))
        );
        assert!(code.cold_at(0).is_none());
    }
    #[test]
    fn local_postfix_update_and_numeric_result_lower_as_one_instruction() {
        let mut arena = super::CodeArena::new();
        let range = arena.append_slice(&[
            super::Op::LoadLocal { dst: 2, slot: 7 },
            super::Op::Const {
                dst: 3,
                value: super::Constant::Number(1.0),
            },
            super::Op::Binary {
                dst: 4,
                operator: crate::ops::BinaryOp::NumericAdd,
                lhs: 2,
                rhs: 3,
            },
            super::Op::StoreLocal { slot: 7, src: 4 },
            super::Op::Unary {
                dst: 5,
                operator: crate::ops::UnaryOp::ToNumeric,
                src: 2,
            },
        ]);
        let store = arena.freeze();
        let code = store.code(range).expect("compact code range");
        assert_eq!(code.len(), 1);
        assert_eq!(
            code.instruction(0),
            Some(crate::ir::Instruction::update_local(5, 4, 7, false))
        );
        assert!(code.cold_at(0).is_none());
    }
    #[test]
    fn proven_local_assignment_lowers_as_one_flagged_move() {
        let mut arena = super::CodeArena::new();
        let range = arena.append_slice(&[
            super::Op::LoadLocal { dst: 2, slot: 7 },
            super::Op::StoreLocal { slot: 8, src: 2 },
            super::Op::Move { dst: 4, src: 2 },
        ]);
        let store = arena.freeze();
        let code = store.code(range).expect("compact code range");
        assert_eq!(code.len(), 1);
        assert_eq!(
            code.instruction(0),
            Some(crate::ir::Instruction::move_local(4, 7, 8))
        );
        assert!(code.cold_at(0).is_none());
    }
    #[test]
    fn checked_local_load_lowers_as_one_physical_instruction() {
        let mut arena = super::CodeArena::new();
        let range = arena.append_slice(&[
            super::Op::CheckInitialized {
                slot: 7,
                name: "value".into(),
            },
            super::Op::LoadLocal { dst: 2, slot: 7 },
        ]);
        let store = arena.freeze();
        let code = store.code(range).expect("compact code range");
        assert_eq!(code.len(), 1);
        assert_eq!(
            code.instruction(0),
            Some(crate::ir::Instruction::load_local_checked(2, 7))
        );
        assert_eq!(
            code.metadata_at(0)
                .and_then(|metadata| metadata.name.as_deref()),
            Some("value")
        );
        assert!(code.cold_at(0).is_none());
    }
    #[test]
    fn checked_local_store_lowers_as_one_physical_instruction() {
        let mut arena = super::CodeArena::new();
        let range = arena.append_slice(&[
            super::Op::CheckInitialized {
                slot: 7,
                name: "value".into(),
            },
            super::Op::StoreLocal { slot: 7, src: 2 },
        ]);
        let store = arena.freeze();
        let code = store.code(range).expect("compact code range");
        assert_eq!(code.len(), 1);
        assert_eq!(
            code.instruction(0),
            Some(crate::ir::Instruction::store_local_checked(7, 2))
        );
        assert!(code.cold_at(0).is_none());
    }
    #[test]
    fn static_binding_initialization_lowers_as_one_instruction() {
        let mut arena = super::CodeArena::new();
        let range = arena.append_slice(&[
            super::Op::StoreLocal { slot: 7, src: 2 },
            super::Op::InitializeLocal { slot: 7 },
        ]);
        let store = arena.freeze();
        let code = store.code(range).expect("compact code range");
        assert_eq!(code.len(), 1);
        assert_eq!(
            code.instruction(0),
            Some(crate::ir::Instruction::init_local(7, 2))
        );
        assert!(code.cold_at(0).is_none());
    }
    #[test]
    fn named_method_get_and_call_lower_as_one_physical_instruction() {
        let mut arena = super::CodeArena::new();
        let range = arena.append_slice(&[
            super::Op::GetProperty {
                dst: 4,
                object: 2,
                key: "run".into(),
            },
            super::Op::CallMethod {
                dst: 6,
                object: 2,
                key: "run".into(),
                callee: Some(4),
                args: vec![5],
                spreads: vec![false],
            },
        ]);
        let store = arena.freeze();
        let code = store.code(range).expect("compact code range");
        assert_eq!(code.len(), 1);
        assert_eq!(
            code.instruction(0),
            Some(crate::ir::Instruction::call_named(6, 2, Some(5)))
        );
        assert_eq!(
            code.metadata_at(0)
                .and_then(|metadata| metadata.name.as_deref()),
            Some("run")
        );
        assert!(code.cold_at(0).is_none());
    }
    #[test]
    fn adjacent_single_argument_method_call_uses_registered_call_word() {
        let mut arena = super::CodeArena::new();
        let range = arena.append_slice(&[
            super::Op::GetProperty {
                dst: 4,
                object: 2,
                key: "run".into(),
            },
            super::Op::Const {
                dst: 5,
                value: super::Constant::Number(1.0),
            },
            super::Op::CallMethod {
                dst: 6,
                object: 2,
                key: "run".into(),
                callee: Some(4),
                args: vec![5],
                spreads: vec![false],
            },
        ]);
        let store = arena.freeze();
        let code = store.code(range).expect("compact code range");
        assert_eq!(
            code.instruction(2),
            Some(crate::ir::Instruction::call_registered_one(6, 2, 4))
        );
        assert!(code.cold_at(2).is_none());
    }
    #[test]
    fn nonconsecutive_call_arguments_live_in_pc_metadata() {
        let mut arena = super::CodeArena::new();
        let range = arena.append_slice(&[super::Op::CallMethod {
            dst: 20,
            object: 2,
            key: "am".into(),
            callee: Some(4),
            args: vec![5, 7, 9, 11, 13, 17],
            spreads: vec![false; 6],
        }]);
        let store = arena.freeze();
        let code = store.code(range).expect("compact code range");
        assert_eq!(
            code.instruction(0),
            Some(crate::ir::Instruction::call_registered_window(20, 2, 4, 6))
        );
        assert_eq!(
            code.operand_window_at(0),
            Some([5, 7, 9, 11, 13, 17].as_slice())
        );
        assert!(code.cold_at(0).is_none());
    }
    #[test]
    fn frozen_code_store_owns_metadata_after_builder_drop() {
        let (store, range) = {
            let mut arena = super::CodeArena::new();
            let range = arena.append_slice(&[super::Op::CheckInitialized {
                slot: 1,
                name: "owned_name".to_string(),
            }]);
            (arena.freeze(), range)
        };
        let metadata = store.metadata(range).expect("metadata remains live");
        assert_eq!(metadata[0].name.as_deref(), Some("owned_name"));
        assert_eq!(Rc::strong_count(&store), 1);
    }

    #[test]
    fn constant_pool_clones_share_immutable_values() {
        let pool = super::ConstantPool::new(vec![super::Constant::String("stable".into())]);
        let clone = pool.clone();
        assert!(Rc::ptr_eq(&pool.values, &clone.values));
        assert_eq!(clone.id(&super::Constant::String("stable".into())), Some(0));
        assert_eq!(clone.get(0), pool.get(0));
    }
    #[test]
    fn check_initialized_metadata_is_out_of_line() {
        let mut arena = super::CodeArena::new();
        let range = arena.append_slice(&[super::Op::CheckInitialized {
            slot: 3,
            name: "temporal_name".to_string(),
        }]);
        let store = arena.freeze();
        let instructions = store.code(range).expect("instruction range");
        assert_eq!(instructions.len(), 1);
        assert_eq!(store.range_len(range.code), Some(1));
        let metadata = store.metadata(range).expect("metadata must resolve");
        assert_eq!(metadata.len(), instructions.len());
        assert_eq!(metadata[0].name.as_deref(), Some("temporal_name"));
        assert_eq!(metadata[0].flags, 1);
    }
    #[test]
    fn machine_exposes_flat_execution_state() {
        let machine = Machine::with_register_count(super::CodeId(7), EnvironmentRef(3), 5);
        assert_eq!(machine.code_id(), super::CodeId(7));
        assert_eq!(machine.program_counter(), 0);
        assert_eq!(machine.register_count(), 5);
        assert_eq!(machine.registers().len(), 5);
        assert!(matches!(machine.completion(), Completion::Normal));
        assert_eq!(machine.frame_count(), 0);
    }
    #[test]
    fn machine_state_has_one_canonical_view() {
        let machine = Machine::with_register_count(super::CodeId(7), EnvironmentRef(3), 5);
        assert_eq!(machine.program_counter(), 0);
        assert_eq!(machine.register_count(), 5);
        assert!(machine.current_frame().is_none());
        assert!(machine.constants().is_none());
        assert!(machine.state_is_valid());
    }
    #[test]
    fn restoring_registers_preserves_width_invariant() {
        let mut machine = Machine::with_register_count(super::CodeId(7), EnvironmentRef(3), 5);
        machine.restore_registers(crate::register_file::RegisterFile::from_values(
            vec![Value::Undefined; 2],
        ));
        assert_eq!(machine.register_count(), 2);
        assert!(machine.state_is_valid());
    }
    #[test]
    fn tail_calls_reuse_one_machine_dispatch_loop() {
        let mut machine = Machine::new(super::CodeId(0), EnvironmentRef(0));
        let mut remaining = 128;
        let completion = machine
            .run_until_complete(Completion::Normal, |_, _| -> Result<Completion, ()> {
                if remaining == 0 {
                    return Ok(Completion::Return(Value::Undefined));
                }
                remaining -= 1;
                Ok(Completion::TailCall(TailCallRequest {
                    callee: Value::Undefined,
                    receiver: Value::Undefined,
                    arguments: Vec::new().into(),
                }))
            })
            .unwrap();
        assert_eq!(completion, Completion::Return(Value::Undefined));
        assert_eq!(remaining, 0);
    }

    #[test]
    fn frame_offsets_track_contiguous_storage() {
        let mut stack = FrameStack::with_capacity(2);
        let range = super::CodeRange::new(super::CodeId(0), 0, 1).unwrap();
        assert_eq!(stack.top_offset(), None);
        assert_eq!(stack.frame_at(0), None);
        stack
            .try_push(super::Frame::Await {
                phase: 0,
                resume: range,
                destination: 0,
            })
            .unwrap();
        assert_eq!(stack.top_offset(), Some(0));
        assert!(stack.frame_at(0).is_some());
        assert!(stack.frame_at_mut(0).is_some());
        assert_eq!(stack.top_offset(), Some(0));
    }
    #[test]
    fn frame_stack_grows_geometrically_without_crossing_limit() {
        let mut stack = FrameStack::with_capacity_and_limit(1, 5);
        assert_eq!(stack.capacity(), 1);
        assert!(stack.try_reserve_for(4));
        assert_eq!(stack.capacity(), 4);
        assert_eq!(stack.remaining(), 5);
        let range = super::CodeRange::new(super::CodeId(0), 0, 1).unwrap();
        for _ in 0..5 {
            stack
                .try_push(super::Frame::Await {
                    phase: 0,
                    resume: range,
                    destination: 0,
                })
                .unwrap();
        }
        assert_eq!(stack.depth(), 5);
        assert!(stack.capacity() >= 5);
        assert_eq!(stack.remaining(), 0);
        let rejected = stack.try_push(super::Frame::Await {
            phase: 0,
            resume: range,
            destination: 0,
        });
        assert!(rejected.is_err());
        assert_eq!(stack.depth(), 5);
        assert!(stack.capacity() >= 5);
    }

    #[test]
    fn frame_stack_rejects_reservation_before_allocating_past_limit() {
        let mut stack = FrameStack::with_capacity_and_limit(2, 3);
        assert!(!stack.try_reserve_for(4));
        assert_eq!(stack.capacity(), 2);
        assert_eq!(stack.remaining(), 3);
    }

    include!("machine_tests.rs");
}
