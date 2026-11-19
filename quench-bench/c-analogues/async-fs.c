// jetstream-row: async-fs
// jetstream-files: generators/async-file-system.js
// iterations: 80
// jetstream-driver: class=Async deterministic=true
/*
 * C analogue of JetStream's async-fs (generators/async-file-system.js). The tree, the
 * file bytes, the swap, the deletion pattern and the visits are the same work as the JS
 * benchmark, on a tree of up to 2501 directories (breadth first, 15 children each, names
 * dir-i) and 800 files, file k in directory (107 * k) mod 2501. As in the JS, the tree is
 * built by prepareForNextIteration, outside the timed region, and only runIteration is
 * timed. The iteration deletes every file in every 13th non-empty directory in post-order,
 * visits the tree twice and checks the counts. The validation is the same: 739 files
 * remain per iteration.
 *
 * The JS async generators and awaits become direct calls; their per-element suspension
 * cost is not part of the C program. In the JS, bytesDeleted is NaN, because
 * totalFileSize() is async and is subtracted without await; the C program does the same
 * second size computation (its value is discarded) and skips the "did not delete" check
 * that the NaN makes unreachable.
 *
 * Deliberate differences: as in sync-fs.c, entries are positional and integer-named, not
 * Map entries, and the tree is freed after each iteration; the byte order swap reads
 * little-endian 8-byte words and copies bytes exactly, and V8 may canonicalize NaN
 * payloads, which can change the bytes of such words after several swaps. */
#define _POSIX_C_SOURCE 199309L
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

enum {
  MAX_DIR_COUNT = 2500,       /* directories are added while the count is <= 2500 */
  MAX_FILE_COUNT = 800,
  EXPECTED_FILE_COUNT = 739,
  DEFAULT_ITERATIONS = 80
};

static void fatal(const char *msg) {
  fprintf(stderr, "Error: %s\n", msg);
  exit(1);
}

/* ToInt32 for doubles: NaN and infinities give 0, otherwise truncate and wrap mod 2^32. */
static int32_t js_to_int32(double d) {
  if (!isfinite(d)) return 0;
  d = trunc(d);
  d = fmod(d, 4294967296.0);
  if (d < 0) d += 4294967296.0;
  return (int32_t)(uint32_t)d;
}

typedef struct { uint8_t *data; size_t len; } File;
typedef struct Dir Dir;
typedef struct { int name; int is_dir; Dir *dir; File *file; } Entry;
struct Dir { Entry *entries; size_t count, cap; };

static File *file_new(size_t len) {
  File *f = malloc(sizeof *f);
  if (!f) fatal("out of memory");
  f->data = malloc(len ? len : 1);
  if (!f->data) fatal("out of memory");
  f->len = len;
  return f;
}

static void file_free(File *f) {
  free(f->data);
  free(f);
}

static Dir *dir_new(void) {
  Dir *d = calloc(1, sizeof *d);
  if (!d) fatal("out of memory");
  return d;
}

static void dir_push(Dir *d, Entry e) {
  if (d->count == d->cap) {
    size_t nc = d->cap ? d->cap * 2 : 4;
    Entry *ne = realloc(d->entries, nc * sizeof *ne);
    if (!ne) fatal("out of memory");
    d->entries = ne;
    d->cap = nc;
  }
  d->entries[d->count++] = e;
}

static void dir_free(Dir *d) {
  for (size_t i = 0; i < d->count; i++) {
    if (d->entries[i].is_dir) dir_free(d->entries[i].dir);
    else file_free(d->entries[i].file);
  }
  free(d->entries);
  free(d);
}

/* Directory.rm for a file entry at position i: the entries after it keep their order. */
static void dir_remove_file_at(Dir *d, size_t i) {
  Entry e = d->entries[i];
  if (e.is_dir) fatal("rm on a directory");
  memmove(&d->entries[i], &d->entries[i + 1], (d->count - i - 1) * sizeof *d->entries);
  d->count--;
  file_free(e.file);
}

static size_t file_count(const Dir *d) {
  size_t n = 0;
  for (size_t i = 0; i < d->count; i++)
    if (!d->entries[i].is_dir) n++;
  return n;
}

static size_t total_file_size(const Dir *d) {
  size_t s = 0;
  for (size_t i = 0; i < d->count; i++) {
    const Entry *e = &d->entries[i];
    s += e->is_dir ? total_file_size(e->dir) : e->file->len;
  }
  return s;
}

/* File.swapByteOrder: each 8-byte word is read as little-endian and written back
 * reversed. The result is the xor of ToInt32 of the words, starting from 0x1a2b3c4d. */
static int32_t swap_byte_order(File *f) {
  int32_t hash = 0x1a2b3c4d;
  size_t words = f->len / 8 * 8;
  for (size_t i = 0; i < words; i += 8) {
    uint8_t *p = f->data + i;
    uint64_t bits = 0;
    for (int k = 7; k >= 0; k--) bits = (bits << 8) | p[k];
    double data;
    memcpy(&data, &bits, sizeof data);
    hash ^= js_to_int32(data);
    for (int k = 0; k < 4; k++) {
      uint8_t t = p[k];
      p[k] = p[7 - k];
      p[7 - k] = t;
    }
  }
  return hash;
}

/* forEachFileRecursively: every file in tree order. */
typedef void (*FileFn)(File *f, void *ctx);
static void for_each_file(Dir *d, FileFn fn, void *ctx) {
  for (size_t i = 0; i < d->count; i++) {
    Entry *e = &d->entries[i];
    if (e->is_dir) for_each_file(e->dir, fn, ctx);
    else fn(e->file, ctx);
  }
}

/* forEachDirectoryRecursively: each subdirectory after its own subdirectories (post-order). */
typedef void (*DirFn)(Dir *d, void *ctx);
static void for_each_dir(Dir *d, DirFn fn, void *ctx) {
  for (size_t i = 0; i < d->count; i++) {
    Entry *e = &d->entries[i];
    if (!e->is_dir) continue;
    for_each_dir(e->dir, fn, ctx);
    fn(e->dir, ctx);
  }
}

/* The file byte generator (randomFileContents): file number c (1-based) has numBytes
 * computed from c and bytes (i + c + 1) % 256. */
static File *make_file(size_t counter) {
  size_t num_bytes = (size_t)((int32_t)((double)counter * 1192.18851371) % 2056);
  File *f = file_new(num_bytes);
  for (size_t i = 0; i < num_bytes; i++) f->data[i] = (uint8_t)((i + counter + 1) % 256);
  return f;
}

/* setupDirectory (prepareForNextIteration): not timed. */
static Dir *setup_directory(void) {
  Dir *fs = dir_new();
  Dir **dirs = malloc((MAX_DIR_COUNT + 1) * sizeof *dirs);
  if (!dirs) fatal("out of memory");
  size_t ndirs = 0;
  dirs[ndirs++] = fs;
  for (size_t di = 0; di < ndirs; di++) {
    for (int i = 0; i < 15; i++) {
      if (ndirs <= MAX_DIR_COUNT) {
        Dir *sub = dir_new();
        Entry e = {i, 1, sub, NULL};
        dir_push(dirs[di], e);
        dirs[ndirs++] = sub;
      }
    }
  }
  for (size_t k = 0; k < MAX_FILE_COUNT; k++) {
    Dir *d = dirs[(k * 107) % ndirs];
    Entry e = {(int)k, 0, NULL, make_file(k + 1)};
    dir_push(d, e);
  }
  free(dirs);
  return fs;
}

typedef struct { size_t files, dirs; } CountVisitor;
typedef struct { size_t bytes; } DraculaVisitor;

static void visit_count(const Dir *d, CountVisitor *v) {
  v->dirs++;
  for (size_t i = 0; i < d->count; i++) {
    const Entry *e = &d->entries[i];
    if (e->is_dir) visit_count(e->dir, v);
    else v->files++;
  }
}

static void visit_bytes(const Dir *d, DraculaVisitor *v) {
  for (size_t i = 0; i < d->count; i++) {
    const Entry *e = &d->entries[i];
    if (e->is_dir) visit_bytes(e->dir, v);
    else v->bytes += e->file->len;
  }
}

typedef struct { size_t counter; } DeleteState;

static void delete_in_dir(Dir *d, void *ctx) {
  DeleteState *st = ctx;
  volatile size_t old_total = total_file_size(d);
  (void)old_total;
  if (file_count(d) == 0) return;
  st->counter++;
  if (st->counter % 13 != 0) return;
  for (size_t i = 0; i < d->count;) {
    if (d->entries[i].is_dir) i++;
    else dir_remove_file_at(d, i);
  }
  /* oldTotalSize - dir.totalFileSize() is NaN in the JS (the second call is async and
   * not awaited), so the JS total stays NaN; the second size computation is still done. */
  volatile size_t new_total = total_file_size(d);
  (void)new_total;
}

typedef struct { int32_t last_hash; } SwapState;
static void swap_one(File *f, void *ctx) {
  SwapState *st = ctx;
  st->last_hash = swap_byte_order(f);
}

typedef struct { size_t file_count, file_bytes; } FileTotals;
static void add_file(File *f, void *ctx) {
  FileTotals *t = ctx;
  t->file_count++;
  t->file_bytes += f->len;
}

static size_t dir_total;
static void add_dir(Dir *d, void *ctx) {
  (void)d;
  (void)ctx;
  dir_total++;
}

static long total_file_count;
static int32_t last_file_hash;
static int have_hash;

/* Benchmark.runIteration on the tree built by setup_directory. */
static void run_iteration(Dir *fs) {
  SwapState swap = {0};
  for_each_file(fs, swap_one, &swap);
  last_file_hash = swap.last_hash;
  have_hash = 1;

  DeleteState del = {0};
  for_each_dir(fs, delete_in_dir, &del);

  CountVisitor cv = {0, 0};
  visit_count(fs, &cv);
  DraculaVisitor dv = {0};
  visit_bytes(fs, &dv);

  FileTotals totals = {0, 0};
  for_each_file(fs, add_file, &totals);
  total_file_count += (long)totals.file_count;

  dir_total = 1;
  for_each_dir(fs, add_dir, NULL);

  if (cv.files != totals.file_count)
    fatal("Invalid total file count");
  if (dv.bytes != totals.file_bytes)
    fatal("Invalid total file bytes");
  if (cv.dirs != dir_total)
    fatal("Invalid total dir count");
}

static double now_ms(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return t.tv_sec * 1e3 + t.tv_nsec / 1e6;
}

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : DEFAULT_ITERATIONS;
  double elapsed = 0;
  for (int it = 0; it < iterations; it++) {
    Dir *fs = setup_directory();          /* untimed, as prepareForNextIteration */
    double start = now_ms();
    run_iteration(fs);
    elapsed += now_ms() - start;
    dir_free(fs);
  }
  if (total_file_count != (long)EXPECTED_FILE_COUNT * iterations || !have_hash)
    fatal("Invalid total file count");
  printf("async-fs %d iterations %.3f ms\n", iterations, elapsed);
  printf("validated: %ld files remaining over %d iterations, last hash %d\n",
         total_file_count, iterations, (int)last_file_hash);
  return 0;
}
