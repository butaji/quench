// jetstream-row: WSL
// jetstream-files: WSL/OriginKind.js WSL/WSyntaxError.js WSL/LexerToken.js WSL/Lexer.js WSL/StandardLibrary.js WSL/AddressSpace.js WSL/Test.js
// iterations: 120
/* C analogue of JetStream's WSL row (WSL/Test.js Benchmark: buildStdlib, then run over every test).
 *
 * One iteration is buildStdlib() (prepare() with no text: the cached stdlib cloned and run through the whole
 * pipeline, Prepare.js) followed by run() (doTest: all 285 tests in sorted-name order, each parsing, checking,
 * inlining and calling its programs). The C side has the same pipeline (wsl-tests-run.h), and the same operations
 * as the JS tests (WSL_TEST_OPS, wsl-tests-table.h). The driver runs each iteration in a new script context, so the
 * stdlib is parsed again in every iteration (standardProgram is per context): tr_new_realm drops the cached parse.
 *
 * Usage: wsl [iterations] [jetstream-root]   iterations defaults to 120 (JetStreamDriver's default count),
 * root defaults to ../jetstream. The first iteration prints each failing test; the timing lines are
 * "stdlib <ms>" and "run <ms>" totals over all iterations. */
#include <setjmp.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "wsl-lex.h"
#include "wsl-inputs.h"
#include "wsl-ast.h"
#include "wsl-parse.h"
#include "wsl-clone.h"
#include "wsl-prep.h"
#include "wsl-names.h"
#include "wsl-types.h"
#include "wsl-synth.h"
#include "wsl-check.h"
#include "wsl-props.h"
#include "wsl-inline.h"
#include "wsl-visit.h"
#include "wsl-inliner.h"
#include "wsl-eval.h"
#include "wsl-tests-run.h"

static double now_ms(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return t.tv_sec * 1e3 + t.tv_nsec / 1e6;
}

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : 120;
  const char *root = argc > 2 ? argv[2] : "../jetstream";
  collect_inputs(root);
  double stdlibMs = 0, runMs = 0;
  int fails = 0;
  for (int it = 0; it < iterations; it++) {
    double t0 = now_ms();
    tr_new_realm();
    tr_prepare(NULL, 0, 0);
    double t1 = now_ms();
    fails = tr_run_all(it == 0);
    double t2 = now_ms();
    stdlibMs += t1 - t0;
    runMs += t2 - t1;
  }
  printf("wsl %d iterations stdlib %.3f ms run %.3f ms total %.3f ms\n", iterations, stdlibMs, runMs, stdlibMs + runMs);
  printf("tests %d failing %d\n", WSL_NTESTS, fails);
  return fails ? 1 : 0;
}
