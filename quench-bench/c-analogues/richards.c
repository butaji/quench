/* C analogue of JetStream's Octane/richards.js: the same objects, links, dynamic dispatch
 * through task function pointers and heap allocation per run, so a JS engine's native
 * code is compared with what a C programmer writes for the same program. */
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

enum { ID_IDLE, ID_WORKER, ID_HANDLER_A, ID_HANDLER_B, ID_DEVICE_A, ID_DEVICE_B, NUMBER_OF_IDS };
enum { KIND_DEVICE, KIND_WORK };
enum { STATE_RUNNING = 0, STATE_RUNNABLE = 1, STATE_SUSPENDED = 2, STATE_HELD = 4,
       STATE_SUSPENDED_RUNNABLE = STATE_SUSPENDED | STATE_RUNNABLE };
enum { DATA_SIZE = 4, COUNT = 1000, EXPECTED_QUEUE_COUNT = 2322, EXPECTED_HOLD_COUNT = 928 };

typedef struct Packet { struct Packet *link; int id, kind, a1; int a2[DATA_SIZE]; } Packet;
struct Scheduler;
struct Tcb;
typedef struct Task { struct Tcb *(*run)(struct Task *, Packet *); struct Scheduler *scheduler; } Task;
typedef struct Tcb { struct Tcb *link; int id, priority; Packet *queue; Task *task; int state; } Tcb;
typedef struct Scheduler { int queueCount, holdCount; Tcb *blocks[NUMBER_OF_IDS]; Tcb *list, *currentTcb; int currentId; } Scheduler;
typedef struct { Task base; int v1, count; } IdleTask;
typedef struct { Task base; Packet *v1; } DeviceTask;
typedef struct { Task base; int v1, v2; } WorkerTask;
typedef struct { Task base; Packet *v1, *v2; } HandlerTask;

enum { PACKETS_PER_RUN = 8 };
static Packet *packets[PACKETS_PER_RUN];
static int packet_count;
static Packet *packet_new(Packet *link, int id, int kind) {
  Packet *p = packets[packet_count++] = malloc(sizeof *p);
  p->link = link; p->id = id; p->kind = kind; p->a1 = 0;
  for (int i = 0; i < DATA_SIZE; i++) p->a2[i] = 0;
  return p;
}
static Packet *add_to(Packet *self, Packet *queue) {
  self->link = NULL;
  if (!queue) return self;
  Packet *peek, *next = queue;
  while ((peek = next->link)) next = peek;
  next->link = self;
  return queue;
}
static int held_or_suspended(Tcb *t) { return (t->state & STATE_HELD) || t->state == STATE_SUSPENDED; }
static Tcb *tcb_run(Tcb *t) {
  Packet *packet;
  if (t->state == STATE_SUSPENDED_RUNNABLE) {
    packet = t->queue; t->queue = packet->link;
    t->state = t->queue ? STATE_RUNNABLE : STATE_RUNNING;
  } else packet = NULL;
  return t->task->run(t->task, packet);
}
static Tcb *check_priority_add(Tcb *t, Tcb *task, Packet *packet) {
  if (!t->queue) { t->queue = packet; t->state |= STATE_RUNNABLE; if (t->priority > task->priority) return t; }
  else t->queue = add_to(packet, t->queue);
  return task;
}
static Tcb *release(Scheduler *s, int id) {
  Tcb *t = s->blocks[id];
  if (!t) return t;
  t->state &= ~STATE_HELD;
  return t->priority > s->currentTcb->priority ? t : s->currentTcb;
}
static Tcb *hold_current(Scheduler *s) { s->holdCount++; s->currentTcb->state |= STATE_HELD; return s->currentTcb->link; }
static Tcb *suspend_current(Scheduler *s) { s->currentTcb->state |= STATE_SUSPENDED; return s->currentTcb; }
static Tcb *queue(Scheduler *s, Packet *packet) {
  Tcb *t = s->blocks[packet->id];
  if (!t) return t;
  s->queueCount++; packet->link = NULL; packet->id = s->currentId;
  return check_priority_add(t, s->currentTcb, packet);
}
static Tcb *idle_run(Task *task, Packet *packet) {
  (void)packet; IdleTask *t = (IdleTask *)task;
  if (--t->count == 0) return hold_current(task->scheduler);
  if ((t->v1 & 1) == 0) { t->v1 >>= 1; return release(task->scheduler, ID_DEVICE_A); }
  t->v1 = (t->v1 >> 1) ^ 0xD008; return release(task->scheduler, ID_DEVICE_B);
}
static Tcb *device_run(Task *task, Packet *packet) {
  DeviceTask *t = (DeviceTask *)task;
  if (!packet) {
    if (!t->v1) return suspend_current(task->scheduler);
    Packet *v = t->v1; t->v1 = NULL; return queue(task->scheduler, v);
  }
  t->v1 = packet; return hold_current(task->scheduler);
}
static Tcb *worker_run(Task *task, Packet *packet) {
  WorkerTask *t = (WorkerTask *)task;
  if (!packet) return suspend_current(task->scheduler);
  t->v1 = t->v1 == ID_HANDLER_A ? ID_HANDLER_B : ID_HANDLER_A;
  packet->id = t->v1; packet->a1 = 0;
  for (int i = 0; i < DATA_SIZE; i++) { if (++t->v2 > 26) t->v2 = 1; packet->a2[i] = t->v2; }
  return queue(task->scheduler, packet);
}
static Tcb *handler_run(Task *task, Packet *packet) {
  HandlerTask *t = (HandlerTask *)task;
  if (packet) { if (packet->kind == KIND_WORK) t->v1 = add_to(packet, t->v1); else t->v2 = add_to(packet, t->v2); }
  if (t->v1) {
    int count = t->v1->a1;
    if (count < DATA_SIZE) {
      if (t->v2) { Packet *v = t->v2; t->v2 = t->v2->link; v->a1 = t->v1->a2[count]; t->v1->a1 = count + 1; return queue(task->scheduler, v); }
    } else { Packet *v = t->v1; t->v1 = t->v1->link; return queue(task->scheduler, v); }
  }
  return suspend_current(task->scheduler);
}
static void add_task(Scheduler *s, int id, int priority, Packet *q, Task *task) {
  Tcb *t = malloc(sizeof *t);
  t->link = s->list; t->id = id; t->priority = priority; t->queue = q; t->task = task;
  t->state = q ? STATE_SUSPENDED_RUNNABLE : STATE_SUSPENDED;
  s->currentTcb = t; s->list = t; s->blocks[id] = t;
}
static void run_richards(void) {
  Scheduler *s = calloc(1, sizeof *s);
  IdleTask *idle = malloc(sizeof *idle); *idle = (IdleTask){{idle_run, s}, 1, COUNT};
  add_task(s, ID_IDLE, 0, NULL, &idle->base); s->currentTcb->state = STATE_RUNNING;
  Packet *q = packet_new(packet_new(NULL, ID_WORKER, KIND_WORK), ID_WORKER, KIND_WORK);
  WorkerTask *w = malloc(sizeof *w); *w = (WorkerTask){{worker_run, s}, ID_HANDLER_A, 0};
  add_task(s, ID_WORKER, 1000, q, &w->base);
  q = packet_new(packet_new(packet_new(NULL, ID_DEVICE_A, KIND_DEVICE), ID_DEVICE_A, KIND_DEVICE), ID_DEVICE_A, KIND_DEVICE);
  HandlerTask *ha = malloc(sizeof *ha); *ha = (HandlerTask){{handler_run, s}, NULL, NULL};
  add_task(s, ID_HANDLER_A, 2000, q, &ha->base);
  q = packet_new(packet_new(packet_new(NULL, ID_DEVICE_B, KIND_DEVICE), ID_DEVICE_B, KIND_DEVICE), ID_DEVICE_B, KIND_DEVICE);
  HandlerTask *hb = malloc(sizeof *hb); *hb = (HandlerTask){{handler_run, s}, NULL, NULL};
  add_task(s, ID_HANDLER_B, 3000, q, &hb->base);
  DeviceTask *da = malloc(sizeof *da); *da = (DeviceTask){{device_run, s}, NULL};
  add_task(s, ID_DEVICE_A, 4000, NULL, &da->base);
  DeviceTask *db = malloc(sizeof *db); *db = (DeviceTask){{device_run, s}, NULL};
  add_task(s, ID_DEVICE_B, 5000, NULL, &db->base);
  s->currentTcb = s->list;
  while (s->currentTcb) {
    if (held_or_suspended(s->currentTcb)) s->currentTcb = s->currentTcb->link;
    else { s->currentId = s->currentTcb->id; s->currentTcb = tcb_run(s->currentTcb); }
  }
  if (s->queueCount != EXPECTED_QUEUE_COUNT || s->holdCount != EXPECTED_HOLD_COUNT) {
    fprintf(stderr, "Error during execution: queueCount = %d, holdCount = %d.\n", s->queueCount, s->holdCount);
    exit(1);
  }
  /* JS garbage is collected; C frees the same objects. */
  for (int i = 0; i < packet_count; i++) free(packets[i]);
  packet_count = 0;
  for (Tcb *t = s->list, *n; t; t = n) { n = t->link; free(t->task); free(t); }
  free(s);
}
int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : 120;
  struct timespec a, b;
  clock_gettime(CLOCK_MONOTONIC, &a);
  for (int i = 0; i < iterations; i++) for (int j = 0; j < 50; j++) run_richards();
  clock_gettime(CLOCK_MONOTONIC, &b);
  printf("richards %d iterations %.3f ms\n", iterations, (b.tv_sec - a.tv_sec) * 1e3 + (b.tv_nsec - a.tv_nsec) / 1e6);
  return 0;
}
