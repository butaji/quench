mod encoding_boundary_tests {
    use super::{from_units, units_of, units_well_formed};
    use crate::value::Value;

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
}
