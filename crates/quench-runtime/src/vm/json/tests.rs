use super::{JsString, JsonParser, JsonTreeBuilder, JsonUnit, JsonValue, WORD_BYTES, write_json_string};

#[test]
fn byte_string_runs_end_where_each_byte_scan_ends() {
    // Every byte value at every position of two 16-byte blocks, a word and past them,
    // after every prefix byte class, as the per-unit definition finds it.
    let fill = [b'a', 0x7f, 0x80, 0xff, b' ', b'!'];
    for &prefix in &fill {
        for length in 0..(4 * WORD_BYTES + 3) {
            for position in 0..=length {
                for ending in 0..=u8::MAX {
                    let mut units = vec![prefix; length];
                    if position < length {
                        units[position] = ending;
                    }
                    let expected = units.iter().position(|unit| unit.ends_string_run());
                    assert_eq!(
                        <u8 as JsonUnit>::string_run_length(&units),
                        expected,
                        "prefix {prefix:#x} length {length} position {position} byte {ending:#x}"
                    );
                }
            }
        }
    }
}

#[test]
fn json_string_writer_preserves_lone_surrogates() {
    let value = JsString::from_units(&[0xD800, b'a' as u16, 0xDC00]);
    let mut output = Vec::new();
    write_json_string(&value, &mut output);
    assert_eq!(output, b"\"\\ud800a\\udc00\"");
}

#[test]
fn json_string_writer_keeps_surrogate_pairs_as_scalars() {
    let value = JsString::from_units(&[0xD83E, 0xDD80]);
    let mut output = Vec::new();
    write_json_string(&value, &mut output);
    assert_eq!(output, "\"🦀\"".as_bytes());
}

#[test]
fn json_parser_keeps_escaped_surrogate_units() {
    let source = r#""\ud800a\udc00""#;
    let units = source.encode_utf16().collect::<Vec<_>>();
    let JsonValue::Source(value, text) =
        JsonParser::new(&units, &mut JsonTreeBuilder).parse().unwrap()
    else {
        panic!("expected source-bearing primitive");
    };
    assert_eq!(text.units(), units);
    let JsonValue::String(value) = *value else {
        panic!("expected string");
    };
    assert_eq!(value.units(), &[0xD800, b'a' as u16, 0xDC00]);
}
