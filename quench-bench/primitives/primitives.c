/* Language-primitive scoreboard, C baseline for primitives.cjs.
 * Same primitives, order, iteration counts and checksums; prints
 * `PRIM <name> <ms> <checksum>`.  Build: cc -O2 -Wall -o prims primitives.c -lm
 * All JS int32 arithmetic is done in uint32_t (identical bits mod 2^32). */
#include <math.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define NOINLINE __attribute__((noinline))

static volatile int32_t seed_source = 12345;
static int32_t SEED;              /* read once from a volatile: not a compile-time constant */
static void *volatile alloc_sink; /* allocations escape here so malloc/free are not elided */

static uint32_t u32d(double d) { return (uint32_t)(int64_t)floor(d); }
static uint32_t lcg(uint32_t x) { return x * 1664525u + 1013904223u; }

static void *xmalloc(size_t n) {
    void *p = malloc(n);
    if (!p) { fputs("out of memory\n", stderr); exit(1); }
    return p;
}

/* ---- immutable setup tables (built untimed) ---- */
static const char *const PIECES[8] = {"ab", "cde", "f", "ghij", "kl", "mnopq", "rs", "t"};
static size_t PIECE_LEN[8];
static const char *const WORDS[16] = {"alpha", "alphabet", "alpine", "beta", "betamax", "bet",
    "gamma", "gam", "delta", "deltas", "epsilon", "eps", "zeta", "zed", "eta", "etas"};
static const char *const OPS[8] = {"add", "sub", "mul", "xor", "shl", "neg", "nop", "mov"};
static const char *const HEXES[16] = {"deadbeef", "cafe01", "0123456789abcdef", "xyz", "abcdefg",
    "12ab34cd", "", "ffff", "a1b2c3d4e5", "DEADBEEF", "00ff00ff00", "beefcafe9", "f00d", "hello",
    "7", "abc-def"};
#define LONG_LEN 65536
static unsigned char LONG_STR[LONG_LEN];
static char SKEYS[512][8];
static char DKEYS[64][4];

static void setup_tables(void) {
    uint32_t x = (uint32_t)SEED;
    for (int j = 0; j < LONG_LEN; j++) {
        x = lcg(x);
        LONG_STR[j] = (unsigned char)(97 + ((x >> 16) % 26));
    }
    for (int j = 0; j < 8; j++) PIECE_LEN[j] = strlen(PIECES[j]);
    for (int j = 0; j < 512; j++) snprintf(SKEYS[j], sizeof SKEYS[j], "key%d", j);
    for (int j = 0; j < 64; j++) snprintf(DKEYS[j], sizeof DKEYS[j], "p%d", j);
}

/* ---- primitives ---- */
static uint32_t int_loop(int32_t n) {
    uint32_t x = (uint32_t)SEED;
    for (int32_t i = 0; i < n; i++) {
        x = x * 1664525u + 1013904223u + (uint32_t)i;
        x ^= x >> 7;
    }
    return x;
}

static uint32_t float_loop(int32_t n) {
    double x = 0.5 + (SEED % 1000) * 1e-4;
    double s = 0;
    for (int32_t i = 0; i < n; i++) {
        x = 3.9 * x * (1 - x);
        s += x;
    }
    return u32d(s * 1000);
}

NOINLINE static uint32_t fib(int32_t k) { return k < 2 ? (uint32_t)k : fib(k - 1) + fib(k - 2); }
static uint32_t recursion_fib(int32_t n) { return fib(n + (SEED & 3)); }

NOINLINE static uint32_t step_direct(uint32_t x, uint32_t i) { return (x ^ i) * 31u + 7u; }
static uint32_t call_direct(int32_t n) {
    uint32_t x = (uint32_t)SEED;
    for (int32_t i = 0; i < n; i++) x = step_direct(x, (uint32_t)i);
    return x;
}

typedef struct { uint32_t k; } StepEnv;
typedef struct { uint32_t (*fn)(const StepEnv *, uint32_t, uint32_t); const StepEnv *env; } StepClosure;
NOINLINE static uint32_t stepper_body(const StepEnv *e, uint32_t x, uint32_t i) { return (x ^ i) * e->k + 1u; }
static uint32_t call_closure(int32_t n) {
    StepEnv env = {(uint32_t)(SEED & 15) + 3u};
    StepClosure f = {stepper_body, &env};
    uint32_t x = (uint32_t)SEED;
    for (int32_t i = 0; i < n; i++) x = f.fn(f.env, x, (uint32_t)i);
    return x;
}

typedef struct { uint32_t k, c; } Acc;
NOINLINE static uint32_t acc_step(const Acc *self, uint32_t x) { return (x ^ self->k) * 31u + self->c; }
static uint32_t call_method(int32_t n) {
    Acc o = {(uint32_t)(SEED & 255), 7u};
    uint32_t x = (uint32_t)SEED;
    for (int32_t i = 0; i < n; i++) x = acc_step(&o, x) + (uint32_t)i;
    return x;
}

typedef struct Shape Shape;
struct Shape { uint32_t (*f)(const Shape *, uint32_t); uint32_t a; };
NOINLINE static uint32_t sq_f(const Shape *s, uint32_t x) { return (x ^ s->a) * 3u + 1u; }
NOINLINE static uint32_t ci_f(const Shape *s, uint32_t x) { return x + s->a * 5u; }
NOINLINE static uint32_t tr_f(const Shape *s, uint32_t x) { return (x ^ (x >> 3)) + s->a; }
static uint32_t call_polymorphic(int32_t n) {
    const Shape objs[3] = {{sq_f, 3}, {ci_f, 5}, {tr_f, 7}};
    uint32_t x = (uint32_t)SEED;
    for (int32_t i = 0; i < n; i++) x = objs[i % 3].f(&objs[i % 3], x);
    return x;
}

typedef struct { uint32_t k; } MixEnv;
typedef struct { uint32_t (*fn)(const MixEnv *, uint32_t); MixEnv *env; } MixClosure;
NOINLINE static uint32_t mixer_body(const MixEnv *e, uint32_t y) { return (y ^ e->k) * 5u + 1u; }
static uint32_t closure_create(int32_t n) {
    uint32_t x = (uint32_t)SEED;
    for (int32_t i = 0; i < n; i++) {
        MixClosure f = {mixer_body, xmalloc(sizeof(MixEnv))};
        f.env->k = (uint32_t)i & 255u;
        alloc_sink = f.env;
        x = f.fn(f.env, x);
        free(f.env);
    }
    return x;
}

typedef struct { uint32_t a, b; } Pair;
static uint32_t object_alloc_ctor(int32_t n) {
    uint32_t x = (uint32_t)SEED;
    for (int32_t i = 0; i < n; i++) {
        Pair *p = xmalloc(sizeof *p);
        p->a = x & 1023u;
        p->b = (uint32_t)i;
        alloc_sink = p;
        x = x * 3u + p->a + p->b;
        free(p);
    }
    return x;
}

typedef struct { uint32_t x, y; } Point;
static uint32_t object_alloc_literal(int32_t n) {
    uint32_t x = (uint32_t)SEED;
    for (int32_t i = 0; i < n; i++) {
        Point *o = xmalloc(sizeof *o);
        o->x = x & 4095u;
        o->y = (uint32_t)i ^ 5u;
        alloc_sink = o;
        x = x * 5u + o->x - o->y;
        free(o);
    }
    return x;
}

static uint32_t field_read_write(int32_t n) {
    struct { uint32_t a, b, c; } o = {(uint32_t)SEED, 1, 2};
    for (int32_t i = 0; i < n; i++) {
        o.a = o.a + o.c;
        o.b = o.b ^ o.a;
        o.c = o.c * 3u + (uint32_t)i;
    }
    return o.a ^ o.b ^ o.c;
}

/* Four layouts with `v` at different offsets, dispatched on a shape tag. */
typedef struct {
    int tag;
    union {
        struct { uint32_t v; } s0;
        struct { uint32_t a, v; } s1;
        struct { uint32_t a, b, v; } s2;
        struct { uint32_t a, b, c, v; } s3;
    } u;
} FObj;
static uint32_t fobj_v(const FObj *o) {
    switch (o->tag) {
    case 0: return o->u.s0.v;
    case 1: return o->u.s1.v;
    case 2: return o->u.s2.v;
    default: return o->u.s3.v;
    }
}
static uint32_t field_polymorphic(int32_t n) {
    uint32_t v = (uint32_t)(SEED & 7);
    FObj shapes[4];
    shapes[0].tag = 0; shapes[0].u.s0.v = v + 1;
    shapes[1].tag = 1; shapes[1].u.s1.a = 0; shapes[1].u.s1.v = v + 2;
    shapes[2].tag = 2; shapes[2].u.s2.a = shapes[2].u.s2.b = 0; shapes[2].u.s2.v = v + 3;
    shapes[3].tag = 3; shapes[3].u.s3.a = shapes[3].u.s3.b = shapes[3].u.s3.c = 0; shapes[3].u.s3.v = v + 4;
    uint32_t x = (uint32_t)SEED;
    for (int32_t i = 0; i < n; i++) x = x * 3u + fobj_v(&shapes[i & 3]);
    return x;
}

typedef struct { uint32_t a, b, c, d; } Quad;
static uint32_t field_add(int32_t n) {
    uint32_t x = (uint32_t)SEED;
    for (int32_t i = 0; i < n; i++) {
        Quad *o = xmalloc(sizeof *o);
        o->a = x & 255u;
        o->b = (uint32_t)i & 1023u;
        o->c = (x >> 8) & 255u;
        o->d = 9;
        alloc_sink = o;
        x = x * 7u + o->a + o->b + o->c + o->d;
        free(o);
    }
    return x;
}

static uint32_t array_index(int32_t n) {
    enum { len = 4096 };
    static uint32_t a[len];
    for (int j = 0; j < len; j++) a[j] = (uint32_t)SEED + (uint32_t)j * 7u;
    uint32_t s = 0;
    for (int32_t r = 0; r < n; r++) {
        for (int j = 0; j < len; j++) {
            s = s + a[j];
            a[j] = a[j] * 5u + s;
        }
    }
    return s;
}

static uint32_t array_double(int32_t n) {
    enum { len = 4096 };
    static double a[len];
    for (int j = 0; j < len; j++) a[j] = (j % 17) * 0.37;
    double t = 0;
    for (int32_t r = 0; r < n; r++) {
        for (int j = 0; j < len; j++) {
            double v = a[j] * 0.5 + a[(j + 1) & (len - 1)] * 0.25 + (j & 7) * 0.125;
            a[j] = v;
            t += v;
        }
    }
    return u32d(t * 16);
}

typedef struct { uint32_t *data; size_t len, cap; } Vec;
static void vec_push(Vec *v, uint32_t x) {
    if (v->len == v->cap) {
        v->cap = v->cap ? v->cap * 2 : 16;
        v->data = realloc(v->data, v->cap * sizeof *v->data);
        if (!v->data) { fputs("out of memory\n", stderr); exit(1); }
    }
    v->data[v->len++] = x;
}
static uint32_t array_push_pop(int32_t n) {
    Vec a = {0};
    uint32_t x = (uint32_t)SEED;
    for (int32_t r = 0; r < n; r++) {
        for (uint32_t j = 0; j < 1000; j++) vec_push(&a, x ^ j);
        while (a.len > 0) x = x * 31u + a.data[--a.len];
    }
    free(a.data);
    return x;
}

typedef struct { uint32_t v, w; } VW;
static uint32_t array_of_objects(int32_t n) {
    uint32_t x = (uint32_t)SEED;
    for (int32_t r = 0; r < n; r++) {
        VW **arr = xmalloc(1000 * sizeof *arr);
        for (uint32_t j = 0; j < 1000; j++) {
            arr[j] = xmalloc(sizeof **arr);
            arr[j]->v = x + j;
            arr[j]->w = j;
        }
        alloc_sink = arr;
        uint32_t s = 0;
        for (int j = 0; j < 1000; j++) s = s + (arr[j]->v ^ arr[j]->w);
        x = x * 31u + s;
        for (int j = 0; j < 1000; j++) free(arr[j]);
        free(arr);
    }
    return x;
}

static uint32_t typed_f64(int32_t n) {
    enum { len = 4096 };
    static double bufa[len], bufb[len];
    double *a = bufa, *b = bufb;
    for (int j = 0; j < len; j++) { a[j] = 1 + (j & 15) * 0.0625; b[j] = a[j]; }
    a[0] = b[0] = SEED % 100;
    a[len - 1] = b[len - 1] = 0;
    for (int32_t r = 0; r < n; r++) {
        for (int j = 1; j < len - 1; j++) b[j] = 0.25 * a[j - 1] + 0.5 * a[j] + 0.25 * a[j + 1];
        double *t = a; a = b; b = t;
    }
    double s = 0;
    for (int j = 0; j < len; j++) s += a[j];
    return u32d(s * 1e6);
}

static uint32_t typed_u8(int32_t n) {
    enum { len = 65536 };
    static uint8_t u[len];
    for (int j = 0; j < len; j++) u[j] = (uint8_t)((j * 7 + SEED) & 255);
    uint8_t x = (uint8_t)(SEED & 255);
    for (int32_t r = 0; r < n; r++) {
        for (int j = 0; j < len; j++) {
            u[j] = (uint8_t)(u[j] + (u[(j + 1) & (len - 1)] ^ x));
            x = u[j];
        }
    }
    uint32_t s = 0;
    for (int j = 0; j < len; j++) s += u[j];
    return s * 256u + x;
}

static uint32_t typed_i32(int32_t n) {
    enum { len = 16384 };
    static uint32_t a[len];
    for (int j = 0; j < len; j++) a[j] = (uint32_t)SEED * (uint32_t)(j + 1);
    for (int32_t r = 0; r < n; r++) {
        for (int j = 1; j < len; j++) a[j] = ((a[j] * 0x01000193u) ^ a[j - 1]) + (uint32_t)r;
    }
    uint32_t s = 0;
    for (int j = 0; j < len; j++) s += a[j];
    return s;
}

static uint32_t string_charcode(int32_t n) {
    uint32_t h = (uint32_t)SEED;
    for (int32_t r = 0; r < n; r++) {
        for (int j = 0; j < LONG_LEN; j++) h = h * 31u + LONG_STR[j];
    }
    return h;
}

static uint32_t string_concat(int32_t n) {
    size_t cap = 64;
    char *s = xmalloc(cap);
    uint32_t h = (uint32_t)SEED;
    for (int32_t r = 0; r < n; r++) {
        size_t len = 0;
        for (uint32_t j = 0; j < 1000; j++) {
            uint32_t p = (h + j) & 7u;
            if (len + PIECE_LEN[p] > cap) {
                cap *= 2;
                s = realloc(s, cap);
                if (!s) { fputs("out of memory\n", stderr); exit(1); }
            }
            memcpy(s + len, PIECES[p], PIECE_LEN[p]);
            len += PIECE_LEN[p];
        }
        for (size_t k = 0; k < len; k++) h = h * 31u + (unsigned char)s[k];
        h = h + (uint32_t)len;
    }
    free(s);
    return h;
}

static uint32_t string_compare(int32_t n) {
    uint32_t x = (uint32_t)SEED, h = 0;
    for (int32_t i = 0; i < n; i++) {
        int c = strcmp(WORDS[x & 15], WORDS[(x >> 4) & 15]);
        h = h * 31u + (c == 0 ? 1u : 0u) + (c < 0 ? 2u : 0u);
        x = lcg(x);
    }
    return h;
}

/* Every OPS entry is 3 chars + NUL, so a 4-byte memcmp is the full string equality. */
static int op_is(const char *op, const char *name) { return memcmp(op, name, 4) == 0; }
static uint32_t string_switch(int32_t n) {
    uint32_t x = (uint32_t)SEED;
    for (int32_t i = 0; i < n; i++) {
        const char *op = OPS[(x >> 8) & 7];
        if (op_is(op, "add")) x = x + (uint32_t)i;
        else if (op_is(op, "sub")) x = x - 7u;
        else if (op_is(op, "mul")) x = x * 3u;
        else if (op_is(op, "xor")) x = x ^ 0x5bd1u;
        else if (op_is(op, "shl")) x = (x << 1) | 1u;
        else if (op_is(op, "neg")) x = 0u - x;
        else x = x + 1u;
        x = lcg(x);
    }
    return x;
}

static uint32_t int_switch(int32_t n) {
    uint32_t x = (uint32_t)SEED;
    for (int32_t i = 0; i < n; i++) {
        switch ((x >> 8) & 7) {
        case 0: x = x + (uint32_t)i; break;
        case 1: x = x - 7u; break;
        case 2: x = x * 3u; break;
        case 3: x = x ^ 0x5bd1u; break;
        case 4: x = (x << 1) | 1u; break;
        case 5: x = 0u - x; break;
        case 6: x = x + 0x1234u; break;
        case 7: x = x ^ (x >> 5); break;
        }
        x = lcg(x);
    }
    return x;
}

/* Open-addressing hash table with linear probing and backward-shift deletion.
 * Keys are either small ints or string pointers; the key kind picks hash/eq. */
typedef struct { uintptr_t key; uint32_t value; uint8_t used; } Slot;
typedef struct { Slot *slots; uint32_t mask, size; int string_keys; } Table;

static uint32_t key_hash(const Table *t, uintptr_t key) {
    if (!t->string_keys) return (uint32_t)key * 2654435761u;
    uint32_t h = 2166136261u;
    for (const unsigned char *p = (const unsigned char *)key; *p; p++) h = (h ^ *p) * 16777619u;
    return h;
}
static int key_eq(const Table *t, uintptr_t a, uintptr_t b) {
    return t->string_keys ? strcmp((const char *)a, (const char *)b) == 0 : a == b;
}
static Table table_new(uint32_t cap, int string_keys) {
    Table t = {calloc(cap, sizeof(Slot)), cap - 1, 0, string_keys};
    if (!t.slots) { fputs("out of memory\n", stderr); exit(1); }
    return t;
}
static uint32_t table_find(const Table *t, uintptr_t key) {
    uint32_t i = key_hash(t, key) & t->mask;
    while (t->slots[i].used && !key_eq(t, t->slots[i].key, key)) i = (i + 1) & t->mask;
    return i;
}
static void table_set_at(Table *t, uint32_t i, uintptr_t key, uint32_t value) {
    if (!t->slots[i].used) { t->slots[i].used = 1; t->slots[i].key = key; t->size++; }
    t->slots[i].value = value;
}
static void table_delete_at(Table *t, uint32_t i) {
    uint32_t j = i;
    for (;;) {
        j = (j + 1) & t->mask;
        if (!t->slots[j].used) break;
        uint32_t home = key_hash(t, t->slots[j].key) & t->mask;
        /* Move j into the hole i unless its home lies cyclically in (i, j]. */
        if (((j - home) & t->mask) >= ((j - i) & t->mask)) { t->slots[i] = t->slots[j]; i = j; }
    }
    t->slots[i].used = 0;
    t->size--;
}

static uint32_t map_churn(int32_t n, Table *m, uintptr_t (*key_of)(uint32_t)) {
    uint32_t x = (uint32_t)SEED, s = 0;
    for (int32_t i = 0; i < n; i++) {
        uintptr_t k = key_of(x);
        uint32_t at = table_find(m, k);
        if (!m->slots[at].used) table_set_at(m, at, k, (uint32_t)i);
        else {
            uint32_t v = m->slots[at].value;
            s = s + v;
            if ((v & 1) == 1) table_delete_at(m, at);
            else m->slots[at].value = v + 1;
        }
        x = lcg(x);
    }
    s += m->size;
    free(m->slots);
    return s;
}
static uintptr_t int_key(uint32_t x) { return (x >> 8) & 8191; }
static uintptr_t string_key(uint32_t x) { return (uintptr_t)SKEYS[(x >> 8) & 511]; }
static uint32_t map_int_keys(int32_t n) { Table m = table_new(16384, 0); return map_churn(n, &m, int_key); }
static uint32_t map_string_keys(int32_t n) { Table m = table_new(1024, 1); return map_churn(n, &m, string_key); }

static uint32_t object_dict(int32_t n) {
    Table d = table_new(128, 1);
    for (int j = 0; j < 64; j++) table_set_at(&d, table_find(&d, (uintptr_t)DKEYS[j]), (uintptr_t)DKEYS[j], (uint32_t)j);
    uint32_t x = (uint32_t)SEED;
    for (int32_t i = 0; i < n; i++) {
        Slot *slot = &d.slots[table_find(&d, (uintptr_t)DKEYS[(x >> 10) & 63])];
        uint32_t v = slot->value + (uint32_t)i;
        slot->value = v;
        x = x * 1664525u + 1013904223u + v;
    }
    uint32_t s = x;
    for (int j = 0; j < 64; j++) s = s * 31u + d.slots[table_find(&d, (uintptr_t)DKEYS[j])].value;
    free(d.slots);
    return s;
}

static uint32_t for_of_array(int32_t n) {
    enum { len = 4096 };
    static uint32_t a[len];
    for (int j = 0; j < len; j++) a[j] = (uint32_t)(j * 7919 + SEED) & 0xffffu;
    uint32_t s = 0;
    for (int32_t r = 0; r < n; r++) {
        for (int j = 0; j < len; j++) s = s * 31u + a[j];
    }
    return s;
}

/* Generic callback-driven map/filter/reduce, the C shape of the Array builtins. */
typedef uint32_t (*MapFn)(const void *env, uint32_t v);
typedef int (*PredFn)(const void *env, uint32_t v);
typedef uint32_t (*FoldFn)(const void *env, uint32_t acc, uint32_t v);
static size_t map_u32(const uint32_t *src, size_t n, uint32_t *dst, MapFn f, const void *env) {
    for (size_t j = 0; j < n; j++) dst[j] = f(env, src[j]);
    return n;
}
static size_t filter_u32(const uint32_t *src, size_t n, uint32_t *dst, PredFn f, const void *env) {
    size_t m = 0;
    for (size_t j = 0; j < n; j++) if (f(env, src[j])) dst[m++] = src[j];
    return m;
}
static uint32_t reduce_u32(const uint32_t *src, size_t n, FoldFn f, const void *env, uint32_t acc) {
    for (size_t j = 0; j < n; j++) acc = f(env, acc, src[j]);
    return acc;
}
static uint32_t mfr_scale(const void *env, uint32_t v) { return v * 3u + *(const uint32_t *)env; }
static int mfr_keep(const void *env, uint32_t v) { (void)env; return (v & 3) != 0; }
static uint32_t mfr_add(const void *env, uint32_t acc, uint32_t v) { (void)env; return acc + v; }
static uint32_t array_map_filter_reduce(int32_t n) {
    uint32_t base[1000], mapped[1000], kept[1000];
    for (int j = 0; j < 1000; j++) base[j] = (uint32_t)(j * 37 + SEED) & 1023u;
    uint32_t s = 0, r = (uint32_t)(SEED & 255);
    for (int32_t round = 0; round < n; round++) {
        size_t m = map_u32(base, 1000, mapped, mfr_scale, &r);
        m = filter_u32(mapped, m, kept, mfr_keep, NULL);
        uint32_t t = reduce_u32(kept, m, mfr_add, NULL, 0);
        s = s * 31u + t;
        r = s & 255u;
    }
    return s;
}

static int cmp_u32(const void *p, const void *q) {
    return (int)*(const uint32_t *)p - (int)*(const uint32_t *)q;
}
static uint32_t array_sort_comparator(int32_t n) {
    uint32_t a[1000];
    uint32_t x = (uint32_t)SEED, h = 0;
    for (int32_t r = 0; r < n; r++) {
        for (int j = 0; j < 1000; j++) { x = lcg(x); a[j] = x >> 12; }
        qsort(a, 1000, sizeof a[0], cmp_u32);
        for (int j = 0; j < 1000; j++) h = h * 31u + a[j];
    }
    return h;
}

static uint32_t math_functions(int32_t n) {
    uint32_t x = (uint32_t)SEED;
    double acc = 0;
    for (int32_t i = 0; i < n; i++) {
        double t = sqrt((double)x + 0.5);
        double f = floor(t * 3.7);
        double a = abs((int32_t)x >> 16);
        double m = f > a ? f : a;
        m = m < 200000.0 ? m : 200000.0;
        x = x * 1103515245u + (uint32_t)m + 12345u;
        acc += t;
    }
    return x ^ u32d(acc);
}

static uint32_t try_catch_nothrow(int32_t n) {
    uint32_t x = (uint32_t)SEED;
    for (int32_t i = 0; i < n; i++) {
        x = x * 31u + (uint32_t)i;
        x ^= x >> 9;
        if (x == 0x7ffffffeu) x = 1;
    }
    return x;
}

/* C reports the error by return code; the caller "catches" it. */
typedef struct { uint32_t code; } Error;
static const Error THROWN = {7};
static const Error *maybe_throw(uint32_t x, uint32_t i, uint32_t *out) {
    if ((i & 15) == 0) return &THROWN;
    *out = x * 31u + i;
    return NULL;
}
static uint32_t try_catch_throw(int32_t n) {
    uint32_t x = (uint32_t)SEED;
    for (int32_t i = 0; i < n; i++) {
        const Error *e = maybe_throw(x, (uint32_t)i, &x);
        if (e) x = x + e->code;
    }
    return x;
}

/* Tagged values; instanceof walks the class parent chain. */
typedef struct Class { const struct Class *parent; } Class;
typedef struct { const Class *cls; uint32_t a, b; } Instance;
typedef struct { uint32_t len; const char *chars; } Str;
typedef enum { T_NUMBER, T_STRING, T_BOOLEAN, T_OBJECT, T_NULL } Tag;
typedef struct { Tag tag; union { double num; const Str *str; int boolean; const Instance *obj; } u; } Value;
static const Class BASE_CLASS = {NULL};
static const Class DERIVED_CLASS = {&BASE_CLASS};
static int instance_of(const Value *v, const Class *cls) {
    if (v->tag != T_OBJECT) return 0;
    for (const Class *c = v->u.obj->cls; c; c = c->parent) if (c == cls) return 1;
    return 0;
}
static uint32_t instanceof_typeof(int32_t n) {
    static const Str hello = {5, "hello"}, ab = {2, "ab"};
    static const Instance base = {&BASE_CLASS, 5, 0}, derived = {&DERIVED_CLASS, 6, 1};
    Value mixed[8] = {
        {T_NUMBER, {.num = 3}}, {T_STRING, {.str = &hello}}, {T_OBJECT, {.obj = &base}},
        {T_NUMBER, {.num = 2.5}}, {T_BOOLEAN, {.boolean = 1}}, {T_OBJECT, {.obj = &derived}},
        {T_NULL, {.num = 0}}, {T_STRING, {.str = &ab}}};
    uint32_t s = (uint32_t)SEED;
    for (int32_t i = 0; i < n; i++) {
        const Value *v = &mixed[((uint32_t)i ^ (s >> 3)) & 7];
        uint32_t c;
        if (v->tag == T_NUMBER) c = (uint32_t)(int32_t)v->u.num;
        else if (v->tag == T_STRING) c = v->u.str->len;
        else if (v->tag == T_BOOLEAN) c = v->u.boolean ? 11 : 13;
        else if (instance_of(v, &DERIVED_CLASS)) c = v->u.obj->a * 2;
        else if (instance_of(v, &BASE_CLASS)) c = v->u.obj->a;
        else c = 17;
        s = s * 3u + c;
    }
    return s;
}

typedef struct { uint32_t v; } Box;
static uint32_t box_get(const Box *b) { return b->v; }
static void box_set(Box *b, uint32_t n) { b->v = n; }
static uint32_t class_getter_setter(int32_t n) {
    Box b = {(uint32_t)SEED};
    for (int32_t i = 0; i < n; i++) box_set(&b, box_get(&b) * 3u + (uint32_t)i);
    return box_get(&b);
}

typedef struct { uint32_t x; } Priv;
static uint32_t priv_run(Priv *self, int32_t n) {
    for (int32_t i = 0; i < n; i++) self->x = self->x * 5u + (uint32_t)i;
    return self->x;
}
static uint32_t class_private_field(int32_t n) { Priv p = {(uint32_t)SEED}; return priv_run(&p, n); }

/* Generator as an explicit resumable state machine. */
typedef struct { int state; int32_t i, n; uint32_t x; } Counter;
static int counter_next(Counter *g, uint32_t *out) {
    switch (g->state) {
    case 0: g->i = 0; break;
    case 1: g->i++; break;
    default: return 0;
    }
    if (g->i >= g->n) { g->state = 2; return 0; }
    g->x = lcg(g->x);
    *out = g->x >> 16;
    g->state = 1;
    return 1;
}
static uint32_t generator_iterate(int32_t n) {
    Counter g = {0, 0, n, (uint32_t)SEED};
    uint32_t s = 0, v;
    while (counter_next(&g, &v)) s = s * 31u + v;
    return s;
}

/* Promises: a microtask ring of continuations, and an async frame as a state machine. */
typedef void (*Reaction)(void *ctx, uint32_t value);
typedef struct { Reaction run; void *ctx; uint32_t value; } Job;
enum { JOB_CAP = 64 };
static Job jobs[JOB_CAP];
static uint32_t job_head, job_tail;
static void enqueue_job(Reaction run, void *ctx, uint32_t value) {
    jobs[job_tail++ % JOB_CAP] = (Job){run, ctx, value};
}
static void drain_jobs(void) {
    while (job_head != job_tail) {
        Job j = jobs[job_head++ % JOB_CAP];
        j.run(j.ctx, j.value);
    }
}
typedef struct { int settled; uint32_t value; Reaction then; void *then_ctx; } Promise;
static void promise_resolve(Promise *p, uint32_t v) {
    p->settled = 1;
    p->value = v;
    if (p->then) enqueue_job(p->then, p->then_ctx, v);
}
static void promise_then(Promise *p, Reaction then, void *ctx) {
    if (p->settled) enqueue_job(then, ctx, p->value);
    else { p->then = then; p->then_ctx = ctx; }
}
static void add_async(uint32_t x, uint32_t i, Promise *out) { promise_resolve(out, x * 31u + i); }
typedef struct { int32_t i, n; uint32_t x; Promise done; } ChainFrame;
static void chain_resume(void *ctx, uint32_t value);
static void chain_step(ChainFrame *f) {
    if (f->i < f->n) {
        Promise p = {0};
        add_async(f->x, (uint32_t)f->i, &p);
        promise_then(&p, chain_resume, f);
    } else {
        promise_resolve(&f->done, f->x);
    }
}
static void chain_resume(void *ctx, uint32_t value) {
    ChainFrame *f = ctx;
    f->x = value;
    f->i++;
    chain_step(f);
}
static uint32_t promise_chain(int32_t n) {
    ChainFrame f = {0, n, (uint32_t)SEED, {0}};
    chain_step(&f);
    drain_jobs();
    return f.done.value;
}

static uint32_t sum_rest(const uint32_t *xs, size_t count) {
    uint32_t s = 0;
    for (size_t k = 0; k < count; k++) s = s * 3u + xs[k];
    return s;
}
static uint32_t rest_spread_args(int32_t n) {
    uint32_t args[4] = {(uint32_t)SEED, 0, 7, 11};
    uint32_t x = (uint32_t)SEED;
    for (int32_t i = 0; i < n; i++) {
        args[0] = x;
        args[1] = (uint32_t)i;
        x = sum_rest(args, 4);
    }
    return x;
}

static uint32_t sum_args(int count, ...) {
    va_list ap;
    va_start(ap, count);
    uint32_t s = 0;
    for (int k = 0; k < count; k++) s = s * 3u + va_arg(ap, uint32_t);
    va_end(ap);
    return s;
}
static uint32_t arguments_object(int32_t n) {
    uint32_t x = (uint32_t)SEED;
    for (int32_t i = 0; i < n; i++) x = sum_args(5, x, (uint32_t)i, 13u, 5u, 3u);
    return x;
}

static uint32_t bigint_arith(int32_t n) {
    const uint64_t M = 4294967291u, A = 2654435761u;
    uint64_t x = (uint64_t)SEED, c = 0;
    for (int32_t i = 0; i < n; i++) {
        x = (x * A + c) % M;
        c += 1;
    }
    return (uint32_t)x;
}

/* Hand-coded /^[a-f0-9]+$/. */
static int is_lower_hex(const char *s) {
    if (!*s) return 0;
    for (; *s; s++) if (!((*s >= 'a' && *s <= 'f') || (*s >= '0' && *s <= '9'))) return 0;
    return 1;
}
static uint32_t regexp_test(int32_t n) {
    uint32_t h = (uint32_t)SEED;
    for (int32_t i = 0; i < n; i++) {
        uint32_t k = ((uint32_t)i ^ (h >> 5)) & 15;
        h = h * 31u + (is_lower_hex(HEXES[k]) ? k + 1 : 0);
    }
    return h;
}

/* JSON for the fixed shape {id,kind,x,y,ok,tag}: snprintf out, flat-object parser in. */
typedef struct { int64_t id, x, y; int ok; char kind[16], tag[16]; } Rec;
static const char *json_string(const char *p, char *dst, size_t cap) {
    size_t n = 0;
    for (p++; *p != '"'; p++) if (n + 1 < cap) dst[n++] = *p;
    dst[n] = 0;
    return p + 1;
}
static const char *json_field(const char *p, Rec *r) {
    char key[16];
    p = json_string(p, key, sizeof key) + 1; /* skip ':' */
    if (*p == '"') return json_string(p, strcmp(key, "kind") == 0 ? r->kind : r->tag, sizeof r->kind);
    if (*p == 't' || *p == 'f') { r->ok = *p == 't'; return p + (*p == 't' ? 4 : 5); }
    char *end;
    int64_t v = strtoll(p, &end, 10);
    if (strcmp(key, "id") == 0) r->id = v;
    else if (strcmp(key, "x") == 0) r->x = v;
    else r->y = v;
    return end;
}
static void json_parse_rec(const char *p, Rec *r) {
    for (p++; *p != '}'; ) {
        p = json_field(p, r);
        if (*p == ',') p++;
    }
}
static uint32_t json_roundtrip(int32_t n) {
    char buf[128];
    uint32_t x = (uint32_t)SEED;
    for (int32_t i = 0; i < n; i++) {
        int len = snprintf(buf, sizeof buf, "{\"id\":%d,\"kind\":\"pt\",\"x\":%u,\"y\":%u,\"ok\":%s,\"tag\":\"q\"}",
                           i, x & 0xffffu, x >> 16, (x & 1) ? "true" : "false");
        Rec p = {0};
        json_parse_rec(buf, &p);
        x = x * 31u + (uint32_t)p.id + (uint32_t)p.x + (uint32_t)p.y + (p.ok ? 1u : 0u) + (uint32_t)len
            + (uint32_t)strlen(p.kind) + (uint32_t)strlen(p.tag);
    }
    return x;
}

/* ---- runner ---- */
typedef struct { const char *name; int32_t n; uint32_t (*run)(int32_t); } Primitive;
static const Primitive PRIMITIVES[] = {
    {"int-loop", 50000000, int_loop},
    {"float-loop", 20000000, float_loop},
    {"recursion-fib", 34, recursion_fib},
    {"call-direct", 50000000, call_direct},
    {"call-closure", 50000000, call_closure},
    {"call-method", 50000000, call_method},
    {"call-polymorphic", 30000000, call_polymorphic},
    {"closure-create", 10000000, closure_create},
    {"object-alloc-ctor", 10000000, object_alloc_ctor},
    {"object-alloc-literal", 10000000, object_alloc_literal},
    {"field-read-write", 50000000, field_read_write},
    {"field-polymorphic", 50000000, field_polymorphic},
    {"field-add", 10000000, field_add},
    {"array-index", 20000, array_index},
    {"array-double", 5000, array_double},
    {"array-push-pop", 20000, array_push_pop},
    {"array-of-objects", 5000, array_of_objects},
    {"typed-f64", 20000, typed_f64},
    {"typed-u8", 500, typed_u8},
    {"typed-i32", 2000, typed_i32},
    {"string-charcode", 500, string_charcode},
    {"string-concat", 8000, string_concat},
    {"string-compare", 20000000, string_compare},
    {"string-switch", 10000000, string_switch},
    {"int-switch", 30000000, int_switch},
    {"map-int-keys", 10000000, map_int_keys},
    {"map-string-keys", 5000000, map_string_keys},
    {"object-dict", 10000000, object_dict},
    {"for-of-array", 10000, for_of_array},
    {"array-map-filter-reduce", 30000, array_map_filter_reduce},
    {"array-sort-comparator", 2000, array_sort_comparator},
    {"math-functions", 8000000, math_functions},
    {"try-catch-nothrow", 50000000, try_catch_nothrow},
    {"try-catch-throw", 30000000, try_catch_throw},
    {"instanceof-typeof", 20000000, instanceof_typeof},
    {"class-getter-setter", 50000000, class_getter_setter},
    {"class-private-field", 50000000, class_private_field},
    {"generator-iterate", 20000000, generator_iterate},
    {"promise-chain", 10000000, promise_chain},
    {"rest-spread-args", 20000000, rest_spread_args},
    {"arguments-object", 20000000, arguments_object},
    {"bigint-arith", 10000000, bigint_arith},
    {"regexp-test", 8000000, regexp_test},
    {"json-roundtrip", 500000, json_roundtrip},
};

static double now_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1e3 + ts.tv_nsec / 1e6;
}

int main(void) {
    SEED = seed_source;
    setup_tables();
    for (size_t k = 0; k < sizeof PRIMITIVES / sizeof PRIMITIVES[0]; k++) {
        const Primitive *p = &PRIMITIVES[k];
        double t0 = now_ms();
        uint32_t sum = p->run(p->n);
        printf("PRIM %s %.3f %u\n", p->name, now_ms() - t0, sum);
        fflush(stdout);
    }
    return 0;
}
