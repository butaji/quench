// jetstream-row: quicksort-wasm
// jetstream-files: wasm/quicksort/build/quicksort.js wasm/quicksort/benchmark.js
// iterations: 50
// jetstream-driver: class=WasmEMCC preload=wasmBinary=wasm/quicksort/build/quicksort.wasm
// c-sources: wasm/quicksort/quicksort.c -Dmain=jetstream_row_main
/* C analogue of the quicksort-wasm row: quicksort.c, the source its Wasm was built from, driven as
 * its benchmark.js does: main() per iteration. main() is renamed in its translation unit so this
 * driver owns the program entry point. Validation checks the array main() leaves sorted, with
 * the same endpoints Quick() checks. */
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

#undef main
int jetstream_row_main(void);

enum { ITERATIONS = 50, SORT_ELEMENTS = 5000 };
extern int sortlist[];
extern int littlest, biggest;

static double now_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1e3 + ts.tv_nsec / 1e6;
}

static int sorted_correctly(void) {
    if (sortlist[1] != littlest || sortlist[SORT_ELEMENTS] != biggest)
        return 0;
    for (int i = 2; i <= SORT_ELEMENTS; i++)
        if (sortlist[i - 1] > sortlist[i])
            return 0;
    return 1;
}

int main(int argc, char **argv) {
    int iterations = argc > 1 ? atoi(argv[1]) : ITERATIONS;
    double start = now_ms();
    for (int i = 0; i < iterations; i++) {
        jetstream_row_main();
        if (!sorted_correctly()) {
            printf("validation: FAILED in iteration %d\n", i);
            return 1;
        }
    }
    double elapsed = now_ms() - start;
    printf("quicksort-wasm: %.3f ms for %d iterations\n", elapsed, iterations);
    printf("validation: ok (every run leaves %d elements sorted between the extremes)\n", SORT_ELEMENTS);
    return 0;
}
