// jetstream-row: proxy-vue
// jetstream-files: proxy/common.js proxy/vue-bundle.js proxy/vue-benchmark.js
// iterations: 120
// jetstream-driver: class=Async deterministic=true
/*
 * C analogue of the JetStream 3 proxy-vue row: proxy/vue-benchmark.js on top of
 * the reminder model and test script in proxy/common.js, running on a C model of
 * the Vue reactivity in proxy/vue-bundle.js. That bundle is Vue 3.2.47: dependencies
 * are sets with per-run bit marks, not the Link/version structures of Vue 3.5.
 *
 * What the JS does per iteration and what this file does the same way:
 *
 *   - reactive() Proxies for the store, the lists, the reminders, the tag Sets,
 *     the reminder arrays and the Maps. Each Proxy trap of the bundle has a C
 *     counterpart: px_get, px_set, px_has, px_delete. px_get runs the getter (if
 *     any) with the proxy as receiver, then tracks the key, as the get trap does,
 *     including reads of method names. Every value that comes out of a trap is
 *     wrapped (wrap()) through the proxyMap cache g_proxies, a hash map from raw
 *     target to Proxy, as reactive() does for nested objects.
 *   - targetMap: a hash map from raw target to its depsMap, an insertion-ordered
 *     hash map from key (property atom, array index, or the ITERATE and
 *     MAP_KEY_ITERATE symbols) to a DepSet. A DepSet is an ordered set of effects
 *     with the bundle's per-run bit marks w and nbits.
 *   - track() and trigger() are the bundle's d() and We(). trigger() chooses deps
 *     as We() does: the key's dep; ITERATE for Set and Map add; MAP_KEY_ITERATE for
 *     Map add; length for an array index add; and every index at or past the new
 *     length for an array length write. One dep runs as it is; several deps are
 *     merged into one set first.
 *   - Effects: effect_run() is ReactiveEffect.run(): mark the deps with the run's
 *     bit, run the body, then remove the effect from each dep it did not read in
 *     this run. The render is a watchEffect (flush "pre"), queued through
 *     queue_job() with the bundle's dedup. Each reminder's isCompleted is a watch
 *     with { immediate: true, flush: "sync" }, whose scheduler runs the job inline.
 *   - The queue is flushed after each test step, standing in for the awaited
 *     microtask of each render() call in the JS test.
 *   - Arrays: the methods the test uses run with the spec's Get, Has, Set, Delete
 *     steps on the receiver. A reactive receiver goes through the traps, so every
 *     element read and every length write is a trap, as in V8. The mutating
 *     methods pause tracking, as the bundle's push/pop/shift/unshift/splice
 *     instrumentations do. indexOf is the bundle's instrumented version: it tracks
 *     the length and every index before searching.
 *   - Map and Set: the bundle's collection instrumentations. get, has, size,
 *     forEach and iteration track their keys (size and iteration track ITERATE);
 *     set and add trigger without tracking.
 *   - Getters (allTags, popularTagsWithCount, completedRemidersCount, isEmpty,
 *     indexInList, store) are not cached, as in the JS. A value returned by a
 *     getter is wrapped like any other read, so the temporary Map and the array of
 *     pairs that the getters build are reactive while the render reads them.
 *   - store.render() builds the whole text, in the same order as the JS: lists,
 *     reminders with tags, counts, popular tags, all-time completed tags. Array
 *     destructuring of a wrapped [tag, count] pair reads length and each index
 *     through the trap, as the array iterator does.
 *   - Math.random is JetStream's seeded generator, reset before each iteration.
 *     The JetStream driver for proxy-vue does not seed Math.random, so the header
 *     sets deterministic=true; the seeded run is what the validator checks.
 *   - The list names are Date.toDateString() of today plus the list index. The
 *     clock is taken once per run (PROXY_VUE_DATE=YYYY-MM-DD pins it to noon of that
 *     day).
 *
 * Validation. tools/proxy-vue-validate.cjs runs the row under node 22 with an
 * instrumented copy of vue-bundle.js (the file itself is read, not changed). For
 * each iteration it prints: effective track() calls (made while an effect is
 * tracking); trigger() calls that found a depsMap; scheduler calls (Ke); active
 * effect runs; queued jobs run; watch callbacks; and the length and FNV-1a hash of
 * the final store render. This file counts the same events at the same points,
 * and checks all eight numbers of every iteration against g_expect, which the
 * validator generates with --date 2026-11-20 --c. The render text holds the list
 * dates, so the render length and hash are checked only when the clock is
 * 2026-11-20 (the table's date); on other days the counts are still checked.
 *
 * Measured in this container (4 vCPU): node 22 runs the composed row's 120
 * iterations in about 650-730 ms (timed inside the program with performance.now());
 * this file runs them in about 50-70 ms.
 *
 * Not modelled, because this row does not use them: Map/Set delete and clear,
 * computed refs, effect scopes, the ownKeys and getOwnPropertyDescriptor traps,
 * symbol keys other than the well-known iterator, readonly and shallow proxies,
 * and the bundle's internal __v_* flag probes (they are not tracked and change no
 * count). Array indices are integer keys, not the strings of the JS, so there is no
 * number-to-string conversion per index. The queue is not sorted: every queued job
 * has the same id, so the bundle's sort keeps insertion order. Sorting is a stable
 * insertion sort; V8 uses TimSort, so the comparator call counts differ, but the
 * comparator reads are untracked and a stable sort's permutation is unique. Objects
 * live in a per-iteration arena and are not collected; string building writes the
 * render straight into one buffer, where the JS builds intermediate strings.
 *
 * Build: cc -O2 -Wall proxy-vue.c. Run: ./proxy-vue [iterations [jetstream-root]].
 */

#define _POSIX_C_SOURCE 200809L

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define ITERATIONS 120
#define RNG_SEED UINT32_C(49734321) /* JetStream's seeded Math.random */
#define NOUN_COUNT 100
#define ADJ_COUNT 10
#define REMINDERS_PER_LIST 10
#define LIST_COUNT 10

/* ---------- arena: everything of one iteration lives until ar_reset() ---------- */

typedef struct Chunk {
    struct Chunk *next;
    size_t used, cap;
    unsigned char *mem;
} Chunk;

static Chunk *g_chunks, *g_cur;

static void *ar_alloc(size_t n)
{
    n = (n + 15) & ~(size_t)15;
    for (;;) {
        if (g_cur && g_cur->used + n <= g_cur->cap) {
            void *p = g_cur->mem + g_cur->used;
            g_cur->used += n;
            memset(p, 0, n);
            return p;
        }
        if (g_cur && g_cur->next) {
            g_cur = g_cur->next;
            continue;
        }
        {
            size_t cap = n > ((size_t)1 << 20) ? n : ((size_t)1 << 20);
            Chunk *c = calloc(1, sizeof *c);
            if (!c || !(c->mem = malloc(cap))) {
                fputs("proxy-vue: out of memory\n", stderr);
                exit(2);
            }
            c->cap = cap;
            if (g_cur)
                g_cur->next = c;
            else
                g_chunks = c;
            g_cur = c;
        }
    }
}

static void ar_reset(void)
{
    for (Chunk *c = g_chunks; c; c = c->next)
        c->used = 0;
    g_cur = g_chunks;
}

#define ALLOC(T) ((T *)ar_alloc(sizeof(T)))
#define ALLOC_N(T, n) ((T *)ar_alloc(sizeof(T) * (size_t)(n)))

/* ---------- atoms: interned property names and tag strings ---------- */

#define ATOM_LIST(X)                                                                   \
    X(A_LENGTH, "length") X(A_CONSTRUCTOR, "constructor")                              \
    X(A_FOREACH, "forEach") X(A_MAP, "map") X(A_FILTER, "filter") X(A_REDUCE, "reduce") \
    X(A_SORT, "sort") X(A_PUSH, "push") X(A_POP, "pop") X(A_SHIFT, "shift")          \
    X(A_UNSHIFT, "unshift") X(A_SPLICE, "splice") X(A_INDEXOF, "indexOf")             \
    X(A_INCLUDES, "includes") X(A_LASTINDEXOF, "lastIndexOf")                         \
    X(A_HAS, "has") X(A_ADD, "add") X(A_SET, "set") X(A_GET, "get")                   \
    X(A_DELETE, "delete") X(A_CLEAR, "clear") X(A_SIZE, "size") X(A_KEYS, "keys")     \
    X(A_VALUES, "values") X(A_ENTRIES, "entries") X(A_ITERATOR, "@@iterator")         \
    X(A_LISTS, "lists") X(A_ALLTIMECOMPLETEDTAGS, "allTimeCompletedTags")             \
    X(A_ALLTAGS, "allTags") X(A_COMPLETEDCOUNT, "completedRemidersCount")             \
    X(A_REMINDERSCOUNT, "remindersCount") X(A_POPULARTAGS, "popularTags")             \
    X(A_POPULARTAGSWC, "popularTagsWithCount") X(A_RENDER, "render")                  \
    X(A_NAME, "name") X(A_REMINDERS, "reminders") X(A_STORE, "store")                 \
    X(A__STORE, "_store") X(A_ISEMPTY, "isEmpty") X(A_TEXT, "text") X(A_TAGS, "tags") \
    X(A_ISCOMPLETED, "isCompleted") X(A__LIST, "_list")                               \
    X(A_INDEXINLIST, "indexInList") X(A__ISCOMPLETEDCHANGED, "_isCompletedChanged")   \
    X(A_MOVE, "moveToAnotherList") X(A_ITERATE_KEY, "ITERATE_KEY")                    \
    X(A_MAPITERATE_KEY, "MAP_KEY_ITERATE_KEY")

enum AtomId {
#define X(id, str) id,
    ATOM_LIST(X)
#undef X
    A__FIXED
};

typedef struct Atom {
    const char *name;
    uint32_t hash;
    int id; /* A_* for the fixed atoms, A__FIXED for interned strings */
} Atom;

static Atom *g_fixed[A__FIXED];
static Atom **g_atab;
static size_t g_acap, g_acount;

static uint32_t str_hash(const char *s, size_t n)
{
    uint32_t h = 2166136261u;
    for (size_t i = 0; i < n; i++) {
        h ^= (unsigned char)s[i];
        h *= 16777619u;
    }
    return h;
}

static void atab_insert(Atom *a)
{
    size_t mask = g_acap - 1;
    size_t i = a->hash & mask;
    while (g_atab[i])
        i = (i + 1) & mask;
    g_atab[i] = a;
}

static Atom *intern_n(const char *s, size_t n, int id)
{
    uint32_t h = str_hash(s, n);

    if ((g_acount + 1) * 2 > g_acap) {
        Atom **old = g_atab;
        size_t ocap = g_acap;
        g_acap = g_acap ? g_acap * 2 : 1024;
        g_atab = calloc(g_acap, sizeof *g_atab);
        if (!g_atab) {
            fputs("proxy-vue: out of memory\n", stderr);
            exit(2);
        }
        for (size_t j = 0; j < ocap; j++)
            if (old[j])
                atab_insert(old[j]);
        free(old);
    }
    if (id == A__FIXED) {
        size_t mask = g_acap - 1;
        size_t i = h & mask;
        while (g_atab[i]) {
            Atom *a = g_atab[i];
            if (a->hash == h && strlen(a->name) == n && memcmp(a->name, s, n) == 0)
                return a;
            i = (i + 1) & mask;
        }
    }
    {
        Atom *a = calloc(1, sizeof *a);
        char *copy = malloc(n + 1);
        if (!a || !copy) {
            fputs("proxy-vue: out of memory\n", stderr);
            exit(2);
        }
        memcpy(copy, s, n);
        copy[n] = 0;
        a->name = copy;
        a->hash = h;
        a->id = id;
        atab_insert(a);
        g_acount++;
        return a;
    }
}

static Atom *intern(const char *s) { return intern_n(s, strlen(s), A__FIXED); }

static void atoms_init(void)
{
    static const char *const names[] = {
#define X(id, str) str,
        ATOM_LIST(X)
#undef X
    };
    for (int i = 0; i < A__FIXED; i++) {
        g_fixed[i] = intern_n(names[i], strlen(names[i]), i);
        g_fixed[i]->id = i;
    }
}

/* ---------- keys: atom pointer, integer array index, or object pointer ---------- */

typedef uintptr_t Key;

#define KA(id) ((Key)g_fixed[(id)])
#define KIDX(i) ((((Key)(i)) << 1) | 1)
#define KEY_IS_INDEX(k) ((k) & 1)
#define KEY_IDX(k) ((uint32_t)((k) >> 1))

static inline int key_fixed_id(Key k)
{
    return KEY_IS_INDEX(k) ? -1 : ((Atom *)k)->id;
}

/* ---------- insertion-ordered hash map from uintptr_t to uintptr_t ---------- */

typedef struct HMap {
    uintptr_t *keys, *vals;
    uint32_t n, cap;
    uint32_t *slots; /* entry index + 1, 0 = empty; 2 * cap slots */
    uint32_t mask;
} HMap;

static inline uint32_t hmix(uintptr_t k)
{
    uint64_t x = (uint64_t)k;
    x ^= x >> 33;
    x *= UINT64_C(0xff51afd7ed558ccd);
    x ^= x >> 33;
    return (uint32_t)x;
}

static int hmap_find(const HMap *m, uintptr_t k, uintptr_t *out)
{
    if (!m->slots)
        return 0;
    uint32_t i = hmix(k) & m->mask;
    for (;;) {
        uint32_t s = m->slots[i];
        if (!s)
            return 0;
        if (m->keys[s - 1] == k) {
            *out = m->vals[s - 1];
            return 1;
        }
        i = (i + 1) & m->mask;
    }
}

static void hmap_put(HMap *m, uintptr_t k, uintptr_t v) /* k must be absent */
{
    if (m->n == m->cap) {
        uint32_t ncap = m->cap ? m->cap * 2 : 8;
        uintptr_t *nk = ALLOC_N(uintptr_t, ncap), *nv = ALLOC_N(uintptr_t, ncap);
        if (m->n) {
            memcpy(nk, m->keys, sizeof *nk * m->n);
            memcpy(nv, m->vals, sizeof *nv * m->n);
        }
        m->keys = nk;
        m->vals = nv;
        m->cap = ncap;
        m->slots = ALLOC_N(uint32_t, 2 * (size_t)ncap);
        m->mask = 2 * ncap - 1;
        for (uint32_t e = 0; e < m->n; e++) {
            uint32_t i = hmix(m->keys[e]) & m->mask;
            while (m->slots[i])
                i = (i + 1) & m->mask;
            m->slots[i] = e + 1;
        }
    }
    {
        uint32_t i = hmix(k) & m->mask;
        while (m->slots[i])
            i = (i + 1) & m->mask;
        m->keys[m->n] = k;
        m->vals[m->n] = v;
        m->n++;
        m->slots[i] = m->n;
    }
}

/* ---------- values ---------- */

typedef struct Raw Raw;
typedef struct Proxy Proxy;

typedef enum { V_UNDEF, V_HOLE, V_BOOL, V_NUM, V_STR, V_OBJ, V_PROXY, V_FUNC } VKind;

typedef struct Value {
    uint8_t k;
    union {
        double n;
        Atom *s;
        Raw *o;
        Proxy *p;
    } u;
} Value;

static inline Value vundef(void) { Value v = {V_UNDEF, {0}}; return v; }
static inline Value vhole(void) { Value v = {V_HOLE, {0}}; return v; }
static inline Value vfunc(void) { Value v = {V_FUNC, {0}}; return v; }
static inline Value vbool(int b) { Value v = {V_BOOL, {0}}; v.u.n = b ? 1 : 0; return v; }
static inline Value vnum(double d) { Value v = {V_NUM, {0}}; v.u.n = d; return v; }
static inline Value vstr(Atom *a) { Value v = {V_STR, {0}}; v.u.s = a; return v; }
static inline Value vobj(Raw *r) { Value v = {V_OBJ, {0}}; v.u.o = r; return v; }
static inline Value vproxy(Proxy *p) { Value v = {V_PROXY, {0}}; v.u.p = p; return v; }

static int same_value(Value a, Value b)
{
    if (a.k != b.k)
        return 0;
    switch (a.k) {
    case V_BOOL:
    case V_NUM:
        return a.u.n == b.u.n || (a.u.n != a.u.n && b.u.n != b.u.n);
    case V_STR:
        return a.u.s == b.u.s;
    case V_OBJ:
        return a.u.o == b.u.o;
    case V_PROXY:
        return a.u.p == b.u.p;
    default:
        return 1;
    }
}

static inline Value unhole(Value v) { return v.k == V_HOLE ? vundef() : v; }

/* ---------- raw objects: plain objects with props, arrays, Maps and Sets ---------- */

typedef enum { K_OBJECT, K_ARRAY, K_MAP, K_SET } Kind;
typedef enum { C_PLAIN, C_STORE, C_LIST, C_REMINDER } Cls;

typedef struct Prop {
    Atom *key;
    Value v;
} Prop;

/* Ordered collection: keys and values in insertion order, indexed by key. */
typedef struct Coll {
    HMap idx;     /* key -> entry index */
    Value *keys;  /* stored key values */
    Value *vals;
    uint32_t n, cap;
} Coll;

struct Raw {
    uint8_t kind, cls;
    Prop *props;
    uint32_t nprops, capprops;
    Value *elems; /* arrays; holes are V_HOLE */
    uint32_t len, cap;
    Coll coll;
};

struct Proxy {
    Raw *target;
};

static Raw *raw_new(Kind kind, Cls cls)
{
    Raw *r = ALLOC(Raw);
    r->kind = (uint8_t)kind;
    r->cls = (uint8_t)cls;
    return r;
}

static Key key_of(Value v)
{
    switch (v.k) {
    case V_STR:
        return (Key)v.u.s;
    case V_OBJ:
        return (Key)v.u.o;
    case V_PROXY:
        return (Key)v.u.p->target;
    default:
        fputs("proxy-vue: unsupported collection key\n", stderr);
        exit(2);
    }
}

static Value unwrap(Value v) { return v.k == V_PROXY ? vobj(v.u.p->target) : v; }

static int coll_find(const Coll *c, Key k, uint32_t *idx)
{
    uintptr_t e;
    if (!hmap_find(&c->idx, k, &e))
        return 0;
    *idx = (uint32_t)e;
    return 1;
}

static void coll_put(Coll *c, Key k, Value kv, Value v)
{
    uint32_t i;
    if (coll_find(c, k, &i)) {
        c->vals[i] = v;
        return;
    }
    if (c->n == c->cap) {
        uint32_t ncap = c->cap ? c->cap * 2 : 8;
        Value *nk = ALLOC_N(Value, ncap), *nv = ALLOC_N(Value, ncap);
        if (c->n) {
            memcpy(nk, c->keys, sizeof *nk * c->n);
            memcpy(nv, c->vals, sizeof *nv * c->n);
        }
        c->keys = nk;
        c->vals = nv;
        c->cap = ncap;
    }
    c->keys[c->n] = kv;
    c->vals[c->n] = v;
    hmap_put(&c->idx, k, c->n);
    c->n++;
}

static Value coll_get(const Coll *c, Key k)
{
    uint32_t i;
    return coll_find(c, k, &i) ? c->vals[i] : vundef();
}

static int coll_has(const Coll *c, Key k)
{
    uint32_t i;
    return coll_find(c, k, &i);
}

static int find_prop(const Raw *r, Atom *a)
{
    for (uint32_t i = 0; i < r->nprops; i++)
        if (r->props[i].key == a)
            return (int)i;
    return -1;
}

static void prop_set(Raw *r, Atom *a, Value v)
{
    int i = find_prop(r, a);
    if (i >= 0) {
        r->props[i].v = v;
        return;
    }
    if (r->nprops == r->capprops) {
        uint32_t ncap = r->capprops ? r->capprops * 2 : 4;
        Prop *np = ALLOC_N(Prop, ncap);
        if (r->nprops)
            memcpy(np, r->props, sizeof *np * r->nprops);
        r->props = np;
        r->capprops = ncap;
    }
    r->props[r->nprops].key = a;
    r->props[r->nprops].v = v;
    r->nprops++;
}

static void raw_arr_reserve(Raw *r, uint32_t n)
{
    if (n <= r->cap)
        return;
    uint32_t ncap = r->cap ? r->cap : 8;
    while (ncap < n)
        ncap *= 2;
    Value *ne = ALLOC_N(Value, ncap);
    if (r->len)
        memcpy(ne, r->elems, sizeof *ne * r->len);
    r->elems = ne;
    r->cap = ncap;
}

static void raw_arr_set(Raw *r, uint32_t i, Value v)
{
    if (i >= r->len) {
        raw_arr_reserve(r, i + 1);
        for (uint32_t j = r->len; j < i; j++)
            r->elems[j] = vhole();
        r->len = i + 1;
    }
    r->elems[i] = v;
}

static void raw_arr_setlen(Raw *r, uint32_t n)
{
    if (n > r->len) {
        raw_arr_reserve(r, n);
        for (uint32_t j = r->len; j < n; j++)
            r->elems[j] = vhole();
    }
    r->len = n;
}

static void raw_arr_del(Raw *r, uint32_t i)
{
    if (i < r->len)
        r->elems[i] = vhole();
}

static Raw *plain_array(void) { return raw_new(K_ARRAY, C_PLAIN); }

/* ---------- reactive core: targetMap, deps, effects, scheduler ---------- */

typedef struct Effect Effect;

typedef struct DepSet {
    Effect **items; /* effects in insertion order */
    uint32_t n, cap;
    uint32_t w;     /* bit set of runs that tracked this dep before */
    uint32_t nbits; /* bit set of runs that tracked this dep in this run */
} DepSet;

typedef enum { EF_RENDER, EF_WATCH } EKind;
typedef enum { SC_QUEUE, SC_SYNC } SKind;

struct Effect {
    uint8_t kind, sched, active, old_sentinel;
    Effect *parent;
    DepSet **deps;
    uint32_t ndeps, capdeps;
    Proxy *self;   /* render: the store; watch: the reminder (reactiveThis) */
    Value old;     /* watch: last value (p) */
};

enum { TR_SET, TR_ADD, TR_DELETE };

static struct {
    unsigned long long track, trigger, sched, run, flush, cb;
} st;

static HMap g_targets; /* raw target -> HMap of key -> DepSet (depsMap) */
static HMap g_proxies; /* raw target -> Proxy (proxyMap) */

static Effect *g_active;     /* activeEffect */
static int g_should_track = 1;
static uint32_t g_trackop = 1; /* 1 << depth */
static int g_depth;
static int g_pause[64];
static int g_pausen;

static Effect **g_q; /* the scheduler queue */
static uint32_t g_qn, g_qcap, g_ln;
static int g_on, g_sn; /* isFlushing, isFlushPending */

static Effect *effect_new(EKind kind, SKind sched, Proxy *self)
{
    Effect *e = ALLOC(Effect);
    e->kind = (uint8_t)kind;
    e->sched = (uint8_t)sched;
    e->active = 1;
    e->self = self;
    return e;
}

static void deps_push(Effect *e, DepSet *d)
{
    if (e->ndeps == e->capdeps) {
        uint32_t ncap = e->capdeps ? e->capdeps * 2 : 8;
        DepSet **nd = ALLOC_N(DepSet *, ncap);
        if (e->ndeps)
            memcpy(nd, e->deps, sizeof *nd * e->ndeps);
        e->deps = nd;
        e->capdeps = ncap;
    }
    e->deps[e->ndeps++] = d;
}

static void dep_add(DepSet *d, Effect *e)
{
    if (d->n == d->cap) {
        uint32_t ncap = d->cap ? d->cap * 2 : 4;
        Effect **ni = ALLOC_N(Effect *, ncap);
        if (d->n)
            memcpy(ni, d->items, sizeof *ni * d->n);
        d->items = ni;
        d->cap = ncap;
    }
    d->items[d->n++] = e;
}

static int dep_has(const DepSet *d, const Effect *e)
{
    for (uint32_t i = 0; i < d->n; i++)
        if (d->items[i] == e)
            return 1;
    return 0;
}

static void dep_remove(DepSet *d, Effect *e)
{
    uint32_t j = 0;
    for (uint32_t i = 0; i < d->n; i++)
        if (d->items[i] != e)
            d->items[j++] = d->items[i];
    d->n = j;
}

/* trackEffects: He() in the bundle, with the bit marks of the current run. */
static void track_effect(DepSet *d)
{
    int t;
    if (g_depth <= 30) {
        t = 0;
        if (!(d->nbits & g_trackop)) {
            d->nbits |= g_trackop;
            t = !(d->w & g_trackop);
        }
    } else {
        t = !dep_has(d, g_active);
    }
    if (t) {
        dep_add(d, g_active);
        deps_push(g_active, d);
    }
}

static HMap *deps_map_of(Raw *t, int create)
{
    uintptr_t v;
    if (hmap_find(&g_targets, (uintptr_t)t, &v))
        return (HMap *)v;
    if (!create)
        return NULL;
    {
        HMap *dm = ALLOC(HMap);
        hmap_put(&g_targets, (uintptr_t)t, (uintptr_t)dm);
        return dm;
    }
}

static DepSet *dep_of(HMap *dm, Key k, int create)
{
    uintptr_t v;
    if (hmap_find(dm, k, &v))
        return (DepSet *)v;
    if (!create)
        return NULL;
    {
        DepSet *d = ALLOC(DepSet);
        hmap_put(dm, k, (uintptr_t)d);
        return d;
    }
}

/* track(): the bundle's d(). Only tracking reads (a tracking effect is running) count. */
static void track(Raw *t, Key key)
{
    if (!g_should_track || !g_active)
        return;
    st.track++;
    track_effect(dep_of(deps_map_of(t, 1), key, 1));
}

static void ke(Effect *e);

/* ze(): run the effects of a snapshot, as the bundle copies a Set before iterating. */
static void run_effects(Effect **items, uint32_t n)
{
    Effect **snap = ALLOC_N(Effect *, n ? n : 1);
    memcpy(snap, items, sizeof *snap * n);
    for (uint32_t i = 0; i < n; i++)
        ke(snap[i]);
}

/* trigger(): the bundle's We(). */
static void trigger(Raw *t, int type, Key key, Value newv)
{
    HMap *dm = deps_map_of(t, 0);
    DepSet **n;
    uint32_t cnt = 0;

    if (!dm)
        return;
    st.trigger++;
    n = ALLOC_N(DepSet *, dm->n + 4);
    if (key == KA(A_LENGTH) && t->kind == K_ARRAY) {
        double newlen = newv.u.n;
        for (uint32_t i = 0; i < dm->n; i++) {
            Key k = dm->keys[i];
            if (k == KA(A_LENGTH) || (KEY_IS_INDEX(k) && (double)KEY_IDX(k) >= newlen))
                n[cnt++] = (DepSet *)dm->vals[i];
        }
    } else {
        n[cnt++] = dep_of(dm, key, 0);
        if (type == TR_ADD) {
            if (t->kind != K_ARRAY) {
                n[cnt++] = dep_of(dm, KA(A_ITERATE_KEY), 0);
                if (t->kind == K_MAP)
                    n[cnt++] = dep_of(dm, KA(A_MAPITERATE_KEY), 0);
            } else if (KEY_IS_INDEX(key)) {
                n[cnt++] = dep_of(dm, KA(A_LENGTH), 0);
            }
        } else if (type == TR_DELETE) {
            if (t->kind != K_ARRAY) {
                n[cnt++] = dep_of(dm, KA(A_ITERATE_KEY), 0);
                if (t->kind == K_MAP)
                    n[cnt++] = dep_of(dm, KA(A_MAPITERATE_KEY), 0);
            }
        } else if (t->kind == K_MAP) {
            n[cnt++] = dep_of(dm, KA(A_ITERATE_KEY), 0);
        }
    }

    if (cnt == 1) {
        if (n[0])
            run_effects(n[0]->items, n[0]->n);
        return;
    }
    {
        /* Ee(r): the effects of all deps, deduplicated in first-seen order */
        uint32_t total = 0, m = 0;
        Effect **r;
        for (uint32_t i = 0; i < cnt; i++)
            if (n[i])
                total += n[i]->n;
        r = ALLOC_N(Effect *, total ? total : 1);
        for (uint32_t i = 0; i < cnt; i++) {
            if (!n[i])
                continue;
            for (uint32_t j = 0; j < n[i]->n; j++) {
                Effect *e = n[i]->items[j];
                uint32_t k = 0;
                while (k < m && r[k] != e)
                    k++;
                if (k == m)
                    r[m++] = e;
            }
        }
        run_effects(r, m);
    }
}

/* ---------- string building for render() ---------- */

typedef struct SB {
    char *p;
    size_t len, cap;
} SB;

static void sb_put(SB *b, const char *s, size_t n)
{
    if (b->len + n + 1 > b->cap) {
        size_t ncap = b->cap ? b->cap : 256;
        while (ncap < b->len + n + 1)
            ncap *= 2;
        b->p = realloc(b->p, ncap);
        if (!b->p) {
            fputs("proxy-vue: out of memory\n", stderr);
            exit(2);
        }
        b->cap = ncap;
    }
    memcpy(b->p + b->len, s, n);
    b->len += n;
    b->p[b->len] = 0;
}

static void sb_s(SB *b, const char *s) { sb_put(b, s, strlen(s)); }
static void sb_c(SB *b, char c) { sb_put(b, &c, 1); }
static void sb_atom(SB *b, Atom *a) { sb_put(b, a->name, strlen(a->name)); }

static void sb_num(SB *b, double d)
{
    char tmp[32];
    snprintf(tmp, sizeof tmp, "%lld", (long long)d);
    sb_s(b, tmp);
}

static void sb_free(SB *b)
{
    free(b->p);
    b->p = NULL;
    b->len = b->cap = 0;
}

static void store_render(Proxy *s, SB *out);

/* ---------- effect run, scheduler queue, flush ---------- */

static Value effect_fn(Effect *h);

/* ReactiveEffect.run(). Returns the body's value. */
static Value effect_run(Effect *h)
{
    Effect *x;
    Value r;
    int saved;

    if (!h->active)
        return effect_fn(h);
    st.run++;
    for (x = g_active; x; x = x->parent)
        if (x == h)
            return vundef();
    h->parent = g_active;
    g_active = h;
    saved = g_should_track;
    g_should_track = 1;
    g_depth++;
    g_trackop = (uint32_t)1 << g_depth;
    if (g_depth <= 30) {
        for (uint32_t i = 0; i < h->ndeps; i++)
            h->deps[i]->w |= g_trackop;
    } else {
        fputs("proxy-vue: effect depth limit\n", stderr);
        exit(2);
    }

    r = effect_fn(h);

    if (g_depth <= 30) {
        uint32_t t = 0;
        for (uint32_t i = 0; i < h->ndeps; i++) {
            DepSet *o = h->deps[i];
            if ((o->w & g_trackop) && !(o->nbits & g_trackop))
                dep_remove(o, h);
            else
                h->deps[t++] = o;
            o->w &= ~g_trackop;
            o->nbits &= ~g_trackop;
        }
        h->ndeps = t;
    }
    g_depth--;
    g_trackop = (uint32_t)1 << g_depth;
    g_active = h->parent;
    g_should_track = saved;
    h->parent = NULL;
    return r;
}

static void job(Effect *e);

static void queue_job(Effect *j)
{
    uint32_t start = g_ln;
    if (g_qn) {
        for (uint32_t i = start; i < g_qn; i++)
            if (g_q[i] == j)
                return;
    }
    if (g_qn == g_qcap) {
        uint32_t ncap = g_qcap ? g_qcap * 2 : 16;
        Effect **nq = malloc(sizeof *nq * ncap);
        if (!nq) {
            fputs("proxy-vue: out of memory\n", stderr);
            exit(2);
        }
        if (g_qn)
            memcpy(nq, g_q, sizeof *nq * g_qn);
        free(g_q);
        g_q = nq;
        g_qcap = ncap;
    }
    g_q[g_qn++] = j;
    if (!g_on)
        g_sn = 1; /* queueFlush(): one flush is pending until the microtask runs */
}

/* Scheduler entry for an effect whose trigger ran: Ke() in the bundle. */
static void ke(Effect *e)
{
    if (e == g_active)
        return;
    st.sched++;
    if (e->sched == SC_QUEUE)
        queue_job(e);
    else
        job(e);
}

/* The job of an effect: the bundle's f. Renders run when active; watches compare and call back. */
static void watch_cb(Effect *e, Value v);

static void job(Effect *e)
{
    Value v;
    int changed;
    if (e->kind == EF_RENDER) {
        if (e->active)
            effect_run(e);
        return;
    }
    if (!e->active)
        return;
    v = effect_run(e);
    changed = e->old_sentinel || !same_value(v, e->old);
    if (changed) {
        st.cb++;
        watch_cb(e, v);
        e->old = v;
        e->old_sentinel = 0;
    }
}

/* flushJobs(): the awaited microtask of each test step. */
static void flush_jobs(void)
{
    g_sn = 0;
    g_on = 1;
    for (g_ln = 0; g_ln < g_qn; g_ln++) {
        st.flush++;
        job(g_q[g_ln]);
    }
    g_qn = 0;
    g_ln = 0;
    g_on = 0;
}

static void tick(void)
{
    if (g_sn)
        flush_jobs();
}

static void pause_tracking(void)
{
    g_pause[g_pausen++] = g_should_track;
    g_should_track = 0;
}

static void resume_tracking(void)
{
    g_should_track = g_pause[--g_pausen];
}

/* ---------- Proxy traps ---------- */

typedef Value (*Getter)(Proxy *self);

static Value getter_all_tags(Proxy *self);
static Value getter_completed_count(Proxy *self);
static Value getter_reminders_count(Proxy *self);
static Value getter_popular_tags(Proxy *self);
static Value getter_popular_with_count(Proxy *self);
static Value getter_list_store(Proxy *self);
static Value getter_list_is_empty(Proxy *self);
static Value getter_list_completed_count(Proxy *self);
static Value getter_index_in_list(Proxy *self);

/* Accessors on the classes (prototype getters); methods are below. */
static Getter class_getter(int cls, Atom *a)
{
    switch (cls) {
    case C_STORE:
        if (a == g_fixed[A_ALLTAGS]) return getter_all_tags;
        if (a == g_fixed[A_COMPLETEDCOUNT]) return getter_completed_count;
        if (a == g_fixed[A_REMINDERSCOUNT]) return getter_reminders_count;
        if (a == g_fixed[A_POPULARTAGS]) return getter_popular_tags;
        if (a == g_fixed[A_POPULARTAGSWC]) return getter_popular_with_count;
        break;
    case C_LIST:
        if (a == g_fixed[A_STORE]) return getter_list_store;
        if (a == g_fixed[A_ISEMPTY]) return getter_list_is_empty;
        if (a == g_fixed[A_COMPLETEDCOUNT]) return getter_list_completed_count;
        break;
    case C_REMINDER:
        if (a == g_fixed[A_INDEXINLIST]) return getter_index_in_list;
        break;
    default:
        break;
    }
    return NULL;
}

static int class_method(int cls, Atom *a)
{
    if (a == g_fixed[A_RENDER] && (cls == C_STORE || cls == C_LIST || cls == C_REMINDER))
        return 1;
    if (cls == C_REMINDER && (a == g_fixed[A_MOVE] || a == g_fixed[A__ISCOMPLETEDCHANGED]))
        return 1;
    return 0;
}

static int is_array_instrumented(int id)
{
    return id == A_PUSH || id == A_POP || id == A_SHIFT || id == A_UNSHIFT || id == A_SPLICE ||
           id == A_INDEXOF || id == A_INCLUDES || id == A_LASTINDEXOF;
}

static int is_collection_method(int id)
{
    return id == A_HAS || id == A_ADD || id == A_SET || id == A_GET || id == A_DELETE ||
           id == A_CLEAR || id == A_FOREACH || id == A_KEYS || id == A_VALUES ||
           id == A_ENTRIES || id == A_ITERATOR;
}

static int is_array_method(int id)
{
    return id == A_FOREACH || id == A_MAP || id == A_FILTER || id == A_REDUCE || id == A_SORT ||
           id == A_CONSTRUCTOR;
}

static Value wrap(Value v)
{
    uintptr_t p;
    Raw *r;
    if (v.k != V_OBJ)
        return v;
    r = v.u.o;
    if (hmap_find(&g_proxies, (uintptr_t)r, &p))
        return vproxy((Proxy *)p);
    {
        Proxy *px = ALLOC(Proxy);
        px->target = r;
        hmap_put(&g_proxies, (uintptr_t)r, (uintptr_t)px);
        return vproxy(px);
    }
}

static Proxy *proxy_of(Raw *r) { return wrap(vobj(r)).u.p; }

/* The get trap. Getters run with the proxy as receiver, then the key is tracked. */
static Value px_get(Proxy *p, Key key)
{
    Raw *t = p->target;
    Value v = vundef();
    int id = key_fixed_id(key);

    switch (t->kind) {
    case K_OBJECT: {
        Atom *a = (Atom *)key;
        int i = find_prop(t, a);
        Getter g;
        if (i >= 0) {
            v = t->props[i].v;
        } else if ((g = class_getter(t->cls, a)) != NULL) {
            v = g(p);
        } else if (class_method(t->cls, a)) {
            v = vfunc();
        }
        break;
    }
    case K_ARRAY:
        if (id == A_ITERATOR || (id >= 0 && is_array_instrumented(id)))
            return vfunc(); /* instrumentation or symbol: not tracked */
        if (KEY_IS_INDEX(key)) {
            uint32_t i = KEY_IDX(key);
            v = i < t->len ? unhole(t->elems[i]) : vundef();
        } else if (id == A_LENGTH) {
            v = vnum((double)t->len);
        } else if (id >= 0 && is_array_method(id)) {
            v = vfunc();
        }
        break;
    case K_MAP:
    case K_SET:
        if (id == A_SIZE) {
            /* size accessor: the instrumentation tracks ITERATE itself */
            track(t, KA(A_ITERATE_KEY));
            return vnum((double)t->coll.n);
        }
        if (id >= 0 && is_collection_method(id))
            return vfunc();
        break;
    }
    track(t, key);
    return wrap(v);
}

/* The set trap: Object.is check on the raw values, then Reflect.set, then trigger. */
static void px_set(Proxy *p, Key key, Value v)
{
    Raw *t = p->target;
    Value n = unwrap(v);
    Value o;
    int s;

    if (t->kind == K_ARRAY) {
        if (KEY_IS_INDEX(key)) {
            uint32_t i = KEY_IDX(key);
            s = i < t->len;
            o = s ? unhole(t->elems[i]) : vundef();
            raw_arr_set(t, i, n);
        } else {
            s = 1; /* length is an own property */
            o = vnum((double)t->len);
            raw_arr_setlen(t, (uint32_t)n.u.n);
        }
    } else {
        int i = find_prop(t, (Atom *)key);
        s = i >= 0;
        o = s ? t->props[i].v : vundef();
        prop_set(t, (Atom *)key, n);
    }
    o = unwrap(o);
    if (s) {
        if (!same_value(n, o))
            trigger(t, TR_SET, key, n);
    } else {
        trigger(t, TR_ADD, key, n);
    }
}

/* The deleteProperty trap (array holes only; the row deletes array indices). */
static void px_delete(Proxy *p, Key key)
{
    Raw *t = p->target;
    int had = KEY_IS_INDEX(key) && KEY_IDX(key) < t->len && t->elems[KEY_IDX(key)].k != V_HOLE;
    if (had) {
        raw_arr_del(t, KEY_IDX(key));
        trigger(t, TR_DELETE, key, vundef());
    }
}

/* The has trap: a membership check, then track. */
static int px_has(Proxy *p, Key key)
{
    Raw *t = p->target;
    int r = 0;
    int id = key_fixed_id(key);

    if (t->kind == K_ARRAY) {
        if (KEY_IS_INDEX(key))
            r = KEY_IDX(key) < t->len && t->elems[KEY_IDX(key)].k != V_HOLE;
        else
            r = id == A_LENGTH || (id >= 0 && (is_array_method(id) || is_array_instrumented(id)));
    } else if (t->kind == K_OBJECT) {
        Atom *a = (Atom *)key;
        r = find_prop(t, a) >= 0 || class_getter(t->cls, a) != NULL || class_method(t->cls, a);
    }
    track(t, key);
    return r;
}

/* ---------- generic array access: a Proxy receiver goes through the traps ---------- */

typedef struct AR {
    Raw *t;
    Proxy *p; /* NULL for plain arrays */
} AR;

static AR ar_of(Value v)
{
    AR a;
    if (v.k == V_PROXY) {
        a.p = v.u.p;
        a.t = v.u.p->target;
    } else {
        a.p = NULL;
        a.t = v.u.o;
    }
    return a;
}

static uint32_t ar_len(AR a)
{
    return a.p ? (uint32_t)px_get(a.p, KA(A_LENGTH)).u.n : a.t->len;
}

static int ar_has(AR a, uint32_t i)
{
    if (a.p)
        return px_has(a.p, KIDX(i));
    return i < a.t->len && a.t->elems[i].k != V_HOLE;
}

static Value ar_get(AR a, uint32_t i)
{
    if (a.p)
        return px_get(a.p, KIDX(i));
    return i < a.t->len ? unhole(a.t->elems[i]) : vundef();
}

static void ar_set(AR a, uint32_t i, Value v)
{
    if (a.p)
        px_set(a.p, KIDX(i), v);
    else
        raw_arr_set(a.t, i, unwrap(v));
}

static void ar_setlen(AR a, uint32_t n)
{
    if (a.p)
        px_set(a.p, KA(A_LENGTH), vnum((double)n));
    else
        raw_arr_setlen(a.t, n);
}

static void ar_del(AR a, uint32_t i)
{
    if (a.p)
        px_delete(a.p, KIDX(i));
    else
        raw_arr_del(a.t, i);
}

/* ArraySpeciesCreate reads the constructor of a reactive receiver. */
static void ar_species(AR a)
{
    if (a.p)
        (void)px_get(a.p, KA(A_CONSTRUCTOR));
}

/* Spec steps of push, pop, shift, unshift, splice and sort on any receiver. */
static uint32_t ar_push(AR a, const Value *items, uint32_t n)
{
    uint32_t len = ar_len(a);
    for (uint32_t j = 0; j < n; j++)
        ar_set(a, len++, items[j]);
    ar_setlen(a, len);
    return len;
}

static Value ar_pop(AR a)
{
    uint32_t len = ar_len(a);
    Value el;
    if (len == 0) {
        ar_setlen(a, 0);
        return vundef();
    }
    el = ar_get(a, len - 1);
    ar_del(a, len - 1);
    ar_setlen(a, len - 1);
    return el;
}

static Value ar_shift(AR a)
{
    uint32_t len = ar_len(a);
    Value first;
    if (len == 0) {
        ar_setlen(a, 0);
        return vundef();
    }
    first = ar_get(a, 0);
    for (uint32_t k = 1; k < len; k++) {
        if (ar_has(a, k))
            ar_set(a, k - 1, ar_get(a, k));
        else
            ar_del(a, k - 1);
    }
    ar_del(a, len - 1);
    ar_setlen(a, len - 1);
    return first;
}

static void ar_unshift(AR a, const Value *items, uint32_t n)
{
    uint32_t len = ar_len(a);
    if (n > 0) {
        for (uint32_t k = len; k > 0; k--) {
            uint32_t from = k - 1, to = k + n - 1;
            if (ar_has(a, from))
                ar_set(a, to, ar_get(a, from));
            else
                ar_del(a, to);
        }
        for (uint32_t j = 0; j < n; j++)
            ar_set(a, j, items[j]);
    }
    ar_setlen(a, len + n);
}

/* splice(start, deleteCount, ...items): returns the removed elements as a plain array. */
static Raw *ar_splice(AR a, uint32_t start, uint32_t dc, const Value *items, uint32_t ic)
{
    uint32_t len = ar_len(a);
    Raw *removed;

    ar_species(a);
    removed = plain_array();
    for (uint32_t k = 0; k < dc; k++)
        if (ar_has(a, start + k))
            raw_arr_set(removed, k, unwrap(ar_get(a, start + k)));
    raw_arr_setlen(removed, dc);

    if (ic < dc) {
        for (uint32_t k = start; k + dc < len; k++) {
            uint32_t from = k + dc, to = k + ic;
            if (ar_has(a, from))
                ar_set(a, to, ar_get(a, from));
            else
                ar_del(a, to);
        }
        for (uint32_t k = len; k > len - dc + ic; k--)
            ar_del(a, k - 1);
    } else if (ic > dc) {
        for (uint32_t k = len - dc; k > start; k--) {
            uint32_t from = k + dc - 1, to = k + ic - 1;
            if (ar_has(a, from))
                ar_set(a, to, ar_get(a, from));
            else
                ar_del(a, to);
        }
    }
    for (uint32_t j = 0; j < ic; j++)
        ar_set(a, start + j, items[j]);
    ar_setlen(a, len - dc + ic);
    return removed;
}

typedef double (*SortCmp)(Value x, Value y, void *ctx);

/* Stable sort of the present elements, then Set for each position and delete of the rest. */
static void ar_sort(AR a, SortCmp cmp, void *ctx)
{
    uint32_t len = ar_len(a), m = 0;
    Value *items = ALLOC_N(Value, len ? len : 1);

    for (uint32_t k = 0; k < len; k++)
        if (ar_has(a, k))
            items[m++] = ar_get(a, k);
    for (uint32_t i = 1; i < m; i++) {
        Value x = items[i];
        uint32_t j = i;
        while (j > 0 && cmp(items[j - 1], x, ctx) > 0) {
            items[j] = items[j - 1];
            j--;
        }
        items[j] = x;
    }
    for (uint32_t j = 0; j < m; j++)
        ar_set(a, j, items[j]);
    for (uint32_t j = m; j < len; j++)
        ar_del(a, j);
}

/* Array.prototype.filter: a plain result of the present elements that pass pred. */
typedef int (*Pred)(Value v, void *ctx);

static Raw *ar_filter(AR a, Pred pred, void *ctx)
{
    uint32_t len;
    Raw *out;

    ar_species(a);
    len = ar_len(a);
    out = plain_array();
    for (uint32_t k = 0; k < len; k++) {
        if (!ar_has(a, k))
            continue;
        {
            Value v = ar_get(a, k);
            if (pred(v, ctx))
                raw_arr_set(out, out->len, v);
        }
    }
    return out;
}

/* Receiver of an instrumented method: pauseTracking, the raw method on the proxy, resetTracking. */
static uint32_t px_push(Proxy *p, Value v)
{
    uint32_t n;
    pause_tracking();
    n = ar_push((AR){p->target, p}, &v, 1);
    resume_tracking();
    return n;
}

static Value px_pop(Proxy *p)
{
    Value v;
    pause_tracking();
    v = ar_pop((AR){p->target, p});
    resume_tracking();
    return v;
}

static Value px_shift(Proxy *p)
{
    Value v;
    pause_tracking();
    v = ar_shift((AR){p->target, p});
    resume_tracking();
    return v;
}

static void px_unshift(Proxy *p, Value v)
{
    pause_tracking();
    ar_unshift((AR){p->target, p}, &v, 1);
    resume_tracking();
}

static void px_splice(Proxy *p, uint32_t start, uint32_t dc)
{
    pause_tracking();
    (void)ar_splice((AR){p->target, p}, start, dc, NULL, 0);
    resume_tracking();
}

/* indexOf: the bundle's instrumented version tracks the length and every index, then searches. */
static int64_t px_index_of(Proxy *p, Value x)
{
    Raw *t = p->target;
    uint32_t len = (uint32_t)px_get(p, KA(A_LENGTH)).u.n;
    Value ux = unwrap(x);

    for (uint32_t i = 0; i < len; i++)
        track(t, KIDX(i));
    for (uint32_t i = 0; i < t->len; i++)
        if (t->elems[i].k != V_HOLE && same_value(t->elems[i], x))
            return i;
    for (uint32_t i = 0; i < t->len; i++)
        if (t->elems[i].k != V_HOLE && same_value(t->elems[i], ux))
            return i;
    return -1;
}

/* ---------- collections: Map and Set through the bundle's instrumentations ---------- */

typedef void (*CollCb)(Value k, Value v, void *ctx);

/* Iteration of a reactive Map or Set (forEach, spread, Array.from): tracks ITERATE first. */
static void coll_iter_px(Proxy *p, CollCb cb, void *ctx)
{
    Raw *t = p->target;
    track(t, KA(A_ITERATE_KEY));
    for (uint32_t i = 0; i < t->coll.n; i++) {
        Value k = t->coll.keys[i];
        Value v = t->kind == K_MAP ? t->coll.vals[i] : k;
        cb(wrap(k), wrap(v), ctx);
    }
}

static int coll_has_px(Proxy *p, Value x)
{
    Raw *t = p->target;
    Value o = unwrap(x);
    int same = same_value(x, o);
    int r;
    if (!same)
        track(t, key_of(x));
    track(t, key_of(o));
    r = same ? coll_has(&t->coll, key_of(o)) : (coll_has(&t->coll, key_of(x)) || coll_has(&t->coll, key_of(o)));
    return r;
}

/* Set.prototype.add through the instrumentation: add and trigger ADD when new. */
static void set_add_px(Proxy *p, Value x)
{
    Raw *t = p->target;
    Value e = unwrap(x);
    Key k = key_of(e);
    if (!coll_has(&t->coll, k)) {
        coll_put(&t->coll, k, e, vundef());
        trigger(t, TR_ADD, k, e);
    }
}

/* Map.prototype.get: tracks the key, returns the wrapped value. */
static Value map_get_px(Proxy *p, Value x)
{
    Raw *t = p->target;
    Value o = unwrap(x);
    if (!same_value(x, o))
        track(t, key_of(x));
    track(t, key_of(o));
    return wrap(coll_get(&t->coll, key_of(o)));
}

/* Map.prototype.set: reads the old value, stores, triggers SET or ADD. */
static void map_set_px(Proxy *p, Value k, Value v)
{
    Raw *t = p->target;
    Value nv = unwrap(v);
    Key key = key_of(k);
    int s = coll_has(&t->coll, key);
    Value old;
    if (!s)
        s = coll_has(&t->coll, key_of(unwrap(k)));
    old = coll_get(&t->coll, key);
    coll_put(&t->coll, key, unwrap(k), nv);
    if (s) {
        if (!same_value(nv, old))
            trigger(t, TR_SET, key, nv);
    } else {
        trigger(t, TR_ADD, key, nv);
    }
}

/* ---------- plain (untracked) collections, as the JS uses for its local Maps ---------- */

static Raw *plain_map(void) { return raw_new(K_MAP, C_PLAIN); }

/* ---------- the test model: store, lists, reminders ---------- */

static const char *const g_nouns[] = {"magazine", "passion", "statement", "extent", "judgment", "setting", "piano", "control", "employer", "speech", "presence", "indication", "philosophy", "worker", "supermarket", "atmosphere", "actor", "chest", "garbage", "feedback", "pizza", "speaker", "selection", "effort", "math", "gate", "profession", "recognition", "software", "coffee", "reading", "hotel", "concept", "poem", "population", "property", "office", "phone", "suggestion", "theory", "drawer", "death", "cigarette", "alcohol", "marketing", "session", "cheek", "establishment", "family", "revenue", "location", "mode", "awareness", "preference", "consequence", "cookie", "loss", "poetry", "cabinet", "difference", "story", "penalty", "king", "opinion", "climate", "imagination", "criticism", "meal", "desk", "dealer", "introduction", "physics", "customer", "child", "problem", "medicine", "church", "unit", "tongue", "communication", "storage", "stranger", "presentation", "discussion", "excitement", "role", "ladder", "estate", "town", "proposal", "employee", "administration", "variety", "television", "president", "inspector", "negotiation", "height", "comparison", "decision"};
static const char *const g_adjectives[] = {"fast", "cheerful", "hungry", "fantastic", "squealing", "critical", "yielding", "abashed", "shocking", "quickest"};

/* Expected per-iteration counts and final render (see tools/proxy-vue-validate.cjs --c). */
static const unsigned int g_expect[120][8] = {
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
    {15083, 636, 658, 144, 9, 134, 1433, 2001294488},
};


static Atom *g_noun_atoms[NOUN_COUNT];
static Atom *g_adj_atoms[ADJ_COUNT];

static uint32_t g_rng;

/* Math.random(): the same 32-bit mixing as JetStream's seeded generator. */
static double random01(void)
{
    uint32_t s = g_rng;
    s = (s + UINT32_C(0x7ed55d16)) + (s << 12);
    s = (s ^ UINT32_C(0xc761c23c)) ^ (s >> 19);
    s = (s + UINT32_C(0x165667b1)) + (s << 5);
    s = (s + UINT32_C(0xd3a2646c)) ^ (s << 9);
    s = (s + UINT32_C(0xfd7046c5)) + (s << 3);
    s = (s ^ UINT32_C(0xb55a4f09)) ^ (s >> 16);
    g_rng = s;
    return (double)s / 4294967296.0;
}

static time_t g_base; /* new Date() of the row: the clock all list dates derive from */

/* listName(index): Date.toDateString() of base + index days. */
static Atom *list_name(int index)
{
    char buf[32];
    struct tm tm;
    localtime_r(&g_base, &tm);
    tm.tm_mday += index;
    tm.tm_isdst = -1;
    mktime(&tm);
    strftime(buf, sizeof buf, "%a %b %d %Y", &tm);
    return intern(buf);
}

/* new ObservableReminderList(store, name) */
static Proxy *list_new(Proxy *store, Atom *name)
{
    Raw *r = raw_new(K_OBJECT, C_LIST);
    prop_set(r, g_fixed[A__STORE], vproxy(store));
    prop_set(r, g_fixed[A_NAME], vstr(name));
    prop_set(r, g_fixed[A_REMINDERS], vobj(plain_array()));
    return proxy_of(r);
}

static void reminder_move(Proxy *self, Proxy *newList);
static void reminder_is_completed_changed(Proxy *self, int completed);

static void watch_cb(Effect *e, Value v)
{
    (void)px_get(e->self, KA(A__ISCOMPLETEDCHANGED));
    reminder_is_completed_changed(e->self, v.u.n != 0);
}

/* The body of an effect: store.render() for the render, reactiveThis.isCompleted for a watch getter. */
static Value effect_fn(Effect *h)
{
    if (h->kind == EF_RENDER) {
        SB out = {0};
        (void)px_get(h->self, KA(A_RENDER));
        store_render(h->self, &out);
        sb_free(&out);
        return vundef();
    }
    return px_get(h->self, KA(A_ISCOMPLETED));
}

/* Set/Map size accessor: pt() tracks ITERATE. */
static double coll_size_px(Proxy *p)
{
    track(p->target, KA(A_ITERATE_KEY));
    return (double)p->target->coll.n;
}

/* ---------- class getters and methods ---------- */

/* Map of tag -> count for allTags: plain Map, no tracking. */
static void count_tag_plain(Value tag, Value unused, void *ctx)
{
    Raw *m = ctx;
    Value old = coll_get(&m->coll, key_of(tag));
    double c = old.k == V_NUM ? old.u.n : 0;
    (void)unused;
    coll_put(&m->coll, key_of(tag), tag, vnum(c + 1));
}

typedef void (*RemFn)(Proxy *reminder, void *ctx);

/* forEachReminder(store, cb): store.lists.forEach(list => list.reminders.forEach(cb)). */
static void each_reminder(Proxy *store, RemFn fn, void *ctx)
{
    AR lists = ar_of(px_get(store, KA(A_LISTS)));
    uint32_t n;

    (void)px_get(lists.p, KA(A_FOREACH));
    n = ar_len(lists);
    for (uint32_t i = 0; i < n; i++) {
        AR rems;
        uint32_t m;
        if (!ar_has(lists, i))
            continue;
        rems = ar_of(px_get(ar_get(lists, i).u.p, KA(A_REMINDERS)));
        (void)px_get(rems.p, KA(A_FOREACH));
        m = ar_len(rems);
        for (uint32_t j = 0; j < m; j++) {
            if (ar_has(rems, j))
                fn(ar_get(rems, j).u.p, ctx);
        }
    }
}

static void all_tags_reminder(Proxy *rem, void *ctx)
{
    Proxy *tags = px_get(rem, KA(A_TAGS)).u.p;
    coll_iter_px(tags, count_tag_plain, ctx); /* reminder.tags.forEach(...) */
}

static Value getter_all_tags(Proxy *self)
{
    Raw *m = plain_map();
    each_reminder(self, all_tags_reminder, m);
    return vobj(m);
}

static double num_of(Value v) { return v.k == V_NUM || v.k == V_BOOL ? v.u.n : 0; }

static Value getter_completed_count(Proxy *self)
{
    AR lists = ar_of(px_get(self, KA(A_LISTS)));
    double count = 0;
    uint32_t n;

    (void)px_get(lists.p, KA(A_REDUCE));
    n = ar_len(lists);
    for (uint32_t i = 0; i < n; i++) {
        if (ar_has(lists, i))
            count = count + num_of(px_get(ar_get(lists, i).u.p, KA(A_COMPLETEDCOUNT)));
    }
    return vnum(count);
}

static Value getter_reminders_count(Proxy *self)
{
    AR lists = ar_of(px_get(self, KA(A_LISTS)));
    double count = 0;
    uint32_t n;

    (void)px_get(lists.p, KA(A_REDUCE));
    n = ar_len(lists);
    for (uint32_t i = 0; i < n; i++) {
        if (ar_has(lists, i)) {
            Value rems = px_get(ar_get(lists, i).u.p, KA(A_REMINDERS));
            count = count + num_of(px_get(rems.u.p, KA(A_LENGTH)));
        }
    }
    return vnum(count);
}

/* [...map] collects [key, value] pairs into a plain array. */
static void collect_pair(Value k, Value v, void *ctx)
{
    Raw *out = ctx;
    Raw *pair = plain_array();
    raw_arr_set(pair, 0, k);
    raw_arr_set(pair, 1, v);
    raw_arr_set(out, out->len, vobj(pair));
}

/* (a, b) => b[1] - a[1] on plain pairs */
static double cmp_count_desc(Value a, Value b, void *ctx)
{
    (void)ctx;
    return num_of(ar_get(ar_of(b), 1)) - num_of(ar_get(ar_of(a), 1));
}

static Value getter_popular_with_count(Proxy *self)
{
    Value av = px_get(self, KA(A_ALLTAGS)); /* this.allTags: a reactive wrapper of the plain Map */
    Raw *pairs = plain_array();
    Raw *top = plain_array();

    coll_iter_px(av.u.p, collect_pair, pairs); /* [...this.allTags] */
    ar_sort(ar_of(vobj(pairs)), cmp_count_desc, NULL);
    for (uint32_t i = 0; i < pairs->len && i < 5; i++) /* .slice(0, 5) */
        raw_arr_set(top, i, pairs->elems[i]);
    return vobj(top);
}

static Value getter_popular_tags(Proxy *self)
{
    Value pv = px_get(self, KA(A_POPULARTAGSWC));
    AR a = ar_of(pv);
    Raw *out = plain_array();
    uint32_t n;

    (void)px_get(a.p, KA(A_MAP));
    ar_species(a);
    n = ar_len(a);
    for (uint32_t i = 0; i < n; i++) {
        if (ar_has(a, i)) {
            Value e = ar_get(a, i);
            raw_arr_set(out, out->len, ar_get(ar_of(e), 0)); /* e[0] */
        }
    }
    return vobj(out);
}

static Value getter_list_store(Proxy *self) { return px_get(self, KA(A__STORE)); }

static Value getter_list_is_empty(Proxy *self)
{
    AR rems = ar_of(px_get(self, KA(A_REMINDERS)));
    return vbool(px_get(rems.p, KA(A_LENGTH)).u.n == 0);
}

static int pred_completed(Value v, void *ctx)
{
    (void)ctx;
    return px_get(v.u.p, KA(A_ISCOMPLETED)).u.n != 0;
}

static Value getter_list_completed_count(Proxy *self)
{
    AR rems = ar_of(px_get(self, KA(A_REMINDERS)));
    Raw *f;

    (void)px_get(rems.p, KA(A_FILTER));
    f = ar_filter(rems, pred_completed, NULL);
    return vnum((double)f->len);
}

/* this._list.reminders.indexOf(this) */
static Value getter_index_in_list(Proxy *self)
{
    Proxy *list = px_get(self, KA(A__LIST)).u.p;
    AR rems = ar_of(px_get(list, KA(A_REMINDERS)));
    return vnum((double)px_index_of(rems.p, vproxy(self)));
}

/* reminder.moveToAnotherList(newList) */
static void reminder_move(Proxy *self, Proxy *newList)
{
    Proxy *lst = px_get(self, KA(A__LIST)).u.p;
    Proxy *rems = px_get(lst, KA(A_REMINDERS)).u.p;
    uint32_t idx = (uint32_t)px_get(self, KA(A_INDEXINLIST)).u.n;
    Proxy *nrems;

    px_splice(rems, idx, 1);
    px_set(self, KA(A__LIST), vproxy(newList));
    nrems = px_get(newList, KA(A_REMINDERS)).u.p;
    px_unshift(nrems, vproxy(self));
}

/* reminder._isCompletedChanged(isCompleted): copy the reminder's tags into the all-time counts. */
static void count_tag_reactive(Value tag, Value unused, void *ctx)
{
    Proxy *map = ctx;
    Value old = map_get_px(map, tag);
    double c = old.k == V_NUM ? old.u.n : 0;
    (void)unused;
    map_set_px(map, tag, vnum(c + 1));
}

static void reminder_is_completed_changed(Proxy *self, int completed)
{
    if (completed) {
        Proxy *list = px_get(self, KA(A__LIST)).u.p;
        Proxy *store = px_get(list, KA(A_STORE)).u.p;
        Proxy *tagsMap = px_get(store, KA(A_ALLTIMECOMPLETEDTAGS)).u.p;
        Proxy *tags = px_get(self, KA(A_TAGS)).u.p;
        coll_iter_px(tags, count_tag_reactive, tagsMap);
    }
}

/* ---------- render() ---------- */

/* renderTags(entries): entries.map(([tag, count]) => `#${tag} (${count})`).join(", ") || "<none>" */
static void pair_unpack(Value pv, Value *tag, Value *cnt)
{
    if (pv.k == V_PROXY) {
        Proxy *p = pv.u.p;
        (void)px_get(p, KA(A_LENGTH)); /* the array iterator reads length before each element */
        *tag = px_get(p, KIDX(0));
        (void)px_get(p, KA(A_LENGTH));
        *cnt = px_get(p, KIDX(1));
    } else {
        *tag = ar_get(ar_of(pv), 0);
        *cnt = ar_get(ar_of(pv), 1);
    }
}

static void render_tags(AR entries, SB *out)
{
    uint32_t n;
    uint32_t emitted = 0;

    if (entries.p)
        (void)px_get(entries.p, KA(A_MAP));
    ar_species(entries);
    n = ar_len(entries);
    for (uint32_t i = 0; i < n; i++) {
        Value tag, cnt;
        if (!ar_has(entries, i))
            continue;
        pair_unpack(ar_get(entries, i), &tag, &cnt);
        if (emitted)
            sb_s(out, ", ");
        sb_c(out, '#');
        sb_atom(out, tag.u.s);
        sb_s(out, " (");
        sb_num(out, cnt.u.n);
        sb_c(out, ')');
        emitted++;
    }
    if (!emitted)
        sb_s(out, "<none>");
}

/* Array.from(this.tags, tag => `#${tag}`).join(" ") */
struct TagJoin {
    SB *sb;
    int first;
};

static void join_hash_tag(Value tag, Value unused, void *ctx)
{
    struct TagJoin *j = ctx;
    (void)unused;
    if (!j->first)
        sb_c(j->sb, ' ');
    sb_c(j->sb, '#');
    sb_atom(j->sb, tag.u.s);
    j->first = 0;
}

/* Reminder.render(): `[x] text (#tag #tag)` */
static void reminder_render(Proxy *r, SB *out)
{
    SB tags = {0};
    double size;
    int done;
    Value text;

    size = coll_size_px(px_get(r, KA(A_TAGS)).u.p);
    if (size) {
        struct TagJoin j = {&tags, 1};
        coll_iter_px(px_get(r, KA(A_TAGS)).u.p, join_hash_tag, &j);
    }
    done = px_get(r, KA(A_ISCOMPLETED)).u.n != 0;
    text = px_get(r, KA(A_TEXT));
    sb_s(out, "[");
    sb_c(out, done ? 'x' : ' ');
    sb_s(out, "] ");
    sb_atom(out, text.u.s);
    if (size) {
        sb_s(out, " (");
        sb_put(out, tags.p, tags.len);
        sb_c(out, ')');
    }
    sb_free(&tags);
}

/* List.render() */
static void list_render(Proxy *l, SB *out)
{
    SB content = {0};
    AR rems;
    Value name;
    double completed, len;

    if (px_get(l, KA(A_ISEMPTY)).u.n != 0) {
        sb_s(&content, "<empty>");
    } else {
        uint32_t n;
        uint32_t emitted = 0;
        rems = ar_of(px_get(l, KA(A_REMINDERS)));
        (void)px_get(rems.p, KA(A_MAP));
        ar_species(rems);
        n = ar_len(rems);
        for (uint32_t i = 0; i < n; i++) {
            if (!ar_has(rems, i))
                continue;
            {
                Proxy *r = ar_get(rems, i).u.p;
                (void)px_get(r, KA(A_RENDER));
                if (emitted)
                    sb_c(&content, '\n');
                reminder_render(r, &content);
                emitted++;
            }
        }
    }
    name = px_get(l, KA(A_NAME));
    completed = px_get(l, KA(A_COMPLETEDCOUNT)).u.n;
    len = px_get(px_get(l, KA(A_REMINDERS)).u.p, KA(A_LENGTH)).u.n;
    sb_s(out, "--- ");
    sb_atom(out, name.u.s);
    sb_s(out, " (");
    sb_num(out, completed);
    sb_s(out, " / ");
    sb_num(out, len);
    sb_s(out, ") ---\n");
    sb_put(out, content.p ? content.p : "", content.len);
    sb_free(&content);
}

static void dashes(SB *out)
{
    for (int i = 0; i < 31; i++)
        sb_c(out, '-');
}

/* ReminderStore.render() */
static void store_render(Proxy *s, SB *out)
{
    AR lists = ar_of(px_get(s, KA(A_LISTS)));
    uint32_t n;
    uint32_t emitted = 0;

    (void)px_get(lists.p, KA(A_MAP)); /* this.lists.map(list => list.render()) */
    ar_species(lists);
    n = ar_len(lists);
    for (uint32_t i = 0; i < n; i++) {
        Proxy *list;
        if (!ar_has(lists, i))
            continue;
        list = ar_get(lists, i).u.p;
        (void)px_get(list, KA(A_RENDER));
        if (emitted)
            sb_c(out, '\n');
        list_render(list, out);
        emitted++;
    }
    if (!emitted)
        sb_s(out, "<empty>");
    sb_c(out, '\n');
    dashes(out);
    sb_c(out, '\n');
    {
        double completed = px_get(s, KA(A_COMPLETEDCOUNT)).u.n;
        double remaining = px_get(s, KA(A_REMINDERSCOUNT)).u.n;
        sb_s(out, "completed: ");
        sb_num(out, completed);
        sb_s(out, " / ");
        sb_num(out, remaining);
    }
    sb_c(out, '\n');
    dashes(out);
    sb_c(out, '\n');
    sb_s(out, "current popular tags: ");
    {
        AR pop = ar_of(px_get(s, KA(A_POPULARTAGSWC)));
        render_tags(pop, out);
    }
    sb_c(out, '\n');
    sb_s(out, "all-time completed tags: ");
    {
        Proxy *all = px_get(s, KA(A_ALLTIMECOMPLETEDTAGS)).u.p;
        Raw *entries = plain_array();
        coll_iter_px(all, collect_pair, entries); /* [...this.allTimeCompletedTags] */
        render_tags(ar_of(vobj(entries)), out);
    }
}

/* ---------- the test steps (the JS test(store, render) body) ---------- */

static void watch_reminder(Proxy *self)
{
    /* watch(() => reactiveThis.isCompleted, cb, { immediate: true, flush: "sync" }) */
    Effect *e = effect_new(EF_WATCH, SC_SYNC, self);
    e->old_sentinel = 1;
    job(e);
}

/* new ObservableReminder(list, text) */
static Proxy *reminder_new(Proxy *list, Atom *text)
{
    Raw *r = raw_new(K_OBJECT, C_REMINDER);
    Proxy *self;
    prop_set(r, g_fixed[A__LIST], vproxy(list));
    prop_set(r, g_fixed[A_TEXT], vstr(text));
    prop_set(r, g_fixed[A_TAGS], vobj(raw_new(K_SET, C_PLAIN)));
    prop_set(r, g_fixed[A_ISCOMPLETED], vbool(0));
    self = proxy_of(r);
    watch_reminder(self);
    return self;
}

/* new ObservableReminderList(store, name) */
static void create_lists(Proxy *store)
{
    for (int i = 0; i < LIST_COUNT; i++) {
        Proxy *lists = px_get(store, KA(A_LISTS)).u.p;
        Atom *name = list_name(i);
        Proxy *l = list_new(store, name);
        px_push(lists, vproxy(l));
    }
}

static void create_reminders(Proxy *store)
{
    AR lists = ar_of(px_get(store, KA(A_LISTS)));
    uint32_t n;

    (void)px_get(lists.p, KA(A_FOREACH));
    n = ar_len(lists);
    for (uint32_t li = 0; li < n; li++) {
        Proxy *list;
        if (!ar_has(lists, li))
            continue;
        list = ar_get(lists, li).u.p;
        for (int i = 0; i < REMINDERS_PER_LIST; i++) {
            Atom *text = g_noun_atoms[((int)li * 10 + i) % NOUN_COUNT];
            Proxy *rems = px_get(list, KA(A_REMINDERS)).u.p;
            Proxy *r = reminder_new(list, text);
            px_push(rems, vproxy(r));
        }
    }
}

static Atom *random_item(Atom **array, int len)
{
    return array[(int)(len * random01())];
}

static void add_random_tags_one(Proxy *r, void *ctx)
{
    (void)ctx;
    if (random01() < 0.5) {
        Proxy *tg = px_get(r, KA(A_TAGS)).u.p;
        Atom *a = random_item(g_adj_atoms, ADJ_COUNT);
        set_add_px(tg, vstr(a));
    }
    if (random01() < 0.5) {
        Proxy *tg = px_get(r, KA(A_TAGS)).u.p;
        Atom *a = random_item(g_adj_atoms, ADJ_COUNT);
        set_add_px(tg, vstr(a));
    }
}

static void push_back_with_tag(Proxy *store, Value tag)
{
    for (int64_t i = (int64_t)px_get(px_get(store, KA(A_LISTS)).u.p, KA(A_LENGTH)).u.n - 2; i >= 0; i--) {
        Proxy *list = px_get(px_get(store, KA(A_LISTS)).u.p, KIDX(i)).u.p;
        Proxy *next = px_get(px_get(store, KA(A_LISTS)).u.p, KIDX(i + 1)).u.p;
        for (int64_t j = (int64_t)px_get(px_get(list, KA(A_REMINDERS)).u.p, KA(A_LENGTH)).u.n - 1; j >= 0; j--) {
            Proxy *rem = px_get(px_get(list, KA(A_REMINDERS)).u.p, KIDX(j)).u.p;
            Proxy *tg = px_get(rem, KA(A_TAGS)).u.p;
            if (coll_has_px(tg, tag)) {
                (void)px_get(rem, KA(A_MOVE));
                reminder_move(rem, next);
            }
        }
    }
}

static int pred_without_tag_fn(Value v, void *ctx)
{
    Value *tag = ctx;
    Proxy *tg = px_get(v.u.p, KA(A_TAGS)).u.p;
    return !coll_has_px(tg, *tag);
}

static void remove_with_tag(Proxy *store, Value tag)
{
    AR lists = ar_of(px_get(store, KA(A_LISTS)));
    uint32_t n;

    (void)px_get(lists.p, KA(A_FOREACH));
    n = ar_len(lists);
    for (uint32_t i = 0; i < n; i++) {
        Proxy *list;
        AR rems;
        Raw *kept;
        if (!ar_has(lists, i))
            continue;
        list = ar_get(lists, i).u.p;
        rems = ar_of(px_get(list, KA(A_REMINDERS)));
        (void)px_get(rems.p, KA(A_FILTER));
        kept = ar_filter(rems, pred_without_tag_fn, &tag);
        px_set(list, KA(A_REMINDERS), vobj(kept));
    }
}

static int pred_not_completed(Value v, void *ctx)
{
    (void)ctx;
    return px_get(v.u.p, KA(A_ISCOMPLETED)).u.n == 0;
}

/* store.lists.forEach(list => { list.reminders.shift(); list.reminders.pop(); }) */
static void remove_first_and_last(Proxy *store)
{
    AR lists = ar_of(px_get(store, KA(A_LISTS)));
    uint32_t n;

    (void)px_get(lists.p, KA(A_FOREACH));
    n = ar_len(lists);
    for (uint32_t i = 0; i < n; i++) {
        Proxy *list;
        if (!ar_has(lists, i))
            continue;
        list = ar_get(lists, i).u.p;
        (void)px_shift(px_get(list, KA(A_REMINDERS)).u.p);
        (void)px_pop(px_get(list, KA(A_REMINDERS)).u.p);
    }
}

static void complete_one(Proxy *r, void *ctx)
{
    (void)ctx;
    px_set(r, KA(A_ISCOMPLETED), vbool(random01() < 0.5));
}

/* (a, b) => b.isCompleted - a.isCompleted */
static double cmp_completed_desc(Value a, Value b, void *ctx)
{
    (void)ctx;
    return num_of(px_get(b.u.p, KA(A_ISCOMPLETED))) - num_of(px_get(a.u.p, KA(A_ISCOMPLETED)));
}

static void sort_by_completed(Proxy *store)
{
    AR lists = ar_of(px_get(store, KA(A_LISTS)));
    uint32_t n;

    (void)px_get(lists.p, KA(A_FOREACH));
    n = ar_len(lists);
    for (uint32_t i = 0; i < n; i++) {
        AR rems;
        if (!ar_has(lists, i))
            continue;
        rems = ar_of(px_get(ar_get(lists, i).u.p, KA(A_REMINDERS)));
        (void)px_get(rems.p, KA(A_SORT));
        ar_sort(rems, cmp_completed_desc, NULL);
    }
}

static void remove_completed(Proxy *store)
{
    AR lists = ar_of(px_get(store, KA(A_LISTS)));
    uint32_t n;

    (void)px_get(lists.p, KA(A_FOREACH));
    n = ar_len(lists);
    for (uint32_t i = 0; i < n; i++) {
        Proxy *list;
        AR rems;
        Raw *kept;
        if (!ar_has(lists, i))
            continue;
        list = ar_get(lists, i).u.p;
        rems = ar_of(px_get(list, KA(A_REMINDERS)));
        (void)px_get(rems.p, KA(A_FILTER));
        kept = ar_filter(rems, pred_not_completed, NULL);
        px_set(list, KA(A_REMINDERS), vobj(kept));
    }
}

/* for (let i = store.lists.length - 1; i >= 0; i--) { if (store.lists[i].isEmpty) store.lists.splice(i, 1); } */
static void remove_empty_lists(Proxy *store)
{
    for (int64_t i = (int64_t)px_get(px_get(store, KA(A_LISTS)).u.p, KA(A_LENGTH)).u.n - 1; i >= 0; i--) {
        Proxy *list = px_get(px_get(store, KA(A_LISTS)).u.p, KIDX(i)).u.p;
        if (px_get(list, KA(A_ISEMPTY)).u.n != 0)
            px_splice(px_get(store, KA(A_LISTS)).u.p, (uint32_t)i, 1);
    }
}

/* The test body. Each `await render(fn)` runs fn and then the queued flush (tick). */
static void run_test(Proxy *store)
{
    Value popular;

    create_lists(store);
    tick();
    create_reminders(store);
    tick();
    each_reminder(store, add_random_tags_one, NULL);
    tick();

    popular = px_get(store, KA(A_POPULARTAGS)); /* const { popularTags } = store; */

    push_back_with_tag(store, ar_get(ar_of(popular), 0));
    tick();
    remove_with_tag(store, ar_get(ar_of(popular), 2));
    tick();
    remove_first_and_last(store);
    tick();
    each_reminder(store, complete_one, NULL);
    tick();
    sort_by_completed(store);
    tick();
    remove_completed(store);
    tick();
    remove_empty_lists(store);
    tick();
}

/* One Benchmark.runIteration(): a fresh store, its render watchEffect, the test, the final render. */
static void run_iteration(unsigned long long out[8])
{
    Raw *sr;
    Proxy *store;
    Effect *render;
    SB fresh = {0};

    ar_reset();
    memset(&g_targets, 0, sizeof g_targets);
    memset(&g_proxies, 0, sizeof g_proxies);
    memset(&st, 0, sizeof st);
    g_rng = RNG_SEED; /* Math.random.__resetSeed() */
    g_active = NULL;
    g_should_track = 1;
    g_depth = 0;
    g_trackop = 1;
    g_pausen = 0;
    g_qn = 0;
    g_ln = 0;
    g_on = 0;
    g_sn = 0;

    sr = raw_new(K_OBJECT, C_STORE);
    prop_set(sr, g_fixed[A_LISTS], vobj(plain_array()));
    prop_set(sr, g_fixed[A_ALLTIMECOMPLETEDTAGS], vobj(plain_map()));
    store = proxy_of(sr);

    render = effect_new(EF_RENDER, SC_QUEUE, store); /* watchEffect(() => store.render()) */
    effect_run(render);
    run_test(store);

    /* The final render, outside any effect: nothing is tracked. */
    store_render(store, &fresh);
    out[0] = st.track;
    out[1] = st.trigger;
    out[2] = st.sched;
    out[3] = st.run;
    out[4] = st.flush;
    out[5] = st.cb;
    out[6] = fresh.len;
    out[7] = str_hash(fresh.p, fresh.len);
    sb_free(&fresh);
}

/* The clock of the row: PROXY_VUE_DATE=YYYY-MM-DD pins it to noon of that day; default is now. */
static void setup_base_time(void)
{
    const char *env = getenv("PROXY_VUE_DATE");
    int y, m, d;

    if (env && sscanf(env, "%d-%d-%d", &y, &m, &d) == 3) {
        struct tm tm;
        memset(&tm, 0, sizeof tm);
        tm.tm_year = y - 1900;
        tm.tm_mon = m - 1;
        tm.tm_mday = d;
        tm.tm_hour = 12;
        tm.tm_isdst = -1;
        g_base = mktime(&tm);
    } else {
        g_base = time(NULL);
    }
}

static void words_init(void)
{
    for (int i = 0; i < NOUN_COUNT; i++)
        g_noun_atoms[i] = intern(g_nouns[i]);
    for (int i = 0; i < ADJ_COUNT; i++)
        g_adj_atoms[i] = intern(g_adjectives[i]);
}

int main(int argc, char **argv)
{
    int iterations = ITERATIONS;
    unsigned long long (*rows)[8];
    struct timespec t0, t1;
    double ms;
    struct tm base;
    int table_date;
    int ok = 1;

    if (argc > 1) {
        iterations = atoi(argv[1]);
        if (iterations < 1) {
            fprintf(stderr, "usage: %s [iterations [jetstream-root]]\n", argv[0]);
            return 2;
        }
    }
    /* argv[2], the JetStream root, is not read: the word lists and the expected table are compiled in. */

    setup_base_time();
    localtime_r(&g_base, &base);
    table_date = base.tm_year + 1900 == 2026 && base.tm_mon == 10 && base.tm_mday == 20;

    atoms_init();
    words_init();
    rows = malloc(sizeof *rows * (size_t)iterations);
    if (!rows) {
        fputs("proxy-vue: out of memory\n", stderr);
        return 2;
    }

    clock_gettime(CLOCK_MONOTONIC, &t0);
    for (int it = 0; it < iterations; it++)
        run_iteration(rows[it]);
    clock_gettime(CLOCK_MONOTONIC, &t1);
    ms = (double)(t1.tv_sec - t0.tv_sec) * 1000.0 + (double)(t1.tv_nsec - t0.tv_nsec) / 1e6;

    printf("proxy-vue %d iterations %.3f ms\n", iterations, ms);

    /* Counts are checked for every iteration (the first ITERATIONS of them). The render length and hash
     * depend on the list dates, so they are checked only when the clock is the table's date. */
    for (int it = 0; it < iterations && it < ITERATIONS && ok; it++) {
        const unsigned int *want = g_expect[it];
        int checked = table_date ? 8 : 6;
        for (int f = 0; f < checked; f++) {
            if (rows[it][f] != want[f]) {
                printf("validation FAILED: iteration %d field %d: got %llu, expected %u\n", it, f, rows[it][f], want[f]);
                ok = 0;
                break;
            }
        }
    }
    if (ok)
        printf("validation ok%s\n", table_date ? "" : " (counts; render hash not checked for this date)");
    free(rows);
    return ok ? 0 : 1;
}
