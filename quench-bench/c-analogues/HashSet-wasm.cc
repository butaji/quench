// jetstream-row: HashSet-wasm
// jetstream-files: wasm/HashSet/build/HashSet.js wasm/HashSet/benchmark.js
// iterations: 50
// jetstream-driver: class=WasmEMCC preload=wasmBinary=wasm/HashSet/build/HashSet.wasm
// c-sources: wasm/HashSet/HashSet.cpp -std=c++11 -DNDEBUG=1 -Dmain=jetstream_row_main -include ../c-analogues/support/hashset-prelude.h
/* C++ analogue of the HashSet-wasm row: HashSet.cpp, the source its Wasm was built from, driven as
 * its benchmark.js does: main() per iteration. main() is renamed in its translation unit so this
 * driver owns the program entry point. JetStream's row has no validate hook, so validation is
 * main() returning 0 in every iteration. */
#include <chrono>
#include <cstdio>
#include <cstdlib>

#undef main
int jetstream_row_main(int, char **);

enum { ITERATIONS = 50 };

int main(int argc, char **argv) {
    int iterations = argc > 1 ? std::atoi(argv[1]) : ITERATIONS;
    auto start = std::chrono::steady_clock::now();
    for (int i = 0; i < iterations; i++) {
        if (jetstream_row_main(0, nullptr) != 0) {
            std::printf("validation: FAILED in iteration %d\n", i);
            return 1;
        }
    }
    auto elapsed = std::chrono::steady_clock::now() - start;
    std::printf("HashSet-wasm: %.3f ms for %d iterations\n",
                std::chrono::duration<double, std::milli>(elapsed).count(), iterations);
    std::printf("validation: ok (main() returned 0 in every iteration; the row has no validate hook)\n");
    return 0;
}
