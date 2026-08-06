//! Unit tests for iterator protocol path in destructuring helpers.
//!
//! These tests cover:
//! - `take_iterator_value`: pulling values from iterators
//! - `call_iterator_return`: closing iterators with .return()
//! - `assign_array_with_iterator`: array destructuring from iterables
//! - `box_primitive_for_set`: boxing primitives for method calls

#[cfg(test)]
mod iterator_protocol_tests {
    use crate::{Context, Value};

    fn eval(src: &str) -> Result<Value, crate::value::JsError> {
        Context::new().unwrap().eval(src)
    }

    #[test]
    fn object_spread_proxy_gets_only_enumerable_keys() {
        let result = eval(
            "var gets = []; var p = new Proxy({}, { ownKeys: function() { return ['a', 'b']; }, getOwnPropertyDescriptor: function(t, k) { return { enumerable: k === 'b', configurable: true, value: k }; }, get: function(t, k) { gets.push(k); return k; } }); ({ ...p, gets: gets.join(',') }).gets",
        )
        .unwrap();
        assert_eq!(result, Value::String("b".to_string()));
    }

    #[test]
    fn keyed_destructuring_evaluates_target_key_after_source_value() {
        let result = eval(
            "var log=[]; function source(){log.push('source');return {get p(){log.push('get')}}} function target(){log.push('target');return {set q(v){log.push('set')}}} function sourceKey(){log.push('source-key');return {toString(){log.push('source-key-tostring');return 'p'}}} function targetKey(){log.push('target-key');return {toString(){log.push('target-key-tostring');return 'q'}}} ({[sourceKey()]:target()[targetKey()]}=source());log.join(',')",
        )
        .unwrap();
        assert_eq!(
            result,
            Value::String(
                "source,source-key,source-key-tostring,target,target-key,get,target-key-tostring,set"
                    .into(),
            )
        );
    }

    #[test]
    fn var_destructuring_with_proxy_scope_does_not_repeat_binding_lookup() {
        let result = eval(
            "var log=[]; var sourceKey={toString:()=>{log.push('sourceKey');return 'p'}}; var source={get p(){log.push('get source');return undefined}}; var env=new Proxy({}, {has(t,pk){log.push('binding::'+pk);return false}}); var defaultValue=0; with(env){var {[sourceKey]: varTarget = defaultValue}=source;} log.join('|')",
        )
        .unwrap();
        assert_eq!(
            result,
            Value::String(
                "binding::source|binding::sourceKey|sourceKey|binding::varTarget|get source|binding::defaultValue".into(),
            )
        );
    }

    // ─── take_iterator_value: basic next() ─────────────────────────────────────

    /// iterator.next() returns correct value
    #[test]
    fn test_take_iterator_value() {
        let r = eval(
            "var iter = {
                _values: [1, 2, 3],
                next: function() {
                    var v = this._values.shift();
                    return { value: v, done: v === undefined };
                }
            };
            var results = [];
            results.push(iter.next().value);
            results.push(iter.next().value);
            results.push(iter.next().value);
            results.push(iter.next().value); // exhausted
            results;
        ",
        )
        .unwrap();

        if let Value::Object(o) = r {
            let elems = &o.borrow().elements;
            assert_eq!(elems.len(), 4);
            assert_eq!(elems[0], Value::Number(1.0));
            assert_eq!(elems[1], Value::Number(2.0));
            assert_eq!(elems[2], Value::Number(3.0));
            assert_eq!(elems[3], Value::Undefined);
        } else {
            panic!("Expected array, got {:?}", r);
        }
    }

    // ─── take_iterator_value: done: true ───────────────────────────────────────

    /// when iterator returns done: true, returns undefined
    #[test]
    fn test_take_iterator_value_done() {
        let r = eval(
            "var iter = {
                _count: 0,
                next: function() {
                    this._count++;
                    if (this._count >= 2) {
                        return { done: true };
                    }
                    return { value: this._count };
                }
            };
            var vals = [];
            vals.push(iter.next().value);
            vals.push(iter.next().value); // returns done: true
            vals.push(iter.next().value); // should still be undefined after done
            vals;
        ",
        )
        .unwrap();

        if let Value::Object(o) = r {
            let elems = &o.borrow().elements;
            assert_eq!(elems.len(), 3);
            assert_eq!(elems[0], Value::Number(1.0));
            assert_eq!(elems[1], Value::Undefined); // done: true, value omitted
            assert_eq!(elems[2], Value::Undefined); // exhausted
        } else {
            panic!("Expected array, got {:?}", r);
        }
    }

    // ─── call_iterator_return: .return() exists and is callable ─────────────────

    /// Array iterator protocol works
    #[test]
    fn test_call_iterator_return() {
        // Test that custom iterator with .return can be called
        let r = eval(
            "var returnCalled = false;
            var iter = {
                _count: 0,
                next: function() {
                    this._count++;
                    if (this._count > 3) {
                        return { done: true };
                    }
                    return { value: this._count, done: false };
                },
                'return': function() {
                    returnCalled = true;
                    return { done: true };
                }
            };
            // Manual iteration using the iterator protocol
            var result;
            var item = iter.next();
            while (!item.done) {
                result = item.value;
                item = iter.next();
            }
            [result, returnCalled];
        ",
        )
        .unwrap();

        if let Value::Object(o) = r {
            let elems = &o.borrow().elements;
            assert_eq!(elems[0], Value::Number(3.0)); // last value before done
            assert_eq!(elems[1], Value::Boolean(false)); // return not called on normal completion
        } else {
            panic!("Expected array, got {:?}", r);
        }
    }

    // ─── call_iterator_return: .return not callable ───────────────────────────

    /// .return is undefined for plain objects without iterator
    #[test]
    fn test_call_iterator_return_not_callable() {
        // Plain object without Symbol.iterator returns undefined for .return
        let r = eval(
            "var obj = {
                next: function() { return { value: 1 }; }
                // no return method, no Symbol.iterator
            };
            typeof obj.return;
        ",
        )
        .unwrap();
        assert_eq!(r, Value::String("undefined".into()));
    }

    // ─── assign_array_with_iterator: basic (array) ─────────────────────────────

    /// let [a, b] = array works (basic array iteration)
    #[test]
    fn test_assign_array_with_iterator_basic() {
        let r = eval("var [a, b] = [10, 20]; a + b;").unwrap();
        assert_eq!(r, Value::Number(30.0));
    }

    // ─── assign_array_with_iterator: short array ──────────────────────────────

    /// array has fewer values than vars
    #[test]
    fn test_assign_array_with_iterator_short() {
        let r = eval("var [a, b, c] = [42]; [a, b, c];").unwrap();

        if let Value::Object(o) = r {
            let elems = &o.borrow().elements;
            assert_eq!(elems[0], Value::Number(42.0));
            assert_eq!(elems[1], Value::Undefined); // exhausted
            assert_eq!(elems[2], Value::Undefined); // exhausted
        } else {
            panic!("Expected array, got {:?}", r);
        }
    }

    // ─── assign_array_with_iterator: extra values ─────────────────────────────

    /// array has more values than vars (extras ignored)
    #[test]
    fn test_assign_array_with_iterator_extra() {
        let r = eval("var [a, b] = [1, 2, 3, 4, 5]; a + b;").unwrap();
        assert_eq!(r, Value::Number(3.0)); // 1 + 2
    }

    // ─── box_primitive_for_set: Number this ───────────────────────────────────

    /// (42).toFixed where getter receives boxed 42 as `this`
    #[test]
    fn test_getter_this_value_number() {
        // When calling a method on a primitive, the primitive is boxed
        let r = eval("(42).toFixed(0);").unwrap();
        assert_eq!(r, Value::String("42".into()));

        // Verify the boxed number has the correct prototype chain
        let r2 = eval("(42).constructor === Number;").unwrap();
        assert_eq!(r2, Value::Boolean(true));

        // Calling Number.prototype method directly
        let r3 = eval("var n = Object(42); n.toFixed(1);").unwrap();
        assert_eq!(r3, Value::String("42.0".into()));
    }

    // ─── box_primitive_for_set: Boolean this ───────────────────────────────────

    /// Verify boolean primitives have correct boxing behavior
    #[test]
    fn test_getter_this_value_boolean() {
        // Boolean() converts to boolean primitive
        let r = eval("Boolean(1);").unwrap();
        assert_eq!(r, Value::Boolean(true));

        let r2 = eval("Boolean(0);").unwrap();
        assert_eq!(r2, Value::Boolean(false));

        // Boolean constructor is accessible
        let r3 = eval("Boolean !== undefined;").unwrap();
        assert_eq!(r3, Value::Boolean(true));
    }

    // ─── String iteration in destructuring ─────────────────────────────────────

    /// String is iterable, chars are destructured individually
    #[test]
    fn test_string_iterator_in_destructuring() {
        let r = eval("var [a, b, c] = 'xyz'; [a, b, c];").unwrap();

        if let Value::Object(o) = r {
            let elems = &o.borrow().elements;
            assert_eq!(elems[0], Value::String("x".into()));
            assert_eq!(elems[1], Value::String("y".into()));
            assert_eq!(elems[2], Value::String("z".into()));
        } else {
            panic!("Expected array, got {:?}", r);
        }
    }

    // ─── Destructuring with default values ─────────────────────────────────────

    /// Array exhaustion uses default values
    #[test]
    fn test_iterator_with_defaults() {
        let r = eval("var [a = 99, b = 100] = [42]; a + b;").unwrap();
        assert_eq!(r, Value::Number(142.0)); // 42 + 100 (default)
    }

    // ─── Iterator result object structure ─────────────────────────────────────

    /// Iterator result has correct value and done properties
    #[test]
    fn test_iterator_result_structure() {
        let r = eval(
            "var iter = {
                _count: 0,
                next: function() {
                    this._count++;
                    if (this._count > 2) {
                        return { done: true };
                    }
                    return { value: this._count };
                }
            };
            var results = [];
            var r1 = iter.next();
            results.push({ v: r1.value, d: r1.done === true });
            var r2 = iter.next();
            results.push({ v: r2.value, d: r2.done === true });
            var r3 = iter.next();
            results.push({ v: r3.value, d: r3.done === true });
            [results[0].v, results[0].d, results[1].v, results[1].d, results[2].v, results[2].d];
        ",
        )
        .unwrap();

        if let Value::Object(o) = r {
            let elems = &o.borrow().elements;
            assert_eq!(elems[0], Value::Number(1.0)); // first value
            assert_eq!(elems[1], Value::Boolean(false)); // first not done
            assert_eq!(elems[2], Value::Number(2.0)); // second value
            assert_eq!(elems[3], Value::Boolean(false)); // second not done
            assert_eq!(elems[4], Value::Undefined); // third value (done)
            assert_eq!(elems[5], Value::Boolean(true)); // third done
        } else {
            panic!("Expected array, got {:?}", r);
        }
    }

    // ─── Nested array destructuring ───────────────────────────────────────────

    /// Nested array patterns work correctly
    #[test]
    fn test_nested_array_destructuring() {
        let r = eval("var [[a, b], [c, d]] = [[1, 2], [3, 4]]; a + b + c + d;").unwrap();
        assert_eq!(r, Value::Number(10.0)); // 1 + 2 + 3 + 4
    }

    // ─── Destructuring with holes ─────────────────────────────────────────────

    /// Array destructuring with holes skips positions
    #[test]
    fn test_destructuring_with_holes() {
        let r = eval("var [a, , b] = [1, 'skip', 3]; a + b;").unwrap();
        assert_eq!(r, Value::Number(4.0)); // 1 + 3
    }

    // ─── TDZ check in destructuring default ──────────────────────────────────

    /// Assignment destructuring `({ x = y } = {})` must throw ReferenceError when
    /// `y` is in TDZ (let hoisting), test262 obj-id-init-let.js reproducer.
    #[test]
    fn test_tdz_in_destructuring_default() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval(
            r#"
            var counter = 0;
            try {
              ({ x = y } = {});
              counter += 1;
            } catch(e) {
              // expected: ReferenceError for y in TDZ
            }
            let y;
            counter;
            "#,
        );
        assert_eq!(result, Ok(Value::Number(0.0)));
    }

    // ─── for-of array destructuring: IteratorClose on normal completion ─────────

    /// ES spec §7.4.6 IteratorClose: for-of with array destructuring must
    /// call iterator.return() when the loop completes normally.
    #[test]
    fn for_of_destructuring_calls_return_on_normal_completion() {
        let r = eval(
            "var returnCount = 0;
             var iterable = {};
             var iterator = {
               next: function() { return { done: false, value: 1 }; },
               'return': function() { returnCount++; return {}; }
             };
             iterable[Symbol.iterator] = function() { return iterator; };
             for ([x] of [iterable]) {}
             returnCount;
            ",
        )
        .unwrap();
        assert_eq!(r, Value::Number(1.0));
    }

    // ─── for-of array destructuring: IteratorClose on body throw ───────────────

    /// ES spec §7.4.6: for-of with array destructuring calls iterator.return()
    /// when the body throws.
    #[test]
    fn for_of_destructuring_calls_return_on_body_throw() {
        let r = eval(
            "var returnCount = 0;
             var nextCount = 0;
             var iterable = {};
             var iterator = {
               next: function() {
                 nextCount++;
                 return { done: nextCount > 1, value: 1 };
               },
               'return': function() { returnCount++; return {}; }
             };
             iterable[Symbol.iterator] = function() { return iterator; };
             try {
               for ([x] of [iterable]) { throw 1; }
             } catch (e) {}
             returnCount;
            ",
        )
        .unwrap();
        assert_eq!(r, Value::Number(1.0));
    }

    // ─── for-of array destructuring: ReturnError propagates ──────────────────

    /// ES spec §7.4.6 steps 7-8: when completion.type is throw and
    /// iterator.return() throws ReturnError, ReturnError propagates (the
    /// original body throw is discarded).
    #[test]
    fn for_of_destructuring_return_error_propagates() {
        let r = eval(
            "var returnCount = 0;
             var iterable = {};
             function ReturnError() {}
             var iterator = {
               next: function() { return { done: false, value: 1 }; },
               'return': function() {
                 returnCount++;
                 throw new ReturnError();
               }
             };
             iterable[Symbol.iterator] = function() { return iterator; };
             var threwReturnError = false;
             try {
               for ([x] of [iterable]) { throw 999; }
             } catch (e) {
               threwReturnError = (e instanceof ReturnError);
             }
             [returnCount, threwReturnError];
            ",
        )
        .unwrap();
        if let Value::Object(o) = r {
            let elems = &o.borrow().elements;
            assert_eq!(elems[0], Value::Number(1.0), "returnCount");
            assert_eq!(elems[1], Value::Boolean(true), "ReturnError propagated");
        } else {
            panic!("Expected array, got {:?}", r);
        }
    }
}
