// jetstream-row: lazy-collections
// jetstream-files: generators/lazy-collections.js
// iterations: 120
/* C analogue of JetStream's generators/lazy-collections.js: the same nine lazy pipelines over
 * the same data (ranges, factorials as bignums, Fibonacci, Mersenne and prime search, powers,
 * prime factors, FizzBuzz, and a binary tree walk), with the same operators (filter, map,
 * flatMap, flatten, chunk, windows, take, takeWhile, unique) and the same total of 635.
 * Each stage is an iterator struct whose next() is a function pointer, pulled by its
 * consumer, so nothing runs until toArray() drives the pipeline, as in the JS generators.
 * Pipeline temporaries (iterators, arrays, bignum limbs, unique sets) come from one arena
 * reset per iteration, the C counterpart of the JS garbage they produce. */
#define _POSIX_C_SOURCE 200809L
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define MAX_SAFE_INTEGER 9007199254740991.0
#define MIN_SAFE_INTEGER (-9007199254740991.0)
#define ARENA_BYTES (16u * 1024u * 1024u)
#define UNIQUE_SLOTS 512
#define MAX_FLATTEN_DEPTH 8

enum { K_NONE, K_NUM, K_STR, K_BIG, K_ARR, K_ITER };

typedef struct Arr Arr;
typedef struct Iter Iter;

typedef struct Value {
  int kind;
  double num;
  const char *str;
  uint32_t *big;
  uint32_t bigLen;
  Arr *arr;
  Iter *iter;
} Value;

struct Arr {
  Value *items;
  long len, cap;
};

typedef int (*NextFn)(Iter *self, Value *out);
struct Iter {
  NextFn next;
};

typedef int (*PredFn)(void *ctx, Value v, long index);
typedef Value (*MapFn)(void *ctx, Value v, long index);

/* ---- arena ---- */
static unsigned char *arena_base;
static size_t arena_used;

static void *arena_alloc(size_t n) {
  n = (n + 15u) & ~(size_t)15u;
  if (arena_used + n > ARENA_BYTES) {
    fprintf(stderr, "lazy-collections: arena exhausted\n");
    exit(1);
  }
  void *p = arena_base + arena_used;
  arena_used += n;
  memset(p, 0, n);
  return p;
}

static Value vnum(double d) { Value v; memset(&v, 0, sizeof v); v.kind = K_NUM; v.num = d; return v; }
static Value vstr(const char *s) { Value v; memset(&v, 0, sizeof v); v.kind = K_STR; v.str = s; return v; }
static Value viter(Iter *it) { Value v; memset(&v, 0, sizeof v); v.kind = K_ITER; v.iter = it; return v; }
static Value varr(Arr *a) { Value v; memset(&v, 0, sizeof v); v.kind = K_ARR; v.arr = a; return v; }

static Arr *arr_new(long cap) {
  Arr *a = arena_alloc(sizeof *a);
  a->cap = cap > 0 ? cap : 4;
  a->items = arena_alloc(sizeof(Value) * (size_t)a->cap);
  a->len = 0;
  return a;
}

static void arr_push(Arr *a, Value v) {
  if (a->len == a->cap) {
    long ncap = a->cap * 2;
    Value *ni = arena_alloc(sizeof(Value) * (size_t)ncap);
    memcpy(ni, a->items, sizeof(Value) * (size_t)a->len);
    a->items = ni;
    a->cap = ncap;
  }
  a->items[a->len++] = v;
}

/* ---- JS-style helpers ---- */
static double clamp_safe(double x) {
  double hi = x < MAX_SAFE_INTEGER ? x : MAX_SAFE_INTEGER;
  return hi > MIN_SAFE_INTEGER ? hi : MIN_SAFE_INTEGER;
}

static int truthy(double d) { return d != 0.0 && !isnan(d); }

/* ---- range(lowerbound, upperbound, step) ---- */
typedef struct {
  Iter base;
  double lb, ub, step, i;
  int started, ascending;
} RangeIter;

static int range_next(Iter *self, Value *out) {
  RangeIter *r = (RangeIter *)self;
  if (!r->started) {
    r->started = 1;
    r->i = r->lb;
  } else {
    r->i += r->step;
  }
  int more = r->ascending ? (r->i <= r->ub) : (r->i >= r->ub);
  if (!more) return 0;
  *out = vnum(r->i);
  return 1;
}

static Iter *range_new(double lowerbound, double upperbound, double step) {
  RangeIter *r = arena_alloc(sizeof *r);
  double fixedStep;
  if (lowerbound < upperbound) fixedStep = clamp_safe(step > 0 ? step : -step);
  else fixedStep = clamp_safe(step > 0 ? -step : step);
  r->lb = clamp_safe(lowerbound);
  r->ub = clamp_safe(upperbound);
  r->step = fixedStep;
  r->ascending = r->lb < r->ub;
  r->base.next = range_next;
  return &r->base;
}

/* ---- array iterator (used by flatten to open arrays) ---- */
typedef struct {
  Iter base;
  Arr *arr;
  long pos;
} ArrayIter;

static int array_next(Iter *self, Value *out) {
  ArrayIter *a = (ArrayIter *)self;
  if (a->pos >= a->arr->len) return 0;
  *out = a->arr->items[a->pos++];
  return 1;
}

static Iter *array_iter_new(Arr *arr) {
  ArrayIter *a = arena_alloc(sizeof *a);
  a->arr = arr;
  a->pos = 0;
  a->base.next = array_next;
  return &a->base;
}

/* ---- map ---- */
typedef struct {
  Iter base;
  Iter *src;
  MapFn fn;
  void *ctx;
  long index;
} MapIter;

static int map_next(Iter *self, Value *out) {
  MapIter *m = (MapIter *)self;
  Value v;
  if (!m->src->next(m->src, &v)) return 0;
  *out = m->fn(m->ctx, v, m->index++);
  return 1;
}

static Iter *map_new(Iter *src, MapFn fn, void *ctx) {
  MapIter *m = arena_alloc(sizeof *m);
  m->src = src;
  m->fn = fn;
  m->ctx = ctx;
  m->index = 0;
  m->base.next = map_next;
  return &m->base;
}

/* ---- filter ---- */
typedef struct {
  Iter base;
  Iter *src;
  PredFn fn;
  void *ctx;
  long index;
} FilterIter;

static int filter_next(Iter *self, Value *out) {
  FilterIter *f = (FilterIter *)self;
  Value v;
  while (f->src->next(f->src, &v)) {
    if (f->fn(f->ctx, v, f->index++)) {
      *out = v;
      return 1;
    }
  }
  return 0;
}

static Iter *filter_new(Iter *src, PredFn fn, void *ctx) {
  FilterIter *f = arena_alloc(sizeof *f);
  f->src = src;
  f->fn = fn;
  f->ctx = ctx;
  f->index = 0;
  f->base.next = filter_next;
  return &f->base;
}

/* ---- flatMap: an iterable result is spliced in, anything else is yielded as is ---- */
typedef struct {
  Iter base;
  Iter *src;
  MapFn fn;
  void *ctx;
  long index;
  Iter *inner;
} FlatMapIter;

static int flatmap_next(Iter *self, Value *out) {
  FlatMapIter *f = (FlatMapIter *)self;
  for (;;) {
    if (f->inner) {
      if (f->inner->next(f->inner, out)) return 1;
      f->inner = NULL;
    }
    Value v;
    if (!f->src->next(f->src, &v)) return 0;
    Value r = f->fn(f->ctx, v, f->index++);
    if (r.kind == K_ITER) {
      f->inner = r.iter;
    } else {
      *out = r;
      return 1;
    }
  }
}

static Iter *flatmap_new(Iter *src, MapFn fn, void *ctx) {
  FlatMapIter *f = arena_alloc(sizeof *f);
  f->src = src;
  f->fn = fn;
  f->ctx = ctx;
  f->index = 0;
  f->inner = NULL;
  f->base.next = flatmap_next;
  return &f->base;
}

/* ---- flatten (deep): arrays and iterators are opened recursively, scalars pass through ---- */
typedef struct {
  Iter base;
  Iter *src;
  Iter *stack[MAX_FLATTEN_DEPTH];
  int depth;
} FlattenIter;

static int flatten_push(FlattenIter *f, Value v) {
  if (v.kind == K_ARR) {
    if (f->depth >= MAX_FLATTEN_DEPTH) return 0;
    f->stack[f->depth++] = array_iter_new(v.arr);
    return 1;
  }
  if (v.kind == K_ITER) {
    if (f->depth >= MAX_FLATTEN_DEPTH) return 0;
    f->stack[f->depth++] = v.iter;
    return 1;
  }
  return 0;
}

static int flatten_next(Iter *self, Value *out) {
  FlattenIter *f = (FlattenIter *)self;
  for (;;) {
    Value v;
    if (f->depth > 0) {
      if (f->stack[f->depth - 1]->next(f->stack[f->depth - 1], &v)) {
        if (!flatten_push(f, v)) {
          *out = v;
          return 1;
        }
        continue;
      }
      f->depth--;
      continue;
    }
    if (!f->src->next(f->src, &v)) return 0;
    if (!flatten_push(f, v)) {
      *out = v;
      return 1;
    }
  }
}

static Iter *flatten_new(Iter *src) {
  FlattenIter *f = arena_alloc(sizeof *f);
  f->src = src;
  f->depth = 0;
  f->base.next = flatten_next;
  return &f->base;
}

/* ---- take(n): yields n items and never pulls past them ---- */
typedef struct {
  Iter base;
  Iter *src;
  long remaining;
} TakeIter;

static int take_next(Iter *self, Value *out) {
  TakeIter *s = (TakeIter *)self;
  if (s->remaining <= 0) return 0;
  Value v;
  if (!s->src->next(s->src, &v)) return 0;
  s->remaining--;
  *out = v;
  return 1;
}

static Iter *take_new(Iter *src, long count) {
  TakeIter *s = arena_alloc(sizeof *s);
  s->src = src;
  s->remaining = count;
  s->base.next = take_next;
  return &s->base;
}

/* ---- takeWhile ---- */
typedef struct {
  Iter base;
  Iter *src;
  PredFn fn;
  void *ctx;
  long index;
  int done;
} TakeWhileIter;

static int takewhile_next(Iter *self, Value *out) {
  TakeWhileIter *t = (TakeWhileIter *)self;
  if (t->done) return 0;
  Value v;
  if (!t->src->next(t->src, &v) || !t->fn(t->ctx, v, t->index++)) {
    t->done = 1;
    return 0;
  }
  *out = v;
  return 1;
}

static Iter *takewhile_new(Iter *src, PredFn fn, void *ctx) {
  TakeWhileIter *t = arena_alloc(sizeof *t);
  t->src = src;
  t->fn = fn;
  t->ctx = ctx;
  t->index = 0;
  t->done = 0;
  t->base.next = takewhile_next;
  return &t->base;
}

/* ---- chunk(size): arrays of `size` items, plus a final partial one ---- */
typedef struct {
  Iter base;
  Iter *src;
  long size;
  Arr *cur;
  int done;
} ChunkIter;

static int chunk_next(Iter *self, Value *out) {
  ChunkIter *c = (ChunkIter *)self;
  if (c->done) return 0;
  for (;;) {
    Value v;
    if (!c->src->next(c->src, &v)) {
      c->done = 1;
      if (c->cur->len > 0) {
        *out = varr(c->cur);
        return 1;
      }
      return 0;
    }
    arr_push(c->cur, v);
    if (c->cur->len == c->size) {
      *out = varr(c->cur);
      c->cur = arr_new(c->size);
      return 1;
    }
  }
}

static Iter *chunk_new(Iter *src, long size) {
  ChunkIter *c = arena_alloc(sizeof *c);
  c->src = src;
  c->size = size;
  c->cur = arr_new(size);
  c->done = 0;
  c->base.next = chunk_next;
  return &c->base;
}

/* ---- windows(size): every run of `size` consecutive items, as a fresh array ---- */
typedef struct {
  Iter base;
  Iter *src;
  long size;
  Arr *buf;
} WindowsIter;

static int windows_next(Iter *self, Value *out) {
  WindowsIter *w = (WindowsIter *)self;
  Value v;
  while (w->src->next(w->src, &v)) {
    arr_push(w->buf, v);
    if (w->buf->len == w->size) {
      Arr *copy = arr_new(w->size);
      for (long k = 0; k < w->size; k++) arr_push(copy, w->buf->items[k]);
      /* result.shift() */
      memmove(w->buf->items, w->buf->items + 1, sizeof(Value) * (size_t)(w->buf->len - 1));
      w->buf->len--;
      *out = varr(copy);
      return 1;
    }
  }
  return 0;
}

static Iter *windows_new(Iter *src, long size) {
  WindowsIter *w = arena_alloc(sizeof *w);
  w->src = src;
  w->size = size;
  w->buf = arr_new(size + 1);
  w->base.next = windows_next;
  return &w->base;
}

/* ---- unique: first occurrence only, SameValueZero equality (like a Set) ---- */
typedef struct {
  Value slots[UNIQUE_SLOTS];
  int count;
} ValueSet;

static uint64_t value_hash(Value v) {
  if (v.kind == K_STR) {
    uint64_t h = 1469598103934665603ull;
    for (const unsigned char *p = (const unsigned char *)v.str; *p; p++) {
      h ^= *p;
      h *= 1099511628211ull;
    }
    return h;
  }
  double d = v.num;
  if (isnan(d)) return 0x7ff8dead0000ull;
  if (d == 0.0) d = 0.0;
  uint64_t bits;
  memcpy(&bits, &d, sizeof bits);
  bits ^= bits >> 33;
  bits *= 0xff51afd7ed558ccdull;
  bits ^= bits >> 33;
  return bits;
}

static int value_same(Value a, Value b) {
  if (a.kind != b.kind) return 0;
  if (a.kind == K_STR) return strcmp(a.str, b.str) == 0;
  if (isnan(a.num) && isnan(b.num)) return 1;
  return a.num == b.num;
}

/* Returns 1 when v was not in the set before. */
static int set_insert(ValueSet *s, Value v) {
  uint64_t h = value_hash(v);
  size_t idx = (size_t)(h % UNIQUE_SLOTS);
  for (int probe = 0; probe < UNIQUE_SLOTS; probe++) {
    Value *slot = &s->slots[(idx + (size_t)probe) % UNIQUE_SLOTS];
    if (slot->kind == K_NONE) {
      if (s->count + 1 > (UNIQUE_SLOTS * 3) / 4) {
        fprintf(stderr, "lazy-collections: unique set overfull\n");
        exit(1);
      }
      *slot = v;
      s->count++;
      return 1;
    }
    if (value_same(*slot, v)) return 0;
  }
  fprintf(stderr, "lazy-collections: unique set full\n");
  exit(1);
}

typedef struct {
  Iter base;
  Iter *src;
  ValueSet *seen;
} UniqueIter;

static int unique_next(Iter *self, Value *out) {
  UniqueIter *u = (UniqueIter *)self;
  Value v;
  while (u->src->next(u->src, &v)) {
    if (set_insert(u->seen, v)) {
      *out = v;
      return 1;
    }
  }
  return 0;
}

static Iter *unique_new(Iter *src) {
  UniqueIter *u = arena_alloc(sizeof *u);
  u->src = src;
  u->seen = arena_alloc(sizeof *u->seen);
  u->base.next = unique_next;
  return &u->base;
}

/* ---- fibonacci(): 0, 1, 1, 2, ... ---- */
typedef struct {
  Iter base;
  double a, b;
} FibIter;

static int fib_next(Iter *self, Value *out) {
  FibIter *f = (FibIter *)self;
  *out = vnum(f->a);
  double temp = f->a;
  f->a = f->b;
  f->b += temp;
  return 1;
}

static Iter *fibonacci_new(void) {
  FibIter *f = arena_alloc(sizeof *f);
  f->a = 0;
  f->b = 1;
  f->base.next = fib_next;
  return &f->base;
}

/* ---- factorial(): BigInt b *= a++ yielding 1!, 2!, 3!, ... ---- */
typedef struct {
  Iter base;
  uint64_t a;
  uint32_t *b;
  uint32_t bLen;
} FactIter;

static int fact_next(Iter *self, Value *out) {
  FactIter *f = (FactIter *)self;
  uint32_t *nb = arena_alloc(sizeof(uint32_t) * (size_t)(f->bLen + 1));
  uint64_t carry = 0;
  for (uint32_t i = 0; i < f->bLen; i++) {
    uint64_t t = (uint64_t)f->b[i] * f->a + carry;
    nb[i] = (uint32_t)t;
    carry = t >> 32;
  }
  uint32_t len = f->bLen;
  if (carry) nb[len++] = (uint32_t)carry;
  f->a++;
  f->b = nb;
  f->bLen = len;
  Value v;
  memset(&v, 0, sizeof v);
  v.kind = K_BIG;
  v.big = nb;
  v.bigLen = len;
  *out = v;
  return 1;
}

static Iter *factorial_new(void) {
  FactIter *f = arena_alloc(sizeof *f);
  f->a = 1;
  f->b = arena_alloc(sizeof(uint32_t));
  f->b[0] = 1;
  f->bLen = 1;
  f->base.next = fact_next;
  return &f->base;
}

/* ---- binary tree iterator: pre-order, value then left then right ---- */
typedef struct BinaryTree {
  double value;
  struct BinaryTree *left, *right;
} BinaryTree;

typedef struct {
  Iter base;
  const BinaryTree *stack[64];
  int top;
} TreeIter;

static int tree_next(Iter *self, Value *out) {
  TreeIter *t = (TreeIter *)self;
  if (t->top == 0) return 0;
  const BinaryTree *node = t->stack[--t->top];
  if (node->right) t->stack[t->top++] = node->right;
  if (node->left) t->stack[t->top++] = node->left;
  *out = vnum(node->value);
  return 1;
}

static Iter *tree_iter_new(const BinaryTree *root) {
  TreeIter *t = arena_alloc(sizeof *t);
  t->top = 0;
  t->stack[t->top++] = root;
  t->base.next = tree_next;
  return &t->base;
}

static BinaryTree *tree_node(double value, BinaryTree *left, BinaryTree *right) {
  BinaryTree *n = calloc(1, sizeof *n);
  n->value = value;
  n->left = left;
  n->right = right;
  return n;
}

/* binaryRoot from the JS: built once at load time, not per iteration. */
static BinaryTree *build_binary_root(void) {
  BinaryTree *n25 = tree_node(25, NULL, NULL);
  BinaryTree *n3 = tree_node(3, n25, NULL);
  BinaryTree *n7 = tree_node(7, n3, NULL);
  BinaryTree *n10 = tree_node(10, NULL, NULL);
  BinaryTree *n6 = tree_node(6, n10, NULL);
  BinaryTree *n4 = tree_node(4, n6, n7);
  BinaryTree *n8 = tree_node(8, NULL, NULL);
  BinaryTree *n24 = tree_node(24, NULL, NULL);
  BinaryTree *n11 = tree_node(11, n24, NULL);
  BinaryTree *n9 = tree_node(9, n11, NULL);
  BinaryTree *n5 = tree_node(5, n8, n9);
  BinaryTree *n2 = tree_node(2, n4, n5);
  BinaryTree *n1 = tree_node(1, n2, NULL);
  BinaryTree *n23 = tree_node(23, NULL, NULL);
  BinaryTree *n22 = tree_node(22, n23, NULL);
  BinaryTree *n14 = tree_node(14, n22, NULL);
  BinaryTree *n15 = tree_node(15, n14, NULL);
  BinaryTree *n21 = tree_node(21, NULL, NULL);
  BinaryTree *n20 = tree_node(20, n15, n21);
  BinaryTree *n16 = tree_node(16, NULL, NULL);
  BinaryTree *n17 = tree_node(17, NULL, NULL);
  BinaryTree *n18 = tree_node(18, n17, NULL);
  BinaryTree *n19 = tree_node(19, n16, n18);
  BinaryTree *n13 = tree_node(13, n20, n19);
  BinaryTree *n12 = tree_node(12, n13, NULL);
  return tree_node(0, n1, n12);
}

/* ---- predicates and mappers ---- */
/* filter(x => !(n % x)) with n in ctx: a zero remainder, or NaN for x == 0, passes. */
/* JS a % b. Engines take an integer remainder when both operands are int32 values (and the
 * divisor is not 0), as a C program over integers does; anything else is fmod. */
static inline double js_mod(double a, double b) {
  int32_t ia = (int32_t)a, ib = (int32_t)b;
  if ((double)ia == a && (double)ib == b && ib > 0 && ia >= 0) return (double)(ia % ib);
  return fmod(a, b);
}

static int pred_divides(void *ctx, Value v, long index) {
  (void)index;
  double r = js_mod(*(double *)ctx, v.num);
  return r == 0.0 || isnan(r);
}

static int pred_mod_zero(void *ctx, Value v, long index) {
  (void)index;
  return js_mod(v.num, *(double *)ctx) == 0.0;
}

static int pred_less_than(void *ctx, Value v, long index) {
  (void)index;
  return v.num < *(double *)ctx;
}

/* isPrime(n) = every(x => n % x)(range(2, n - 1)), with every's truthiness test. */
static int is_prime(double n) {
  Iter *r = range_new(2, n - 1, 1);
  Value x;
  while (r->next(r, &x)) {
    if (!truthy(js_mod(n, x.num))) return 0;
  }
  return 1;
}

static int pred_is_prime(void *ctx, Value v, long index) {
  (void)ctx;
  (void)index;
  return is_prime(v.num);
}

static Value map_mersenne(void *ctx, Value v, long index) {
  (void)ctx;
  (void)index;
  return vnum(pow(2.0, v.num + 1) - 1);
}

static Value map_pow2(void *ctx, Value v, long index) {
  (void)ctx;
  (void)index;
  return vnum(pow(v.num, 2.0));
}

/* x * 2 with x an array: JS coerces the array to "a,b,c" and the product is NaN. */
static Value map_times_two(void *ctx, Value v, long index) {
  (void)ctx;
  (void)index;
  if (v.kind == K_NUM) return vnum(v.num * 2);
  return vnum(NAN);
}

static Value map_factors(void *ctx, Value v, long index) {
  (void)ctx;
  (void)index;
  double *n = arena_alloc(sizeof *n);
  *n = v.num;
  /* factors(n) = filter(x => !(n % x))(range(1, n)) */
  return viter(filter_new(range_new(1, *n, 1), pred_divides, n));
}

static const char *fizzbuzz_strings[3] = {"fizz", "buzz", "fizzbuzz"};

static Value map_fizzbuzz(void *ctx, Value v, long index) {
  (void)ctx;
  (void)index;
  double x = v.num;
  if (js_mod(x, 15.0) == 0.0) return vstr(fizzbuzz_strings[2]);
  if (js_mod(x, 5.0) == 0.0) return vstr(fizzbuzz_strings[1]);
  if (js_mod(x, 3.0) == 0.0) return vstr(fizzbuzz_strings[0]);
  return v;
}

static const char *letter_table[26] = {
  "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m",
  "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z"};

/* String.fromCharCode("a".charCodeAt() + x) for the divisor values in this workload (0..25). */
static Value map_char(void *ctx, Value v, long index) {
  (void)ctx;
  (void)index;
  int k = (int)v.num;
  if (k < 0 || k > 25) {
    fprintf(stderr, "lazy-collections: character out of range\n");
    exit(1);
  }
  return vstr(letter_table[k]);
}

/* ---- consumer ---- */
static Arr *to_array(Iter *it) {
  Arr *a = arr_new(16);
  Value v;
  while (it->next(it, &v)) arr_push(a, v);
  return a;
}

/* ---- the nine pipelines of runIteration() ---- */
static double M100 = 100, M4 = 4, M400 = 400, LIMIT_1000 = 1000;
static BinaryTree *root_tree;

static long run_iteration(void) {
  long total = 0;

  /* example */
  {
    Iter *p = range_new(0, 10000000, 1);
    p = filter_new(p, pred_mod_zero, &M100);
    p = filter_new(p, pred_mod_zero, &M4);
    p = filter_new(p, pred_mod_zero, &M400);
    p = takewhile_new(p, pred_less_than, &LIMIT_1000);
    total += to_array(p)->len;
  }

  /* factorials */
  {
    Iter *p = factorial_new();
    p = chunk_new(p, 3);
    p = flatten_new(p);
    p = take_new(p, 100);
    total += to_array(p)->len;
  }

  /* doubleFibonaccis */
  {
    Iter *p = fibonacci_new();
    p = take_new(p, 78);
    p = windows_new(p, 3);
    p = flatmap_new(p, map_times_two, NULL);
    p = unique_new(p);
    total += to_array(p)->len;
  }

  /* mersennePrimes */
  {
    Iter *p = range_new(1, MAX_SAFE_INTEGER, 1);
    p = map_new(p, map_mersenne, NULL);
    p = filter_new(p, pred_is_prime, NULL);
    p = take_new(p, 5);
    total += to_array(p)->len;
  }

  /* primes */
  {
    Iter *p = range_new(1, MAX_SAFE_INTEGER, 1);
    p = filter_new(p, pred_is_prime, NULL);
    p = take_new(p, 150);
    total += to_array(p)->len;
  }

  /* powersOfTwo */
  {
    Iter *p = range_new(1, MAX_SAFE_INTEGER, 1);
    p = map_new(p, map_pow2, NULL);
    p = chunk_new(p, 2);
    p = flatten_new(p);
    p = take_new(p, 100);
    total += to_array(p)->len;
  }

  /* primeFactors */
  {
    Iter *p = range_new(1, MAX_SAFE_INTEGER, 1);
    p = flatmap_new(p, map_factors, NULL);
    p = filter_new(p, pred_is_prime, NULL);
    p = unique_new(p);
    p = take_new(p, 150);
    total += to_array(p)->len;
  }

  /* fizzbuzz */
  {
    Iter *p = range_new(1, MAX_SAFE_INTEGER, 1);
    p = map_new(p, map_fizzbuzz, NULL);
    p = take_new(p, 100);
    total += to_array(p)->len;
  }

  /* binaryTree */
  {
    Iter *p = tree_iter_new(root_tree);
    p = flatmap_new(p, map_factors, NULL);
    p = windows_new(p, 4);
    p = flatten_new(p);
    p = map_new(p, map_char, NULL);
    p = unique_new(p);
    total += to_array(p)->len;
  }

  return total;
}

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : 120;
  struct timespec a, b;
  long totalLength = 0;
  arena_base = malloc(ARENA_BYTES);
  if (!arena_base) return 1;
  root_tree = build_binary_root();

  clock_gettime(CLOCK_MONOTONIC, &a);
  for (int i = 0; i < iterations; i++) {
    arena_used = 0;
    totalLength = run_iteration();
    if (totalLength != 635) {
      fprintf(stderr, "this.totalLength of %ld is invalid!\n", totalLength);
      return 1;
    }
  }
  clock_gettime(CLOCK_MONOTONIC, &b);
  printf("lazy-collections %d iterations %.3f ms\n", iterations, (b.tv_sec - a.tv_sec) * 1e3 + (b.tv_nsec - a.tv_nsec) / 1e6);
  printf("totalLength %ld\n", totalLength);
  return 0;
}
