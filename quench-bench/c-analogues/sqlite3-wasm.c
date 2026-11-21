// jetstream-row: sqlite3-wasm
// jetstream-files: utils/polyfills/fast-text-encoding/1.0.3/text.js sqlite3/benchmark.js sqlite3/build/jswasm/speedtest1.js
// iterations: 30
// jetstream-driver: class=WasmEMCC preload=wasmBinary=sqlite3/build/jswasm/speedtest1.wasm
// c-sources: ../c-analogues/support/sqlite3/speedtest1.c ../c-analogues/support/sqlite3/sqlite3.c -I../c-analogues/support/sqlite3 -DSQLITE_SPEEDTEST1_WASM -Dmain=speedtest1_main -DSQLITE_THREADSAFE=0 -DSQLITE_TEMP_STORE=2 -DSQLITE_ENABLE_MATH_FUNCTIONS -DSQLITE_OS_KV_OPTIONAL=1 -DSQLITE_USE_URI=1 -DSQLITE_OMIT_DEPRECATED -DSQLITE_OMIT_UTF16 -DSQLITE_OMIT_LOAD_EXTENSION -DSQLITE_OMIT_SHARED_CACHE -DSQLITE_ENABLE_BYTECODE_VTAB -DSQLITE_ENABLE_DBPAGE_VTAB -DSQLITE_ENABLE_DBSTAT_VTAB -DSQLITE_ENABLE_FTS5 -DSQLITE_ENABLE_OFFSET_SQL_FUNC -DSQLITE_ENABLE_PREUPDATE_HOOK -DSQLITE_ENABLE_RTREE -DSQLITE_ENABLE_SESSION -DSQLITE_ENABLE_STMTVTAB -DSQLITE_ENABLE_UNKNOWN_SQL_FUNCTION -DSQLITE_DEFAULT_CACHE_SIZE=-16384 -DSQLITE_DEFAULT_PAGE_SIZE=8192 -DSQLITE_DQS=0 -DSQLITE_ENABLE_API_ARMOR=1 -DSQLITE_STRICT_SUBTYPE=1 -lm
/* C analogue of the sqlite3-wasm row: SQLite 3.48.0 and its speedtest1.c, the sources the row's
 * speedtest1.wasm was built from (sqlite3/build.sh), with the compile options of the Wasm build
 * (ext/wasm/GNUmakefile SQLITE_OPT plus the defaults sqlite3-wasm.c sets: 8 KiB pages, 16 MiB
 * cache, temp store in memory, single thread). Driven as sqlite3/benchmark.js does: each iteration
 * looks up the "unix" VFS and calls speedtest1's main with the same argv. No database name is
 * given, so speedtest1 opens a temporary database, which SQLITE_TEMP_STORE=2 keeps in memory, as in
 * the row (no file I/O on either side). SQLITE_SPEEDTEST1_WASM makes main() reset its globals, as
 * the Wasm build needs for repeated calls. main() is renamed so this driver owns the entry point.
 * JetStream's row has no validate hook; validation is main() returning 0 in every iteration and
 * the database work having run (sqlite3_memory_highwater > 0). */
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

#include "sqlite3.h"

#undef main
int speedtest1_main(int argc, char **argv);

enum { ITERATIONS = 30 };

static double now_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1e3 + ts.tv_nsec / 1e6;
}

int main(int argc, char **argv) {
    int iterations = argc > 1 ? atoi(argv[1]) : ITERATIONS;
    char *row_argv[] = {
        "speedtest1", "--singlethread", "--nomemstat", "--big-transactions",
        "--size", "2", "--vfs", "unix", NULL,
    };
    int row_argc = (int)(sizeof row_argv / sizeof row_argv[0]) - 1;
    double start = now_ms();
    for (int i = 0; i < iterations; i++) {
        if (!sqlite3_vfs_find("unix")) {
            printf("validation: FAILED (no unix VFS)\n");
            return 1;
        }
        if (speedtest1_main(row_argc, row_argv) != 0) {
            printf("validation: FAILED (speedtest1 main returned nonzero in iteration %d)\n", i);
            return 1;
        }
    }
    double elapsed = now_ms() - start;
    fflush(stdout);
    printf("sqlite3-wasm: %.3f ms for %d iterations\n", elapsed, iterations);
    sqlite3_int64 high = sqlite3_memory_highwater(0);
    if (high <= 0) {
        printf("validation: FAILED (no SQLite memory was used)\n");
        return 1;
    }
    printf("validation: ok (speedtest1 main returned 0 in every iteration; the row has no validate hook)\n");
    return 0;
}
