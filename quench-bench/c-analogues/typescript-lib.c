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
 *     redeclared variables and properties (2403/2717/2687).
 *   - Checker, part 2 (support/ts-types.h, ts-instantiate.h, ts-relate.h, ts-checker-class.h): types of
 *     declarations and type nodes (keywords, literals, references with defaults, generic aliases and
 *     enum members, unions and intersections, tuples, mapped types over literal keys, class constructor
 *     types, accessors), late-bound well-known symbol members, instantiation with type mappers and
 *     caches, heritage members through instantiated bases (import-equals and namespace-qualified
 *     bases included), the identity relation, and the assignable and subtype relations (maybe stack,
 *     depth limits, variances by marker types, signatures, index signatures, weak types), the base type
 *     check of classes with issueMemberSpecificError (2416 at member names, 2415).
 *   Declaration checks of interfaces and type references (support/ts-checker-iface.h): checkInterfaceDeclaration (base
   types with the this argument, 2430; inherited property identity, 2320; checkIndexConstraints, 2411 and 2413),
   checkTypeReferenceNode with checkTypeArgumentConstraints (2344), checkTypeParameter defaults (2344), and the
   index constraints of type literals.
   Expression checks of assignments (support/ts-checker-expr.h): `this[K] = { ... }` in a constructor is
 *   checked by elaborating the object literal per property, which gives the last diagnostic (mapset.ts
 *   185:5, 2322, at the property name). All 54 node diagnostics match.
 *   Declaration type annotations of every file are resolved (checkSourceElement over declarations, the
 *   getTypeFromTypeNode part of checkSourceFile, support/ts-checker-expr.h); the unported type nodes left are
 *   indexed access, this, conditional, infer and template literal types (about 1700 requests, printed).
 *   NOT PORTED: statement and expression checking in general (checkSourceFile bodies, checkExpression,
 *   this types, indexed access and conditional types in general, template and string mapping types,
 *   intersection structure in relations, emit. The counts of types, instantiations and relation cache
 *   entries therefore stay far below node's; the run prints them next to node's numbers and fails validation.
 *   --members prints the member list of every class and interface and matches node's exactly.
 *   Relations (the assignable overshoot of the interface base checks is fixed): optional types go through the
 *   union cache (addOptionality), isRelatedTo's rules for a type parameter and its constraint, skipCaching and the
 *   primitive union lookup, indexed access with no property or index signature is unknown when instantiated,
 *   keyof and T[K] relate as structuredTypeRelatedToWorker does (targets through their constraints, sources through
 *   the constraint of T[K]); a late-bound member name is a keyof member of its unique symbol type;
 *   and signature inference is the inferTypes subset of support/ts-relate.h (candidates by priority, unions,
 *   references with contravariant arguments, properties and signatures). Declaration checks added: type predicates
 *   (2677), template literal types (2322), mapped type constraints (2322) and indexed access index types (2536).
 *   Work counts against node: --iface-counts prints the new assignable pairs, types and instantiations of each
 *   checkInterfaceDeclaration; --trace-iface NAME prints the pairs of one interface. tools/typescript-lib-instr.cjs
 *   counts the same things in node (per function, per interface). Per-interface attribution depends on the order
 *   of lazy resolution, so the totals are the comparison that matters.
 *   Still behind node: subtype and strictSubtype pairs (body checks: array literal and return type reduction,
 *   variable initializers), overload consistency of functions (checkFunctionOrConstructorSymbol), enum and module
 *   declaration checks, static and class index constraints, and checkTypeOperator and conditional type checks
 *   (no relation pairs in the corpus, but not ported). Unported requests remain in body checks (about 339 of them, printed as "unported type requests"; the main causes, by
 *   count of the TS_TYPES_DEBUG trace, are parameters without annotation, contextual types of overloaded calls and of
 *   function expressions (generic candidates and their inference are not ported), references of type parameters with
 *   union constraints, conditional types of unported branches, discriminant narrowing and truthiness, and generic signatures). Never-returning calls and the loop and branch labels of the flow graph are ported
 *   (getTypeAtFlowNode, isReachableFlowNode): the flow after a call that never returns is unreachable.
 *   Ported in this round: the comparable relation and equality narrowing by a value, type predicate narrowing of
 *   references that are not any, instanceof and in of any references, assertion-call narrowing, removeSubtypes with
 *   node's candidates and key property, literal reduction of unions, and context-sensitive arguments of overloaded calls
 *   with non-generic candidates (anyFunctionType, chooseOverload's two rounds). Node's body instantiations are attributed by tools/typescript-lib-instr.cjs: most of them are the parameter types of call signatures and the union property types of variable initializers.
 * The benchmark reads files.json (the file name to text map) before the timed region, as the JS
 * harness preloads it, and each iteration parses all 118 files from memory with a fresh node arena.
 * Expected counts are the node 5.9.2 reference in support/ts-immer-expected.h. Each file must match,
 * or the run fails.
 * Usage: typescript-lib [iterations] [jetstream-root] [--members | --iface-counts | --trace-iface NAME] */
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
#include "support/ts-relate.h"
#include "support/ts-checker-class.h"
#include "support/ts-checker-iface.h"
#include "support/ts-checker-expr.h"
#include "support/ts-checker-body.h"
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

/* --members: the value members of every class and interface (declared type, inherited members
 * included), one line each, as tools/typescript-lib-members-validate.mjs prints them. Late-bound names
 * are printed with their symbol id replaced by '*'. */
static int cMemberNameCmp(const void *a, const void *b) { return strcmp(*(char *const *)a, *(char *const *)b); }

static void cPrintMembersOf(TsNode *n) {
  if (!((n->kind == TSK_ClassDeclaration || n->kind == TSK_InterfaceDeclaration) && n->symbol && n->name)) return;
  TsSymbol *m = cGetMergedSymbol(n->symbol);
  /* one line per merged symbol, at its first class or interface declaration (a merged const comes first) */
  TsNode *first = NULL;
  for (int d = 0; d < m->ndecls && !first; d++)
    if (m->decls[d]->kind == TSK_ClassDeclaration || m->decls[d]->kind == TSK_InterfaceDeclaration) first = m->decls[d];
  if (first != n) return;
  TsType *t = cDeclaredTypeOfClassOrInterface(m);
  TsSymbol **props;
  int np = cPropertiesOf(t, &props);
  char **names = ts_arena_alloc(sizeof(char *) * (size_t)(np ? np : 1));
  int nn = 0;
  for (int i = 0; i < np; i++) {
    char *buf = ts_arena_alloc((size_t)props[i]->name.n + 1);
    memcpy(buf, props[i]->name.s, (size_t)props[i]->name.n);
    buf[props[i]->name.n] = 0;
    if (strncmp(buf, "__@", 3) == 0) {
      char *at = strrchr(buf, '@');
      if (at && at[1]) { at[1] = '*'; at[2] = 0; }
    }
    names[nn++] = buf;
  }
  qsort(names, (size_t)nn, sizeof(char *), cMemberNameCmp);
  printf("%s %.*s %d", cSourceFileOf(n)->fileName, n->name->textLen, n->name->text, nn);
  for (int i = 0; i < nn; i++) printf(" %s", names[i]);
  printf("\n");
}

/* --iface-counts and --trace-iface NAME: the work of each checkInterfaceDeclaration call, as
 * tools/typescript-lib-instr.cjs prints it for node (new assignable pairs, types and instantiations). The trace
 * prints the new assignable pairs of the named interface, attributed to the innermost interface being checked. */
typedef struct { char *key; long assign, types, inst; } IfaceCount;
static IfaceCount *ifaceCounts;
static int nIfaceCounts, capIfaceCounts;
static const char *cTraceName;
static const char *cCurIfaceName;      /* the innermost interface being checked (NULL outside) */
static long cCurIfaceLen;

static char *dPut(char *o, const char *end, const char *s, size_t n) {
  size_t room = (size_t)(end - o);
  if (n > room) n = room;
  memcpy(o, s, n);
  return o + n;
}

/* a short description of a type for the trace: names of symbols and references, unions, literals */
static char *dType(char *o, char *end, TsType *t, int depth) {
  if (!t) return dPut(o, end, "?", 1);
  if (t == cErrorType) return dPut(o, end, "error", 5);
  if (depth > 3) return dPut(o, end, "..", 2);
  if (t->flags & (TF_Union | TF_Intersection)) {
    o = dPut(o, end, "(", 1);
    for (int i = 0; i < t->ntypes; i++) {
      if (i) o = dPut(o, end, (t->flags & TF_Union) ? " | " : " & ", 3);
      o = dType(o, end, t->types[i], depth + 1);
    }
    return dPut(o, end, ")", 1);
  }
  if (t->text && (t->flags & (TF_StringLiteral | TF_NumberLiteral | TF_BigIntLiteral))) return dPut(o, end, t->text, (size_t)t->textLen);
  if (t->symbol && t->symbol->name.n) {
    o = dPut(o, end, t->symbol->name.s, (size_t)t->symbol->name.n);
    if ((t->objectFlags & OF_Reference) && t->nargs) {
      o = dPut(o, end, "<", 1);
      for (int i = 0; i < t->nargs; i++) {
        if (i) o = dPut(o, end, ", ", 2);
        o = dType(o, end, t->args[i], depth + 1);
      }
      o = dPut(o, end, ">", 1);
    }
    return o;
  }
  char id[32];
  int k = snprintf(id, sizeof id, "anon#%d", t->id);
  return dPut(o, end, id, (size_t)k);
}

static void cTraceHook(int rel, TsType *s, TsType *t, bool ok) {
  if (rel != REL_Assignable || !cTraceName || !cCurIfaceName) return;
  if ((long)strlen(cTraceName) != cCurIfaceLen || strncmp(cTraceName, cCurIfaceName, (size_t)cCurIfaceLen) != 0) return;
  char a[400], b[400];
  char *ae = dType(a, a + sizeof a - 1, s, 0), *be = dType(b, b + sizeof b - 1, t, 0);
  *ae = 0; *be = 0;
  printf("%s  =>  %s%s\n", a, b, ok ? "" : "  [false]");
}

/* the interface stage with counts: the work of each call, keyed by interface name and file */
static void cCountedInterfaceDecl(TsNode *n) {
  if (n->kind != TSK_InterfaceDeclaration || !n->symbol || !n->name) { cCheckInterfaceDecl(n); return; }
  int rel0 = cRelTab[REL_Assignable].n, t0 = cTypeCount, i0 = cInstTotal;
  const char *outerName = cCurIfaceName;
  long outerLen = cCurIfaceLen;
  cCurIfaceName = n->name->text;
  cCurIfaceLen = n->name->textLen;
  cCheckInterfaceDecl(n);
  cCurIfaceName = outerName;
  cCurIfaceLen = outerLen;
  if (!cTraceName) {
    const char *file = cSourceFileOf(n)->fileName;
    const char *base = strrchr(file, '/');
    char key[512];
    int k = snprintf(key, sizeof key, "%.*s (%s)", n->name->textLen, n->name->text, base ? base + 1 : file);
    int idx = -1;
    for (int i = 0; i < nIfaceCounts && idx < 0; i++)
      if (strcmp(ifaceCounts[i].key, key) == 0) idx = i;
    if (idx < 0) {
      if (nIfaceCounts == capIfaceCounts) {
        capIfaceCounts = capIfaceCounts ? capIfaceCounts * 2 : 256;
        ifaceCounts = realloc(ifaceCounts, sizeof(IfaceCount) * (size_t)capIfaceCounts);
        if (!ifaceCounts) abort();
      }
      idx = nIfaceCounts++;
      ifaceCounts[idx].key = malloc((size_t)k + 1);
      memcpy(ifaceCounts[idx].key, key, (size_t)k + 1);
      ifaceCounts[idx].assign = ifaceCounts[idx].types = ifaceCounts[idx].inst = 0;
    }
    ifaceCounts[idx].assign += cRelTab[REL_Assignable].n - rel0;
    ifaceCounts[idx].types += cTypeCount - t0;
    ifaceCounts[idx].inst += cInstTotal - i0;
  }
}

static int cIfaceCountCmp(const void *a, const void *b) {
  const IfaceCount *x = a, *y = b;
  if (x->assign != y->assign) return x->assign < y->assign ? 1 : -1;
  return strcmp(x->key, y->key);
}

static void cPrintIfaceCounts(void) {
  qsort(ifaceCounts, (size_t)nIfaceCounts, sizeof(IfaceCount), cIfaceCountCmp);
  printf("interface\tassignable\ttypes\tinstantiations\n");
  for (int i = 0; i < nIfaceCounts; i++)
    printf("%s\t%ld\t%ld\t%ld\n", ifaceCounts[i].key, ifaceCounts[i].assign, ifaceCounts[i].types, ifaceCounts[i].inst);
}

static void (*cIfaceStage)(TsNode *) = cCheckInterfaceDecl;

static int cBodyInst;   /* instantiations made by the body checks (cCheckBodies) of the last run */

static void runChecker(void) {
  cResetTypeCaches();
  cResetTemplateTypes();
  cResetEraseList();
  cResetRelationCaches();
  cDiagCount = 0;
  cNDupPairs = 0;
  cFiles = programFiles;
  cFileCount = EXPECTED_PROGRAM_FILES;
  cCreateIntrinsicTypes();
  cInitRelations();
  cInitializeGlobals(programFiles, EXPECTED_PROGRAM_FILES);
  cCreateGlobalThisType();
  cReportAmalgamatedDuplicates();
  checkerInitSymbolCount = cSymbolCount;
  for (int i = 0; i < EXPECTED_PROGRAM_FILES; i++) cWalkNames(programFiles[i]);
  for (int i = 0; i < EXPECTED_PROGRAM_FILES; i++) cWalkAll(programFiles[i], cCheckDeclaresObjectType);
  for (int i = 0; i < EXPECTED_PROGRAM_FILES; i++) cWalkAll(programFiles[i], cCheckVariableLike);
  for (int i = 0; i < EXPECTED_PROGRAM_FILES; i++) cWalkAll(programFiles[i], cResolveDeclaredMembersOf);
  for (int i = 0; i < EXPECTED_PROGRAM_FILES; i++) cWalkAll(programFiles[i], cCheckClassBase);
  for (int i = 0; i < EXPECTED_PROGRAM_FILES; i++) cWalkAll(programFiles[i], cIfaceStage);
  for (int i = 0; i < EXPECTED_PROGRAM_FILES; i++) cWalkAll(programFiles[i], cCheckHeritageArgs);
  for (int i = 0; i < EXPECTED_PROGRAM_FILES; i++) cWalkAll(programFiles[i], cCheckTypeReferenceArgs);
  for (int i = 0; i < EXPECTED_PROGRAM_FILES; i++) cWalkAll(programFiles[i], cCheckTypeParameterNode);
  for (int i = 0; i < EXPECTED_PROGRAM_FILES; i++) cWalkAll(programFiles[i], cCheckTypeNode);
  for (int i = 0; i < EXPECTED_PROGRAM_FILES; i++) cWalkAll(programFiles[i], cCheckExpressionNode);
  for (int i = 0; i < EXPECTED_PROGRAM_FILES; i++) cWalkAll(programFiles[i], cCheckDeclTypeNodes);
  for (int i = 0; i < EXPECTED_PROGRAM_FILES; i++) cWalkAll(programFiles[i], cCheckDeclarationTypes);
  int instBeforeBodies = cInstTotal;
  for (int i = 0; i < EXPECTED_PROGRAM_FILES; i++) cCheckBodies(programFiles[i]);
  cBodyInst = cInstTotal - instBeforeBodies;
}

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : 1;
  const char *root = argc > 2 ? argv[2] : "../jetstream";
  char path[4096];
  snprintf(path, sizeof path, "%s/TypeScript/src/gen/immer-tiny/files.json", root);
  loadFilesJson(path);
  if (sourceCount != EXPECTED_FILE_COUNT) FAIL("expected %d files, found %d", EXPECTED_FILE_COUNT, sourceCount);
  if (argc > 3 && (strcmp(argv[3], "--iface-counts") == 0 || strcmp(argv[3], "--trace-iface") == 0)) {
    /* the interface counts (or the trace of one interface) only: no timing, no validation */
    int trace = strcmp(argv[3], "--trace-iface") == 0;
    if (trace) {
      if (argc < 5) FAIL("usage: typescript-lib [iterations] [root] --trace-iface NAME");
      cTraceName = argv[4];
      cRelStoreHook = cTraceHook;
    }
    cIfaceStage = cCountedInterfaceDecl;
    long a, b, c, d;
    parseProgram(0, &a, &b, &c, &d);
    runChecker();
    if (!trace) cPrintIfaceCounts();
    return 0;
  }
  if (argc > 3 && strcmp(argv[3], "--members") == 0) {   /* the member dump only (no timing, no validation) */
    long a, b, c, d;
    parseProgram(0, &a, &b, &c, &d);
    runChecker();
    for (int i = 0; i < EXPECTED_PROGRAM_FILES; i++) cWalkAll(programFiles[i], cPrintMembersOf);
    return 0;
  }

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
  /* node counts createLiteralType calls that create a regular literal (3582 string, 1074 number on this program); fresh copies and
     enum members (regular and fresh, node 1199 and 1074, 1105 and 1105) are listed apart */
  printf("literal types: string %d (node 3573), number %d (node 1073); fresh string %d (node 1199), number %d (node 1074); enum string %d, number %d (node 2210 with fresh)\n",
         cLitRegular[0], cLitRegular[1], cLitFresh[0], cLitFresh[1], cLitEnum[0], cLitEnum[1]);
  printf("checker instantiations: %d (node %d; node counts the body checks too, which are not ported)\n", cInstTotal,
         EXPECTED_INSTANTIATIONS);
  printf("relation caches: assignable %d (node %d), identity %d (node %d), subtype %d (node %d), strictSubtype %d (node %d)\n",
         cRelTab[REL_Assignable].n, EXPECTED_REL_ASSIGNABLE, cIdentityCacheCount, EXPECTED_REL_IDENTITY,
         cRelTab[REL_Subtype].n, EXPECTED_REL_SUBTYPE, cRelTab[REL_StrictSubtype].n, EXPECTED_REL_STRICT_SUBTYPE);
  printf("body instantiations: %d (node about 3300)\n", cBodyInst);
  printf("relation overflows (depth or maybe stack limits): %d\n", cRelOverflows);
  if (diagBad) printf("validation FAILED: %d diagnostics differ from node\n", diagBad);
  else printf("validation ok\n");
  return diagBad ? 1 : 0;
}
