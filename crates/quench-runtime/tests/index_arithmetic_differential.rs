//! Index arithmetic in 64-bit words: element indices made of int32 products and sums, run in the
//! interpreter and under the JIT, must read the same elements and leave at the same places. The
//! indices cross the lengths of their arrays (the last element, the first past it), int32's and
//! uint32's spans and the span a Number holds exactly, and a product that is a negative zero.

use quench_runtime::{Engine, ExecutionMode, MemoryPolicy, Runtime, SystemHost};

/// How a run ended: its result, or what it threw.
fn run(source: &str, mode: ExecutionMode) -> String {
    let outcome = Engine::specialize(source, "index-arithmetic.js")
        .map_err(|errors| format!("{errors:?}"))
        .and_then(|program| {
            let mut runtime = Runtime::new(SystemHost);
            runtime.set_memory_policy(MemoryPolicy::Throughput);
            runtime.set_execution_mode(mode);
            runtime.execute(&program).map_err(|error| error.to_string())
        });
    format!("{outcome:?}")
}

fn agree(source: &str) {
    assert_eq!(run(source, ExecutionMode::Interpreted), run(source, ExecutionMode::Compiled), "{source}");
}

/// The sum of `a[i * stride + j]` over `rows` rows from `first` by `columns` columns, `rounds` times,
/// as a thrown string (an element past the array is `undefined`, whose sum is NaN).
fn grid(kind: &str, length: u64, [first, rows, columns, stride]: [u64; 4], rounds: u64) -> String {
    format!("function grid(a, first, rows, columns, stride, rounds) {{ var s = 0;
        for (var r = 0; r < rounds; r++) for (var i = first; i < first + rows; i++)
            for (var j = 0; j < columns; j++) s += a[i * stride + j];
        return s; }}
        var a = new {kind}({length}); for (var q = 0; q < {length}; q++) a[q] = q % 7 + 0.5;
        throw 'sum ' + grid(a, {first}, {rows}, {columns}, {stride}, {rounds});")
}

#[test]
fn indices_at_and_past_the_length_leave_exactly() {
    for kind in ["Float64Array", "Int32Array", "Uint8Array"] {
        for length in [4095, 4096, 4097] {
            agree(&grid(kind, length, [0, 64, 64, 64], 40));
        }
    }
}

#[test]
fn a_dense_array_is_indexed_as_a_typed_one_is() {
    let source = "function grid(a, n) { var s = 0; for (var r = 0; r < 60; r++) for (var i = 0; i < n; i++)
        for (var j = 0; j < n; j++) s += a[i * n + j]; return s; }
        var a = []; for (var q = 0; q < 4096; q++) a.push(q % 5);
        throw 'dense ' + grid(a, 64) + ' ' + grid(a, 65);";
    agree(source);
}

#[test]
fn indices_across_the_int32_uint32_and_exact_spans_leave_exactly() {
    // Strides whose products reach 2^31, 2^32 and 2^53 and pass them: the element is absent in
    // each, and the sum NaN, as long as the product is read as the Number it is.
    for stride in [65_536_u64, 2_147_483_647, 1 << 30] {
        for first in [0, 2, 32_768, 4_194_303, 1_073_741_820] {
            agree(&grid("Float64Array", 4096, [first, 3, 4, stride], 20_000));
        }
    }
}

#[test]
fn a_product_that_is_a_negative_zero_addresses_the_first_element() {
    let source = "function zero(a, n) { var s = 0; for (var r = 0; r < 200; r++) for (var i = 0; i < n; i++)
        for (var j = 0; j < n; j++) s += a[(i - 3) * j + j * 0]; return s; }
        var a = new Float64Array(64); for (var q = 0; q < 64; q++) a[q] = q + 1;
        throw 'zero ' + zero(a, 100) + ' ' + zero(a, 6);";
    agree(source);
}

// Links the runtime layer crates (quench-layers).
extern crate quench_layers;
