//! Executable complexity claims for the VM's foundational data structures.
//!
//! These tests assert scaling ratios rather than absolute timings. They are
//! intentionally separate from benchmark fixtures: a regression must violate
//! a general complexity claim before it can be reported by the test suite.

#[cfg(test)]
mod tests {
    use std::hint::black_box;
    use std::time::Instant;

    /// Property lookup is O(1) in the number of historical shapes: once an
    /// object's shape is known, its property slot is derived from that shape,
    /// not from a scan of every shape ever interned. We use a wall-clock ratio
    /// because the public execution-trace counters do not expose this lookup
    /// as a per-operation counter. The bound is intentionally below the
    /// 100x growth a K-shaped linear scan would show (K=10 versus K=1000).
    #[test]
    fn property_access_does_not_scale_with_historical_shape_count() {
        let small = measure_property_access(10, 20_000);
        let large = measure_property_access(1_000, 200);
        let ratio = large / small.max(1e-9);
        eprintln!(
            "architecture invariant #1: K=10 {small:.6} ms/access, K=1000 {large:.6} ms/access, ratio {ratio:.3}"
        );
        assert!(
            ratio < 8.0,
            "property access scaled with historical shapes: K=10 {small:.3} ms/access, K=1000 {large:.3} ms/access, ratio {ratio:.2}"
        );
    }

    fn measure_property_access(shape_count: usize, repetitions: usize) -> f64 {
        let source = format!(
            r#"
                var objects = [];
                for (var i = 0; i < {shape_count}; i++) {{
                    var object = {{}};
                    object["unique" + i] = i;
                    object.target = 1;
                    objects[i] = object;
                }}
                var total = 0;
                for (var round = 0; round < {repetitions}; round++) {{
                    for (var i = 0; i < {shape_count}; i++) total += objects[i].target;
                }}
                total;
            "#
        );
        let program = crate::reduce::reduce_source(&source).expect("shape probe reduces");
        let started = Instant::now();
        let result =
            crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
                .expect("shape probe executes");
        black_box(result);
        started.elapsed().as_secs_f64() * 1_000.0 / (shape_count * repetitions) as f64
    }

    /// Shape transitions are O(1) in the number of properties already on an
    /// object. The measurement is a wall-clock ratio because the trace API
    /// does not expose transition work separately from assignment. Setup is
    /// amortized over many independent transitions; a 16x bound rejects a
    /// linear transition walk while tolerating debug-build noise.
    #[test]
    fn shape_transition_does_not_scale_with_property_count() {
        let small = measure_shape_transition(4, 2_000);
        let large = measure_shape_transition(64, 500);
        let ratio = large / small.max(1e-9);
        eprintln!(
            "architecture invariant #2: properties=4 {small:.6} ms/transition, properties=64 {large:.6} ms/transition, ratio {ratio:.3}"
        );
        assert!(
            ratio < 16.0,
            "shape transition scaled with property count: ratio {ratio:.2}"
        );
    }

    fn measure_shape_transition(property_count: usize, repetitions: usize) -> f64 {
        let source = format!(
            r#"
                var objects = [];
                for (var n = 0; n < {repetitions}; n++) {{
                    var object = {{}};
                    for (var i = 0; i < {property_count}; i++) object["p" + i] = i;
                    objects[n] = object;
                }}
                var total = 0;
                for (var round = 0; round < 20; round++) {{
                    for (var n = 0; n < {repetitions}; n++) {{
                        objects[n].last = n + round;
                        total += objects[n].last;
                        delete objects[n].last;
                    }}
                }}
                total;
            "#
        );
        run_source_ms(&source) / (repetitions * 20) as f64
    }

    /// Call dispatch is O(1) in the number of distinct callees previously
    /// observed. We use a wall-clock ratio because dispatch is not exposed as
    /// a standalone deterministic counter. Both runs perform the same number
    /// of calls; a 16x bound catches a linear callee-history scan.
    #[test]
    fn call_dispatch_does_not_scale_with_historical_callee_count() {
        let small = measure_call_dispatch(10, 20_000);
        let large = measure_call_dispatch(1_000, 200);
        let ratio = large / small.max(1e-9);
        eprintln!(
            "architecture invariant #3: callees=10 {small:.6} ms/call, callees=1000 {large:.6} ms/call, ratio {ratio:.3}"
        );
        assert!(
            ratio < 16.0,
            "call dispatch scaled with callee history: ratio {ratio:.2}"
        );
    }

    fn measure_call_dispatch(callee_count: usize, repetitions: usize) -> f64 {
        let source = format!(
            r#"
                var fns = [];
                for (var i = 0; i < {callee_count}; i++) {{
                    fns[i] = (function(value) {{ return value + 1; }});
                }}
                var total = 0;
                for (var round = 0; round < {repetitions}; round++) {{
                    for (var i = 0; i < {callee_count}; i++) total += fns[i](i);
                }}
                total;
            "#
        );
        run_source_ms(&source) / (callee_count * repetitions) as f64
    }

    /// Enumeration is O(current-key-count), independent of prior mutation
    /// history. The timed phase repeatedly enumerates the same final object;
    /// only the pre-timed mutation history differs. A 16x ratio catches a
    /// history-proportional scan while avoiding an absolute timing threshold.
    #[test]
    fn enumeration_does_not_scale_with_mutation_history() {
        let small = measure_enumeration_history(0, 2_000);
        let large = measure_enumeration_history(2_000, 2_000);
        let ratio = large / small.max(1e-9);
        eprintln!(
            "architecture invariant #4: history=0 {small:.6} ms/enum, history=2000 {large:.6} ms/enum, ratio {ratio:.3}"
        );
        assert!(
            ratio < 16.0,
            "enumeration scaled with mutation history: ratio {ratio:.2}"
        );
    }

    fn measure_enumeration_history(history: usize, repetitions: usize) -> f64 {
        let source = format!(
            r#"
                var object = {{a: 1, b: 2, c: 3, d: 4}};
                for (var i = 0; i < {history}; i++) {{
                    object["transient" + i] = i;
                    delete object["transient" + i];
                }}
                var total = 0;
                for (var round = 0; round < {repetitions}; round++) {{
                    for (var key in object) total += object[key];
                }}
                total;
            "#
        );
        run_source_ms(&source) / repetitions as f64
    }

    /// Dispatch cost per bytecode is O(1) in program length. The programs use
    /// only a catalog-stable arithmetic operation and perform no allocations;
    /// a wall-clock per-operation ratio avoids relying on noisy absolute
    /// throughput. A 16x bound rejects a dispatch structure that rescans the
    /// whole stream/catalog for each instruction.
    #[test]
    fn dispatch_cost_does_not_scale_with_program_length() {
        let small = measure_dispatch_cost(200);
        let large = measure_dispatch_cost(20_000);
        let ratio = large / small.max(1e-9);
        eprintln!(
            "architecture invariant #5: length=200 {small:.6} ms/op, length=20000 {large:.6} ms/op, ratio {ratio:.3}"
        );
        assert!(
            ratio < 16.0,
            "dispatch scaled with program length: ratio {ratio:.2}"
        );
    }

    fn measure_dispatch_cost(length: usize) -> f64 {
        let mut source = String::with_capacity(length * 8 + 32);
        source.push_str("var x = 0;\n");
        for _ in 0..length {
            source.push_str("x = x + 1;\n");
        }
        source.push_str("x;");
        run_source_ms(&source) / length as f64
    }

    fn run_source_ms(source: &str) -> f64 {
        let program = crate::reduce::reduce_source(source).expect("invariant source reduces");
        let started = Instant::now();
        let result =
            crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
                .expect("invariant source executes");
        black_box(result);
        started.elapsed().as_secs_f64() * 1_000.0
    }

    /// Packed-array index zero access is O(1) in array length. Both programs
    /// perform the same number of reads; setup is amortized and the ratio
    /// bound rejects an implementation that scans the backing array.
    #[test]
    fn packed_array_access_does_not_scale_with_length() {
        let small = measure_array_access(10, 100_000);
        let large = measure_array_access(100_000, 100_000);
        let ratio = large / small.max(1e-9);
        eprintln!(
            "architecture invariant #063.1: length=10 {small:.6} ms/access, length=100000 {large:.6} ms/access, ratio {ratio:.3}"
        );
        assert!(
            ratio < 16.0,
            "array access scaled with length: ratio {ratio:.2}"
        );
    }

    fn measure_array_access(length: usize, repetitions: usize) -> f64 {
        let source = format!(
            r#"
                var array = [];
                for (var i = 0; i < {length}; i++) array[i] = i;
                var total = 0;
                for (var i = 0; i < {repetitions}; i++) total += array[0];
                total;
            "#
        );
        run_source_ms(&source) / repetitions as f64
    }

    /// Map/Set lookup and update are O(1) in the number of unrelated
    /// instances ever created. This cross-instance form mirrors invariant #1
    /// for shapes and uses a wall-clock ratio because the trace API has no
    /// collection-operation counter.
    #[test]
    fn collection_access_does_not_scale_with_instance_history() {
        let small = measure_collection_access(10, 20_000);
        let large = measure_collection_access(1_000, 200);
        let ratio = large / small.max(1e-9);
        eprintln!(
            "architecture invariant #063.2: instances=10 {small:.6} ms/access, instances=1000 {large:.6} ms/access, ratio {ratio:.3}"
        );
        assert!(
            ratio < 16.0,
            "collection access scaled with instance history: ratio {ratio:.2}"
        );
    }

    fn measure_collection_access(instance_count: usize, repetitions: usize) -> f64 {
        let source = format!(
            r#"
                var maps = [], sets = [];
                for (var i = 0; i < {instance_count}; i++) {{
                    var map = new Map(); map.set("key", i); maps[i] = map;
                    var set = new Set(); set.add(i); sets[i] = set;
                }}
                var total = 0;
                for (var round = 0; round < {repetitions}; round++) {{
                    for (var i = 0; i < {instance_count}; i++) {{
                        total += maps[i].get("key");
                        sets[i].has(i);
                        maps[i].set("key", i);
                    }}
                }}
                total;
            "#
        );
        run_source_ms(&source) / (instance_count * repetitions) as f64
    }

    /// Collection iteration is proportional to live entries, not historical
    /// add/delete churn. The final Map, Set, and Array each have four live
    /// entries while only the pre-loop mutation history changes.
    #[test]
    fn collection_iteration_does_not_scale_with_mutation_history() {
        let small = measure_collection_iteration(0, 1_000);
        let large = measure_collection_iteration(2_000, 1_000);
        let ratio = large / small.max(1e-9);
        eprintln!(
            "architecture invariant #063.3: history=0 {small:.6} ms/iteration, history=2000 {large:.6} ms/iteration, ratio {ratio:.3}"
        );
        assert!(
            ratio < 16.0,
            "collection iteration scaled with mutation history: ratio {ratio:.2}"
        );
    }

    fn measure_collection_iteration(history: usize, repetitions: usize) -> f64 {
        let source = format!(
            r#"
                var map = new Map(), set = new Set(), array = [];
                map.set("a", 1); map.set("b", 2); map.set("c", 3); map.set("d", 4);
                set.add(1); set.add(2); set.add(3); set.add(4);
                array[0] = 1; array[1] = 2; array[2] = 3; array[3] = 4;
                for (var i = 0; i < {history}; i++) {{
                    var key = "transient" + i;
                    map.set(key, i); map.delete(key);
                    set.add(i + 1000000); set.delete(i + 1000000);
                    array.push(i); array.pop();
                }}
                var total = 0;
                for (var round = 0; round < {repetitions}; round++) {{
                    map.forEach(function(value) {{ total += value; }});
                    set.forEach(function(value) {{ total += value; }});
                    for (var i = 0; i < array.length; i++) total += array[i];
                }}
                total;
            "#
        );
        run_source_ms(&source) / repetitions as f64
    }

    /// Repeated append is near-linear in the total bytes produced, not
    /// quadratic from repeatedly rescanning the accumulated string. The
    /// expected work grows 10x between these sizes, so a 30x bound catches a
    /// naive O(n²) implementation while tolerating runtime noise.
    #[test]
    fn string_append_is_not_quadratic() {
        let small = measure_string_append(500);
        let large = measure_string_append(5_000);
        let ratio = large / small.max(1e-9);
        eprintln!(
            "architecture invariant #064.1: append length=500 {small:.3} ms, length=5000 {large:.3} ms, ratio {ratio:.3}"
        );
        assert!(
            ratio < 30.0,
            "string append grew superlinearly: ratio {ratio:.2}"
        );
    }

    fn measure_string_append(length: usize) -> f64 {
        let source = format!(
            r#"
                var value = "";
                for (var i = 0; i < {length}; i++) value += "x";
                value.length;
            "#
        );
        run_source_ms(&source)
    }

    /// split/indexOf/includes are proportional to the searched input, not to
    /// unrelated strings created elsewhere in the runtime. Equal operation
    /// counts and a 16x unrelated-string range make this a cross-state ratio.
    #[test]
    fn string_search_does_not_scale_with_unrelated_string_count() {
        let small = measure_string_search(10, 20_000);
        let large = measure_string_search(1_000, 2_000);
        let ratio = large / small.max(1e-9);
        eprintln!(
            "architecture invariant #064.2: unrelated=10 {small:.6} ms/search, unrelated=1000 {large:.6} ms/search, ratio {ratio:.3}"
        );
        assert!(
            ratio < 16.0,
            "string search scaled with unrelated strings: ratio {ratio:.2}"
        );
    }

    fn measure_string_search(unrelated: usize, repetitions: usize) -> f64 {
        let source = format!(
            r#"
                var noise = [];
                for (var i = 0; i < {unrelated}; i++) noise[i] = "noise" + i;
                var value = "alpha beta gamma delta";
                var total = 0;
                for (var i = 0; i < {repetitions}; i++) {{
                    total += value.split(" ").length;
                    total += value.indexOf("gamma");
                    if (value.includes("delta")) total++;
                }}
                total + noise.length;
            "#
        );
        run_source_ms(&source) / repetitions as f64
    }

    /// Equal-length string comparison is proportional to the compared length,
    /// not to an unrelated state dimension. The 100x input-length range gets
    /// a 300x bound to distinguish linear comparison from a hidden quadratic
    /// scan without asserting an absolute time.
    #[test]
    fn string_comparison_scales_with_compared_length_only() {
        let small = measure_string_comparison(100, 10_000);
        let large = measure_string_comparison(10_000, 100);
        let ratio = large / small.max(1e-9);
        eprintln!(
            "architecture invariant #064.3: length=100 {small:.6} ms/compare, length=10000 {large:.6} ms/compare, ratio {ratio:.3}"
        );
        assert!(
            ratio < 300.0,
            "string comparison grew beyond linear: ratio {ratio:.2}"
        );
    }

    fn measure_string_comparison(length: usize, repetitions: usize) -> f64 {
        let source = format!(
            r#"
                var left = "", right = "";
                for (var i = 0; i < {length}; i++) {{ left += "a"; right += "a"; }}
                var total = 0;
                for (var i = 0; i < {repetitions}; i++) {{
                    if (left === right) total++;
                    if (!(left < right)) total++;
                }}
                total;
            "#
        );
        run_source_ms(&source) / repetitions as f64
    }

    /// Closure creation is proportional to captured state, not to the total
    /// number of closures previously created. Equal per-closure use keeps
    /// setup amortized while the 16x bound catches a global closure-list scan.
    #[test]
    fn closure_creation_does_not_scale_with_closure_history() {
        let small = measure_closure_creation(10, 20_000);
        let large = measure_closure_creation(1_000, 200);
        let ratio = large / small.max(1e-9);
        eprintln!(
            "architecture invariant #065.1: closures=10 {small:.6} ms/closure, closures=1000 {large:.6} ms/closure, ratio {ratio:.3}"
        );
        assert!(
            ratio < 16.0,
            "closure creation scaled with closure history: ratio {ratio:.2}"
        );
    }

    fn measure_closure_creation(closure_count: usize, repetitions: usize) -> f64 {
        let source = format!(
            r#"
                var closures = [];
                for (var i = 0; i < {closure_count}; i++) {{
                    (function(value) {{ closures[i] = function() {{ return value; }}; }})(i);
                }}
                var total = 0;
                for (var round = 0; round < {repetitions}; round++) {{
                    for (var i = 0; i < {closure_count}; i++) total += closures[i]();
                }}
                total;
            "#
        );
        run_source_ms(&source) / (closure_count * repetitions) as f64
    }

    /// Recursive frame setup/teardown is O(1) per frame. The ratio compares
    /// marginal frame cost at depth 10 and 100, rather than total stack space.
    #[test]
    fn recursive_frame_cost_is_constant_per_frame() {
        let shallow = measure_recursive_frames(10, 2_000);
        let deep = measure_recursive_frames(100, 200);
        let ratio = deep / shallow.max(1e-9);
        eprintln!(
            "architecture invariant #065.2: depth=10 {shallow:.6} ms/frame, depth=100 {deep:.6} ms/frame, ratio {ratio:.3}"
        );
        assert!(
            ratio < 16.0,
            "recursive frame cost grew with depth: ratio {ratio:.2}"
        );
    }

    fn measure_recursive_frames(depth: usize, repetitions: usize) -> f64 {
        let source = format!(
            r#"
                function descend(n) {{ if (n === 0) return 1; return 1 + descend(n - 1); }}
                var total = 0;
                for (var i = 0; i < {repetitions}; i++) total += descend({depth});
                total;
            "#
        );
        run_source_ms(&source) / (depth * repetitions) as f64
    }

    /// Argument marshaling is proportional to argument count, not to the
    /// number of unrelated functions defined in the program. Equal call
    /// counts and a 16x bound catch a global function-table scan.
    #[test]
    fn argument_marshaling_does_not_scale_with_function_catalog() {
        let small = measure_argument_marshaling(10, 20_000);
        let large = measure_argument_marshaling(1_000, 200);
        let ratio = large / small.max(1e-9);
        eprintln!(
            "architecture invariant #065.3: functions=10 {small:.6} ms/call, functions=1000 {large:.6} ms/call, ratio {ratio:.3}"
        );
        assert!(
            ratio < 16.0,
            "argument marshaling scaled with function catalog: ratio {ratio:.2}"
        );
    }

    fn measure_argument_marshaling(function_count: usize, repetitions: usize) -> f64 {
        let source = format!(
            r#"
                var functions = [];
                for (var i = 0; i < {function_count}; i++) functions[i] = function(a, b, c) {{ return a + b + c; }};
                var total = 0;
                for (var round = 0; round < {repetitions}; round++) {{
                    for (var i = 0; i < {function_count}; i++) total += functions[i](i, i + 1, i + 2);
                }}
                total;
            "#
        );
        run_source_ms(&source) / (function_count * repetitions) as f64
    }

    /// Allocation of an acyclic object is amortized O(1) in live-heap size.
    /// Equal allocation counts and a 16x ratio reject an accidental full-heap
    /// scan while tolerating allocator/cache noise.
    #[test]
    fn object_allocation_does_not_scale_with_live_heap_size() {
        let small = measure_object_allocation(10, 200_000);
        let large = measure_object_allocation(10_000, 200_000);
        let ratio = large / small.max(1e-9);
        eprintln!(
            "architecture invariant #066.1: live=10 {small:.6} ms/allocation, live=10000 {large:.6} ms/allocation, ratio {ratio:.3}"
        );
        assert!(
            ratio < 16.0,
            "allocation scaled with live heap size: ratio {ratio:.2}"
        );
    }

    fn measure_object_allocation(live_count: usize, repetitions: usize) -> f64 {
        let source = format!(
            r#"
                var live = [];
                for (var i = 0; i < {live_count}; i++) live[i] = {{ value: i }};
                var total = 0;
                for (var i = 0; i < {repetitions}; i++) {{
                    var object = {{ value: i }};
                    total += object.value;
                }}
                total + live.length;
            "#
        );
        run_source_ms(&source) / repetitions as f64
    }

    /// Dropping an acyclic object is O(1) in the number of unrelated live
    /// objects. Each temporary becomes unreachable immediately; the ratio
    /// compares equal drop counts with different retained-heap sizes.
    #[test]
    fn object_drop_does_not_scale_with_live_heap_size() {
        let small = measure_object_drop(10, 200_000);
        let large = measure_object_drop(10_000, 200_000);
        let ratio = large / small.max(1e-9);
        eprintln!(
            "architecture invariant #066.2: live=10 {small:.6} ms/drop, live=10000 {large:.6} ms/drop, ratio {ratio:.3}"
        );
        assert!(
            ratio < 16.0,
            "drop scaled with live heap size: ratio {ratio:.2}"
        );
    }

    fn measure_object_drop(live_count: usize, repetitions: usize) -> f64 {
        let source = format!(
            r#"
                var live = [];
                for (var i = 0; i < {live_count}; i++) live[i] = {{ value: i }};
                var total = 0;
                for (var i = 0; i < {repetitions}; i++) {{
                    var temporary = {{ value: i }};
                    total += temporary.value;
                    temporary = undefined;
                }}
                total + live.length;
            "#
        );
        run_source_ms(&source) / repetitions as f64
    }
}
