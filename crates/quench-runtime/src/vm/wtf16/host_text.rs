//! UTF-16 units as lossy UTF-8 host text.

/// First UTF-16 unit that is not ASCII.
const FIRST_NON_ASCII: u16 = 0x80;

/// Units checked together for ASCII: their OR has no bit above 0x7F exactly when all are ASCII.
const ASCII_CHUNK: usize = 16;

/// The bits of a unit above its ASCII range.
const NON_ASCII_BITS: u16 = !0x7F;

/// The number of leading units of `units` that are all ASCII. Whole chunks are checked by
/// one OR each, which vectorizes; the first chunk with a non-ASCII unit is scanned unit by
/// unit, so the result is exact.
fn ascii_run_len(units: &[u16]) -> usize {
    let mut at = 0;
    while at + ASCII_CHUNK <= units.len() {
        let chunk = &units[at..at + ASCII_CHUNK];
        if chunk.iter().fold(0u16, |bits, &unit| bits | unit) & NON_ASCII_BITS != 0 {
            break;
        }
        at += ASCII_CHUNK;
    }
    at + units[at..].iter().take_while(|&&unit| unit < FIRST_NON_ASCII).count()
}

/// `String::from_utf16_lossy`: a surrogate pair decodes to its character, and a lone
/// surrogate to U+FFFD.
pub(super) fn utf16_lossy(units: &[u16]) -> String {
    let first_run = ascii_run_len(units);
    if first_run == units.len() {
        // All ASCII: one byte per unit, with no per-character encoding.
        let mut bytes = Vec::with_capacity(units.len());
        bytes.extend(units.iter().map(|&unit| unit as u8));
        // SAFETY: every byte is ASCII, which is UTF-8.
        return unsafe { String::from_utf8_unchecked(bytes) };
    }
    let mut bytes = Vec::with_capacity(units.len() + units.len() / 2);
    let mut at = 0;
    let mut run = first_run;
    loop {
        // Each ASCII run is copied as a whole; a run ends at the next non-ASCII unit.
        bytes.extend(units[at..at + run].iter().map(|&unit| unit as u8));
        at += run;
        if at == units.len() {
            break;
        }
        let unit = units[at];
        at += 1;
        let code = if (0xD800..0xDC00).contains(&unit) && units.get(at).is_some_and(|next| (0xDC00..0xE000).contains(next)) {
            let low = units[at];
            at += 1;
            0x10000 + ((u32::from(unit) - 0xD800) << 10) + (u32::from(low) - 0xDC00)
        } else if (0xD800..0xE000).contains(&unit) {
            u32::from(char::REPLACEMENT_CHARACTER)
        } else {
            u32::from(unit)
        };
        // SAFETY: `code` is a scalar value: a non-surrogate unit, a decoded pair or U+FFFD.
        let character = unsafe { char::from_u32_unchecked(code) };
        let mut buffer = [0; 4];
        bytes.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
        run = ascii_run_len(&units[at..]);
    }
    // SAFETY: every byte sequence pushed is a whole UTF-8 encoding.
    unsafe { String::from_utf8_unchecked(bytes) }
}

#[cfg(test)]
mod tests {
    use super::utf16_lossy;

    #[test]
    fn matches_the_standard_conversion() {
        let samples: [&[u16]; 6] = [
            &[],
            &[0x61, 0x62],
            &[0x61, 0xE9, 0x62],
            &[0xD83D, 0xDE00, 0x61],
            &[0x61, 0xD800, 0x62, 0xDC00, 0xD83D],
            &[0xD83D],
        ];
        for units in samples {
            assert_eq!(utf16_lossy(units), String::from_utf16_lossy(units));
        }
        let long: Vec<u16> = (0..200u16)
            .map(|i| match i % 17 {
                0 => 0xD83D,
                1 => 0xDE00,
                n => 0x41 + n,
            })
            .collect();
        assert_eq!(utf16_lossy(&long), String::from_utf16_lossy(&long));
    }

    #[test]
    fn ascii_runs_across_chunk_boundaries() {
        for split in [0usize, 1, 15, 16, 17, 31, 32, 33, 47] {
            let mut units: Vec<u16> = (0..64u16).map(|i| 0x41 + i % 26).collect();
            units[split] = 0x00E9;
            assert_eq!(utf16_lossy(&units), String::from_utf16_lossy(&units));
            units[split] = 0xD83D;
            assert_eq!(utf16_lossy(&units), String::from_utf16_lossy(&units));
            units.truncate(split + 1);
            assert_eq!(utf16_lossy(&units), String::from_utf16_lossy(&units));
        }
        let ascii: Vec<u16> = (0..100u16).map(|i| 0x20 + i % 90).collect();
        assert_eq!(utf16_lossy(&ascii), String::from_utf16_lossy(&ascii));
    }
}
