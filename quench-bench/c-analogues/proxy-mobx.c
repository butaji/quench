// jetstream-row: proxy-mobx
// jetstream-files: proxy/common.js proxy/mobx-bundle.js proxy/mobx-benchmark.js
// iterations: 360
// jetstream-driver: class=Async namespace=mobx@proxy/mobx-bundle.js
/*
 * C analogue of the JetStream 3 proxy-mobx row: proxy/mobx-benchmark.js run
 * on top of the reminder model and test script in proxy/common.js.
 *
 * The JS uses MobX (makeAutoObservable, an autorun that renders the whole
 * store, one reaction per reminder on isCompleted, and runInAction batching
 * so each test step flushes its reactions once, at the end of the batch).
 * This file models the same reactive behaviour in C:
 *
 *   - Observed values are Cells. Each Cell records the derivations that read
 *     it. Getters call cell_track(); setters call cell_changed(). These take
 *     the place of the JS property and Proxy traps.
 *   - Derivations are an autorun (the render) and a reaction (data + effect).
 *     Tracking is dynamic: every run unsubscribes from its previous reads and
 *     subscribes to the reads it makes now.
 *   - Batching: writes inside a batch only queue derivations. The queue is
 *     drained when the outermost batch ends. A write outside a batch drains
 *     it at once.
 *   - Each render writes the full store text into a buffer, which is hashed
 *     with FNV-1a. Validation renders the final store again with tracking off
 *     and requires the same hash, so a missed dependency shows up as a
 *     failure.
 *
 * Validation: List names are Date.toDateString() and Math.random() is the
 * seeded generator of the JetStream composition, reset at the start of each
 * iteration. A build with -DPROXY_TRACE prints each iteration's final render
 * and its autorun and reaction-effect counts. Under a seeded node run of this
 * row, the renders and counts match for all 360 iterations (11 renders and 34
 * effects per iteration).
 *
 * Remaining simplifications: Map and Set are tracked as whole collections, not
 * per key. MobX memoises computed getters while they are observed; this model
 * recomputes them on each read, so it does more getter work than MobX here.
 * Neither changes the run counts above, because render reads every key.
 */

#define _POSIX_C_SOURCE 200809L

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define ITERATIONS 360
#define LIST_COUNT 10
#define REMINDERS_PER_LIST 10
#define MAX_DISTINCT_TAGS 16
#define RNG_SEED UINT32_C(49734321) /* JetStream's deterministic Math.random seed */

static const char *const nouns[] = {"magazine", "passion", "statement", "extent", "judgment", "setting", "piano", "control", "employer", "speech", "presence", "indication", "philosophy", "worker", "supermarket", "atmosphere", "actor", "chest", "garbage", "feedback", "pizza", "speaker", "selection", "effort", "math", "gate", "profession", "recognition", "software", "coffee", "reading", "hotel", "concept", "poem", "population", "property", "office", "phone", "suggestion", "theory", "drawer", "death", "cigarette", "alcohol", "marketing", "session", "cheek", "establishment", "family", "revenue", "location", "mode", "awareness", "preference", "consequence", "cookie", "loss", "poetry", "cabinet", "difference", "story", "penalty", "king", "opinion", "climate", "imagination", "criticism", "meal", "desk", "dealer", "introduction", "physics", "customer", "child", "problem", "medicine", "church", "unit", "tongue", "communication", "storage", "stranger", "presentation", "discussion", "excitement", "role", "ladder", "estate", "town", "proposal", "employee", "administration", "variety", "television", "president", "inspector", "negotiation", "height", "comparison", "decision"};
static const char *const adjectives[] = {"fast", "cheerful", "hungry", "fantastic", "squealing", "critical", "yielding", "abashed", "shocking", "quickest"};

#define NOUN_COUNT (sizeof nouns / sizeof nouns[0])
#define ADJ_COUNT (sizeof adjectives / sizeof adjectives[0])

/* ---- deterministic randomness (replaces Math.random) ---- */

static uint32_t g_rng;

/* Uniform in [0, 1): the same 32-bit mixing as the seeded Math.random of the JetStream
 * composition (compose_row.py SEEDED_RANDOM), so a seeded run draws the same values. */
static double random01(void)
{
    uint32_t s = g_rng;

    s = (s + UINT32_C(0x7ed55d16)) + (s << 12);
    s = (s ^ UINT32_C(0xc761c23c)) ^ (s >> 19);
    s = (s + UINT32_C(0x165667b1)) + (s << 5);
    s = (s + UINT32_C(0xd3a2646c)) ^ (s << 9);
    s = (s + UINT32_C(0xfd7046c5)) + (s << 3);
    s = (s ^ UINT32_C(0xb55a4f09)) ^ (s >> 16);
    g_rng = s;
    return (double)s / 4294967296.0;
}

/* ---- allocation helpers ---- */

static void *xalloc(size_t n)
{
    void *p = calloc(1, n);

    if (!p) {
        fputs("proxy: out of memory\n", stderr);
        exit(2);
    }
    return p;
}

static void *xrealloc(void *p, size_t n)
{
    void *q = realloc(p, n);

    if (!q) {
        fputs("proxy: out of memory\n", stderr);
        exit(2);
    }
    return q;
}

#define PUSH(arr, n, cap, v)                          \
    do {                                              \
        if ((n) == (cap)) {                           \
            (cap) = (cap) ? (cap) * 2 : 8;            \
            (arr) = xrealloc((arr), (cap) * sizeof *(arr)); \
        }                                             \
        (arr)[(n)++] = (v);                           \
    } while (0)

/* ---- observable cells and derivations ---- */

typedef struct Derivation Derivation;

/* A Cell is one observed value. It records the derivations that read it. */
typedef struct Cell {
    Derivation **obs;
    size_t nobs, capobs;
    uint64_t tracked_in;   /* epoch of the last run that recorded this cell */
} Cell;

typedef intptr_t (*DataFn)(void *ctx);
typedef void (*EffectFn)(void *ctx, intptr_t value);

struct Derivation {
    DataFn data;           /* NULL for an autorun */
    EffectFn effect;
    void *ctx;
    Cell **deps;
    size_t ndeps, capdeps;
    intptr_t last;
    int sync;              /* run inline on change instead of queueing */
    int immediate;         /* reaction: call the effect on creation */
    int queued;
    int disposed;
};

static Derivation *g_current;   /* the derivation whose run is tracking reads */
static uint64_t g_epoch;
static Derivation **g_derivs;
static size_t g_nderivs, g_capderivs;

static void cell_track(Cell *c)
{
    Derivation *d = g_current;

    if (!d || c->tracked_in == g_epoch)
        return;
    c->tracked_in = g_epoch;
    PUSH(d->deps, d->ndeps, d->capdeps, c);
    PUSH(c->obs, c->nobs, c->capobs, d);
}

/* Removes d from c's observers, keeping the order of the others. */
static void obs_remove(Cell *c, Derivation *d)
{
    for (size_t i = 0; i < c->nobs; i++) {
        if (c->obs[i] == d) {
            memmove(&c->obs[i], &c->obs[i + 1], (c->nobs - i - 1) * sizeof *c->obs);
            c->nobs--;
            return;
        }
    }
}

static void unsubscribe(Derivation *d)
{
    for (size_t i = 0; i < d->ndeps; i++)
        obs_remove(d->deps[i], d);
    d->ndeps = 0;
}

/* ---- scheduler: MobX batching ----
 * A write inside a batch only queues the derivations that observe it. The
 * queue is drained when the outermost batch ends. A write outside any batch
 * drains it immediately. */

static Derivation **g_queue;
static size_t g_qhead, g_qtail, g_qcap;
static int g_batch_depth;
static int g_flushing;

static void run_derivation(Derivation *d, int first);

static void enqueue(Derivation *d)
{
    if (d->queued || d->disposed)
        return;
    d->queued = 1;
    PUSH(g_queue, g_qtail, g_qcap, d);
}

static Derivation *dequeue(void)
{
    if (g_qhead == g_qtail) {
        g_qhead = g_qtail = 0;
        return NULL;
    }
    return g_queue[g_qhead++];
}

static void flush(void)
{
    Derivation *d;

    if (g_flushing)
        return;
    g_flushing = 1;
    while ((d = dequeue()) != NULL) {
        d->queued = 0;
        if (!d->disposed)
            run_derivation(d, 0);
    }
    g_flushing = 0;
}

static void cell_changed(Cell *c)
{
    for (size_t i = 0; i < c->nobs; i++)
        enqueue(c->obs[i]);
    if (g_batch_depth == 0)
        flush();
}

static void batch_begin(void)
{
    g_batch_depth++;
}

static void batch_end(void)
{
    if (--g_batch_depth == 0)
        flush();
}

/* runInAction(() => stmt) */
#define STEP(stmt) do { batch_begin(); stmt; batch_end(); } while (0)

/* An autorun runs its effect tracked. A reaction runs its data tracked and
 * its effect untracked, and only when the data value has changed. */
static void run_derivation(Derivation *d, int first)
{
    Derivation *prev = g_current;
    intptr_t v;

    unsubscribe(d);
    g_epoch++;
    g_current = d;
    if (!d->data) {
        d->effect(d->ctx, 0);
        g_current = prev;
        return;
    }
    v = d->data(d->ctx);
    g_current = prev;
    if (first) {
        d->last = v;
        return;
    }
    if (v == d->last)
        return;
    d->last = v;
    d->effect(d->ctx, v);
}

static Derivation *derivation_add(DataFn data, EffectFn effect, void *ctx)
{
    Derivation *d = xalloc(sizeof *d);

    d->data = data;
    d->effect = effect;
    d->ctx = ctx;
    PUSH(g_derivs, g_nderivs, g_capderivs, d);
    return d;
}

/* autorun(fn) */
static Derivation *make_autorun(EffectFn fn, void *ctx)
{
    Derivation *d = derivation_add(NULL, fn, ctx);

    run_derivation(d, 1);
    return d;
}

/* reaction(data, effect): the effect runs only when the data changes. */
static Derivation *make_reaction(DataFn data, EffectFn effect, void *ctx)
{
    Derivation *d = derivation_add(data, effect, ctx);

    run_derivation(d, 1);
    return d;
}

/* ---- observable collections (stand-ins for observable arrays and maps) ---- */

typedef struct Arr {
    void **items;
    size_t len, cap;
    Cell cell;
} Arr;

static void arr_read(Arr *a)
{
    cell_track(&a->cell);
}

static void arr_grow(Arr *a)
{
    if (a->len == a->cap) {
        a->cap = a->cap ? a->cap * 2 : 8;
        a->items = xrealloc(a->items, a->cap * sizeof *a->items);
    }
}

static void arr_push(Arr *a, void *x)
{
    arr_grow(a);
    a->items[a->len++] = x;
    cell_changed(&a->cell);
}

static void arr_unshift(Arr *a, void *x)
{
    arr_grow(a);
    memmove(&a->items[1], &a->items[0], a->len * sizeof *a->items);
    a->items[0] = x;
    a->len++;
    cell_changed(&a->cell);
}

static void arr_splice_at(Arr *a, size_t i)
{
    a->len--;
    memmove(&a->items[i], &a->items[i + 1], (a->len - i) * sizeof *a->items);
    cell_changed(&a->cell);
}

static void *arr_shift(Arr *a)
{
    void *x;

    if (a->len == 0)
        return NULL;
    x = a->items[0];
    arr_splice_at(a, 0);
    return x;
}

static void arr_pop(Arr *a)
{
    if (a->len)
        arr_splice_at(a, a->len - 1);
}

/* Index of x in a; a->len if it is absent (callers only pass present items). */
static size_t arr_index_of(const Arr *a, const void *x)
{
    for (size_t i = 0; i < a->len; i++) {
        if (a->items[i] == x)
            return i;
    }
    return a->len;
}

typedef int (*KeepFn)(void *item, const void *ctx);

/* filter() assigned back in place. */
static void arr_retain(Arr *a, KeepFn keep, const void *ctx)
{
    size_t w = 0;

    for (size_t r = 0; r < a->len; r++) {
        if (keep(a->items[r], ctx))
            a->items[w++] = a->items[r];
    }
    a->len = w;
    cell_changed(&a->cell);
}

typedef int (*CmpFn)(const void *a, const void *b);

/* Stable insertion sort, the same order as Array.prototype.sort. */
static void arr_sort(Arr *a, CmpFn cmp)
{
    for (size_t i = 1; i < a->len; i++) {
        void *x = a->items[i];
        size_t j = i;

        while (j > 0 && cmp(a->items[j - 1], x) > 0) {
            a->items[j] = a->items[j - 1];
            j--;
        }
        a->items[j] = x;
    }
    cell_changed(&a->cell);
}

static void arr_free(Arr *a)
{
    free(a->items);
    free(a->cell.obs);
}

/* Map<string, number> in insertion order. */
typedef struct Entry {
    const char *key;
    intptr_t count;
} Entry;

static Entry *entry_find(const Arr *m, const char *key)
{
    for (size_t i = 0; i < m->len; i++) {
        Entry *e = m->items[i];

        if (strcmp(e->key, key) == 0)
            return e;
    }
    return NULL;
}

/* map.set(key, (map.get(key) ?? 0) + 1) */
static void map_inc(Arr *m, const char *key)
{
    Entry *e = entry_find(m, key);

    if (e) {
        e->count++;
        cell_changed(&m->cell);
        return;
    }
    e = xalloc(sizeof *e);
    e->key = key;
    e->count = 1;
    arr_push(m, e);
}

/* Set<string> of tags, in insertion order. */
static int tags_has(Arr *t, const char *tag)
{
    arr_read(t);
    if (!tag)
        return 0;
    for (size_t i = 0; i < t->len; i++) {
        if (strcmp((const char *)t->items[i], tag) == 0)
            return 1;
    }
    return 0;
}

static void tags_add(Arr *t, const char *tag)
{
    if (tag && !tags_has(t, tag))
        arr_push(t, (void *)tag);
}

/* ---- output buffer for render() ---- */

typedef struct StrBuf {
    char *p;
    size_t len, cap;
} StrBuf;

static void sb_reserve(StrBuf *sb, size_t extra)
{
    size_t need = sb->len + extra + 1;
    size_t cap = sb->cap ? sb->cap : 256;

    if (need <= sb->cap)
        return;
    while (cap < need)
        cap *= 2;
    sb->p = xrealloc(sb->p, cap);
    sb->cap = cap;
}

static void sb_puts(StrBuf *sb, const char *s)
{
    size_t n = strlen(s);

    sb_reserve(sb, n);
    memcpy(sb->p + sb->len, s, n);
    sb->len += n;
    sb->p[sb->len] = '\0';
}

static void sb_putc(StrBuf *sb, char c)
{
    sb_reserve(sb, 1);
    sb->p[sb->len++] = c;
    sb->p[sb->len] = '\0';
}

static void sb_num(StrBuf *sb, long v)
{
    char tmp[32];

    snprintf(tmp, sizeof tmp, "%ld", v);
    sb_puts(sb, tmp);
}

static void sb_dashes(StrBuf *sb)
{
    for (int i = 0; i < 31; i++)
        sb_putc(sb, '-');
}

static uint64_t fnv1a(const char *p, size_t n)
{
    uint64_t h = UINT64_C(1469598103934665603);

    for (size_t i = 0; i < n; i++) {
        h ^= (unsigned char)p[i];
        h *= UINT64_C(1099511628211);
    }
    return h;
}

/* ---- reminder model (proxy/common.js) ---- */

typedef struct Store Store;
typedef struct List List;
typedef struct Reminder Reminder;

struct Store {
    Arr lists;             /* List*   store.lists */
    Arr all_time;          /* Entry*  store.allTimeCompletedTags */
    uint64_t last_hash;    /* hash of the most recent autorun render */
    int render_runs;
};

struct List {
    Store *store;
    char name[32];
    Arr reminders;         /* Reminder* */
};

struct Reminder {
    List *list;
    const char *text;
    Arr tags;              /* const char*, a Set in insertion order */
    Cell cell;             /* isCompleted */
    int completed;
};

/* Objects are owned here and freed in teardown(). Removed objects stay
 * allocated until then, as garbage would in the JS. */
static Reminder **g_reminders;
static size_t g_nreminders, g_capreminders;
static List **g_lists;
static size_t g_nlists, g_caplists;

/* listName(index): new Date().toDateString() plus index days, e.g. "Thu Nov 19 2026". */
static void list_name(char *out, size_t n, size_t index)
{
    time_t now = time(NULL);
    struct tm tm;

    localtime_r(&now, &tm);
    tm.tm_mday += (int)index;
    mktime(&tm);
    strftime(out, n, "%a %b %d %Y", &tm);
}

static List *list_new(Store *s, size_t index)
{
    List *l = xalloc(sizeof *l);

    l->store = s;
    list_name(l->name, sizeof l->name, index);
    PUSH(g_lists, g_nlists, g_caplists, l);
    return l;
}

static intptr_t reminder_completed_data(void *ctx)
{
    Reminder *r = ctx;

    cell_track(&r->cell);
    return r->completed;
}

/* _isCompletedChanged: credit each tag of a newly completed reminder. */
static size_t g_effect_runs;

static void reminder_completed_changed(void *ctx, intptr_t completed)
{
    Reminder *r = ctx;

    g_effect_runs++;
    if (!completed)
        return;
    for (size_t i = 0; i < r->tags.len; i++)
        map_inc(&r->list->store->all_time, (const char *)r->tags.items[i]);
}

static Reminder *reminder_new(List *list, const char *text)
{
    Reminder *r = xalloc(sizeof *r);

    r->list = list;
    r->text = text;
    PUSH(g_reminders, g_nreminders, g_capreminders, r);
    make_reaction(reminder_completed_data, reminder_completed_changed, r);
    return r;
}

static void reminder_set_completed(Reminder *r, int value)
{
    if (r->completed == value)
        return;
    r->completed = value;
    cell_changed(&r->cell);
}

static void reminder_move_to_list(Reminder *r, List *next)
{
    Arr *from = &r->list->reminders;

    arr_splice_at(from, arr_index_of(from, r));
    r->list = next;
    arr_unshift(&next->reminders, r);
}

static void for_each_reminder(Store *s, void (*fn)(Reminder *))
{
    arr_read(&s->lists);
    for (size_t i = 0; i < s->lists.len; i++) {
        List *l = s->lists.items[i];

        arr_read(&l->reminders);
        for (size_t j = 0; j < l->reminders.len; j++)
            fn(l->reminders.items[j]);
    }
}

static const char *random_adjective(void)
{
    return adjectives[(size_t)(ADJ_COUNT * random01())];
}

static int cmp_completed_desc(const void *a, const void *b)
{
    return ((const Reminder *)b)->completed - ((const Reminder *)a)->completed;
}

static int keep_without_tag(void *item, const void *tag)
{
    return !tags_has(&((Reminder *)item)->tags, tag);
}

static int keep_incomplete(void *item, const void *unused)
{
    (void)unused;
    return !((Reminder *)item)->completed;
}

/* ---- render() (ReminderStore, ReminderList and Reminder .render) ---- */

static size_t list_completed_count(List *l)
{
    size_t n = 0;

    arr_read(&l->reminders);
    for (size_t i = 0; i < l->reminders.len; i++) {
        Reminder *r = l->reminders.items[i];

        cell_track(&r->cell);
        n += (size_t)r->completed;
    }
    return n;
}

static void render_reminder(Reminder *r, StrBuf *sb)
{
    cell_track(&r->cell);
    arr_read(&r->tags);
    sb_puts(sb, r->completed ? "[x] " : "[ ] ");
    sb_puts(sb, r->text);
    if (r->tags.len) {
        sb_puts(sb, " (");
        for (size_t i = 0; i < r->tags.len; i++) {
            if (i)
                sb_putc(sb, ' ');
            sb_putc(sb, '#');
            sb_puts(sb, (const char *)r->tags.items[i]);
        }
        sb_putc(sb, ')');
    }
}

static void render_list(List *l, StrBuf *sb)
{
    size_t done = list_completed_count(l);

    arr_read(&l->reminders);
    sb_puts(sb, "--- ");
    sb_puts(sb, l->name);
    sb_puts(sb, " (");
    sb_num(sb, (long)done);
    sb_puts(sb, " / ");
    sb_num(sb, (long)l->reminders.len);
    sb_puts(sb, ") ---\n");
    if (l->reminders.len == 0) {
        sb_puts(sb, "<empty>");
        return;
    }
    for (size_t i = 0; i < l->reminders.len; i++) {
        if (i)
            sb_putc(sb, '\n');
        render_reminder(l->reminders.items[i], sb);
    }
}

/* renderTags(entries) */
static void sb_tags(StrBuf *sb, const char *const *keys, const intptr_t *counts, size_t n)
{
    if (n == 0) {
        sb_puts(sb, "<none>");
        return;
    }
    for (size_t i = 0; i < n; i++) {
        if (i)
            sb_puts(sb, ", ");
        sb_putc(sb, '#');
        sb_puts(sb, keys[i]);
        sb_puts(sb, " (");
        sb_num(sb, (long)counts[i]);
        sb_putc(sb, ')');
    }
}

/* allTags, sorted by count (stable) and cut to five: popularTagsWithCount. */
static size_t popular_tags(Store *s, const char **names, intptr_t *counts)
{
    const char *keys[MAX_DISTINCT_TAGS];
    intptr_t cnt[MAX_DISTINCT_TAGS];
    size_t n = 0, take;

    arr_read(&s->lists);
    for (size_t i = 0; i < s->lists.len; i++) {
        List *l = s->lists.items[i];

        arr_read(&l->reminders);
        for (size_t j = 0; j < l->reminders.len; j++) {
            Reminder *r = l->reminders.items[j];

            arr_read(&r->tags);
            for (size_t k = 0; k < r->tags.len; k++) {
                const char *t = r->tags.items[k];
                size_t m = 0;

                while (m < n && strcmp(keys[m], t) != 0)
                    m++;
                if (m == n) {
                    if (n == MAX_DISTINCT_TAGS) {
                        fputs("proxy: too many distinct tags\n", stderr);
                        exit(2);
                    }
                    keys[n] = t;
                    cnt[n] = 0;
                    n++;
                }
                cnt[m]++;
            }
        }
    }
    for (size_t i = 1; i < n; i++) {
        for (size_t j = i; j > 0 && cnt[j - 1] < cnt[j]; j--) {
            const char *tk = keys[j - 1];
            intptr_t tc = cnt[j - 1];

            keys[j - 1] = keys[j];
            cnt[j - 1] = cnt[j];
            keys[j] = tk;
            cnt[j] = tc;
        }
    }
    take = n < 5 ? n : 5;
    for (size_t i = 0; i < take; i++) {
        names[i] = keys[i];
        counts[i] = cnt[i];
    }
    return take;
}

static void render_store(Store *s, StrBuf *sb)
{
    const char *pkeys[5];
    intptr_t pcounts[5];
    const char *akeys[MAX_DISTINCT_TAGS];
    intptr_t acounts[MAX_DISTINCT_TAGS];
    size_t completed = 0, total = 0, np, nall;

    arr_read(&s->lists);
    if (s->lists.len == 0)
        sb_puts(sb, "<empty>");
    for (size_t i = 0; i < s->lists.len; i++) {
        if (i)
            sb_putc(sb, '\n');
        render_list(s->lists.items[i], sb);
    }
    sb_putc(sb, '\n');
    sb_dashes(sb);
    sb_putc(sb, '\n');

    for (size_t i = 0; i < s->lists.len; i++) {
        List *l = s->lists.items[i];

        completed += list_completed_count(l);
        total += l->reminders.len;
    }
    sb_puts(sb, "completed: ");
    sb_num(sb, (long)completed);
    sb_puts(sb, " / ");
    sb_num(sb, (long)total);
    sb_putc(sb, '\n');
    sb_dashes(sb);
    sb_putc(sb, '\n');

    np = popular_tags(s, pkeys, pcounts);
    sb_puts(sb, "current popular tags: ");
    sb_tags(sb, pkeys, pcounts, np);

    sb_puts(sb, "\nall-time completed tags: ");
    arr_read(&s->all_time);
    nall = s->all_time.len;
    if (nall > MAX_DISTINCT_TAGS) {
        fputs("proxy: too many distinct tags\n", stderr);
        exit(2);
    }
    for (size_t i = 0; i < nall; i++) {
        Entry *e = s->all_time.items[i];

        akeys[i] = e->key;
        acounts[i] = e->count;
    }
    sb_tags(sb, akeys, acounts, nall);
}

/* The autorun body: render the whole store and remember the hash. */
static void render_effect(void *ctx, intptr_t unused)
{
    Store *s = ctx;
    static StrBuf buf;

    (void)unused;
    buf.len = 0;
    render_store(s, &buf);
    s->last_hash = fnv1a(buf.p, buf.len);
    s->render_runs++;
}

/* ---- the test script (proxy/common.js test()) ---- */

static void create_lists(Store *s, size_t count)
{
    for (size_t i = 0; i < count; i++)
        arr_push(&s->lists, list_new(s, i));
}

static void create_reminders(Store *s, size_t count)
{
    for (size_t li = 0; li < s->lists.len; li++) {
        List *list = s->lists.items[li];

        for (size_t i = 0; i < count; i++) {
            const char *text = nouns[((li * 10) + i) % NOUN_COUNT];

            arr_push(&list->reminders, reminder_new(list, text));
        }
    }
}

static void add_random_tags_one(Reminder *r)
{
    if (random01() < 0.5)
        tags_add(&r->tags, random_adjective());
    if (random01() < 0.5)
        tags_add(&r->tags, random_adjective());
}

static void complete_random_one(Reminder *r)
{
    reminder_set_completed(r, random01() < 0.5);
}

static void sort_by_completed(Store *s)
{
    for (size_t i = 0; i < s->lists.len; i++) {
        List *l = s->lists.items[i];

        arr_sort(&l->reminders, cmp_completed_desc);
    }
}

static void push_back_with_tag(Store *s, const char *tag)
{
    for (long i = (long)s->lists.len - 2; i >= 0; i--) {
        List *list = s->lists.items[i];
        List *next = s->lists.items[i + 1];

        for (long j = (long)list->reminders.len - 1; j >= 0; j--) {
            Reminder *r = list->reminders.items[j];

            if (tags_has(&r->tags, tag))
                reminder_move_to_list(r, next);
        }
    }
}

static void remove_with_tag(Store *s, const char *tag)
{
    for (size_t i = 0; i < s->lists.len; i++) {
        List *l = s->lists.items[i];

        arr_retain(&l->reminders, keep_without_tag, tag);
    }
}

static void remove_completed(Store *s)
{
    for (size_t i = 0; i < s->lists.len; i++) {
        List *l = s->lists.items[i];

        arr_retain(&l->reminders, keep_incomplete, NULL);
    }
}

static void remove_first_and_last(Store *s)
{
    for (size_t i = 0; i < s->lists.len; i++) {
        List *l = s->lists.items[i];

        arr_shift(&l->reminders);
        arr_pop(&l->reminders);
    }
}

static void remove_empty_lists(Store *s)
{
    for (long i = (long)s->lists.len - 1; i >= 0; i--) {
        List *l = s->lists.items[i];

        if (l->reminders.len == 0)
            arr_splice_at(&s->lists, (size_t)i);
    }
}

static void run_test(Store *s)
{
    const char *popular[5];
    intptr_t counts[5];
    const char *tag0, *tag2;
    size_t n;

    STEP(create_lists(s, LIST_COUNT));
    STEP(create_reminders(s, REMINDERS_PER_LIST));
    STEP(for_each_reminder(s, add_random_tags_one));

    n = popular_tags(s, popular, counts);
    tag0 = n > 0 ? popular[0] : NULL;
    tag2 = n > 2 ? popular[2] : NULL;

    STEP(push_back_with_tag(s, tag0));
    STEP(remove_with_tag(s, tag2));
    STEP(remove_first_and_last(s));
    STEP(for_each_reminder(s, complete_random_one));
    STEP(sort_by_completed(s));
    STEP(remove_completed(s));
    STEP(remove_empty_lists(s));
}

/* ---- iteration, validation and teardown ---- */

static void teardown(Store *s)
{
    for (size_t i = 0; i < g_nderivs; i++) {
        Derivation *d = g_derivs[i];

        d->disposed = 1;
        unsubscribe(d);
        free(d->deps);
        free(d);
    }
    g_nderivs = 0;
    g_qhead = g_qtail = 0;

    for (size_t i = 0; i < g_nreminders; i++) {
        Reminder *r = g_reminders[i];

        arr_free(&r->tags);
        free(r->cell.obs);
        free(r);
    }
    g_nreminders = 0;

    for (size_t i = 0; i < g_nlists; i++) {
        arr_free(&g_lists[i]->reminders);
        free(g_lists[i]);
    }
    g_nlists = 0;

    for (size_t i = 0; i < s->all_time.len; i++)
        free(s->all_time.items[i]);
    arr_free(&s->all_time);
    arr_free(&s->lists);
    free(s);
}

/* Returns 1 when the reactive render matched a fresh render of the final
 * store, and stores that render's hash in *hash_out. */
static int run_iteration(uint64_t *hash_out)
{
    StrBuf fresh = { NULL, 0, 0 };
    Store *s;
    int ok;

    g_rng = RNG_SEED;
    g_effect_runs = 0;
    s = xalloc(sizeof *s);
    make_autorun(render_effect, s);     /* autorun(() => store.render()) */
    run_test(s);

    /* No derivation is running here, so this render reads untracked. */
    render_store(s, &fresh);
    *hash_out = fnv1a(fresh.p, fresh.len);
#ifdef PROXY_TRACE
    printf("@@RENDER@@\n%.*s\n@@END@@ autorun=%d effects=%zu\n", (int)fresh.len, fresh.p, s->render_runs,
           g_effect_runs);
#endif
    ok = s->render_runs > 1 && *hash_out == s->last_hash;

    free(fresh.p);
    teardown(s);
    return ok;
}

int main(void)
{
    struct timespec t0, t1;
    uint64_t checksum = UINT64_C(1469598103934665603);
    double ms;

    clock_gettime(CLOCK_MONOTONIC, &t0);
    for (int it = 0; it < ITERATIONS; it++) {
        uint64_t h = 0;

        if (!run_iteration(&h)) {
            fprintf(stderr, "proxy-mobx: validation failed in iteration %d\n", it);
            return 1;
        }
        checksum = (checksum ^ h) * UINT64_C(1099511628211);
    }
    clock_gettime(CLOCK_MONOTONIC, &t1);

    ms = (double)(t1.tv_sec - t0.tv_sec) * 1000.0
       + (double)(t1.tv_nsec - t0.tv_nsec) / 1e6;
    printf("proxy-mobx: %d iterations in %.3f ms\n", ITERATIONS, ms);
    printf("checksum: %016llx, validation: ok\n", (unsigned long long)checksum);
    return 0;
}
