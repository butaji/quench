//! EnumInfo tests

#[cfg(test)]
mod enum_info_tests {
    use crate::analyzer::{EnumInfo, EnumVariant, TypeInfo};

    #[test]
    fn test_enum_info() {
        let info = EnumInfo {
            name: "Color".to_string(),
            variants: vec![
                EnumVariant {
                    tag: "Red".to_string(),
                    fields: vec![],
                },
                EnumVariant {
                    tag: "Green".to_string(),
                    fields: vec![],
                },
                EnumVariant {
                    tag: "Blue".to_string(),
                    fields: vec![],
                },
            ],
        };
        assert_eq!("Color", info.name);
        assert_eq!(3, info.variants.len());
    }

    #[test]
    fn test_enum_info_with_data() {
        let info = EnumInfo {
            name: "Message".to_string(),
            variants: vec![
                EnumVariant {
                    tag: "Move".to_string(),
                    fields: vec![
                        ("x".to_string(), TypeInfo::Integer(0)),
                        ("y".to_string(), TypeInfo::Integer(0)),
                    ],
                },
                EnumVariant {
                    tag: "Quit".to_string(),
                    fields: vec![],
                },
            ],
        };
        assert_eq!("Message", info.name);
        assert_eq!(2, info.variants.len());
    }
}
