/* WSL test inputs: the standard library and the literal arguments of doPrep(...) and
 * doLex(...) in WSL/Test.js, collected the way the C analogues read them. Shared by wsl.c
 * and wsl-ast-dump.c. */
#ifndef WSL_INPUTS_H
#define WSL_INPUTS_H

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "wsl-lex.h"

/* ---- workload: the inputs of WSL/Test.js and WSL/StandardLibrary.js ---- */
typedef struct { int native; char *text; int len; } Input;
static Input *INPUTS;
static int NINPUTS, CAPINPUTS;

static void add_input(int native, const char *s, int n) {
  if (NINPUTS == CAPINPUTS) { CAPINPUTS = CAPINPUTS ? CAPINPUTS * 2 : 512; INPUTS = realloc(INPUTS, CAPINPUTS * sizeof *INPUTS); }
  char *c = malloc(n + 1);
  memcpy(c, s, n);
  c[n] = 0;
  INPUTS[NINPUTS].native = native; INPUTS[NINPUTS].text = c; INPUTS[NINPUTS].len = n;
  NINPUTS++;
}
static char *read_file(const char *path, long *len) {
  FILE *f = fopen(path, "rb");
  if (!f) { perror(path); exit(2); }
  fseek(f, 0, SEEK_END);
  long n = ftell(f);
  fseek(f, 0, SEEK_SET);
  char *b = malloc(n + 1);
  if (fread(b, 1, n, f) != (size_t)n) { fprintf(stderr, "read failed: %s\n", path); exit(2); }
  b[n] = 0;
  fclose(f);
  *len = n;
  return b;
}
/* Single or double quoted literal starting at src[i]; returns the index after it. */
static int decode_quoted(const char *src, long n, long i, char **out, int *outlen) {
  char q = src[i];
  char *buf = malloc(n - i + 1);
  int k = 0;
  for (long j = i + 1; j < n; j++) {
    char c = src[j];
    if (c == q) { buf[k] = 0; *out = buf; *outlen = k; return (int)(j + 1); }
    if (c == '\\' && j + 1 < n) {
      char d = src[++j];
      buf[k++] = d == 'n' ? '\n' : d == 't' ? '\t' : d;
      continue;
    }
    buf[k++] = c;
  }
  fprintf(stderr, "unterminated literal\n");
  exit(3);
}
static const char *ADDRESS_SPACES[] = {"constant", "device", "threadgroup", "thread"};
/* Template literal at src[i] == '`': one input, or one per address space when it has ${addressSpace}. */
static int decode_template(const char *src, long n, long i, int native) {
  long parts_cap = 0, nparts = 1;
  char *parts[8] = {0};
  int plen[8] = {0};
  int bufcap = 1 << 16;
  parts[0] = malloc(bufcap); plen[0] = 0;
  long j = i + 1;
  for (;;) {
    if (j >= n) { fprintf(stderr, "unterminated template\n"); exit(3); }
    char c = src[j];
    if (c == '`') break;
    if (c == '\\' && j + 1 < n) {
      char d = src[++j];
      parts[nparts - 1][plen[nparts - 1]++] = d == 'n' ? '\n' : d == 't' ? '\t' : d;
      j++; continue;
    }
    if (c == '$' && j + 1 < n && src[j + 1] == '{') {
      long close = j + 2;
      while (close < n && src[close] != '}') close++;
      if (close - (j + 2) != 12 || memcmp(src + j + 2, "addressSpace", 12) != 0) { fprintf(stderr, "unknown interpolation\n"); exit(3); }
      parts[nparts] = malloc(bufcap); plen[nparts] = 0; nparts++;
      j = close + 1; continue;
    }
    parts[nparts - 1][plen[nparts - 1]++] = c; j++;
  }
  long end = j + 1;
  if (nparts == 1) {
    add_input(native, parts[0], plen[0]);
  } else {
    for (int a = 0; a < 4; a++) {
      char *s = malloc(bufcap * 8);
      int k = 0;
      for (long p = 0; p < nparts; p++) {
        memcpy(s + k, parts[p], plen[p]); k += plen[p];
        if (p < nparts - 1) { int l = (int)strlen(ADDRESS_SPACES[a]); memcpy(s + k, ADDRESS_SPACES[a], l); k += l; }
      }
      add_input(native, s, k);
      free(s);
    }
  }
  for (long p = 0; p < nparts; p++) free(parts[p]);
  (void)parts_cap;
  return (int)end;
}
static void extract_test_inputs(const char *test, long n) {
  static const char *names[] = {"doPrep(", "doLex("};
  for (long i = 0; i < n; i++) {
    for (int w = 0; w < 2; w++) {
      size_t L = strlen(names[w]);
      if (i + (long)L > n || memcmp(test + i, names[w], L) != 0) continue;
      if (i > 0 && (lex_is_word((unsigned char)test[i - 1]) || test[i - 1] == '$')) continue;
      long j = i + L;
      while (j < n && lex_is_js_space((unsigned char)test[j])) j++;
      if (j >= n) continue;
      int native = w == 1;
      if (test[j] == '"' || test[j] == '\'') {
        char *s; int sl;
        decode_quoted(test, n, j, &s, &sl);
        add_input(native, s, sl);
        free(s);
      } else if (test[j] == '`') {
        decode_template(test, n, j, native);
      }
      i = j - 1;
      break;
    }
  }
}


/* StandardLibrary.js appends the generated swizzle functions to standardLibrary at load time:
 * for each maxDepth in 2..4 and maxItems in 2..4, every name of maxDepth letters over the first
 * maxItems of x, y, z, w (481 functions). The text is reproduced here exactly. */
static const char *SWIZZLE_NAMES[] = {"x", "y", "z", "w"};
static char *SWIZZLE_BUF;
static size_t SWIZZLE_LEN, SWIZZLE_CAP;
static void swizzle_put(const char *s) {
  size_t n = strlen(s);
  if (SWIZZLE_LEN + n + 1 > SWIZZLE_CAP) {
    SWIZZLE_CAP = (SWIZZLE_LEN + n + 1) * 2 + 4096;
    SWIZZLE_BUF = realloc(SWIZZLE_BUF, SWIZZLE_CAP);
  }
  memcpy(SWIZZLE_BUF + SWIZZLE_LEN, s, n);
  SWIZZLE_LEN += n;
  SWIZZLE_BUF[SWIZZLE_LEN] = 0;
}
static void gen_swizzle(int maxDepth, int maxItems, int *arr, int depth) {
  char line[256];
  if (depth == maxDepth) {
    snprintf(line, sizeof line, "vec%d<T> operator.", depth);
    swizzle_put(line);
    for (int i = 0; i < depth; i++) swizzle_put(SWIZZLE_NAMES[arr[i]]);
    snprintf(line, sizeof line, "<T>(vec%d<T> v)\n{\n    vec%d<T> result;\n", maxItems, depth);
    swizzle_put(line);
    for (int i = 0; i < depth; i++) {
      snprintf(line, sizeof line, "    result.%s = v.%s;\n", SWIZZLE_NAMES[i], SWIZZLE_NAMES[arr[i]]);
      swizzle_put(line);
    }
    swizzle_put("    return result;\n}\n");
    return;
  }
  for (int i = 0; i < maxItems; i++) {
    arr[depth] = i;
    gen_swizzle(maxDepth, maxItems, arr, depth + 1);
  }
}
static void append_generated_swizzles(void) {
  for (int maxDepth = 2; maxDepth <= 4; maxDepth++)
    for (int maxItems = 2; maxItems <= 4; maxItems++) {
      int arr[8];
      gen_swizzle(maxDepth, maxItems, arr, 0);
    }
}

/* The inputs of WSL/StandardLibrary.js and WSL/Test.js, in the order the row runs them. */
static void collect_inputs(const char *root) {
  char path[4096];
  long n;
  snprintf(path, sizeof path, "%s/WSL/StandardLibrary.js", root);
  char *std = read_file(path, &n);
  const char *lib = strstr(std, "let standardLibrary = `");
  if (!lib) { fprintf(stderr, "standard library not found\n"); exit(2); }
  decode_template(lib + strlen("let standardLibrary = "), n - (lib - std) - strlen("let standardLibrary = "), 0, 1);
  /* the row's standardLibrary is this template plus the generated swizzles */
  append_generated_swizzles();
  {
    char *full = malloc(INPUTS[0].len + SWIZZLE_LEN + 1);
    memcpy(full, INPUTS[0].text, INPUTS[0].len);
    memcpy(full + INPUTS[0].len, SWIZZLE_BUF, SWIZZLE_LEN);
    full[INPUTS[0].len + SWIZZLE_LEN] = 0;
    INPUTS[0].text = full;
    INPUTS[0].len += (int)SWIZZLE_LEN;
  }
  /* decode_template above expects the backtick at index 0 of its window, which lib + 22 is */
  free(std);
  snprintf(path, sizeof path, "%s/WSL/Test.js", root);
  char *test = read_file(path, &n);
  extract_test_inputs(test, n);
  free(test);
  /* the address space list is fixed in AddressSpace.js; check the file exists so the root is right */
  snprintf(path, sizeof path, "%s/WSL/AddressSpace.js", root);
  free(read_file(path, &n));

}

#endif
