//! Source position tables, delta-coded per function, and their decoding.

use super::SourcePosition;

/// A function's source positions in increasing pc order, each delta-coded against the one
/// before it as LEB128 varints: the pc advance, the line change (zigzag), then the column,
/// as a change (zigzag) on the same line and absolute on a new one. Most entries take three
/// bytes, a quarter of a `SourcePosition`, and a function keeps one per source location
/// change while it compiles and for as long as it lives.
#[derive(Clone, Debug, Default)]
pub(crate) struct SourcePositions {
    bytes: Vec<u8>,
    /// The final entry, which the next one is coded against.
    last: Option<SourcePosition>,
    /// Where decoding may start, made on the first lookup of a table longer than one interval
    /// (`SourcePositions::at`): a stack trace through a large function otherwise decoded its
    /// table from the start for every frame (Babel: about 20K instructions a frame).
    checkpoints: std::cell::OnceCell<Box<SourcePositionIndex>>,
}

/// A table's lookup memo: its checkpoints and its last lookup's pc and answer. A throw site
/// that a loop runs again, and each frame under it, asks for the same pc every time.
#[derive(Clone, Debug)]
struct SourcePositionIndex {
    checkpoints: Box<[SourcePositionCheckpoint]>,
    recent: std::cell::Cell<Option<(u32, Option<SourcePosition>)>>,
}

/// The entries between two `SourcePositions` checkpoints.
const SOURCE_POSITION_CHECKPOINT_INTERVAL: usize = 32;

/// A point decoding may resume from: the byte offset of an entry, the entry before it
/// (which it is coded against) and the entry's own pc.
#[derive(Clone, Copy, Debug)]
struct SourcePositionCheckpoint {
    offset: usize,
    previous: SourcePosition,
    pc: u32,
}

/// Two tables are equal when their entries are; the index is a memo of them.
impl PartialEq for SourcePositions {
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes && self.last == other.last
    }
}
impl Eq for SourcePositions {}

/// The payload bits of one LEB128 byte; the high bit marks a continuation.
const LEB128_PAYLOAD_BITS: u32 = 7;
const LEB128_CONTINUATION: u8 = 1 << LEB128_PAYLOAD_BITS;

impl SourcePositions {
    pub(crate) fn last(&self) -> Option<SourcePosition> {
        self.last
    }

    /// The coded entries and the final one, as a residual stores them
    /// (`BinaryWriter::source_positions`).
    pub(crate) fn encoded(&self) -> (&[u8], Option<SourcePosition>) {
        (&self.bytes, self.last)
    }

    /// The table a residual stored (`encoded`); validation judges it against its function.
    pub(crate) fn from_encoded(bytes: Vec<u8>, last: Option<SourcePosition>) -> Self {
        Self { bytes, last, checkpoints: std::cell::OnceCell::new() }
    }

    /// Appends `position`. Compiled code orders positions by pc; a decoded table keeps any
    /// order exactly, for validation to judge.
    pub(crate) fn push(&mut self, position: SourcePosition) {
        let previous = self.last.unwrap_or(SourcePosition { pc: 0, line: 0, column: 0 });
        let line_change = i64::from(position.line) - i64::from(previous.line);
        self.write(u64::from(position.pc.wrapping_sub(previous.pc)));
        self.write(zigzag(line_change));
        self.write(if line_change == 0 {
            zigzag(i64::from(position.column) - i64::from(previous.column))
        } else {
            u64::from(position.column)
        });
        self.last = Some(position);
        // The checkpoints exist only once a lookup made them.
        if self.checkpoints.get().is_some() {
            self.checkpoints = std::cell::OnceCell::new();
        }
    }

    /// An empty table with room for `bytes` coded bytes.
    pub(crate) fn with_capacity(bytes: usize) -> Self {
        Self { bytes: Vec::with_capacity(bytes), last: None, checkpoints: std::cell::OnceCell::new() }
    }

    /// The bytes the entries are coded in.
    pub(crate) fn coded_len(&self) -> usize {
        self.bytes.len()
    }

    #[inline(always)]
    fn write(&mut self, mut value: u64) {
        // Most deltas fit one byte.
        if value < u64::from(LEB128_CONTINUATION) {
            self.bytes.push(value as u8);
            return;
        }
        loop {
            let byte = (value & u64::from(!LEB128_CONTINUATION)) as u8;
            value >>= LEB128_PAYLOAD_BITS;
            if value == 0 {
                self.bytes.push(byte);
                return;
            }
            self.bytes.push(byte | LEB128_CONTINUATION);
        }
    }

    pub(crate) fn iter(&self) -> SourcePositionIter<'_> {
        self.iter_from(0, SourcePosition { pc: 0, line: 0, column: 0 })
    }

    fn iter_from(&self, offset: usize, previous: SourcePosition) -> SourcePositionIter<'_> {
        SourcePositionIter { bytes: &self.bytes[offset..], previous }
    }

    /// The position in effect at `pc`: the last entry at or before it, decoded from the last
    /// checkpoint at or before it.
    pub(crate) fn at(&self, pc: u32) -> Option<SourcePosition> {
        let index = self.checkpoints.get_or_init(|| {
            Box::new(SourcePositionIndex { checkpoints: self.make_checkpoints(), recent: std::cell::Cell::new(None) })
        });
        if let Some((recent, position)) = index.recent.get()
            && recent == pc
        {
            return position;
        }
        let checkpoints = &index.checkpoints;
        let start = checkpoints.partition_point(|checkpoint| checkpoint.pc <= pc);
        let iter = match start.checked_sub(1) {
            Some(index) => self.iter_from(checkpoints[index].offset, checkpoints[index].previous),
            None => self.iter(),
        };
        let found = iter.take_while(|position| position.pc <= pc).last();
        index.recent.set(Some((pc, found)));
        found
    }

    /// A checkpoint at every `SOURCE_POSITION_CHECKPOINT_INTERVAL`th entry after the first;
    /// none for a table shorter than one interval, which decodes as fast from its start.
    fn make_checkpoints(&self) -> Box<[SourcePositionCheckpoint]> {
        let mut checkpoints = Vec::new();
        let mut iter = self.iter();
        let mut entry = 0;
        loop {
            let offset = self.bytes.len() - iter.bytes.len();
            let previous = iter.previous;
            let Some(position) = iter.next() else { break };
            if entry > 0 && entry % SOURCE_POSITION_CHECKPOINT_INTERVAL == 0 {
                checkpoints.push(SourcePositionCheckpoint { offset, previous, pc: position.pc });
            }
            entry += 1;
        }
        checkpoints.into_boxed_slice()
    }

    pub(crate) fn shrink_to_fit(&mut self) {
        self.bytes.shrink_to_fit();
    }

    pub(crate) fn capacity_bytes(&self) -> usize {
        self.bytes.capacity()
    }
}

impl FromIterator<SourcePosition> for SourcePositions {
    fn from_iter<I: IntoIterator<Item = SourcePosition>>(positions: I) -> Self {
        let mut table = Self::default();
        for position in positions {
            table.push(position);
        }
        table
    }
}

fn zigzag(value: i64) -> u64 {
    ((value << 1) ^ (value >> (i64::BITS - 1))) as u64
}

fn unzigzag(value: u64) -> i64 {
    (value >> 1) as i64 ^ -((value & 1) as i64)
}

/// Decodes a `SourcePositions` table in order.
#[derive(Clone)]
pub(crate) struct SourcePositionIter<'a> {
    bytes: &'a [u8],
    previous: SourcePosition,
}

impl SourcePositionIter<'_> {
    fn read(&mut self) -> u64 {
        let mut value = 0;
        let mut shift = 0;
        while let Some((&byte, rest)) = self.bytes.split_first() {
            self.bytes = rest;
            // A stored table is validated after decoding; an overlong varint decodes to junk
            // the validation rejects rather than shifting past the value's width.
            value |= u64::from(byte & !LEB128_CONTINUATION).checked_shl(shift).unwrap_or(0);
            if byte & LEB128_CONTINUATION == 0 {
                break;
            }
            shift += LEB128_PAYLOAD_BITS;
        }
        value
    }
}

impl Iterator for SourcePositionIter<'_> {
    type Item = SourcePosition;

    fn next(&mut self) -> Option<SourcePosition> {
        if self.bytes.is_empty() {
            return None;
        }
        let previous = self.previous;
        let pc = previous.pc.wrapping_add(self.read() as u32);
        let line_change = unzigzag(self.read());
        let column = self.read();
        let column = if line_change == 0 {
            (i64::from(previous.column) + unzigzag(column)) as u32
        } else {
            column as u32
        };
        let line = (i64::from(previous.line) + line_change) as u32;
        self.previous = SourcePosition { pc, line, column };
        Some(self.previous)
    }
}
