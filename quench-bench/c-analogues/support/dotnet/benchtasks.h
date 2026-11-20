/*
 * Header-only C11 port of the BenchTasks half of the JetStream dotnet Wasm rows
 * (src/dotnet/Program.cs RunIteration, src/dotnet/Benchmarks/{BenchTask,Exceptions,Json,String}.cs).
 *
 * Measurements that RunIteration actually executes (batch = benchTasksBatchSize):
 *
 *   RunIteration loops i = 0..2 over tasks, j = 0..Measurements.Length-1, and calls
 *   task[i].RunBatch(i, batch) every time: measurementIdx is the TASK index, not j. So the
 *   same measurement runs Length times per task. Lengths: Exceptions 8, Json 6, String 15.
 *
 *   1. Exceptions.NoExceptionHandling (Measurements[0]), 8 RunBatch calls.
 *      InitialSamples 10000 -> 10000*batch RunStep calls per call, 80000*batch total.
 *      RunStep calls a non-inlined DoNothing that reads a flag; no exception is thrown.
 *      batch=10: 800000 steps. batch=50: 4000000 steps.
 *
 *   2. Json.TextDeserialize (Measurements[1]), 6 RunBatch calls.
 *      JsonTask.Initialize() is never called by RunIteration, so jsonText is null.
 *      JsonSerializer.Deserialize throws ArgumentNullException on the first step, and
 *      Measurement.RunBatch's catch(Exception) swallows it. Each call: 0 completed steps,
 *      1 managed throw. Total: 6 throws, 0 JSON work.
 *
 *   3. String.NormalizeMeasurementASCII (Measurements[2]), 15 RunBatch calls.
 *      BeforeBatch fills a 65536-char ASCII string from System.Random(123456) (Net5 compat
 *      generator). InitialSamples 3 -> 3*batch RunStep calls per call, 45*batch total.
 *      batch=10: 450 steps. batch=50: 2250 steps. Each step is an NFC quick check.
 *
 * Not ported, because RunIteration never executes them:
 *   - Exceptions 1..7 (throwing and filter variants, TryFinally). Only index 0 runs.
 *   - Json serialize/deserialize bodies and the text/org-chart data (Initialize is never
 *     called). The JSON parser and serializer are therefore not implemented here.
 *   - String measurements other than index 2 (Normalize of random Unicode, IsNormalized,
 *     culture-sensitive TextInfo/CompareInfo/Compare/IndexOf).
 * Asking for any of those aborts with a message, so the port never reports results for
 * work it did not do.
 *
 * Semantics reproduced on purpose:
 *   - measurementIdx = task index (see above).
 *   - RunBatch's try/catch wraps only the step loop. An exception ends the loop, and the
 *     call returns normally. The outer Measurements[idx] lookup is not in a try.
 *   - ASCIIStringMeasurement.BeforeBatch uses the base field len = 64K, not the 8K hidden
 *     by NormalizeMeasurementASCII. The string is therefore 65536 code units.
 *   - System.Random(seed) uses the Net5 compatibility generator (Knuth subtractive,
 *     MSEED 161803398), and Next(max) = (int)(Sample() * max) with
 *     Sample() = InternalSample() * (1.0 / int.MaxValue).
 *
 * Exceptions: a C# throw becomes setjmp/longjmp to the innermost measurement RunBatch.
 * The throw path is three nested frames, matching ThrowIfNull -> Deserialize -> RunStep:
 * bt_throw_if_null -> json_deserialize_text -> json_text_deserialize_step. The exception
 * object is malloc'd (message formatted like .NET) and freed on catch.
 *
 * NFC quick check: .NET normalizes already-normalized input by returning the same
 * instance. For code units below U+0300 (ASCII here), UAX #15 quick check says NFC YES
 * with ccc=0, so the normalized form equals the input. Input with any unit >= U+0300
 * aborts, because full NFC decomposition tables are not ported and no executed input
 * reaches that path.
 *
 * Checksum: every step result (the NFC scan length, caught exception hash, completed step
 * count per RunBatch) is mixed into the value returned by dotnet_bench_tasks. Work is
 * deterministic and is not precomputed.
 *
 * Single-threaded: the active try frame and the exception in flight are file-scope.
 * Build with -std=c11 so no FMA contraction changes the double sample arithmetic.
 */
#ifndef DOTNET_BENCHTASKS_H
#define DOTNET_BENCHTASKS_H

#include <limits.h>
#include <setjmp.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#if defined(__GNUC__) || defined(__clang__)
#define BT_NOINLINE __attribute__((noinline))
#else
#define BT_NOINLINE
#endif

#define BT_MEASUREMENTS_EXCEPTIONS 8
#define BT_MEASUREMENTS_JSON 6
#define BT_MEASUREMENTS_STRING 15
#define BT_TASK_COUNT 3

#define BT_NOEXC_INITIAL_SAMPLES 10000
#define BT_JSON_DEFAULT_INITIAL_SAMPLES 2
#define BT_STRING_INITIAL_SAMPLES 3

#define BT_STRING_LEN (64 * 1024)
#define BT_STRING_SEED 123456
#define BT_STRING_ASCII_BOUND 0x80
#define BT_NFC_QC_YES_BOUND 0x300

#define BT_NET5_MSEED 161803398
#define BT_RANDOM_MAX_INT INT_MAX
#define BT_MSG_MAX 128
#define BT_CHECKSUM_SEED 0xcbf29ce484222325ULL

_Noreturn static void bt_unsupported(const char *what)
{
    fprintf(stderr, "benchtasks: %s is not ported (RunIteration does not execute it)\n", what);
    abort();
}

/* ---- checksum ---- */

static uint64_t bt_checksum;

static uint64_t bt_mix(uint64_t h, uint64_t v)
{
    h ^= v + 0x9E3779B97F4A7C15ULL + (h << 6) + (h >> 2);
    return h;
}

static uint64_t bt_hash_bytes(const char *s)
{
    uint64_t h = 0xcbf29ce484222325ULL;
    for (; *s; s++) {
        h ^= (unsigned char)*s;
        h *= 0x100000001b3ULL;
    }
    return h;
}

/* ---- System.Random(seed): Net5 compatibility generator ---- */

typedef struct {
    int inext;
    int inextp;
    int seed_array[56];
} bt_random;

static void bt_random_init(bt_random *r, int seed)
{
    int ii = 0;
    int mk = 1;
    int subtraction = (seed == INT_MIN) ? BT_RANDOM_MAX_INT : abs(seed);
    int mj = BT_NET5_MSEED - subtraction;

    r->seed_array[55] = mj;
    for (int i = 1; i < 55; i++) {
        if ((ii += 21) >= 55)
            ii -= 55;
        r->seed_array[ii] = mk;
        mk = mj - mk;
        if (mk < 0)
            mk += BT_RANDOM_MAX_INT;
        mj = r->seed_array[ii];
    }
    for (int k = 1; k < 5; k++) {
        for (int i = 1; i < 56; i++) {
            int n = i + 30;
            if (n >= 55)
                n -= 55;
            r->seed_array[i] -= r->seed_array[1 + n];
            if (r->seed_array[i] < 0)
                r->seed_array[i] += BT_RANDOM_MAX_INT;
        }
    }
    r->inext = 0;
    r->inextp = 21;
}

static int bt_random_internal_sample(bt_random *r)
{
    int locnext = r->inext;
    int locnextp = r->inextp;
    int ret;

    if (++locnext >= 56)
        locnext = 1;
    if (++locnextp >= 56)
        locnextp = 1;
    ret = r->seed_array[locnext] - r->seed_array[locnextp];
    if (ret == BT_RANDOM_MAX_INT)
        ret--;
    if (ret < 0)
        ret += BT_RANDOM_MAX_INT;
    r->seed_array[locnext] = ret;
    r->inext = locnext;
    r->inextp = locnextp;
    return ret;
}

/* Random.Next(maxValue) = (int)(Sample() * maxValue) */
static int bt_random_next(bt_random *r, int max_value)
{
    double sample = (double)bt_random_internal_sample(r) * (1.0 / (double)BT_RANDOM_MAX_INT);
    return (int)(sample * (double)max_value);
}

/* ---- exceptions: C# throw -> longjmp to the innermost measurement RunBatch ---- */

typedef struct {
    const char *type_name;
    char *message;
} bt_exception;

static jmp_buf *bt_active_try;
static bt_exception *bt_in_flight;

static void *bt_xalloc(size_t n)
{
    void *p = malloc(n);
    if (p == NULL)
        abort();
    return p;
}

/* Innermost frame of a C# throw: allocates the exception object, then unwinds. */
static BT_NOINLINE void bt_throw_if_null(const void *arg, const char *param)
{
    bt_exception *ex;

    if (arg != NULL)
        return;
    ex = bt_xalloc(sizeof *ex);
    ex->type_name = "ArgumentNullException";
    ex->message = bt_xalloc(BT_MSG_MAX);
    snprintf(ex->message, BT_MSG_MAX, "Value cannot be null. (Parameter '%s')", param);
    bt_in_flight = ex;
    longjmp(*bt_active_try, 1);
}

/* catch (Exception): consume the exception in flight and fold its identity. */
static uint64_t bt_catch_all(void)
{
    bt_exception *ex = bt_in_flight;
    uint64_t h = bt_mix(bt_hash_bytes(ex->type_name), bt_hash_bytes(ex->message));

    free(ex->message);
    free(ex);
    bt_in_flight = NULL;
    return h;
}

/* ---- Measurement.RunBatch ---- */

typedef void (*bt_step_fn)(void);

typedef struct {
    bt_step_fn run_step;
    int initial_samples;
} bt_measurement;

static int64_t bt_current_step;

/*
 * Measurement.RunBatch(batchSize): for currentStep < InitialSamples*batchSize, RunStep.
 * The try/catch wraps only the loop. An exception ends the loop and the call returns.
 * Returns the completed step count, mixed with the caught exception if any.
 */
static uint64_t bt_measurement_run_batch(const bt_measurement *m, int batch_size)
{
    jmp_buf frame;
    jmp_buf *outer = bt_active_try;
    int64_t limit = (int64_t)m->initial_samples * (int64_t)batch_size;
    uint64_t h;

    bt_active_try = &frame;
    if (setjmp(frame) == 0) {
        for (bt_current_step = 0; bt_current_step < limit; bt_current_step++)
            m->run_step();
        h = bt_mix(0, (uint64_t)bt_current_step);
    } else {
        h = bt_mix(bt_mix(0, (uint64_t)bt_current_step), bt_catch_all());
    }
    bt_active_try = outer;
    return h;
}

/* ---- Exceptions task: Measurements[0] = NoExceptionHandling ---- */

static volatile bool bt_noexc_increase_counter = false;
static unsigned bt_noexc_unused_counter;

static BT_NOINLINE void bt_noexc_do_nothing(void)
{
    if (bt_noexc_increase_counter)
        bt_noexc_unused_counter++;
}

static void bt_noexc_run_step(void)
{
    bt_noexc_do_nothing();
}

static const bt_measurement bt_noexc = { bt_noexc_run_step, BT_NOEXC_INITIAL_SAMPLES };

/* ---- Json task: Measurements[1] = TextDeserialize ---- */

/* JsonTask.jsonText: assigned only by Initialize(), which RunIteration never calls. */
static const char *volatile bt_json_text = NULL;

static BT_NOINLINE void bt_json_deserialize_text(const char *json)
{
    bt_throw_if_null(json, "json");
    bt_unsupported("JSON parsing of non-null input");
}

static void bt_json_text_deserialize_step(void)
{
    bt_json_deserialize_text(bt_json_text);
}

static const bt_measurement bt_json_text_deserialize = {
    bt_json_text_deserialize_step, BT_JSON_DEFAULT_INITIAL_SAMPLES
};

/* ---- String task: Measurements[2] = NormalizeMeasurementASCII ---- */

static uint16_t *bt_string_data;
static uint16_t *bt_string_str;

static void bt_normalize_ascii_before_batch(void)
{
    bt_random rng;

    bt_string_data = bt_xalloc(BT_STRING_LEN * sizeof *bt_string_data);
    bt_random_init(&rng, BT_STRING_SEED);
    for (size_t i = 0; i < BT_STRING_LEN; i++)
        bt_string_data[i] = (uint16_t)bt_random_next(&rng, BT_STRING_ASCII_BOUND);

    /* new string(data): the string is a copy of the char array. */
    bt_string_str = bt_xalloc(BT_STRING_LEN * sizeof *bt_string_str);
    memcpy(bt_string_str, bt_string_data, BT_STRING_LEN * sizeof *bt_string_str);
}

static void bt_normalize_ascii_after_batch(void)
{
    free(bt_string_data);
    free(bt_string_str);
    bt_string_data = NULL;
    bt_string_str = NULL;
}

/* string.Normalize() on already-normalized input: quick check, then return the input. */
static void bt_normalize_ascii_run_step(void)
{
    const uint16_t *s = bt_string_str;
    uint32_t not_yes = 0;

    for (size_t i = 0; i < BT_STRING_LEN; i++)
        not_yes += (uint32_t)(s[i] >= BT_NFC_QC_YES_BOUND);
    if (not_yes != 0)
        bt_unsupported("NFC of input with code units >= U+0300");
    bt_checksum = bt_mix(bt_checksum, (uint64_t)BT_STRING_LEN);
}

static const bt_measurement bt_normalize_ascii = {
    bt_normalize_ascii_run_step, BT_STRING_INITIAL_SAMPLES
};

/* ---- BenchTask.RunBatch(measurementIdx, batchSize) for the executed (task, idx) pairs ---- */

static uint64_t bt_task_run_batch(int task, int measurement_idx, int batch_size)
{
    if (task == 0 && measurement_idx == 0)
        return bt_measurement_run_batch(&bt_noexc, batch_size);
    if (task == 1 && measurement_idx == 1)
        return bt_measurement_run_batch(&bt_json_text_deserialize, batch_size);
    if (task == 2 && measurement_idx == 2) {
        uint64_t h;

        bt_normalize_ascii_before_batch();
        h = bt_measurement_run_batch(&bt_normalize_ascii, batch_size);
        bt_normalize_ascii_after_batch();
        return h;
    }
    bt_unsupported("BenchTask measurement outside the executed set");
}

/*
 * RunIteration's BenchTasks loop. Returns a checksum of all executed work for one iteration.
 * batch_size is benchTasksBatchSize (10 for interp, 50 for aot).
 */
static uint64_t dotnet_bench_tasks(int batch_size)
{
    static const int measurement_count[BT_TASK_COUNT] = {
        BT_MEASUREMENTS_EXCEPTIONS, BT_MEASUREMENTS_JSON, BT_MEASUREMENTS_STRING
    };

    bt_checksum = BT_CHECKSUM_SEED;
    for (int i = 0; i < BT_TASK_COUNT; i++) {
        for (int j = 0; j < measurement_count[i]; j++) {
            /* Steps also fold into bt_checksum, so the batch result is sequenced first. */
            uint64_t h = bt_task_run_batch(i, i, batch_size);

            bt_checksum = bt_mix(bt_checksum, h);
        }
    }
    return bt_checksum;
}

#endif /* DOTNET_BENCHTASKS_H */
