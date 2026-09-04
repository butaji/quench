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
                for (var n = 0; n < {repetitions}; n++) {{
                    objects[n].last = n;
                    total += objects[n].last;
                }}
                total;
            "#
        );
        run_source_ms(&source) / repetitions as f64
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
}
