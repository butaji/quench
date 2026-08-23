mod encoding_boundary_tests {
    use super::{
        expand_utf16, from_units, source_encoding, units_of, units_well_formed,
        StringSourceEncoding,
    };
    use crate::value::Value;


    #[test]
    fn expansion_is_delayed_and_exact_for_both_sources() {
        let utf8 = Value::String("A😀".to_owned());
        assert_eq!(source_encoding(&utf8), Some(StringSourceEncoding::Utf8));
        assert_eq!(expand_utf16(&utf8), Some(vec![0x41, 0xd83d, 0xde00]));

        let lone = from_units(vec![0x41, 0xd800, 0x42]);
        assert_eq!(source_encoding(&lone), Some(StringSourceEncoding::Utf16));
        assert_eq!(expand_utf16(&lone), Some(vec![0x41, 0xd800, 0x42]));
    }

    #[test]
    fn expansion_is_not_available_for_non_strings() {
        assert_eq!(expand_utf16(&Value::Undefined), None);
        assert_eq!(expand_utf16(&Value::Number(1.0)), None);
    }

    #[test]
    fn valid_surrogate_pair_uses_string_and_round_trips_units() {
        let units = vec![0xd83d, 0xde00];
        assert!(units_well_formed(&units));
        let value = from_units(units.clone());
        assert!(matches!(value, Value::String(_)));
        assert_eq!(units_of(&value), Some(units));
    }

    #[test]
    fn each_lone_surrogate_stays_string_units_at_boundary() {
        for unit in [0xd800, 0xdbff, 0xdc00, 0xdfff] {
            let units = vec![unit];
            assert!(!units_well_formed(&units));
            let value = from_units(units.clone());
            assert!(matches!(value, Value::StringUnits(_)));
            assert_eq!(units_of(&value), Some(units));
        }
    }

    #[test]
    fn malformed_pair_boundaries_are_not_repaired() {
        for units in [vec![0xd800, 0xd800], vec![0xdc00, 0xdc00], vec![0xd800, 0x61]] {
            assert!(!units_well_formed(&units));
            let value = from_units(units.clone());
            assert!(matches!(value, Value::StringUnits(_)));
            assert_eq!(units_of(&value), Some(units));
        }
    }

    #[test]
    fn source_encoding_identifies_owned_canonical_buffer() {
        let utf8 = Value::String("😀".to_owned());
        assert_eq!(source_encoding(&utf8), Some(StringSourceEncoding::Utf8));

        let utf16 = from_units(vec![0xd800]);
        assert_eq!(source_encoding(&utf16), Some(StringSourceEncoding::Utf16));
    }

    #[test]
    fn source_encoding_is_not_derived_compact_storage() {
        let utf8_latin1 = Value::String("abc".to_owned());
        assert_eq!(source_encoding(&utf8_latin1), Some(StringSourceEncoding::Utf8));

        let utf16_latin1 = from_units(vec![0x61, 0x62, 0x63]);
        assert_eq!(source_encoding(&utf16_latin1), Some(StringSourceEncoding::Utf8));
    }

    #[test]
    fn non_strings_have_no_source_encoding() {
        assert_eq!(source_encoding(&Value::Undefined), None);
        assert_eq!(source_encoding(&Value::Boolean(true)), None);
    }
}
