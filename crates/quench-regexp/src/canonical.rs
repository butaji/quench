// Case canonicalization (ECMA-262 Canonicalize), shared by the matcher and the build script
// that tabulates its inverse (`build.rs`).

pub(crate) const ASCII_MAX: u16 = 0x7F;
pub(crate) const BMP_MAX: u32 = u16::MAX as u32;

pub(crate) fn canonical(value: u32, unicode: bool) -> u32 {
    let Some(character) = char::from_u32(value) else {
        return value;
    };
    if unicode {
        return u32::from(icu_casemap::CaseMapper::new().simple_fold(character));
    }
    let mut uppercase = character.to_uppercase();
    let Some(canonical) = uppercase.next() else {
        return value;
    };
    if uppercase.next().is_some()
        || u32::from(canonical) > BMP_MAX
        || (value > u32::from(ASCII_MAX) && u32::from(canonical) <= u32::from(ASCII_MAX))
    {
        value
    } else {
        u32::from(canonical)
    }
}

/// The code points whose canonical form is another code point, as `(form, member)` pairs
/// ordered by form: the table `build.rs` writes for each mode.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn preimages(unicode: bool) -> Vec<(u32, u32)> {
    let last = if unicode { u32::from(char::MAX) } else { BMP_MAX };
    let mut preimages: Vec<(u32, u32)> = (0..=last)
        .filter_map(|member| {
            let key = canonical(member, unicode);
            (key != member).then_some((key, member))
        })
        .collect();
    preimages.sort_unstable();
    preimages
}
