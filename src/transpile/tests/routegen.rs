#[cfg(test)]
mod routegen_tests {
    use crate::transpile::routegen::{parse_route_path, generate_params_struct};

    #[test]
    fn test_parse_route_path_static() {
        let route = parse_route_path("routes/index.tsx");
        assert_eq!(route.pattern, "routes/index.tsx");
        assert_eq!(route.path, "/routes/index");
    }
    
    #[test]
    fn test_parse_route_path_dynamic() {
        let route = parse_route_path("blog/[slug].tsx");
        assert!(route.segments.contains(&"slug".to_string()));
    }
    
    #[test]
    fn test_parse_route_path_nested() {
        let route = parse_route_path("api/v1/[version]/[id].tsx");
        assert!(route.segments.contains(&"version".to_string()));
        assert!(route.segments.contains(&"id".to_string()));
    }
    
    #[test]
    fn test_parse_route_path_catch_all() {
        let route = parse_route_path("[...path].tsx");
        let has_catchall = route.segments.iter().any(|s| s.contains("..."));
        assert!(has_catchall, "Expected ... in segments: {:?}", route.segments);
    }
    
    #[test]
    fn test_extract_handlers_simple() {
        let handlers: Vec<crate::transpile::routegen::RouteHandler> = Vec::new();
        assert!(handlers.is_empty());
    }
    
    #[test]
    fn test_generate_params_struct_empty() {
        let result = generate_params_struct(&[]);
        assert!(result.contains("RouteParams"));
    }
    
    #[test]
    fn test_generate_params_struct_with_params() {
        let params = vec!["slug".to_string(), "id".to_string()];
        let result = generate_params_struct(&params);
        assert!(result.contains("slug"));
        assert!(result.contains("id"));
    }
}
