// jetstream-row: tsf-wasm
// jetstream-files: wasm/TSF/build/tsf.js wasm/TSF/benchmark.js
// iterations: 50
// jetstream-driver: class=WasmEMCC preload=wasmBinary=wasm/TSF/build/tsf.wasm
// c-sources: wasm/TSF/gpc_code_gen_util.c wasm/TSF/gpc_instruction.c wasm/TSF/gpc_intable.c wasm/TSF/gpc_program.c wasm/TSF/gpc_proto.c wasm/TSF/gpc_stack_height.c wasm/TSF/gpc_threaded.c wasm/TSF/tsf_adaptive_reader.c wasm/TSF/tsf_asprintf.c wasm/TSF/tsf_buf_reader.c wasm/TSF/tsf_buf_writer.c wasm/TSF/tsf_buffer.c wasm/TSF/tsf_copier.c wasm/TSF/tsf_define_helpers.c wasm/TSF/tsf_destructor.c wasm/TSF/tsf_error.c wasm/TSF/tsf_fsdb.c wasm/TSF/tsf_fsdb_protocol.c wasm/TSF/tsf_generator.c wasm/TSF/tsf_gpc_code_gen.c wasm/TSF/tsf_io.c wasm/TSF/tsf_io_utils.c wasm/TSF/tsf_ir.c wasm/TSF/tsf_ir_different.c wasm/TSF/tsf_ir_speed.c wasm/TSF/tsf_limits.c wasm/TSF/tsf_named_type.c wasm/TSF/tsf_native.c wasm/TSF/tsf_parser.c wasm/TSF/tsf_primitive.c wasm/TSF/tsf_ra_type_man.c wasm/TSF/tsf_reflect.c wasm/TSF/tsf_serial_in_man.c wasm/TSF/tsf_serial_out_man.c wasm/TSF/tsf_sha1.c wasm/TSF/tsf_sha1_writer.c wasm/TSF/tsf_sort.c wasm/TSF/tsf_st.c wasm/TSF/tsf_st_typetable.c wasm/TSF/tsf_stream_file_input.c wasm/TSF/tsf_stream_file_output.c wasm/TSF/tsf_type.c wasm/TSF/tsf_type_in_map.c wasm/TSF/tsf_type_out_map.c wasm/TSF/tsf_type_table.c wasm/TSF/tsf_version.c wasm/TSF/tsf_zip_abstract.c wasm/TSF/tsf_zip_attr.c wasm/TSF/tsf_zip_reader.c wasm/TSF/tsf_zip_writer.c -Iwasm/TSF -DTSF_BUILD_SYSTEM=1
/* C analogue of the tsf-wasm row: the TSF sources the row's Wasm was built from, driven as its
 * benchmark.js does (runIteration(50) per iteration). TSF writes its test files to the working
 * directory, so the driver runs in a fresh temporary directory, as the Wasm row runs in MEMFS. */
#include <dirent.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

int runIteration(unsigned count);

enum { ITERATIONS = 50, RUN_COUNT = 50 };
static const char *TEST_FILES[] = { "tsf_ir_speed_test_file.tsf", "tsf_ir_speed_test_zip_file.tsf" };

static double now_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1e3 + ts.tv_nsec / 1e6;
}

static int enter_scratch_directory(char *directory, size_t size) {
    const char *tmp = getenv("TMPDIR");
    snprintf(directory, size, "%s/quench-tsf-XXXXXX", tmp ? tmp : "/tmp");
    return mkdtemp(directory) != NULL && chdir(directory) == 0;
}

static void remove_scratch_directory(const char *directory) {
    for (size_t i = 0; i < sizeof TEST_FILES / sizeof TEST_FILES[0]; i++)
        unlink(TEST_FILES[i]);
    (void)chdir("/");
    rmdir(directory);
}

int main(int argc, char **argv) {
    int iterations = argc > 1 ? atoi(argv[1]) : ITERATIONS;
    char directory[4096];
    if (!enter_scratch_directory(directory, sizeof directory)) {
        fprintf(stderr, "cannot create a scratch directory\n");
        return 1;
    }
    double start = now_ms();
    for (int i = 0; i < iterations; i++) {
        if (runIteration(RUN_COUNT) != 0) {
            fprintf(stderr, "validation: FAILED: runIteration returned an error in iteration %d\n", i);
            remove_scratch_directory(directory);
            return 1;
        }
    }
    double elapsed = now_ms() - start;
    remove_scratch_directory(directory);
    printf("tsf-wasm: %.3f ms for %d iterations\n", elapsed, iterations);
    printf("validation: ok (runIteration returned 0 in every iteration)\n");
    return 0;
}
