//! StructInfo tests

#[cfg(test)]
mod struct_info_tests {
    use crate::analyzer::{StructInfo, TypeInfo};

    #[test]
    fn test_struct_info() {
        let info = StructInfo {
            name: "Task".to_string(),
            fields: vec![
                ("id".to_string(), TypeInfo::Integer(0)),
                ("title".to_string(), TypeInfo::String),
                ("done".to_string(), TypeInfo::Boolean),
            ],
        };
        assert_eq!("Task", info.name);
        assert_eq!(3, info.fields.len());
    }

    #[test]
    fn test_struct_info_empty() {
        let info = StructInfo {
            name: "Empty".to_string(),
            fields: Vec::new(),
        };
        assert!(info.fields.is_empty());
    }
}
