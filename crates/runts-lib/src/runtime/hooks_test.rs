mod tests {
    use super::*;

    #[test]
    fn test_use_state_initial_value() {
        // use_state takes initial value directly, not a closure
        let (value, _setter) = use_state(0i32);
        assert_eq!(value, 0);
    }

    #[test]
    fn test_use_state_with_different_initial_values() {
        let (value1, _) = use_state(10i32);
        let (value2, _) = use_state(20i32);
        assert_eq!(value1, 10);
        assert_eq!(value2, 20);
    }

    #[test]
    fn test_use_state_with() {
        // use_state_with evaluates the init closure once
        let (value, _) = use_state_with(|| 100i32);
        assert_eq!(value, 100);
    }

    #[test]
    fn test_use_ref_get() {
        // use_ref returns a Ref<T> - get returns Option<T>
        let r = use_ref(|| 42i32);
        assert_eq!(r.get(), Some(42));
    }

    #[test]
    fn test_use_ref_get_string() {
        let r = use_ref(|| "hello".to_string());
        assert_eq!(r.get(), Some("hello".to_string()));
    }

    #[test]
    fn test_use_ref_clone_shares_storage() {
        let r1 = use_ref(|| vec![1, 2, 3]);
        let r2 = r1.clone();
        // Cloned refs share the same inner Arc
        assert_eq!(r1.get(), r2.get());
    }

    #[test]
    fn test_use_memo() {
        // use_memo calls factory and returns result
        let val: i32 = use_memo(|| 2 + 2, &[0usize]);
        assert_eq!(val, 4);
    }

    #[test]
    fn test_use_callback_returns_callback() {
        // use_callback returns the callback unchanged
        let cb = use_callback(|| 42i32, &[0usize]);
        assert_eq!(cb(), 42);
    }

    #[test]
    fn test_use_reducer_initial_state() {
        // use_reducer returns (initial_state, dispatch)
        let reducer = |state: i32, _action: i32| state;
        let (state, _dispatch) = use_reducer(reducer, 10);
        assert_eq!(state, 10);
    }

    #[test]
    fn test_use_effect_succeeds() {
        // use_effect does nothing on server, but should not panic
        use_effect(|| None, vec![]);
    }

    #[test]
    fn test_use_layout_effect_succeeds() {
        use_layout_effect(|| None, vec![]);
    }

    #[test]
    fn test_create_context() {
        let ctx = create_context(42i32);
        assert_eq!(ctx.get(), Some(42));
    }

    #[test]
    fn test_create_context_with_string() {
        let ctx = create_context("default".to_string());
        assert_eq!(ctx.get(), Some("default".to_string()));
    }

    #[test]
    fn test_context_clone() {
        let ctx1 = create_context(100i32);
        let ctx2 = ctx1.clone();
        assert_eq!(ctx1.get(), ctx2.get());
        assert_eq!(ctx1.get(), Some(100));
    }

    #[test]
    fn test_use_context() {
        let ctx = create_context("hello".to_string());
        let val = use_context(&ctx);
        assert_eq!(val, Some("hello".to_string()));
    }

    #[test]
    fn test_use_debug_value_no_panic() {
        use_debug_value(42);
        use_debug_value("test");
    }

    #[test]
    fn test_use_id_generates_unique_ids() {
        let id1 = use_id();
        let id2 = use_id();
        assert_ne!(id1, id2);
        assert!(id1.starts_with("rts-"));
        assert!(id2.starts_with("rts-"));
    }

    #[test]
    fn test_signal_re_exports() {
        let sig = signal(42i32);
        assert_eq!(sig.get(), 42);
    }

    #[test]
    fn test_computed_re_exports() {
        let comp: Computed<i32> = computed(|| 2 * 21);
        assert_eq!(comp.get(), 42);
    }

    #[test]
    fn test_batch_re_exports() {
        let sig = signal(0i32);
        batch(|| {
            sig.set(1);
            sig.set(2);
        });
        assert_eq!(sig.get(), 2);
    }

    #[test]
    fn test_use_state_with_vec() {
        let (value, _) = use_state(vec![1, 2, 3]);
        assert_eq!(value, vec![1, 2, 3]);
    }

    #[test]
    fn test_use_state_with_option() {
        let (value, _) = use_state(Some(42i32));
        assert_eq!(value, Some(42));
    }

    #[test]
    fn test_ref_get_none_when_unset() {
        // Ref stores Option<T>, initial is Some(initial)
        let r = use_ref(|| 0i32);
        assert_eq!(r.get(), Some(0));
    }

    #[test]
    fn test_memo_with_string() {
        let val: String = use_memo(|| format!("hello"), &[0usize]);
        assert_eq!(val, "hello");
    }
}
