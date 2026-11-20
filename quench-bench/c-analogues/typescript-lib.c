// jetstream-row: typescript-lib
// jetstream-files: TypeScript/src/mock/sys.js TypeScript/dist/bundle.js TypeScript/benchmark.js
// iterations: 1
// jetstream-driver: class=Async preload=tsconfig=TypeScript/src/gen/immer-tiny/actually-a-type-script-config-please-dont-firewall-me.json preload=files=TypeScript/src/gen/immer-tiny/files.json
/*
 * C analogue of JetStream's typescript-lib (TypeScript/benchmark.js, compileTest over the immer-tiny
 * project of 118 files). THIS PORT IS ONLY A PART OF THE CHECKER. Ported and validated here:
 *   - Stage 1: the TypeScript 5.9.2 scanner, the parser and the JSDoc parser (support/ts-scanner.h,
 *     support/ts-parser.h, support/ts-jsdoc.h). Every file is parsed in ParseAll mode, the mode that
 *     createProgram uses, and the per-file nodeCount and identifierCount are compared with node's.
 *   - Stage 2, symbols: the binder's symbol part (support/ts-binder.h). Each parsed file is bound and
 *     its symbolCount (file.symbolCount in TypeScript) is compared with node's.
 *   - Stage 2, flow nodes: the control-flow graph of binder.ts, counted per file (createFlowNode calls).
 *   - Checker, part 1 (support/ts-checker*.h): initializeTypeChecker (global merge, 6200 merge
 *     diagnostics, symbol count after init matches node's 56307), module resolution (classic),
 *     name resolution with 2304/2580, import/export member errors, duplicate index signatures (2374),
 *     redeclared variables and properties (2403/2717/2687), and the identity relation with its cache
 *     (support/ts-types.h). 45 of the 54 node diagnostics match; the other 9 need class heritage checks
 *     and base-member instantiation (mapset.ts). NOT PORTED: expression and statement checking
 *     (checkSourceFile bodies), generic instantiation beyond references, assignability, emit. The
 *     counts of types, instantiations and relation cache entries therefore stay far below node's; the
 *     run prints them next to node's numbers and fails validation.
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
#include "support/ts-checker.h"
#include "support/ts-checker-names.h"
#include "support/ts-checker-decls.h"
#include "support/ts-checker-vars.h"
#include "support/ts-checker-expected.h"

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

/* Parses and binds every file, in program order, into one arena (the checker needs all of them).
 * With verify set, compares each file with node. Returns the number of files that differ. */
static TsNode *programFiles[EXPECTED_PROGRAM_FILES];
static TsNode *srcSf[EXPECTED_FILE_COUNT + 8];

static int parseProgram(int verify, long *sumNodes, long *sumIdents, long *sumSymbols, long *sumFlows) {
  int bad = 0;
  *sumNodes = 0;
  *sumIdents = 0;
  *sumSymbols = 0;
  *sumFlows = 0;
  ts_arena_reset();
  for (int i = 0; i < sourceCount; i++) {
    TsNode *sf = parseSourceFile(sources[i].name, sources[i].text, sources[i].len, sources[i].kind, 0);
    tsBindSourceFile(sf, sources[i].name);
    srcSf[i] = sf;
    *sumNodes += P.nodeCount;
    *sumIdents += P.identifierCount;
    *sumSymbols += sf->binderSymbolCount;
    *sumFlows += sf->binderFlowCount;
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
    if (b && sf->binderFlowCount != b->flows) {
      printf("FLOW MISMATCH %s flows %d (node %ld)\n", sources[i].name, sf->binderFlowCount, b->flows);
      bad++;
    }
  }
  /* program order: getSourceFiles() of node (support/ts-checker-expected.h) */
  for (int k = 0; k < EXPECTED_PROGRAM_FILES; k++) {
    programFiles[k] = NULL;
    for (int i = 0; i < sourceCount; i++)
      if (strcmp(sources[i].name, expectedProgramOrder[k]) == 0) { programFiles[k] = srcSf[i]; break; }
    if (!programFiles[k]) { printf("NOT IN PROGRAM %s\n", expectedProgramOrder[k]); bad++; }
  }
  return bad;
}

/* ---- checker run: initializeTypeChecker, then the diagnostics of getPreEmitDiagnostics ---- */
static long checkerSymbolCount;
static int checkerInitSymbolCount; /* checker symbolCount after initializeTypeChecker, before the checks */

/* Matches the produced diagnostics with node's list by file, line, column and code. */
static int compareDiagnostics(int verbose) {
  int n = cDiagCount;
  bool *used = calloc((size_t)(n ? n : 1), sizeof(bool));
  int *line = malloc(sizeof(int) * (size_t)(n ? n : 1)), *col = malloc(sizeof(int) * (size_t)(n ? n : 1));
  for (int i = 0; i < n; i++) {
    line[i] = col[i] = 0;
    if (cDiags[i].file) cLineCol(cDiags[i].file, cDiags[i].start, &line[i], &col[i]);
  }
  int matched = 0, missing = 0, extra = 0;
  for (int e = 0; e < EXPECTED_DIAG_COUNT; e++) {
    const ExpectedDiag *x = &expectedDiags[e];
    int hit = -1;
    for (int i = 0; i < n && hit < 0; i++) {
      if (used[i] || cDiags[i].code != x->code || line[i] != x->line || col[i] != x->column) continue;
      bool sameFile = (!x->file && !cDiags[i].file) || (x->file && cDiags[i].file && strcmp(x->file, cDiags[i].file->fileName) == 0);
      if (sameFile) hit = i;
    }
    if (hit >= 0) { used[hit] = true; matched++; }
    else {
      missing++;
      if (verbose) printf("MISSING %s %d:%d %d\n", x->file ? x->file : "-", x->line, x->column, x->code);
    }
  }
  for (int i = 0; i < n; i++) {
    if (used[i]) continue;
    extra++;
    if (verbose) printf("EXTRA %s %d:%d %d\n", cDiags[i].file ? cDiags[i].file->fileName : "-", line[i], col[i], cDiags[i].code);
  }
  if (verbose) printf("diagnostics matched %d of %d (produced %d, extra %d)\n", matched, EXPECTED_DIAG_COUNT, n, extra);
  free(used); free(line); free(col);
  return missing + extra;
}

/* the member part of checkClassLikeDeclaration and checkInterfaceDeclaration: the resolved members (base
 * types instantiated, inherited members) and the index infos of every class and interface. */
static void cResolveDeclaredMembersOf(TsNode *n) {
  if ((n->kind == TSK_ClassDeclaration || n->kind == TSK_InterfaceDeclaration) && n->symbol) {
    TsType *t = cDeclaredTypeOfClassOrInterface(cGetMergedSymbol(n->symbol));
    cResolveMembers(t);
    cResolveIndexInfos(t);
  }
}

static void runChecker(void) {
  cResetTypeCaches();
  cDiagCount = 0;
  cNDupPairs = 0;
  cFiles = programFiles;
  cFileCount = EXPECTED_PROGRAM_FILES;
  cCreateIntrinsicTypes();
  cInitializeGlobals(programFiles, EXPECTED_PROGRAM_FILES);
  cCreateGlobalThisType();
  cReportAmalgamatedDuplicates();
  checkerInitSymbolCount = cSymbolCount;
  for (int i = 0; i < EXPECTED_PROGRAM_FILES; i++) cWalkNames(programFiles[i]);
  for (int i = 0; i < EXPECTED_PROGRAM_FILES; i++) cWalkAll(programFiles[i], cCheckDeclaresObjectType);
  for (int i = 0; i < EXPECTED_PROGRAM_FILES; i++) cWalkAll(programFiles[i], cCheckVariableLike);
  for (int i = 0; i < EXPECTED_PROGRAM_FILES; i++) cWalkAll(programFiles[i], cResolveDeclaredMembersOf);
}

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : 1;
  const char *root = argc > 2 ? argv[2] : "../jetstream";
  char path[4096];
  snprintf(path, sizeof path, "%s/TypeScript/src/gen/immer-tiny/files.json", root);
  loadFilesJson(path);
  if (sourceCount != EXPECTED_FILE_COUNT) FAIL("expected %d files, found %d", EXPECTED_FILE_COUNT, sourceCount);

  long sumNodes = 0, sumIdents = 0, sumSymbols = 0, sumFlows = 0;
  int bad = parseProgram(1, &sumNodes, &sumIdents, &sumSymbols, &sumFlows);
  if (bad) FAIL("%d files differ from node", bad);
  runChecker();
  checkerSymbolCount = cSymbolCount;
  printf("checker init: symbols %ld (node %d), diagnostics produced %d\n", sumSymbols + checkerInitSymbolCount,
         EXPECTED_SYMBOLS_AFTER_INIT, cDiagCount);
  int diagBad = compareDiagnostics(1);

  double elapsed = 0;
  for (int it = 0; it < iterations; it++) {
    double start = now_ms();
    parseProgram(0, &sumNodes, &sumIdents, &sumSymbols, &sumFlows);
    runChecker();
    elapsed += now_ms() - start;
  }
  printf("typescript-lib %d iterations %.3f ms\n", iterations, elapsed);
  printf("validated: %d files match node (nodeCount, identifierCount, binder symbolCount and flow node count); sum nodes %ld "
         "identifiers %ld symbols %ld flow nodes %ld\n",
         sourceCount, sumNodes, sumIdents, sumSymbols, sumFlows);
  printf("checker counts: symbols %ld (node %d), types %d (node %d, after init %d), identity cache %d (node %d), unported type requests %d\n",
         sumSymbols + checkerSymbolCount, EXPECTED_SYMBOLS, cTypeCount, EXPECTED_TYPES, EXPECTED_TYPES_AFTER_INIT,
         cIdentityCacheCount, EXPECTED_REL_IDENTITY, cUnportedCount);
  printf("checker instantiations: %d (node %d; node counts the body checks too, which are not ported)\n", cInstTotal,
         EXPECTED_INSTANTIATIONS);
  if (diagBad) printf("validation FAILED: %d diagnostics differ from node\n", diagBad);
  else printf("validation ok\n");
  return diagBad ? 1 : 0;
}
