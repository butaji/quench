// jetstream-row: mobx-startup
// jetstream-files: utils/StartupBenchmark.js mobx/benchmark.js
// iterations: 30
// jetstream-driver: class=Async preload=BUNDLE=mobx/dist/bundle.es6.min.js
/* C analogue of the JetStream mobx-startup row (mobx/dist/bundle.es6.min.js and mobx/benchmark.js).
 *
 * Each timed iteration does what the JS does: it takes this iteration's copy of the MobX bundle (the
 * cache-busting comment replaced, as StartupBenchmark does; the copies are made before timing), parses
 * it with the JS parser analogue (js-parser-analogue.h, the same parse work as the engine's compile),
 * and then runs the scenario (runTest) whose log must have 6824 entries.
 *
 * The bundle's module body is not executed here. The scenario that runTest runs (src/test.mjs with
 * Person, Room, Message and Notifications) is ported to C, and the MobX features it uses are
 * reimplemented in the same reactive model: observable values, arrays, maps and sets with change
 * tracking; computed values (cached, invalidated through their dependencies); reactions (a value
 * expression and an effect, run when the value changes); autoruns (re-run when a dependency
 * changes); and actions (changes are batched until the outermost action ends). Reactions run in the
 * order they became stale, as MobX queues them. The log lines are the ones the JS pushes, in order.
 * Usage: mobx-startup [iterations] [jetstream-root]; the default is the row's 30 iterations. */
#define _DEFAULT_SOURCE
#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include "js-parser-analogue.h"

/* ================= reactive core ================= */
typedef struct Deriv Deriv;
typedef struct Obs { Deriv **observers; uint32_t nobs, capobs; } Obs;
enum { K_AUTORUN, K_REACTION, K_COMPUTED };
struct Deriv {
  Obs obs;                       /* observers of this derivation (used by computed values) */
  Obs **deps;                    /* sources read by the last run */
  uint32_t ndeps, capdeps;
  int kind, stale, pending, disposed;
  void *ctx;
  void (*autorun_fn)(void *ctx);
  void *(*expr)(void *ctx);
  void (*effect)(void *ctx, void *value);
  void *value;
};

static Deriv *g_track;           /* the derivation whose run is reading */
static int g_batch, g_running;
static Deriv **g_queue;
static size_t g_qh, g_qt, g_qcap;

static void obs_add_observer(Obs *o, Deriv *d) {
  for (uint32_t i = 0; i < o->nobs; i++) if (o->observers[i] == d) return;
  if (o->nobs == o->capobs) {
    o->capobs = o->capobs ? o->capobs * 2 : 4;
    o->observers = realloc(o->observers, o->capobs * sizeof *o->observers);
  }
  o->observers[o->nobs++] = d;
}
static void obs_remove_observer(Obs *o, Deriv *d) {
  for (uint32_t i = 0; i < o->nobs; i++)
    if (o->observers[i] == d) {
      memmove(o->observers + i, o->observers + i + 1, (o->nobs - i - 1) * sizeof *o->observers);
      o->nobs--;
      return;
    }
}
static void deriv_clear_deps(Deriv *d) {
  for (uint32_t i = 0; i < d->ndeps; i++) obs_remove_observer(d->deps[i], d);
  d->ndeps = 0;
}
static void deriv_add_dep(Deriv *d, Obs *o) {
  for (uint32_t i = 0; i < d->ndeps; i++) if (d->deps[i] == o) return;
  if (d->ndeps == d->capdeps) {
    d->capdeps = d->capdeps ? d->capdeps * 2 : 8;
    d->deps = realloc(d->deps, d->capdeps * sizeof *d->deps);
  }
  d->deps[d->ndeps++] = o;
  obs_add_observer(o, d);
}
static void obs_read(Obs *o) { if (g_track) deriv_add_dep(g_track, o); }

static void run_pending(void);
static void propagate(Obs *o);
static void mark_stale(Deriv *d) {
  if (d->kind == K_COMPUTED) {
    if (!d->stale) { d->stale = 1; propagate(&d->obs); }
  } else if (!d->pending && !d->disposed) {
    d->pending = 1;
    if (g_qt == g_qcap) {
      g_qcap = g_qcap ? g_qcap * 2 : 64;
      g_queue = realloc(g_queue, g_qcap * sizeof *g_queue);
    }
    g_queue[g_qt++] = d;
  }
}
static void propagate(Obs *o) {
  for (uint32_t i = 0; i < o->nobs; i++) mark_stale(o->observers[i]);
}
/* A change to an observable: its observers are marked; outside an action the reactions run now. */
static void obs_changed(Obs *o) {
  propagate(o);
  if (g_batch == 0 && !g_running) run_pending();
}
static void run_autorun(Deriv *d) {
  deriv_clear_deps(d);
  Deriv *prev = g_track;
  g_track = d;
  d->autorun_fn(d->ctx);
  g_track = prev;
}
static void run_reaction(Deriv *d) {
  deriv_clear_deps(d);
  Deriv *prev = g_track;
  g_track = d;
  void *v = d->expr(d->ctx);
  g_track = prev;
  if (v != d->value) {
    d->value = v;
    prev = g_track;
    g_track = NULL;                 /* effects are untracked */
    d->effect(d->ctx, v);
    g_track = prev;
  }
}
static void run_pending(void) {
  if (g_running) return;
  g_running = 1;
  while (g_qh < g_qt) {
    Deriv *d = g_queue[g_qh++];
    d->pending = 0;
    if (d->disposed) continue;
    if (d->kind == K_AUTORUN) run_autorun(d);
    else run_reaction(d);
  }
  g_qh = g_qt = 0;
  g_running = 0;
}
static void action_begin(void) { g_batch++; }
static void action_end(void) { if (--g_batch == 0) run_pending(); }

static Deriv *autorun_new(void (*fn)(void *), void *ctx) {
  Deriv *d = calloc(1, sizeof *d);
  d->kind = K_AUTORUN;
  d->autorun_fn = fn;
  d->ctx = ctx;
  run_autorun(d);
  return d;
}
static Deriv *reaction_new(void *(*expr)(void *), void (*effect)(void *, void *), void *ctx) {
  Deriv *d = calloc(1, sizeof *d);
  d->kind = K_REACTION;
  d->expr = expr;
  d->effect = effect;
  d->ctx = ctx;
  deriv_clear_deps(d);
  Deriv *prev = g_track;
  g_track = d;
  d->value = expr(ctx);             /* first run only tracks; the effect is not called */
  g_track = prev;
  return d;
}
static void derivation_dispose(Deriv *d) {
  deriv_clear_deps(d);
  d->disposed = 1;
}
static Deriv *computed_new(void *(*expr)(void *), void *ctx) {
  Deriv *d = calloc(1, sizeof *d);
  d->kind = K_COMPUTED;
  d->expr = expr;
  d->ctx = ctx;
  d->stale = 1;
  return d;
}
/* Reads a computed value: tracked by the current reader, recomputed only when stale. */
static void *computed_get(Deriv *c) {
  obs_read(&c->obs);
  if (c->stale) {
    deriv_clear_deps(c);
    Deriv *prev = g_track;
    g_track = c;
    c->value = c->expr(c->ctx);
    g_track = prev;
    c->stale = 0;
  }
  return c->value;
}

/* ---- observable values ---- */
typedef struct { Obs o; const char *v; } OStr;
typedef struct { Obs o; void *v; } OPtr;
static const char *ostr_get(OStr *s) { obs_read(&s->o); return s->v; }
static void ostr_set(OStr *s, const char *v) {
  if (strcmp(s->v ? s->v : "", v) == 0) return;
  s->v = v;
  obs_changed(&s->o);
}
static void optr_set(OPtr *p, void *v) {
  if (p->v == v) return;
  p->v = v;
  obs_changed(&p->o);
}

/* ---- observable arrays ---- */
typedef struct { Obs o; void **items; size_t n, cap; } OArr;
static size_t oarr_len(OArr *a) { obs_read(&a->o); return a->n; }
static void *oarr_at(OArr *a, size_t i) { obs_read(&a->o); return a->items[i]; }
static void oarr_push(OArr *a, void *v) {
  if (a->n == a->cap) {
    a->cap = a->cap ? a->cap * 2 : 8;
    a->items = realloc(a->items, a->cap * sizeof *a->items);
  }
  a->items[a->n++] = v;
  obs_changed(&a->o);
}
/* roomUpdates = roomUpdates.slice(from): the array is replaced and its observers are notified */
static void oarr_slice_from(OArr *a, size_t from) {
  obs_read(&a->o);
  if (from > a->n) from = a->n;
  memmove(a->items, a->items + from, (a->n - from) * sizeof *a->items);
  a->n -= from;
  obs_changed(&a->o);
}

/* ---- observable maps and sets (insertion order, as the JS Map and Set), keyed by an integer or a string ---- */
typedef struct { int64_t ik; const char *sk; void *v; } MEnt;
typedef struct { Obs o; MEnt *e; size_t n, cap; uint32_t *idx; size_t capidx; } OMap;
static uint64_t omap_hash(int64_t ik, const char *sk) {
  uint64_t h;
  if (sk) { h = 1469598103934665603ULL; for (const char *p = sk; *p; p++) h = (h ^ (unsigned char)*p) * 1099511628211ULL; }
  else h = (uint64_t)ik * 0x9E3779B97F4A7C15ULL;
  return h ^ (h >> 29);
}
static void omap_reindex(OMap *m, size_t want) {
  size_t cap = 16;
  while (cap < want * 2) cap *= 2;
  m->idx = realloc(m->idx, cap * sizeof *m->idx);
  memset(m->idx, 0, cap * sizeof *m->idx);
  m->capidx = cap;
  for (size_t i = 0; i < m->n; i++) {
    size_t p = omap_hash(m->e[i].ik, m->e[i].sk) & (cap - 1);
    while (m->idx[p]) p = (p + 1) & (cap - 1);
    m->idx[p] = (uint32_t)i + 1;
  }
}
static long omap_find(const OMap *m, int64_t ik, const char *sk) {
  if (!m->capidx) return -1;
  size_t p = omap_hash(ik, sk) & (m->capidx - 1);
  while (m->idx[p]) {
    const MEnt *e = &m->e[m->idx[p] - 1];
    if (sk ? (e->sk && strcmp(e->sk, sk) == 0) : (!e->sk && e->ik == ik)) return (long)(m->idx[p] - 1);
    p = (p + 1) & (m->capidx - 1);
  }
  return -1;
}
static void *omap_get(OMap *m, int64_t ik, const char *sk) {
  obs_read(&m->o);
  long i = omap_find(m, ik, sk);
  return i < 0 ? NULL : m->e[i].v;
}
static int omap_has(OMap *m, int64_t ik, const char *sk) {
  obs_read(&m->o);
  return omap_find(m, ik, sk) >= 0;
}
static size_t omap_size(OMap *m) { obs_read(&m->o); return m->n; }
static void omap_set(OMap *m, int64_t ik, const char *sk, void *v) {
  long i = omap_find(m, ik, sk);
  if (i >= 0) {
    if (m->e[i].v == v) return;
    m->e[i].v = v;
    obs_changed(&m->o);
    return;
  }
  if (m->n == m->cap) {
    m->cap = m->cap ? m->cap * 2 : 8;
    m->e = realloc(m->e, m->cap * sizeof *m->e);
  }
  m->e[m->n].ik = ik;
  m->e[m->n].sk = sk;
  m->e[m->n].v = v;
  m->n++;
  if (m->n * 2 > m->capidx) omap_reindex(m, m->n);
  else {
    size_t p = omap_hash(ik, sk) & (m->capidx - 1);
    while (m->idx[p]) p = (p + 1) & (m->capidx - 1);
    m->idx[p] = (uint32_t)m->n;
  }
  obs_changed(&m->o);
}
static int omap_delete(OMap *m, int64_t ik, const char *sk) {
  long i = omap_find(m, ik, sk);
  if (i < 0) return 0;
  memmove(m->e + i, m->e + i + 1, (m->n - (size_t)i - 1) * sizeof *m->e);
  m->n--;
  omap_reindex(m, m->n);
  obs_changed(&m->o);
  return 1;
}
/* forEach in insertion order; reads the map (tracked) */
static void omap_entries(OMap *m, MEnt **out, size_t *count) {
  obs_read(&m->o);
  *out = m->e;
  *count = m->n;
}

/* ================= logs, strings and the scenario ================= */
typedef struct { char **v; size_t n, cap; } Logs;
static Logs g_logs;
static void log_push(const char *s) {
  if (g_logs.n == g_logs.cap) {
    g_logs.cap = g_logs.cap ? g_logs.cap * 2 : 1024;
    g_logs.v = realloc(g_logs.v, g_logs.cap * sizeof *g_logs.v);
  }
  g_logs.v[g_logs.n++] = strdup(s);
}
static void log_line(const char *fmt, ...) __attribute__((format(printf, 1, 2)));
static void log_line(const char *fmt, ...) {
  char buf[1024];
  va_list ap;
  va_start(ap, fmt);
  vsnprintf(buf, sizeof buf, fmt, ap);
  va_end(ap);
  log_push(buf);
}
static void check(int cond, const char *what) {
  if (!cond) { fprintf(stderr, "mobx-startup: assertion failure: %s\n", what); exit(1); }
}
/* strings the observables keep live until the iteration ends */
typedef struct { char **v; size_t n, cap; } StrArena;
static StrArena g_strs;
static const char *keep(const char *s) {
  if (g_strs.n == g_strs.cap) {
    g_strs.cap = g_strs.cap ? g_strs.cap * 2 : 1024;
    g_strs.v = realloc(g_strs.v, g_strs.cap * sizeof *g_strs.v);
  }
  char *c = strdup(s);
  g_strs.v[g_strs.n++] = c;
  return c;
}
static void free_strings(void) {
  for (size_t i = 0; i < g_strs.n; i++) free(g_strs.v[i]);
  g_strs.n = 0;
  for (size_t i = 0; i < g_logs.n; i++) free(g_logs.v[i]);
  g_logs.n = 0;
}

typedef struct Room Room;
typedef struct Message Message;
typedef struct Person Person;
typedef struct Notifications Notifications;

static int g_message_id;
struct Person {
  int id;
  OStr name;
  OPtr lastMessage;
  OMap rooms;                 /* Set<Room> */
  OStr status;
};
struct Message {
  int id;
  OStr text;
  Person *author;
  double timestamp;
};
struct Room {
  const char *name;
  OMap people;                /* Map<id, Person> */
  OArr messages;
  OArr roomUpdates;
  Deriv *lastMessage;         /* computed */
};
struct Notifications {
  OMap lastMessages;          /* Map<roomName, Message> */
  OMap roomDisposers;         /* Map<roomName, disposer>; not observable in the JS either */
};

static void *room_last_message_expr(void *ctx) {
  Room *r = ctx;
  if (oarr_len(&r->messages) == 0) return NULL;
  return oarr_at(&r->messages, oarr_len(&r->messages) - 1);
}

static Person *person_new(int id, const char *name) {
  Person *p = calloc(1, sizeof *p);
  p->id = id;
  p->name.v = keep(name);
  p->status.v = keep("Online");
  return p;
}
static void person_set_status(Person *p, const char *status);
static void person_add_room(Person *p, Room *r) {
  action_begin();
  omap_set(&p->rooms, (int64_t)(intptr_t)r, NULL, r);
  action_end();
}
static void person_remove_room(Person *p, Room *r) {
  action_begin();
  omap_delete(&p->rooms, (int64_t)(intptr_t)r, NULL);
  action_end();
}
static void person_set_last_message(Person *p, Message *m) {
  action_begin();
  optr_set(&p->lastMessage, m);
  action_end();
}
static Room *room_new(const char *name) {
  Room *r = calloc(1, sizeof *r);
  r->name = keep(name);
  r->lastMessage = computed_new(room_last_message_expr, r);
  return r;
}
static void room_add_person(Room *r, Person *p) {
  action_begin();
  if (!omap_has(&r->people, p->id, NULL)) {
    omap_set(&r->people, p->id, NULL, p);
    person_add_room(p, r);
  }
  action_end();
}
static void room_add_message(Room *r, Message *m) {
  action_begin();
  oarr_push(&r->messages, m);
  person_set_last_message(m->author, m);
  action_end();
}
#define MAX_ROOM_UPDATES 10
static void room_add_status_update(Room *r, Person *p, const char *status) {
  action_begin();
  const char *name = ostr_get(&p->name);
  char buf[512];
  snprintf(buf, sizeof buf, "%s is now %s", name, status);
  oarr_push(&r->roomUpdates, (void *)keep(buf));
  if (oarr_len(&r->roomUpdates) > MAX_ROOM_UPDATES * 2) oarr_slice_from(&r->roomUpdates, MAX_ROOM_UPDATES);
  action_end();
}
static Message *message_new(const char *text, Person *author) {
  Message *m = calloc(1, sizeof *m);
  m->id = g_message_id++;
  m->text.v = keep(text);
  m->author = author;
  m->timestamp = js_now_ms();
  return m;
}
static void room_dispose(Room *r, Notifications *n);
static void person_set_status(Person *p, const char *status) {
  action_begin();
  ostr_set(&p->status, status);
  MEnt *es;
  size_t cnt;
  omap_entries(&p->rooms, &es, &cnt);
  Room **snapshot = malloc((cnt + 1) * sizeof *snapshot);
  for (size_t i = 0; i < cnt; i++) snapshot[i] = es[i].v;
  for (size_t i = 0; i < cnt; i++) room_add_status_update(snapshot[i], p, status);
  free(snapshot);
  action_end();
}
static void room_dispose(Room *r, Notifications *n) {
  action_begin();
  MEnt *es;
  size_t cnt;
  omap_entries(&r->people, &es, &cnt);
  Person **snapshot = malloc((cnt + 1) * sizeof *snapshot);
  for (size_t i = 0; i < cnt; i++) snapshot[i] = es[i].v;
  for (size_t i = 0; i < cnt; i++) person_remove_room(snapshot[i], r);
  free(snapshot);
  if (n) {
    /* notifications.removeRoom(this.name) */
    action_begin();
    Deriv *disp = omap_get(&n->roomDisposers, 0, r->name);
    if (disp) {
      derivation_dispose(disp);
      omap_delete(&n->roomDisposers, 0, r->name);
      omap_delete(&n->lastMessages, 0, r->name);
    }
    action_end();
  }
  action_end();
}
/* room.members: Array.from(people.values()) */
static size_t room_members(Room *r, Person **out, size_t cap) {
  MEnt *es;
  size_t cnt;
  omap_entries(&r->people, &es, &cnt);
  size_t k = cnt < cap ? cnt : cap;
  for (size_t i = 0; i < k; i++) out[i] = es[i].v;
  return cnt;
}

static void notif_update_last_message(Notifications *n, const char *roomName, Message *m) {
  action_begin();
  omap_set(&n->lastMessages, 0, roomName, m);
  action_end();
}
typedef struct { Notifications *n; Room *r; } RoomReaction;
static void *reaction_expr_room_last(void *ctx) { RoomReaction *rr = ctx; return computed_get(rr->r->lastMessage); }
static void reaction_effect_update(void *ctx, void *value) {
  RoomReaction *rr = ctx;
  if (value) notif_update_last_message(rr->n, rr->r->name, value);
}
static void notif_add_room(Notifications *n, Room *r) {
  action_begin();
  RoomReaction *rr = calloc(1, sizeof *rr);
  rr->n = n;
  rr->r = r;
  Deriv *disp = reaction_new(reaction_expr_room_last, reaction_effect_update, rr);
  omap_set(&n->roomDisposers, 0, r->name, disp);
  action_end();
}
static Notifications *notifications_new(Room **rooms, size_t nrooms) {
  Notifications *n = calloc(1, sizeof *n);
  for (size_t i = 0; i < nrooms; i++) notif_add_room(n, rooms[i]);
  return n;
}

/* the two autoruns of runTest */
typedef struct { Notifications *n; } NotifCtx;
static void autorun_notifications(void *ctx) {
  NotifCtx *c = ctx;
  log_line("\n--- Global Notifications (Last Messages) ---");
  if (omap_size(&c->n->lastMessages) == 0) {
    log_line("No messages yet.");
  } else {
    MEnt *es;
    size_t cnt;
    omap_entries(&c->n->lastMessages, &es, &cnt);
    for (size_t i = 0; i < cnt; i++) {
      Message *m = es[i].v;
      log_line("[%s] \"%s\" - %s", es[i].sk, ostr_get(&m->text), ostr_get(&m->author->name));
    }
  }
  log_line("------------------------------------------");
}
typedef struct { Room **rooms; size_t n; } RoomsCtx;
static void autorun_room_updates(void *ctx) {
  RoomsCtx *c = ctx;
  for (size_t i = 0; i < c->n; i++) {
    Room *r = c->rooms[i];
    size_t len = oarr_len(&r->roomUpdates);
    if (len > 0) log_line("[%s Update] %s", r->name, (const char *)oarr_at(&r->roomUpdates, len - 1));
  }
}
static int oarr_includes_str(OArr *a, const char *s) {
  size_t len = oarr_len(a);
  for (size_t i = 0; i < len; i++) if (strcmp((const char *)oarr_at(a, i), s) == 0) return 1;
  return 0;
}

/* runTest(): the scenario of src/test.mjs */
static void run_test(void) {
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

  log_line("\n--- Setting up Large Meeting Room ---");
  Person **largeMembers = calloc(200, sizeof *largeMembers);
  size_t nLarge = 0;
  for (int i = 3; i < 203; i++) {
    char nm[32];
    snprintf(nm, sizeof nm, "Person %d", i);
    Person *p = person_new(i, keep(nm));
    largeMembers[nLarge++] = p;
    room_add_person(largeRoom, p);
  }
  log_line("[INFO] Added %zu members to the large room.", omap_size(&largeRoom->people));
  room_add_person(largeRoom, alice);
  room_add_person(largeRoom, bob);
  log_line("[INFO] Added Alice and Bob to the Large Meeting Room.");

  check(omap_size(&alice->rooms) == 3, "alice.rooms.size === 3");
  check(omap_has(&alice->rooms, (int64_t)(intptr_t)techRoom, NULL) && omap_has(&alice->rooms, (int64_t)(intptr_t)generalRoom, NULL) && omap_has(&alice->rooms, (int64_t)(intptr_t)largeRoom, NULL), "alice.rooms.has");
  check(omap_size(&bob->rooms) == 2 && omap_has(&bob->rooms, (int64_t)(intptr_t)techRoom, NULL) && omap_has(&bob->rooms, (int64_t)(intptr_t)largeRoom, NULL), "bob.rooms");
  log_line("[PASS] Verified Person.rooms associations with Set.");

  NotifCtx nctx = {notifications};
  Deriv *notificationsDisposer = autorun_new(autorun_notifications, &nctx);
  RoomsCtx rctx = {rooms, 3};
  Deriv *roomUpdatesDisposer = autorun_new(autorun_room_updates, &rctx);

  log_line("\n--- Status Update Scenario ---");
  person_set_status(alice, "Away");
  check(oarr_includes_str(&techRoom->roomUpdates, "Alice is now Away"), "tech Alice Away");
  check(oarr_includes_str(&generalRoom->roomUpdates, "Alice is now Away"), "general Alice Away");
  check(oarr_includes_str(&largeRoom->roomUpdates, "Alice is now Away"), "large Alice Away");
  log_line("[PASS] Verified that status updates are sent to all of Alice's rooms.");
  person_set_status(bob, "Busy");
  check(oarr_includes_str(&techRoom->roomUpdates, "Bob is now Busy"), "tech Bob Busy");
  check(!oarr_includes_str(&generalRoom->roomUpdates, "Bob is now Busy"), "general no Bob Busy");
  check(oarr_includes_str(&largeRoom->roomUpdates, "Bob is now Busy"), "large Bob Busy");
  log_line("[PASS] Verified that status updates are only sent to Bob's rooms.");

  log_line("\n--- Sending Messages & Verifying Notifications ---");
  Message *msg1 = message_new("First message to Tech Talk!", alice);
  room_add_message(techRoom, msg1);
  check(omap_get(&notifications->lastMessages, 0, "Tech Talk") == msg1, "tech last message");
  log_line("[PASS] Verified Tech Talk notification.");

  for (int i = 0; i < 20; i++) {
    char txt[32];
    snprintf(txt, sizeof txt, "ping %d", i);
    Message *m = message_new(keep(txt), alice);
    room_add_message(generalRoom, m);
    check(omap_get(&notifications->lastMessages, 0, "General Chat") == m, "general last message");
  }
  log_line("[PASS] Verified General Chat notification.");

  for (int i = 0; i <= 5; i++) {
    for (size_t k = 0; k < nLarge; k++) {
      Person *member = largeMembers[k];
      char txt[128];
      snprintf(txt, sizeof txt, "Hi, I am %s", ostr_get(&member->name));
      Message *m = message_new(keep(txt), member);
      room_add_message(largeRoom, m);
      check(computed_get(largeRoom->lastMessage) == m, "large last message");
      check(omap_get(&notifications->lastMessages, 0, "Large Meeting Room") == m, "large notification");
    }
  }
  log_line("[PASS] Verified Large Meeting Room notification.");

  for (int i = 0; i < 20; i++) {
    char txt[32];
    snprintf(txt, sizeof txt, "ping %d", i);
    Message *m = message_new(keep(txt), bob);
    room_add_message(techRoom, m);
    check(omap_get(&notifications->lastMessages, 0, "Tech Talk") == m, "tech notification update");
  }
  log_line("[PASS] Verified Tech Talk notification update.");

  Room *tempRooms[10];
  for (int i = 0; i < 10; i++) {
    char roomId[64];
    snprintf(roomId, sizeof roomId, "General Chat %d", i);
    const char *rid = keep(roomId);
    Room *tempRoom = room_new(rid);
    tempRooms[i] = tempRoom;
    notif_add_room(notifications, tempRoom);
    for (size_t k = 0; k < 10; k++) room_add_person(tempRoom, largeMembers[k]);
    Person *first5[5];
    size_t nm = room_members(tempRoom, first5, 5);
    if (nm > 5) nm = 5;
    for (size_t k = 0; k < nm; k++) {
      char txt[64];
      snprintf(txt, sizeof txt, "ping %d", i);
      Message *m = message_new(keep(txt), first5[k]);
      room_add_message(tempRoom, m);
      check(omap_get(&notifications->lastMessages, 0, rid) == m, "temp room notification");
    }
  }
  for (int i = 0; i < 10; i++) room_dispose(tempRooms[i], notifications);

  derivation_dispose(notificationsDisposer);
  derivation_dispose(roomUpdatesDisposer);
  room_dispose(techRoom, notifications);
  room_dispose(generalRoom, notifications);
  room_dispose(largeRoom, notifications);
  free(largeMembers);
}

/* ================= the bundle: per-iteration copies, parse, and the run ================= */
static const char *CACHE_BUST = "\n/*ThouShaltNotCache*/\n";

/* StartupBenchmark's createIterationSourceCode: each cache-busting comment becomes a numbered comment */
static char *iteration_source(const char *bundle, size_t n, int iteration, size_t *out_n) {
  int cacheId = (iteration / 8) * 8;
  char id[32];
  int idn = snprintf(id, sizeof id, "\n/*%d*/\n", cacheId);
  size_t cl = strlen(CACHE_BUST);
  char *out = malloc(n * 2 + 64);
  size_t o = 0;
  for (size_t i = 0; i < n; ) {
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

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : 30;
  const char *root = argc > 2 ? argv[2] : "quench-bench/jetstream";
  char path[4096];
  snprintf(path, sizeof path, "%s/mobx/dist/bundle.es6.min.js", root);
  size_t bn;
  char *bundle = js_read_file(path, &bn);
  JsContext ctx;
  js_context_init(&ctx);
  double total = 0;
  int ok = 1;
  for (int it = 0; it < iterations; it++) {
    size_t n;
    char *src = iteration_source(bundle, bn, it, &n);   /* made outside the timed part, as in the JS */
    double t0 = js_now_ms();
    size_t tokens = 0;
    js_parse(&ctx, src, n, 0, &tokens);                  /* compile the iteration's bundle */
    free(src);
    run_test();
    double t1 = js_now_ms();
    if (g_logs.n != 6824) {
      fprintf(stderr, "mobx-startup: expected 6824 log entries, got %zu\n", g_logs.n);
      ok = 0;
    }
    total += t1 - t0;
    free_strings();
  }
  printf("mobx-startup %d iterations %.3f ms (average %.3f ms)\n", iterations, total, total / iterations);
  printf("validation %s: each run's log has 6824 entries\n", ok ? "ok" : "FAILED");
  js_context_destroy(&ctx);
  free(bundle);
  return ok ? 0 : 1;
}
