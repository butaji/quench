//! WHATWG label resolution and stateless decoding shared by Node adapters.

pub(crate) fn decode_bytes(encoding: &str, bytes: &[u8], fatal: bool) -> Result<String, ()> {
    #[cfg(quench_icu)]
    if needs_icu(encoding) {
        // Node follows the Encoding Standard's GBK decoder alias while keeping
        // the public canonical label as gbk.
        let converter = if encoding == "gbk" {
            "gb18030"
        } else {
            encoding
        };
        return super::text_decoder_icu::decode(converter, bytes, fatal);
    }
    let codec = encoding_rs::Encoding::for_label_no_replacement(encoding.as_bytes()).ok_or(())?;
    // TextDecoder handles its matching BOM through ignoreBOM. Codec sniffing
    // would incorrectly switch an explicitly requested UTF-8/UTF-16 encoding.
    if fatal {
        codec
            .decode_without_bom_handling_and_without_replacement(bytes)
            .map(|text| text.into_owned())
            .ok_or(())
    } else {
        Ok(codec.decode_without_bom_handling(bytes).0.into_owned())
    }
}

pub(crate) fn canonical_encoding(label: &str) -> Option<String> {
    encoding_rs::Encoding::for_label(label.as_bytes())
        .map(|encoding| encoding.name().to_ascii_lowercase())
        .filter(|encoding| cfg!(quench_icu) || !needs_icu(encoding))
}

fn needs_icu(encoding: &str) -> bool {
    encoding_rs::Encoding::for_label(encoding.as_bytes()).is_some_and(|codec| {
        !codec.is_single_byte()
            && !matches!(encoding, "utf-8" | "utf-16le" | "utf-16be" | "replacement")
    })
}

/// The UTF-8 sequence bounds of the Encoding Standard's decoder: the byte range a lead
/// byte's second byte must fall in, and how many continuation bytes the lead needs.
fn utf8_lead(lead: u8) -> Option<(std::ops::RangeInclusive<u8>, usize)> {
    match lead {
        0xC2..=0xDF => Some((0x80..=0xBF, 1)),
        0xE0 => Some((0xA0..=0xBF, 2)),
        0xED => Some((0x80..=0x9F, 2)),
        0xE1..=0xEC | 0xEE..=0xEF => Some((0x80..=0xBF, 2)),
        0xF0 => Some((0x90..=0xBF, 3)),
        0xF4 => Some((0x80..=0x8F, 3)),
        0xF1..=0xF3 => Some((0x80..=0xBF, 3)),
        _ => None,
    }
}

/// The longest UTF-8 sequence a lead byte starts, and the continuation byte range.
const UTF8_MAX_SEQUENCE: usize = 4;
const UTF8_CONTINUATION: std::ops::RangeInclusive<u8> = 0x80..=0xBF;
/// UTF-16's code unit width, and the high (lead) surrogate range.
const UTF16_UNIT: usize = 2;
const HIGH_SURROGATES: std::ops::RangeInclusive<u16> = 0xD800..=0xDBFF;

/// How many trailing bytes of `bytes` begin a character a later chunk may complete: a
/// streaming decode keeps them for the next call. Encodings without multi-byte sequences
/// keep none.
pub(crate) fn incomplete_tail(encoding: &str, bytes: &[u8]) -> usize {
    match encoding {
        "utf-8" => {
            let start = bytes.len().saturating_sub(UTF8_MAX_SEQUENCE - 1);
            for lead_index in (start..bytes.len()).rev() {
                let Some((second, continuation)) = utf8_lead(bytes[lead_index]) else { continue };
                let tail = &bytes[lead_index + 1..];
                let valid = tail
                    .iter()
                    .enumerate()
                    .all(|(index, byte)| if index == 0 { second.contains(byte) } else { UTF8_CONTINUATION.contains(byte) });
                return if valid && tail.len() < continuation { bytes.len() - lead_index } else { 0 };
            }
            0
        }
        "utf-16le" | "utf-16be" => {
            let odd = bytes.len() % UTF16_UNIT;
            let whole = bytes.len() - odd;
            if whole < UTF16_UNIT {
                return odd;
            }
            let unit = [bytes[whole - UTF16_UNIT], bytes[whole - 1]];
            let unit = if encoding == "utf-16le" { u16::from_le_bytes(unit) } else { u16::from_be_bytes(unit) };
            if HIGH_SURROGATES.contains(&unit) { odd + UTF16_UNIT } else { odd }
        }
        _ => 0,
    }
}
