//! # Driver Tests
//!
//! Tests for build driver, cache manager, and configuration.

#[cfg(test)]
mod driver_tests {
    use crate::driver::{BuildMode, BuildOptions};

    #[test]
    fn test_build_options_new() {
        let workspace = std::path::PathBuf::from("/test");
        let options = BuildOptions::new(workspace.clone());
        assert_eq!(workspace, options.workspace);
        assert_eq!(BuildMode::Dev, options.mode);
        assert!(!options.verbose);
    }

    #[test]
    fn test_build_options_dev() {
        let options = BuildOptions::new(std::path::PathBuf::from(".")).dev();
        assert_eq!(BuildMode::Dev, options.mode);
    }

    #[test]
    fn test_build_options_release() {
        let options = BuildOptions::new(std::path::PathBuf::from(".")).release();
        assert_eq!(BuildMode::Release, options.mode);
    }

    #[test]
    fn test_build_options_setters() {
        let mut options = BuildOptions::new(std::path::PathBuf::from("."));
        options.verbose = true;
        options.target_crate = Some("app".to_string());
        assert!(options.verbose);
        assert_eq!(Some("app".to_string()), options.target_crate);
    }

    #[test]
    fn test_build_mode_default() {
        let mode = BuildMode::default();
        let debug = format!("{:?}", mode);
        assert!(debug.contains("Dev"));
    }

    #[test]
    fn test_build_mode_debug() {
        let mode = BuildMode::Dev;
        let debug = format!("{:?}", mode);
        assert!(debug.contains("Dev"));
    }

    #[test]
    fn test_build_mode_release() {
        let mode = BuildMode::Release;
        let debug = format!("{:?}", mode);
        assert!(debug.contains("Release"));
    }

    #[test]
    fn test_build_options_with_config() {
        let config_path = std::path::PathBuf::from("/path/to/runie.toml");
        let options = BuildOptions {
            mode: BuildMode::Release,
            workspace: std::path::PathBuf::from("/workspace"),
            target_crate: Some("myapp".to_string()),
            config: Some(config_path.clone()),
            transpile_file: None,
            watch_transpile: false,
            verbose: true,
            json: false,
        };

        assert_eq!(config_path, options.config.unwrap());
        assert!(options.verbose);
        assert_eq!(Some("myapp".to_string()), options.target_crate);
    }
}

#[cfg(test)]
mod config_tests {
    use crate::driver::RuneConfig;
    use std::path::Path;

    #[test]
    fn test_rune_config_default() {
        let config = RuneConfig::default();
        // Just verify it constructs with some name
        assert!(!config.project.name.is_empty());
    }

    #[test]
    fn test_rune_config_load_missing_file() {
        let config = RuneConfig::load(Path::new("/nonexistent/config.toml"));
        // Should fail because file doesn't exist
        assert!(config.is_err());
    }

    #[test]
    fn test_rune_config_project_name() {
        let mut config = RuneConfig::default();
        config.project.name = "myapp".to_string();
        assert_eq!("myapp", config.project.name);
    }

    #[test]
    fn test_rune_config_target_crate() {
        let mut config = RuneConfig::default();
        config.build.target_crate = "frontend".to_string();
        assert_eq!("frontend", config.build.target_crate);
    }

    #[test]
    fn test_rune_config_hot_reload_toggle() {
        let mut config = RuneConfig::default();
        assert!(config.dev.hot_reload);
        config.dev.hot_reload = false;
        assert!(!config.dev.hot_reload);
    }

    #[test]
    fn test_rune_config_debounce() {
        let mut config = RuneConfig::default();
        config.dev.debounce = 250;
        assert_eq!(250, config.dev.debounce);
    }

    #[test]
    fn test_rune_config_lto() {
        let mut config = RuneConfig::default();
        assert!(config.release.lto);
        config.release.lto = false;
        assert!(!config.release.lto);
    }
}

#[cfg(test)]
mod cache_tests {
    use crate::driver::CacheManager;
    use tempfile::TempDir;

    #[test]
    fn test_cache_manager_new() {
        let temp = TempDir::new().unwrap();
        let cache = CacheManager::new(temp.path());
        assert!(cache.is_ok());
        let cache = cache.unwrap();
        assert!(cache.root().exists());
    }

    #[test]
    fn test_cache_manager_generated_dir() {
        let temp = TempDir::new().unwrap();
        let cache = CacheManager::new(temp.path()).unwrap();
        let generated = cache.generated_dir();
        assert!(generated.to_string_lossy().contains("rune-cache"));
    }

    #[test]
    fn test_cache_manager_module_path() {
        let temp = TempDir::new().unwrap();
        let cache = CacheManager::new(temp.path()).unwrap();
        let path = cache.module_path("main");
        assert!(path.to_string_lossy().contains("main.rs"));
    }

    #[test]
    fn test_cache_manager_cargo_toml_path() {
        let temp = TempDir::new().unwrap();
        let cache = CacheManager::new(temp.path()).unwrap();
        let path = cache.generated_cargo_toml();
        assert!(path.to_string_lossy().ends_with("Cargo.toml"));
    }

    #[test]
    fn test_cache_manager_hot_dir() {
        let temp = TempDir::new().unwrap();
        let cache = CacheManager::new(temp.path()).unwrap();
        let hot = cache.hot_dir();
        assert!(hot.to_string_lossy().contains("hot"));
    }

    #[test]
    fn test_cache_manager_current_dylib_link() {
        let temp = TempDir::new().unwrap();
        let cache = CacheManager::new(temp.path()).unwrap();
        let link = cache.current_dylib_link();
        assert!(link.to_string_lossy().contains(".current"));
    }

    #[test]
    fn test_cache_manager_needs_regeneration_no_sources() {
        let temp = TempDir::new().unwrap();
        let cache = CacheManager::new(temp.path()).unwrap();
        let needs = cache.needs_regeneration(&[]);
        assert!(needs);
    }

    #[test]
    fn test_cache_manager_clean() {
        let temp = TempDir::new().unwrap();
        let cache = CacheManager::new(temp.path()).unwrap();
        std::fs::write(cache.generated_cargo_toml(), "test").unwrap();
        let result = cache.clean();
        assert!(result.is_ok());
    }

    #[test]
    fn test_cache_manager_workspace_path() {
        let temp = TempDir::new().unwrap();
        let cache = CacheManager::new(temp.path()).unwrap();
        assert_eq!(temp.path(), cache.workspace());
    }
}
