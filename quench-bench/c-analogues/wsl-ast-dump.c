/* Parse validation for the C WSL parser (wsl-parse.h): for every test input, prints the same
 * line as tools/wsl-ast-dump.cjs does for the row's Parse.js:
 *   index kind textFNV status nodes dumpFNV
 * then a summary line. The two outputs must be identical.
 * Usage: wsl-ast-dump [jetstream-root] [--dump N] [--bench ITERATIONS]
 * (root defaults to ../jetstream) */
#include <setjmp.h>
#include <stdarg.h>
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
static int CLONE_FLAG;
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
  if (CLONE_FLAG && native) {
    /* cloneProgram(program): dump the clone instead of the parse tree */
    AstList cl = {0};
    statements_clone(&cl, &prog.statements);
    for (int k = 0; k < cl.n; k++) dump_node(b, cl.v[k], 1);
  } else
    for (int k = 0; k < prog.statements.n; k++) dump_node(b, prog.statements.v[k], 1);
  long nodes = -1;
  for (size_t c = 0; c < b->n; c++) if (b->s[c] == '\n') nodes++;
  return nodes;
}

static double now_ms(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return t.tv_sec * 1e3 + t.tv_nsec / 1e6;
}

/* Timed parse stage: every iteration parses all inputs (stdlib and doPrep) into fresh programs,
 * the same work as the row's Parse.js loop in tools/wsl-ast-dump.cjs --bench. No dump is built. */
static void bench_parse(int iterations) {
  long parsed = 0, failed = 0;
  double elapsed = 0;
  for (int it = 0; it < iterations; it++) {
    double t0 = now_ms();
    long p = 0, f = 0;
    for (int i = 0; i < NINPUTS; i++) {
      const Input *in = &INPUTS[i];
      if (i > 0 && in->native) continue;   /* doLex inputs are not parsed */
      arena_reset();
      static Program prog;
      memset(&prog, 0, sizeof prog);
      jmp_buf top;
      WSL_NH = 0;
      WSL_HANDLERS[WSL_NH++] = &top;
      if (setjmp(top) == 0) {
        parse_program(&prog, i == 0 ? "/internal/stdlib" : "/internal/test", i == 0, i == 0 ? 27 : 0, in->text, in->len);
        p++;
      } else
        f++;
      WSL_NH = 0;
    }
    elapsed += now_ms() - t0;
    parsed = p;
    failed = f;
  }
  printf("wsl parse %d iterations %.3f ms\n", iterations, elapsed);
  printf("parsed %ld failed %ld\n", parsed, failed);
}

/* --stage N (as tools/wsl-ast-dump.cjs): 0 parses each input; 1 clones the standard library (or
 * parses a doPrep input into the clone and reduces it with programWithUnnecessaryThingsRemoved),
 * then folds constexprs. The standard library is parsed once and kept below the arena mark. */
static int STAGE = 0;
static int UNIFY_MODE = 0;   /* --unify (1): the pairwise unification harness; --resolve (2): the overload harness */
static int PROPS_MODE = 0;   /* --props: check, the literal check and resolveProperties, then the stage-6 dump */
static int POST_MODE = 0;    /* --post: check, then the post-check passes; status lines only */
static int CHECK_MODE = 0;   /* --check: the checker on the stage-6 state, with its expression trace */
static int VISIT_MODE = 0;   /* --visit: the post-check state, the instances, then the Visitor.js traversal trace (wsl-visit.h) */
static int INSTANTIATE_MODE = 0;   /* --instantiate: the post-check state, then the instances of inline()'s first loop */
static int INLINE_MODE = 0;   /* --inline: inline() on the post-check state, then the instances with their EPtrs and the buffers */
static int EVAL_MODE = 0;   /* --eval: inline(), then the evaluator on each non-generic function without parameters */
static int EBUF_MODE = 0;   /* --ebuf: LateChecker, the layout and EBufferBuilder of each instance, then the EBuffers */
static int LAYOUT_MODE = 0;   /* --layout: LateChecker and the layout of each instance, then the layout log */
static int LATE_MODE = 0;   /* --late: the post-check state, then LateChecker's traversal trace of each instance (wsl-late.h) */
static Program STD_PROG, PREP_IN, PREP_OUT;
static size_t STD_MARK;

static void std_parse_once(void) {
  arena_reset();
  memset(&STD_PROG, 0, sizeof STD_PROG);
  jmp_buf top;
  WSL_NH = 0;
  WSL_HANDLERS[WSL_NH++] = &top;
  if (setjmp(top) != 0) {
    fprintf(stderr, "stdlib parse failed: %s: %s\n", WSL_ERR_CLASS, WSL_ERR_MSG);
    exit(7);
  }
  parse_program(&STD_PROG, "/internal/stdlib", 1, 27, INPUTS[0].text, INPUTS[0].len);
  WSL_NH = 0;
  STD_MARK = arena_mark();
}

/* The instance table in the order of the JS funcInstantiator's _instances Map, each as a header and its tree */
static void instance_dump(Buf *b) {
  int k = 0;
  for (int f = 0; f < FI_NF; f++) {
    for (int i = 0; i < FI_N; i++) {
      if (FI_TAB[i].func != FI_FUNCS[f]) continue;
      buf_fmt(b, "instance %d %s %d\n", k++, ast_class_name(FI_TAB[i].func), FI_TAB[i].nt);
      dump_node(b, FI_TAB[i].inst, 1);
      buf_str(b, "\n");
    }
  }
}

static long prepare_one(int i, const Input *in, Buf *b, char *status, size_t statuscap) {
  arena_reset_to(STD_MARK);
  fi_reset();
  /* a throw unwinds the recursion stacks without popping them */
  RC_N = 0;
  TDR_N = 0;
  FL_N = 0;
  jmp_buf top;
  WSL_NH = 0;
  WSL_HANDLERS[WSL_NH++] = &top;
  if (setjmp(top) != 0) {
    WSL_NH = 0;
    snprintf(status, statuscap, "err:%s: %s", WSL_ERR_CLASS, WSL_ERR_MSG);
    return -1;
  }
  CUR_ORIGIN = i == 0 ? "/internal/stdlib" : "/internal/test";
  CUR_TEXT = in->text;
  CUR_LEN = in->len;
  ORIGIN_TEXT = in->text;
  ORIGIN_LEN = in->len;
  CUR_LINEOFF = i == 0 ? 27 : 0;
  CUR_NATIVE = i == 0;
  if (i == 0) {
    prog_clone(&PREP_OUT, &STD_PROG);
  } else {
    prog_clone(&PREP_IN, &STD_PROG);
    parse_program(&PREP_IN, "/internal/test", 0, 0, in->text, in->len);
    program_removed(&PREP_OUT, &PREP_IN);
  }
  fold_program(&PREP_OUT);
  if (STAGE >= 2) {
    NameCtx *global = program_global_ctx(&PREP_OUT);
    PROG_GLOBAL_NC = global;
    names_types(&PREP_OUT, global);
    names_protocols(&PREP_OUT, global);
    if (STAGE >= 3) {
      /* resolveTypeDefsInTypes, resolveTypeDefsInProtocols, checkRecursiveTypes, then the synthesis */
      for (int k = 0; k < PREP_OUT.statements.n; k++) {
        Ast *st = PREP_OUT.statements.v[k];
        if (st->tag == A_TYPEDEF || st->tag == A_STRUCTTYPE || st->tag == A_ENUMTYPE || st->tag == A_NATIVETYPE)
          tdr_visit(st);
      }
      for (int k = 0; k < PREP_OUT.statements.n; k++) {
        Ast *st = PREP_OUT.statements.v[k];
        if (st->tag == A_PROTOCOLDECL) tdr_visit(st);
      }
      for (int k = 0; k < PREP_OUT.statements.n; k++) rc_visit(PREP_OUT.statements.v[k]);
      synth_struct_accessors(&PREP_OUT, global, PREP_OUT.statements.n);
      synth_enum_functions(&PREP_OUT, global, PREP_OUT.statements.n);
    }
    if (STAGE >= 4) names_functions(&PREP_OUT, global);
    if (STAGE >= 5) resolve_typedefs_in_functions(&PREP_OUT);
    if (STAGE >= 6) flatten_protocol_extends(&PREP_OUT);
  }
  if (INSTANTIATE_MODE || VISIT_MODE) {
    check_program(&PREP_OUT);
    post_check_passes(&PREP_OUT);
    fi_instantiate_top(&PREP_OUT);
    WSL_NH = 0;
    if (VISIT_MODE) return visit_trace(b, &PREP_OUT);
    instance_dump(b);
    long nodes = -1;   /* the row's dump has no trailing newline: its lines are the newlines but the last */
    for (size_t c = 0; c < b->n; c++) if (b->s[c] == '\n') nodes++;
    return nodes;
  }
  if (LATE_MODE) {
    check_program(&PREP_OUT);
    post_check_passes(&PREP_OUT);
    return late_trace(b, &PREP_OUT);
  }
  if (LAYOUT_MODE) {
    check_program(&PREP_OUT);
    post_check_passes(&PREP_OUT);
    layout_run(b, &PREP_OUT);
    return 0;
  }
  if (EBUF_MODE) {
    check_program(&PREP_OUT);
    post_check_passes(&PREP_OUT);
    ebuf_pass(b, &PREP_OUT);
    return 0;
  }
  if (INLINE_MODE) {
    check_program(&PREP_OUT);
    post_check_passes(&PREP_OUT);
    EBUF_COUNT = 0;
    EBUF_NALL = 0;
    inline_program(&PREP_OUT);
    inline_dump(b);
    return 0;
  }
  if (EVAL_MODE) {
    check_program(&PREP_OUT);
    post_check_passes(&PREP_OUT);
    EBUF_COUNT = 0;
    EBUF_NALL = 0;
    inline_program(&PREP_OUT);
    eval_dump(b, &PREP_OUT);
    return 0;
  }
  if (PROPS_MODE) {
    check_program(&PREP_OUT);
    for (int k = 0; k < PREP_OUT.statements.n; k++) littype_visit(PREP_OUT.statements.v[k]);
    resolve_properties(&PREP_OUT);
  }
  if (CHECK_MODE) {
    check_program(&PREP_OUT);
    if (POST_MODE) post_check_passes(&PREP_OUT);
    WSL_NH = 0;
    return 0;
  }
  WSL_NH = 0;
  if (UNIFY_MODE) {
    if (UNIFY_MODE == 2) resolve_harness(&PREP_OUT, b);
    else if (UNIFY_MODE == 3) literal_harness(&PREP_OUT, b);
    else if (UNIFY_MODE == 4) intrinsics_harness(&PREP_OUT, b);
    else if (UNIFY_MODE == 5) reasons_harness_all(&PREP_OUT, b);
    else unify_harness(&PREP_OUT, b);
    WSL_NH = 0;
    return 0;
  }
  buf_str(b, "program\n");
  for (int k = 0; k < PREP_OUT.statements.n; k++) dump_node(b, PREP_OUT.statements.v[k], 1);
  long nodes = -1;
  for (size_t c = 0; c < b->n; c++) if (b->s[c] == '\n') nodes++;
  return nodes;
}

int main(int argc, char **argv) {
  const char *root = "../jetstream";
  int dumpAt = -1;
  int benchIterations = 0;
  for (int i = 1; i < argc; i++) {
    if (strcmp(argv[i], "--stage") == 0 && i + 1 < argc) STAGE = atoi(argv[++i]);
    else if (strcmp(argv[i], "--unify") == 0) UNIFY_MODE = 1;
    else if (strcmp(argv[i], "--resolve") == 0) UNIFY_MODE = 2;
    else if (strcmp(argv[i], "--literal") == 0) UNIFY_MODE = 3;
    else if (strcmp(argv[i], "--intrinsics") == 0) UNIFY_MODE = 4;
    else if (strcmp(argv[i], "--reasons") == 0) UNIFY_MODE = 5;
    else if (strcmp(argv[i], "--clone") == 0) CLONE_FLAG = 1;
    else if (strcmp(argv[i], "--check") == 0) CHECK_MODE = 1;
    else if (strcmp(argv[i], "--post") == 0) { CHECK_MODE = 1; POST_MODE = 1; }
    else if (strcmp(argv[i], "--props") == 0) { PROPS_MODE = 1; DUMP_NO_ROLES = 1; }
    else if (strcmp(argv[i], "--instantiate") == 0) { INSTANTIATE_MODE = 1; DUMP_NO_ROLES = 1; }
    else if (strcmp(argv[i], "--visit") == 0) { VISIT_MODE = 1; DUMP_NO_ROLES = 1; }
    else if (strcmp(argv[i], "--late") == 0) { LATE_MODE = 1; DUMP_NO_ROLES = 1; }
    else if (strcmp(argv[i], "--layout") == 0) { LAYOUT_MODE = 1; DUMP_NO_ROLES = 1; }
    else if (strcmp(argv[i], "--ebuf") == 0) { EBUF_MODE = 1; DUMP_NO_ROLES = 1; }
    else if (strcmp(argv[i], "--inline") == 0) { INLINE_MODE = 1; DUMP_NO_ROLES = 1; }
    else if (strcmp(argv[i], "--eval") == 0) { EVAL_MODE = 1; DUMP_NO_ROLES = 1; }
    else if (strcmp(argv[i], "--dump") == 0 && i + 1 < argc) dumpAt = atoi(argv[++i]);
    else if (strcmp(argv[i], "--bench") == 0 && i + 1 < argc) benchIterations = atoi(argv[++i]);
    else root = argv[i];
  }
  collect_inputs(root);
  if (CHECK_MODE || LATE_MODE || LAYOUT_MODE || EBUF_MODE || INLINE_MODE || EVAL_MODE) STAGE = 6;
  REFS_ON = STAGE >= 2;
  if (STAGE >= 1) std_parse_once();
  if (benchIterations > 0) {
    bench_parse(benchIterations);
    return 0;
  }

  uint32_t all = 0x811c9dc5u;
  int ok = 0, nerr = 0;
  long totalNodes = 0;
  Buf dumpOut = {0};
  for (int i = 0; CHECK_MODE && i < NINPUTS; i++) {
    /* --check (as tools/wsl-ast-dump.cjs --check): the input line, then the trace of a checked input */
    const Input *in = &INPUTS[i];
    if (i > 0 && in->native) continue;
    char status[16400] = "ok";
    Buf b = {0};
    CHK_TRACE.n = 0;
    prepare_one(i, in, &b, status, sizeof status);
    printf("input %d %s %s\n", i, i == 0 ? "stdlib" : "doPrep", status);
    if (!POST_MODE && CHK_TRACE.n && strcmp(status, "ok") == 0) {
      CHK_TRACE.s[CHK_TRACE.n - 1] = 0;   /* the trace's last newline: console.log supplies it */
      printf("%s\n", CHK_TRACE.s);
    }
  }
  if (CHECK_MODE) return 0;
  for (int i = 0; (LATE_MODE || LAYOUT_MODE || EBUF_MODE || INLINE_MODE || EVAL_MODE) && i < NINPUTS; i++) {
    /* --late (as tools/wsl-ast-dump.cjs --late): the input line, then the LateChecker trace (kept up to an error) */
    const Input *in = &INPUTS[i];
    if (i > 0 && in->native) continue;
    char status[16400] = "ok";
    Buf b = {0};
    prepare_one(i, in, &b, status, sizeof status);
    printf("input %d %s %s\n", i, i == 0 ? "stdlib" : "doPrep", status);
    if (b.n) printf("%s\n", b.s);
    free(b.s);
  }
  if (LATE_MODE || LAYOUT_MODE || EBUF_MODE || INLINE_MODE || EVAL_MODE) return 0;
  for (int i = 0; i < NINPUTS; i++) {
    const Input *in = &INPUTS[i];
    const char *kind = i == 0 ? "stdlib" : in->native ? "doLex" : "doPrep";
    char status[16384] = "ok";
    Buf b = {0};
    long nodes = 0;
    if (STAGE == 0) arena_reset();
    if (i > 0 && in->native) {
      /* doLex: the token list (no "program" line) */
      /* --instantiate skips the doLex inputs, as tools/wsl-ast-dump.cjs does (an empty dump) */
      int count = INSTANTIATE_MODE || VISIT_MODE ? 0 : dump_lex_input(in, &b, status, sizeof status);
      nodes = count;
      if (strcmp(status, "ok") != 0) { b.n = 0; nodes = 0; }
    } else if (STAGE >= 1) {
      nodes = prepare_one(i, in, &b, status, sizeof status);
      if (nodes < 0) { b.n = 0; nodes = 0; }
    } else {
      nodes = parse_one(i, in, &b, status, sizeof status);
      if (nodes < 0) { b.n = 0; nodes = 0; }
    }
    if (UNIFY_MODE) {
      /* the harness text for each input (doLex inputs have none) */
      if (!(i > 0 && in->native)) fwrite(b.s, 1, b.n, stdout);
      free(b.s);
      continue;
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
  if (!UNIFY_MODE) printf("summary inputs %d ok %d errors %d nodes %ld hash %08x\n", NINPUTS, ok, nerr, totalNodes, all);
  if (dumpOut.s) printf("%s\n", dumpOut.s);
  return 0;
}
