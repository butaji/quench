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
use crate::{
    completion::Completion,
    ir::ConstantKey,
    ops::{Constant, Op},
    value::Value,
};
use std::{rc::Rc, sync::OnceLock};

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
    pub named_cache: std::cell::Cell<u64>,
}

impl InstructionMeta {
    pub const fn empty() -> Self {
        Self {
            source: None,
            name: None,
            flags: 0,
            operand_window: u32::MAX,
            named_cache: std::cell::Cell::new(0),
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
        | Op::SetProperty { key: name, .. } => Some(Rc::<str>::from(name.as_str())),
        _ => None,
    };
    InstructionMeta {
        source,
        name,
        flags: u16::from(matches!(op, Op::CheckInitialized { .. })),
        operand_window: u32::MAX,
        named_cache: std::cell::Cell::new(0),
    }
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
                .map_or(true, |offset| usize::from(offset) + 1 == self.frames.len())
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
        self.metadata.push(metadata);
        self.operand_windows.push(operand_windows);
        self.catch_ranges
            .push(std::mem::take(&mut self.pending_catches));
        let end = self.instructions.len() as u32;
        self.ranges.push((start, end));
        self.parameter_ends.push(parameter_end);
        self.constants.push(constants);
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
                if !(ops_contain_call(init)
                    || ops_contain_call(test)
                    || ops_contain_call(loop_body)
                    || ops_contain_call(update))
                    || ops_contain_short_circuit(test)
                    || test_always_true(test)
                    || ops_use_arguments(init)
                    || ops_use_arguments(test)
                    || ops_use_arguments(loop_body)
                    || ops_use_arguments(update)
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
        Rc::new(CodeStore {
            instructions: self.instructions.into_boxed_slice().into(),
            cold: self.cold.into_boxed_slice().into(),
            ranges: self.ranges.into_boxed_slice().into(),
            parameter_ends: self.parameter_ends.into_boxed_slice().into(),
            constants: self.constants.into_boxed_slice().into(),
            metadata: self.metadata.into_boxed_slice().into(),
            operand_windows: self.operand_windows.into_boxed_slice().into(),
            catch_ranges: self.catch_ranges.into_boxed_slice().into(),
        })
    }
}

fn trace_source(op: &Op) -> Option<u32> {
    match op {
        Op::TraceSite { source } => Some(*source),
        _ => None,
    }
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

#[derive(Debug, Clone, PartialEq)]
pub struct CodeStore {
    instructions: Rc<[crate::ir::Instruction]>,
    cold: Rc<[Op]>,
    ranges: Rc<[(u32, u32)]>,
    parameter_ends: Rc<[Option<u32>]>,
    constants: Rc<[ConstantPool]>,
    metadata: Rc<[Vec<InstructionMeta>]>,
    operand_windows: Rc<[Vec<Rc<[u16]>>]>,
    catch_ranges: Rc<[Vec<CatchRange>]>,
}

impl CodeStore {
    pub(crate) fn instruction_count(&self) -> usize {
        self.instructions.len()
    }

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
}

#[derive(Clone, Copy)]
pub struct CodeView<'a> {
    store: &'a CodeStore,
    range: CodeRange,
}

impl<'a> CodeView<'a> {
    #[inline]
    #[cfg(feature = "execution-trace")]
    pub(crate) fn trace_identity(self) -> (usize, u32) {
        (self.store as *const CodeStore as usize, self.range.code.0)
    }

    pub fn len(self) -> usize {
        self.range.end.saturating_sub(self.range.start) as usize
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
        (pc < self.len()).then(|| self.store.instructions[(self.range.start as usize) + pc])
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

#[derive(Debug, Clone)]
pub struct FunctionCode {
    store: Rc<OnceLock<Rc<CodeStore>>>,
    pub range: CodeRange,
    source: Option<Rc<[Op]>>,
    capture_slots: Rc<[u16]>,
    facts: Rc<crate::facts::FunctionFacts>,
}

impl FunctionCode {
    pub fn from_ops(body: Vec<Op>) -> Self {
        let capture_slots = collect_capture_slots(&body);
        let (_, range, store) = freeze_tree(body);
        Self {
            store,
            range,
            source: None,
            capture_slots,
            facts: Rc::default(),
        }
    }

    pub fn pending(body: Vec<Op>) -> Self {
        let capture_slots = collect_capture_slots(&body);
        Self {
            store: Rc::new(OnceLock::new()),
            range: CodeRange {
                code: CodeId(0),
                start: 0,
                end: body.len() as u32,
            },
            source: Some(body.into_boxed_slice().into()),
            capture_slots,
            facts: Rc::default(),
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
                store: store.clone(),
                range,
                source: None,
                capture_slots,
                facts: Rc::default(),
            })
            .collect()
    }

    pub fn new(store: Rc<CodeStore>, range: CodeRange) -> Self {
        let linked = Rc::new(OnceLock::new());
        let _ = linked.set(store);
        Self {
            store: linked,
            range,
            source: None,
            capture_slots: Rc::from([u16::MAX]),
            facts: Rc::default(),
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

    pub fn code_id(&self) -> CodeId {
        self.range.code
    }

    pub fn len(&self) -> usize {
        self.range.end.saturating_sub(self.range.start) as usize
    }

    pub(crate) fn register_capacity(&self) -> usize {
        self.store()
            .map_or_else(|| self.len(), |store| store.instruction_count())
            .max(32)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(crate) fn code(&self) -> Option<CodeView<'_>> {
        self.store.get()?.code(self.range)
    }

    pub(crate) fn store(&self) -> Option<Rc<CodeStore>> {
        self.store.get().cloned()
    }

    pub(crate) fn capture_slots(&self) -> &[u16] {
        &self.capture_slots
    }

    pub(crate) fn uses_slot(&self, slot: u16) -> bool {
        self.capture_slots.binary_search(&u16::MAX).is_ok()
            || self.capture_slots.binary_search(&slot).is_ok()
    }

    pub(crate) fn rehome(&mut self, arena: &mut CodeArena, store: &Rc<OnceLock<Rc<CodeStore>>>) {
        let body = self.source.take().map(|body| body.to_vec());
        let Some(body) = body else { return };
        self.range = arena.append_tree(body, store);
        self.store = store.clone();
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
        self.store = store.clone();
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
            && (self.source.is_some() || Rc::ptr_eq(&self.store, &other.store))
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
        dst: u16,
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
        body_resume: CodeRange,
        resume: CodeRange,
        dst: u16,
        yield_dst: u16,
        post_test: bool,
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
    #[cfg(test)]
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
    #[cfg(test)]
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
