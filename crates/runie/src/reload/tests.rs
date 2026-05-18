//! # Reload Tests
//!
//! Tests for hot reload functionality and error translation.

#[cfg(test)]
mod reload_event_tests {
    use crate::reload::ReloadEvent;
    use std::path::PathBuf;

    #[test]
    fn test_reload_event_files_changed() {
        let event = ReloadEvent::FilesChanged(vec![
            PathBuf::from("file1.r.ts"),
            PathBuf::from("file2.r.tsx"),
        ]);
        match event {
            ReloadEvent::FilesChanged(paths) => assert_eq!(2, paths.len()),
            _ => panic!("Expected FilesChanged"),
        }
    }

    #[test]
    fn test_reload_event_protocol_changed() {
        let event = ReloadEvent::ProtocolChanged;
        assert!(matches!(event, ReloadEvent::ProtocolChanged));
    }

    #[test]
    fn test_reload_event_error() {
        let event = ReloadEvent::Error("test error".to_string());
        match event {
            ReloadEvent::Error(msg) => assert_eq!("test error", msg),
            _ => panic!("Expected Error"),
        }
    }

    #[test]
    fn test_reload_event_debug() {
        let event = ReloadEvent::FilesChanged(vec![PathBuf::from("test.r.ts")]);
        let debug = format!("{:?}", event);
        assert!(debug.contains("FilesChanged"));
    }
}

#[cfg(test)]
mod signaler_tests {
    use crate::reload::HostSignaler;
    use tempfile::TempDir;

    #[test]
    fn test_host_signaler_new() {
        let temp = TempDir::new().unwrap();
        let signaler = HostSignaler::new(temp.path());
        assert!(signaler.is_ok());
    }

    #[test]
    fn test_host_signaler_signal() {
        let temp = TempDir::new().unwrap();
        let signaler = HostSignaler::new(temp.path()).unwrap();
        let result = signaler.signal();
        assert!(result.is_ok());

        let signals: Vec<_> = std::fs::read_dir(temp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "signal"))
            .collect();
        assert!(!signals.is_empty());
    }

    #[test]
    fn test_host_signaler_clear() {
        let temp = TempDir::new().unwrap();
        let signaler = HostSignaler::new(temp.path()).unwrap();
        signaler.signal().unwrap();
        signaler.signal().unwrap();
        let result = signaler.clear();
        assert!(result.is_ok());

        let signals: Vec<_> = std::fs::read_dir(temp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "signal"))
            .collect();
        assert!(signals.is_empty());
    }

    #[test]
    fn test_host_signaler_current_dylib_none() {
        let temp = TempDir::new().unwrap();
        let signaler = HostSignaler::new(temp.path()).unwrap();
        let result = signaler.current_dylib();
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_host_signaler_should_restart() {
        let temp = TempDir::new().unwrap();
        let signaler = HostSignaler::new(temp.path()).unwrap();
        assert!(!signaler.should_restart().unwrap());
        signaler.mark_restart_needed().unwrap();
        assert!(signaler.should_restart().unwrap());
        signaler.clear_restart_needed().unwrap();
        assert!(!signaler.should_restart().unwrap());
    }

    #[test]
    fn test_host_signaler_multiple_signals() {
        let temp = TempDir::new().unwrap();
        let signaler = HostSignaler::new(temp.path()).unwrap();
        for _ in 0..5 {
            signaler.signal().unwrap();
        }

        let signals: Vec<_> = std::fs::read_dir(temp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "signal"))
            .collect();
        assert!(signals.len() <= 10);
    }
}

#[cfg(test)]
mod error_translator_tests {
    use crate::reload::ErrorTranslator;

    #[test]
    fn test_error_translator_new() {
        let translator = ErrorTranslator::new();
        let result = translator.translate("error: test");
        assert!(!result.original.is_empty());
    }

    #[test]
    fn test_error_translator_borrow_error() {
        let translator = ErrorTranslator::new();
        let rust_error = "error[E0382]: borrow of moved value";
        let result = translator.translate(rust_error);
        assert!(!result.original.is_empty());
    }

    #[test]
    fn test_error_translator_type_mismatch() {
        let translator = ErrorTranslator::new();
        let rust_error = "error[E0308]: mismatched types";
        let result = translator.translate(rust_error);
        assert!(!result.original.is_empty());
    }

    #[test]
    fn test_error_translator_method_not_found() {
        let translator = ErrorTranslator::new();
        let rust_error = "error[E0599]: no method named";
        let result = translator.translate(rust_error);
        assert!(!result.original.is_empty());
    }

    #[test]
    fn test_translated_error_display() {
        let translator = ErrorTranslator::new();
        let rust_error = "error: test";
        let result = translator.translate(rust_error);
        let display = format!("{}", result);
        assert!(!display.is_empty());
    }
}

#[cfg(test)]
mod dylib_versioning_tests {
    #[test]
    fn test_dylib_version_format() {
        use std::time::SystemTime;
        use std::time::UNIX_EPOCH;

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();

        let version = format!("libapp_{}.so", timestamp);
        assert!(version.starts_with("libapp_"));
        assert!(std::path::Path::new(&version)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("so")));
    }
}
