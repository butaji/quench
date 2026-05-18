//! Function, TypeMap, ExportInfo, and ImportInfo tests

#[cfg(test)]
mod function_info_tests {
    use crate::analyzer::{FunctionInfo, TypeInfo};

    fn make_function_info(
        name: &str,
        params: Vec<(&str, TypeInfo)>,
        return_type: TypeInfo,
        is_async: bool,
    ) -> FunctionInfo {
        FunctionInfo {
            name: name.to_string(),
            params: params
                .into_iter()
                .map(|(n, t)| (n.to_string(), t))
                .collect(),
            return_type: Box::new(return_type),
            is_async,
            is_method: false,
        }
    }

    #[test]
    fn test_function_info() {
        let info = make_function_info(
            "add",
            vec![("a", TypeInfo::Integer(0)), ("b", TypeInfo::Integer(0))],
            TypeInfo::Integer(0),
            false,
        );
        assert_eq!("add", info.name);
        assert_eq!(2, info.params.len());
        assert!(!info.is_async);
    }

    #[test]
    fn test_function_info_async() {
        let info = make_function_info(
            "fetch",
            vec![("url", TypeInfo::String)],
            TypeInfo::String,
            true,
        );
        assert!(info.is_async);
    }

    #[test]
    fn test_function_signature() {
        let info = make_function_info(
            "greet",
            vec![("name", TypeInfo::String)],
            TypeInfo::String,
            false,
        );
        let sig = info.to_rust_signature();
        assert!(sig.contains("fn"));
    }
}

#[cfg(test)]
mod type_map_tests {
    use crate::analyzer::{TypeInfo, TypeMap};

    #[test]
    fn test_type_map_insert_get() {
        let mut map = TypeMap::default();
        map.insert("count".to_string(), TypeInfo::Integer(0));

        assert!(map.get("count").is_some());
        assert!(map.get("missing").is_none());
    }

    #[test]
    fn test_type_map_get_missing() {
        let map = TypeMap::default();
        assert!(map.get("missing").is_none());
    }
}

#[cfg(test)]
mod export_info_tests {
    use crate::analyzer::{ExportInfo, FunctionInfo, TypeInfo};

    #[test]
    fn test_export_info() {
        let info = ExportInfo {
            name: "add".to_string(),
            rust_name: "add".to_string(),
            type_info: TypeInfo::Function(FunctionInfo {
                name: "add".to_string(),
                params: vec![],
                return_type: Box::new(TypeInfo::Integer(0)),
                is_async: false,
                is_method: false,
            }),
        };
        assert_eq!("add", info.name);
        assert_eq!("add", info.rust_name);
    }
}

#[cfg(test)]
mod import_info_tests {
    use crate::analyzer::ImportInfo;

    #[test]
    fn test_import_info() {
        let info = ImportInfo {
            path: "./state.r.ts".to_string(),
            names: vec!["Task".to_string(), "createTask".to_string()],
            is_native: false,
        };
        assert_eq!("./state.r.ts", info.path);
        assert_eq!(2, info.names.len());
        assert!(!info.is_native);
    }

    #[test]
    fn test_import_info_native() {
        let info = ImportInfo {
            path: "native:handlers".to_string(),
            names: vec!["handleKey".to_string()],
            is_native: true,
        };
        assert!(info.is_native);
    }
}
