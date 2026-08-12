pub use crate::identity::{CodeId, CodeRange, EnvironmentRef, FrameId, PackedCompletion};
use crate::{completion::Completion, ops::Op, value::Value};
use std::{rc::Rc, sync::OnceLock};

#[derive(Debug, Default)]
pub struct CodeArena {
    ops: Vec<Op>,
    ranges: Vec<(u32, u32)>,
}

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
}

impl Default for RegisterWindow {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FrameStack {
    pub base: u32,
    pub count: u16,
    pub frames: Vec<Frame>,
    limit: Option<u16>,
}

impl FrameStack {
    const DEFAULT_CAPACITY: u16 = 64;

    pub fn new() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }

    pub fn with_capacity(capacity: u16) -> Self {
        Self {
            base: 0,
            count: 0,
            frames: Vec::with_capacity(usize::from(capacity)),
            limit: Some(capacity),
        }
    }
}

impl Default for FrameStack {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameStack {
    pub fn try_push(&mut self, frame: Frame) -> Result<(), Frame> {
        if self.limit.is_some_and(|limit| self.count >= limit) {
            return Err(frame);
        }
        self.push(frame);
        Ok(())
    }

    fn push(&mut self, frame: Frame) {
        self.frames.push(frame);
        self.count = u16::try_from(self.frames.len()).unwrap_or(u16::MAX);
    }

    pub fn pop(&mut self) -> Option<Frame> {
        let frame = self.frames.pop();
        self.count = u16::try_from(self.frames.len()).unwrap_or(u16::MAX);
        frame
    }
}

impl CodeArena {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append(&mut self, body: Vec<Op>) -> CodeRange {
        self.append_slice(&body)
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
        let end = self.ops.len() as u32;
        self.ranges.push((start, end));
        CodeRange { code, start, end }
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
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodeStore {
    ops: Rc<[Op]>,
    ranges: Rc<[(u32, u32)]>,
}

impl CodeStore {
    pub fn get(&self, range: CodeRange) -> Option<&[Op]> {
        let (start, end) = self.ranges.get(range.code.0 as usize).copied()?;
        (range.start >= start && range.end <= end)
            .then(|| &self.ops[range.start as usize..range.end as usize])
    }

    pub fn range_len(&self, code: CodeId) -> Option<u32> {
        let (start, end) = self.ranges.get(code.0 as usize).copied()?;
        Some(end.saturating_sub(start))
    }
}

#[derive(Debug, Clone)]
pub struct FunctionCode {
    store: Rc<OnceLock<Rc<CodeStore>>>,
    pub range: CodeRange,
}

impl FunctionCode {
    pub fn from_ops(body: Vec<Op>) -> Self {
        let mut arena = CodeArena::new();
        let store = Rc::new(OnceLock::new());
        let range = arena.append_tree(body, &store);
        let _ = store.set(arena.freeze());
        Self { store, range }
    }

    /// Materialize related nested bodies in one immutable store.
    pub fn from_ops_many(bodies: Vec<Vec<Op>>) -> Vec<Self> {
        let mut arena = CodeArena::new();
        let ranges = bodies
            .into_iter()
            .map(|body| arena.append(body))
            .collect::<Vec<_>>();
        let store = arena.freeze();
        ranges
            .into_iter()
            .map(|range| Self::new(store.clone(), range))
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

impl PartialEq for FunctionCode {
    fn eq(&self, other: &Self) -> bool {
        self.range == other.range && self.ops() == other.ops()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    Try {
        phase: u8,
        body: CodeRange,
        handler: Option<CodeRange>,
        finalizer: Option<CodeRange>,
    },
    Iterator {
        phase: u8,
        iterator: Value,
        binding: u16,
        body: CodeRange,
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
    Control {
        phase: u8,
        body: CodeRange,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Machine {
    pub(crate) store: Option<Rc<CodeStore>>,
    pub(crate) code: CodeId,
    pub(crate) pc: u32,
    pub(crate) registers: RegisterWindow,
    pub(crate) environment: EnvironmentRef,
    pub(crate) completion: Completion,
    pub(crate) frames: FrameStack,
}

impl Machine {
    pub fn new(code: CodeId, environment: EnvironmentRef) -> Self {
        Self {
            store: None,
            code,
            pc: 0,
            registers: RegisterWindow::new(),
            environment,
            completion: Completion::Normal,
            frames: FrameStack::new(),
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

    pub(crate) fn code(&self, range: CodeRange) -> Option<&[Op]> {
        self.store.as_ref()?.get(range)
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
        F: FnOnce(&mut Self) -> Result<Completion, E>,
    {
        self.completion = input;
        let completion = execute(self)?;
        self.completion = completion.clone();
        Ok(completion)
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
        if !matches!(self.frames.frames.last(), Some(Frame::Await { .. })) {
            return false;
        }
        self.frames.pop();
        true
    }

    pub fn frame_count(&self) -> u16 {
        self.frames.count
    }
}

#[cfg(test)]
mod tests {
    use super::{CodeId, CodeRange, EnvironmentRef, Frame, FrameStack, Machine, RegisterWindow};
    use crate::value::Value;

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
    fn code_arena_can_import_existing_function_ranges() {
        let function = super::FunctionCode::from_ops(vec![super::Op::ParameterEnd]);
        let mut arena = super::CodeArena::new();
        let range = arena.append_function(&function).unwrap();
        let store = arena.freeze();
        assert_eq!(store.get(range).map(<[_]>::len), Some(1));
    }

    #[test]
    fn linked_nested_bodies_share_one_immutable_code_store() {
        let child = super::FunctionCode::from_ops(vec![super::Op::ParameterEnd]);
        let root = super::FunctionCode::from_ops(vec![super::Op::IteratorBinding {
            iterator: 0,
            body: child,
            close_normal: false,
        }]);
        let Some([super::Op::IteratorBinding { body, .. }]) = root.ops() else {
            panic!("root body is not an iterator binding");
        };
        assert!(std::rc::Rc::ptr_eq(&root.store, &body.store));
        assert_ne!(root.range, body.range);
    }

    #[test]
    fn machine_resolves_frame_ranges_from_its_function_store() {
        let function = super::FunctionCode::from_ops(vec![super::Op::ParameterEnd]);
        let machine = Machine::with_function(&function, EnvironmentRef(0), 1);
        assert_eq!(machine.code(function.range), function.ops());
    }

    #[test]
    fn register_window_is_pre_sized_from_code_metadata() {
        let window = RegisterWindow::with_count(3);
        assert_eq!(window.count, 3);
        assert_eq!(window.values.len(), 3);
        assert!(window
            .values
            .iter()
            .all(|value| matches!(value, Value::Undefined)));
    }

    #[test]
    fn frame_stack_can_reserve_fixed_metadata_capacity() {
        let mut stack = FrameStack::with_capacity(1);
        assert_eq!(stack.count, 0);
        assert!(stack.frames.is_empty());
        assert!(stack.frames.capacity() >= 1);
        let frame = Frame::Control {
            phase: 0,
            body: CodeRange::new(CodeId(0), 0, 0).unwrap(),
        };
        assert!(stack.try_push(frame.clone()).is_ok());
        assert!(stack.try_push(frame).is_err());
    }
    use crate::completion::Completion;

    #[test]
    fn machine_step_updates_completion_and_frame_count() {
        let mut machine = Machine::new(CodeId(1), EnvironmentRef(2));
        let mut frames = FrameStack::new();
        frames.push(Frame::Await {
            phase: 0,
            resume: super::CodeRange {
                code: CodeId(1),
                start: 0,
                end: 1,
            },
        });
        assert_eq!(frames.count, 1);
        assert!(frames.pop().is_some());
        assert_eq!(frames.count, 0);
        let completion = machine
            .step(Completion::Normal, |_| {
                Ok::<_, ()>(Completion::Return(crate::value::Value::Undefined))
            })
            .expect("machine test transition should succeed");
        assert!(matches!(
            completion,
            Completion::Return(crate::value::Value::Undefined)
        ));
    }
}
