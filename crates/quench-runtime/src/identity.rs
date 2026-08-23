#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HeapRef(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShapeId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PropertyKeyId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContinuationId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CodeId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CodeRange {
    pub code: CodeId,
    pub start: u32,
    pub end: u32,
}

impl CodeRange {
    pub fn new(code: CodeId, start: u32, end: u32) -> Option<Self> {
        (start <= end).then_some(Self { code, start, end })
    }

    pub fn len(self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }

    pub fn contains(self, offset: u32) -> bool {
        offset >= self.start && offset < self.end
    }
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
