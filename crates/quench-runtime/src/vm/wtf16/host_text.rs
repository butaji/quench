//! UTF-16 units as lossy UTF-8 host text.

/// First UTF-16 unit that is not ASCII.
const FIRST_NON_ASCII: u16 = 0x80;

/// `String::from_utf16_lossy`: a surrogate pair decodes to its character, and a lone
/// surrogate to U+FFFD.
pub(super) fn utf16_lossy(units: &[u16]) -> String {
    // One pass that encodes each unit in place: a split at every change between ASCII and
    // other units scanned each run twice and decoded the rest through `char`.
    let mut bytes = Vec::with_capacity(units.len() + units.len() / 2);
    let mut at = 0;
    while at < units.len() {
        let unit = units[at];
        at += 1;
        if unit < FIRST_NON_ASCII {
            bytes.push(unit as u8);
            continue;
        }
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
}
