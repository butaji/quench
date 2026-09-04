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
}
