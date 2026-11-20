/* The machine-translated reference for the mandreel row: the JS run as C (support/mandreel-gen.c), not
 * a row analogue of its own. The row's analogue is ../mandreel-native.cc, which builds the same
 * benchmark from Bullet's C++ source. This file has no `jetstream-row:` header, so the AOT gate does
 * not measure it; it is kept as the bit-exact baseline the native analogue is compared against.
 */
/* C analogue of JetStream's Octane/mandreel.js: Mandreel's LLVM-to-JS translation of the Bullet
 * physics demo (BenchmarkDemo), run for 80 iterations.
 *
 * The JS is machine-translated to C by tools/mandreel-to-c.cjs into support/mandreel-gen.c: the same
 * functions, the same heap (a 15.9 MB byte array with typed-array views), the same arithmetic
 * (ToInt32, Math.fround, float32 stores), the same function table for indirect calls, and the same
 * top-level script, run in source order. Regenerate with:
 *   node tools/mandreel-to-c.cjs <Octane/mandreel.js> support/mandreel-gen.c
 *
 * Per iteration (JS Benchmark.runIteration -> runMandreel), exactly as the JS does it:
 *   - the heap is reset to the snapshot taken after the data segment was emitted (the JS copies
 *     my_heap32 into heap32 word by word; the generator turns that loop into one memcpy of the same
 *     words),
 *   - the C++ global constructors run again (mandreel_call_constructors),
 *   - SetResolution(640, 480), __mandreel_internal_init and __init (which builds the Bullet world
 *     from the demo's data: the landscape and the bodies) run,
 *   - 20 frames of render() run, each with Mandreel_flushTimeouts and updateMandreelStats,
 *   - Mandreel_checkState checks a checksum of the heap.
 * Only runMandreel is timed. The setup (the JS top-level script, startApp's loader chain and
 * global_init, which emits the data segment) runs once before the timer starts, as it does at load.
 *
 * Differences from the JS, all outside the benchmark's work:
 *   - The loader chain (startApp, mandreelLoadFat, the XHR and script-element paths, and the state
 *     callbacks) is written out in F_setupMandreel below for the WebGL branch only, with the global
 *     writes that chain makes; the rest of the setup (mandreelAppInit, global_init) is translated
 *     code. The DOM and XHR objects the chain creates are not modelled, since nothing reads them.
 *   - Strings are not JS strings: puts and mandreel_fopen read the C string the JS would read and
 *     do the same string work, with the same (empty) cache lookup.
 *   - Pause statistics (updateMandreelStats) keep a count instead of an array of samples.
 *   - Typed-array accesses keep the JS rule for out-of-range indices (a store is ignored, a load gives
 *     undefined) with a range test on every access; such an access would be counted in js_oob_count,
 *     and the run fails if there is one. This run makes none.
 *   - The per-frame timeout and async-call queues are never filled in this run, so their flushes do
 *     nothing; the JS loops over empty arrays.
 *
 * Validation: node runs the same program with hashes taken after the setup and after each
 * iteration: FNV-1a over the 32-bit words of the heap, and over the bit patterns of every numeric
 * JS global (booleans as 0/1, other non-numbers as canonical NaN). Three globals are left out: the
 * wall-clock sample time, and the two holding an object or function (the fake XHR request and the
 * state callback), which the C writes as a non-zero marker where the JS tests them for truthiness.
 * The C computes the same hashes (after the timed part, so they are not timed) and compares them
 * with the constants below. Regenerate the constants with
 *   node tools/mandreel-validate.cjs --write-c support/mandreel-translated.c
 * The checksum that the JS checks each frame (Mandreel_checkState) runs inside the timed part too.
 */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "mandreel-rt.h"

/* The JS globals used by the hand-written code (defined in support/mandreel-gen.c). */
extern void *V_my_old_constructors;
extern void *V_mandreel_call_constructors;
extern double V_g_stack_pointer;
extern double V_mandreelAppStartStateFunc;
extern double V_mandreelAppLog;
extern double V_mandreelFatPreloadRequest;
extern double V_mandreelSampleTimeStart;
extern double V_g_addr_emit;
extern double V_mandreel_pos_function_table;
extern double V_r_g0;

/* The translated functions (support/mandreel-gen.c). */
extern double F_my_mandreel_call_constructors(double, double, double);
extern double F_mandreel_call_constructors(double, double, double);
extern double F_mandreelAppStartRenderWebGL(void);
extern double F_startMandreelTimer(void);
extern double F_runMandreel(void);
extern void js_toplevel_init(void);

static double state_failures = 0;   /* Mandreel_checkState mismatches (the JS prints an alert) */
static double pause_samples = 0;    /* updateMandreelStats calls (the JS pushes each sample) */

/* ---- overrides of JS functions the translator leaves to C ---- */

/* setupMandreel, with the WebGL start-up chain written out. The JS runs, in order:
 *   startApp -> mandreelAppStart (webgl): the fake DOM (canvas, flash elements), the state callbacks
 *     (appStartState does nothing for these states), mandreelLoadFat -> the fake XHR answers at once
 *     with no data, so the fat callback leaves the fat globals at their defaults and calls
 *     mandreelLoadMandreelJsScript, which inserts a script element that never loads;
 *   _mandreelAppAudioReady -> _mandreelAppStartReady -> mandreelAppStartRenderWebGL, which runs
 *     mandreelAppInit (translated) and sets imandreel_is_ready.
 * Here the globals of that chain are written with the values the JS gives them (mandreelAppStartStateFunc
 * holds appStartState, which is a function value and so only a non-zero marker here; mandreelAppLog is
 * params.log = true). The translated _mandreelAppAudioReady and _mandreelAppStartReady are not called:
 * they call the state callback, which the C has no function value for. */
double F_setupMandreel(void) {
  V_my_old_constructors = V_mandreel_call_constructors;
  V_mandreel_call_constructors = (void *)F_my_mandreel_call_constructors;
  F_startMandreelTimer();
  V_mandreelAppStartStateFunc = 1.0;
  V_mandreelAppLog = 1.0;
  V_mandreelFatPreloadRequest = 1.0;  /* the fake XHR object (a truthy marker) */
  F_mandreelAppStartRenderWebGL();
  return NAN;
}

/* The timeout queue is empty for the whole run (nothing calls Mandreel_setTimeout). */
double F_Mandreel_flushTimeouts(void) {
  return NAN;
}

/* mandreelPauseTimes.push(pause): the sample count is kept, the values are not. */
double F_updateMandreelStats(double time) {
  V_mandreelSampleTimeStart = time;
  pause_samples++;
  return NAN;
}

/* Checksum of every 100th heap word, as in mandreel.js. */
double F_Mandreel_checkState(void) {
  double sum = 0;
  for (double i = 0; i < (double)(JS_MEM / 4); i += 100) {
    sum = JS_AND(sum * 3 + LD_I32(i), 16777215.0);
  }
  if (sum != 8001026.0) {
    fprintf(stderr, "Check sum mismatch: expected ???, actual %.0f\n", sum);
    state_failures++;
  }
  return NAN;
}

/* The async call queues are empty for the whole run. */
double F___mandreel_process_async_calls(void) {
  return NAN;
}

/* puts(sp): the JS reads the C string and passes it to dump, which does nothing. */
double F_puts(double sp) {
  double addr = LD_U32(sp / 4.0);
  if (addr != 0) {
    double n = 0;
    while (LD_U8(addr + n) != 0) n++;
    (void)n;
  }
  return NAN;
}

/* mandreel_fopen(sp): reads the file name and mode (both C strings), lowercases the name, maps
 * backslashes, builds the full name, and looks it up in the (empty) cache table: not found. */
double F_mandreel_fopen(double sp) {
  double ptr_name = LD_I32(sp / 4.0);
  double ptr_flags = LD_I32(sp / 4.0 + 1);
  char name[256], full[512];
  size_t n = 0, m = 0;
  if (ptr_name != 0) {
    while (LD_U8(ptr_name + n) != 0 && n < sizeof name - 1) {
      char c = (char)LD_U8(ptr_name + n);
      name[n++] = (c >= 'A' && c <= 'Z') ? (char)(c + 32) : c == '\\' ? '/' : c;
    }
  }
  name[n] = 0;
  if (ptr_flags != 0) {
    while (LD_U8(ptr_flags + m) != 0 && m < 15) m++;
  }
  m = (size_t)snprintf(full, sizeof full, "DataPC/%s.dat", name);
  (void)m;
  /* mandreel_cache_files[name] is undefined: the file does not exist. */
  V_r_g0 = 0;
  return NAN;
}

/* ---- runtime calls of the generated code ---- */

double js_register_delegate(void *fn) {
  double id = V_mandreel_pos_function_table;
  js_fntab[(int64_t)id] = fn;
  V_mandreel_pos_function_table++;
  return id * 4;
}

/* The emit calls of one data-segment run, in order (the stores of emit_8, emit_16, emit_32 and emit_fill). */
void js_emit_ops(const struct js_emit_op *ops, int n) {
  for (int k = 0; k < n; k++) {
    for (int32_t j = 0; j < ops[k].count; j++) {
      if (ops[k].width == 1) {
        ST_U8(V_g_addr_emit, ops[k].data);
        V_g_addr_emit = V_g_addr_emit + 1.0;
      } else if (ops[k].width == 2) {
        ST_U16((js_toint32(V_g_addr_emit) >> 1u), ops[k].data);
        V_g_addr_emit = V_g_addr_emit + 2.0;
      } else {
        ST_U32((js_toint32(V_g_addr_emit) >> 2u), ops[k].data);
        V_g_addr_emit = V_g_addr_emit + 4.0;
      }
    }
  }
}

double js_emit_bytes(const uint16_t *units, int count) {
  for (int i = 0; i < count; i++) {
    ST_U8(V_g_addr_emit, units[i]);
    V_g_addr_emit++;
  }
  return NAN;
}

/* fill_ptr_from_string(ptr, s) for a string constant: the bytes, then the terminating zero. */
double js_fill_cstring(double ptr, const char *s) {
  size_t n = strlen(s);
  for (size_t i = 0; i < n; i++) ST_U8(ptr + (double)i, (unsigned char)s[i]);
  ST_U8(ptr + (double)n, 0);
  return NAN;
}

static double clock_origin_ms = -1;

double js_perf_now(void) {
  struct timespec ts;
  clock_gettime(CLOCK_MONOTONIC, &ts);
  double now = (double)ts.tv_sec * 1000.0 + (double)ts.tv_nsec / 1e6;
  if (clock_origin_ms < 0) clock_origin_ms = now;
  return now - clock_origin_ms;
}

void js_unsupported(const char *function, const char *what) {
  fprintf(stderr, "mandreel: %s reached code outside the translated subset: %s\n", function, what);
  exit(3);
}

void js_unsupported_at(const char *function, const char *what) {
  js_unsupported(function, what);
}

__attribute__((noinline, cold)) void js_oob_slow(void) { js_oob_count++; }

/* ---- storage ---- */

uint8_t js_heap[JS_MEM] __attribute__((aligned(8)));
uint8_t js_my_heap[JS_MEM] __attribute__((aligned(8)));
uint64_t js_oob_count = 0; /* typed-array accesses outside the array (see mandreel-rt.h) */
void *js_fntab[4096];

/* ---- validation ---- */

static uint32_t fnv_word(uint32_t h, uint32_t w) {
  return (h ^ w) * 16777619u;
}

/* FNV-1a over the 32-bit words of the heap, in address order. */
static uint32_t heap_hash(void) {
  uint32_t h = 0x811c9dc5u;
  const uint32_t *words = (const uint32_t *)js_heap;
  for (size_t i = 0; i < JS_MEM / 4; i++) h = fnv_word(h, words[i]);
  return h;
}

/* The same hash over the numeric globals, by name order of the translator's table. */
static uint32_t globals_hash(void) {
  uint32_t h = 0x811c9dc5u;
  for (const struct js_global_entry *e = js_global_table; e->name; e++) {
    double v = *e->value;
    uint64_t bits;
    if (v != v) {
      bits = 0x7ff8000000000000ull; /* canonical NaN: also undefined and non-numbers */
    } else {
      memcpy(&bits, &v, sizeof bits);
    }
    h = fnv_word(h, (uint32_t)bits);
    h = fnv_word(h, (uint32_t)(bits >> 32));
  }
  return h;
}

/* BEGIN EXPECTED (generated by tools/mandreel-validate.cjs --write-c; do not edit) */
#define HAVE_EXPECTED 1
#define EXPECT_ITERATIONS 80
static const uint32_t expect_setup_heap = 0xd84bdddfu;
static const uint32_t expect_setup_globals = 0x43406901u;
static const uint32_t expect_heap[EXPECT_ITERATIONS] = {0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu, 0x8a3f4edbu};
static const uint32_t expect_globals[EXPECT_ITERATIONS] = {0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u, 0x266f6bf6u};
/* END EXPECTED */

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : 80;
  /* argv[2] is the JetStream root: nothing is read from it. */
  js_toplevel_init();
  int ok = 1;
  if (getenv("MANDREEL_DUMP")) {
    /* Debugging aid: the setup state, for diffing against tools/mandreel-validate.cjs --dump. */
    char path[512];
    snprintf(path, sizeof path, "%s/c-globals.txt", getenv("MANDREEL_DUMP"));
    FILE *f = fopen(path, "w");
    for (const struct js_global_entry *e = js_global_table; e->name; e++) {
      double v = *e->value;
      uint64_t bits;
      memcpy(&bits, &v, sizeof bits);
      fprintf(f, "%s %.17g %x %x\n", e->name, v, (unsigned)(bits >> 32), (unsigned)bits);
    }
    fclose(f);
    snprintf(path, sizeof path, "%s/c-heap.bin", getenv("MANDREEL_DUMP"));
    f = fopen(path, "wb");
    fwrite(js_heap, 1, JS_MEM, f);
    fclose(f);
  }
#if HAVE_EXPECTED
  if (heap_hash() != expect_setup_heap || globals_hash() != expect_setup_globals) {
    fprintf(stderr, "setup state differs from node\n");
    ok = 0;
  }
#endif
  double total_ms = 0;
  for (int i = 0; i < iterations; i++) {
    double t0 = js_perf_now();
    F_runMandreel();
    total_ms += js_perf_now() - t0;
#if HAVE_EXPECTED
    if (i < EXPECT_ITERATIONS) {
      if (heap_hash() != expect_heap[i] || globals_hash() != expect_globals[i]) {
        fprintf(stderr, "iteration %d state differs from node\n", i);
        ok = 0;
      }
    }
#endif
  }
  printf("mandreel %d iterations %.3f ms\n", iterations, total_ms);
  if (state_failures != 0) ok = 0;
  if (js_oob_count != 0) {
    fprintf(stderr, "%llu typed-array accesses outside the array (the JS run has none)\n", (unsigned long long)js_oob_count);
    ok = 0;
  }
  printf("validation %s\n", ok ? "ok" : "FAILED");
  return ok ? 0 : 1;
}
