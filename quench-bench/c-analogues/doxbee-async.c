// jetstream-row: doxbee-async
// jetstream-files: simple/doxbee-async.js
// iterations: 120
// jetstream-driver: class=Async
/*
 * C analogue of JetStream's doxbee-async (simple/doxbee-async.js): the same fake database
 * and the same 10,000 doxbee(i, "foo") calls per iteration, followed by Promise.all, as in
 * doxbee-promise.c. The JS version is an async function, so each call is a sequence of
 * await points, and each await resumption is one task on the FIFO queue, with the work of
 * the code between two awaits done in that task. A call has 8 awaits (blob put, get,
 * Version insert, createQuery, execWithin, FileVersion insert, whereUpdate, commit), so
 * with the Promise.all element that is 9 tasks per call. The name is taken from
 * idOrPath.split("/"), which allocates an array as the JS does.
 *
 * Tasks and calls are recycled through free lists and a call's objects are bump-allocated
 * inside it (the C counterpart of the engine's nursery allocation and GC).
 * Deliberate differences: the JS promise and async machinery (promise objects, the
 * microtask queue, the extra ticks V8 adds for an async function that returns a promise)
 * is not modeled; the tasks are the resumptions. Object lifetimes are the call's, freed
 * when its Promise.all element runs. The JS has no validate step; the program checks that
 * every call completed and every task ran. */
#define _POSIX_C_SOURCE 199309L
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

enum { CALLS_PER_ITERATION = 10000, TASKS_PER_CALL = 9, DEFAULT_ITERATIONS = 120, MAX_CALL_OBJECTS = 24, CALL_ARENA_BYTES = 512 };

static void fatal(const char *msg) {
  fprintf(stderr, "Error: %s\n", msg);
  exit(1);
}

/* The fake database objects (simple/doxbee-async.js lib/fakes-async.js). */
typedef struct { int64_t id; } UserAccount;
typedef struct { int64_t user_account_id, blob_id, creator_id, previous_id; int has_previous; int64_t id; } Version;
typedef struct { int dummy; } Blob;
typedef struct { int state; } Transaction;
typedef struct { const char *name; int64_t id, version; } QueryRecord;
typedef struct { int kind; } Queryish;

static UserAccount user_account = {1};

typedef enum {
  ST_PUT_DONE = 1,   /* await blob.put: then byUuidOrPath().get() */
  ST_GET_DONE,       /* await get: build and insert the version */
  ST_VERSION_DONE,   /* await Version.insert: create the file record */
  ST_CREATE_DONE,    /* await createQuery: await q.execWithin(tx) */
  ST_EXEC_DONE,      /* await execWithin: await FileVersion.insert */
  ST_FILE_VERSION_DONE, /* await FileVersion insert: await whereUpdate */
  ST_WHERE_DONE,     /* await whereUpdate: await tx.commit() */
  ST_COMMIT_DONE,    /* await commit: the function returns */
  ST_ELEMENT         /* Promise.all element resolved */
} Stage;

typedef struct Call {
  int index;
  const char *id_or_path;
  int64_t stream;
  Blob *blob;
  Transaction *tx;
  int64_t blob_id;
  Version *version;
  int64_t file_id;
  int nobjs;                      /* objects the call allocated, all freed with it at ST_ELEMENT */
  size_t arena_used;
  _Alignas(16) unsigned char arena[CALL_ARENA_BYTES]; /* the call's objects, bump-allocated */
  struct Call *next_free;
} Call;

/* Free lists: tasks and calls are recycled as the engine's GC recycles promise reactions and
 * async frames; a call's objects are bump-allocated inside it and die with it. */
static Call *free_calls;
static struct Task *free_tasks;

typedef struct Task {
  int stage;
  Call *call;
  struct Task *next;
} Task;

static Task *queue_head, *queue_tail;
static Call **promise_slots;
static long tasks_run, calls_completed;

static void schedule(Call *c, int stage) {
  Task *t = free_tasks;
  if (t) free_tasks = t->next;
  else if (!(t = malloc(sizeof *t))) fatal("out of memory");
  t->stage = stage;
  t->call = c;
  t->next = NULL;
  if (queue_tail) queue_tail->next = t;
  else queue_head = t;
  queue_tail = t;
}

static Task *dequeue(void) {
  Task *t = queue_head;
  if (!t) return NULL;
  queue_head = t->next;
  if (!queue_head) queue_tail = NULL;
  return t;
}

static void *call_alloc(Call *c, size_t size) {
  if (c->nobjs == MAX_CALL_OBJECTS) fatal("too many objects for one call");
  size_t at = (c->arena_used + 15) & ~(size_t)15;
  if (at + size > CALL_ARENA_BYTES) fatal("call arena full");
  void *p = c->arena + at;
  memset(p, 0, size);
  c->arena_used = at + size;
  c->nobjs++;
  return p;
}

static Queryish *queryish(Call *c) {
  return call_alloc(c, sizeof(Queryish));
}

static int64_t version_create_hash(const Version *v) {
  (void)v;
  return 1;
}

/* idOrPath.split("/") allocates the parts; the file name is the last one. */
static const char *last_segment(Call *c, const char *path) {
  size_t parts = 1;
  for (const char *p = path; *p; p++)
    if (*p == '/') parts++;
  const char **array = call_alloc(c, parts * sizeof *array);
  size_t k = 0;
  const char *start = path;
  for (const char *p = path;; p++) {
    if (*p == '/' || *p == 0) {
      array[k++] = start;
      if (!*p) break;
      start = p + 1;
    }
  }
  return array[parts - 1];
}

static Call *doxbee_start(int index, const char *id_or_path, int64_t stream) {
  Call *c = free_calls;
  if (c) free_calls = c->next_free;
  else if (!(c = malloc(sizeof *c))) fatal("out of memory");
  memset(c, 0, offsetof(Call, arena));
  c->index = index;
  c->id_or_path = id_or_path;
  c->stream = stream;
  c->blob = call_alloc(c, sizeof(Blob));          /* blobManager.create(account) */
  c->tx = call_alloc(c, sizeof(Transaction));     /* db.begin() */
  schedule(c, ST_PUT_DONE);                       /* await blob.put(stream) */
  return c;
}

static void run_task(Task *t) {
  Call *c = t->call;
  switch (t->stage) {
  case ST_PUT_DONE:
    c->blob_id = 0;                               /* blobId is undefined in the fake */
    queryish(c);                                  /* byUuidOrPath(idOrPath).get() */
    schedule(c, ST_GET_DONE);
    break;
  case ST_GET_DONE: {
    /* file is undefined, so previousId is null */
    Version *v = call_alloc(c, sizeof(Version));
    v->user_account_id = user_account.id;
    v->blob_id = c->blob_id;
    v->creator_id = user_account.id;
    v->has_previous = 0;
    v->id = version_create_hash(v);
    c->version = v;
    queryish(c);                                  /* Version.insert(version).execWithin(tx) */
    schedule(c, ST_VERSION_DONE);
    break;
  }
  case ST_VERSION_DONE: {
    const char *name = last_segment(c, c->id_or_path);
    QueryRecord *rec = call_alloc(c, sizeof(QueryRecord));   /* createQuery(idOrPath, {...}) */
    rec->name = name;
    rec->id = c->index;                           /* uuid.v1() is undefined; index stands in */
    rec->version = c->version->id;
    schedule(c, ST_CREATE_DONE);
    break;
  }
  case ST_CREATE_DONE:
    queryish(c);                                  /* q.execWithin(tx) */
    schedule(c, ST_EXEC_DONE);
    break;
  case ST_EXEC_DONE: {
    QueryRecord *rec = call_alloc(c, sizeof(QueryRecord));   /* {fileId, versionId} */
    rec->id = c->index;
    rec->version = c->version->id;
    queryish(c);
    schedule(c, ST_FILE_VERSION_DONE);
    break;
  }
  case ST_FILE_VERSION_DONE: {
    QueryRecord *rec = call_alloc(c, sizeof(QueryRecord));  /* whereUpdate({id}, {version}) */
    rec->id = c->index;
    rec->version = c->version->id;
    queryish(c);
    schedule(c, ST_WHERE_DONE);
    break;
  }
  case ST_WHERE_DONE:
    schedule(c, ST_COMMIT_DONE);                  /* tx.commit() is awaited next */
    break;
  case ST_COMMIT_DONE:
    c->tx->state = 1;                             /* commit has run; the async function returns */
    schedule(c, ST_ELEMENT);
    break;
  case ST_ELEMENT:
    promise_slots[c->index] = NULL;
    c->next_free = free_calls;                    /* the call and its objects die together */
    free_calls = c;
    calls_completed++;
    break;
  default:
    fatal("unknown stage");
  }
  tasks_run++;
}

static void run_iteration(void) {
  Call **promises = malloc(CALLS_PER_ITERATION * sizeof *promises);  /* new Array(10_000) */
  if (!promises) fatal("out of memory");
  promise_slots = promises;
  for (int i = 0; i < CALLS_PER_ITERATION; i++) {
    promises[i] = doxbee_start(i, "foo", i);
  }
  while (queue_head) {
    Task *t = dequeue();
    run_task(t);
    t->next = free_tasks;
    free_tasks = t;
  }
  free(promises);
}

static double now_ms(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return t.tv_sec * 1e3 + t.tv_nsec / 1e6;
}

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : DEFAULT_ITERATIONS;
  double start = now_ms();
  for (int it = 0; it < iterations; it++) run_iteration();
  double elapsed = now_ms() - start;
  long expected_calls = (long)CALLS_PER_ITERATION * iterations;
  if (calls_completed != expected_calls || tasks_run != expected_calls * TASKS_PER_CALL) {
    fprintf(stderr, "Error: completed %ld of %ld calls, %ld tasks\n", calls_completed, expected_calls, tasks_run);
    return 1;
  }
  printf("doxbee-async %d iterations %.3f ms\n", iterations, elapsed);
  printf("validated: %ld calls, %ld tasks\n", calls_completed, tasks_run);
  return 0;
}
