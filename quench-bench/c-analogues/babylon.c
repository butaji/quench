// jetstream-row: Babylon
// jetstream-files: ARES-6/Babylon/index.js ARES-6/Babylon/benchmark.js
// iterations: 120
// jetstream-driver: class=Async preload=airBlob=ARES-6/Babylon/air-blob.js,basicBlob=ARES-6/Babylon/basic-blob.js,inspectorBlob=ARES-6/Babylon/inspector-blob.js,babylonBlob=ARES-6/Babylon/babylon-blob.js
/* C analogue of JetStream's ARES-6 Babylon row. The JS row runs Babel's parser (ARES-6/Babylon/
 * index.js) over four blobs each iteration: air, basic and inspector as scripts and babylon as a
 * module. This analogue parses the same four sources with the shared parser in
 * js-parser-analogue.h, once per iteration, and keeps the AST alive until the next parse as
 * the JS parser does. Expected counts were produced once by running the row's own parser under
 * node 22 (tokens: true): nodes are the AST objects with a string `type`, not counting comments;
 * tokens are the File.tokens entries that are not comments (EOF included). Counts are checked on
 * the first iteration, outside the timed region. Usage: babylon [root], root defaults to
 * ../jetstream. */
#include "js-parser-analogue.h"

typedef struct {
  const char *file;
  int module;
  size_t expect_nodes, expect_tokens;
} Input;

static const Input inputs[] = {
  {"ARES-6/Babylon/air-blob.js", 0, 463, 628},
  {"ARES-6/Babylon/basic-blob.js", 0, 1273, 1559},
  {"ARES-6/Babylon/inspector-blob.js", 0, 5522, 6890},
  {"ARES-6/Babylon/babylon-blob.js", 1, 289, 376},
};
#define NINPUTS (sizeof inputs / sizeof inputs[0])

int main(int argc, char **argv) {
  const char *root = argc > 2 ? argv[2] : "../jetstream";
  const int iterations = argc > 1 ? atoi(argv[1]) : 120;
  char *srcs[NINPUTS];
  size_t lens[NINPUTS];
  char path[4096];
  for (size_t i = 0; i < NINPUTS; i++) {
    snprintf(path, sizeof path, "%s/%s", root, inputs[i].file);
    srcs[i] = js_read_file(path, &lens[i]);
  }
  JsContext ctx;
  js_context_init(&ctx);
  double total = 0;
  int ok = 1;
  for (int it = 0; it < iterations; it++) {
    double t0 = js_now_ms();
    for (size_t i = 0; i < NINPUTS; i++) {
      size_t tokens = 0;
      JsNode *file = js_parse(&ctx, srcs[i], lens[i], inputs[i].module, &tokens);
      if (it == 0) {
        size_t nodes = js_count_nodes(file);
        if (nodes != inputs[i].expect_nodes || tokens != inputs[i].expect_tokens) {
          fprintf(stderr, "%s: nodes %zu (expected %zu), tokens %zu (expected %zu)\n", inputs[i].file,
                  nodes, inputs[i].expect_nodes, tokens, inputs[i].expect_tokens);
          ok = 0;
        }
      }
    }
    total += js_now_ms() - t0;
  }
  printf("Babylon %d iterations %.3f ms (average %.3f ms)\n", iterations, total, total / iterations);
  printf("validation %s: node and token counts of the four inputs match the JS parser\n", ok ? "ok" : "FAILED");
  js_context_destroy(&ctx);
  for (size_t i = 0; i < NINPUTS; i++) free(srcs[i]);
  return ok ? 0 : 1;
}
