// jetstream-row: json-stringify-inspector
// jetstream-files: SeaMonster/inspector-json-payload.js.z SeaMonster/json-stringify-inspector.js
// iterations: 20
/* C analogue of JetStream's SeaMonster/json-stringify-inspector.js. Setup (untimed) inflates the
 * same payload and builds the object in a C DOM; the canonical JSON text is produced once, as the
 * JS constructor does not serialize, so it is only used for validation. Each timed iteration
 * serializes the DOM to a fresh buffer, as JSON.stringify builds a new string. The last output must
 * equal the canonical text. Usage: json-stringify-inspector [iterations] [jetstream-root] */
#include "json-analogue.h"

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : 20;
  const char *root = argc > 2 ? argv[2] : "../jetstream";
  char path[4096];
  snprintf(path, sizeof path, "%s/SeaMonster/inspector-json-payload.js.z", root);

  JCounts counts = {0, 0, 0, 0};
  JValue *obj = load_payload(path, &counts);
  Buf canonical = {NULL, 0, 0};
  json_write(&canonical, obj);

  Buf result = {NULL, 0, 0};
  double elapsed = 0;
  for (int i = 0; i < iterations; i++) {
    buf_free(&result); /* the JS result string becomes garbage; freed outside the timed region */
    double t0 = now_ms();
    json_write(&result, obj);
    double t1 = now_ms();
    elapsed += t1 - t0;
  }

  if (result.len != canonical.len || memcmp(result.data, canonical.data, canonical.len) != 0)
    die("json-stringify-inspector: validation failed");

  printf("json-stringify-inspector %d iterations %.3f ms\n", iterations, elapsed);
  printf("checksum %016llx objects %zu strings %zu numbers %zu\n", (unsigned long long)fnv1a(result.data, result.len),
         counts.objects, counts.strings, counts.numbers);

  jv_free(obj);
  buf_free(&result);
  buf_free(&canonical);
  return 0;
}
