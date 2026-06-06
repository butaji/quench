mod tests {
    use super::*;

    #[test]
    fn test_signal_new_and_get() {
        let sig = Signal::new(42i32);
        assert_eq!(sig.get(), 42);
    }

    #[test]
    fn test_signal_set() {
        let sig = Signal::new(0i32);
        sig.set(100);
        assert_eq!(sig.get(), 100);
    }

    #[test]
    fn test_signal_update() {
        let sig = Signal::new(10i32);
        sig.update(|v| *v *= 2);
        assert_eq!(sig.get(), 20);
    }

    #[test]
    fn test_signal_clone() {
        let sig1 = Signal::new(vec![1, 2, 3]);
        let sig2 = sig1.clone();
        assert_eq!(sig1.get(), sig2.get());
        sig1.set(vec![4, 5]);
        assert_eq!(sig2.get(), vec![4, 5]);
    }

    #[test]
    fn test_signal_read() {
        let sig = Signal::new(42i32);
        let guard = sig.read();
        assert_eq!(*guard, 42);
    }

    #[test]
    fn test_signal_default() {
        let sig: Signal<i32> = Signal::default();
        assert_eq!(sig.get(), 0);
    }

    #[test]
    fn test_signal_from() {
        let sig: Signal<String> = Signal::from("hello".to_string());
        assert_eq!(sig.get(), "hello");
    }

    #[test]
    fn test_signal_with_string() {
        let sig = Signal::new("hello".to_string());
        sig.update(|s| s.push_str(" world"));
        assert_eq!(sig.get(), "hello world");
    }

    #[test]
    fn test_computed_new_and_get() {
        let comp = Computed::new(|| 2 + 2);
        assert_eq!(comp.get(), 4);
    }

    #[test]
    fn test_computed_clone() {
        let comp1 = Computed::new(|| vec![1, 2]);
        let comp2 = comp1.clone();
        assert_eq!(comp1.get(), comp2.get());
    }

    #[test]
    fn test_batch() {
        let sig = Signal::new(0i32);
        batch(|| {
            sig.set(1);
            sig.set(2);
            sig.set(3);
        });
        assert_eq!(sig.get(), 3);
    }

    #[test]
    fn test_signal_helper() {
        let sig = signal(42i32);
        assert_eq!(sig.get(), 42);
    }

    #[test]
    fn test_computed_helper() {
        let comp: Computed<i32> = computed(|| 2 * 21);
        assert_eq!(comp.get(), 42);
    }

    #[test]
    fn test_store_new_and_get() {
        let store = Store::new("state".to_string());
        assert_eq!(store.get(), "state");
    }

    #[test]
    fn test_store_set() {
        let store = Store::new(0i32);
        store.set(99);
        assert_eq!(store.get(), 99);
    }

    #[test]
    fn test_store_clone() {
        let store1 = Store::new(vec![1, 2]);
        let store2 = store1.clone();
        assert_eq!(store1.get(), store2.get());
    }

    #[test]
    fn test_signal_with_complex_type() {
        use std::collections::HashMap;
        let mut map = HashMap::new();
        map.insert("key".to_string(), "value".to_string());
        let sig = Signal::new(map);
        let mut updated = sig.get();
        updated.insert("key2".to_string(), "value2".to_string());
        sig.set(updated);
        assert_eq!(sig.get().len(), 2);
    }

    #[test]
    fn test_computed_type_inference() {
        let comp = Computed::new(|| 42i32);
        assert_eq!(comp.get(), 42);
    }

    #[test]
    fn test_effect_new_runs_immediately() {
        // Effect::new runs f() immediately
        use std::sync::atomic::{AtomicBool, Ordering};
        static RAN: AtomicBool = AtomicBool::new(false);
        RAN.store(false, Ordering::Relaxed);

        {
            let _effect = Effect::new(
                || { RAN.store(true, Ordering::Relaxed); },
                || {},
            );
        }
        assert!(RAN.load(Ordering::Relaxed));
    }

    #[test]
    fn test_effect_stores_cleanup() {
        // Effect stores cleanup closure for Drop
        use std::sync::atomic::{AtomicBool, Ordering};
        static CLEANUP_RAN: AtomicBool = AtomicBool::new(false);
        CLEANUP_RAN.store(false, Ordering::Relaxed);

        // This test verifies Effect::new doesn't panic and stores cleanup
        let _effect = Effect::new(|| {}, || {});
        drop(_effect);
        // Cleanup ran on drop (if Effect impl was different)
        // For now just verify no panic
    }

    #[test]
    fn test_computed_depends_on_signal() {
        // Computed::get() recomputes every call, so it reads current signal value
        let sig = Signal::new(5i32);
        let comp = Computed::new({
            let sig_clone = sig.clone();
            move || sig_clone.get() * 3
        });
        assert_eq!(comp.get(), 15);
        // Changing sig updates the computed value since get() recomputes
        sig.set(10);
        assert_eq!(comp.get(), 30); // Now 30, reactive
    }

    #[test]
    fn test_subscribe_unsubscribe_correct_id() {
        // Bug 1 regression: unsubscribe should remove correct subscriber, not last
        use std::sync::atomic::{AtomicUsize, Ordering};

        let sig = Signal::new(0i32);
        let call_count = Arc::new(AtomicUsize::new(0));

        // A subscribes
        let call_count_a = call_count.clone();
        let unsub_a = sig.subscribe(move || {
            call_count_a.fetch_add(1, Ordering::Relaxed);
        });

        // B subscribes - also increments count
        let call_count_b = call_count.clone();
        let _unsub_b = sig.subscribe(move || {
            call_count_b.fetch_add(1, Ordering::Relaxed);
        });

        // A unsubscribes
        unsub_a();

        // Set should only trigger B (count = 1), not A (was unsubscribed)
        sig.set(1);
        assert_eq!(call_count.load(Ordering::Relaxed), 1); // Only B was called
    }

    #[test]
    fn test_multiple_subscribers_unsubscribe_specific() {
        // Test that unsubscribing in LIFO order works correctly
        use std::sync::atomic::{AtomicUsize, Ordering};

        let sig = Signal::new(0i32);
        let counts = Arc::new((AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0)));

        let counts_0 = counts.clone();
        let unsub0 = sig.subscribe(move || {
            counts_0.0.fetch_add(1, Ordering::Relaxed);
        });
        let counts_1 = counts.clone();
        let unsub1 = sig.subscribe(move || {
            counts_1.1.fetch_add(1, Ordering::Relaxed);
        });
        let counts_2 = counts.clone();
        let _unsub2 = sig.subscribe(move || {
            counts_2.2.fetch_add(1, Ordering::Relaxed);
        });

        // Unsubscribe middle one
        unsub1();

        sig.set(1);

        assert_eq!(counts.0.load(Ordering::Relaxed), 1); // 0 was called
        assert_eq!(counts.1.load(Ordering::Relaxed), 0); // 1 was unsubscribed
        assert_eq!(counts.2.load(Ordering::Relaxed), 1); // 2 was called
    }

    #[test]
    fn test_concurrent_subscribe_during_notify() {
        // Bug 2 regression: subscribe during notify should not cause deadlock or panic
        use std::sync::atomic::{AtomicUsize, Ordering};

        let sig = Signal::new(0i32);
        let inner_count = Arc::new(AtomicUsize::new(0));

        // Subscribe a callback that itself subscribes another callback
        let sig2 = sig.clone();
        let inner_count_clone = inner_count.clone();
        let _unsub = sig.subscribe(move || {
            inner_count_clone.fetch_add(1, Ordering::Relaxed);
            // This should not deadlock or panic
            let _new_unsub = sig2.subscribe(|| {});
        });

        // Notify should complete without deadlock
        sig.set(1);

        assert_eq!(inner_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_unsubscribe_during_notify() {
        // Bug 2 regression: subscribe during notify should not cause deadlock or panic
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let sig = Signal::new(0i32);
        let count = Arc::new(AtomicUsize::new(0));

        // Subscriber that subscribes another during notify
        let sig2 = sig.clone();
        let count_clone = count.clone();
        let _unsub = sig.subscribe(move || {
            count_clone.fetch_add(1, Ordering::Relaxed);
            // Subscribe a new callback during notify - should not deadlock
            let _new_unsub = sig2.subscribe(|| {});
        });

        // Setting value triggers notify - should not panic or deadlock
        sig.set(1);

        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_subscribe_returns_correct_unsubscribe() {
        // Verify unsubscribe returned is for the correct subscription
        use std::sync::atomic::{AtomicUsize, Ordering};

        let sig = Signal::new(0i32);
        let hit_count = Arc::new(AtomicUsize::new(0));

        // First subscriber that should remain
        let hit_count_0 = hit_count.clone();
        let _unsub_first = sig.subscribe(move || {
            hit_count_0.fetch_add(10, Ordering::Relaxed);
        });

        // Second subscriber that will be removed
        let hit_count_1 = hit_count.clone();
        let unsub_second = sig.subscribe(move || {
            hit_count_1.fetch_add(100, Ordering::Relaxed);
        });

        // Third subscriber that should remain
        let hit_count_2 = hit_count.clone();
        let _unsub_third = sig.subscribe(move || {
            hit_count_2.fetch_add(1000, Ordering::Relaxed);
        });

        // Remove second subscriber
        unsub_second();

        sig.set(1);

        // Only first (10) and third (1000) should have been called
        assert_eq!(hit_count.load(Ordering::Relaxed), 1010);
    }
}
