// jetstream-row: json-parse-inspector
// jetstream-files: SeaMonster/inspector-json-payload.js.z SeaMonster/json-parse-inspector.js
// iterations: 20
// jetstream-driver: exports=obj
/* C analogue of JetStream's SeaMonster/json-parse-inspector.js. Setup (untimed, as in the JS
 * constructor) inflates the same payload, builds the object, and serializes it to the input
 * string. Each timed iteration parses that string into a fresh DOM: one node per JSON value in an
 * arena (as JSON.parse allocates one JS value per node), strings without escapes as views into
 * the input. The arena is reset before each timed parse, so the previous DOM is released outside
 * the timed region. The last result is serialized again and must equal the input string.
 * Usage: json-parse-inspector [iterations [jetstream-root]]; run from the repo root. */
#include "json-analogue.h"

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : 20; /* argv[1] = iterations (// iterations: header) */
  const char *root = argc > 2 ? argv[2] : "quench-bench/jetstream";
  char path[4096];
  snprintf(path, sizeof path, "%s/SeaMonster/inspector-json-payload.js.z", root);

  JCounts counts = {0, 0, 0, 0};
  JDoc *setup = doc_new();
  JValue *obj = load_payload(setup, path, &counts);
  Buf str = {NULL, 0, 0};
  json_write(&str, obj); /* this.str = JSON.stringify(obj) */
  doc_free(setup);

  JDoc *doc = doc_new();
  JCounts scratch = {0, 0, 0, 0};
  JValue *result = NULL;
  double elapsed = 0;
  for (int i = 0; i < iterations; i++) {
    doc_reset(doc); /* the JS result becomes garbage; released outside the timed region */
    result = NULL;
    double t0 = now_ms();
    result = json_parse(doc, str.data, str.len, &scratch);
    double t1 = now_ms();
    if (!result) die("json-parse-inspector: parse failed");
    elapsed += t1 - t0;
  }

  Buf again = {NULL, 0, 0};
  json_write(&again, result);
  if (again.len != str.len || memcmp(again.data, str.data, str.len) != 0) die("json-parse-inspector: validation failed");

  printf("json-parse-inspector %d iterations %.3f ms\n", iterations, elapsed);
  printf("checksum %016llx objects %zu strings %zu numbers %zu\n", (unsigned long long)fnv1a(str.data, str.len),
         counts.objects, counts.strings, counts.numbers);

  doc_free(doc);
  buf_free(&again);
  buf_free(&str);
  return 0;
}
