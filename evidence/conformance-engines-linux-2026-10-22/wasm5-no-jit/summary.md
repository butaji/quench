# wasm no-jit — Quench vs Node vs Bun

host: 4 CPUs, Intel(R) Xeon(R) Processor @ 2.10GHz; jobs 4; rounds 5; elapsed 30s; excluded 0 cases
versions: quench 304a28fcce38d70ccbc653c28e54344f67ab38d0 · node v26.12.1 · bun 1.4.2

| engine | cases | passed | timeouts | CPU s | wall s | RSS median MiB | RSS p95 MiB | RSS max MiB |
|---|---|---|---|---|---|---|---|---|
| quench | 288 | 288 | 0 | 6.4 | 6.8 | 10.2 | 14.9 | 146.7 |
| node | 288 | 249 | 0 | 10.1 | 11.2 | 43.6 | 50.7 | 62.4 |
| bun | 288 | 220 | 0 | 4.0 | 4.6 | 21.2 | 24.3 | 30.9 |

Quench / other (lower is better; a win needs <= 0.80):

| metric | vs node | vs bun |
|---|---|---|
| cpu_s | 0.64 | 1.61 |
| wall_s | 0.61 | 1.50 |
| rss_median_mib | 0.23 | 0.48 |
| rss_max_mib | 2.35 | 4.75 |

Areas where Quench is not 20% better than every engine on every metric: 53 of 288

| area | cases | short of the margin |
|---|---|---|
| address64.wast | 1 | cpu_s vs bun 0.83x; wall_s vs bun 0.86x |
| align64.wast | 1 | cpu_s vs bun 0.87x; wall_s vs bun 0.88x |
| block.wast | 1 | wall_s vs bun 0.94x |
| br_table.wast | 1 | cpu_s vs node 2.44x; cpu_s vs bun 6.83x; wall_s vs node 2.43x; wall_s vs bun 6.55x |
| call_indirect.wast | 1 | cpu_s vs bun 1.25x; wall_s vs bun 1.29x |
| call_ref.wast | 1 | cpu_s vs bun 0.88x |
| const.wast | 1 | cpu_s vs bun 1.18x; wall_s vs bun 1.18x |
| conversions.wast | 1 | cpu_s vs bun 1.07x; wall_s vs bun 0.89x |
| f32.wast | 1 | cpu_s vs node 1.56x; cpu_s vs bun 3.91x; wall_s vs node 1.61x; wall_s vs bun 3.94x |
| f32_cmp.wast | 1 | cpu_s vs node 1.27x; cpu_s vs bun 3.50x; wall_s vs node 1.17x; wall_s vs bun 3.47x |
| f64.wast | 1 | cpu_s vs node 1.52x; cpu_s vs bun 3.77x; wall_s vs node 1.54x; wall_s vs bun 4.01x |
| f64_cmp.wast | 1 | cpu_s vs node 1.35x; cpu_s vs bun 3.34x; wall_s vs node 1.26x; wall_s vs bun 3.03x |
| float_exprs.wast | 1 | cpu_s vs bun 1.60x; wall_s vs bun 1.37x |
| float_misc.wast | 1 | cpu_s vs bun 0.90x; wall_s vs bun 0.98x |
| func.wast | 1 | cpu_s vs bun 0.81x |
| i32.wast | 1 | cpu_s vs bun 0.84x |
| i64.wast | 1 | cpu_s vs bun 0.84x |
| if.wast | 1 | cpu_s vs bun 1.01x; wall_s vs bun 1.06x |
| left-to-right.wast | 1 | cpu_s vs bun 2.00x; wall_s vs bun 1.74x |
| memory_copy.wast | 1 | cpu_s vs node 2.96x; cpu_s vs bun 3.03x; wall_s vs node 3.02x; wall_s vs bun 2.68x; rss_median_mib vs bun 0.80x; rss_max_mib vs bun 0.80x |
| memory_copy64.wast | 1 | cpu_s vs node 3.69x; cpu_s vs bun 6.09x; wall_s vs node 3.82x; wall_s vs bun 4.79x; rss_median_mib vs bun 0.83x; rss_max_mib vs bun 0.83x |
| memory_fill64.wast | 1 | cpu_s vs node 1.20x; cpu_s vs bun 4.21x; wall_s vs node 1.03x; wall_s vs bun 4.40x |
| memory_grow64.wast | 1 | cpu_s vs node 1.84x; cpu_s vs bun 7.76x; wall_s vs node 1.72x; wall_s vs bun 8.35x; rss_median_mib vs node 1.29x; rss_median_mib vs bun 3.12x; rss_max_mib vs node 1.29x; rss_max_mib vs bun 3.12x |
| memory_init64.wast | 1 | cpu_s vs bun 0.97x; wall_s vs bun 1.10x |
| names.wast | 1 | cpu_s vs node 1.14x; cpu_s vs bun 3.21x; wall_s vs node 1.07x; wall_s vs bun 3.17x |
| proposals/custom-descriptors/br_on_cast_desc_eq.wast | 1 | cpu_s vs bun 1.30x; wall_s vs bun 1.23x |
| proposals/custom-descriptors/br_on_cast_desc_eq_fail.wast | 1 | cpu_s vs bun 1.47x; wall_s vs bun 1.55x |
| proposals/custom-descriptors/ref_cast_desc_eq.wast | 1 | cpu_s vs bun 0.91x; wall_s vs bun 0.84x |
| proposals/threads/atomic.wast | 1 | cpu_s vs bun 0.88x; wall_s vs bun 0.89x |
| return_call.wast | 1 | cpu_s vs node 16.46x; cpu_s vs bun 2.87x; wall_s vs node 15.82x; wall_s vs bun 2.89x |
| return_call_indirect.wast | 1 | cpu_s vs node 2.26x; cpu_s vs bun 2.65x; wall_s vs node 2.32x; wall_s vs bun 2.58x |
| return_call_ref.wast | 1 | cpu_s vs node 11.57x; cpu_s vs bun 3.18x; wall_s vs node 10.73x; wall_s vs bun 3.29x |
| simd_boolean.wast | 1 | cpu_s vs bun 1.04x; wall_s vs bun 0.98x |
| simd_const.wast | 1 | cpu_s vs bun 1.02x; wall_s vs bun 0.87x |
| simd_conversions.wast | 1 | cpu_s vs bun 0.84x; wall_s vs bun 0.88x |
| simd_f32x4.wast | 1 | cpu_s vs bun 1.78x; wall_s vs bun 1.75x |
| simd_f32x4_arith.wast | 1 | cpu_s vs node 2.00x; cpu_s vs bun 5.87x; wall_s vs node 2.21x; wall_s vs bun 5.80x |
| simd_f32x4_cmp.wast | 1 | cpu_s vs node 3.21x; cpu_s vs bun 10.14x; wall_s vs node 3.20x; wall_s vs bun 10.51x |
| simd_f32x4_pmin_pmax.wast | 1 | cpu_s vs node 8.75x; cpu_s vs bun 24.63x; wall_s vs node 8.92x; wall_s vs bun 22.86x |
| simd_f64x2.wast | 1 | cpu_s vs bun 2.21x; wall_s vs bun 2.12x |
| simd_f64x2_arith.wast | 1 | cpu_s vs node 2.23x; cpu_s vs bun 6.89x; wall_s vs node 2.05x; wall_s vs bun 6.63x |
| simd_f64x2_cmp.wast | 1 | cpu_s vs node 3.83x; cpu_s vs bun 12.71x; wall_s vs node 3.61x; wall_s vs bun 12.78x |
| simd_f64x2_pmin_pmax.wast | 1 | cpu_s vs node 7.45x; cpu_s vs bun 23.81x; wall_s vs node 7.84x; wall_s vs bun 24.40x |
| simd_i16x8_cmp.wast | 1 | cpu_s vs bun 1.08x; wall_s vs bun 1.15x |
| simd_i32x4_cmp.wast | 1 | cpu_s vs bun 1.21x; wall_s vs bun 1.14x |
| simd_i8x16_arith2.wast | 1 | cpu_s vs bun 0.81x |
| simd_i8x16_cmp.wast | 1 | cpu_s vs bun 1.20x; wall_s vs bun 1.24x |
| simd_lane.wast | 1 | cpu_s vs bun 1.06x; wall_s vs bun 1.11x |
| skip-stack-guard-page.wast | 1 | cpu_s vs node 10.35x; cpu_s vs bun 19.28x; wall_s vs node 10.41x; wall_s vs bun 17.27x; rss_median_mib vs node 3.30x; rss_median_mib vs bun 5.74x; rss_max_mib vs node 3.30x; rss_max_mib vs bun 5.74x |
| table_copy.wast | 1 | cpu_s vs node 1.00x; cpu_s vs bun 3.15x; wall_s vs node 0.90x; wall_s vs bun 3.22x |
| table_copy64.wast | 1 | cpu_s vs node 0.94x; cpu_s vs bun 2.89x; wall_s vs node 0.93x; wall_s vs bun 3.13x |
| table_init.wast | 1 | cpu_s vs bun 1.92x; wall_s vs bun 1.88x |
| table_init64.wast | 1 | cpu_s vs bun 1.52x; wall_s vs bun 1.19x |
