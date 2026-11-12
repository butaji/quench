//! UTF-16 units as lossy UTF-8 host text.

/// `String::from_utf16_lossy`, with each ASCII run copied as bytes instead of decoded unit
/// by unit: most source text is ASCII. A run of non-ASCII units never splits a surrogate
/// pair, so each decodes as the standard conversion would.
pub(super) fn utf16_lossy(units: &[u16]) -> String {
    let mut text = String::with_capacity(units.len());
    let mut rest = units;
    while !rest.is_empty() {
        let (ascii, tail) = rest.split_at(rest.iter().take_while(|&&unit| unit < 0x80).count());
        text.extend(ascii.iter().map(|&unit| char::from(unit as u8)));
        let (wide, tail) = tail.split_at(tail.iter().take_while(|&&unit| unit >= 0x80).count());
        text.extend(char::decode_utf16(wide.iter().copied()).map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER)));
        rest = tail;
    }
    text
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
