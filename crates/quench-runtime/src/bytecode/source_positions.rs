//! Source position tables, delta-coded per function, and their decoding.

use super::SourceOffset;

/// A function's source positions in increasing pc order, each delta-coded against the one
/// before it as LEB128 varints: the pc advance, then the source byte offset's change (zigzag).
/// Most entries take two bytes; a function keeps one per source location change for as long as
/// it lives. Lines and columns are derived from the offsets only when something asks for them
/// (`ProgramSource::position`).
///
/// A frozen table: its coded bytes are boxed to their exact length. A compile grows them in a
/// `SourcePositionsBuilder` first, and a rewrite that relocates the entries builds a new table.
#[derive(Clone, Debug, Default)]
pub(crate) struct SourcePositions {
    bytes: Box<[u8]>,
    /// The final entry, which the next one is coded against.
    last: Option<SourceOffset>,
    /// Where decoding may start, made on the first lookup of a table longer than one interval
    /// (`SourcePositions::at`): a stack trace through a large function otherwise decoded its
    /// table from the start for every frame (Babel: about 20K instructions a frame).
    checkpoints: std::cell::OnceCell<Box<SourcePositionIndex>>,
}

/// The entries of a function being compiled: the coded bytes in a growable buffer, which
/// `freeze` boxes.
#[derive(Clone, Debug, Default)]
pub(crate) struct SourcePositionsBuilder {
    bytes: Vec<u8>,
    last: Option<SourceOffset>,
}

/// A table's lookup memo: its checkpoints and its last lookup's pc and answer. A throw site
/// that a loop runs again, and each frame under it, asks for the same pc every time.
#[derive(Clone, Debug)]
struct SourcePositionIndex {
    checkpoints: Box<[SourcePositionCheckpoint]>,
    recent: std::cell::Cell<Option<(u32, Option<u32>)>>,
}

/// The entries between two `SourcePositions` checkpoints.
const SOURCE_POSITION_CHECKPOINT_INTERVAL: usize = 32;

/// A point decoding may resume from: the byte offset of an entry in the coded bytes, the
/// entry before it (which it is coded against) and the entry's own pc.
#[derive(Clone, Copy, Debug)]
struct SourcePositionCheckpoint {
    at: usize,
    previous: SourceOffset,
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
    pub(crate) fn last(&self) -> Option<SourceOffset> {
        self.last
    }

    /// The coded entries and the final one, as a residual stores them
    /// (`BinaryWriter::source_positions`).
    pub(crate) fn encoded(&self) -> (&[u8], Option<SourceOffset>) {
        (&self.bytes[..], self.last)
    }

    /// The table a residual stored (`encoded`); validation judges it against its function.
    pub(crate) fn from_encoded(bytes: Vec<u8>, last: Option<SourceOffset>) -> Self {
        Self { bytes: bytes.into_boxed_slice(), last, checkpoints: std::cell::OnceCell::new() }
    }

    pub(crate) fn iter(&self) -> SourcePositionIter<'_> {
        decode_from(&self.bytes, 0, SourceOffset { pc: 0, offset: 0 })
    }

    /// The source byte offset in effect at `pc`: the last entry at or before it, decoded from
    /// the last checkpoint at or before it.
    pub(crate) fn at(&self, pc: u32) -> Option<u32> {
        let index = self.checkpoints.get_or_init(|| {
            Box::new(SourcePositionIndex { checkpoints: make_checkpoints(&self.bytes), recent: std::cell::Cell::new(None) })
        });
        if let Some((recent, offset)) = index.recent.get()
            && recent == pc
        {
            return offset;
        }
        let checkpoints = &index.checkpoints;
        let start = checkpoints.partition_point(|checkpoint| checkpoint.pc <= pc);
        let iter = match start.checked_sub(1) {
            Some(index) => decode_from(&self.bytes, checkpoints[index].at, checkpoints[index].previous),
            None => self.iter(),
        };
        let found = iter.take_while(|entry| entry.pc <= pc).last().map(|entry| entry.offset);
        index.recent.set(Some((pc, found)));
        found
    }

    /// The bytes the entries are coded in.
    pub(crate) fn coded_len(&self) -> usize {
        self.bytes.len()
    }

    pub(crate) fn capacity_bytes(&self) -> usize {
        self.bytes.len()
    }
}

impl SourcePositionsBuilder {
    pub(crate) fn last(&self) -> Option<SourceOffset> {
        self.last
    }

    /// Appends `entry`. Compiled code orders entries by pc.
    pub(crate) fn push(&mut self, entry: SourceOffset) {
        let previous = self.last.unwrap_or(SourceOffset { pc: 0, offset: 0 });
        self.write(u64::from(entry.pc.wrapping_sub(previous.pc)));
        self.write(zigzag(i64::from(entry.offset) - i64::from(previous.offset)));
        self.last = Some(entry);
    }

    /// An empty table with room for `bytes` coded bytes.
    pub(crate) fn with_capacity(bytes: usize) -> Self {
        Self { bytes: Vec::with_capacity(bytes), last: None }
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

    /// The table of the entries pushed so far, its bytes boxed to their exact length.
    pub(crate) fn freeze(self) -> SourcePositions {
        SourcePositions { bytes: self.bytes.into_boxed_slice(), last: self.last, checkpoints: std::cell::OnceCell::new() }
    }
}

/// Decodes `bytes` from byte `at`, where the entry before the one decoded first is `previous`.
fn decode_from(bytes: &[u8], at: usize, previous: SourceOffset) -> SourcePositionIter<'_> {
    SourcePositionIter { bytes: &bytes[at..], previous }
}

/// A checkpoint at every `SOURCE_POSITION_CHECKPOINT_INTERVAL`th entry after the first; none for
/// a table shorter than one interval, which decodes as fast from its start.
fn make_checkpoints(bytes: &[u8]) -> Box<[SourcePositionCheckpoint]> {
    let mut checkpoints = Vec::new();
    let mut iter = decode_from(bytes, 0, SourceOffset { pc: 0, offset: 0 });
    let mut entry = 0;
    loop {
        let at = bytes.len() - iter.bytes.len();
        let previous = iter.previous;
        let Some(decoded) = iter.next() else { break };
        if entry > 0 && entry % SOURCE_POSITION_CHECKPOINT_INTERVAL == 0 {
            checkpoints.push(SourcePositionCheckpoint { at, previous, pc: decoded.pc });
        }
        entry += 1;
    }
    checkpoints.into_boxed_slice()
}

impl FromIterator<SourceOffset> for SourcePositionsBuilder {
    fn from_iter<I: IntoIterator<Item = SourceOffset>>(positions: I) -> Self {
        let mut table = Self::default();
        for position in positions {
            table.push(position);
        }
        table
    }
}

impl FromIterator<SourceOffset> for SourcePositions {
    fn from_iter<I: IntoIterator<Item = SourceOffset>>(positions: I) -> Self {
        positions.into_iter().collect::<SourcePositionsBuilder>().freeze()
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
    previous: SourceOffset,
}

impl SourcePositionIter<'_> {
    #[inline(always)]
    fn read(&mut self) -> u64 {
        // Most deltas are one byte: no continuation, no loop.
        if let Some((&byte, rest)) = self.bytes.split_first()
            && byte < LEB128_CONTINUATION
        {
            self.bytes = rest;
            return u64::from(byte);
        }
        self.read_multi_byte()
    }

    fn read_multi_byte(&mut self) -> u64 {
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
    type Item = SourceOffset;

    fn next(&mut self) -> Option<SourceOffset> {
        if self.bytes.is_empty() {
            return None;
        }
        let previous = self.previous;
        let pc = previous.pc.wrapping_add(self.read() as u32);
        let offset = (i64::from(previous.offset) + unzigzag(self.read())) as u32;
        self.previous = SourceOffset { pc, offset };
        Some(self.previous)
    }
}
