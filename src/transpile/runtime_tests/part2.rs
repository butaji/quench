//! Runtime tests - part 2

#![allow(clippy::too_many_lines)]
#[cfg(test)]
mod part2_tests {
    use std::collections::{HashMap, HashSet};
    // ERROR HANDLING TESTS
    // ========================================================================

    #[test]
    fn test_option_handling() {
        let value: Option<i32> = Some(42);
        let _result = match value {
            Some(n) => n * 2,
            None => 0,
        };
    }

    #[test]
    fn test_result_handling() {
        fn divide(a: i32, b: i32) -> Option<i32> {
            if b == 0 {
                None
            } else {
                Some(a / b)
            }
        }

        let _ok = divide(10, 2);
        let _err = divide(10, 0);
    }

    #[test]
    fn test_option_map_filter() {
        let value = Some(5);
        let _doubled = value.map(|n| n * 2);
        let _filtered = value.filter(|n| *n > 3);
        let _default = value.unwrap_or(0);
    }

    // ========================================================================
    // ITERATOR TESTS
    // ========================================================================

    #[test]
    fn test_iterator_chain() {
        let nums = vec![1, 2, 3, 4, 5];
        let result: i32 = nums.iter().filter(|x| *x % 2 == 0).map(|x| x * x).sum();
        assert_eq!(result, 20);
    }

    #[test]
    fn test_iterator_any_all() {
        let nums = vec![2, 4, 6, 8];
        let _all_even = nums.iter().all(|x| *x % 2 == 0);
        let _any_divisible = nums.iter().any(|x| *x % 4 == 0);
    }

    #[test]
    fn test_iterator_fold() {
        let nums = vec![1, 2, 3, 4, 5];
        let _sum = nums.iter().fold(0, |acc, x| acc + x);
        let _product = nums.iter().fold(1, |acc, x| acc * x);
    }

    // ========================================================================
    // STRING TESTS
    // ========================================================================

    #[test]
    fn test_string_operations() {
        let s = "Hello, World!";
        let _upper = s.to_uppercase();
        let _lower = s.to_lowercase();
        let _contains = s.contains("World");
        let _starts = s.starts_with("Hello");
        let _ends = s.ends_with("!");
        let _split: Vec<&str> = s.split(", ").collect();
        let _replace = s.replace("World", "Rust");
    }

    #[test]
    fn test_string_interpolation() {
        let name = "Alice";
        let age = 30;
        let _greeting = format!("Hello, {}! You are {} years old.", name, age);
    }

    // ========================================================================
    // COLLECTION TESTS
    // ========================================================================

    #[test]
    fn test_hashmap_operations() {
        let mut map: HashMap<&str, i32> = HashMap::new();
        map.insert("a", 1);
        map.insert("b", 2);
        let _val = map.get("a").copied();
        *map.entry("a").or_insert(0) += 1;
        map.remove("b");
        let _keys: Vec<&&str> = map.keys().collect();
        let _values: Vec<&i32> = map.values().collect();
    }

    #[test]
    fn test_vec_operations() {
        let mut v = vec![1, 2, 3];
        v.push(4);
        let _last = v.pop();
        v.insert(0, 0);
        v.remove(1);
        v.push(5);
        v.sort();
    }

    #[test]
    fn test_hashset_operations() {
        let mut set: HashSet<i32> = vec![1, 2, 3].into_iter().collect();
        let _has = set.contains(&2);
        set.insert(4);
        set.remove(&1);
        let _len = set.len();
        let _is_empty = set.is_empty();
    }

    // ========================================================================
    // PERFORMANCE TESTS
    // ========================================================================

    #[test]
    fn test_loop_performance() {
        let mut sum = 0i64;
        for i in 0..1_000_000 {
            sum += i;
        }
        assert!(sum > 0);
    }

    #[test]
    fn test_vec_alloc_performance() {
        let v: Vec<i32> = (0..1000).collect();
        assert_eq!(v.len(), 1000);
    }

    #[test]
    fn test_hashmap_lookup_performance() {
        let mut map: HashMap<i32, i32> = HashMap::new();
        for i in 0..1000 {
            map.insert(i, i * 2);
        }

        let mut sum = 0i32;
        for i in 0..1000 {
            sum += map[&i];
        }
        assert_eq!(sum, 999000);
    }

    #[test]
    fn test_iterator_benchmark() {
        let nums: Vec<i32> = (0..10000).collect();
        let result: i32 = nums
            .iter()
            .filter(|x| *x % 2 == 0)
            .map(|x| x * x)
            .take(100)
            .sum();
        assert!(result > 0);
    }

    // ========================================================================
    // TYPE COERCION TESTS
    // ========================================================================

    #[test]
    fn test_integer_conversions() {
        let i: i32 = 42;
        let _f: f64 = i as f64;
        let _u: u8 = i as u8;
    }

    #[test]
    fn test_string_conversions() {
        let n = 42;
        let _s = n.to_string();
        let s = "123";
        let _parsed: i32 = s.parse().unwrap_or(0);
    }

    // ========================================================================
    // ADVANCED PATTERNS
    // ========================================================================

    #[test]
    fn test_lazy_evaluation() {
        let nums = vec![1, 2, 3, 4, 5];
        let _evens = nums.iter().filter(|x| *x % 2 == 0);
        let _collected: Vec<&i32> = nums.iter().filter(|x| *x % 2 == 0).collect();
    }

    #[test]
    fn test_lazy_evaluation_with_map() {
        let nums = vec![1, 2, 3, 4, 5];
        let result: Vec<i32> = nums
            .iter()
            .map(|x| x * 2)
            .filter(|x| *x > 4)
            .take(2)
            .collect();
        assert_eq!(result, vec![6, 8]);
    }

    #[test]
    fn test_fold_vs_sum() {
        let nums = vec![1, 2, 3, 4, 5];
        let sum_fold = nums.iter().fold(0, |acc, x| acc + x);
        let sum: i32 = nums.iter().sum();
        assert_eq!(sum_fold, sum);
        assert_eq!(sum, 15);
    }

    // ========================================================================
    // RECURSION TESTS
    // ========================================================================

    #[test]
    fn test_recursive_fibonacci() {
        fn fib(n: u32) -> u64 {
            match n {
                0 => 0,
                1 => 1,
                _ => fib(n - 1) + fib(n - 2),
            }
        }

        assert_eq!(fib(0), 0);
        assert_eq!(fib(1), 1);
        assert_eq!(fib(10), 55);
    }

    #[test]
    fn test_tail_recursive_factorial() {
        fn factorial(n: u64) -> u64 {
            fn fact_helper(n: u64, acc: u64) -> u64 {
                if n <= 1 {
                    acc
                } else {
                    fact_helper(n - 1, n * acc)
                }
            }
            fact_helper(n, 1)
        }

        assert_eq!(factorial(0), 1);
        assert_eq!(factorial(5), 120);
    }

    // ========================================================================
    // MEMORY EFFICIENCY TESTS
    // ========================================================================

    #[test]
    fn test_zero_copy_slices() {
        let text = "Hello, World!";
        let slice = &text[0..5];
        assert_eq!(slice, "Hello");
    }

    #[test]
    fn test_stack_allocated_arrays() {
        let arr: [i32; 4] = [1, 2, 3, 4];
        let _sum: i32 = arr.iter().sum();
    }

    #[test]
    fn test_reference_counting() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let rc = Rc::new(RefCell::new(vec![1, 2, 3]));
        let _rc2 = rc.clone();

        rc.borrow_mut().push(4);
        assert_eq!(*rc.borrow(), vec![1, 2, 3, 4]);
        assert_eq!(Rc::strong_count(&rc), 2);
    }

    #[test]
    fn test_atomic_reference_counting() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let arc = Arc::new(AtomicUsize::new(0));
        let _arc2 = arc.clone();

        arc.fetch_add(1, Ordering::SeqCst);
        assert_eq!(arc.load(Ordering::SeqCst), 1);
        assert_eq!(Arc::strong_count(&arc), 2);
    }
}
