// jetstream-row: zlib-wasm
// jetstream-files: wasm/zlib/build/zlib.js wasm/zlib/benchmark.js
// iterations: 40
// jetstream-driver: class=WasmEMCC preload=wasmBinary=wasm/zlib/build/zlib.wasm
// c-sources: wasm/zlib/src/zpipe.c wasm/zlib/src/main.c -Dmain=jetstream_row_main -lz
/* C analogue of the zlib-wasm row: zpipe.c and main.c, the sources its Wasm was built from, linked
 * against the system zlib, driven as its benchmark.js does: each iteration writes the row's input
 * (the zlib.wasm binary) into a file, then ten compress/decompress round trips. Validation compares
 * the round-trip output with the input. Arguments: [iterations] [JetStream root]. The JetStream root
 * holds the input file; it defaults to quench-bench/jetstream. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

#undef main

int compressFile(const char *infilename, const char *outfilename);
int decompressFile(const char *infilename, const char *outfilename);

enum { ITERATIONS = 40, ROUND_TRIPS = 10 };
#define INPUT_NAME "input"
#define COMPRESSED_NAME "input.z"
#define ROUND_TRIP_NAME "input.z.out"
#define INPUT_PATH "wasm/zlib/build/zlib.wasm"

static double now_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1e3 + ts.tv_nsec / 1e6;
}

static unsigned char *read_file(const char *path, size_t *size) {
    FILE *file = fopen(path, "rb");
    if (!file)
        return NULL;
    fseek(file, 0, SEEK_END);
    *size = (size_t)ftell(file);
    rewind(file);
    unsigned char *data = malloc(*size);
    if (data && fread(data, 1, *size, file) != *size) {
        free(data);
        data = NULL;
    }
    fclose(file);
    return data;
}

static int write_file(const char *path, const unsigned char *data, size_t size) {
    FILE *file = fopen(path, "wb");
    if (!file)
        return 0;
    size_t written = fwrite(data, 1, size, file);
    return fclose(file) == 0 && written == size;
}

static long file_size(const char *path) {
    struct stat info;
    return stat(path, &info) == 0 ? (long)info.st_size : -1;
}

static int round_trips(long input_size) {
    for (int i = 0; i < ROUND_TRIPS; i++) {
        if (compressFile(INPUT_NAME, COMPRESSED_NAME) != 0 || decompressFile(COMPRESSED_NAME, ROUND_TRIP_NAME) != 0)
            return 0;
        if (file_size(INPUT_NAME) != file_size(ROUND_TRIP_NAME) || file_size(ROUND_TRIP_NAME) != input_size)
            return 0;
    }
    return 1;
}

static int validate(const unsigned char *input, size_t size) {
    size_t output_size = 0;
    unsigned char *output = read_file(ROUND_TRIP_NAME, &output_size);
    int ok = output && output_size == size && memcmp(input, output, size) == 0;
    free(output);
    return ok;
}

static void remove_scratch_files(void) {
    unlink(INPUT_NAME);
    unlink(COMPRESSED_NAME);
    unlink(ROUND_TRIP_NAME);
}

int main(int argc, char **argv) {
    int iterations = argc > 1 ? atoi(argv[1]) : ITERATIONS;
    const char *root = argc > 2 ? argv[2] : "quench-bench/jetstream";
    char path[4096], directory[4096];
    snprintf(path, sizeof path, "%s/%s", root, INPUT_PATH);
    size_t size = 0;
    unsigned char *input = read_file(path, &size);
    if (!input) {
        fprintf(stderr, "cannot read %s\n", path);
        return 1;
    }
    const char *tmp = getenv("TMPDIR");
    snprintf(directory, sizeof directory, "%s/quench-zlib-XXXXXX", tmp ? tmp : "/tmp");
    if (!mkdtemp(directory) || chdir(directory) != 0)
        return 1;
    double start = now_ms();
    for (int i = 0; i < iterations; i++) {
        if (!write_file(INPUT_NAME, input, size) || !round_trips((long)size)) {
            printf("validation: FAILED in iteration %d\n", i);
            return 1;
        }
    }
    double elapsed = now_ms() - start;
    int ok = validate(input, size);
    remove_scratch_files();
    (void)chdir("/");
    rmdir(directory);
    free(input);
    printf("zlib-wasm: %.3f ms for %d iterations\n", elapsed, iterations);
    if (ok)
        printf("validation: ok (round trip of %zu bytes matches the input)\n", size);
    else
        printf("validation: FAILED (round trip does not match the input)\n");
    return ok ? 0 : 1;
}
