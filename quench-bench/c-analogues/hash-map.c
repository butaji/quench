/* C analogue of JetStream's simple/hash-map.js: the same chained hash map (a port of
 * Java's HashMap), one heap entry per key, power-of-two tables that start at length 1
 * (the JS constructor reads the undefined `this.capacity`) and double on rehash, so a
 * JS engine is compared with what a C programmer writes for the same program. */
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

typedef struct Entry { int key, hash, value; struct Entry *next; } Entry;
typedef struct { Entry **data; int length, count, threshold; double load_factor; } HashMap;

static int calculate_capacity(int x) {
  if (x >= 1 << 30) return 1 << 30;
  if (x == 0) return 16;
  x = x - 1;
  x |= x >> 1; x |= x >> 2; x |= x >> 4; x |= x >> 8; x |= x >> 16;
  return x + 1;
}

static void compute_threshold(HashMap *m) { m->threshold = (int)(m->length * m->load_factor); }

static void rehash(HashMap *m) {
  int length = calculate_capacity(m->length ? m->length << 1 : 1);
  Entry **data = calloc(length, sizeof *data);
  for (int i = 0; i < m->length; i++) {
    for (Entry *e = m->data[i], *next; e; e = next) {
      next = e->next;
      int index = e->hash & (length - 1);
      e->next = data[index];
      data[index] = e;
    }
  }
  free(m->data);
  m->data = data;
  m->length = length;
  compute_threshold(m);
}

static Entry *find(const HashMap *m, int key, int index, int hash) {
  Entry *e = m->data[index];
  while (e && (e->hash != hash || e->key != key)) e = e->next;
  return e;
}

static void put(HashMap *m, int key, int value) {
  int hash = key, index = hash & (m->length - 1);
  Entry *e = find(m, key, index, hash);
  if (!e) {
    e = malloc(sizeof *e);
    *e = (Entry){key, hash, 0, m->data[index]};
    m->data[index] = e;
    if (++m->count > m->threshold) rehash(m);
  }
  e->value = value;
}

static int get(const HashMap *m, int key) {
  Entry *e = find(m, key, key & (m->length - 1), key);
  return e ? e->value : 0;
}

static void run(void) {
  enum { COUNT = 90000 };
  HashMap m = {calloc(1, sizeof(Entry *)), 1, 0, 0, 0.75};
  compute_threshold(&m);
  for (int i = 0; i < COUNT; i++) put(&m, i, 42);
  double result = 0;
  for (int j = 0; j < 5; j++)
    for (int i = 0; i < COUNT; i++) result += get(&m, i);
  double key_sum = 0, value_sum = 0;
  for (int i = 0; i < m.length; i++)
    for (Entry *e = m.data[i]; e; e = e->next) { key_sum += e->key; value_sum += e->value; }
  if (result != 42.0 * COUNT * 5 || key_sum != (COUNT * (COUNT + 1.0) / 2) - COUNT || value_sum != 42.0 * COUNT) {
    fprintf(stderr, "Error: result = %f keySum = %f valueSum = %f\n", result, key_sum, value_sum);
    exit(1);
  }
  /* JS garbage is collected; C frees the same objects. */
  for (int i = 0; i < m.length; i++)
    for (Entry *e = m.data[i], *next; e; e = next) { next = e->next; free(e); }
  free(m.data);
}

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : 120;
  struct timespec a, b;
  clock_gettime(CLOCK_MONOTONIC, &a);
  for (int i = 0; i < iterations; i++) run();
  clock_gettime(CLOCK_MONOTONIC, &b);
  printf("hash-map %d iterations %.3f ms\n", iterations, (b.tv_sec - a.tv_sec) * 1e3 + (b.tv_nsec - a.tv_nsec) / 1e6);
  return 0;
}
