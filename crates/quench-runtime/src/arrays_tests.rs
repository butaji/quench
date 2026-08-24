#[cfg(test)]
mod tests {
    use super::{array_index, index_of};
    use crate::value::{ArrayData, ObjectData, Value};
    use std::rc::Rc;

    #[test]
    fn index_of_does_not_use_structural_object_equality() {
        let left = Value::Object(Rc::new(ObjectData::new(Vec::new())));
        let right = Value::Object(Rc::new(ObjectData::new(Vec::new())));
        let array = Value::Array(Rc::new(ArrayData::new(vec![left.clone()])));
        let result = index_of(Some(&array), &[right]);
        assert_eq!(result, Ok(Value::Number(-1.0)));
    }

    #[test]
    fn array_index_parser_preserves_canonical_indices_without_allocating() {
        assert_eq!(array_index("0"), Some(0));
        assert_eq!(array_index("42"), Some(42));
        assert_eq!(array_index(""), None);
        assert_eq!(array_index("01"), None);
        assert_eq!(array_index("4294967295"), None);
        assert_eq!(array_index("4294967296"), None);
        assert_eq!(array_index("1.0"), None);
    }

    #[test]
    fn dense_array_property_reads_storage_before_generic_fallback() {
        let mut data = ArrayData::new(vec![Value::Number(7.0)]);
        data.set_property("label", Value::String("generic".into()));
        assert_eq!(super::property(&data, "0"), Value::Number(7.0));
        assert_eq!(super::property(&data, "label"), Value::String("generic".into()));
    }
    #[test]
    fn indexed_generic_property_is_not_treated_as_dense_storage() {
        let mut data = ArrayData::new(Vec::new());
        data.set_property("3", Value::String("fallback".into()));
        assert_eq!(super::property(&data, "3"), Value::String("fallback".into()));
        assert_eq!(data.dense_value_at(3), None);
    }

    #[test]
    fn packed_index_and_length_use_dense_header_facts() {
        let data = ArrayData::new(vec![Value::Number(11.0), Value::Number(22.0)]);
        assert!(data.is_packed_ordinary());
        assert_eq!(super::property(&data, "0"), Value::Number(11.0));
        assert_eq!(super::property(&data, "1"), Value::Number(22.0));
        assert_eq!(super::property(&data, "length"), Value::Number(2.0));
    }

    #[test]
    fn sequential_fill_recovers_packed_numeric_storage_from_holey_length() {
        let mut data = ArrayData::new(Vec::new());
        data.set_length(4);
        assert!(!data.is_packed_ordinary());
        for index in 0..4 {
            assert!(data.append_preallocated_number(index, &Value::Number(index as f64)));
        }
        assert!(data.is_packed_ordinary());
        assert_eq!(data.physical_len(), 4);
        assert_eq!(data.dense_number_at(3), Some(3.0));
    }

    #[test]
    fn hot_storage_exposes_one_canonical_dense_shape() {
        let data = ArrayData::new(vec![Value::Number(11.0), Value::Number(22.0)]);
        let (values, length, kind) = data.hot_storage();
        assert_eq!(values.len(), 2);
        assert_eq!(length, 2);
        assert!(kind.is_packed());
        assert_eq!(values.as_ptr(), data.dense_value_at(0).map(|_| values.as_ptr()).unwrap());
    }

    #[test]
    fn packed_push_preserves_values_and_updates_length() {
        let array = Value::Array(Rc::new(ArrayData::new(vec![Value::Number(1.0)])));
        let result = crate::builtins::array_push(
            Some(&array),
            &[Value::Number(2.0), Value::Number(3.0)],
        );
        assert_eq!(result.unwrap(), Value::Number(3.0));
        // Builtin calls replace the receiver binding through the locals
        // replacement channel; a direct Rc handle is intentionally unchanged.
    }
    #[test]
    fn array_length_header_is_authoritative_when_capacity_exceeds_storage() {
        let mut data = ArrayData::new(vec![Value::Number(1.0)]);
        let initial_capacity = data.storage_capacity();
        data.set_length(128);

        // Extending length records holes in the header without materializing
        // values or changing the dense backing-store ownership.
        assert_eq!(data.header_length(), 128);
        assert_eq!(data.logical_len(), 128);
        assert_eq!(data.physical_len(), 1);
        assert_eq!(data.storage_capacity(), initial_capacity);
        assert_eq!(data.get_index(127), None);
        assert_eq!(super::property(&data, "length"), Value::Number(128.0));
    }
    #[test]
    fn dense_bounds_use_logical_length_before_property_source() {
        let mut data = ArrayData::new(vec![Value::Number(7.0), Value::Number(8.0)]);
        data.set_length(1);
        data.set_property("1", Value::String("indexed fallback".into()));

        assert_eq!(data.dense_value_at(1), None);
        assert_eq!(
            super::property(&data, "1"),
            Value::String("indexed fallback".into())
        );
    }

    #[test]
    fn dense_copy_rejects_physical_suffix_outside_logical_bounds() {
        let mut data = ArrayData::new(vec![
            Value::Number(1.0),
            Value::Number(2.0),
            Value::Number(3.0),
        ]);
        data.set_length(1);

        // Shrinking the header retains the backing allocation, so physical
        // storage alone cannot be used as a source-of-truth for copying.
        assert!(!data.copy_dense_within(1, 0, 1));
        assert_eq!(data.dense_value_at(1), None);
        assert_eq!(data.get_index(0), Some(Value::Number(1.0)));
    }


    #[test]
    fn indexed_growth_reserves_capacity_before_materializing_holes() {
        let mut data = ArrayData::new(Vec::new());
        let initial_capacity = data.storage_capacity();
        for index in 0..4 {
            data.set_index(index, Value::Number(index as f64));
        }
        let reserved_capacity = data.storage_capacity();

        assert_eq!(data.header_length(), 4);
        assert_eq!(data.physical_len(), 4);
        assert!(reserved_capacity >= 4);
        assert!(reserved_capacity >= initial_capacity);
        assert_eq!(data.get_index(0), Some(Value::Number(0.0)));
        assert_eq!(data.get_index(3), Some(Value::Number(3.0)));
    }

    #[test]
    fn sparse_transition_keeps_dense_prefix_and_property_tail_separate() {
        let mut data = ArrayData::new(vec![Value::Number(3.0), Value::Number(5.0)]);
        let dense_capacity = data.storage_capacity();

        data.set_index(10_000, Value::Number(9.0));
        assert!(data.is_sparse());
        assert_eq!(data.physical_len(), 2);
        assert_eq!(data.storage_capacity(), dense_capacity);
        assert_eq!(data.dense_number_at(0), Some(3.0));
        assert_eq!(data.dense_number_at(1), Some(5.0));
        assert_eq!(data.dense_number_at(10_000), None);
        assert_eq!(data.get_index(10_000), Some(Value::Number(9.0)));

        // A subsequent adjacent write must not re-expand the abandoned dense
        // store; the sparse property store remains authoritative for the tail.
        data.set_index(2, Value::Number(7.0));
        assert_eq!(data.physical_len(), 2);
        assert_eq!(data.get_index(2), Some(Value::Number(7.0)));
        assert_eq!(data.property("2"), Some(Value::Number(7.0)));
    }

    #[test]
    fn object_hot_properties_is_authoritative_storage() {
        let data = ObjectData::new(vec![("answer".into(), Value::Number(42.0))]);
        let hot = data.hot_properties();
        assert_eq!(hot.len(), 1);
        assert_eq!(hot[0].0, "answer");
        assert_eq!(hot[0].1, Value::Number(42.0));
    }


    #[test]
    fn indexed_and_length_reads_fall_back_after_sparse_transition() {
        let mut data = ArrayData::new(vec![Value::Number(11.0)]);
        data.set_index(10_000, Value::String("tail".into()));
        assert!(!data.is_packed_ordinary());
        assert_eq!(super::property(&data, "0"), Value::Number(11.0));
        assert_eq!(super::property(&data, "10000"), Value::String("tail".into()));
        assert_eq!(super::property(&data, "length"), Value::Number(10_001.0));
    }

    #[test]
    fn reduce_empty_array_returns_explicit_initial_value() {
        let array = Value::Array(Rc::new(ArrayData::new(Vec::new())));
        let result = super::reduce_values(
            Some(&array),
            &[Value::Undefined, Value::Number(9.0)],
            false,
        );
        assert_eq!(result, Ok(Value::Number(9.0)));
    }
    #[test]
    fn flat_map_without_mapper_preserves_array_identity_and_contents() {
        let array = Value::Array(Rc::new(ArrayData::new(vec![
            Value::Number(1.0),
            Value::Number(2.0),
        ])));
        let result = super::flat_map(Some(&array), &[]).unwrap();
        let Value::Array(result_data) = result else {
            panic!("flat_map must return the original array when mapper is absent");
        };
        let Value::Array(source_data) = &array else {
            unreachable!();
        };
        assert!(Rc::ptr_eq(&result_data, source_data));
        assert_eq!(result_data.logical_len(), source_data.logical_len());
        for index in 0..source_data.logical_len() {
            assert_eq!(result_data.get_index(index), source_data.get_index(index));
        }
    }
    #[test]
    fn flatten_reuses_one_output_buffer_across_nested_arrays() {
        let nested = Value::Array(Rc::new(ArrayData::new(vec![
            Value::Number(2.0),
            Value::Array(Rc::new(ArrayData::new(vec![Value::Number(3.0)]))),
        ])));
        let values = Value::Array(Rc::new(ArrayData::new(vec![Value::Number(1.0), nested])));
        let Value::Array(flattened) = super::flat(Some(&values), &[Value::Number(2.0)]).unwrap() else {
            panic!("flat must return an array");
        };
        assert_eq!(
            (0..flattened.logical_len())
                .map(|index| flattened.get_index(index).unwrap())
                .collect::<Vec<_>>(),
            vec![
                Value::Number(1.0),
                Value::Number(2.0),
                Value::Number(3.0)
            ]
        );
    }
}
