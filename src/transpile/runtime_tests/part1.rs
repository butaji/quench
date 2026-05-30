//! Runtime tests - part 1

#![allow(clippy::too_many_lines)]
#[cfg(test)]
mod part1_tests {
    use std::collections::HashMap;
    // ========================================================================
    // LITERAL TESTS
    // ========================================================================

    #[test]
    fn test_string_literals() {
        let _s1: String = String::new();
        let _s2 = "hello".to_string();
        let _s3 = "world".to_string();
        let _s4 = "template".to_string();

        // Escape sequences
        let _s5 = "line1\nline2".to_string();
        let _s6 = "tab\there".to_string();
        let _s7 = "quote\"escaped".to_string();
    }

    #[test]
    fn test_number_literals() {
        let _n1: i32 = 0;
        let _n2: i32 = 42;
        let _n3: i32 = -17;
        let _n4: f64 = 3.14;
        let _n5: f64 = -2.5;
        let _n6: f64 = 1e10;
        let _n7: i32 = 0xFF;
        let _n8: i32 = 0b1010;
        let _n9: i32 = 0o777;
    }

    #[test]
    fn test_boolean_literals() {
        let _t: bool = true;
        let _f: bool = false;
    }

    #[test]
    fn test_null_undefined() {
        let _n: Option<i32> = None;
        let _u: Option<i32> = None;
    }

    // ========================================================================
    // EXPRESSION TESTS
    // ========================================================================

    #[test]
    fn test_binary_arithmetic() {
        let a: i32 = 10;
        let b: i32 = 3;

        let _add: i32 = a + b;
        let _sub: i32 = a - b;
        let _mul: i32 = a * b;
        let _div: i32 = a / b;
        let _mod: i32 = a % b;
    }

    #[test]
    fn test_binary_comparison() {
        let a: i32 = 5;
        let b: i32 = 3;

        let _eq: bool = a == 5;
        let _neq: bool = a != b;
        let _strict_eq: bool = a == 5;
        let _strict_neq: bool = a != b;
        let _lt: bool = a < 10;
        let _lte: bool = a <= 5;
        let _gt: bool = a > 3;
        let _gte: bool = a >= 5;
    }

    #[test]
    fn test_binary_logical() {
        let t: bool = true;
        let f: bool = false;

        let _and: bool = t && f;
        let _or: bool = t || f;
        let _not: bool = !f;
    }

    #[test]
    fn test_nullish_coalescing() {
        let _a: Option<&str> = None.or(Some("default"));
        let _b: Option<i32> = None;
        let _c = _b.unwrap_or(0);
    }

    #[test]
    fn test_binary_bitwise() {
        let a: u8 = 0b1100;
        let b: u8 = 0b1010;

        let _and: u8 = a & b;
        let _or: u8 = a | b;
        let _xor: u8 = a ^ b;
    }

    #[test]
    fn test_shift_operators() {
        let a: u8 = 1;
        let _left: u8 = a << 3;
        let _right: u8 = 8 >> 2;
    }

    // ========================================================================
    // ARRAY TESTS
    // ========================================================================

    #[test]
    fn test_array_literal() {
        let _arr1: Vec<i32> = vec![];
        let _arr2 = vec![1, 2, 3];
        let _arr3 = vec![1, 2, 3, 4, 5];
        let _arr4: Vec<i32> = (0..10).collect();
    }

    #[test]
    fn test_array_access() {
        let arr = vec![10, 20, 30];
        let _first = arr[0];
        let _second = arr[1];
        let _last = arr[2];
    }

    #[test]
    fn test_array_methods() {
        let arr = vec![3, 1, 4, 1, 5, 9, 2, 6];

        let _doubled: Vec<i32> = arr.iter().map(|x| x * 2).collect();
        let _evens: Vec<i32> = arr.iter().filter(|x| *x % 2 == 0).copied().collect();
        let _sum: i32 = arr.iter().sum();
        let _found = arr.iter().find(|x| **x > 5);
        let _has_five = arr.contains(&5);
    }

    #[test]
    fn test_array_spread() {
        let a = vec![1, 2, 3];
        let b = vec![4, 5, 6];

        let mut combined = a.clone();
        combined.extend(b.iter().copied());
        let _result = combined;
    }

    // ========================================================================
    // OBJECT/HASHMAP TESTS
    // ========================================================================

    #[test]
    fn test_object_literal() {
        let mut obj: HashMap<&str, i32> = HashMap::new();
        obj.insert("a", 1);
        obj.insert("b", 2);
        let _result = obj;
    }

    #[test]
    fn test_object_access() {
        let mut obj: HashMap<&str, i32> = HashMap::new();
        obj.insert("name", 42);
        obj.insert("age", 30);

        let _name = obj.get("name").copied();
        let _age = obj.get("age").copied();
    }

    #[test]
    fn test_nested_objects() {
        let mut inner: HashMap<&str, &str> = HashMap::new();
        inner.insert("city", "NYC");

        let mut outer: HashMap<&str, HashMap<&str, &str>> = HashMap::new();
        outer.insert("address", inner);

        let _city = outer.get("address").and_then(|m| m.get("city")).copied();
    }

    // ========================================================================
    // FUNCTION TESTS
    // ========================================================================

    #[test]
    fn test_function_declaration() {
        fn add(a: i32, b: i32) -> i32 {
            a + b
        }
        let _result = add(5, 3);
    }

    #[test]
    fn test_arrow_function() {
        let add = |a: i32, b: i32| a + b;
        let _result = add(5, 3);

        let square = |x: i32| x * x;
        let _sq = square(4);
    }

    #[test]
    fn test_closure() {
        let mut counter = 0;
        let mut increment = || {
            counter += 1;
            counter
        };

        let _first = increment();
        let _second = increment();
    }

    #[test]
    fn test_higher_order_function() {
        let nums = vec![1, 2, 3, 4, 5];
        let _evens: Vec<&i32> = nums.iter().filter(|x| *x % 2 == 0).collect();

        let add_n = |n: i32| move |x: i32| x + n;
        let add_5 = add_n(5);
        let _result = add_5(10);
    }

    // ========================================================================
    // CONTROL FLOW TESTS
    // ========================================================================

    #[test]
    fn test_if_else() {
        let x: i32 = 10;

        let _result = if x > 5 {
            "big"
        } else if x > 0 {
            "small"
        } else {
            "negative"
        };
    }

    #[test]
    fn test_match() {
        let day: i32 = 3;

        let _name = match day {
            1 => "Monday",
            2 => "Tuesday",
            3 => "Wednesday",
            4 => "Thursday",
            5 => "Friday",
            6 | 7 => "Weekend",
            _ => "Unknown",
        };
    }

    #[test]
    fn test_loop() {
        let mut sum = 0i32;
        for i in 1..=5 {
            sum += i;
        }
        assert_eq!(sum, 15);
    }

    #[test]
    fn test_while_loop() {
        let mut count = 0;
        while count < 3 {
            count += 1;
        }
        assert_eq!(count, 3);
    }

    #[test]
    fn test_nested_loops() {
        let mut result = Vec::new();
        for i in 0..2 {
            for j in 0..2 {
                result.push((i, j));
            }
        }
        assert_eq!(result.len(), 4);
    }

    // ========================================================================
    // OPTIONAL CHAINING TESTS
    // ========================================================================

    #[test]
    fn test_optional_chaining() {
        let obj: Option<HashMap<&str, i32>> = None;
        let _value = obj.as_ref().and_then(|m| m.get("key")).copied();

        let mut map: HashMap<&str, i32> = HashMap::new();
        map.insert("a", 1);
        let obj2 = Some(map);
        let _v = obj2.as_ref().and_then(|m| m.get("a")).copied();
    }

    // ========================================================================
    // DESTRUCTURING TESTS
    // ========================================================================

    #[test]
    fn test_array_destructuring() {
        let arr = vec![1, 2, 3];
        let (first, second, third) = (arr[0], arr[1], arr[2]);
        assert_eq!(first, 1);
        assert_eq!(second, 2);
        assert_eq!(third, 3);
    }

    #[test]
    fn test_object_destructuring() {
        let mut map: HashMap<&str, i32> = HashMap::new();
        map.insert("name", 42);
        map.insert("age", 30);

        let name = map.get("name").copied().unwrap_or(0);
        let age = map.get("age").copied().unwrap_or(0);

        assert_eq!(name, 42);
        assert_eq!(age, 30);
    }

    // ========================================================================
    // STRUCT TESTS
    // ========================================================================

    #[test]
    fn test_struct_creation() {
        #[derive(Debug, Clone, PartialEq)]
        struct Point {
            x: f64,
            y: f64,
        }

        let p = Point { x: 3.0, y: 4.0 };
        assert_eq!(p.x, 3.0);
        assert_eq!(p.y, 4.0);
    }

    #[test]
    fn test_struct_methods() {
        #[derive(Debug, Clone)]
        struct Point {
            x: f64,
            y: f64,
        }

        impl Point {
            fn new(x: f64, y: f64) -> Self {
                Self { x, y }
            }
            fn distance(&self) -> f64 {
                (self.x * self.x + self.y * self.y).sqrt()
            }
        }

        let p = Point::new(3.0, 4.0);
        assert_eq!(p.distance(), 5.0);
    }

    #[test]
    fn test_struct_nested() {
        #[derive(Debug, Clone)]
        struct Base {
            name: String,
        }

        #[derive(Debug, Clone)]
        struct Derived {
            base: Base,
            value: i32,
        }

        let d = Derived {
            base: Base {
                name: "test".to_string(),
            },
            value: 42,
        };
        assert_eq!(d.base.name, "test");
        assert_eq!(d.value, 42);
    }

    // ========================================================================
}
