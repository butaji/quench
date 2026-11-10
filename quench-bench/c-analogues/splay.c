/* C analogue of JetStream's Octane/splay.js: the same top-down splay tree of heap nodes
 * keyed by doubles from JetStream's deterministic Math.random (reset before every
 * iteration, so most fresh keys collide with live ones and are redrawn), the same
 * depth-5 payload trees with a ten-int array and a per-leaf key string, freed when their
 * node is removed, and the same pause samples, built once at setup outside the timed
 * loop. The JS validates nothing per iteration; after the loop the tree is checked as
 * SplayTearDown does (8000 keys, strictly sorted) and against what node computes for
 * the random draws made and the final key sum. Deliberate differences: the splay dummy
 * node lives on the stack where the JS allocates one per splay, and String(key) is
 * printed with %.17g rather than JS's shortest round-trip digits. */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

enum { TREE_SIZE = 8000, MODIFICATIONS = 80, PAYLOAD_DEPTH = 5, RUNS_PER_ITERATION = 50, LEAF_ARRAY = 10 };
enum { SAMPLE_PERIOD = 20, SAMPLE_PHASE = 19, KEY_DIGITS = 32 };
static const uint32_t INITIAL_SEED = 49734321;
static const double TWO_POW_32 = 4294967296.0;
enum { DEFAULT_ITERATIONS = 120 };
/* What node computes after the default run; the tree evolves across iterations. */
static const long long EXPECTED_RANDOM_CALLS = 1202728;
static const double EXPECTED_KEY_SUM = 4050.348380475305;

/* JetStream's deterministic Math.random: Robert Jenkins' 32-bit integer hash. */
static uint32_t seed;
static long long random_calls;
static double math_random(void) {
  random_calls++;
  seed = (seed + 0x7ed55d16) + (seed << 12);
  seed = (seed ^ 0xc761c23c) ^ (seed >> 19);
  seed = (seed + 0x165667b1) + (seed << 5);
  seed = (seed + 0xd3a2646c) ^ (seed << 9);
  seed = (seed + 0xfd7046c5) + (seed << 3);
  seed = (seed ^ 0xb55a4f09) ^ (seed >> 16);
  return seed / TWO_POW_32;
}

typedef struct { int array[LEAF_ARRAY]; char *string; } Leaf;
typedef struct { void *left, *right; } Interior; /* children are Interior above depth 1, Leaf at it */

static void *generate_payload_tree(int depth, const char *tag) {
  if (depth == 0) {
    Leaf *leaf = malloc(sizeof *leaf);
    for (int i = 0; i < LEAF_ARRAY; i++) leaf->array[i] = i;
    int length = snprintf(NULL, 0, "String for key %s in leaf node", tag);
    leaf->string = malloc(length + 1);
    snprintf(leaf->string, length + 1, "String for key %s in leaf node", tag);
    return leaf;
  }
  Interior *node = malloc(sizeof *node);
  node->left = generate_payload_tree(depth - 1, tag);
  node->right = generate_payload_tree(depth - 1, tag);
  return node;
}

static void free_payload_tree(void *payload, int depth) {
  if (depth == 0) {
    free(((Leaf *)payload)->string);
  } else {
    free_payload_tree(((Interior *)payload)->left, depth - 1);
    free_payload_tree(((Interior *)payload)->right, depth - 1);
  }
  free(payload);
}

typedef struct Node { double key; void *value; struct Node *left, *right; } Node;
static Node *root;

static Node *node_new(double key, void *value) {
  Node *n = malloc(sizeof *n);
  *n = (Node){key, value, NULL, NULL};
  return n;
}

/* Sleator and Tarjan's simplified top-down splay. */
static void splay(double key) {
  if (!root) return;
  Node dummy = {0}, *left = &dummy, *right = &dummy, *current = root;
  for (;;) {
    if (key < current->key) {
      if (!current->left) break;
      if (key < current->left->key) {
        Node *tmp = current->left;
        current->left = tmp->right; tmp->right = current; current = tmp;
        if (!current->left) break;
      }
      right->left = current; right = current; current = current->left;
    } else if (key > current->key) {
      if (!current->right) break;
      if (key > current->right->key) {
        Node *tmp = current->right;
        current->right = tmp->left; tmp->left = current; current = tmp;
        if (!current->right) break;
      }
      left->right = current; left = current; current = current->right;
    } else break;
  }
  left->right = current->left;
  right->left = current->right;
  current->left = dummy.right;
  current->right = dummy.left;
  root = current;
}

static void insert(double key, void *value) {
  if (!root) { root = node_new(key, value); return; }
  splay(key);
  if (root->key == key) return;
  Node *node = node_new(key, value);
  if (key > root->key) { node->left = root; node->right = root->right; root->right = NULL; }
  else { node->right = root; node->left = root->left; root->left = NULL; }
  root = node;
}

static Node *remove_key(double key) {
  if (root) splay(key);
  if (!root || root->key != key) { fprintf(stderr, "Error: Key not found: %.17g\n", key); exit(1); }
  Node *removed = root;
  if (!root->left) root = root->right;
  else { Node *right = root->right; root = root->left; splay(key); root->right = right; }
  return removed;
}

static Node *find(double key) {
  if (!root) return NULL;
  splay(key);
  return root->key == key ? root : NULL;
}

static Node *find_max(Node *start) {
  Node *current = start ? start : root;
  while (current->right) current = current->right;
  return current;
}

static Node *find_greatest_less_than(double key) {
  if (!root) return NULL;
  splay(key);
  if (root->key < key) return root;
  return root->left ? find_max(root->left) : NULL;
}

static double *samples;
static int sample_count, sample_capacity;
static double sample_time_start;
static double performance_now(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return t.tv_sec * 1e3 + t.tv_nsec / 1e6;
}
static void update_stats(double time) {
  if (sample_count == sample_capacity) {
    sample_capacity = sample_capacity ? sample_capacity * 2 : 16;
    samples = realloc(samples, sample_capacity * sizeof *samples);
  }
  samples[sample_count++] = time - sample_time_start;
  sample_time_start = time;
}

static double insert_new_node(void) {
  double key;
  do key = math_random(); while (find(key));
  char tag[KEY_DIGITS];
  snprintf(tag, sizeof tag, "%.17g", key);
  insert(key, generate_payload_tree(PAYLOAD_DEPTH, tag));
  return key;
}

static void free_node(Node *n) { free_payload_tree(n->value, PAYLOAD_DEPTH); free(n); }

static void splay_setup(void) {
  sample_time_start = performance_now();
  for (int i = 0; i < TREE_SIZE; i++) {
    insert_new_node();
    if ((i + 1) % SAMPLE_PERIOD == SAMPLE_PHASE) update_stats(performance_now());
  }
}

static void splay_run(void) {
  for (int i = 0; i < MODIFICATIONS; i++) {
    double key = insert_new_node();
    Node *greatest = find_greatest_less_than(key);
    free_node(remove_key(greatest ? greatest->key : key));
  }
  update_stats(performance_now());
}

/* SplayTree.Node.traverse_ visiting keys in order, as exportKeys does. */
static void export_keys(Node *n, double *keys, int *count) {
  for (; n; n = n->right) {
    if (n->left) export_keys(n->left, keys, count);
    keys[(*count)++] = n->key;
  }
}

static void tear_down(int iterations) {
  static double keys[TREE_SIZE + 1];
  int length = 0;
  export_keys(root, keys, &length);
  double sum = 0;
  for (int i = 0; i < length; i++) sum += keys[i];
  if (length != TREE_SIZE) { fprintf(stderr, "Error: Splay tree has wrong size %d\n", length); exit(1); }
  for (int i = 0; i < length - 1; i++)
    if (keys[i] >= keys[i + 1]) { fprintf(stderr, "Error: Splay tree not sorted\n"); exit(1); }
  if (sample_count != TREE_SIZE / SAMPLE_PERIOD + iterations * RUNS_PER_ITERATION ||
      (iterations == DEFAULT_ITERATIONS && (random_calls != EXPECTED_RANDOM_CALLS || sum != EXPECTED_KEY_SUM))) {
    fprintf(stderr, "Error: samples = %d random calls = %lld key sum = %.17g\n", sample_count, random_calls, sum);
    exit(1);
  }
}

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : DEFAULT_ITERATIONS;
  seed = INITIAL_SEED;
  splay_setup();
  struct timespec a, b;
  clock_gettime(CLOCK_MONOTONIC, &a);
  for (int i = 0; i < iterations; i++) {
    seed = INITIAL_SEED;
    for (int r = 0; r < RUNS_PER_ITERATION; r++) splay_run();
  }
  clock_gettime(CLOCK_MONOTONIC, &b);
  tear_down(iterations);
  printf("splay %d iterations %.3f ms\n", iterations, (b.tv_sec - a.tv_sec) * 1e3 + (b.tv_nsec - a.tv_nsec) / 1e6);
  return 0;
}
