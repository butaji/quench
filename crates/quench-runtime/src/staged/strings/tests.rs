use super::*;
use crate::heap::Heap;
use crate::staged::Concrete;
use crate::staged::value::length_value;
use crate::Value;
use crate::vm::wtf16::{JsString, SHORT_ASCII_CAPACITY, Units};

fn stage() -> Concrete { Concrete::pure() }

/// The string rows of `value` on the heap's own records at `index`, as words.
fn read(heap: &Heap, value: Value, index: Value) -> Option<(Option<u64>, Option<u64>)> {
    let mut s = stage();
    let base = heap.string_record_base().map_or(0, |base| base as u64);
    let string = flat_string(&mut s, base, value.to_bits(), ())?;
    let code = string_char_code_at(&mut s, string, index.to_bits(), ());
    let length = length_value(&mut s, string.length, ());
    Some((code, length))
}

#[test]
fn rows_read_every_flat_form_through_the_record() {
    let mut heap = Heap::new();
    let long = "k".repeat(SHORT_ASCII_CAPACITY + 5);
    let forms = [
        (heap.alloc_string(JsString::from_str("")), Vec::<u16>::new()),
        (heap.alloc_string(JsString::from_str("hello")), "hello".encode_utf16().collect()),
        (heap.alloc_string(JsString::from_str(&long)), long.encode_utf16().collect()),
    ];
    for (value, units) in forms {
        for (at, unit) in units.iter().enumerate() {
            let read = read(&heap, value, Value::integer(at as i32)).unwrap();
            assert_eq!(read.0, Some(Value::integer(i32::from(*unit)).to_bits()));
            assert_eq!(read.1, Some(Value::integer(units.len() as i32).to_bits()));
        }
        let outside = read(&heap, value, Value::integer(units.len() as i32)).unwrap();
        assert_eq!(outside.0, Some(Value::NAN.to_bits()), "outside the string");
        assert_eq!(read(&heap, value, Value::number(0.5)).unwrap().0, None, "a double index leaves");
    }
    let wide = heap.alloc_string(JsString::from_units(&[0x61, 0x20ac]));
    assert!(read(&heap, wide, Value::integer(0)).is_none(), "wide units are behind `Repr::Shared`: the row leaves");
}

#[test]
fn a_slice_a_non_string_and_a_missing_base_leave() {
    let mut heap = Heap::new();
    let long = JsString::from_str(&"s".repeat(SHORT_ASCII_CAPACITY * 3));
    let slice = heap.alloc_string(long.substring(1..40));
    assert!(read(&heap, slice, Value::integer(0)).is_none(), "a slice is no form a row reads");
    assert!(read(&heap, Value::integer(3), Value::integer(0)).is_none());
    let empty = Heap::new();
    assert!(read(&empty, slice, Value::integer(0)).is_none(), "no record base");
}

#[test]
fn char_code_at_reads_units_of_either_width_and_answers_nan_outside() {
    let narrow = Units::Ascii(b"aZ");
    let wide = [0x61u16, 0x20ac];
    let wide = Units::Wide(&wide);
    let at = |units, index: Value| concrete_char_code_at(ConcreteString::Units(units), index).map(Value::to_bits);
    assert_eq!(at(narrow, Value::integer(1)), Some(Value::integer(0x5a).to_bits()));
    assert_eq!(at(wide, Value::integer(1)), Some(Value::integer(0x20ac).to_bits()));
    for index in [2, 100, -1, i32::MIN] {
        assert_eq!(at(narrow, Value::integer(index)), Some(Value::NAN.to_bits()), "{index} is outside");
    }
    assert_eq!(at(Units::Ascii(b""), Value::integer(0)), Some(Value::NAN.to_bits()));
    assert_eq!(at(narrow, Value::number(0.5)), None, "a double index leaves to the native");
    assert_eq!(at(narrow, Value::UNDEFINED), None, "an absent index leaves to the native");
}

#[test]
fn char_at_answers_ascii_units_from_the_table_and_leaves_the_rest() {
    let mut table = [Value::UNDEFINED; 128];
    table[usize::from(b'y')] = Value::integer(7);
    table[0x7f] = Value::integer(9);
    let table = table.as_ptr();
    let wide = [0x61u16, 0x20ac, 0x7f];
    let wide = Units::Wide(&wide);
    let at = |units, index: i32| concrete_char_at(ConcreteString::Units(units), table, Value::integer(index));
    assert_eq!(at(Units::Ascii(b"xy"), 1), Some(Value::integer(7)));
    assert_eq!(at(Units::Ascii(b"xy"), 0), None, "a unit with no string yet leaves to the native");
    assert_eq!(at(wide, 1), None, "a unit past ASCII leaves");
    assert_eq!(at(wide, 2), Some(Value::integer(9)), "the last ASCII unit stays");
    assert_eq!(at(wide, 3), None, "outside leaves for the native's empty string");
    assert_eq!(at(wide, -1), None);
    assert_eq!(concrete_char_at(ConcreteString::Units(wide), table, Value::number(1.5)), None);
    let no_table: *const Value = std::ptr::null();
    assert_eq!(concrete_char_at(ConcreteString::Units(wide), no_table, Value::integer(2)), None, "no table yet");
}

/// The ECMAScript WhiteSpace and LineTerminator units, as the specification lists them.
fn spec_space(unit: u16) -> bool {
    matches!(unit, 0x09..=0x0D | 0x20 | 0xA0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000 | 0xFEFF)
}

#[test]
fn trim_space_is_the_specification_set_for_every_unit() {
    for unit in 0..=u16::MAX {
        assert_eq!(is_trim_space(&mut stage(), UnitWidth::Wide, u64::from(unit)), spec_space(unit), "{unit:#x}");
    }
    for unit in 0..=u8::MAX {
        assert_eq!(is_trim_space(&mut stage(), UnitWidth::Narrow, u64::from(unit)), spec_space(u16::from(unit)), "{unit:#x}");
    }
}

#[test]
fn trim_bounds_strip_the_chosen_ends() {
    let narrow = |text: &[u8], ends| {
        let (start, end) = trim_bounds(&mut stage(), UnitWidth::Narrow, text.as_ptr() as u64, text.len() as u64, ends);
        (start, end)
    };
    assert_eq!(narrow(b"", (true, true)), (0, 0));
    assert_eq!(narrow(b"   ", (true, true)), (3, 3));
    assert_eq!(narrow(b"   ", (false, true)), (0, 0));
    assert_eq!(narrow(b" \t a b \n", (true, true)), (3, 6));
    assert_eq!(narrow(b" \t a b \n", (true, false)), (3, 9));
    assert_eq!(narrow(b" \t a b \n", (false, true)), (0, 6));
    assert_eq!(narrow(b"\xa0x\xa0", (true, true)), (1, 2));
    let wide: Vec<u16> = [0xFEFF, 0x3000, 0x41, 0x2028, 0x42, 0x205F].to_vec();
    let (start, end) = trim_bounds(&mut stage(), UnitWidth::Wide, wide.as_ptr() as u64, wide.len() as u64, (true, true));
    assert_eq!((start, end), (2, 5));
}
