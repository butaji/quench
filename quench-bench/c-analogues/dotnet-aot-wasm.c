// jetstream-row: dotnet-aot-wasm
// jetstream-files: wasm/dotnet/aot.js wasm/dotnet/benchmark.js
// iterations: 15
// jetstream-driver: class=Async preload=dotnetUrl=wasm/dotnet/build-aot/wwwroot/_framework/dotnet.js,dotnetNativeUrl=wasm/dotnet/build-aot/wwwroot/_framework/dotnet.native.js,dotnetRuntimeUrl=wasm/dotnet/build-aot/wwwroot/_framework/dotnet.runtime.js,wasmBinaryUrl=wasm/dotnet/build-aot/wwwroot/_framework/dotnet.native.wasm,icuCustomUrl=wasm/dotnet/build-aot/wwwroot/_framework/icudt_CJK.dat,dllCollectionsConcurrentUrl=wasm/dotnet/build-aot/wwwroot/_framework/System.Collections.Concurrent.wasm,dllCollectionsUrl=wasm/dotnet/build-aot/wwwroot/_framework/System.Collections.wasm,dllComponentModelPrimitivesUrl=wasm/dotnet/build-aot/wwwroot/_framework/System.ComponentModel.Primitives.wasm,dllComponentModelTypeConverterUrl=wasm/dotnet/build-aot/wwwroot/_framework/System.ComponentModel.TypeConverter.wasm,dllDrawingPrimitivesUrl=wasm/dotnet/build-aot/wwwroot/_framework/System.Drawing.Primitives.wasm,dllDrawingUrl=wasm/dotnet/build-aot/wwwroot/_framework/System.Drawing.wasm,dllIOPipelinesUrl=wasm/dotnet/build-aot/wwwroot/_framework/System.IO.Pipelines.wasm,dllLinqUrl=wasm/dotnet/build-aot/wwwroot/_framework/System.Linq.wasm,dllMemoryUrl=wasm/dotnet/build-aot/wwwroot/_framework/System.Memory.wasm,dllObjectModelUrl=wasm/dotnet/build-aot/wwwroot/_framework/System.ObjectModel.wasm,dllPrivateCorelibUrl=wasm/dotnet/build-aot/wwwroot/_framework/System.Private.CoreLib.wasm,dllRuntimeInteropServicesJavaScriptUrl=wasm/dotnet/build-aot/wwwroot/_framework/System.Runtime.InteropServices.JavaScript.wasm,dllTextEncodingsWebUrl=wasm/dotnet/build-aot/wwwroot/_framework/System.Text.Encodings.Web.wasm,dllTextJsonUrl=wasm/dotnet/build-aot/wwwroot/_framework/System.Text.Json.wasm,dllAppUrl=wasm/dotnet/build-aot/wwwroot/_framework/dotnet.wasm
// c-sources: -I../c-analogues/support/dotnet -lm
/* C analogue of the dotnet-aot-wasm row: wasm/dotnet/src/dotnet (C#) ported to C, driven as
 * wasm/dotnet/benchmark.js drives the aot build: each iteration runs Interop.RunIteration with
 * benchTasksBatchSize = 50 (the Exceptions, Json and String BenchTasks, support/dotnet/benchtasks.h)
 * and renders the TwoPlanes ray tracer scene at 100x100 with hardwareConcurrency 1
 * (support/dotnet/raytracer.h). The interp and aot rows run the same C# with different sizes, so
 * both analogues share these headers. The row has no validate hook; the C validates that both
 * halves are deterministic and pinned: every iteration must produce the checksum and image hash of the
 * first, and those must equal the values the C port produced when it was written (no .NET runtime
 * was available to cross-check them). */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

#include "benchtasks.h"
#include "raytracer.h"

enum { ITERATIONS = 15, BENCH_TASKS_BATCH_SIZE = 50, SCENE_WIDTH = 100, SCENE_HEIGHT = 100 };

static const uint64_t EXPECTED_TASKS = 0x9d18fafa1ce115d3ULL, EXPECTED_IMAGE = 0xba2570b58bdca30dULL;

static double now_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1e3 + ts.tv_nsec / 1e6;
}

int main(int argc, char **argv) {
    int iterations = argc > 1 ? atoi(argv[1]) : ITERATIONS;
    uint64_t first_tasks = 0, first_image = 0;
    int mismatch = -1;
    double start = now_ms();
    for (int i = 0; i < iterations; i++) {
        uint64_t tasks = dotnet_bench_tasks(BENCH_TASKS_BATCH_SIZE);
        uint64_t image = dotnet_render(SCENE_WIDTH, SCENE_HEIGHT);
        if (i == 0) {
            first_tasks = tasks;
            first_image = image;
        } else if ((tasks != first_tasks || image != first_image) && mismatch < 0) {
            mismatch = i;
        }
    }
    double elapsed = now_ms() - start;
    printf("dotnet-aot-wasm: %.3f ms for %d iterations\n", elapsed, iterations);
    if (mismatch >= 0) {
        printf("validation: FAILED (iteration %d differs from the first)\n", mismatch);
        return 1;
    }
    if (first_tasks != EXPECTED_TASKS || first_image != EXPECTED_IMAGE) {
        printf("validation: FAILED (tasks checksum %016llx, image hash %016llx)\n",
               (unsigned long long)first_tasks, (unsigned long long)first_image);
        return 1;
    }
    printf("validation: ok (tasks checksum %016llx, image hash %016llx)\n",
           (unsigned long long)first_tasks, (unsigned long long)first_image);
    return 0;
}
