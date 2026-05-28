#[cfg(test)]
mod analyzer_tests {
    use crate::transpile::analyzer::Analyzer;

    fn create_analyzer() -> Analyzer { Analyzer::new() }

    #[test]
    fn test_detect_hooks() {
        let mut analyzer = create_analyzer();
        analyzer.hooks.insert("useState".to_string());
        analyzer.hooks.insert("useEffect".to_string());
        assert!(analyzer.hooks.contains("useState"));
        assert!(analyzer.hooks.contains("useEffect"));
    }
    
    #[test]
    fn test_detect_signals() {
        let mut analyzer = create_analyzer();
        analyzer.signals.insert("signal".to_string());
        analyzer.signals.insert("computed".to_string());
        assert!(analyzer.signals.contains("signal"));
        assert!(analyzer.signals.contains("computed"));
    }
    
    #[test]
    fn test_detect_components() {
        let mut analyzer = create_analyzer();
        analyzer.components.insert("Counter".to_string());
        analyzer.components.insert("Header".to_string());
        assert!(analyzer.components.contains("Counter"));
        assert!(analyzer.components.contains("Header"));
    }
    
    #[test]
    fn test_island_detection() {
        let mut analyzer = create_analyzer();
        analyzer.analyze_file_path("islands/Counter.tsx");
        assert!(analyzer.is_island);
        assert!(!analyzer.is_route);
    }
    
    #[test]
    fn test_route_detection() {
        let mut analyzer = create_analyzer();
        analyzer.analyze_file_path("routes/blog/[slug].tsx");
        assert!(analyzer.is_route);
        assert!(!analyzer.is_island);
        assert_eq!(analyzer.route_pattern, Some("/blog/:slug".to_string()));
    }
    
    #[test]
    fn test_layout_detection() {
        let mut analyzer = create_analyzer();
        analyzer.analyze_file_path("routes/_layout.tsx");
        assert!(analyzer.is_layout);
    }
    
    #[test]
    fn test_app_detection() {
        let mut analyzer = create_analyzer();
        analyzer.analyze_file_path("routes/_app.tsx");
        assert!(analyzer.is_app);
    }
    
    #[test]
    fn test_middleware_detection() {
        let mut analyzer = create_analyzer();
        analyzer.analyze_file_path("routes/_middleware.ts");
        assert!(analyzer.is_middleware);
    }
    
    #[test]
    fn test_extract_route_pattern_simple() {
        let analyzer = create_analyzer();
        assert_eq!(analyzer.extract_route_pattern("routes/index.tsx"), "/");
        assert_eq!(analyzer.extract_route_pattern("routes/about.tsx"), "/about");
    }
    
    #[test]
    fn test_extract_route_pattern_nested() {
        let analyzer = create_analyzer();
        assert_eq!(analyzer.extract_route_pattern("routes/blog/index.tsx"), "/blog");
        assert_eq!(analyzer.extract_route_pattern("routes/blog/[slug].tsx"), "/blog/:slug");
    }
    
    #[test]
    fn test_hook_name_validation() {
        let analyzer = create_analyzer();
        assert!(analyzer.is_hook_name("useState"));
        assert!(analyzer.is_hook_name("useEffect"));
        assert!(analyzer.is_hook_name("useMemo"));
        assert!(!analyzer.is_hook_name("render"));
        assert!(!analyzer.is_hook_name("component"));
    }
    
    #[test]
    fn test_signal_name_validation() {
        let analyzer = create_analyzer();
        assert!(analyzer.is_signal_name("signal"));
        assert!(analyzer.is_signal_name("useSignal"));
        assert!(analyzer.is_signal_name("useComputed"));
        assert!(!analyzer.is_signal_name("useState"));
    }
}
