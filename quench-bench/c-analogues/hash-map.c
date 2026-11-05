/* C analogue of JetStream's simple/hash-map.js: the same chained hash map of heap entries,
 * its power-of-two rehashing (which starts from the one-slot table the source's
 * `new Array(this.capacity)` makes, `capacity` being undefined there) and the same
 * put/get/iterate driver and checks. Integer keys hash to themselves, as `computeHashCode`
 * gives an int32 Number. */
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

typedef struct Entry { int key, value, hash; struct Entry *next; } Entry;
typedef struct { Entry **data; int length, count, threshold, mod_count; double load_factor; } HashMap;

static int calculate_capacity(int x) {
  if (x >= 1 << 30) return 1 << 30;
  if (x == 0) return 16;
  x = x - 1; x |= x >> 1; x |= x >> 2; x |= x >> 4; x |= x >> 8; x |= x >> 16;
  return x + 1;
}
static void compute_threshold(HashMap *map) { map->threshold = (int)(map->length * map->load_factor); }
static void map_init(HashMap *map) {
  map->length = 1; map->data = calloc(1, sizeof(Entry *));
  map->count = 0; map->mod_count = 0; map->load_factor = 0.75;
  compute_threshold(map);
}
static Entry *find_key_entry(HashMap *map, int key, int index, int hash) {
  Entry *entry = map->data[index];
  while (entry && (entry->hash != hash || entry->key != key)) entry = entry->next;
  return entry;
}
static void rehash(HashMap *map) {
  int capacity = map->length;
  int length = calculate_capacity(!capacity ? 1 : capacity << 1);
  Entry **new_data = calloc(length, sizeof(Entry *));
  for (int i = 0; i < map->length; ++i) {
    Entry *entry = map->data[i];
    map->data[i] = NULL;
    while (entry) {
      int index = entry->hash & (length - 1);
      Entry *next = entry->next;
      entry->next = new_data[index];
      new_data[index] = entry;
      entry = next;
    }
  }
  free(map->data);
  map->data = new_data; map->length = length;
  compute_threshold(map);
}
static void put(HashMap *map, int key, int value) {
  int hash = key, index = hash & (map->length - 1);
  Entry *entry = find_key_entry(map, key, index, hash);
  if (!entry) {
    map->mod_count++;
    entry = malloc(sizeof *entry);
    *entry = (Entry){key, 0, hash, map->data[index]};
    map->data[index] = entry;
    if (++map->count > map->threshold) rehash(map);
  }
  entry->value = value;
}
static int get(HashMap *map, int key, int *found) {
  int hash = key;
  Entry *entry = find_key_entry(map, key, hash & (map->length - 1), hash);
  *found = entry != NULL;
  return entry ? entry->value : 0;
}
static void map_free(HashMap *map) {
  for (int i = 0; i < map->length; i++) for (Entry *e = map->data[i], *n; e; e = n) { n = e->next; free(e); }
  free(map->data);
}
static void run(void) {
  enum { COUNT = 90000 };
  HashMap map; map_init(&map);
  for (int i = 0; i < COUNT; ++i) put(&map, i, 42);
  double result = 0;
  for (int j = 0; j < 5; ++j)
    for (int i = 0; i < COUNT; ++i) { int found; int v = get(&map, i, &found); result += found ? v : 0; }
  /* The entry iterator: positions over the table and follows each chain. */
  double key_sum = 0, value_sum = 0;
  for (int position = 0; position < map.length; position++)
    for (Entry *entry = map.data[position]; entry; entry = entry->next) { key_sum += entry->key; value_sum += entry->value; }
  if (result != 42.0 * COUNT * 5) { fprintf(stderr, "Error: result = %f\n", result); exit(1); }
  if (key_sum != ((double)COUNT * (COUNT + 1) / 2) - COUNT) { fprintf(stderr, "Error: keySum = %f\n", key_sum); exit(1); }
  if (value_sum != 42.0 * COUNT) { fprintf(stderr, "Error: valueSum = %f\n", value_sum); exit(1); }
  map_free(&map);
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
