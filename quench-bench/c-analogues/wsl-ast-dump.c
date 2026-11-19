/* Parse validation for the C WSL parser (wsl-parse.h): for every test input, prints the same
 * line as tools/wsl-ast-dump.cjs does for the row's Parse.js:
 *   index kind textFNV status nodes dumpFNV
 * then a summary line. The two outputs must be identical.
 * Usage: wsl-ast-dump [jetstream-root] [--dump N]   (root defaults to ../jetstream) */
#include <setjmp.h>
#include <stdarg.h>
#include "wsl-lex.h"
#include "wsl-inputs.h"
#include "wsl-ast.h"
#include "wsl-parse.h"

static uint32_t fnv_bytes(uint32_t h, const char *s, size_t n) {
  for (size_t i = 0; i < n; i++) {
    h ^= (unsigned char)s[i];
    h *= 16777619u;
  }
  return h;
}

/* Lexer-only input (doLex): the token list, one "kind \"text\"" line per token. */
static int dump_lex_input(const Input *in, Buf *b, char *err, size_t errcap) {
  Lexer L = {in->text, in->len, 0, in->native, "/internal/test", 0, NULL};
  Tok t;
  int r;
  int count = 0;
  while ((r = lex_next(&L, &t)) == 1) {
    if (count++) buf_str(b, "\n");
    buf_str(b, t.kind);
    buf_str(b, " \"");
    buf_put(b, t.text, (size_t)t.len);
    buf_str(b, "\"");
  }
  if (r < 0) {
    char line[512];
    snprintf(line, sizeof line, "Syntax error at /internal/test:%d: %s", lex_line_of(&L, L.index), L.errmsg);
    snprintf(err, errcap, "err:WSyntaxError: %s", line);
    return 0;
  }
  return count;
}

/* Parses one input (stdlib or doPrep) into b; returns the node count, or -1 with status set. */
static long parse_one(int i, const Input *in, Buf *b, char *status, size_t statuscap) {
  int native = i == 0 ? 1 : 0;
  const char *origin = i == 0 ? "/internal/stdlib" : "/internal/test";
  int lineOffset = i == 0 ? 27 : 0;
  static Program prog;
  memset(&prog, 0, sizeof prog);
  jmp_buf top;
  WSL_NH = 0;
  WSL_HANDLERS[WSL_NH++] = &top;
  if (setjmp(top) != 0) {
    WSL_NH = 0;
    snprintf(status, statuscap, "err:%s: %s", WSL_ERR_CLASS, WSL_ERR_MSG);
    return -1;
  }
  parse_program(&prog, origin, native, lineOffset, in->text, in->len);
  WSL_NH = 0;
  buf_str(b, "program\n");
  for (int k = 0; k < prog.statements.n; k++) dump_node(b, prog.statements.v[k], 1);
  long nodes = -1;
  for (size_t c = 0; c < b->n; c++) if (b->s[c] == '\n') nodes++;
  return nodes;
}

int main(int argc, char **argv) {
  const char *root = "../jetstream";
  int dumpAt = -1;
  for (int i = 1; i < argc; i++) {
    if (strcmp(argv[i], "--dump") == 0 && i + 1 < argc) dumpAt = atoi(argv[++i]);
    else root = argv[i];
  }
  collect_inputs(root);

  uint32_t all = 0x811c9dc5u;
  int ok = 0, nerr = 0;
  long totalNodes = 0;
  Buf dumpOut = {0};
  for (int i = 0; i < NINPUTS; i++) {
    const Input *in = &INPUTS[i];
    const char *kind = i == 0 ? "stdlib" : in->native ? "doLex" : "doPrep";
    char status[700] = "ok";
    Buf b = {0};
    long nodes = 0;
    arena_reset();
    if (i > 0 && in->native) {
      /* doLex: the token list (no "program" line) */
      int count = dump_lex_input(in, &b, status, sizeof status);
      nodes = count;
      if (strcmp(status, "ok") != 0) { b.n = 0; nodes = 0; }
    } else {
      nodes = parse_one(i, in, &b, status, sizeof status);
      if (nodes < 0) { b.n = 0; nodes = 0; }
    }
    /* the dump text has no trailing newline; the reference hashes it without one */
    size_t dlen = b.n && b.s[b.n - 1] == '\n' ? b.n - 1 : b.n;
    uint32_t dumpHash = fnv_bytes(0x811c9dc5u, b.s ? b.s : "", dlen);
    uint32_t textHash = fnv_bytes(0x811c9dc5u, in->text, (size_t)in->len);
    if (strcmp(status, "ok") == 0) ok++; else nerr++;
    totalNodes += nodes;
    all = (all ^ dumpHash) * 16777619u;
    printf("%d %s %08x %s %ld %08x\n", i, kind, textHash, status, nodes, dumpHash);
    if (i == dumpAt && b.s) { dumpOut = b; dumpOut.s[dlen] = 0; dumpOut.n = dlen; }
    else free(b.s);
  }
  printf("summary inputs %d ok %d errors %d nodes %ld hash %08x\n", NINPUTS, ok, nerr, totalNodes, all);
  if (dumpOut.s) printf("%s\n", dumpOut.s);
  return 0;
}
