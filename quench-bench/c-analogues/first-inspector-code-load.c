// jetstream-row: first-inspector-code-load
// jetstream-files: code-load/code-first-load.js
// iterations: 120
// jetstream-driver: class=Async preload=inspectorPayloadBlob=code-load/inspector-payload-minified.js
/* C analogue of JetStream's first-inspector-code-load row. Each iteration the JS row evaluates
 * "function test<i>() { let _____top_level_____ = <random>; <inspector payload> }" with indirect
 * eval. Preparing that text is outside the timed region, as in the driver
 * (prepareForNextIteration); the timed part is the parse of the text, which is what eval does
 * for a 2 MB source. This analogue builds the same text (the function index increments each
 * iteration and the random literal is fixed for the run, as in init) and parses it with the
 * shared parser. Execution is not part of the analogue. Expected counts: node 22 with the row's
 * own parser (tokens: true) on the wrapped text, 418226 nodes and 522926 tokens, checked on the
 * first iteration. Usage: first-inspector-code-load [root], default ../jetstream. */
#include "js-parser-analogue.h"

int main(int argc, char **argv) {
  const char *root = argc > 2 ? argv[2] : "../jetstream";
  const int iterations = argc > 1 ? atoi(argv[1]) : 120;
  char path[4096];
  size_t plen;
  snprintf(path, sizeof path, "%s/code-load/inspector-payload-minified.js", root);
  char *payload = js_read_file(path, &plen);
  /* init(): inspectorText = `let _____top_level_____ = ${Math.random()}; ${payload}` */
  const char *prefix = "let _____top_level_____ = 0.7392847591234567; ";
  size_t prefix_n = strlen(prefix);
  size_t body_n = prefix_n + plen;
  char *body = malloc(body_n + 1);
  memcpy(body, prefix, prefix_n);
  memcpy(body + prefix_n, payload, plen);
  body[body_n] = 0;
  free(payload);
  char *text = malloc(body_n + 64);
  JsContext ctx;
  js_context_init(&ctx);
  double total = 0;
  int ok = 1;
  for (int it = 0; it < iterations; it++) {
    /* prepareForNextIteration (untimed): this.text = `function test${index}() { ${inspectorText} }` */
    int tn = snprintf(text, 64, "function test%d() { ", it);
    memcpy(text + tn, body, body_n);
    memcpy(text + tn + body_n, " }", 2);
    size_t n = (size_t)tn + body_n + 2;
    double t0 = js_now_ms();
    size_t tokens = 0;
    JsNode *file = js_parse(&ctx, text, n, 0, &tokens);
    total += js_now_ms() - t0;
    if (it == 0) {
      size_t nodes = js_count_nodes(file);
      if (nodes != 418226 || tokens != 522926) {
        fprintf(stderr, "nodes %zu (expected 418226), tokens %zu (expected 522926)\n", nodes, tokens);
        ok = 0;
      }
    }
  }
  printf("first-inspector-code-load %d iterations %.3f ms (average %.3f ms)\n", iterations, total, total / iterations);
  printf("validation %s: node and token counts of the wrapped payload match the JS parser\n", ok ? "ok" : "FAILED");
  js_context_destroy(&ctx);
  free(text);
  free(body);
  return ok ? 0 : 1;
}
