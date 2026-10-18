//! DNS message encoding and decoding (RFC 1035 and the record types Node resolves).
//! Decoding validates the whole message like c-ares' `ares_dns_parse`: a malformed
//! header, name, question or record of a known type rejects the response.

/// Fixed DNS header length.
const HEADER_LEN: usize = 12;
/// Question type and class after the name.
const QUESTION_FIXED_LEN: usize = 4;
/// Longest label and longest encoded name (RFC 1035 §2.3.4).
const MAX_LABEL_LEN: usize = 63;
const MAX_NAME_LEN: usize = 255;
/// Top two bits of a length byte mark a compression pointer.
const POINTER_MASK: u8 = 0xC0;
const POINTER_OFFSET_MASK: u16 = 0x3FFF;
/// Header flag bits.
const FLAG_QR: u16 = 0x8000;
const FLAG_TC: u16 = 0x0200;
const FLAG_RD: u16 = 0x0100;
const RCODE_MASK: u16 = 0x000F;
/// The Internet class.
pub(crate) const CLASS_IN: u16 = 1;
/// c-ares' default EDNS(0) advertised UDP payload (`EDNSPACKETSZ`).
pub(crate) const EDNS_PAYLOAD: u16 = 1232;
const IPV4_LEN: usize = 4;
const IPV6_LEN: usize = 16;

/// Record types, by their IANA numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub(crate) enum RecordType {
    A = 1,
    Ns = 2,
    Cname = 5,
    Soa = 6,
    Ptr = 12,
    Mx = 15,
    Txt = 16,
    Aaaa = 28,
    Srv = 33,
    Naptr = 35,
    Opt = 41,
    Tlsa = 52,
    Any = 255,
    Caa = 257,
}

impl RecordType {
    const ALL: [Self; 14] = [
        Self::A, Self::Ns, Self::Cname, Self::Soa, Self::Ptr, Self::Mx, Self::Txt, Self::Aaaa,
        Self::Srv, Self::Naptr, Self::Opt, Self::Tlsa, Self::Any, Self::Caa,
    ];

    fn from_number(number: u16) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| *kind as u16 == number)
    }
}

/// Response codes with a distinct resolver outcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResponseCode {
    NoError,
    FormErr,
    ServFail,
    NxDomain,
    NotImp,
    Refused,
    Other(u8),
}

impl ResponseCode {
    fn from_bits(bits: u16) -> Self {
        match bits & RCODE_MASK {
            0 => Self::NoError,
            1 => Self::FormErr,
            2 => Self::ServFail,
            3 => Self::NxDomain,
            4 => Self::NotImp,
            5 => Self::Refused,
            other => Self::Other(other as u8),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Soa {
    pub(crate) nsname: String,
    pub(crate) hostmaster: String,
    pub(crate) serial: u32,
    pub(crate) refresh: u32,
    pub(crate) retry: u32,
    pub(crate) expire: u32,
    pub(crate) minttl: u32,
}

/// Decoded record data; names are presentation text with c-ares escaping.
#[derive(Clone, Debug)]
pub(crate) enum RecordData {
    A([u8; IPV4_LEN]),
    Aaaa([u8; IPV6_LEN]),
    Name(String),
    Soa(Soa),
    Mx { priority: u16, exchange: String },
    Txt(Vec<Vec<u8>>),
    Srv { priority: u16, weight: u16, port: u16, target: String },
    Naptr { order: u16, preference: u16, flags: Vec<u8>, service: Vec<u8>, regexp: Vec<u8>, replacement: String },
    Tlsa { usage: u8, selector: u8, matching: u8, data: Vec<u8> },
    Caa { flags: u8, tag: Vec<u8>, value: Vec<u8> },
    Other,
}

#[derive(Clone, Debug)]
pub(crate) struct Record {
    pub(crate) kind: Option<RecordType>,
    pub(crate) class: u16,
    pub(crate) ttl: u32,
    pub(crate) data: RecordData,
}

pub(crate) struct Response {
    pub(crate) id: u16,
    pub(crate) truncated: bool,
    pub(crate) code: ResponseCode,
    /// The first question's name, as presentation text.
    pub(crate) question: Option<String>,
    pub(crate) answers: Vec<Record>,
    /// The response carried an EDNS(0) OPT record.
    pub(crate) has_opt: bool,
}

/// Encoding a query name failed: an empty interior label, an over-long label or name, or
/// a malformed escape.
pub(crate) struct BadName;

/// Encode a recursive query for `name` (presentation form, escapes allowed).
pub(crate) fn encode_query(id: u16, name: &str, kind: RecordType, edns: bool) -> Result<Vec<u8>, BadName> {
    let mut message = Vec::with_capacity(HEADER_LEN + name.len() + 2 + QUESTION_FIXED_LEN);
    message.extend_from_slice(&id.to_be_bytes());
    message.extend_from_slice(&FLAG_RD.to_be_bytes());
    let additional = u16::from(edns);
    for count in [1, 0, 0, additional] {
        message.extend_from_slice(&u16::to_be_bytes(count));
    }
    encode_name(name, &mut message)?;
    message.extend_from_slice(&(kind as u16).to_be_bytes());
    message.extend_from_slice(&CLASS_IN.to_be_bytes());
    if edns {
        // Root owner, OPT type, payload size as class, zero extended flags and no options.
        message.push(0);
        message.extend_from_slice(&(RecordType::Opt as u16).to_be_bytes());
        message.extend_from_slice(&EDNS_PAYLOAD.to_be_bytes());
        message.extend_from_slice(&0u32.to_be_bytes());
        message.extend_from_slice(&0u16.to_be_bytes());
    }
    Ok(message)
}

fn encode_name(name: &str, out: &mut Vec<u8>) -> Result<(), BadName> {
    let start = out.len();
    let name = name.strip_suffix('.').unwrap_or(name);
    if !name.is_empty() {
        let bytes = name.as_bytes();
        let mut label = Vec::new();
        let mut index = 0;
        loop {
            let end = index == bytes.len();
            if end || bytes[index] == b'.' {
                if label.is_empty() || label.len() > MAX_LABEL_LEN {
                    return Err(BadName);
                }
                out.push(label.len() as u8);
                out.append(&mut label);
                if end {
                    break;
                }
                index += 1;
                continue;
            }
            if bytes[index] == b'\\' {
                let (byte, consumed) = unescape(&bytes[index + 1..]).ok_or(BadName)?;
                label.push(byte);
                index += 1 + consumed;
            } else {
                label.push(bytes[index]);
                index += 1;
            }
        }
    }
    out.push(0);
    if out.len() - start > MAX_NAME_LEN {
        return Err(BadName);
    }
    Ok(())
}

/// A presentation escape after its backslash: `\DDD` or a literal character.
fn unescape(rest: &[u8]) -> Option<(u8, usize)> {
    const DECIMAL_ESCAPE_DIGITS: usize = 3;
    let first = *rest.first()?;
    if !first.is_ascii_digit() {
        return Some((first, 1));
    }
    let digits = rest.get(..DECIMAL_ESCAPE_DIGITS)?;
    if !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let value = digits.iter().fold(0u32, |value, digit| value * 10 + u32::from(digit - b'0'));
    u8::try_from(value).ok().map(|byte| (byte, DECIMAL_ESCAPE_DIGITS))
}

/// The response failed validation (c-ares `ARES_EBADRESP`).
#[derive(Debug)]
pub(crate) struct BadResponse;

struct Reader<'a> {
    message: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], BadResponse> {
        let end = self.offset.checked_add(length).ok_or(BadResponse)?;
        let bytes = self.message.get(self.offset..end).ok_or(BadResponse)?;
        self.offset = end;
        Ok(bytes)
    }

    fn u8(&mut self) -> Result<u8, BadResponse> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, BadResponse> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Result<u32, BadResponse> {
        let bytes = self.take(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// A possibly compressed name, as presentation text without the trailing dot.
    fn name(&mut self) -> Result<String, BadResponse> {
        let mut text = String::new();
        let mut offset = self.offset;
        let mut resume = None;
        let mut total = 0usize;
        // Every pointer must move strictly backwards, which bounds the walk.
        let mut limit = offset;
        loop {
            let length = *self.message.get(offset).ok_or(BadResponse)?;
            if length & POINTER_MASK == POINTER_MASK {
                let low = *self.message.get(offset + 1).ok_or(BadResponse)?;
                let target = usize::from(u16::from_be_bytes([length, low]) & POINTER_OFFSET_MASK);
                if target >= limit {
                    return Err(BadResponse);
                }
                resume.get_or_insert(offset + 2);
                limit = target;
                offset = target;
                continue;
            }
            if length & POINTER_MASK != 0 {
                return Err(BadResponse);
            }
            offset += 1;
            if length == 0 {
                break;
            }
            let label = self.message.get(offset..offset + usize::from(length)).ok_or(BadResponse)?;
            total += usize::from(length) + 1;
            if total > MAX_NAME_LEN {
                return Err(BadResponse);
            }
            if !text.is_empty() {
                text.push('.');
            }
            escape_label(label, &mut text);
            offset += usize::from(length);
        }
        self.offset = resume.unwrap_or(offset);
        Ok(text)
    }

    /// One length-prefixed character string.
    fn character_string(&mut self) -> Result<Vec<u8>, BadResponse> {
        let length = self.u8()?;
        Ok(self.take(usize::from(length))?.to_vec())
    }
}

/// c-ares presentation escaping: non-printable bytes as `\DDD`, reserved characters
/// backslash-escaped.
fn escape_label(label: &[u8], text: &mut String) {
    const PRINTABLE: std::ops::RangeInclusive<u8> = 0x20..=0x7E;
    for &byte in label {
        if !PRINTABLE.contains(&byte) {
            text.push_str(&format!("\\{byte:03}"));
            continue;
        }
        if matches!(byte, b'"' | b'.' | b';' | b'\\' | b'(' | b')' | b'@' | b'$') {
            text.push('\\');
        }
        text.push(char::from(byte));
    }
}

pub(crate) fn decode(message: &[u8]) -> Result<Response, BadResponse> {
    let mut reader = Reader { message, offset: 0 };
    let id = reader.u16()?;
    let flags = reader.u16()?;
    if flags & FLAG_QR == 0 {
        return Err(BadResponse);
    }
    let questions = reader.u16()?;
    let answers = reader.u16()?;
    let authorities = reader.u16()?;
    let additional = reader.u16()?;
    let mut question = None;
    for _ in 0..questions {
        let name = reader.name()?;
        reader.take(QUESTION_FIXED_LEN)?;
        question.get_or_insert(name);
    }
    let mut records = Vec::with_capacity(usize::from(answers));
    for _ in 0..answers {
        records.push(record(&mut reader)?);
    }
    let mut has_opt = false;
    for _ in 0..u32::from(authorities) + u32::from(additional) {
        has_opt |= record(&mut reader)?.kind == Some(RecordType::Opt);
    }
    Ok(Response {
        id,
        truncated: flags & FLAG_TC != 0,
        code: ResponseCode::from_bits(flags),
        question,
        answers: records,
        has_opt,
    })
}

fn record(reader: &mut Reader<'_>) -> Result<Record, BadResponse> {
    reader.name()?;
    let kind = RecordType::from_number(reader.u16()?);
    let class = reader.u16()?;
    let ttl = reader.u32()?;
    let length = usize::from(reader.u16()?);
    let end = reader.offset + length;
    if end > reader.message.len() {
        return Err(BadResponse);
    }
    let data = match kind {
        Some(RecordType::A) => RecordData::A(reader.take(IPV4_LEN)?.try_into().map_err(|_| BadResponse)?),
        Some(RecordType::Aaaa) => RecordData::Aaaa(reader.take(IPV6_LEN)?.try_into().map_err(|_| BadResponse)?),
        Some(RecordType::Ns | RecordType::Cname | RecordType::Ptr) => RecordData::Name(reader.name()?),
        Some(RecordType::Soa) => RecordData::Soa(Soa {
            nsname: reader.name()?,
            hostmaster: reader.name()?,
            serial: reader.u32()?,
            refresh: reader.u32()?,
            retry: reader.u32()?,
            expire: reader.u32()?,
            minttl: reader.u32()?,
        }),
        Some(RecordType::Mx) => RecordData::Mx { priority: reader.u16()?, exchange: reader.name()? },
        Some(RecordType::Txt) => {
            let mut strings = Vec::new();
            while reader.offset < end {
                strings.push(reader.character_string()?);
            }
            RecordData::Txt(strings)
        }
        Some(RecordType::Srv) => RecordData::Srv {
            priority: reader.u16()?,
            weight: reader.u16()?,
            port: reader.u16()?,
            target: reader.name()?,
        },
        Some(RecordType::Naptr) => RecordData::Naptr {
            order: reader.u16()?,
            preference: reader.u16()?,
            flags: reader.character_string()?,
            service: reader.character_string()?,
            regexp: reader.character_string()?,
            replacement: reader.name()?,
        },
        Some(RecordType::Tlsa) => RecordData::Tlsa {
            usage: reader.u8()?,
            selector: reader.u8()?,
            matching: reader.u8()?,
            data: reader.take(end.checked_sub(reader.offset).ok_or(BadResponse)?)?.to_vec(),
        },
        Some(RecordType::Caa) => {
            let flags = reader.u8()?;
            let tag = reader.character_string()?;
            let value = reader.take(end.checked_sub(reader.offset).ok_or(BadResponse)?)?.to_vec();
            RecordData::Caa { flags, tag, value }
        }
        Some(RecordType::Opt | RecordType::Any) | None => {
            reader.take(length)?;
            RecordData::Other
        }
    };
    if reader.offset != end {
        return Err(BadResponse);
    }
    Ok(Record { kind, class, ttl, data })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_names_reject_bad_labels_and_accept_escapes() {
        assert!(encode_query(1, "a..b", RecordType::A, false).is_err());
        assert!(encode_query(1, &"x".repeat(MAX_LABEL_LEN + 1), RecordType::A, false).is_err());
        let query = encode_query(7, "a\\.b.test.", RecordType::Txt, true).ok().unwrap();
        assert_eq!(&query[HEADER_LEN..HEADER_LEN + 10], b"\x03a.b\x04test\x00");
    }

    #[test]
    fn compressed_names_must_point_backwards() {
        let mut message = vec![0, 1, 0x81, 0x80, 0, 1, 0, 0, 0, 0, 0, 0];
        message.extend_from_slice(&[0xC0, 12, 0, 1, 0, 1]);
        assert!(decode(&message).is_err());
    }
}
