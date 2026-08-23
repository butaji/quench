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
use std::{collections::HashMap, rc::Rc, sync::OnceLock};

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
}

impl InstructionMeta {
    pub const fn empty() -> Self {
        Self {
            source: None,
            name: None,
            flags: 0,
        }
    }
}

/// Per-code canonical constant table.
///
/// `values` owns one entry per [`ConstantKey`], and `ids` is the complete
/// inverse index. IDs are assigned in first-use order and are never truncated:
/// construction fails if the table cannot be represented by `u16` IDs.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstantPool {
    values: Rc<[Constant]>,
    ids: HashMap<ConstantKey, u16>,
}
impl ConstantPool {
    pub fn try_new(values: Vec<Constant>) -> Result<Self, &'static str> {
        let mut canonical = Vec::with_capacity(values.len());
        let mut ids = HashMap::with_capacity(values.len());
        for value in values {
            let key = ConstantKey::from(&value);
            if ids.contains_key(&key) {
                continue;
            }
            let id = u16::try_from(canonical.len()).map_err(|_| "constant pool exceeds u16 IDs")?;
            canonical.push(value);
            ids.insert(key, id);
        }
        Ok(Self {
            values: canonical.into(),
            ids,
        })
    }

    pub fn new(values: Vec<Constant>) -> Self {
        Self::try_new(values).expect("constant pool exceeds u16 IDs")
    }

    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    pub fn id(&self, value: &Constant) -> Option<u16> {
        self.ids.get(&ConstantKey::from(value)).copied()
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

#[derive(Debug, Default)]
pub struct CodeArena {
    ops: Vec<Op>,
    ranges: Vec<(u32, u32)>,
    metadata: Vec<Vec<InstructionMeta>>,
}

fn metadata_for(op: &Op) -> InstructionMeta {
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
        | Op::InitializeResolvedBinding { name, .. }
        | Op::SetResolvedLocalBinding { name, .. }
        | Op::LoadResolvedLocalBinding { name, .. }
        | Op::LoadBinding { name, .. } => Some(Rc::<str>::from(name.as_str())),
        _ => None,
    };
    InstructionMeta {
        source: None,
        name,
        flags: u16::from(matches!(op, Op::CheckInitialized { .. })),
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
    pub values: Vec<Value>,
}

impl RegisterWindow {
    pub fn new() -> Self {
        Self {
            base: 0,
            count: 0,
            values: Vec::new(),
        }
    }

    pub fn with_count(count: u16) -> Self {
        Self {
            base: 0,
            count,
            values: vec![Value::Undefined; usize::from(count)],
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
        let start = self.ops.len() as u32;
        self.ops.extend_from_slice(body);
        self.metadata.push(body.iter().map(metadata_for).collect());
        let end = self.ops.len() as u32;
        self.ranges.push((start, end));
        CodeRange { code, start, end }
    }

    pub fn append(&mut self, body: Vec<Op>) -> CodeRange {
        self.append_slice(&body)
    }

    pub fn append_function(&mut self, function: &FunctionCode) -> Option<CodeRange> {
        let body = function.ops()?;
        Some(self.append_slice(body))
    }

    pub fn get(&self, range: CodeRange) -> Option<&[Op]> {
        let (start, end) = self.ranges.get(range.code.0 as usize).copied()?;
        (range.start >= start && range.end <= end)
            .then(|| &self.ops[range.start as usize..range.end as usize])
    }
    pub fn constant_pool(&self, range: CodeRange) -> ConstantPool {
        ConstantPool::new(
            self.get(range)
                .into_iter()
                .flat_map(|ops| ops.iter())
                .filter_map(|op| match op {
                    Op::Const { value, .. } => Some(value.clone()),
                    _ => None,
                })
                .collect(),
        )
    }

    pub fn len(&self) -> usize {
        self.ranges.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    pub fn freeze(self) -> Rc<CodeStore> {
        Rc::new(CodeStore {
            ops: self.ops.into_boxed_slice().into(),
            ranges: self.ranges.into_boxed_slice().into(),
            metadata: self.metadata.into_boxed_slice().into(),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodeStore {
    ops: Rc<[Op]>,
    ranges: Rc<[(u32, u32)]>,
    metadata: Rc<[Vec<InstructionMeta>]>,
}

impl CodeStore {
    pub fn get(&self, range: CodeRange) -> Option<&[Op]> {
        let (start, end) = self.ranges.get(range.code.0 as usize).copied()?;
        (range.start >= start && range.end <= end)
            .then(|| &self.ops[range.start as usize..range.end as usize])
    }
    /// Rare metadata lives out of line from hot instructions.
    pub fn metadata(&self, range: CodeRange) -> Option<&[InstructionMeta]> {
        self.metadata.get(range.code.0 as usize).map(Vec::as_slice)
    }
    /// Build the compact pool used by lowering and diagnostics.
    pub fn constant_pool(&self, range: CodeRange) -> ConstantPool {
        let mut values = Vec::new();
        if let Some(ops) = self.get(range) {
            for op in ops {
                let Op::Const { value, .. } = op else {
                    continue;
                };
                if !values.iter().any(|item| item == value) {
                    values.push(value.clone());
                }
            }
        }
        ConstantPool::new(values)
    }

    pub fn range_len(&self, code: CodeId) -> Option<u32> {
        let (start, end) = self.ranges.get(code.0 as usize).copied()?;
        Some(end.saturating_sub(start))
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

    pub fn ops(&self) -> &[Op] {
        self.store.get(self.entry).unwrap_or(&[])
    }

    pub fn store(&self) -> Rc<CodeStore> {
        self.store.clone()
    }

    pub fn entry(&self) -> CodeRange {
        self.entry
    }
}

#[derive(Debug, Clone)]
pub struct FunctionCode {
    store: Rc<OnceLock<Rc<CodeStore>>>,
    pub range: CodeRange,
}

impl FunctionCode {
    pub fn from_ops(body: Vec<Op>) -> Self {
        let (_, range, store) = freeze_tree(body);
        Self { store, range }
    }

    /// Materialize related nested bodies in one immutable store.
    pub fn from_ops_many(bodies: Vec<Vec<Op>>) -> Vec<Self> {
        let mut arena = CodeArena::new();
        let store = Rc::new(OnceLock::new());
        let ranges = bodies
            .into_iter()
            .map(|body| arena.append_tree(body, &store))
            .collect::<Vec<_>>();
        let _ = store.set(arena.freeze());
        ranges
            .into_iter()
            .map(|range| Self {
                store: store.clone(),
                range,
            })
            .collect()
    }

    pub fn new(store: Rc<CodeStore>, range: CodeRange) -> Self {
        let linked = Rc::new(OnceLock::new());
        let _ = linked.set(store);
        Self {
            store: linked,
            range,
        }
    }

    pub fn ops(&self) -> Option<&[Op]> {
        self.store.get()?.get(self.range)
    }

    pub fn code_id(&self) -> CodeId {
        self.range.code
    }

    pub(crate) fn store(&self) -> Option<Rc<CodeStore>> {
        self.store.get().cloned()
    }

    pub(crate) fn rehome(&mut self, arena: &mut CodeArena, store: &Rc<OnceLock<Rc<CodeStore>>>) {
        let Some(body) = self.ops() else {
            return;
        };
        self.range = arena.append_tree(body.to_vec(), store);
        self.store = store.clone();
    }
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
        self.range == other.range && self.ops() == other.ops()
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
    },
    Delegate {
        phase: u8,
        iterator: Value,
        destination: u16,
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
        F: FnOnce(&mut Vec<Value>) -> Result<Completion, E>,
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
        F: FnMut(&mut Vec<Value>, Completion) -> Result<Completion, E>,
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
        let caller_ops = self
            .store
            .as_ref()
            .and_then(|store| store.get(self.code_range()))
            .map(Rc::from)
            .unwrap_or_else(|| Rc::from([]));
        let continuation = crate::completion::CallContinuation {
            callee,
            receiver,
            arguments,
            caller_ops,
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
    pub(crate) fn resume_call(
        &mut self,
        value: Value,
    ) -> Option<crate::completion::CallContinuation> {
        let continuation = self.pop_call_frame()?;
        self.restore_registers(continuation.caller_registers.clone());
        self.environment = continuation.caller_environment;
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

    pub(crate) fn take_registers(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.registers.values)
    }

    pub(crate) fn restore_registers(&mut self, registers: Vec<Value>) {
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
    /// Borrow the contiguous register storage used by the active frame.
    #[inline]
    pub fn registers_mut(&mut self) -> &mut Vec<Value> {
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
        assert_eq!(store.range_len(range.code), Some(1));
        assert_eq!(store.get(range).map(<[_]>::len), Some(1));
        let invalid = super::CodeRange::new(range.code, 0, 2).unwrap();
        assert!(store.get(invalid).is_none());
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

        let instructions = store.get(range).expect("code range must resolve");
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
        machine.restore_registers(vec![Value::Undefined; 2]);
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
                    arguments: Vec::new(),
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
                })
                .unwrap();
        }
        assert_eq!(stack.depth(), 5);
        assert!(stack.capacity() >= 5);
        assert_eq!(stack.remaining(), 0);
        let rejected = stack.try_push(super::Frame::Await {
            phase: 0,
            resume: range,
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
