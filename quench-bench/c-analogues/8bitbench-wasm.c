// jetstream-row: 8bitbench-wasm
// jetstream-files: utils/polyfills/fast-text-encoding/1.0.3/text.js 8bitbench/build/rust/pkg/emu_bench.js 8bitbench/benchmark.js
// iterations: 15
// jetstream-driver: class=WasmEMCC preload=wasmBinary=8bitbench/build/rust/pkg/emu_bench_bg.wasm,romBinary=8bitbench/build/assets/program.bin
// c-sources: ../c-analogues/support/8bitbench/emu.c -I../c-analogues/support/8bitbench
/* C analogue of the 8bitbench-wasm row. The emulator (support/8bitbench/emu.c) is a port of
 * the Rust crate that emu_bench_bg.wasm was built from. This driver follows benchmark.js:
 * the ROM is read once (init), each iteration does loadRom() followed by 2*60 js_tick()
 * calls with the input "t" that getInput() returns, and validate() runs one more tick and
 * checks the byte sum of the RGBA frame handed to updateVideo(). */
#define _POSIX_C_SOURCE 200809L

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "emu.h"

enum {
    DEFAULT_ITERATIONS = 15,
    FRAMES_PER_ITERATION = 2 * 60,
};

static const uint64_t EXPECTED_VIDEO_SUM = 40792276ULL;

static double now_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1e3 + ts.tv_nsec / 1e6;
}

/* getInput() in the JetStream driver. */
static const char *get_input(void) {
    return "t";
}

/* Benchmark.init(): reads the ROM once. */
static uint8_t *read_rom(const char *path, size_t *len) {
    FILE *f = fopen(path, "rb");
    if (!f) {
        perror(path);
        return NULL;
    }
    if (fseek(f, 0, SEEK_END) != 0) {
        fclose(f);
        return NULL;
    }
    long n = ftell(f);
    if (n < 0 || fseek(f, 0, SEEK_SET) != 0) {
        fclose(f);
        return NULL;
    }
    uint8_t *buf = malloc((size_t)n);
    if (!buf) {
        fclose(f);
        return NULL;
    }
    if (fread(buf, 1, (size_t)n, f) != (size_t)n) {
        free(buf);
        fclose(f);
        return NULL;
    }
    fclose(f);
    *len = (size_t)n;
    return buf;
}

/* Benchmark.runIteration(): loadRom(romBinary), then 2*60 js_tick() calls. */
static Emu *run_iteration(Emu *emu, const uint8_t *rom, size_t rom_len) {
    emu_free(emu);
    emu = emu_load_rom(rom, rom_len);
    if (!emu) {
        fprintf(stderr, "8bitbench-wasm: could not load ROM\n");
        exit(1);
    }
    for (int i = 0; i < FRAMES_PER_ITERATION; i++)
        emu_tick(emu, get_input());
    return emu;
}

int main(int argc, char **argv) {
    int iterations = argc > 1 ? atoi(argv[1]) : DEFAULT_ITERATIONS;
    const char *root = argc > 2 ? argv[2] : "../jetstream";

    char rom_path[4096];
    snprintf(rom_path, sizeof rom_path, "%s/8bitbench/build/assets/program.bin", root);

    size_t rom_len = 0;
    uint8_t *rom = read_rom(rom_path, &rom_len);
    if (!rom)
        return 1;

    Emu *emu = NULL;
    double start = now_ms();
    for (int i = 0; i < iterations; i++)
        emu = run_iteration(emu, rom, rom_len);
    double elapsed = now_ms() - start;
    printf("8bitbench-wasm: %.3f ms for %d iterations\n", elapsed, iterations);

    /* Benchmark.validate(): one more tick, then sum the frame given to updateVideo(). */
    if (!emu) {
        printf("validation: FAILED (no emulator state)\n");
        free(rom);
        return 1;
    }
    const uint8_t *frame = emu_tick(emu, get_input());
    uint64_t sum = 0;
    for (size_t i = 0; i < EMU_FRAME_BYTES; i++)
        sum += frame[i];

    int status;
    if (sum == EXPECTED_VIDEO_SUM) {
        printf("validation: ok (video sum %llu)\n", (unsigned long long)sum);
        status = 0;
    } else {
        printf("validation: FAILED (video sum %llu, expected %llu)\n", (unsigned long long)sum,
               (unsigned long long)EXPECTED_VIDEO_SUM);
        status = 1;
    }

    emu_free(emu);
    free(rom);
    return status;
}
