use crate::{completion::Completion, ops::Op, value::Value};

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

impl CodeArena {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append(&mut self, body: Vec<Op>) -> CodeRange {
        let code = CodeId(self.ranges.len() as u32);
        let start = self.ops.len() as u32;
        self.ops.extend(body);
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
}
