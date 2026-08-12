use crate::{completion::Completion, value::Value};

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
    pub(crate) registers: Vec<Value>,
    pub(crate) environment: EnvironmentRef,
    pub(crate) completion: Completion,
    pub(crate) frames: Vec<Frame>,
}

impl Machine {
    pub fn new(code: CodeId, environment: EnvironmentRef) -> Self {
        Self {
            code,
            pc: 0,
            registers: Vec::new(),
            environment,
            completion: Completion::Normal,
            frames: Vec::new(),
        }
    }
}
