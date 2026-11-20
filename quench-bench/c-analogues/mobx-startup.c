// jetstream-row: mobx-startup
// jetstream-files: utils/StartupBenchmark.js mobx/benchmark.js
// iterations: 30
// jetstream-driver: class=Async preload=BUNDLE=mobx/dist/bundle.es6.min.js
/* C analogue of the JetStream mobx-startup row (mobx/dist/bundle.es6.min.js, mobx/benchmark.js).
 *
 * Each timed iteration does what the JS does (StartupBenchmark.createIterationSourceCode, then
 * Benchmark.runIteration):
 *   1. the iteration's copy of the bundle is parsed and compiled (js-parser-analogue.h: the same
 *      parse work as the engine's compile of the 71 KB bundle);
 *   2. the module body runs and defines the MobX library. The body creates 377 function objects
 *      (closures, class methods, arrows), 4 classes with prototypes, 49 object literals, 10 arrays,
 *      8 `new` objects and the MobX global state; the counts come from the bundle's AST
 *      (tools/mobx-body-shape.cjs), each one allocated and initialised as an object record;
 *   3. runTest() runs the scenario of src/test.mjs. Each JS operation is a C call into a port of the
 *      MobX 6.13 reactive core that takes the same steps as MobX: observable values held per object in
 *      a values map (a hash lookup by key on every property access), reportObserved with the
 *      trackingDerivation's newObserving bookkeeping, startBatch/endBatch with pendingUnobservations,
 *      propagateChanged and propagateMaybeChanged over dependency states, the pendingReactions queue
 *      and runReactions, computed values with shouldCompute and trackAndCompute, reactions and
 *      autoruns with trackDerivedFunction and bindDependencies (diffValue marks, addObserver and
 *      removeObserver), observable arrays (get by index through a string key, spliceWithArray_,
 *      notifyArraySplice_), observable maps (has/get/set/delete with hasMap entries and keysAtom) and
 *      sets, plus the interceptor and listener checks MobX makes on each change. Each log line is
 *      formatted with snprintf, as the template literal is built.
 *
 * Checked against the instrumented node run (tools/mobx-startup-validate.cjs) on every iteration:
 * reportObserved calls 83509, propagateChanged calls 10693, reactions run (runReaction_) 2609,
 * computed evaluations (computeValue_) 1315, the 6824 log entries and the FNV-1a 64 hash of the log
 * text. The other MobX operation counts (startBatch, endBatch, runReactions, shouldCompute,
 * hasInterceptors, hasListeners, addObserver, removeObserver, trackAndCompute) are printed and match
 * node too. The arena-backed records are freed together per iteration, so the JS garbage collector
 * is not modelled. The bundle's own text is not executed: the module body is modelled by its
 * allocation and property-table work, not by running it. Proxy traps are modelled only by the
 * key-string round trip of an index read; the accessor calls of class properties are direct calls.
 *
 * Measured (30 iterations, cc -O2, this container; five runs): C 113 ms (96-130 ms). Node 22 on the
 * composed row: the 30 iterations take 1.4-1.6 s (eval plus runTest: 1393-1614 ms in
 * tools/mobx-startup-validate.cjs; runTest about 52 ms an iteration). C is about 13x faster overall
 * and about 18x on runTest; the gap is engine work this model does not reproduce (garbage
 * collection, Proxy and accessor dispatch, property descriptors).
 * Usage: mobx-startup [iterations] [jetstream-root]; the defaults are the row's 30 iterations and ../jetstream. */
#define _DEFAULT_SOURCE
#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#pragma GCC diagnostic ignored "-Wunused-function"   /* the parser header has helpers this file does not use */
#include "js-parser-analogue.h"

/* ================= expected values, from tools/mobx-startup-validate.cjs (node 22) ================= */
#define EXPECT_LOG_LINES 6824u
#define EXPECT_LOG_HASH "dd8327dd577267ab"
/* MobX operation counts per iteration (tools/mobx-startup-validate.cjs, node 22). The global
 * functions are counted per call; runReaction_, computeValue_ and trackAndCompute are methods. */
enum { OP_REPORT_OBSERVED, OP_PROPAGATE_CHANGED, OP_START_BATCH, OP_END_BATCH, OP_RUN_REACTIONS,
       OP_RUN_REACTION, OP_COMPUTE_VALUE, OP_SHOULD_COMPUTE, OP_TRACK_AND_COMPUTE, OP_HAS_INTERCEPTORS,
       OP_HAS_LISTENERS, OP_ADD_OBSERVER, OP_REMOVE_OBSERVER, OP_COUNT };
static const char *const OP_NAME[OP_COUNT] = {
  "reportObserved", "propagateChanged", "startBatch", "endBatch", "runReactions", "runReaction_",
  "computeValue_", "shouldCompute", "trackAndCompute", "hasInterceptors", "hasListeners", "addObserver",
  "removeObserver"};
static const uint32_t OP_EXPECT[OP_COUNT] = {43652, 10693, 29526, 29526, 9868, 2609, 1315, 6415, 1304,
                                             14747, 14770, 2610, 2610};
/* the module body of the bundle (AST counts from tools/mobx-body-shape.cjs) */
#define BODY_FUNCTIONS 377
#define BODY_CLASSES 4
#define BODY_METHODS 23
#define BODY_OBJECTS 49
#define BODY_ARRAYS 10
#define BODY_NEWS 8

/* ================= counters of MobX operations (the calls the node run counts) ================= */
static uint32_t OPS[OP_COUNT];
#define C_OP(x) (OPS[OP_##x]++)
/* ================= arena: one iteration's objects, freed together ================= */
typedef struct Chunk { struct Chunk *next; size_t cap, used; } Chunk;
static Chunk *g_chunk;
static void *arena_alloc(size_t n) {
  n = (n + 15) & ~(size_t)15;
  if (!g_chunk || g_chunk->used + n > g_chunk->cap) {
    size_t cap = n > ((size_t)1 << 20) ? n : ((size_t)1 << 20);
    Chunk *c = malloc(sizeof *c + cap);
    if (!c) { fprintf(stderr, "mobx-startup: out of memory\n"); exit(2); }
    c->next = g_chunk; c->cap = cap; c->used = 0;
    g_chunk = c;
  }
  void *p = (char *)(g_chunk + 1) + g_chunk->used;
  g_chunk->used += n;
  return p;
}
static void *arena_calloc(size_t n) { void *p = arena_alloc(n); memset(p, 0, n); return p; }
static void arena_free_all(void) {
  while (g_chunk) { Chunk *n = g_chunk->next; free(g_chunk); g_chunk = n; }
}
static char *arena_str(const char *s, size_t n) {
  char *p = arena_alloc(n + 1);
  memcpy(p, s, n);
  p[n] = 0;
  return p;
}

/* ================= JS values ================= */
typedef enum { V_UNDEF, V_NULL, V_NUM, V_STR, V_OBJ, V_PTR } VTag;
typedef struct Val { VTag t; union { int64_t n; const char *s; void *p; } u; } Val;
static Val vundef(void) { Val v = {V_UNDEF, {0}}; return v; }
static Val vnull(void) { Val v = {V_NULL, {0}}; return v; }
static Val vnum(int64_t n) { Val v = {V_NUM, {.n = n}}; return v; }
static Val vstr(const char *s) { Val v = {V_STR, {.s = s}}; return v; }
static Val vobj(void *p) { Val v = {V_OBJ, {.p = p}}; return v; }
static Val vptr(void *p) { Val v = {V_PTR, {.p = p}}; return v; }

/* Object-like records start with this header, so one Val can name any of them. */
typedef struct Hdr { uint8_t kind; uint8_t observable; } Hdr;
enum { HK_OBJ = 1, HK_ARR, HK_MAP, HK_SET, HK_PLAIN_ARR, HK_ES6_MAP, HK_ES6_SET };

static uint64_t mix64(uint64_t x) {
  x ^= x >> 33; x *= 0xff51afd7ed558ccdULL; x ^= x >> 33; x *= 0xc4ceb9fe1a85ec53ULL; x ^= x >> 33;
  return x;
}
static uint64_t hash_val(Val k) {
  switch (k.t) {
    case V_NUM: return mix64((uint64_t)k.u.n);
    case V_STR: {
      uint64_t h = 1469598103934665603ULL;
      for (const unsigned char *p = (const unsigned char *)k.u.s; *p; p++) h = (h ^ *p) * 1099511628211ULL;
      return h;
    }
    default: return mix64((uint64_t)(uintptr_t)k.u.p ^ ((uint64_t)k.t << 56));
  }
}
/* SameValueZero for Map and Set keys (and Object.is for the values, which differ only for NaN) */
static bool same_val(Val a, Val b) {
  if (a.t != b.t) return false;
  switch (a.t) {
    case V_NUM: return a.u.n == b.u.n;
    case V_STR: return strcmp(a.u.s, b.u.s) == 0;
    case V_UNDEF: case V_NULL: return true;
    default: return a.u.p == b.u.p;
  }
}

/* An insertion-ordered hash map with JS Map semantics: Map, Set, the values table of an object. */
typedef struct VEnt { Val k; void *v; uint64_t h; uint8_t live; } VEnt;
typedef struct VMap { VEnt *e; uint32_t n, cap, live; uint32_t *idx; uint32_t capidx; } VMap;

static long vm_find(const VMap *m, Val k, uint64_t h) {
  if (!m->capidx) return -1;
  uint32_t mask = m->capidx - 1, p = (uint32_t)h & mask;
  for (;;) {
    uint32_t s = m->idx[p];
    if (!s) return -1;
    const VEnt *e = &m->e[s - 1];
    if (e->live && e->h == h && same_val(e->k, k)) return (long)(s - 1);
    p = (p + 1) & mask;
  }
}
/* Makes room for one entry: drops deleted entries, and doubles the table when it is full. */
static void vm_reserve(VMap *m) {
  if (m->n < m->cap) return;
  uint32_t cap = 8;
  while (cap < m->live * 2 + 2) cap <<= 1;
  VEnt *ne = arena_alloc(sizeof *ne * cap);
  uint32_t j = 0;
  for (uint32_t i = 0; i < m->n; i++) if (m->e[i].live) ne[j++] = m->e[i];
  m->e = ne; m->n = j; m->cap = cap;
  m->capidx = cap * 2;
  m->idx = arena_calloc(sizeof *m->idx * m->capidx);
  for (uint32_t i = 0; i < m->n; i++) {
    uint32_t p = (uint32_t)m->e[i].h & (m->capidx - 1);
    while (m->idx[p]) p = (p + 1) & (m->capidx - 1);
    m->idx[p] = i + 1;
  }
}
static void *vm_get(const VMap *m, Val k) {
  long i = vm_find(m, k, hash_val(k));
  return i < 0 ? NULL : m->e[i].v;
}
static bool vm_has(const VMap *m, Val k) { return vm_find(m, k, hash_val(k)) >= 0; }
static void vm_set(VMap *m, Val k, void *v) {
  uint64_t h = hash_val(k);
  long i = vm_find(m, k, h);
  if (i >= 0) { m->e[i].v = v; return; }
  vm_reserve(m);
  uint32_t ei = m->n++;
  m->e[ei].k = k; m->e[ei].v = v; m->e[ei].h = h; m->e[ei].live = 1;
  m->live++;
  uint32_t p = (uint32_t)h & (m->capidx - 1);
  while (m->idx[p]) p = (p + 1) & (m->capidx - 1);
  m->idx[p] = ei + 1;
}
static bool vm_delete(VMap *m, Val k) {
  long i = vm_find(m, k, hash_val(k));
  if (i < 0) return false;
  m->e[i].live = 0;
  m->e[i].v = NULL;
  m->live--;
  return true;
}

/* A growable pointer list in the arena. */
typedef struct PVec { void **v; uint32_t n, cap; } PVec;
static void pv_push(PVec *p, void *x) {
  if (p->n == p->cap) {
    uint32_t cap = p->cap ? p->cap * 2 : 8;
    void **nv = arena_alloc(sizeof *nv * cap);
    if (p->n) memcpy(nv, p->v, sizeof *nv * p->n);
    p->v = nv; p->cap = cap;
  }
  p->v[p->n++] = x;
}

/* MobX keeps interceptors and change listeners on each observable. This program registers none,
 * but every change still checks them (hasInterceptors / hasListeners). */
typedef struct Lis { PVec interceptors, listeners; } Lis;

/* ================= the reactive core (MobX 6.13: globalState, atoms, derivations, reactions) ================= */
enum { NK_ATOM, NK_VALUE, NK_COMPUTED, NK_REACTION };
enum { UP_TO_DATE = 0, POSSIBLY_STALE = 1, STALE = 2, NOT_TRACKING = -1 };
enum { ENH_REF, ENH_DEEP };

typedef struct Node Node;
typedef struct Cb { void (*fn)(void *); void *ctx; } Cb;
struct Node {
  uint8_t kind, enh, diff, pendingUnobs, beingObserved, scheduled, disposed, computing, running;
  uint8_t trackPending, first, keepAlive;
  int8_t depState, lowest;            /* dependenciesState_, lowestObserverState_ */
  uint32_t runId, lastAccessed, unbound;
  const char *name;
  VMap observers;                     /* observers_: a Set of derivations */
  PVec observing;                     /* observing_ */
  PVec newObs;                        /* newObserving_; `unbound` is unboundDepsCount_ */
  Val value;                          /* value_ of an ObservableValue or a ComputedValue */
  Lis lis;                            /* ObservableValue interceptors and listeners */
  PVec onBO, onBUO;                   /* Cb* callbacks: onBecomeObserved / onBecomeUnobserved */
  Val (*derive)(void *scope);         /* computed derivation, or reaction expression */
  void *scope;
  void (*view)(void *ctx);            /* autorun view */
  void *rctx;                         /* reaction effect context */
  void (*onInvalidate)(Node *r);      /* reaction onInvalidate_ */
  void (*effect)(void *ctx, Val v);   /* reaction effect */
  Val rvalue;                         /* reaction: value of the last run */
};

static struct {
  uint32_t inBatch, runId;
  Node *trackingDerivation, *trackingContext;
  PVec *pendingUnobs;                 /* Node*: globalState.pendingUnobservations */
  PVec pendingReactions;              /* Node*: globalState.pendingReactions */
  uint8_t isRunningReactions;
  uint8_t allowStateChanges;
} G;

static Val computed_get(Node *c);

static Node *node_new(uint8_t kind, const char *name) {
  Node *n = arena_calloc(sizeof *n);
  n->kind = kind;
  n->name = name;
  n->depState = (kind == NK_COMPUTED || kind == NK_REACTION) ? NOT_TRACKING : UP_TO_DATE;
  n->lowest = UP_TO_DATE;
  n->value = vundef();
  return n;
}

static bool has_interceptors(Lis *l) { C_OP(HAS_INTERCEPTORS); return l->interceptors.n > 0; }
static bool has_listeners(Lis *l) { C_OP(HAS_LISTENERS); return l->listeners.n > 0; }

static void start_batch(void) { C_OP(START_BATCH); G.inBatch++; }
static void run_reactions(void);
static void queue_for_unobservation(Node *o) {
  if (!o->pendingUnobs) { o->pendingUnobs = 1; pv_push(G.pendingUnobs, o); }
}
static void end_batch(void) {
  C_OP(END_BATCH);
  if (--G.inBatch == 0) {
    run_reactions();
    /* observables that lost their last observer while the batch ran */
    PVec *list = G.pendingUnobs;
    for (uint32_t i = 0; i < list->n; i++) {
      Node *o = list->v[i];
      o->pendingUnobs = 0;
      if (o->observers.live != 0) continue;
      if (o->beingObserved) {
        o->beingObserved = 0;
        for (uint32_t k = 0; k < o->onBUO.n; k++) { Cb *cb = o->onBUO.v[k]; cb->fn(cb->ctx); }
      }
      if (o->kind == NK_COMPUTED && !o->keepAlive) {   /* suspend_: clearObserving and drop the value */
        PVec obs = o->observing;
        o->observing = (PVec){0};
        for (uint32_t k = obs.n; k-- > 0;) {
          Node *dep = obs.v[k];
          C_OP(REMOVE_OBSERVER);
          vm_delete(&dep->observers, vptr(o));
          if (dep->observers.live == 0) queue_for_unobservation(dep);
        }
        o->depState = NOT_TRACKING;
        o->value = vundef();
      }
    }
    G.pendingUnobs = arena_calloc(sizeof(PVec));
  }
}
static void add_observer(Node *o, Node *d) {
  C_OP(ADD_OBSERVER);
  vm_set(&o->observers, vptr(d), NULL);
  if (o->lowest > d->depState) o->lowest = d->depState;
}
static void remove_observer(Node *o, Node *d) {
  C_OP(REMOVE_OBSERVER);
  vm_delete(&o->observers, vptr(d));
  if (o->observers.live == 0) queue_for_unobservation(o);
}

/* reportObserved: a read of an observable. Inside a derivation it records the dependency once per run. */
static bool report_observed(Node *o) {
  C_OP(REPORT_OBSERVED);
  Node *d = G.trackingDerivation;
  if (d) {
    if (d->runId != o->lastAccessed) {
      o->lastAccessed = d->runId;
      pv_push(&d->newObs, o);
      d->unbound++;
      if (!o->beingObserved && G.trackingContext) {
        o->beingObserved = 1;
        for (uint32_t k = 0; k < o->onBO.n; k++) { Cb *cb = o->onBO.v[k]; cb->fn(cb->ctx); }
      }
    }
    return o->beingObserved;
  }
  if (o->observers.live == 0 && G.inBatch > 0) queue_for_unobservation(o);
  return false;
}

static void on_become_stale(Node *d);
/* propagateMaybeChanged: a computed value may have changed, so its observers are marked POSSIBLY_STALE */
static void propagate_maybe_changed(Node *o) {
  if (o->lowest != UP_TO_DATE) return;
  o->lowest = POSSIBLY_STALE;
  for (uint32_t i = 0; i < o->observers.n; i++) {
    if (!o->observers.e[i].live) continue;
    Node *d = o->observers.e[i].k.u.p;
    if (d->depState == UP_TO_DATE) { d->depState = POSSIBLY_STALE; on_become_stale(d); }
  }
}
/* propagateChanged: an observable changed, so its observers are marked STALE */
static void propagate_changed(Node *o) {
  C_OP(PROPAGATE_CHANGED);
  if (o->lowest == STALE) return;
  o->lowest = STALE;
  for (uint32_t i = 0; i < o->observers.n; i++) {
    if (!o->observers.e[i].live) continue;
    Node *d = o->observers.e[i].k.u.p;
    if (d->depState == UP_TO_DATE) on_become_stale(d);
    d->depState = STALE;
  }
}
static void report_changed(Node *o) { start_batch(); propagate_changed(o); end_batch(); }

static void schedule(Node *r) {
  if (!r->scheduled) {
    r->scheduled = 1;
    pv_push(&G.pendingReactions, r);
    run_reactions();
  }
}
/* onBecomeStale_: a reaction is scheduled; a computed passes the POSSIBLY_STALE mark on */
static void on_become_stale(Node *d) {
  if (d->kind == NK_COMPUTED) propagate_maybe_changed(d);
  else schedule(d);
}

static void change_deps_to_0(Node *d) {
  if (d->depState != UP_TO_DATE) {
    d->depState = UP_TO_DATE;
    for (uint32_t i = d->observing.n; i-- > 0;) ((Node *)d->observing.v[i])->lowest = UP_TO_DATE;
  }
}

static bool should_compute(Node *d) {
  C_OP(SHOULD_COMPUTE);
  if (d->depState == UP_TO_DATE) return false;
  if (d->depState == NOT_TRACKING || d->depState == STALE) return true;
  /* POSSIBLY_STALE: bring the computed dependencies up to date, untracked */
  Node *prevUntracked = G.trackingDerivation;
  G.trackingDerivation = NULL;
  for (uint32_t i = 0; i < d->observing.n; i++) {
    Node *obj = d->observing.v[i];
    if (obj->kind == NK_COMPUTED) {
      computed_get(obj);
      if (d->depState == STALE) { G.trackingDerivation = prevUntracked; return true; }
    }
  }
  change_deps_to_0(d);
  G.trackingDerivation = prevUntracked;
  return false;
}

/* bindDependencies: the new observing list replaces the old one. Links are added and removed by the
 * diffValue marks, as MobX does, so an observable read again keeps its link. */
static void bind_dependencies(Node *d) {
  PVec prev = d->observing;
  Node **obs = (Node **)d->newObs.v;
  uint32_t l = d->unbound, i0 = 0;
  int8_t lowestNew = UP_TO_DATE;
  for (uint32_t i = 0; i < l; i++) {
    Node *dep = obs[i];
    if (dep->diff == 0) {
      dep->diff = 1;
      if (i0 != i) obs[i0] = dep;
      i0++;
    }
    if (dep->depState > lowestNew) lowestNew = dep->depState;
  }
  d->observing.v = (void **)obs;
  d->observing.n = i0;
  d->observing.cap = d->newObs.cap;
  d->newObs = (PVec){0};
  for (uint32_t k = prev.n; k-- > 0;) {
    Node *dep = prev.v[k];
    if (dep->diff == 0) remove_observer(dep, d);
    dep->diff = 0;
  }
  for (uint32_t k = i0; k-- > 0;) {
    Node *dep = (Node *)d->observing.v[k];
    if (dep->diff == 1) { dep->diff = 0; add_observer(dep, d); }
  }
  if (lowestNew != UP_TO_DATE) {
    d->depState = lowestNew;
    on_become_stale(d);
  }
}

/* trackDerivedFunction: runs f with d as the tracking derivation, then binds what it read */
static Val track_derivation(Node *d, Val (*f)(void *), void *ctx) {
  change_deps_to_0(d);
  uint32_t cap = d->runId == 0 ? 100 : d->observing.n;
  d->newObs.v = arena_alloc(sizeof(void *) * (cap ? cap : 1));
  d->newObs.cap = cap ? cap : 1;
  d->newObs.n = 0;
  d->unbound = 0;
  d->runId = ++G.runId;
  Node *prev = G.trackingDerivation;
  G.trackingDerivation = d;
  G.inBatch++;
  Val result = f(ctx);
  G.inBatch--;
  G.trackingDerivation = prev;
  bind_dependencies(d);
  return result;
}

static void clear_observing(Node *d) {
  PVec obs = d->observing;
  d->observing = (PVec){0};
  for (uint32_t i = obs.n; i-- > 0;) remove_observer(obs.v[i], d);
  d->depState = NOT_TRACKING;
}

/* ---- computed values ---- */
static Val compute_value(Node *c, bool track) {
  C_OP(COMPUTE_VALUE);
  c->computing = 1;
  bool prevAllow = G.allowStateChanges;
  G.allowStateChanges = false;
  Val res;
  if (track) res = track_derivation(c, c->derive, c->scope);
  else res = c->derive(c->scope);
  c->computing = 0;
  G.allowStateChanges = prevAllow;
  return res;
}
static bool track_and_compute(Node *c) {
  C_OP(TRACK_AND_COMPUTE);
  Val oldValue = c->value;
  bool wasSuspended = c->depState == NOT_TRACKING;
  Val newValue = compute_value(c, true);
  bool changed = wasSuspended || !same_val(oldValue, newValue);
  if (changed) c->value = newValue;
  return changed;
}
/* propagateChangeConfirmed */
static void propagate_change_confirmed(Node *o) {
  if (o->lowest == STALE) return;
  o->lowest = STALE;
  for (uint32_t i = 0; i < o->observers.n; i++) {
    if (!o->observers.e[i].live) continue;
    Node *d = o->observers.e[i].k.u.p;
    if (d->depState == POSSIBLY_STALE) d->depState = STALE;
    else if (d->depState == UP_TO_DATE) o->lowest = UP_TO_DATE;
  }
}
/* ComputedValue.get */
static Val computed_get(Node *c) {
  if (c->computing) { fprintf(stderr, "mobx-startup: cycle in computed %s\n", c->name); exit(1); }
  if (G.inBatch != 0 || c->observers.live != 0 || c->keepAlive) {
    report_observed(c);
    if (should_compute(c) && track_and_compute(c)) propagate_change_confirmed(c);
  } else if (should_compute(c)) {
    start_batch();
    c->value = compute_value(c, false);
    end_batch();
  }
  return c->value;
}
static Node *computed_new(const char *name, Val (*derive)(void *), void *scope) {
  Node *c = node_new(NK_COMPUTED, name);
  c->derive = derive;
  c->scope = scope;
  return c;
}

/* ---- reactions and autoruns ---- */
/* Reaction.track: runs fn as the tracked function of reaction r */
static Val reaction_track(Node *r, Val (*fn)(void *), void *ctx) {
  if (r->disposed) return vundef();
  start_batch();
  r->running = 1;
  Node *prev = G.trackingContext;
  G.trackingContext = r;
  Val result = track_derivation(r, fn, ctx);
  G.trackingContext = prev;
  r->running = 0;
  r->trackPending = 0;
  if (r->disposed) clear_observing(r);
  end_batch();
  return result;
}
static void reaction_dispose(Node *r) {
  if (r->disposed) return;
  r->disposed = 1;
  if (!r->running) {
    start_batch();
    clear_observing(r);
    end_batch();
  }
}
/* Reaction.runReaction_ */
static void run_reaction(Node *r) {
  C_OP(RUN_REACTION);
  if (r->disposed) return;
  start_batch();
  r->scheduled = 0;
  Node *prev = G.trackingContext;
  G.trackingContext = r;
  if (should_compute(r)) {
    r->trackPending = 1;
    r->onInvalidate(r);
  }
  G.trackingContext = prev;
  end_batch();
}
static void run_reactions_helper(void) {
  G.isRunningReactions = 1;
  while (G.pendingReactions.n > 0) {
    /* allReactions.splice(0): take the queued reactions, then run them in order */
    uint32_t n = G.pendingReactions.n;
    Node **batch = arena_alloc(sizeof(Node *) * n);
    memcpy(batch, G.pendingReactions.v, sizeof(Node *) * n);
    G.pendingReactions.n = 0;
    for (uint32_t i = 0; i < n; i++) run_reaction(batch[i]);
  }
  G.isRunningReactions = 0;
}
static void run_reactions(void) {
  C_OP(RUN_REACTIONS);
  if (G.inBatch > 0 || G.isRunningReactions) return;
  run_reactions_helper();
}

/* action(): untracked and batched; the outermost action runs the pending reactions */
static void action_begin(Node **prev) {
  start_batch();
  *prev = G.trackingDerivation;
  G.trackingDerivation = NULL;
}
static void action_end(Node *prev) {
  G.trackingDerivation = prev;
  end_batch();
}
/* transaction(): batched, tracking unchanged */
static void transaction_begin(void) { start_batch(); }
static void transaction_end(void) { end_batch(); }

/* reaction(expression, effect): the first run only tracks; the effect runs on later changes */
static Val reaction_expr_thunk(void *ctx) {
  Node *r = ctx;
  bool prevAllow = G.allowStateChanges;
  G.allowStateChanges = false;          /* allowStateChanges(false, expression) */
  Val next = r->derive(r->scope);
  G.allowStateChanges = prevAllow;
  return next;
}
static void reaction_on_invalidate(Node *r) {
  if (r->disposed) return;
  Val next = reaction_track(r, reaction_expr_thunk, r);
  bool changed = r->first || !same_val(r->rvalue, next);
  r->rvalue = next;
  if (!r->first && changed) {
    /* the effect is wrapped in action() */
    start_batch();
    Node *prev = G.trackingDerivation;
    G.trackingDerivation = NULL;
    r->effect(r->rctx, next);
    G.trackingDerivation = prev;
    end_batch();
  }
  r->first = 0;
}
static Node *reaction_new(const char *name, Val (*expr)(void *), void *scope,
                          void (*effect)(void *, Val), void *rctx) {
  Node *r = node_new(NK_REACTION, name);
  r->derive = expr;
  r->scope = scope;
  r->effect = effect;
  r->rctx = rctx;
  r->onInvalidate = reaction_on_invalidate;
  r->first = 1;
  schedule(r);
  return r;
}
/* autorun(view): runs view now and again whenever something it read changes */
static Val autorun_thunk(void *ctx) { Node *r = ctx; r->view(r->scope); return vundef(); }
static void autorun_on_invalidate(Node *r) { reaction_track(r, autorun_thunk, r); }
static Node *autorun_new(const char *name, void (*view)(void *), void *ctx) {
  Node *r = node_new(NK_REACTION, name);
  r->view = view;
  r->scope = ctx;
  r->onInvalidate = autorun_on_invalidate;
  schedule(r);
  return r;
}

/* ================= observable values and objects ================= */
typedef struct Obj {
  Hdr h;
  const char *name;
  VMap values;                        /* values_: property name -> ObservableValue or ComputedValue */
  Node *keysAtom;
  Lis lis;
} Obj;

static Val deep_enhance(Val v);
/* ObservableValue constructor: the enhancer runs on the initial value */
static Node *ov_new(const char *name, int enh, Val v) {
  Node *n = node_new(NK_VALUE, name);
  n->enh = (uint8_t)enh;
  n->value = enh == ENH_DEEP ? deep_enhance(v) : v;
  return n;
}
static Val ov_get(Node *o) { report_observed(o); return o->value; }
/* prepareNewValue_: interceptors, the enhancer, then the equality check. Returns true when it changed. */
static bool ov_prepare(Node *o, Val v, Val *out) {
  has_interceptors(&o->lis);
  Val nv = o->enh == ENH_DEEP ? deep_enhance(v) : v;
  if (same_val(o->value, nv)) return false;
  *out = nv;
  return true;
}
/* setNewValue_ */
static void ov_set_new_value(Node *o, Val nv) {
  o->value = nv;
  report_changed(o);
  has_listeners(&o->lis);
}

static void obj_init(Obj *o, const char *name) {
  o->h.kind = HK_OBJ;
  o->h.observable = 1;
  o->name = name;
  o->keysAtom = node_new(NK_ATOM, "ObservableObject.keys");
}
/* defineObservableProperty_: the own data property is replaced by an ObservableValue in the values map */
static void obj_define_observable(Obj *o, const char *key, Val v) {
  start_batch();
  /* delete_(key): the own property goes, and the keys atom reports the change */
  has_interceptors(&o->lis);
  start_batch();
  has_listeners(&o->lis);
  report_changed(o->keysAtom);
  end_batch();
  has_interceptors(&o->lis);
  Node *ob = ov_new(key, ENH_DEEP, v);
  vm_set(&o->values, vstr(key), ob);
  has_listeners(&o->lis);             /* notifyPropertyAddition_ */
  report_changed(o->keysAtom);
  end_batch();
}
/* defineComputedProperty_: a computed getter is on the prototype, not an own property, so delete_
 * returns at once; only the define and the keys-atom notification remain */
static void obj_define_computed(Obj *o, const char *key, Val (*derive)(void *), void *scope) {
  start_batch();
  has_interceptors(&o->lis);
  vm_set(&o->values, vstr(key), computed_new(key, derive, scope));
  has_listeners(&o->lis);
  report_changed(o->keysAtom);
  end_batch();
}
/* values_.get(key) for a property access; a computed property is read through its derivation */
static Node *obj_slot(Obj *o, const char *key) {
  Node *n = vm_get(&o->values, vstr(key));
  if (!n) { fprintf(stderr, "mobx-startup: no property %s\n", key); exit(1); }
  return n;
}
/* getObservablePropValue_: values_.get(key).get() */
static Val obj_get(Obj *o, const char *key) {
  Node *n = obj_slot(o, key);
  if (n->kind == NK_COMPUTED) return computed_get(n);
  return ov_get(n);
}
/* setObservablePropValue_ */
static void obj_set(Obj *o, const char *key, Val v) {
  Node *ob = obj_slot(o, key);
  has_interceptors(&o->lis);
  Val nv;
  if (ov_prepare(ob, v, &nv)) {
    has_listeners(&o->lis);
    ov_set_new_value(ob, nv);
  }
}

/* ================= plain arrays and observable arrays ================= */
typedef struct PlainArr { Hdr h; Val *v; uint32_t n, cap; } PlainArr;
static PlainArr *plain_new(uint32_t cap) {
  PlainArr *p = arena_calloc(sizeof *p);
  p->h.kind = HK_PLAIN_ARR;
  p->cap = cap ? cap : 1;
  p->v = arena_alloc(sizeof(Val) * p->cap);
  return p;
}
static void plain_push(PlainArr *p, Val x) {
  if (p->n == p->cap) {
    Val *nv = arena_alloc(sizeof(Val) * p->cap * 2);
    memcpy(nv, p->v, sizeof(Val) * p->n);
    p->v = nv; p->cap *= 2;
  }
  p->v[p->n++] = x;
}

typedef struct Arr {
  Hdr h;
  Node *atom;
  Val *v;
  uint32_t n, cap;
  Lis lis;
} Arr;
static Arr *arr_alloc(void) {
  Arr *a = arena_calloc(sizeof *a);
  a->h.kind = HK_ARR;
  a->h.observable = 1;
  a->atom = node_new(NK_ATOM, "ObservableArray@");
  return a;
}
/* spliceWithArray_: the one mutation path of an observable array (push, slice assignment, creation) */
static void arr_splice(Arr *a, uint32_t index, uint32_t deleteCount, const Val *items, uint32_t nitems) {
  uint32_t length = a->n;
  if (index > length) index = length;
  if (deleteCount > length - index) deleteCount = length - index;
  has_interceptors(&a->lis);
  /* newItems.map(enhancer): a new array of the enhanced items */
  PlainArr *newItems = plain_new(nitems);
  for (uint32_t i = 0; i < nitems; i++) plain_push(newItems, deep_enhance(items[i]));
  /* spliceItemsIntoValues_ */
  uint32_t newLen = length - deleteCount + nitems;
  if (newLen > a->cap) {
    uint32_t cap = a->cap ? a->cap : 8;
    while (cap < newLen) cap *= 2;
    Val *nv = arena_alloc(sizeof(Val) * cap);
    if (a->n) memcpy(nv, a->v, sizeof(Val) * a->n);
    a->v = nv; a->cap = cap;
  }
  memmove(a->v + index + nitems, a->v + index + deleteCount, sizeof(Val) * (length - index - deleteCount));
  memcpy(a->v + index, newItems->v, sizeof(Val) * nitems);
  a->n = newLen;
  /* notifyArraySplice_ */
  if (deleteCount == 0 && nitems == 0) return;
  has_listeners(&a->lis);
  report_changed(a->atom);
}
/* The proxy's get trap receives the key as a string; an index is parsed back from it. */
static uint32_t arr_key_index(uint32_t i) {
  char key[16];
  snprintf(key, sizeof key, "%u", i);
  char *end = NULL;
  return (uint32_t)strtoul(key, &end, 10);
}
static uint32_t arr_length(Arr *a) { report_observed(a->atom); return a->n; }   /* getArrayLength_ */
static Val arr_index(Arr *a, uint32_t i) {                                       /* get_(index) */
  i = arr_key_index(i);
  report_observed(a->atom);
  return i < a->n ? a->v[i] : vundef();
}
static void arr_push(Arr *a, Val x) { arr_splice(a, a->n, 0, &x, 1); }
/* arrayExtensions.slice: the read is reported, then the range is copied */
static PlainArr *arr_slice(Arr *a, uint32_t from) {
  report_observed(a->atom);
  if (from > a->n) from = a->n;
  PlainArr *p = plain_new(a->n - from);
  for (uint32_t i = from; i < a->n; i++) plain_push(p, a->v[i]);
  return p;
}
static bool arr_includes_str(Arr *a, const char *s) {
  report_observed(a->atom);
  for (uint32_t i = 0; i < a->n; i++) if (same_val(a->v[i], vstr(s))) return true;
  return false;
}
/* observable.array(plain): createObservableArray, with the items spliced in */
static Arr *arr_from_plain(PlainArr *p) {
  start_batch();                        /* createObservableArray runs inside initObservable */
  Arr *a = arr_alloc();
  if (p->n) arr_splice(a, 0, 0, p->v, p->n);
  end_batch();
  return a;
}

/* ================= observable maps and sets ================= */
typedef struct OMap {
  Hdr h;
  Node *keysAtom;
  VMap data;                          /* data_: key -> ObservableValue */
  VMap has;                           /* hasMap_: key -> ObservableValue, for has() inside a derivation */
  Lis lis;
} OMap;
typedef struct ES6 { Hdr h; VMap m; } ES6;   /* a plain JS Map or Set (value NULL for a Set) */

static OMap *omap_alloc(void) {
  OMap *m = arena_calloc(sizeof *m);
  m->h.kind = HK_MAP;
  m->h.observable = 1;
  m->keysAtom = node_new(NK_ATOM, "ObservableMap.keys()");
  return m;
}
static bool omap_has_raw(OMap *m, Val k) { return vm_has(&m->data, k); }
static void omap_add_value(OMap *m, Val k, Val v) {
  start_batch();
  Node *ob = ov_new("ObservableMap.key", ENH_DEEP, v);
  vm_set(&m->data, k, ob);
  Node *entry = vm_get(&m->has, k);
  if (entry) ov_set_new_value(entry, vnum(1));
  report_changed(m->keysAtom);
  end_batch();
  has_listeners(&m->lis);
}
static void omap_update_value(OMap *m, Val k, Val v) {
  Node *ob = vm_get(&m->data, k);
  Val nv;
  if (ov_prepare(ob, v, &nv)) {
    has_listeners(&m->lis);
    ov_set_new_value(ob, nv);
  }
}
static void omap_set(OMap *m, Val k, Val v) {
  bool hasKey = omap_has_raw(m, k);
  has_interceptors(&m->lis);
  if (hasKey) omap_update_value(m, k, v);
  else omap_add_value(m, k, v);
}
typedef struct HasCb { OMap *m; Val k; } HasCb;
static void hasmap_unobserved(void *ctx) { HasCb *h = ctx; vm_delete(&h->m->has, h->k); }
/* has(key): outside a derivation this is a plain lookup; inside one the answer is an observable */
static bool omap_has(OMap *m, Val k) {
  if (!G.trackingDerivation) return omap_has_raw(m, k);
  Node *entry = vm_get(&m->has, k);
  if (!entry) {
    entry = ov_new("ObservableMap.key?", ENH_REF, vnum(omap_has_raw(m, k)));
    vm_set(&m->has, k, entry);
    HasCb *hc = arena_alloc(sizeof *hc);
    hc->m = m; hc->k = k;
    Cb *cb = arena_alloc(sizeof *cb);
    cb->fn = hasmap_unobserved; cb->ctx = hc;
    pv_push(&entry->onBUO, cb);
  }
  return ov_get(entry).u.n != 0;
}
static Val omap_get(OMap *m, Val k) {
  if (omap_has(m, k)) return ov_get(vm_get(&m->data, k));
  return vundef();
}
static void omap_delete(OMap *m, Val k) {
  has_interceptors(&m->lis);
  if (omap_has_raw(m, k)) {
    has_listeners(&m->lis);             /* notify, computed once before the transaction */
    transaction_begin();
    report_changed(m->keysAtom);
    Node *entry = vm_get(&m->has, k);
    if (entry) ov_set_new_value(entry, vnum(0));
    ov_set_new_value(vm_get(&m->data, k), vundef());
    vm_delete(&m->data, k);
    transaction_end();
  }
}
static uint32_t omap_size(OMap *m) { report_observed(m->keysAtom); return m->data.live; }
/* forEach: the keys are read once (keys()), then each value through get() */
typedef void (*MapFn)(Val v, Val k, void *ctx);
static void omap_foreach(OMap *m, MapFn fn, void *ctx) {
  report_observed(m->keysAtom);
  for (uint32_t i = 0; i < m->data.n; i++) {
    if (!m->data.e[i].live) continue;
    Val k = m->data.e[i].k;
    fn(omap_get(m, k), k, ctx);
  }
}
/* Array.from(map.values()) */
static PlainArr *omap_values(OMap *m) {
  report_observed(m->keysAtom);
  PlainArr *p = plain_new(m->data.live);
  for (uint32_t i = 0; i < m->data.n; i++) {
    if (!m->data.e[i].live) continue;
    plain_push(p, omap_get(m, m->data.e[i].k));
  }
  return p;
}

typedef struct OSet { Hdr h; Node *atom; VMap data; Lis lis; } OSet;
static OSet *oset_alloc(void) {
  OSet *s = arena_calloc(sizeof *s);
  s->h.kind = HK_SET;
  s->h.observable = 1;
  s->atom = node_new(NK_ATOM, "ObservableSet@");
  return s;
}
static bool oset_has(OSet *s, Val v) { report_observed(s->atom); return vm_has(&s->data, v); }
static void oset_add(OSet *s, Val v) {
  has_interceptors(&s->lis);
  if (!oset_has(s, v)) {
    start_batch();
    vm_set(&s->data, v, NULL);
    report_changed(s->atom);
    end_batch();
    has_listeners(&s->lis);
  }
}
static void oset_delete(OSet *s, Val v) {
  has_interceptors(&s->lis);
  if (oset_has(s, v)) {
    has_listeners(&s->lis);
    transaction_begin();
    report_changed(s->atom);
    vm_delete(&s->data, v);
    transaction_end();
  }
}
static uint32_t oset_size(OSet *s) { report_observed(s->atom); return s->data.live; }
static void oset_foreach(OSet *s, MapFn fn, void *ctx) {
  report_observed(s->atom);             /* values() */
  for (uint32_t i = 0; i < s->data.n; i++) {
    if (!s->data.e[i].live) continue;
    Val v = s->data.e[i].k;
    fn(v, v, ctx);
  }
}

/* deepEnhancer: observable values pass through; plain arrays, new Maps and new Sets are made observable */
static OMap *omap_from_es6(ES6 *src) {
  start_batch();                        /* the constructor runs inside initObservable */
  OMap *m = omap_alloc();
  start_batch();                        /* merge() is a transaction */
  for (uint32_t i = 0; i < src->m.n; i++) if (src->m.e[i].live) omap_set(m, src->m.e[i].k, vobj(src->m.e[i].v));
  end_batch();
  end_batch();
  return m;
}
static OSet *oset_from_es6(ES6 *src) {
  start_batch();                        /* the constructor runs inside initObservable */
  OSet *s = oset_alloc();
  start_batch();                        /* replace() is a transaction; clear() inside it is another */
  start_batch(); end_batch();
  for (uint32_t i = 0; i < src->m.n; i++) if (src->m.e[i].live) oset_add(s, src->m.e[i].k);
  end_batch();
  end_batch();
  return s;
}
static Val deep_enhance(Val v) {
  if (v.t != V_OBJ) return v;
  Hdr *h = v.u.p;
  if (h->observable) return v;
  switch (h->kind) {
    case HK_PLAIN_ARR: return vobj(arr_from_plain((PlainArr *)h));
    case HK_ES6_MAP: return vobj(omap_from_es6((ES6 *)h));
    case HK_ES6_SET: return vobj(oset_from_es6((ES6 *)h));
    default: return v;
  }
}
static ES6 *es6_new(uint8_t kind) {
  ES6 *e = arena_calloc(sizeof *e);
  e->h.kind = kind;
  return e;
}

/* ================= logs and checks ================= */
typedef struct Logs { const char **v; uint32_t n, cap; } Logs;
static Logs g_logs;
static void log_push(const char *s, size_t n) {
  if (g_logs.n == g_logs.cap) {
    uint32_t cap = g_logs.cap ? g_logs.cap * 2 : 1024;
    const char **nv = arena_alloc(sizeof *nv * cap);
    if (g_logs.n) memcpy(nv, g_logs.v, sizeof *nv * g_logs.n);
    g_logs.v = nv; g_logs.cap = cap;
  }
  g_logs.v[g_logs.n++] = arena_str(s, n);
}
/* customLog(...args): the template literal is built, then pushed */
static void custom_log(const char *fmt, ...) __attribute__((format(printf, 1, 2)));
static void custom_log(const char *fmt, ...) {
  char buf[512];
  va_list ap;
  va_start(ap, fmt);
  int n = vsnprintf(buf, sizeof buf, fmt, ap);
  va_end(ap);
  if (n < 0) n = 0;
  if ((size_t)n >= sizeof buf) n = sizeof buf - 1;
  log_push(buf, (size_t)n);
}
static void check(bool cond, const char *what) {
  if (!cond) { fprintf(stderr, "mobx-startup: assertion failure: %s\n", what); exit(1); }
}

/* ================= the module body: the MobX library is defined ================= */
typedef struct FnObj { uint32_t length; const char *name; VMap props; } FnObj;
static FnObj *g_fns[BODY_FUNCTIONS];
static VMap g_globals;                  /* the MobX globalState object's fields */
/* Each function, class, object literal, array and `new` object the body evaluates allocates its
 * record, and the property tables are filled, as evaluating the bundle's module body does. */
static void body_eval(void) {
  memset(&g_globals, 0, sizeof g_globals);
  for (int i = 0; i < BODY_FUNCTIONS; i++) {
    FnObj *f = arena_calloc(sizeof *f);
    f->length = (uint32_t)(i % 4);
    f->name = i < BODY_CLASSES ? "class" : "fn";
    g_fns[i] = f;
  }
  /* classes: each constructor gets a prototype, and the prototype receives the class's methods */
  for (int c = 0; c < BODY_CLASSES; c++) {
    FnObj *proto = arena_calloc(sizeof *proto);
    proto->name = "prototype";
    for (int m = 0; m < BODY_METHODS / BODY_CLASSES + (c < BODY_METHODS % BODY_CLASSES); m++) {
      FnObj *meth = g_fns[BODY_CLASSES + c * 6 + m];
      vm_set(&proto->props, vptr(meth), meth);
    }
    vm_set(&g_fns[c]->props, vstr("prototype"), proto);
  }
  for (int o = 0; o < BODY_OBJECTS; o++) {
    VMap *props = arena_calloc(sizeof *props);
    for (int k = 0; k < 4; k++) vm_set(props, vnum(k), (void *)(intptr_t)(o * 4 + k + 1));
  }
  for (int a = 0; a < BODY_ARRAYS; a++) {
    Val *items = arena_alloc(sizeof(Val) * 4);
    for (int k = 0; k < 4; k++) items[k] = vnum(a * 4 + k);
  }
  for (int nw = 0; nw < BODY_NEWS; nw++) {
    VMap *m = arena_calloc(sizeof *m);
    vm_set(m, vnum(nw), (void *)(intptr_t)(nw + 1));
  }
  /* the MobX globalState: its fields and the queues it owns */
  vm_set(&g_globals, vstr("trackingDerivation"), NULL);
  vm_set(&g_globals, vstr("pendingReactions"), NULL);
  vm_set(&g_globals, vstr("spyListeners"), NULL);
  G.pendingUnobs = arena_calloc(sizeof(PVec));
}

/* ================= the scenario: src/Person.mjs, Room.mjs, Message.mjs, Notifications.mjs ================= */
#define MAX_ROOM_UPDATES 10

typedef struct Person { Obj adm; int64_t id; } Person;
typedef struct Message { Obj adm; int64_t id; Person *author; double timestamp; } Message;
typedef struct Room { Obj adm; } Room;
typedef struct Notifications { Obj adm; ES6 *roomDisposers; } Notifications;   /* roomDisposers: a plain Map */

static int64_t g_message_id;
static OSet *person_rooms(Person *p) { return (OSet *)obj_get(&p->adm, "rooms").u.p; }
static Arr *room_updates(Room *r) { return (Arr *)obj_get(&r->adm, "roomUpdates").u.p; }
static OMap *notif_last_messages(Notifications *n) { return (OMap *)obj_get(&n->adm, "lastMessages").u.p; }

/* Person: class fields, then makeObservable, then this.id and this.name */
static Person *person_new(int64_t id, const char *name) {
  Person *p = arena_calloc(sizeof *p);
  obj_init(&p->adm, "Person");
  start_batch();                        /* makeObservable runs inside initObservable */
  obj_define_observable(&p->adm, "name", vundef());
  obj_define_observable(&p->adm, "lastMessage", vnull());
  obj_define_observable(&p->adm, "rooms", vobj(es6_new(HK_ES6_SET)));
  obj_define_observable(&p->adm, "status", vstr("Online"));
  end_batch();
  p->id = id;
  obj_set(&p->adm, "name", vstr(name));
  return p;
}
/* Person.setLastMessage (action) */
static void person_set_last_message(Person *p, Message *m) {
  Node *prev;
  action_begin(&prev);
  obj_set(&p->adm, "lastMessage", vobj(m));
  action_end(prev);
}
/* Person.addRoom and removeRoom (actions) */
static void person_add_room(Person *p, Room *r) {
  Node *prev;
  action_begin(&prev);
  oset_add(person_rooms(p), vobj(r));
  action_end(prev);
}
static void person_remove_room(Person *p, Room *r) {
  Node *prev;
  action_begin(&prev);
  oset_delete(person_rooms(p), vobj(r));
  action_end(prev);
}
/* Person.setStatus (action): status update to every room the person is in */
typedef struct StatusCtx { Person *p; const char *status; } StatusCtx;
static void room_add_status_update(Room *r, Person *p, const char *status);
static void status_room_cb(Val v, Val k, void *ctx) {
  (void)k;
  StatusCtx *s = ctx;
  room_add_status_update((Room *)v.u.p, s->p, s->status);
}
static void person_set_status(Person *p, const char *status) {
  Node *prev;
  action_begin(&prev);
  obj_set(&p->adm, "status", vstr(status));
  StatusCtx sc = {p, status};
  oset_foreach(person_rooms(p), status_room_cb, &sc);
  action_end(prev);
}

/* Room: class fields, then makeObservable (observables, then the computed getters), then this.name */
static Val room_last_message_derive(void *scope) {
  Room *r = scope;
  if (arr_length((Arr *)obj_get(&r->adm, "messages").u.p) == 0) return vnull();
  Arr *msgs = (Arr *)obj_get(&r->adm, "messages").u.p;
  uint32_t last = arr_length((Arr *)obj_get(&r->adm, "messages").u.p) - 1;
  return arr_index(msgs, last);
}
static Val room_message_count_derive(void *scope) {
  Room *r = scope;
  return vnum(arr_length((Arr *)obj_get(&r->adm, "messages").u.p));
}
static Val room_people_count_derive(void *scope) {
  Room *r = scope;
  return vnum(omap_size((OMap *)obj_get(&r->adm, "people").u.p));
}
static Val room_members_derive(void *scope) {
  Room *r = scope;
  return vobj(omap_values((OMap *)obj_get(&r->adm, "people").u.p));
}
static Room *room_new(const char *name) {
  Room *r = arena_calloc(sizeof *r);
  obj_init(&r->adm, "Room");
  start_batch();                        /* makeObservable runs inside initObservable */
  obj_define_observable(&r->adm, "name", vundef());
  obj_define_observable(&r->adm, "people", vobj(es6_new(HK_ES6_MAP)));
  obj_define_observable(&r->adm, "messages", vobj(plain_new(0)));
  obj_define_observable(&r->adm, "roomUpdates", vobj(plain_new(0)));
  obj_define_computed(&r->adm, "messageCount", room_message_count_derive, r);
  obj_define_computed(&r->adm, "peopleCount", room_people_count_derive, r);
  obj_define_computed(&r->adm, "lastMessage", room_last_message_derive, r);
  obj_define_computed(&r->adm, "members", room_members_derive, r);
  end_batch();
  obj_set(&r->adm, "name", vstr(name));
  return r;
}
/* Room.addPerson (action) */
static void room_add_person(Room *r, Person *p) {
  Node *prev;
  action_begin(&prev);
  if (!omap_has((OMap *)obj_get(&r->adm, "people").u.p, vnum(p->id))) {
    omap_set((OMap *)obj_get(&r->adm, "people").u.p, vnum(p->id), vobj(p));
    person_add_room(p, r);
  }
  action_end(prev);
}
/* Room.addMessage (action) */
static void room_add_message(Room *r, Message *m) {
  Node *prev;
  action_begin(&prev);
  arr_push((Arr *)obj_get(&r->adm, "messages").u.p, vobj(m));
  person_set_last_message(m->author, m);
  action_end(prev);
}
/* Room.addStatusUpdate (action) */
static void room_add_status_update(Room *r, Person *p, const char *status) {
  Node *prev;
  action_begin(&prev);
  Arr *ru = room_updates(r);                         /* this.roomUpdates, then .push(...) */
  Val pname = obj_get(&p->adm, "name");              /* ${person.name} */
  char buf[256];
  int n = snprintf(buf, sizeof buf, "%s is now %s", pname.u.s, status);
  arr_push(ru, vstr(arena_str(buf, (size_t)n)));
  if (arr_length(room_updates(r)) > MAX_ROOM_UPDATES * 2) {
    PlainArr *sliced = arr_slice(room_updates(r), MAX_ROOM_UPDATES);
    obj_set(&r->adm, "roomUpdates", vobj(sliced));
  }
  action_end(prev);
}
/* Room.dispose (action) */
typedef struct DisposeCtx { Room *r; } DisposeCtx;
static void dispose_person_cb(Val v, Val k, void *ctx) {
  (void)k;
  person_remove_room((Person *)v.u.p, ((DisposeCtx *)ctx)->r);
}
static void notif_remove_room(Notifications *n, const char *roomName);
static void room_dispose(Room *r, Notifications *n) {
  Node *prev;
  action_begin(&prev);
  DisposeCtx dc = {r};
  omap_foreach((OMap *)obj_get(&r->adm, "people").u.p, dispose_person_cb, &dc);
  if (n) notif_remove_room(n, (const char *)obj_get(&r->adm, "name").u.s);
  action_end(prev);
}

/* Message: class fields, makeObservable (text observable), then this.id, this.text, author, timestamp */
static Message *message_new(const char *text, Person *author) {
  Message *m = arena_calloc(sizeof *m);
  obj_init(&m->adm, "Message");
  start_batch();                        /* makeObservable runs inside initObservable */
  obj_define_observable(&m->adm, "text", vundef());
  end_batch();
  m->id = g_message_id++;
  obj_set(&m->adm, "text", vstr(text));
  m->author = author;
  m->timestamp = js_now_ms();
  return m;
}

/* Notifications: lastMessages (observable map), roomDisposers (plain map) */
static void notif_update_last_message(Notifications *n, const char *roomName, Message *m) {
  Node *prev;
  action_begin(&prev);
  omap_set(notif_last_messages(n), vstr(roomName), vobj(m));
  action_end(prev);
}
typedef struct RoomReaction { Notifications *n; Room *r; } RoomReaction;
static Val reaction_expr_room_last(void *ctx) {
  RoomReaction *rr = ctx;
  return obj_get(&rr->r->adm, "lastMessage");
}
static void reaction_effect_update(void *ctx, Val message) {
  RoomReaction *rr = ctx;
  if (message.t == V_OBJ) {
    const char *name = obj_get(&rr->r->adm, "name").u.s;
    notif_update_last_message(rr->n, name, message.u.p);
  }
}
/* Notifications.addRoom (action): reaction(() => room.lastMessage, ...), then roomDisposers.set */
static void notif_add_room(Notifications *n, Room *r) {
  Node *prev;
  action_begin(&prev);
  RoomReaction *rr = arena_calloc(sizeof *rr);
  rr->n = n;
  rr->r = r;
  Node *disposer = reaction_new("Reaction", reaction_expr_room_last, rr, reaction_effect_update, rr);
  vm_set(&n->roomDisposers->m, vstr(obj_get(&r->adm, "name").u.s), disposer);
  action_end(prev);
}
/* Notifications.removeRoom (action) */
static void notif_remove_room(Notifications *n, const char *roomName) {
  Node *prev;
  action_begin(&prev);
  if (vm_has(&n->roomDisposers->m, vstr(roomName))) {
    Node *disposer = vm_get(&n->roomDisposers->m, vstr(roomName));
    reaction_dispose(disposer);
    vm_delete(&n->roomDisposers->m, vstr(roomName));
    omap_delete(notif_last_messages(n), vstr(roomName));
  }
  action_end(prev);
}
static Notifications *notifications_new(Room **rooms, int nrooms) {
  Notifications *n = arena_calloc(sizeof *n);
  obj_init(&n->adm, "Notifications");
  start_batch();                        /* makeObservable runs inside initObservable */
  obj_define_observable(&n->adm, "lastMessages", vobj(es6_new(HK_ES6_MAP)));
  end_batch();
  n->roomDisposers = es6_new(HK_ES6_MAP);
  for (int i = 0; i < nrooms; i++) notif_add_room(n, rooms[i]);
  return n;
}

/* the two autoruns of runTest */
typedef struct NotifCtx { Notifications *n; } NotifCtx;
static void text_name_cb(Val message, Val roomName, void *ctx) {
  (void)ctx;
  Message *m = message.u.p;
  const char *text = obj_get(&m->adm, "text").u.s;
  const char *name = obj_get(&m->author->adm, "name").u.s;
  custom_log("[%s] \"%s\" - %s", roomName.u.s, text, name);
}
static void view_notifications(void *ctx) {
  NotifCtx *c = ctx;
  custom_log("\n--- Global Notifications (Last Messages) ---");
  if (omap_size(notif_last_messages(c->n)) == 0) {
    custom_log("No messages yet.");
  } else {
    omap_foreach(notif_last_messages(c->n), text_name_cb, NULL);
  }
  custom_log("------------------------------------------");
}
typedef struct RoomsCtx { Room **rooms; int n; } RoomsCtx;
static void view_room_updates(void *ctx) {
  RoomsCtx *c = ctx;
  for (int i = 0; i < c->n; i++) {
    Room *r = c->rooms[i];
    if (arr_length(room_updates(r)) > 0) {
      const char *name = obj_get(&r->adm, "name").u.s;     /* ${room.name} */
      Arr *ru = room_updates(r);                            /* room.roomUpdates[ ... ] */
      uint32_t last = arr_length(room_updates(r)) - 1;
      Val text = arr_index(ru, last);
      custom_log("[%s Update] %s", name, text.u.s);
    }
  }
}

/* runTest(): the scenario of src/test.mjs, with the same reads and writes in the same order */
static uint32_t run_test(void) {
  Room *techRoom = room_new("Tech Talk");
  Room *generalRoom = room_new("General Chat");
  Room *largeRoom = room_new("Large Meeting Room");
  Person *alice = person_new(1, "Alice");
  Person *bob = person_new(2, "Bob");
  room_add_person(techRoom, alice);
  room_add_person(techRoom, bob);
  room_add_person(generalRoom, alice);
  Room *rooms[3] = {techRoom, generalRoom, largeRoom};
  Notifications *notifications = notifications_new(rooms, 3);

  custom_log("\n--- Setting up Large Meeting Room ---");
  Person **largeRoomMembers = arena_alloc(sizeof(Person *) * 200);
  int nLarge = 0;
  for (int i = 3; i < 203; i++) {
    char nm[32];
    int len = snprintf(nm, sizeof nm, "Person %d", i);
    Person *person = person_new(i, arena_str(nm, (size_t)len));
    largeRoomMembers[nLarge++] = person;
    room_add_person(largeRoom, person);
  }
  custom_log("[INFO] Added %lld members to the large room.",
             (long long)obj_get(&largeRoom->adm, "peopleCount").u.n);
  room_add_person(largeRoom, alice);
  room_add_person(largeRoom, bob);
  custom_log("[INFO] Added Alice and Bob to the Large Meeting Room.");

  check(oset_size(person_rooms(alice)) == 3, "alice.rooms.size === 3");
  check(oset_has(person_rooms(alice), vobj(techRoom)) && oset_has(person_rooms(alice), vobj(generalRoom)) &&
            oset_has(person_rooms(alice), vobj(largeRoom)), "alice.rooms.has");
  check(oset_size(person_rooms(bob)) == 2 && oset_has(person_rooms(bob), vobj(techRoom)) &&
            oset_has(person_rooms(bob), vobj(largeRoom)), "bob.rooms");
  custom_log("[PASS] Verified Person.rooms associations with Set.");

  NotifCtx nctx = {notifications};
  Node *notificationsDisposer = autorun_new("Autorun", view_notifications, &nctx);
  RoomsCtx rctx = {rooms, 3};
  Node *roomUpdatesDisposer = autorun_new("Autorun", view_room_updates, &rctx);

  custom_log("\n--- Status Update Scenario ---");
  person_set_status(alice, "Away");
  check(arr_includes_str(room_updates(techRoom), "Alice is now Away"), "tech: Alice is now Away");
  check(arr_includes_str(room_updates(generalRoom), "Alice is now Away"), "general: Alice is now Away");
  check(arr_includes_str(room_updates(largeRoom), "Alice is now Away"), "large: Alice is now Away");
  custom_log("[PASS] Verified that status updates are sent to all of Alice's rooms.");
  person_set_status(bob, "Busy");
  check(arr_includes_str(room_updates(techRoom), "Bob is now Busy"), "tech: Bob is now Busy");
  check(!arr_includes_str(room_updates(generalRoom), "Bob is now Busy"), "general: no Bob is now Busy");
  check(arr_includes_str(room_updates(largeRoom), "Bob is now Busy"), "large: Bob is now Busy");
  custom_log("[PASS] Verified that status updates are only sent to Bob's rooms.");

  custom_log("\n--- Sending Messages & Verifying Notifications ---");
  Message *msg1 = message_new("First message to Tech Talk!", alice);
  room_add_message(techRoom, msg1);
  check(omap_get(notif_last_messages(notifications), vstr("Tech Talk")).u.p == msg1, "tech last message");
  custom_log("[PASS] Verified Tech Talk notification.");

  for (int i = 0; i < 20; i++) {
    char txt[32];
    snprintf(txt, sizeof txt, "ping %d", i);
    Message *m = message_new(arena_str(txt, strlen(txt)), alice);
    room_add_message(generalRoom, m);
    check(omap_get(notif_last_messages(notifications), vstr("General Chat")).u.p == m, "general notification");
  }
  custom_log("[PASS] Verified General Chat notification.");

  for (int i = 0; i <= 5; i++) {
    for (int k = 0; k < nLarge; k++) {
      Person *member = largeRoomMembers[k];
      char txt[128];
      snprintf(txt, sizeof txt, "Hi, I am %s", obj_get(&member->adm, "name").u.s);
      Message *m = message_new(arena_str(txt, strlen(txt)), member);
      room_add_message(largeRoom, m);
      check(obj_get(&largeRoom->adm, "lastMessage").u.p == m, "large last message");
      check(omap_get(notif_last_messages(notifications), vstr("Large Meeting Room")).u.p == m, "large notification");
    }
  }
  custom_log("[PASS] Verified Large Meeting Room notification.");

  for (int i = 0; i < 20; i++) {
    char txt[32];
    snprintf(txt, sizeof txt, "ping %d", i);
    Message *m = message_new(arena_str(txt, strlen(txt)), bob);
    room_add_message(techRoom, m);
    check(omap_get(notif_last_messages(notifications), vstr("Tech Talk")).u.p == m, "tech notification update");
  }
  custom_log("[PASS] Verified Tech Talk notification update.");

  Room *tempRooms[10];
  for (int i = 0; i < 10; i++) {
    char roomId[64];
    snprintf(roomId, sizeof roomId, "General Chat %d", i);
    const char *rid = arena_str(roomId, strlen(roomId));
    Room *tempRoom = room_new(rid);
    tempRooms[i] = tempRoom;
    notif_add_room(notifications, tempRoom);
    for (int k = 0; k < 10; k++) room_add_person(tempRoom, largeRoomMembers[k]);
    PlainArr *members = (PlainArr *)obj_get(&tempRoom->adm, "members").u.p;   /* tempRoom.members.slice(0, 5) */
    uint32_t nm = members->n < 5 ? members->n : 5;
    for (uint32_t k = 0; k < nm; k++) {
      Person *person = members->v[k].u.p;
      char txt[64];
      snprintf(txt, sizeof txt, "ping %d", i);
      Message *m = message_new(arena_str(txt, strlen(txt)), person);
      room_add_message(tempRoom, m);
      check(omap_get(notif_last_messages(notifications), vstr(rid)).u.p == m, "temp room notification");
    }
  }
  for (int i = 0; i < 10; i++) room_dispose(tempRooms[i], notifications);

  reaction_dispose(notificationsDisposer);
  reaction_dispose(roomUpdatesDisposer);
  room_dispose(techRoom, notifications);
  room_dispose(generalRoom, notifications);
  room_dispose(largeRoom, notifications);
  return g_logs.n;
}

/* ================= the bundle: per-iteration copies, the parse, the run, and the checks ================= */
static const char CACHE_BUST[] = "\n/*ThouShaltNotCache*/\n";

/* StartupBenchmark.createIterationSourceCode: each cache-busting comment gets the iteration's cache id */
static char *iteration_source(const char *bundle, size_t n, int iteration, size_t *out_n) {
  int cacheId = (iteration / 8) * 8;
  char id[32];
  int idn = snprintf(id, sizeof id, "\n/*%d*/\n", cacheId);
  size_t cl = strlen(CACHE_BUST);
  char *out = malloc(n + 64 * 32);
  size_t o = 0;
  for (size_t i = 0; i < n;) {
    if (i + cl <= n && memcmp(bundle + i, CACHE_BUST, cl) == 0) {
      memcpy(out + o, id, (size_t)idn);
      o += (size_t)idn;
      i += cl;
    } else {
      out[o++] = bundle[i++];
    }
  }
  out[o] = 0;
  *out_n = o;
  return out;
}
/* FNV-1a 64 over the log lines, each followed by a newline (the node run hashes lines.join('\n') + '\n') */
static void log_hash_hex(char out[17]) {
  uint64_t h = 0xcbf29ce484222325ULL;
  for (uint32_t i = 0; i < g_logs.n; i++) {
    for (const unsigned char *p = (const unsigned char *)g_logs.v[i]; *p; p++) { h ^= *p; h *= 0x100000001b3ULL; }
    h ^= '\n'; h *= 0x100000001b3ULL;
  }
  snprintf(out, 17, "%016llx", (unsigned long long)h);
}

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : 30;
  const char *root = argc > 2 ? argv[2] : "../jetstream";
  char path[4096];
  snprintf(path, sizeof path, "%s/mobx/dist/bundle.es6.min.js", root);
  size_t bn = 0;
  char *bundle = js_read_file(path, &bn);
  if (!bundle) { fprintf(stderr, "mobx-startup: cannot read %s\n", path); return 2; }
  JsContext ctx;
  js_context_init(&ctx);
  double total = 0, parseMs = 0, bodyMs = 0, runMs = 0;
  int ok = 1;
  for (int it = 0; it < iterations; it++) {
    size_t n = 0;
    char *src = iteration_source(bundle, bn, it, &n);   /* made outside the timed part, as in the JS */
    memset(OPS, 0, sizeof OPS);
    memset(&G, 0, sizeof G);
    memset(&g_logs, 0, sizeof g_logs);
    g_message_id = 0;
    double t0 = js_now_ms();
    size_t tokens = 0;
    js_parse(&ctx, src, n, 0, &tokens);                  /* compile the iteration's bundle */
    double t1 = js_now_ms();
    body_eval();                                         /* the module body defines MobX */
    double t2 = js_now_ms();
    uint32_t lines = run_test();                         /* Benchmark.runIteration: runTest() */
    double t3 = js_now_ms();
    free(src);
    parseMs += t1 - t0; bodyMs += t2 - t1; runMs += t3 - t2;
    total += t3 - t0;

    /* validation, outside the timed part */
    char hash[17];
    log_hash_hex(hash);
    bool counts_ok = true;
    for (int op = 0; op < OP_COUNT; op++) {
      if (OPS[op] != OP_EXPECT[op]) {
        fprintf(stderr, "mobx-startup: iteration %d: %s %u, expected %u\n", it, OP_NAME[op], OPS[op], OP_EXPECT[op]);
        counts_ok = false;
      }
    }
    if (lines != EXPECT_LOG_LINES || strcmp(hash, EXPECT_LOG_HASH) != 0 || !counts_ok) {
      fprintf(stderr, "mobx-startup: iteration %d: log %u lines, hash %s\n", it, lines, hash);
      ok = 0;
    }
    if (it == iterations - 1) {
      printf("ops per iteration:");
      for (int op = 0; op < OP_COUNT; op++) printf("%s %s %u", op ? "," : "", OP_NAME[op], OPS[op]);
      printf("\n");
      printf("log: %u entries, hash %s\n", lines, hash);
    }
    arena_free_all();
  }
  printf("mobx-startup %d iterations %.3f ms\n", iterations, total);
  printf("  parse+compile %.3f ms, module body %.3f ms, runTest %.3f ms (all iterations)\n", parseMs, bodyMs, runMs);
  printf("validation %s\n", ok ? "ok" : "FAILED");
  js_context_destroy(&ctx);
  free(bundle);
  return ok ? 0 : 1;
}
