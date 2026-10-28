# wasm jit — Quench vs Node vs Bun

host: 4 CPUs, Intel(R) Xeon(R) Processor @ 2.10GHz; jobs 4; rounds 5; elapsed 31s; excluded 0 cases
versions: quench 304a28fcce38d70ccbc653c28e54344f67ab38d0 · node v26.12.1 · bun 1.4.2

| engine | cases | passed | timeouts | CPU s | wall s | RSS median MiB | RSS p95 MiB | RSS max MiB |
|---|---|---|---|---|---|---|---|---|
| quench | 288 | 288 | 0 | 7.3 | 7.9 | 12.6 | 16.3 | 70.1 |
| node | 288 | 249 | 0 | 10.0 | 11.3 | 43.6 | 50.8 | 62.3 |
| bun | 288 | 220 | 0 | 4.2 | 4.9 | 22.4 | 31.9 | 38.6 |

Quench / other (lower is better; a win needs <= 0.80):

| metric | vs node | vs bun |
|---|---|---|
| cpu_s | 0.72 | 1.72 |
| wall_s | 0.70 | 1.60 |
| rss_median_mib | 0.29 | 0.56 |
| rss_max_mib | 1.13 | 1.82 |

Areas where Quench is not 20% better than every engine on every metric: 67 of 288

| area | cases | short of the margin |
|---|---|---|
| address.wast | 1 | cpu_s vs bun 1.02x; wall_s vs bun 1.06x |
| address64.wast | 1 | wall_s vs bun 0.83x |
| block.wast | 1 | cpu_s vs bun 1.04x; wall_s vs bun 1.17x |
| br.wast | 1 | cpu_s vs bun 1.06x; wall_s vs bun 0.84x |
| br_if.wast | 1 | cpu_s vs bun 1.23x; wall_s vs bun 1.43x |
| br_table.wast | 1 | cpu_s vs node 10.74x; cpu_s vs bun 23.13x; wall_s vs node 10.09x; wall_s vs bun 25.62x; rss_median_mib vs bun 1.07x; rss_max_mib vs bun 1.07x |
| call.wast | 1 | cpu_s vs bun 0.90x |
| call_indirect.wast | 1 | cpu_s vs node 0.85x; cpu_s vs bun 1.10x; wall_s vs bun 1.39x |
| const.wast | 1 | cpu_s vs node 1.68x; cpu_s vs bun 1.73x; wall_s vs node 1.59x; wall_s vs bun 1.48x |
| conversions.wast | 1 | cpu_s vs bun 0.81x; wall_s vs bun 0.98x |
| elem.wast | 1 | cpu_s vs bun 1.09x; wall_s vs bun 1.24x |
| endianness.wast | 1 | cpu_s vs bun 0.92x; wall_s vs bun 0.91x |
| f32.wast | 1 | cpu_s vs node 1.54x; cpu_s vs bun 3.15x; wall_s vs node 1.65x; wall_s vs bun 3.81x |
| f32_cmp.wast | 1 | cpu_s vs node 1.32x; cpu_s vs bun 2.69x; wall_s vs node 1.35x; wall_s vs bun 3.00x |
| f64.wast | 1 | cpu_s vs node 1.57x; cpu_s vs bun 2.92x; wall_s vs node 1.44x; wall_s vs bun 2.74x |
| f64_cmp.wast | 1 | cpu_s vs node 1.48x; cpu_s vs bun 2.75x; wall_s vs node 1.54x; wall_s vs bun 2.95x |
| float_exprs.wast | 1 | cpu_s vs node 1.94x; cpu_s vs bun 3.07x; wall_s vs node 2.01x; wall_s vs bun 3.65x |
| float_literals.wast | 1 | cpu_s vs bun 1.26x; wall_s vs bun 1.24x |
| float_misc.wast | 1 | cpu_s vs bun 1.00x; wall_s vs bun 1.02x |
| func.wast | 1 | cpu_s vs bun 1.41x; wall_s vs bun 1.29x |
| global.wast | 1 | cpu_s vs bun 0.91x; wall_s vs bun 0.81x |
| i32.wast | 1 | cpu_s vs bun 0.84x; wall_s vs bun 0.94x |
| i64.wast | 1 | wall_s vs bun 0.81x |
| if.wast | 1 | cpu_s vs bun 1.19x; wall_s vs bun 1.19x |
| imports.wast | 1 | wall_s vs bun 0.80x |
| int_exprs.wast | 1 | cpu_s vs bun 1.27x; wall_s vs bun 1.16x |
| int_literals.wast | 1 | wall_s vs bun 0.98x |
| left-to-right.wast | 1 | cpu_s vs node 1.41x; cpu_s vs bun 3.59x; wall_s vs node 1.21x; wall_s vs bun 3.36x |
| load.wast | 1 | cpu_s vs bun 0.94x; wall_s vs bun 0.96x |
| load2.wast | 1 | wall_s vs bun 0.81x |
| local_tee.wast | 1 | cpu_s vs bun 1.38x; wall_s vs bun 1.25x |
| loop.wast | 1 | cpu_s vs bun 1.20x; wall_s vs bun 1.27x |
| memory_copy.wast | 1 | cpu_s vs node 2.94x; cpu_s vs bun 2.53x; wall_s vs node 3.21x; wall_s vs bun 2.48x |
| memory_copy64.wast | 1 | cpu_s vs node 3.62x; cpu_s vs bun 5.08x; wall_s vs node 3.54x; wall_s vs bun 3.76x |
| memory_fill64.wast | 1 | cpu_s vs node 1.30x; cpu_s vs bun 3.62x; wall_s vs node 1.21x; wall_s vs bun 3.25x |
| memory_grow64.wast | 1 | cpu_s vs node 2.24x; cpu_s vs bun 6.69x; wall_s vs node 2.22x; wall_s vs bun 7.00x; rss_median_mib vs node 1.29x; rss_median_mib vs bun 3.02x; rss_max_mib vs node 1.29x; rss_max_mib vs bun 3.02x |
| memory_init64.wast | 1 | cpu_s vs bun 0.80x |
| memory_trap.wast | 1 | wall_s vs bun 0.83x |
| names.wast | 1 | cpu_s vs node 2.32x; cpu_s vs bun 5.12x; wall_s vs node 2.40x; wall_s vs bun 5.13x |
| nop.wast | 1 | cpu_s vs bun 1.31x; wall_s vs bun 1.32x |
| proposals/custom-descriptors/br_on_cast_desc_eq.wast | 1 | cpu_s vs bun 1.22x; wall_s vs bun 1.03x |
| proposals/custom-descriptors/br_on_cast_desc_eq_fail.wast | 1 | cpu_s vs bun 1.36x; wall_s vs bun 1.35x |
| return.wast | 1 | cpu_s vs bun 0.92x; wall_s vs bun 0.85x |
| return_call.wast | 1 | cpu_s vs node 15.92x; cpu_s vs bun 25.03x; wall_s vs node 14.78x; wall_s vs bun 25.32x |
| return_call_indirect.wast | 1 | cpu_s vs node 2.62x; cpu_s vs bun 5.46x; wall_s vs node 2.50x; wall_s vs bun 5.60x |
| return_call_ref.wast | 1 | cpu_s vs node 12.01x; cpu_s vs bun 22.70x; wall_s vs node 11.74x; wall_s vs bun 24.28x |
| select.wast | 1 | cpu_s vs bun 1.00x; wall_s vs bun 0.93x |
| simd_const.wast | 1 | cpu_s vs bun 1.16x; wall_s vs bun 1.20x |
| simd_f32x4.wast | 1 | cpu_s vs bun 1.76x; wall_s vs bun 1.72x |
| simd_f32x4_arith.wast | 1 | cpu_s vs node 1.93x; cpu_s vs bun 4.56x; wall_s vs node 1.86x; wall_s vs bun 4.90x |
| simd_f32x4_cmp.wast | 1 | cpu_s vs node 3.26x; cpu_s vs bun 8.11x; wall_s vs node 3.34x; wall_s vs bun 9.72x |
| simd_f32x4_pmin_pmax.wast | 1 | cpu_s vs node 9.17x; cpu_s vs bun 22.58x; wall_s vs node 9.68x; wall_s vs bun 23.65x |
| simd_f64x2.wast | 1 | cpu_s vs bun 1.80x; wall_s vs bun 1.70x |
| simd_f64x2_arith.wast | 1 | cpu_s vs node 2.27x; cpu_s vs bun 5.47x; wall_s vs node 2.26x; wall_s vs bun 5.80x |
| simd_f64x2_cmp.wast | 1 | cpu_s vs node 3.81x; cpu_s vs bun 10.02x; wall_s vs node 3.83x; wall_s vs bun 11.16x |
| simd_f64x2_pmin_pmax.wast | 1 | cpu_s vs node 7.24x; cpu_s vs bun 20.93x; wall_s vs node 6.86x; wall_s vs bun 22.67x |
| simd_i16x8_cmp.wast | 1 | cpu_s vs bun 0.95x; wall_s vs bun 1.06x |
| simd_i32x4_cmp.wast | 1 | cpu_s vs bun 0.98x; wall_s vs bun 0.95x |
| simd_i8x16_cmp.wast | 1 | cpu_s vs bun 0.94x; wall_s vs bun 0.95x |
| simd_lane.wast | 1 | wall_s vs bun 0.86x |
| skip-stack-guard-page.wast | 1 | cpu_s vs node 5.22x; cpu_s vs bun 8.62x; wall_s vs node 4.89x; wall_s vs bun 8.83x; rss_median_mib vs node 1.58x; rss_median_mib vs bun 2.53x; rss_max_mib vs node 1.58x; rss_max_mib vs bun 2.53x |
| table_copy.wast | 1 | cpu_s vs node 2.31x; cpu_s vs bun 4.09x; wall_s vs node 2.18x; wall_s vs bun 4.89x |
| table_copy64.wast | 1 | cpu_s vs node 2.26x; cpu_s vs bun 4.12x; wall_s vs node 2.22x; wall_s vs bun 4.63x |
| table_init.wast | 1 | cpu_s vs node 1.55x; cpu_s vs bun 3.22x; wall_s vs node 1.44x; wall_s vs bun 3.47x |
| table_init64.wast | 1 | cpu_s vs node 1.54x; cpu_s vs bun 3.37x; wall_s vs node 1.63x; wall_s vs bun 3.47x |
| traps.wast | 1 | wall_s vs bun 0.82x |
| unwind.wast | 1 | cpu_s vs bun 0.88x; wall_s vs bun 0.88x |
