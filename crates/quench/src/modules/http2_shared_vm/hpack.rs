//! HPACK (RFC 7541): the static table, a dynamic table per direction, and the field
//! representations a header block is a sequence of. The encoder chooses representations
//! as nghttp2 does, so peers see the same indexing a Node peer would produce.

use super::huffman;
use std::collections::VecDeque;

/// A header field: its name and value octets, and whether it must never be indexed
/// (Node's `sensitiveHeaders`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Field {
    pub(super) name: Vec<u8>,
    pub(super) value: Vec<u8>,
    pub(super) sensitive: bool,
}

/// The static table (RFC 7541 Appendix A); index 1 is its first entry.
const STATIC_TABLE: [(&str, &str); 61] = [
    (":authority", ""),
    (":method", "GET"),
    (":method", "POST"),
    (":path", "/"),
    (":path", "/index.html"),
    (":scheme", "http"),
    (":scheme", "https"),
    (":status", "200"),
    (":status", "204"),
    (":status", "206"),
    (":status", "304"),
    (":status", "400"),
    (":status", "404"),
    (":status", "500"),
    ("accept-charset", ""),
    ("accept-encoding", "gzip, deflate"),
    ("accept-language", ""),
    ("accept-ranges", ""),
    ("accept", ""),
    ("access-control-allow-origin", ""),
    ("age", ""),
    ("allow", ""),
    ("authorization", ""),
    ("cache-control", ""),
    ("content-disposition", ""),
    ("content-encoding", ""),
    ("content-language", ""),
    ("content-length", ""),
    ("content-location", ""),
    ("content-range", ""),
    ("content-type", ""),
    ("cookie", ""),
    ("date", ""),
    ("etag", ""),
    ("expect", ""),
    ("expires", ""),
    ("from", ""),
    ("host", ""),
    ("if-match", ""),
    ("if-modified-since", ""),
    ("if-none-match", ""),
    ("if-range", ""),
    ("if-unmodified-since", ""),
    ("last-modified", ""),
    ("link", ""),
    ("location", ""),
    ("max-forwards", ""),
    ("proxy-authenticate", ""),
    ("proxy-authorization", ""),
    ("range", ""),
    ("referer", ""),
    ("refresh", ""),
    ("retry-after", ""),
    ("server", ""),
    ("set-cookie", ""),
    ("strict-transport-security", ""),
    ("transfer-encoding", ""),
    ("user-agent", ""),
    ("vary", ""),
    ("via", ""),
    ("www-authenticate", ""),
];

/// The octets an entry costs beyond its name and value (RFC 7541 §4.1).
const ENTRY_OVERHEAD: usize = 32;
/// The dynamic table size both directions start with (SETTINGS_HEADER_TABLE_SIZE).
pub(super) const DEFAULT_TABLE_SIZE: usize = 4096;

/// A representation's leading bits and the width of the integer prefix that follows them
/// (RFC 7541 §6).
#[derive(Clone, Copy)]
struct Representation {
    pattern: u8,
    prefix: u32,
}

const INDEXED: Representation = Representation { pattern: 0x80, prefix: 7 };
const INCREMENTAL: Representation = Representation { pattern: 0x40, prefix: 6 };
const SIZE_UPDATE: Representation = Representation { pattern: 0x20, prefix: 5 };
const NEVER_INDEXED: Representation = Representation { pattern: 0x10, prefix: 4 };
const WITHOUT_INDEXING: Representation = Representation { pattern: 0x00, prefix: 4 };
/// A string literal's Huffman flag, ahead of its 7-bit length prefix.
const HUFFMAN_FLAG: u8 = 0x80;
const STRING_LENGTH_PREFIX: u32 = 7;
/// The bits each integer continuation octet carries, and its continuation flag.
const CONTINUATION_BITS: u32 = 7;
const CONTINUATION_FLAG: u8 = 0x80;

impl Representation {
    fn matches(self, byte: u8) -> bool {
        let mask = !((1u16 << self.prefix) - 1) as u8;
        byte & mask == self.pattern
    }
}

/// Names nghttp2 never adds to its dynamic table: values that rarely repeat.
const UNINDEXED_NAMES: [&[u8]; 8] = [
    b":path",
    b"age",
    b"content-length",
    b"etag",
    b"if-modified-since",
    b"if-none-match",
    b"location",
    b"set-cookie",
];
/// nghttp2 sends `authorization`, and cookies shorter than this, as never indexed.
const SHORT_COOKIE: usize = 20;
/// nghttp2 indexes an entry only when it fills at most this share of the table.
const INDEXED_SHARE: (usize, usize) = (3, 4);

fn entry_size(name: &[u8], value: &[u8]) -> usize {
    name.len() + value.len() + ENTRY_OVERHEAD
}

/// A dynamic table: the newest entry first, evicted from the back to fit `capacity`.
#[derive(Default)]
struct DynamicTable {
    entries: VecDeque<(Vec<u8>, Vec<u8>)>,
    size: usize,
    capacity: usize,
}

impl DynamicTable {
    fn new() -> Self {
        Self { capacity: DEFAULT_TABLE_SIZE, ..Self::default() }
    }

    fn resize(&mut self, capacity: usize) {
        self.capacity = capacity;
        self.evict_to(capacity);
    }

    fn evict_to(&mut self, limit: usize) {
        while self.size > limit {
            let Some((name, value)) = self.entries.pop_back() else { break };
            self.size -= entry_size(&name, &value);
        }
    }

    fn insert(&mut self, name: Vec<u8>, value: Vec<u8>) {
        let size = entry_size(&name, &value);
        if size > self.capacity {
            self.entries.clear();
            self.size = 0;
            return;
        }
        self.evict_to(self.capacity - size);
        self.size += size;
        self.entries.push_front((name, value));
    }

    /// Entry `index` of the combined index space (RFC 7541 §2.3.3).
    fn get(&self, index: usize) -> Option<(&[u8], &[u8])> {
        match index {
            0 => None,
            index if index <= STATIC_TABLE.len() => {
                let (name, value) = STATIC_TABLE[index - 1];
                Some((name.as_bytes(), value.as_bytes()))
            }
            index => self.entries.get(index - STATIC_TABLE.len() - 1).map(|(name, value)| (&name[..], &value[..])),
        }
    }

    /// The index of an entry holding `name` and `value`, else of one holding `name`.
    fn search(&self, name: &[u8], value: &[u8]) -> (Option<usize>, Option<usize>) {
        let statics = STATIC_TABLE.iter().map(|(name, value)| (name.as_bytes(), value.as_bytes()));
        let dynamics = self.entries.iter().map(|(name, value)| (&name[..], &value[..]));
        let mut name_index = None;
        for (index, (entry_name, entry_value)) in statics.chain(dynamics).enumerate() {
            if entry_name == name {
                if entry_value == value {
                    return (Some(index + 1), name_index.or(Some(index + 1)));
                }
                name_index = name_index.or(Some(index + 1));
            }
        }
        (None, name_index)
    }
}

fn encode_integer(value: usize, representation: Representation, out: &mut Vec<u8>) {
    let limit = (1usize << representation.prefix) - 1;
    if value < limit {
        out.push(representation.pattern | value as u8);
        return;
    }
    out.push(representation.pattern | limit as u8);
    let mut rest = value - limit;
    while rest >= usize::from(CONTINUATION_FLAG) {
        out.push((rest as u8 & !CONTINUATION_FLAG) | CONTINUATION_FLAG);
        rest >>= CONTINUATION_BITS;
    }
    out.push(rest as u8);
}

fn encode_string(bytes: &[u8], out: &mut Vec<u8>) {
    let compressed = huffman::encoded_length(bytes);
    if compressed < bytes.len() {
        encode_integer(compressed, Representation { pattern: HUFFMAN_FLAG, prefix: STRING_LENGTH_PREFIX }, out);
        huffman::encode(bytes, out);
    } else {
        encode_integer(bytes.len(), Representation { pattern: 0, prefix: STRING_LENGTH_PREFIX }, out);
        out.extend_from_slice(bytes);
    }
}

/// A malformed header block: a COMPRESSION_ERROR (RFC 9113 §4.3).
#[derive(Debug, PartialEq, Eq)]
pub(super) struct CompressionError;

/// The bytes of a block a decoder reads from.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn integer(&mut self, prefix: u32) -> Result<usize, CompressionError> {
        let limit = (1usize << prefix) - 1;
        let first = usize::from(*self.bytes.get(self.at).ok_or(CompressionError)?) & limit;
        self.at += 1;
        if first < limit {
            return Ok(first);
        }
        let mut value = limit;
        let mut shift = 0u32;
        loop {
            let byte = *self.bytes.get(self.at).ok_or(CompressionError)?;
            self.at += 1;
            let part = usize::from(byte & !CONTINUATION_FLAG).checked_shl(shift).ok_or(CompressionError)?;
            value = value.checked_add(part).ok_or(CompressionError)?;
            if byte & CONTINUATION_FLAG == 0 {
                return Ok(value);
            }
            shift += CONTINUATION_BITS;
            if shift >= usize::BITS - CONTINUATION_BITS {
                return Err(CompressionError);
            }
        }
    }

    fn string(&mut self) -> Result<Vec<u8>, CompressionError> {
        let huffman = self.bytes.get(self.at).ok_or(CompressionError)? & HUFFMAN_FLAG != 0;
        let length = self.integer(STRING_LENGTH_PREFIX)?;
        let end = self.at.checked_add(length).filter(|&end| end <= self.bytes.len()).ok_or(CompressionError)?;
        let raw = &self.bytes[self.at..end];
        self.at = end;
        if huffman { huffman::decode(raw).ok_or(CompressionError) } else { Ok(raw.to_vec()) }
    }
}

/// The receiving direction: the peer's dynamic table, bounded by our acknowledged
/// SETTINGS_HEADER_TABLE_SIZE.
pub(super) struct Decoder {
    table: DynamicTable,
    limit: usize,
}

impl Decoder {
    pub(super) fn new() -> Self {
        Self { table: DynamicTable::new(), limit: DEFAULT_TABLE_SIZE }
    }

    /// The table size our acknowledged settings allow the peer.
    pub(super) fn set_limit(&mut self, limit: usize) {
        self.limit = limit;
        if self.table.capacity > limit {
            self.table.resize(limit);
        }
    }

    pub(super) fn table_size(&self) -> usize {
        self.table.size
    }

    pub(super) fn decode(&mut self, block: &[u8]) -> Result<Vec<Field>, CompressionError> {
        let mut reader = Reader { bytes: block, at: 0 };
        let mut fields = Vec::new();
        while reader.at < block.len() {
            let byte = block[reader.at];
            if INDEXED.matches(byte) {
                let index = reader.integer(INDEXED.prefix)?;
                let (name, value) = self.table.get(index).ok_or(CompressionError)?;
                fields.push(Field { name: name.to_vec(), value: value.to_vec(), sensitive: false });
                continue;
            }
            if SIZE_UPDATE.matches(byte) {
                let size = reader.integer(SIZE_UPDATE.prefix)?;
                if size > self.limit || !fields.is_empty() {
                    return Err(CompressionError);
                }
                self.table.resize(size);
                continue;
            }
            let representation = [INCREMENTAL, NEVER_INDEXED, WITHOUT_INDEXING]
                .into_iter()
                .find(|representation| representation.matches(byte))
                .ok_or(CompressionError)?;
            let index = reader.integer(representation.prefix)?;
            let name = if index == 0 {
                reader.string()?
            } else {
                self.table.get(index).ok_or(CompressionError)?.0.to_vec()
            };
            let value = reader.string()?;
            let sensitive = representation.pattern == NEVER_INDEXED.pattern;
            if representation.pattern == INCREMENTAL.pattern {
                self.table.insert(name.clone(), value.clone());
            }
            fields.push(Field { name, value, sensitive });
        }
        Ok(fields)
    }
}

/// The sending direction: our dynamic table, sized by the peer's SETTINGS_HEADER_TABLE_SIZE
/// up to the size we start with.
pub(super) struct Encoder {
    table: DynamicTable,
    /// The smallest size the table passed through since the last block, which the next
    /// block announces before its fields (RFC 7541 §4.2).
    pending_update: Option<usize>,
}

impl Encoder {
    pub(super) fn new() -> Self {
        Self { table: DynamicTable::new(), pending_update: None }
    }

    pub(super) fn set_peer_limit(&mut self, limit: usize) {
        let capacity = limit.min(DEFAULT_TABLE_SIZE);
        if capacity != self.table.capacity {
            self.pending_update = Some(self.pending_update.map_or(capacity, |pending| pending.min(capacity)));
            self.table.resize(capacity);
        }
    }

    pub(super) fn table_size(&self) -> usize {
        self.table.size
    }

    pub(super) fn encode(&mut self, fields: &[Field]) -> Vec<u8> {
        let mut out = Vec::new();
        if let Some(smallest) = self.pending_update.take() {
            encode_integer(smallest, SIZE_UPDATE, &mut out);
            if smallest != self.table.capacity {
                encode_integer(self.table.capacity, SIZE_UPDATE, &mut out);
            }
        }
        for field in fields {
            self.encode_field(field, &mut out);
        }
        out
    }

    fn encode_field(&mut self, field: &Field, out: &mut Vec<u8>) {
        let Field { name, value, sensitive } = field;
        let never = *sensitive
            || name == b"authorization"
            || (name == b"cookie" && value.len() < SHORT_COOKIE);
        let (exact, by_name) = self.table.search(name, value);
        if let (Some(index), false) = (exact, never) {
            encode_integer(index, INDEXED, out);
            return;
        }
        let indexing = !never
            && entry_size(name, value) * INDEXED_SHARE.1 <= self.table.capacity * INDEXED_SHARE.0
            && !UNINDEXED_NAMES.contains(&&name[..]);
        let representation = match (never, indexing) {
            (true, _) => NEVER_INDEXED,
            (false, true) => INCREMENTAL,
            (false, false) => WITHOUT_INDEXING,
        };
        encode_integer(by_name.unwrap_or(0), representation, out);
        if by_name.is_none() {
            encode_string(name, out);
        }
        encode_string(value, out);
        if indexing {
            self.table.insert(name.clone(), value.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(name: &str, value: &str) -> Field {
        Field { name: name.into(), value: value.into(), sensitive: false }
    }

    #[test]
    fn decodes_rfc_request_examples_with_huffman() {
        // RFC 7541 C.4.1 and C.4.2.
        let mut decoder = Decoder::new();
        let first = [
            0x82, 0x86, 0x84, 0x41, 0x8c, 0xf1, 0xe3, 0xc2, 0xe5, 0xf2, 0x3a, 0x6b, 0xa0, 0xab, 0x90, 0xf4, 0xff,
        ];
        assert_eq!(decoder.decode(&first).unwrap(), vec![
            field(":method", "GET"),
            field(":scheme", "http"),
            field(":path", "/"),
            field(":authority", "www.example.com"),
        ]);
        let second = [0x82, 0x86, 0x84, 0xbe, 0x58, 0x86, 0xa8, 0xeb, 0x10, 0x64, 0x9c, 0xbf];
        assert_eq!(decoder.decode(&second).unwrap(), vec![
            field(":method", "GET"),
            field(":scheme", "http"),
            field(":path", "/"),
            field(":authority", "www.example.com"),
            field("cache-control", "no-cache"),
        ]);
        assert_eq!(decoder.table_size(), 110);
    }

    #[test]
    fn encoder_round_trips_through_decoder() {
        let mut encoder = Encoder::new();
        let mut decoder = Decoder::new();
        let fields = vec![
            field(":status", "200"),
            field("content-type", "text/plain"),
            Field { name: b"x-secret".to_vec(), value: b"v".to_vec(), sensitive: true },
            field("authorization", "token"),
            field("x-long", &"a".repeat(300)),
        ];
        for _ in 0..3 {
            let block = encoder.encode(&fields);
            let decoded = decoder.decode(&block).unwrap();
            assert_eq!(decoded[..3], fields[..3]);
            assert!(decoded[3].sensitive);
            assert_eq!(decoded[4], fields[4]);
        }
        encoder.set_peer_limit(0);
        let block = encoder.encode(&fields);
        assert_eq!(decoder.decode(&block).unwrap()[1], fields[1]);
        assert_eq!(decoder.table_size(), 0);
    }

    #[test]
    fn rejects_truncated_blocks() {
        let mut decoder = Decoder::new();
        assert_eq!(decoder.decode(&[0x41, 0x8c, 0xf1]), Err(CompressionError));
        assert_eq!(decoder.decode(&[0xff]), Err(CompressionError));
        assert_eq!(decoder.decode(&[0x80]), Err(CompressionError));
    }
}
