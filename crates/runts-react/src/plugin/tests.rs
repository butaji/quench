mod tests {
    use super::*;

    #[test]
    fn test_react_plugin_name() {
        let plugin = ReactPlugin;
        assert_eq!(plugin.name(), "react");
    }

    #[test]
    fn test_react_plugin_help() {
        let plugin = ReactPlugin;
        assert!(plugin.help_text().contains("React"));
    }

    #[test]
    fn test_codegen_component_module() {
        let plugin = ReactPlugin;
        let code = plugin.codegen_component_module("component/LazyHome.jsx");
        assert!(code.contains("LazyHome"));
        assert!(code.contains("render()"));
    }

    #[test]
    fn test_codegen_server_module() {
        let plugin = ReactPlugin;
        let code = plugin.codegen_server_module("server1.js");
        assert!(code.contains("async fn handler()"));
        assert!(code.contains("Html("));
        assert!(code.contains("Router::new()"));
    }

    #[test]
    fn test_codegen_module_detects_component() {
        let plugin = ReactPlugin;
        let hir_json = r#"{"source_path": "component/Test.jsx", "items_json": [], "types": {}}"#;
        let code = plugin.codegen_module(hir_json).unwrap();
        assert!(code.contains("Test"));
    }

    #[test]
    fn test_codegen_module_detects_server() {
        let plugin = ReactPlugin;
        let hir_json = r#"{"source_path": "server.js", "items_json": [], "types": {}}"#;
        let code = plugin.codegen_module(hir_json).unwrap();
        assert!(code.contains("handler()"));
    }

    #[test]
    fn test_cargo_deps() {
        let plugin = ReactPlugin;
        let deps = plugin.codegen_entry(&[]);
        assert!(deps.is_ok());
    }

    #[test]
    fn test_codegen_entry() {
        let plugin = ReactPlugin;
        let entry = plugin.codegen_entry(&[]).unwrap();
        assert!(entry.contains("axum"));
        assert!(entry.contains("tokio::main"));
    }

    #[test]
    fn test_extract_component_name() {
        assert_eq!(extract_component_name("component/LazyHome.jsx"), "LazyHome");
        assert_eq!(extract_component_name("component/index.jsx"), "Index");
        assert_eq!(extract_component_name("src/Foo.js"), "Foo");
    }

    #[test]
    fn test_extract_route_path() {
        assert_eq!(extract_route_path("server.js"), "/");
        assert_eq!(extract_route_path("server1.js"), "/");
        assert_eq!(extract_route_path("server2.js"), "/2");
        assert_eq!(extract_route_path("serverApi.js"), "/Api");
    }

    #[test]
    fn test_codegen_entry_with_modules() {
        let plugin = ReactPlugin;
        let modules = vec![
            runts_plugin::hir::Module {
                source_path: Some("server1.js".to_string()),
                route_info: None,
                items_json: None,
            }
        ];
        let entry = plugin.codegen_entry(&modules).unwrap();
        assert!(entry.contains("axum"));
        assert!(entry.contains("/"));
    }

    #[test]
    fn test_codegen_entry_with_jsx_components() {
        let plugin = ReactPlugin;
        let modules = vec![
            runts_plugin::hir::Module {
                source_path: Some("component/LazyHome.jsx".to_string()),
                route_info: None,
                items_json: None,
            },
            runts_plugin::hir::Module {
                source_path: Some("component/LazyPage.jsx".to_string()),
                route_info: None,
                items_json: None,
            },
            runts_plugin::hir::Module {
                source_path: Some("server1.js".to_string()),
                route_info: None,
                items_json: None,
            }
        ];
        let entry = plugin.codegen_entry(&modules).unwrap();
        // Should use LazyHome (first component) not hardcoded App
        assert!(entry.contains("LazyHome::render()"));
        assert!(!entry.contains("App::render()"));
        assert!(entry.contains("axum"));
    }

    #[test]
    fn test_find_first_component_name() {
        let modules = vec![
            runts_plugin::hir::Module {
                source_path: Some("server1.js".to_string()),
                route_info: None,
                items_json: None,
            },
            runts_plugin::hir::Module {
                source_path: Some("component/LazyPage.jsx".to_string()),
                route_info: None,
                items_json: None,
            },
            runts_plugin::hir::Module {
                source_path: Some("component/LazyHome.jsx".to_string()),
                route_info: None,
                items_json: None,
            },
        ];
        assert_eq!(find_first_component_name(&modules), Some("LazyPage".to_string()));
    }

    #[test]
    fn test_find_first_component_name_no_components() {
        let modules = vec![
            runts_plugin::hir::Module {
                source_path: Some("server1.js".to_string()),
                route_info: None,
                items_json: None,
            },
        ];
        assert_eq!(find_first_component_name(&modules), None);
    }
}
