use super::{Bencher, black_box};
use super::super::wtf16::JsString;

enum Runs { Ascii, Wide, Mixed }
const ASCII_UNIT: u16 = b'A' as u16;
// Include unpaired surrogates: flattening copies code units, never decodes scalars.
const WIDE_UNITS: [u16; 4] = [0x4e2d, 0xd800, ASCII_UNIT, 0xdc00];

fn flatten(b: &mut Bencher, length: usize, runs: Runs) {
    let expected: Vec<_> = (0..length).map(|index| {
        match runs {
            Runs::Ascii => ASCII_UNIT,
            Runs::Mixed if index < length / 2 => ASCII_UNIT,
            Runs::Wide | Runs::Mixed => WIDE_UNITS[index % WIDE_UNITS.len()],
        }
    }).collect();
    let mut rope = JsString::from_units(&expected[..length / 2]);
    rope.push_js_string(&JsString::from_units(&expected[length / 2..]));
    let validate = || {
        assert!(rope.existing_flat().is_none());
        let flat = rope.unkept_flat();
        assert!(flat.units().iter().eq(expected.iter().copied()));
        assert!(rope.existing_flat().is_none());
    };
    validate();
    // One flatten and result drop; rope construction and unit validation are outside timing.
    b.iter(|| black_box(black_box(&rope).unkept_flat()));
    validate();
}

#[bench]
fn primitive_string_flatten_ascii_2048(b: &mut Bencher) { flatten(b, 2048, Runs::Ascii); }
#[bench]
fn primitive_string_flatten_wide_2048(b: &mut Bencher) { flatten(b, 2048, Runs::Wide); }
#[bench]
fn primitive_string_flatten_mixed_2048(b: &mut Bencher) { flatten(b, 2048, Runs::Mixed); }
#[bench]
fn primitive_string_flatten_wide_8192(b: &mut Bencher) { flatten(b, 8192, Runs::Wide); }
#[bench]
fn primitive_string_flatten_mixed_8192(b: &mut Bencher) { flatten(b, 8192, Runs::Mixed); }
