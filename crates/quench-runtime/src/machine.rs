use crate::{completion::Completion, ops::Op, value::Value};
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CodeId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CodeRange {
    pub code: CodeId,
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EnvironmentRef(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackedCompletion {
    pub tag: u8,
    pub flags: u8,
    pub payload: u32,
    pub aux: u32,
}

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
}

impl FrameStack {
    pub fn new() -> Self {
        Self {
            base: 0,
            count: 0,
            frames: Vec::new(),
        }
    }
}

impl Default for FrameStack {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameStack {
    pub fn push(&mut self, frame: Frame) {
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

    pub fn append_slice(&mut self, body: &[Op]) -> CodeRange {
        let code = CodeId(self.ranges.len() as u32);
        let start = self.ops.len() as u32;
        self.ops.extend_from_slice(body);
        let end = self.ops.len() as u32;
        self.ranges.push((start, end));
        CodeRange { code, start, end }
    }

    pub fn get(&self, range: CodeRange) -> Option<&[Op]> {
        let (start, end) = self.ranges.get(range.code.0 as usize).copied()?;
        let start = start.max(range.start);
        let end = end.min(range.end);
        (start <= end).then(|| &self.ops[start as usize..end as usize])
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

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionCode {
    store: Rc<CodeStore>,
    pub range: CodeRange,
}

impl FunctionCode {
    pub fn new(store: Rc<CodeStore>, range: CodeRange) -> Self {
        Self { store, range }
    }

    pub fn ops(&self) -> Option<&[Op]> {
        let (start, end) = self.store.ranges.get(self.range.code.0 as usize).copied()?;
        let start = start.max(self.range.start);
        let end = end.min(self.range.end);
        (start <= end).then(|| &self.store.ops[start as usize..end as usize])
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
            code,
            pc: 0,
            registers: RegisterWindow::new(),
            environment,
            completion: Completion::Normal,
            frames: FrameStack::new(),
        }
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

    pub fn push_frame(&mut self, frame: Frame) {
        self.frames.push(frame);
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
    use super::{CodeId, EnvironmentRef, Frame, FrameStack, Machine};
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
