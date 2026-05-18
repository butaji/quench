//! Ownership analysis tests

#[cfg(test)]
mod ownership_analyzer_tests {
    use crate::analyzer::{OwnershipAnalyzer, TypeInfo, TypeMap};

    #[test]
    fn test_ownership_analyzer_new() {
        let _analyzer = OwnershipAnalyzer::new();
        // Just ensure it constructs
    }

    #[test]
    fn test_ownership_analyzer_consume() {
        let mut analyzer = OwnershipAnalyzer::new();
        analyzer.record_consume("value");
        assert!(analyzer.was_consumed("value"));
        assert!(!analyzer.was_consumed("other"));
    }

    #[test]
    fn test_ownership_analyzer_mut_ref() {
        let mut analyzer = OwnershipAnalyzer::new();
        analyzer.record_mut_ref("counter");
        // Was consumed should be false
        assert!(!analyzer.was_consumed("counter"));
    }

    #[test]
    fn test_ownership_analyzer_shared_ref() {
        let mut analyzer = OwnershipAnalyzer::new();
        analyzer.record_shared_ref("data");
        assert!(!analyzer.was_consumed("data"));
    }

    #[test]
    fn test_ownership_analysis() {
        let mut types = TypeMap::default();
        types.insert("x".to_string(), TypeInfo::Integer(0));

        let mut analyzer = OwnershipAnalyzer::new();
        let result = analyzer.analyze(&types);
        // Verify analyze works - result should have bindings
        let has_bindings = !result.bindings().is_empty();
        assert!(has_bindings);
    }
}

#[cfg(test)]
mod borrow_mode_tests {
    use crate::analyzer::BorrowMode;

    #[test]
    fn test_borrow_mode_shared() {
        let mode = BorrowMode::Shared;
        assert!(!mode.is_mutable());
        assert_eq!("&", mode.to_rust_prefix());
    }

    #[test]
    fn test_borrow_mode_mut() {
        let mode = BorrowMode::Mut;
        assert!(mode.is_mutable());
        assert_eq!("&mut ", mode.to_rust_prefix());
    }

    #[test]
    fn test_borrow_mode_owned() {
        let mode = BorrowMode::Owned;
        assert!(mode.is_mutable());
        assert_eq!("", mode.to_rust_prefix());
    }

    #[test]
    fn test_borrow_mode_combine() {
        assert_eq!(
            BorrowMode::Shared,
            BorrowMode::Shared.combine(BorrowMode::Shared)
        );
        assert_eq!(BorrowMode::Mut, BorrowMode::Mut.combine(BorrowMode::Shared));
        assert_eq!(
            BorrowMode::Owned,
            BorrowMode::Owned.combine(BorrowMode::Owned)
        );
    }
}
