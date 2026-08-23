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
include!("machine_tail.rs");
