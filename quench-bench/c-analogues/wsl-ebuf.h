/* A port of EBuffer.js, EPtr.js and EBufferBuilder.js: the memory cells of the interpreter. An EBuffer is a
 * fixed array of cells, numbered in creation order (eBufferCount); an EPtr is a buffer and an offset. The
 * EBufferBuilder gives every node that needs a memory location its EPtr: the parameters, variables and
 * anonymous variables (their types' default values are written into the cells), the function and call results
 * (returnEPtr, resultEPtr), and a box (a one-cell buffer) for each expression that yields a value. The visits
 * are the Visitor traversal of wsl-visit.h with VTrace.ebuf set; its overrides are in the cases there.
 * The cells hold undefined (never written), null (a reference), a number or a boolean; the JS join of a cell is
 * "" for undefined and null. The EBuffer list (EBUF_ALL) is the registry the --ebuf dump prints. */
#ifndef WSL_EBUF_H
#define WSL_EBUF_H

typedef struct { uint8_t kind; double v; void *p; } Cell;   /* kind: CELL_UNDEF, CELL_NULL, CELL_NUM, CELL_BOOL, CELL_PTR (p: EPtrV *), CELL_ARR (p: ArrRefV *) */
enum { CELL_UNDEF = 0, CELL_NULL = 1, CELL_NUM = 2, CELL_BOOL = 3, CELL_PTR = 4, CELL_ARR = 5 };
typedef struct EBuf { int index; int64_t n; Cell *cells; } EBuf;
struct EPtrV { EBuf *buf; int64_t off; };
/* EArrayRef (EArrayRef.js): ptr is NULL for a reference made from a null pointer; len is its length (a JS number) */
typedef struct { EPtrV *ptr; double len; } ArrRefV;

static int EBUF_COUNT;          /* eBufferCount */
static EBuf **EBUF_ALL;         /* every buffer made, in creation order */
static int EBUF_NALL, EBUF_ACAP;

/* The evaluator's heap (wsl-eval.h). While EB_OWN is set, the buffers, their cells and the EPtrs are made here, not in the
 * AST arena: a list of chunks that eb_reset() makes reusable after each run (the arena cannot give back what a long
 * loop allocates). Buffers made here are not in the EBuffer registry (the --eval mode does not dump them). */
typedef struct { char *base; size_t used, cap; } EbChunk;
static EbChunk *EB_CH;
static int EB_NCH, EB_CURCH;
static int EB_OWN;
static void *eb_alloc(size_t n) {
  if (!EB_OWN) return arena_alloc(n);
  n = (n + 15) & ~(size_t)15;
  if (!EB_NCH) {
    EB_NCH = 8;
    EB_CH = calloc(EB_NCH, sizeof(EbChunk));
  }
  for (;;) {
    EbChunk *c = &EB_CH[EB_CURCH];
    if (c->used + n <= c->cap) {
      void *p = c->base + c->used;
      c->used += n;
      memset(p, 0, n);
      return p;
    }
    EB_CURCH++;
    if (EB_CURCH == EB_NCH) {
      int cap = EB_NCH * 2;
      EbChunk *nv = calloc(cap, sizeof(EbChunk));
      memcpy(nv, EB_CH, sizeof(EbChunk) * EB_NCH);
      free(EB_CH);
      EB_CH = nv;
      EB_NCH = cap;
    }
    c = &EB_CH[EB_CURCH];
    if (c->cap < n) {
      free(c->base);
      size_t cap = n > ((size_t)64 << 20) ? n : ((size_t)64 << 20);
      c->base = malloc(cap);
      if (!c->base) { fprintf(stderr, "out of memory\n"); exit(4); }
      c->cap = cap;
    }
    c->used = 0;
  }
}
static void eb_reset(void) { EB_CURCH = 0; if (EB_NCH) EB_CH[0].used = 0; }

static EBuf *ebuf_new(int64_t n) {
  EBuf *b = eb_alloc(sizeof(EBuf));
  b->index = EBUF_COUNT++;
  b->n = n;
  b->cells = eb_alloc(sizeof(Cell) * (size_t)(n ? n : 1));
  if (EB_OWN) return b;
  memset(b->cells, 0, sizeof(Cell) * (size_t)(n ? n : 1));
  if (EBUF_NALL == EBUF_ACAP) {
    int cap = EBUF_ACAP ? EBUF_ACAP * 2 : 256;
    EBuf **nv = malloc(sizeof(EBuf *) * cap);
    if (EBUF_NALL) memcpy(nv, EBUF_ALL, sizeof(EBuf *) * EBUF_NALL);
    free(EBUF_ALL);
    EBUF_ALL = nv;
    EBUF_ACAP = cap;
  }
  EBUF_ALL[EBUF_NALL++] = b;
  return b;
}

static void eb_fatal(const char *what) {
  fprintf(stderr, "unsupported in the EBuffer port: %s\n", what);
  exit(9);
}

static void ebuf_set(EBuf *b, int64_t i, Cell v) {
  if (i < 0 || i >= b->n) eb_fatal("out of bounds buffer access");
  b->cells[i] = v;
}

static EPtrV *eptr_new(EBuf *b, int64_t off) {
  EPtrV *p = eb_alloc(sizeof(EPtrV));
  p->buf = b;
  p->off = off;
  return p;
}

/* EPtr.box(): a one-cell buffer, whose cell is undefined */
static EPtrV *eptr_box(void) { return eptr_new(ebuf_new(1), 0); }

/* "B<index>:<offset>", EPtr.toString */
static const char *eptr_str(const EPtrV *p) {
  char tmp[64];
  snprintf(tmp, sizeof tmp, "B%d:%lld", p->buf->index, (long long)p->off);
  return cat(1, tmp);
}

/* EArrayRef.toString: "A:<" + ptr + ", " + length + ">" (a null ptr joins as "") */
static const char *arrref_str(const ArrRefV *r) {
  const char *ptr = r->ptr ? eptr_str(r->ptr) : "null";
  return cat(5, "A:<", ptr, ", ", js_num(r->len), ">");
}

/* Buffer.toString: "B<index>:[" + the cells joined by "," + "]" */
static const char *ebuf_str(const EBuf *b) {
  Buf s = {0};
  buf_fmt(&s, "B%d:[", b->index);
  for (int64_t i = 0; i < b->n; i++) {
    if (i) buf_str(&s, ",");
    const Cell *c = &b->cells[i];
    if (c->kind == CELL_NUM) buf_str(&s, js_num(c->v));
    else if (c->kind == CELL_BOOL) buf_str(&s, c->v != 0 ? "true" : "false");
    else if (c->kind == CELL_PTR) buf_str(&s, eptr_str(c->p));
    else if (c->kind == CELL_ARR) buf_str(&s, arrref_str(c->p));
  }
  buf_str(&s, "]");
  return s.s;
}

/* type.populateDefaultValue(buffer, offset) (the Intrinsics, StructType, EnumType, ArrayType, TypeRef and
 * ReferenceType methods) */
static void populate_default(const Ast *t, EBuf *b, int64_t off) {
  Cell zero = {CELL_NUM, 0}, fls = {CELL_BOOL, 0}, nul = {CELL_NULL, 0};
  switch (t->tag) {
  case A_TYPEREF:
    if (t->u.typeRef.typeArguments.n) chk_error("Cannot get default value of a type instantiation");
    populate_default(t->u.typeRef.type, b, off);
    return;
  case A_NATIVETYPE: {
    NtKind k = nt_kind(t);
    if (k == NT_VOID) return;
    if (k == NT_BOOL) { ebuf_set(b, off, fls); return; }
    if (k == NT_NONE) eb_fatal("populateDefaultValue of a native type without intrinsics");
    ebuf_set(b, off, zero);
    return;
  }
  case A_STRUCTTYPE:
    if (!t->hasSize) eb_fatal("populateDefaultValue of a struct without layout");
    for (int i = 0; i < t->u.structType.fields.n; i++) {
      const Ast *f = t->u.structType.fields.v[i];
      populate_default(f->u.named.type, b, off + f->offset);
    }
    return;
  case A_ENUMTYPE:
    populate_default(t->u.enumType.baseType, b, off);
    return;
  case A_ARRAYTYPE: {
    int64_t cnt = arr_count(t);
    for (int64_t i = 0; i < cnt; i++) {
      int ok;
      int64_t es = ty_size(t->u.arrayType.elementType, &ok);
      if (!ok) eb_fatal("array element without size");
      populate_default(t->u.arrayType.elementType, b, off + i * es);
    }
    return;
  }
  case A_PTRTYPE:
  case A_ARRAYREFTYPE:
    ebuf_set(b, off, nul);
    return;
  default:
    eb_fatal("populateDefaultValue of a type without one");
  }
}

/* EBufferBuilder._createEPtr(type) */
static EPtrV *eb_create(const Ast *type) {
  int ok;
  int64_t sz = ty_size(type, &ok);
  if (!ok) chk_error(cat(2, "Type does not have size: ", js_tostr(type)));
  EBuf *b = ebuf_new(sz);
  populate_default(type, b, 0);
  return eptr_new(b, 0);
}

#endif
