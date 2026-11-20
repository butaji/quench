// jetstream-row: doxbee-promise
// jetstream-files: simple/doxbee-promise.js
// iterations: 120
/*
 * C analogue of JetStream's doxbee-promise (simple/doxbee-promise.js). Each iteration
 * starts 10,000 doxbee(i, "foo") calls, keeps their promises in an array, then drains the
 * work queue and waits for all of them (Promise.all). The fake database is the same as
 * the JS one: a blob manager, a transaction, query objects that all resolve immediately,
 * version and file records, and a commit. The file does not exist, because the fake
 * get() resolves to undefined, so every call takes the create-file branch.
 *
 * Execution model: each call is a chain of continuation records. A continuation is a
 * heap-allocated record {stage, call} put on a FIFO queue; running one performs the work
 * of the JS callback for that stage and enqueues the next. The queue is drained in order,
 * so the calls interleave one reaction at a time as the JS microtask queue does. A call
 * runs 9 .then/.catch reactions, and the element for Promise.all is one more task, so 10
 * tasks per call. The reactions, their order and the objects each step allocates
 * (blob, transaction, query objects, version, file records) follow the JS.
 *
 * Deliberate differences: the JS promise machinery itself (promise objects, the extra
 * ticks when a callback returns a promise, and V8's job queue) is not modeled; the
 * tasks are the reactions only. Object lifetimes are managed by the call, which is freed
 * when its Promise.all element runs (the JS GC does this). The JS has no validate step;
 * the program checks that all calls completed and that every task ran. */
#define _POSIX_C_SOURCE 199309L
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

enum { CALLS_PER_ITERATION = 10000, TASKS_PER_CALL = 10, DEFAULT_ITERATIONS = 120, MAX_CALL_OBJECTS = 24 };

static void fatal(const char *msg) {
  fprintf(stderr, "Error: %s\n", msg);
  exit(1);
}

/* The fake database objects (simple/doxbee-promise.js lib/fakes-promises.js). */
typedef struct { int64_t id; } UserAccount;
typedef struct { int64_t user_account_id, blob_id, creator_id, previous_id; int has_previous; int64_t id; } Version;
typedef struct { int dummy; } Blob;
typedef struct { int state; } Transaction;
typedef struct { const char *name; int64_t id, version; } QueryRecord;
typedef struct { int kind; } Queryish;

static UserAccount user_account = {1};

typedef enum {
  ST_BLOB_PUT = 1,   /* blob.put resolved: blobId = blobIdV, start byUuidOrPath().get() */
  ST_GET,            /* get resolved: build the version, insert it */
  ST_VERSION,        /* version inserted: create the file record */
  ST_CREATE_EXEC,    /* createQuery resolved: q.execWithin(tx) */
  ST_NEW_ID,         /* execWithin resolved: fileId = newId */
  ST_FILE_VERSION,   /* FileVersion.insert(...).execWithin(tx) */
  ST_WHERE_UPDATE,   /* File.whereUpdate(...).execWithin(tx) */
  ST_COMMIT,         /* tx.commit() */
  ST_CATCH,          /* .catch pass-through */
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
  void *objs[MAX_CALL_OBJECTS];   /* everything the call allocated, freed at ST_ELEMENT */
  int nobjs;
} Call;

typedef struct Task {
  int stage;
  Call *call;
  struct Task *next;
} Task;

static Task *queue_head, *queue_tail;
static Call **promise_slots;
static long tasks_run, calls_completed;

static void schedule(Call *c, int stage) {
  Task *t = malloc(sizeof *t);
  if (!t) fatal("out of memory");
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
  void *p = calloc(1, size);
  if (!p) fatal("out of memory");
  c->objs[c->nobjs++] = p;
  return p;
}

/* queryish(): an object whose execWithin/get resolve immediately. */
static Queryish *queryish(Call *c) {
  return call_alloc(c, sizeof(Queryish));
}

/* Version.createHash(v) in the fake returns 1. */
static int64_t version_create_hash(const Version *v) {
  (void)v;
  return 1;
}

/* idOrPath.lastIndexOf("/") + 1 and substring: the file name after the last slash. */
static const char *file_name_of(const char *path) {
  const char *slash = strrchr(path, '/');
  return slash ? slash + 1 : path;
}

static Call *doxbee_start(int index, const char *id_or_path, int64_t stream) {
  Call *c = calloc(1, sizeof *c);
  if (!c) fatal("out of memory");
  c->index = index;
  c->id_or_path = id_or_path;
  c->stream = stream;
  c->blob = call_alloc(c, sizeof(Blob));          /* blobManager.create(account) */
  c->tx = call_alloc(c, sizeof(Transaction));     /* db.begin() */
  schedule(c, ST_BLOB_PUT);                       /* blob.put(stream) resolves */
  return c;
}

static void run_task(Task *t) {
  Call *c = t->call;
  switch (t->stage) {
  case ST_BLOB_PUT:
    c->blob_id = 0;                               /* blobIdV is undefined in the fake */
    queryish(c);                                  /* byUuidOrPath(idOrPath) */
    schedule(c, ST_GET);
    break;
  case ST_GET: {
    /* file is undefined, so previousId is null and the create branch runs */
    Version *v = call_alloc(c, sizeof(Version));
    v->user_account_id = user_account.id;
    v->blob_id = c->blob_id;
    v->creator_id = user_account.id;
    v->has_previous = 0;
    v->id = version_create_hash(v);
    c->version = v;
    queryish(c);                                  /* Version.insert(version) */
    schedule(c, ST_VERSION);
    break;
  }
  case ST_VERSION: {
    /* !file: fileName = last segment; createQuery(idOrPath, {...}) resolves to a query */
    const char *name = file_name_of(c->id_or_path);
    QueryRecord *rec = call_alloc(c, sizeof(QueryRecord));
    rec->name = name;
    rec->id = c->index;                           /* uuid.v1() is undefined; index stands in */
    rec->version = c->version->id;
    schedule(c, ST_CREATE_EXEC);
    break;
  }
  case ST_CREATE_EXEC:
    queryish(c);                                  /* the cqQueryish execWithin(tx) */
    schedule(c, ST_NEW_ID);
    break;
  case ST_NEW_ID:
    c->file_id = c->index;
    schedule(c, ST_FILE_VERSION);
    break;
  case ST_FILE_VERSION: {
    QueryRecord *rec = call_alloc(c, sizeof(QueryRecord));  /* {fileId, versionId} */
    rec->id = c->file_id;
    rec->version = c->version->id;
    queryish(c);
    schedule(c, ST_WHERE_UPDATE);
    break;
  }
  case ST_WHERE_UPDATE: {
    QueryRecord *rec = call_alloc(c, sizeof(QueryRecord));  /* {id}, {version} */
    rec->id = c->file_id;
    rec->version = c->version->id;
    queryish(c);
    schedule(c, ST_COMMIT);
    break;
  }
  case ST_COMMIT:
    c->tx->state = 1;                             /* tx.commit() */
    schedule(c, ST_CATCH);
    break;
  case ST_CATCH:
    schedule(c, ST_ELEMENT);
    break;
  case ST_ELEMENT:
    promise_slots[c->index] = NULL;
    for (int i = 0; i < c->nobjs; i++) free(c->objs[i]);
    free(c);
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
    free(t);
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
  printf("doxbee-promise %d iterations %.3f ms\n", iterations, elapsed);
  printf("validated: %ld calls, %ld tasks\n", calls_completed, tasks_run);
  return 0;
}
