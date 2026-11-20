// jetstream-row: typescript-lib
// jetstream-files: TypeScript/src/mock/sys.js TypeScript/dist/bundle.js TypeScript/benchmark.js
// iterations: 1
// jetstream-driver: class=Async preload=tsconfig=TypeScript/src/gen/immer-tiny/actually-a-type-script-config-please-dont-firewall-me.json preload=files=TypeScript/src/gen/immer-tiny/files.json
/*
 * C analogue of JetStream's typescript-lib (TypeScript/benchmark.js, compileTest over the immer-tiny
 * project of 118 files). THIS PORT IS NOT THE CHECKER. The checker stage (type checking, emit and
 * diagnostics) is NOT YET PORTED. What is ported and validated here:
 *   - Stage 1: the TypeScript 5.9.2 scanner, the parser and the JSDoc parser (support/ts-scanner.h,
 *     support/ts-parser.h, support/ts-jsdoc.h). Every file is parsed in ParseAll mode, the mode that
 *     createProgram uses, and the per-file nodeCount and identifierCount are compared with node's.
 *   - Stage 2, symbols: the binder's symbol part (support/ts-binder.h). Each parsed file is bound and
 *     its symbolCount (file.symbolCount in TypeScript) is compared with node's.
 *   - Stage 2, flow nodes: NOT PORTED. The control-flow graph (createFlowNode) is not built.
 * The benchmark reads files.json (the file name to text map) before the timed region, as the JS
 * harness preloads it, and each iteration parses all 118 files from memory with a fresh node arena.
 * Expected counts are the node 5.9.2 reference in support/ts-immer-expected.h. Each file must match,
 * or the run fails.
 * Usage: typescript-lib [iterations] [jetstream-root] */
#define _POSIX_C_SOURCE 199309L
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wunused-function"
#include "support/ts-parser.h"
#pragma GCC diagnostic pop
#include "support/ts-immer-expected.h"
#include "support/ts-binder.h"
#include "support/ts-binder-expected.h"

#define FAIL(...)                  \
  do {                             \
    fprintf(stderr, __VA_ARGS__);  \
    fputc('\n', stderr);           \
    exit(1);                       \
  } while (0)

typedef struct {
  char *name;
  char *text;
  int len;
  int kind;
} SourceEntry;

static SourceEntry sources[EXPECTED_FILE_COUNT + 8];
static int sourceCount;

static double now_ms(void) {
  struct timespec ts;
  clock_gettime(CLOCK_MONOTONIC, &ts);
  return ts.tv_sec * 1e3 + ts.tv_nsec / 1e6;
}

/* ---- files.json: a JSON object whose values are strings ---- */
static const char *jp;
static const char *jend;

static void jsonSkipWs(void) {
  while (jp < jend && (*jp == ' ' || *jp == '\t' || *jp == '\n' || *jp == '\r')) jp++;
}

static int hexDigit(char c) {
  if (c >= '0' && c <= '9') return c - '0';
  if (c >= 'a' && c <= 'f') return c - 'a' + 10;
  if (c >= 'A' && c <= 'F') return c - 'A' + 10;
  FAIL("files.json: bad hex digit");
  return 0;
}

static unsigned jsonHex4(const char *p) {
  unsigned v = 0;
  for (int i = 0; i < 4; i++) v = v * 16 + (unsigned)hexDigit(p[i]);
  return v;
}

static char *putUtf8(char *o, unsigned cp) {
  if (cp < 0x80) *o++ = (char)cp;
  else if (cp < 0x800) { *o++ = (char)(0xC0 | (cp >> 6)); *o++ = (char)(0x80 | (cp & 0x3F)); }
  else if (cp < 0x10000) {
    *o++ = (char)(0xE0 | (cp >> 12)); *o++ = (char)(0x80 | ((cp >> 6) & 0x3F)); *o++ = (char)(0x80 | (cp & 0x3F));
  } else {
    *o++ = (char)(0xF0 | (cp >> 18)); *o++ = (char)(0x80 | ((cp >> 12) & 0x3F));
    *o++ = (char)(0x80 | ((cp >> 6) & 0x3F)); *o++ = (char)(0x80 | (cp & 0x3F));
  }
  return o;
}

/* Parses a JSON string at jp; returns a malloc'd, NUL-terminated UTF-8 buffer and its byte length. */
static char *jsonString(int *outLen) {
  if (jp >= jend || *jp != '"') FAIL("files.json: expected string");
  jp++;
  char *buf = malloc((size_t)(jend - jp) + 8); /* decoded text is never longer than its source */
  char *o = buf;
  while (jp < jend && *jp != '"') {
    if (*jp != '\\') { *o++ = *jp++; continue; }
    jp++;
    char c = *jp++;
    switch (c) {
      case '"': *o++ = '"'; break;
      case '\\': *o++ = '\\'; break;
      case '/': *o++ = '/'; break;
      case 'b': *o++ = '\b'; break;
      case 'f': *o++ = '\f'; break;
      case 'n': *o++ = '\n'; break;
      case 'r': *o++ = '\r'; break;
      case 't': *o++ = '\t'; break;
      case 'u': {
        unsigned cp = jsonHex4(jp);
        jp += 4;
        if (cp >= 0xD800 && cp < 0xDC00 && jp + 6 <= jend && jp[0] == '\\' && jp[1] == 'u') {
          unsigned lo = jsonHex4(jp + 2);
          if (lo >= 0xDC00 && lo < 0xE000) {
            cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
            jp += 6;
          }
        }
        o = putUtf8(o, cp);
        break;
      }
      default: FAIL("files.json: bad escape");
    }
  }
  if (jp >= jend) FAIL("files.json: unterminated string");
  jp++;
  *o = 0;
  *outLen = (int)(o - buf);
  return buf;
}

static int kindForName(const char *name) {
  size_t n = strlen(name);
  if (n >= 4 && strcmp(name + n - 4, ".tsx") == 0) return 4;
  if (n >= 3 && strcmp(name + n - 3, ".jsx") == 0) return 2;
  if (n >= 3 && strcmp(name + n - 3, ".js") == 0) return 1;
  if (n >= 5 && strcmp(name + n - 5, ".json") == 0) return 6;
  return 3;
}

static void loadFilesJson(const char *path) {
  FILE *f = fopen(path, "rb");
  if (!f) FAIL("cannot open %s", path);
  fseek(f, 0, SEEK_END);
  long n = ftell(f);
  fseek(f, 0, SEEK_SET);
  char *src = malloc((size_t)n + 1);
  if (fread(src, 1, (size_t)n, f) != (size_t)n) FAIL("short read on %s", path);
  fclose(f);
  jp = src;
  jend = src + n;
  jsonSkipWs();
  if (jp >= jend || *jp != '{') FAIL("files.json: expected object");
  jp++;
  jsonSkipWs();
  while (jp < jend && *jp != '}') {
    if (sourceCount >= (int)(sizeof sources / sizeof sources[0])) FAIL("too many files");
    int keyLen;
    char *key = jsonString(&keyLen);
    jsonSkipWs();
    if (jp >= jend || *jp != ':') FAIL("files.json: expected colon");
    jp++;
    jsonSkipWs();
    int textLen;
    char *text = jsonString(&textLen);
    sources[sourceCount].name = key;
    sources[sourceCount].text = text;
    sources[sourceCount].len = textLen;
    sources[sourceCount].kind = kindForName(key);
    sourceCount++;
    jsonSkipWs();
    if (jp < jend && *jp == ',') { jp++; jsonSkipWs(); }
  }
  free(src);
}

/* Parses and binds every file once; with verify set, compares each file with node and returns the
 * number of files that differ. */
static int parseAllAndCheck(int verify, long *sumNodes, long *sumIdents, long *sumSymbols) {
  int bad = 0;
  *sumNodes = 0;
  *sumIdents = 0;
  *sumSymbols = 0;
  for (int i = 0; i < sourceCount; i++) {
    ts_arena_reset();
    TsNode *sf = parseSourceFile(sources[i].name, sources[i].text, sources[i].len, sources[i].kind, 0);
    tsBindSourceFile(sf, sources[i].name);
    *sumNodes += P.nodeCount;
    *sumIdents += P.identifierCount;
    *sumSymbols += sf->binderSymbolCount;
    if (!verify) continue;
    const ExpectedFile *e = NULL;
    for (int k = 0; k < EXPECTED_FILE_COUNT; k++)
      if (strcmp(expectedFiles[k].name, sources[i].name) == 0) { e = &expectedFiles[k]; break; }
    if (!e) { printf("NO REFERENCE %s\n", sources[i].name); bad++; continue; }
    if (P.nodeCount != e->nodes || P.identifierCount != e->idents) {
      printf("MISMATCH %s nodes %ld (node %ld) identifiers %ld (node %ld)\n", sources[i].name, (long)P.nodeCount,
             e->nodes, (long)P.identifierCount, e->idents);
      bad++;
    }
    const ExpectedBinder *b = NULL;
    for (int k = 0; k < EXPECTED_BINDER_COUNT; k++)
      if (strcmp(expectedBinder[k].name, sources[i].name) == 0) { b = &expectedBinder[k]; break; }
    if (!b || sf->binderSymbolCount != b->symbols) {
      printf("BINDER MISMATCH %s symbols %d (node %ld)\n", sources[i].name, sf->binderSymbolCount, b ? b->symbols : -1L);
      bad++;
    }
  }
  return bad;
}

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : 1;
  const char *root = argc > 2 ? argv[2] : "../jetstream";
  char path[4096];
  snprintf(path, sizeof path, "%s/TypeScript/src/gen/immer-tiny/files.json", root);
  loadFilesJson(path);
  if (sourceCount != EXPECTED_FILE_COUNT) FAIL("expected %d files, found %d", EXPECTED_FILE_COUNT, sourceCount);

  long sumNodes = 0, sumIdents = 0, sumSymbols = 0;
  int bad = parseAllAndCheck(1, &sumNodes, &sumIdents, &sumSymbols);
  if (bad) FAIL("%d files differ from node", bad);

  double elapsed = 0;
  for (int it = 0; it < iterations; it++) {
    double start = now_ms();
    parseAllAndCheck(0, &sumNodes, &sumIdents, &sumSymbols);
    elapsed += now_ms() - start;
  }
  printf("typescript-lib %d iterations %.3f ms\n", iterations, elapsed);
  printf("validated: %d files match node (nodeCount, identifierCount, binder symbolCount); sum nodes %ld "
         "identifiers %ld symbols %ld; flow nodes and checker not yet ported\n",
         sourceCount, sumNodes, sumIdents, sumSymbols);
  return 0;
}
