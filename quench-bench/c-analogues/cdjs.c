// jetstream-row: cdjs
// jetstream-files: cdjs/constants.js cdjs/util.js cdjs/red_black_tree.js cdjs/call_sign.js cdjs/vector_2d.js cdjs/vector_3d.js cdjs/motion.js cdjs/reduce_collision_set.js cdjs/simulator.js cdjs/collision.js cdjs/collision_detector.js cdjs/benchmark.js
// iterations: 60
/*
 * C analogue of JetStream's cdjs (Benchmark.runIteration -> benchmark()). Each iteration
 * builds 1000 aircraft call signs, then runs 18 frames of the same simulation: every
 * aircraft gets a position from cos/sin of the frame time, the collision detector keeps
 * the last position of each call sign in a red-black tree and turns each frame into
 * motions, flood-fills each motion through 2x2 voxels into a voxel red-black tree, and
 * tests every pair of motions that share a voxel with the same closed-form intersection
 * test. The iteration checks the same total of 1336 collisions as benchmark.js.
 *
 * Mirrored data structures: the red-black tree with the same insert fix-up and in-order
 * successor walk (keys are copied into nodes, values are opaque pointers), the per-motion
 * "seen" tree, the voxel map whose lists keep motions in frame order, and the frame and
 * collision records. Floating-point expressions keep the JS evaluation order so the
 * results are bit-identical with IEEE doubles (build with -ffp-contract=off to keep FMA
 * out).
 *
 * Deliberate differences: call signs are compared with strcmp, which orders the ASCII
 * alphanumeric names "foo<digits>" exactly as localeCompare does; the RedBlackTree
 * remove path is not ported because the benchmark never removes a key (every frame sees
 * the same 1000 call signs, so toRemove is always empty, and the program aborts if that
 * ever changes); Math.cos and Math.sin are the C library's, which may differ in the last
 * bit from V8's. */
#define _POSIX_C_SOURCE 199309L
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

enum {
  NUM_AIRCRAFT = 1000,
  NUM_FRAMES = 18,
  EXPECTED_COLLISIONS = 1336,
  DEFAULT_ITERATIONS = 60
};

static const double MIN_X = 0, MAX_X = 1000, MIN_Y = 0, MAX_Y = 1000, MIN_Z = 0, MAX_Z = 10;
static const double PROXIMITY_RADIUS = 1;
static const double GOOD_VOXEL_SIZE = 2; /* PROXIMITY_RADIUS * 2 */

static void fatal(const char *msg) {
  fprintf(stderr, "Error: %s\n", msg);
  exit(1);
}

/* ---- util.js ---- */
static int compare_numbers(double a, double b) {
  if (a == b) return 0;
  if (a < b) return -1;
  if (a > b) return 1;
  if (a == a) return 1;
  return -1;
}

/* ToInt32 for the finite values used here (truncation, then wrap modulo 2^32). */
static int32_t js_to_int32(double d) {
  if (!isfinite(d)) return 0;
  d = trunc(d);
  d = fmod(d, 4294967296.0);
  if (d < 0) d += 4294967296.0;
  return (int32_t)(uint32_t)d;
}

/* ---- vectors ---- */
typedef struct { double x, y; } Vec2;
typedef struct { double x, y, z; } Vec3;

static Vec2 vec2_minus(Vec2 a, Vec2 b) { Vec2 r = {a.x - b.x, a.y - b.y}; return r; }
static Vec2 vec2_plus(Vec2 a, Vec2 b) { Vec2 r = {a.x + b.x, a.y + b.y}; return r; }

static int vec2_compare(const void *pa, const void *pb) {
  const Vec2 *a = pa, *b = pb;
  int r = compare_numbers(a->x, b->x);
  if (r) return r;
  return compare_numbers(a->y, b->y);
}

static Vec3 vec3_minus(Vec3 a, Vec3 b) { Vec3 r = {a.x - b.x, a.y - b.y, a.z - b.z}; return r; }
static Vec3 vec3_plus(Vec3 a, Vec3 b) { Vec3 r = {a.x + b.x, a.y + b.y, a.z + b.z}; return r; }
static Vec3 vec3_times(Vec3 a, double k) { Vec3 r = {a.x * k, a.y * k, a.z * k}; return r; }
static double vec3_dot(Vec3 a, Vec3 b) { return a.x * b.x + a.y * b.y + a.z * b.z; }
static double vec3_squared_magnitude(Vec3 a) { return vec3_dot(a, a); }
static double vec3_magnitude(Vec3 a) { return sqrt(vec3_squared_magnitude(a)); }

/* ---- CallSign (call_sign.js); strcmp matches localeCompare on these names ---- */
typedef struct { char *name; } CallSign;

static int callsign_compare(const void *pa, const void *pb) {
  const CallSign *a = *(CallSign *const *)pa, *b = *(CallSign *const *)pb;
  int r = strcmp(a->name, b->name);
  return r < 0 ? -1 : r > 0 ? 1 : 0;
}

/* ---- RedBlackTree (red_black_tree.js) ---- */
typedef struct RBNode {
  struct RBNode *left, *right, *parent;
  int red;
  void *value;
  unsigned char key[];       /* the key, copied in */
} RBNode;

typedef struct {
  RBNode *root;
  size_t keysize;
  int (*compare)(const void *, const void *);
} RBTree;

static void rb_init(RBTree *t, size_t keysize, int (*compare)(const void *, const void *)) {
  t->root = NULL;
  t->keysize = keysize;
  t->compare = compare;
}

static RBNode *tree_minimum(RBNode *x) {
  while (x->left) x = x->left;
  return x;
}


static RBNode *successor(RBNode *x) {
  if (x->right) return tree_minimum(x->right);
  RBNode *y = x->parent;
  while (y && x == y->right) {
    x = y;
    y = y->parent;
  }
  return y;
}

static void left_rotate(RBTree *t, RBNode *x) {
  RBNode *y = x->right;
  x->right = y->left;
  if (y->left) y->left->parent = x;
  y->parent = x->parent;
  if (!x->parent) t->root = y;
  else if (x == x->parent->left) x->parent->left = y;
  else x->parent->right = y;
  y->left = x;
  x->parent = y;
}

static void right_rotate(RBTree *t, RBNode *y) {
  RBNode *x = y->left;
  y->left = x->right;
  if (x->right) x->right->parent = y;
  x->parent = y->parent;
  if (!y->parent) t->root = x;
  else if (y == y->parent->left) y->parent->left = x;
  else y->parent->right = x;
  x->right = y;
  y->parent = x;
}

static RBNode *find_node(const RBTree *t, const void *key) {
  for (RBNode *cur = t->root; cur;) {
    int c = t->compare(key, cur->key);
    if (!c) return cur;
    cur = c < 0 ? cur->left : cur->right;
  }
  return NULL;
}

/* RedBlackTree.put: returns the previous value for an existing key, NULL otherwise. */
static void *rb_put(RBTree *t, const void *key, void *value) {
  RBNode *y = NULL, *x = t->root;
  while (x) {
    y = x;
    int c = t->compare(key, x->key);
    if (c < 0) {
      x = x->left;
    } else if (c > 0) {
      x = x->right;
    } else {
      void *old = x->value;
      x->value = value;
      return old;
    }
  }
  RBNode *z = malloc(sizeof *z + t->keysize);
  if (!z) fatal("out of memory");
  z->left = z->right = NULL;
  z->parent = y;
  z->red = 1;
  z->value = value;
  memcpy(z->key, key, t->keysize);
  if (!y) t->root = z;
  else if (t->compare(key, y->key) < 0) y->left = z;
  else y->right = z;

  x = z;
  while (x != t->root && x->parent->red) {
    if (x->parent == x->parent->parent->left) {
      RBNode *uncle = x->parent->parent->right;
      if (uncle && uncle->red) {
        x->parent->red = 0;
        uncle->red = 0;
        x->parent->parent->red = 1;
        x = x->parent->parent;
      } else {
        if (x == x->parent->right) {
          x = x->parent;
          left_rotate(t, x);
        }
        x->parent->red = 0;
        x->parent->parent->red = 1;
        right_rotate(t, x->parent->parent);
      }
    } else {
      RBNode *uncle = x->parent->parent->left;
      if (uncle && uncle->red) {
        x->parent->red = 0;
        uncle->red = 0;
        x->parent->parent->red = 1;
        x = x->parent->parent;
      } else {
        if (x == x->parent->left) {
          x = x->parent;
          right_rotate(t, x);
        }
        x->parent->red = 0;
        x->parent->parent->red = 1;
        left_rotate(t, x->parent->parent);
      }
    }
  }
  t->root->red = 0;
  return NULL;
}

static void *rb_get(const RBTree *t, const void *key) {
  RBNode *n = find_node(t, key);
  return n ? n->value : NULL;
}

/* In-order walk, as RedBlackTree.forEach. */
static void rb_for_each(RBTree *t, void (*fn)(const void *key, void *value, void *ctx), void *ctx) {
  if (!t->root) return;
  for (RBNode *cur = tree_minimum(t->root); cur; cur = successor(cur)) fn(cur->key, cur->value, ctx);
}

static void rb_free_nodes(RBNode *n) {
  if (!n) return;
  rb_free_nodes(n->left);
  rb_free_nodes(n->right);
  free(n);
}

static void rb_clear(RBTree *t) {
  rb_free_nodes(t->root);
  t->root = NULL;
}

/* ---- Motion, Collision ---- */
typedef struct { const CallSign *callsign; Vec3 pos_one, pos_two; } Motion;

static Vec3 motion_delta(const Motion *m) { return vec3_minus(m->pos_two, m->pos_one); }

/* Motion.findIntersection: returns 1 and the point when the two motions collide. */
static int find_intersection(const Motion *m, const Motion *other, Vec3 *out) {
  Vec3 init1 = m->pos_one;
  Vec3 init2 = other->pos_one;
  Vec3 vec1 = motion_delta(m);
  Vec3 vec2 = motion_delta(other);
  double radius = PROXIMITY_RADIUS;
  double a = vec3_squared_magnitude(vec3_minus(vec2, vec1));
  if (a != 0) {
    double b = 2 * vec3_dot(vec3_minus(init1, init2), vec3_minus(vec1, vec2));
    double c = -radius * radius + vec3_squared_magnitude(vec3_minus(init2, init1));
    double discr = b * b - 4 * a * c;
    if (discr < 0) return 0;
    double v1 = (-b - sqrt(discr)) / (2 * a);
    double v2 = (-b + sqrt(discr)) / (2 * a);
    if (v1 <= v2 && ((v1 <= 1 && 1 <= v2) || (v1 <= 0 && 0 <= v2) || (0 <= v1 && v2 <= 1))) {
      double v = v1 <= 0 ? 0 : v1;
      Vec3 result1 = vec3_plus(init1, vec3_times(vec1, v));
      Vec3 result2 = vec3_plus(init2, vec3_times(vec2, v));
      Vec3 result = vec3_times(vec3_plus(result1, result2), 0.5);
      if (result.x >= MIN_X && result.x <= MAX_X && result.y >= MIN_Y && result.y <= MAX_Y &&
          result.z >= MIN_Z && result.z <= MAX_Z) {
        *out = result;
        return 1;
      }
    }
    return 0;
  }
  double dist = vec3_magnitude(vec3_minus(init2, init1));
  if (dist <= radius) {
    *out = vec3_times(vec3_plus(init1, init2), 0.5);
    return 1;
  }
  return 0;
}

typedef struct { const CallSign *a, *b; Vec3 position; } Collision;

/* ---- voxel map (reduce_collision_set.js) ---- */
typedef struct { const Motion **items; size_t n, cap; } MotionList;

static void motion_list_push(MotionList *l, const Motion *m) {
  if (l->n == l->cap) {
    size_t nc = l->cap ? l->cap * 2 : 4;
    const Motion **ni = realloc(l->items, nc * sizeof *ni);
    if (!ni) fatal("out of memory");
    l->items = ni;
    l->cap = nc;
  }
  l->items[l->n++] = m;
}

static Vec2 voxel_hash(Vec3 position) {
  double voxel_size = GOOD_VOXEL_SIZE;
  double x_div = js_to_int32(position.x / voxel_size);
  double y_div = js_to_int32(position.y / voxel_size);
  Vec2 result;
  result.x = voxel_size * x_div;
  result.y = voxel_size * y_div;
  if (position.x < 0) result.x -= voxel_size;
  if (position.y < 0) result.y -= voxel_size;
  return result;
}

static int is_in_voxel(Vec2 voxel, const Motion *motion) {
  if (voxel.x > MAX_X || voxel.x < MIN_X || voxel.y > MAX_Y || voxel.y < MIN_Y) return 0;
  Vec3 init = motion->pos_one;
  Vec3 fin = motion->pos_two;
  double v_s = GOOD_VOXEL_SIZE;
  double r = PROXIMITY_RADIUS / 2;
  double v_x = voxel.x;
  double x0 = init.x;
  double xv = fin.x - init.x;
  double v_y = voxel.y;
  double y0 = init.y;
  double yv = fin.y - init.y;
  double low_x, high_x, low_y, high_y, tmp;
  low_x = (v_x - r - x0) / xv;
  high_x = (v_x + v_s + r - x0) / xv;
  if (xv < 0) {
    tmp = low_x;
    low_x = high_x;
    high_x = tmp;
  }
  low_y = (v_y - r - y0) / yv;
  high_y = (v_y + v_s + r - y0) / yv;
  if (yv < 0) {
    tmp = low_y;
    low_y = high_y;
    high_y = tmp;
  }
  return (((xv == 0 && v_x <= x0 + r && x0 - r <= v_x + v_s) ||
           ((low_x <= 1 && 1 <= high_x) || (low_x <= 0 && 0 <= high_x) ||
            (0 <= low_x && high_x <= 1))) &&
          ((yv == 0 && v_y <= y0 + r && y0 - r <= v_y + v_s) ||
           ((low_y <= 1 && 1 <= high_y) || (low_y <= 0 && 0 <= high_y) ||
            (0 <= low_y && high_y <= 1))) &&
          (xv == 0 || yv == 0 ||
           (low_y <= high_x && high_x <= high_y) ||
           (low_y <= low_x && low_x <= high_y) ||
           (low_x <= low_y && high_y <= high_x)));
}

typedef struct {
  RBTree *voxel_map;
  RBTree seen;
  const Motion *motion;
} DrawState;

static void put_into_map(DrawState *ds, Vec2 voxel) {
  MotionList *array = rb_get(ds->voxel_map, &voxel);
  if (!array) {
    array = calloc(1, sizeof *array);
    if (!array) fatal("out of memory");
    rb_put(ds->voxel_map, &voxel, array);
  }
  motion_list_push(array, ds->motion);
}

static void recurse(DrawState *ds, Vec2 next) {
  Vec2 horizontal = {GOOD_VOXEL_SIZE, 0};
  Vec2 vertical = {0, GOOD_VOXEL_SIZE};
  if (!is_in_voxel(next, ds->motion)) return;
  if (rb_put(&ds->seen, &next, (void *)1)) return;
  put_into_map(ds, next);
  recurse(ds, vec2_minus(next, horizontal));
  recurse(ds, vec2_plus(next, horizontal));
  recurse(ds, vec2_minus(next, vertical));
  recurse(ds, vec2_plus(next, vertical));
  recurse(ds, vec2_minus(vec2_minus(next, horizontal), vertical));
  recurse(ds, vec2_plus(vec2_minus(next, horizontal), vertical));
  recurse(ds, vec2_minus(vec2_plus(next, horizontal), vertical));
  recurse(ds, vec2_plus(vec2_plus(next, horizontal), vertical));
}

static void draw_motion_on_voxel_map(RBTree *voxel_map, const Motion *motion) {
  DrawState ds;
  ds.voxel_map = voxel_map;
  ds.motion = motion;
  rb_init(&ds.seen, sizeof(Vec2), vec2_compare);
  recurse(&ds, voxel_hash(motion->pos_one));
  rb_clear(&ds.seen);
}

typedef struct { MotionList **items; size_t n, cap; } ReducedList;

static void collect_reduced(const void *key, void *value, void *ctx) {
  (void)key;
  ReducedList *out = ctx;
  MotionList *list = value;
  if (list->n > 1) {
    if (out->n == out->cap) {
      size_t nc = out->cap ? out->cap * 2 : 16;
      MotionList **ni = realloc(out->items, nc * sizeof *ni);
      if (!ni) fatal("out of memory");
      out->items = ni;
      out->cap = nc;
    }
    out->items[out->n++] = list;
  }
}

static void free_voxel_value(const void *key, void *value, void *ctx) {
  (void)key;
  (void)ctx;
  MotionList *list = value;
  free(list->items);
  free(list);
}

/* reduceCollisionSet(motions): the voxel map, and the lists with more than one motion
 * in voxel order. The caller frees both with reduced_free. */
typedef struct { RBTree voxel_map; ReducedList list; } Reduced;

static Reduced reduce_collision_set(const Motion *motions, size_t n) {
  Reduced r;
  rb_init(&r.voxel_map, sizeof(Vec2), vec2_compare);
  r.list.items = NULL;
  r.list.n = r.list.cap = 0;
  for (size_t i = 0; i < n; i++) draw_motion_on_voxel_map(&r.voxel_map, &motions[i]);
  rb_for_each(&r.voxel_map, collect_reduced, &r.list);
  return r;
}

static void reduced_free(Reduced *r) {
  rb_for_each(&r->voxel_map, free_voxel_value, NULL);
  rb_clear(&r->voxel_map);
  free(r->list.items);
}

/* ---- Simulator (simulator.js) ---- */
typedef struct { const CallSign *callsign; Vec3 position; } FrameEntry;

static FrameEntry *simulate(const CallSign *aircraft, size_t n, double time) {
  FrameEntry *frame = malloc(n * sizeof *frame);
  if (!frame) fatal("out of memory");
  for (size_t i = 0; i < n; i += 2) {
    Vec3 p1 = {time, cos(time) * 2 + (double)i * 3, 10};
    Vec3 p2 = {time, sin(time) * 2 + (double)i * 3, 10};
    frame[i].callsign = &aircraft[i];
    frame[i].position = p1;
    frame[i + 1].callsign = &aircraft[i + 1];
    frame[i + 1].position = p2;
  }
  return frame;
}


/* ---- CollisionDetector (collision_detector.js) ---- */
typedef struct {
  RBTree state;            /* call sign -> pointer to that aircraft's last position */
  FrameEntry *prev_frame;  /* owns the positions the state tree points into */
} Detector;

static void detector_init(Detector *d) {
  rb_init(&d->state, sizeof(const CallSign *), callsign_compare);
  d->prev_frame = NULL;
}

static void detector_free(Detector *d) {
  rb_clear(&d->state);
  free(d->prev_frame);
  d->prev_frame = NULL;
}

/* handleNewFrame(frame): takes ownership of frame, returns the collisions (*count of them). */
static Collision *handle_new_frame(Detector *d, FrameEntry *frame, size_t n, size_t *count) {
  Motion *motions = malloc(n * sizeof *motions);
  if (!motions) fatal("out of memory");
  RBTree seen;
  rb_init(&seen, sizeof(const CallSign *), callsign_compare);
  for (size_t i = 0; i < n; i++) {
    const CallSign *key = frame[i].callsign;
    Vec3 *old_position = rb_put(&d->state, &key, &frame[i].position);
    Vec3 new_position = frame[i].position;
    rb_put(&seen, &key, (void *)1);
    Vec3 old = old_position ? *old_position : new_position;
    motions[i].callsign = key;
    motions[i].pos_one = old;
    motions[i].pos_two = new_position;
  }

  /* toRemove: call signs in the state tree but not in this frame. Never happens here. */
  for (RBNode *cur = d->state.root ? tree_minimum(d->state.root) : NULL; cur; cur = successor(cur)) {
    if (!rb_get(&seen, cur->key)) fatal("unsupported: call sign left the simulation");
  }
  rb_clear(&seen);

  Reduced reduced = reduce_collision_set(motions, n);
  size_t cap = 64, nc = 0;
  Collision *collisions = malloc(cap * sizeof *collisions);
  if (!collisions) fatal("out of memory");
  for (size_t r = 0; r < reduced.list.n; r++) {
    const MotionList *list = reduced.list.items[r];
    for (size_t i = 0; i < list->n; i++) {
      for (size_t j = i + 1; j < list->n; j++) {
        const Motion *m1 = list->items[i], *m2 = list->items[j];
        Vec3 point;
        if (!find_intersection(m1, m2, &point)) continue;
        if (nc == cap) {
          cap *= 2;
          Collision *grown = realloc(collisions, cap * sizeof *grown);
          if (!grown) fatal("out of memory");
          collisions = grown;
        }
        Collision c = {m1->callsign, m2->callsign, point};
        collisions[nc++] = c;
      }
    }
  }
  reduced_free(&reduced);
  free(motions);
  free(d->prev_frame);
  d->prev_frame = frame;
  *count = nc;
  return collisions;
}

/* ---- Benchmark (cdjs/benchmark.js) ---- */
static void benchmark(void) {
  CallSign *aircraft = malloc(NUM_AIRCRAFT * sizeof *aircraft);   /* new Simulator */
  if (!aircraft) fatal("out of memory");
  for (int i = 0; i < NUM_AIRCRAFT; i++) {
    aircraft[i].name = malloc(16);
    if (!aircraft[i].name) fatal("out of memory");
    snprintf(aircraft[i].name, 16, "foo%d", i);
  }
  Detector detector;                                                /* new CollisionDetector */
  detector_init(&detector);
  size_t total = 0;
  for (int i = 0; i < NUM_FRAMES; i++) {
    double time = i / 10.0;
    FrameEntry *frame = simulate(aircraft, NUM_AIRCRAFT, time);
    size_t count = 0;
    Collision *collisions = handle_new_frame(&detector, frame, NUM_AIRCRAFT, &count);
    free(collisions);
    total += count;
  }
  detector_free(&detector);
  for (int i = 0; i < NUM_AIRCRAFT; i++) free(aircraft[i].name);
  free(aircraft);
  if (total != EXPECTED_COLLISIONS) {
    fprintf(stderr, "Error: Bad number of collisions: %zu (expected %d)\n", total, EXPECTED_COLLISIONS);
    exit(1);
  }
}

static double now_ms(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return t.tv_sec * 1e3 + t.tv_nsec / 1e6;
}

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : DEFAULT_ITERATIONS;
  double start = now_ms();
  for (int it = 0; it < iterations; it++) benchmark();
  double elapsed = now_ms() - start;
  printf("cdjs %d iterations %.3f ms\n", iterations, elapsed);
  printf("validated: %d collisions per iteration\n", EXPECTED_COLLISIONS);
  return 0;
}
