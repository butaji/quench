// jetstream-row: richards-wasm
// jetstream-files: wasm/richards/build/richards.js wasm/richards/benchmark.js
// iterations: 50
// jetstream-driver: class=WasmEMCC preload=wasmBinary=wasm/richards/build/richards.wasm
// c-sources: wasm/richards/richards.c -I../c-analogues/support
/* C analogue of the richards-wasm row: richards.c, the source its Wasm was built from, driven as
 * its benchmark.js does: setup(200000) per iteration, scheduleIter() until it returns 0, then the
 * same packet-count validation. */
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

void setup(int count);
int scheduleIter(void);
int getQpktcount(void);
int getHoldcount(void);

enum { ITERATIONS = 50, TASK_COUNT = 200000 };
enum { EXPECTED_QPKT_COUNT = 465212, EXPECTED_HOLD_COUNT = 186084 };

static double now_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1e3 + ts.tv_nsec / 1e6;
}

static void run_iteration(void) {
    setup(TASK_COUNT);
    while (scheduleIter()) {}
}

int main(int argc, char **argv) {
    int iterations = argc > 1 ? atoi(argv[1]) : ITERATIONS;
    double start = now_ms();
    for (int i = 0; i < iterations; i++)
        run_iteration();
    double elapsed = now_ms() - start;
    int qpkt = getQpktcount(), hold = getHoldcount();
    printf("richards-wasm: %.3f ms for %d iterations\n", elapsed, iterations);
    if (qpkt != EXPECTED_QPKT_COUNT || hold != EXPECTED_HOLD_COUNT) {
        printf("validation: FAILED (qpkt %d, hold %d)\n", qpkt, hold);
        return 1;
    }
    printf("validation: ok (qpkt %d, hold %d)\n", qpkt, hold);
    return 0;
}
