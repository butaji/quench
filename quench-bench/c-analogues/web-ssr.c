// jetstream-row: web-ssr
// jetstream-files: utils/StartupBenchmark.js web-ssr/benchmark.js
// iterations: 30
// jetstream-driver: class=Async preload=BUNDLE=web-ssr/dist/bundle.min.js
/* C analogue of the JetStream web-ssr row (web-ssr/dist/bundle.min.js, React 19.1.1 renderToString of
 * src/components.cjs over src/wine-data.cjs).
 *
 * Each timed iteration takes this iteration's copy of the bundle (cache-busting comment replaced, as
 * StartupBenchmark does; the copies are made before timing), parses it with the JS parser analogue (the
 * compile work of the engine's eval), builds the WINE_DATA of wine-data.cjs (the five literal wines, from
 * the file's object literals, and the 95 generated ones), renders WineList to HTML, and checks the HTML
 * length (in UTF-16 code units) and its quickHash, as the JS validate() does.
 *
 * The render is a C port of the components' JSX with a renderer that follows React's server output rules
 * for this tree: text children are escaped and get a "<!-- -->" separator when the previous sibling is text
 * (an absent child, false, null or undefined, does not break the run), attributes are escaped and style
 * objects print as "camelCase:value" pairs joined by ';'. The bundle's module body is not executed here.
 * Usage: web-ssr [iterations] [jetstream-root]. */
#define _DEFAULT_SOURCE
#include <math.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include "js-parser-analogue.h"

/* ---- string arena: strings and render nodes live until the end of the iteration ---- */
typedef struct { char **v; size_t n, cap; } SArena;
static SArena g_sa;
static const char *keep_str(const char *s) {
  if (g_sa.n == g_sa.cap) { g_sa.cap = g_sa.cap ? g_sa.cap * 2 : 4096; g_sa.v = realloc(g_sa.v, g_sa.cap * sizeof *g_sa.v); }
  char *c = strdup(s);
  g_sa.v[g_sa.n++] = c;
  return c;
}
static const char *keep_fmt(const char *fmt, ...) __attribute__((format(printf, 1, 2)));
static const char *keep_fmt(const char *fmt, ...) {
  char buf[1024];
  va_list ap;
  va_start(ap, fmt);
  vsnprintf(buf, sizeof buf, fmt, ap);
  va_end(ap);
  return keep_str(buf);
}
static void free_arena(void) {
  for (size_t i = 0; i < g_sa.n; i++) free(g_sa.v[i]);
  g_sa.n = 0;
}

/* ---- render tree (React elements and host text) ---- */
typedef enum { RN_NONE, RN_TEXT, RN_ELEM, RN_FRAG } RKind;
typedef struct RNode RNode;
struct RNode { RKind kind; const char *text; const char *tag; const char *attrs; RNode **kids; size_t nkids; };
/* every render node and child array is tracked and freed at the end of the iteration */
static void **g_mem;
static size_t g_nmem, g_capmem;
static void *track(void *p) {
  if (g_nmem == g_capmem) { g_capmem = g_capmem ? g_capmem * 2 : 4096; g_mem = realloc(g_mem, g_capmem * sizeof *g_mem); }
  g_mem[g_nmem++] = p;
  return p;
}
static void free_tracked(void) {
  for (size_t i = 0; i < g_nmem; i++) free(g_mem[i]);
  g_nmem = 0;
}
static RNode *node_new(RKind k) {
  RNode *n = track(calloc(1, sizeof *n));
  n->kind = k;
  return n;
}
static RNode *NO(void) { return node_new(RN_NONE); }
static RNode *T(const char *s) { RNode *n = node_new(RN_TEXT); n->text = s; return n; }
static RNode *TN(long long v) { return T(keep_fmt("%lld", v)); }
static RNode *fr_n(RNode **kids, size_t k) {
  RNode *n = node_new(RN_FRAG);
  n->kids = track(malloc((k ? k : 1) * sizeof *n->kids));
  for (size_t i = 0; i < k; i++) n->kids[i] = kids[i];
  n->nkids = k;
  return n;
}
static RNode *fr_make(RNode **kids) {   /* NULL-terminated list */
  size_t k = 0;
  while (kids[k]) k++;
  return fr_n(kids, k);
}
static RNode *el_make(const char *tag, const char *attrs, RNode *frag) {
  RNode *n = node_new(RN_ELEM);
  n->tag = tag;
  n->attrs = attrs;
  n->kids = frag ? frag->kids : NULL;
  n->nkids = frag ? frag->nkids : 0;
  return n;
}
#define FR(...) fr_make((RNode *[]){__VA_ARGS__, NULL})
#define EL(tag, attrs, ...) el_make(tag, attrs, FR(__VA_ARGS__))
#define EL0(tag, attrs) el_make(tag, attrs, fr_make((RNode *[]){NULL}))

/* ---- attributes: the escaped text React writes for each prop ---- */
static void html_escape_into(char **out, size_t *n, size_t *cap, const char *s) {
  for (const char *p = s; *p; p++) {
    const char *rep = NULL;
    switch (*p) {
      case '&': rep = "&amp;"; break;
      case '<': rep = "&lt;"; break;
      case '>': rep = "&gt;"; break;
      case '"': rep = "&quot;"; break;
      case '\'': rep = "&#x27;"; break;
    }
    size_t add = rep ? strlen(rep) : 1;
    if (*n + add + 1 > *cap) { *cap = (*n + add + 1) * 2; *out = realloc(*out, *cap); }
    if (rep) memcpy(*out + *n, rep, add); else (*out)[*n] = *p;
    *n += add;
    (*out)[*n] = 0;
  }
}
static const char *escaped(const char *s) {
  char *out = NULL;
  size_t n = 0, cap = 0;
  html_escape_into(&out, &n, &cap, s);
  const char *r = keep_str(out ? out : "");
  free(out);
  return r;
}
static const char *attr(const char *name, const char *value) { return keep_fmt(" %s=\"%s\"", name, escaped(value)); }
/* style={{camelKey: value, ...}} -> "kebab-key:value;..." (values are strings here) */
static const char *style(int n, const char **keys, const char **vals) {
  char buf[1024] = "";
  size_t len = 0;
  for (int i = 0; i < n; i++) {
    if (i) buf[len++] = ';';
    for (const char *k = keys[i]; *k; k++) {
      if (*k >= 'A' && *k <= 'Z') { buf[len++] = '-'; buf[len++] = (char)(*k - 'A' + 'a'); }
      else buf[len++] = *k;
    }
    buf[len++] = ':';
    len += (size_t)snprintf(buf + len, sizeof buf - len, "%s", vals[i]);
  }
  buf[len] = 0;
  return keep_fmt(" style=\"%s\"", escaped(buf));
}

/* ---- serialization with React's text rules ---- */
typedef struct { char *p; size_t n, cap; } OutBuf;
static void ob_add(OutBuf *b, const char *s, size_t len) {
  if (b->n + len + 1 > b->cap) { b->cap = (b->n + len + 1) * 2; b->p = realloc(b->p, b->cap); }
  memcpy(b->p + b->n, s, len);
  b->n += len;
  b->p[b->n] = 0;
}
static void ob_str(OutBuf *b, const char *s) { ob_add(b, s, strlen(s)); }
static void serialize(OutBuf *b, const RNode *n, bool *prev_text) {
  switch (n->kind) {
    case RN_NONE:
      return;
    case RN_TEXT:
      if (*prev_text) ob_str(b, "<!-- -->");
      ob_str(b, escaped(n->text));
      *prev_text = true;
      return;
    case RN_FRAG:
      for (size_t i = 0; i < n->nkids; i++) serialize(b, n->kids[i], prev_text);
      return;
    case RN_ELEM: {
      ob_str(b, "<");
      ob_str(b, n->tag);
      ob_str(b, n->attrs);
      ob_str(b, ">");
      bool inner = false;
      for (size_t i = 0; i < n->nkids; i++) serialize(b, n->kids[i], &inner);
      ob_str(b, "</");
      ob_str(b, n->tag);
      ob_str(b, ">");
      *prev_text = false;
      return;
    }
  }
}

/* ---- data: wine-data.cjs ---- */
typedef struct { const char *user; int rating; const char *comment; } WReview;
typedef struct { const char *grape; int percentage; } WGrape;
typedef struct { int present, quantity, price, has_sale, sale; } WBottle;
typedef struct {
  const char *name;
  struct { const char *name, *url, *desc, *est; } wm;   /* the winemaker */
  const char *tasting;
  const char *tags[4]; int ntags;
  WGrape grapes[4]; int ngrapes;
  WReview reviews[4]; int nreviews;
  int vintage;
  const char *region_name, *province, *country;
  WBottle b750, magnum, balthazar;
} WineRec;

static const WineRec literal_wines[5] = {
  {"Ch\303\242teau Ma\303\256tre 2015",
    {"Ch\303\242teau Ma\303\256tre", "https://www.chateau-ma\303\256tre.com", "Ch\303\242teau Ma\303\256tre, a First Growth Bordeaux, is one of the most famous wines in the world.", "1590"}, "Aromas of blackcurrant, violet, and cedar, with a long, elegant finish.",
    {"elegant", "structured", "classic", "bordeaux"}, 4,
    {{"Cabernet Sauvignon", 87}, {"Merlot", 8}, {"Cabernet Franc", 3}, {"Petit Verdot", 2}}, 4,
    {{"WineEnthusiast", 99, "A legendary vintage. Perfect structure and complexity."}, {"SommelierChoice", 98, "Absolutely stunning. Aromas of blackcurrant, violet, and cedar."}}, 2,
    2015,
    "Bordeaux", "Gironde", "France",
    {1, 120, 1500, 1, 1350}, {1, 24, 3200, 0, 0}, {1, 6, 10000, 0, 0}},
  {"Domaine de la Roman\303\251e-Comptoir 2016",
    {"Domaine de la Roman\303\251e-Comptoir", "https://www.romanee-comptoir.fr", "Producer of the world's most sought-after Pinot Noir.", "1869"}, "Ethereal and unforgettable, with notes of red fruit, spice, and earth.",
    {"ethereal", "collectible", "burgundy", "pinot-noir"}, 4,
    {{"Pinot Noir", 100}}, 1,
    {{"CriticalAcclaim", 100, "The pinnacle of Pinot Noir. Ethereal and unforgettable."}, {"CollectorX", 100, "A wine that transcends words. Pure silk."}}, 2,
    2016,
    "Burgundy", "Burgundy", "France",
    {1, 50, 25000, 0, 0}, {1, 10, 55000, 1, 50000}, {0, 0, 0, 0, 0}},
  {"Screaming Sparrow Cabernet Sauvignon 2018",
    {"Screaming Sparrow Winery", "https://www.screaming-sparrow-wines.com", "A cult winery in Napa Valley, known for its extremely limited production Cabernet Sauvignon.", "1986"}, "Rich, opulent, and perfectly balanced, with layers of dark fruit, chocolate, and spice.",
    {"cult", "napa-valley", "powerful", "opulent"}, 4,
    {{"Cabernet Sauvignon", 92}, {"Merlot", 5}, {"Cabernet Franc", 3}}, 3,
    {{"NapaFan", 99, "Incredibly powerful and elegant at the same time. A Napa icon."}, {"LuxuryWines", 98, "A blockbuster of a wine. Rich, opulent, and perfectly balanced."}}, 2,
    2018,
    "Napa Valley", "California", "USA",
    {1, 30, 3500, 0, 0}, {1, 5, 7500, 0, 0}, {0, 0, 0, 0, 0}},
  {"Penn & Folders Grange 2017",
    {"Penn & Folders", "https://www.penn-folders.com", "One of Australia's most famed and respected winemakers.", "1844"}, "Bold, complex, and built to last, with notes of dark fruit, spice, and a hint of vanilla.",
    {"bold", "australian", "shiraz", "iconic"}, 4,
    {{"Shiraz", 97}, {"Cabernet Sauvignon", 3}}, 2,
    {{"AussieWineLover", 97, "A true Australian legend. Bold, complex, and built to last."}, {"GlobalVino", 98, "Concentrated dark fruit, spice, and a long, savory finish."}}, 2,
    2017,
    "South Australia", "South Australia", "Australia",
    {1, 200, 800, 1, 750}, {1, 50, 1700, 0, 0}, {1, 10, 5000, 0, 0}},
  {"Vega Siciliano \303\232nico 2011",
    {"Bodegas Vega Siciliano", "https://www.vega-siciliano.es", "An iconic Spanish winery in Ribera del Duero, known for its long-aging wines.", "1864"}, "Complex and majestic, with layers of flavor that unfold in the glass, including notes of leather, tobacco, and dark fruit.",
    {"iconic", "spanish", "tempranillo", "complex"}, 4,
    {{"Tinto Fino", 95}, {"Cabernet Sauvignon", 5}}, 2,
    {{"SpanishWineGuru", 97, "An iconic Spanish wine with incredible aging potential. Complex and majestic."}, {"EuroCollector", 96, "A benchmark for Ribera del Duero. Layers of flavor that unfold in the glass."}}, 2,
    2011,
    "Ribera del Duero", "Castile and Le\303\263n", "Spain",
    {1, 150, 500, 0, 0}, {1, 30, 1100, 1, 1000}, {0, 0, 0, 0, 0}}
};

/* The generated wines: Array.from({ length: 95 }, (_, i) => ({...})) */
static void generated_wine(int i, WineRec *w) {
  memset(w, 0, sizeof *w);
  w->name = keep_fmt("Generic Wine No. %d", i + 1);
  w->wm.name = keep_fmt("Winery %d", i % 10);
  w->wm.url = keep_fmt("http://winery-%d.com", i % 10);
  w->wm.desc = keep_fmt("Description for winery %d", i % 10);
  w->wm.est = keep_fmt("%d", 1900 + (i % 20));
  w->tasting = "A solid wine for the price. Good fruit and structure. Notes of cherry and spice.";
  w->tags[0] = "solid"; w->tags[1] = "value"; w->tags[2] = keep_fmt("generic-tag-%d", i % 3); w->ntags = 3;
  w->region_name = keep_fmt("Region %d", i % 5);
  w->province = keep_fmt("Province %d", i % 2);
  w->country = "Country";
  w->grapes[0] = (WGrape){"Grape A", 60};
  w->grapes[1] = (WGrape){"Grape B", 40};
  w->ngrapes = 2;
  w->reviews[0] = (WReview){keep_fmt("User%d", i), 85 + (i % 10), keep_fmt("A solid wine for the price. Good fruit and structure. Review %d-1.", i)};
  w->reviews[1] = (WReview){keep_fmt("User%d", i + 100), 88 + (i % 5), keep_fmt("Enjoyable, with notes of cherry and spice. Review %d-2.", i)};
  w->nreviews = 2;
  w->vintage = 2000 + (i % 20);
  w->b750 = (WBottle){1, i * 10, 50 + i, (i % 3 == 0) ? 1 : 0, (i % 3 == 0) ? 45 + i : 0};
  w->magnum = (WBottle){1, i, 120 + i * 2, 0, 0};
}

/* ---- components (the JSX of components.cjs) ---- */
static double js_round_pos(double x) { double f = floor(x); return (x - f >= 0.5) ? f + 1 : f; }
static RNode *c_review(const WReview *r) {
  return EL("div", attr("class", "review"),
            EL("h4", "", T(r->user), T(" ("), TN(r->rating), T("/100)")),
            EL("p", "", T(r->comment)));
}
static RNode *c_grape(const WGrape *g) {
  const char *ks1[] = {"display", "border", "width", "height", "marginLeft", "verticalAlign"};
  const char *vs1[] = {"inline-block", "1px solid #ccc", "100px", "10px", "10px", "middle"};
  const char *ks2[] = {"width", "height", "backgroundColor"};
  const char *vs2[] = {keep_fmt("%d%%", g->percentage), "100%", "#663399"};
  return EL("li", "",
            EL("a", attr("href", keep_fmt("/list/?grape=%s", g->grape)), T(g->grape)),
            T(" ("), TN(g->percentage), T("%)"),
            EL("div", style(6, ks1, vs1), EL("div", style(3, ks2, vs2), NO())));
}
static RNode *c_price(int price, int has_sale, int sale) {
  bool onSale = has_sale && sale != 0 && sale < price;
  if (onSale)
    return EL("span", "",
              EL("span", style(1, (const char *[]){"textDecoration"}, (const char *[]){"line-through"}), T("$"), TN(price)),
              T(" $"), TN(sale));
  return EL("span", "", T("$"), TN(price));
}
static RNode *c_price_rating(int price) {
  const char *r = price < 100 ? "€" : price < 1000 ? "€€" : "€€€";
  return EL("span", "", T(r));
}
static RNode *c_bottle(const char *label, const WBottle *b) {
  bool onSale = b->has_sale && b->sale != 0 && b->sale < b->price;
  int percentageDiff = onSale ? (int)js_round_pos(((double)(b->price - b->sale) / b->price) * 100) : 0;
  return EL("li", "",
            T(keep_fmt("%s ", label)), TN(b->quantity), T(" "),
            b->quantity < 10 ? T("(Low stock)") : NO(),
            T(" - "), c_price(b->price, b->has_sale, b->sale), T(" "), c_price_rating(b->price),
            onSale ? EL("span", "", T("❗("), TN(percentageDiff), T("% off)")) : NO());
}
static RNode *c_stock(const WineRec *w) {
  bool any = false;
  const WBottle *bs[3] = {&w->b750, &w->magnum, &w->balthazar};
  for (int i = 0; i < 3; i++) if (bs[i]->present && bs[i]->quantity > 0) any = true;
  RNode *list;
  if (any)
    list = EL("ul", "",
              w->b750.present ? c_bottle("750ml:", &w->b750) : NO(),
              w->magnum.present ? c_bottle("Magnum:", &w->magnum) : NO(),
              w->balthazar.present ? c_bottle("Balthazar:", &w->balthazar) : NO());
  else
    list = EL("p", "", T("Out of stock"));
  return EL("div", attr("class", "stock"), EL("h4", "", T("Stock")), list);
}
static const char *flag_for(const char *country) {
  if (!strcmp(country, "France")) return "🇫🇷";
  if (!strcmp(country, "USA")) return "🇺🇸";
  if (!strcmp(country, "Australia")) return "🇦🇺";
  if (!strcmp(country, "Spain")) return "🇪🇸";
  if (!strcmp(country, "Country")) return "🏳️";
  return NULL;
}
static RNode *c_region(const WineRec *w) {
  const char *flag = flag_for(w->country);
  return EL("h3", "",
            T(w->region_name), T(" - "),
            EL("a", attr("href", keep_fmt("/list/?country=%s&province=%s", w->country, w->province)), T(w->province)),
            T(","),
            EL("a", attr("href", keep_fmt("/list/?country=%s", w->country)), T(w->country)),
            flag ? T(flag) : NO());
}
static RNode *c_wine(const WineRec *w) {
  bool high = false;
  for (int i = 0; i < w->nreviews; i++) if (w->reviews[i].rating > 90) high = true;
  RNode *tags[4], *grapes[4], *reviews[4];
  for (int i = 0; i < w->ntags; i++)
    tags[i] = EL("li", "", EL("a", attr("href", keep_fmt("/list/?tag=%s", w->tags[i])), T(w->tags[i])));
  for (int i = 0; i < w->ngrapes; i++) grapes[i] = c_grape(&w->grapes[i]);
  for (int i = 0; i < w->nreviews; i++) reviews[i] = c_review(&w->reviews[i]);
  RNode *kids[16];
  size_t k = 0;
  kids[k++] = EL("h2", "", T(w->name), T(" "), high ? T("\u2b50") : NO());
  kids[k++] = EL("div", attr("class", "winemaker"),
                 EL("h3", "", EL("a", attr("href", w->wm.url), T(w->wm.name))),
                 EL("p", "", T(w->wm.desc)),
                 EL("p", "", T("Established: "), T(w->wm.est)));
  kids[k++] = c_region(w);
  kids[k++] = EL("h4", "", T("Vintage: "), TN(w->vintage));
  kids[k++] = EL("h4", "", T("Tasting Notes"));
  kids[k++] = EL("p", "", T(w->tasting));
  kids[k++] = el_make("ul", attr("class", "tags"), fr_n(tags, (size_t)w->ntags));
  kids[k++] = EL("h4", "", T("Grape Composition"));
  kids[k++] = el_make("ul", attr("class", "grape-composition"), fr_n(grapes, (size_t)w->ngrapes));
  kids[k++] = c_stock(w);
  kids[k++] = EL("h4", "", T("Reviews"));
  for (int i = 0; i < w->nreviews; i++) kids[k++] = reviews[i];
  return el_make("div", attr("class", "wine-card"), fr_n(kids, k));
}

/* ---- WINE_DATA: the five literal wines (wine-data.cjs), then the 95 generated ones ---- */
#define NLIT 5
#define NGEN 95
static WineRec g_wines[NLIT + NGEN];
static void build_wines(void) {
  for (int i = 0; i < NLIT; i++) g_wines[i] = literal_wines[i];
  for (int i = 0; i < NGEN; i++) generated_wine(i, &g_wines[NLIT + i]);
}

/* WineList: the heading, then one WineCard per wine, inside div.wine-list */
static RNode *c_wine_list(void) {
  RNode *kids[NLIT + NGEN + 1];
  size_t k = 0;
  kids[k++] = EL("h1", "", T("Complex Wine List"));
  for (int i = 0; i < NLIT + NGEN; i++) kids[k++] = c_wine(&g_wines[i]);
  return el_make("div", attr("class", "wine-list"), fr_n(kids, k));
}

/* quickHash of StartupBenchmark: hash over UTF-16 code units, from the end, step 919 (int32 arithmetic) */
static size_t utf16_units(const char *s) {
  size_t n = 0;
  for (const unsigned char *p = (const unsigned char *)s; *p; p++)
    if ((*p & 0xC0) != 0x80) n += (*p >= 0xF0) ? 2 : 1;
  return n;
}
static uint16_t *utf16_decode(const char *s, size_t *out_n) {
  uint16_t *u = malloc((utf16_units(s) + 1) * sizeof *u);
  size_t k = 0;
  for (const unsigned char *p = (const unsigned char *)s; *p;) {
    uint32_t cp;
    int len;
    if (*p < 0x80) { cp = *p; len = 1; }
    else if (*p < 0xE0) { cp = *p & 0x1F; len = 2; }
    else if (*p < 0xF0) { cp = *p & 0x0F; len = 3; }
    else { cp = *p & 0x07; len = 4; }
    for (int j = 1; j < len; j++) cp = (cp << 6) | (p[j] & 0x3F);
    p += len;
    if (cp >= 0x10000) {
      cp -= 0x10000;
      u[k++] = (uint16_t)(0xD800 + (cp >> 10));
      u[k++] = (uint16_t)(0xDC00 + (cp & 0x3FF));
    } else {
      u[k++] = (uint16_t)cp;
    }
  }
  u[k] = 0;
  *out_n = k;
  return u;
}
static int32_t quick_hash(const char *s) {
  size_t n;
  uint16_t *u = utf16_decode(s, &n);
  uint32_t h = 5381;
  for (long i = (long)n; i > 0; i -= 919) {
    uint32_t c = (i < (long)n) ? u[i] : 0;   /* charCodeAt(length) is NaN, and NaN | 0 is 0 */
    h = (h * 33u) ^ c;
  }
  free(u);
  return (int32_t)h;
}

/* ---- bundle copies: the cache-busting comment is replaced per iteration, as StartupBenchmark does ---- */
static const char CACHE_BUST[] = "\n/*ThouShaltNotCache*/\n";
static char *iteration_source(const char *bundle, size_t n, int iteration, size_t *out_n) {
  int cacheId = (iteration / 8) * 8;
  char id[32];
  int idn = snprintf(id, sizeof id, "\n/*%d*/\n", cacheId);
  size_t cl = strlen(CACHE_BUST);
  char *out = malloc(n * 2 + 64);
  size_t o = 0;
  for (size_t i = 0; i < n;) {
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
  const char *dump = argc > 3 ? argv[3] : NULL;   /* optional: write the first iteration's HTML here */
  char path[4096];
  snprintf(path, sizeof path, "%s/web-ssr/dist/bundle.min.js", root);
  size_t bn;
  char *bundle = js_read_file(path, &bn);
  JsContext ctx;
  js_context_init(&ctx);
  double total = 0;
  int ok = 1;
  size_t units = 0;
  int32_t hash = 0;
  for (int it = 0; it < iterations; it++) {
    size_t n;
    char *src = iteration_source(bundle, bn, it, &n);   /* made outside the timed part, as in the JS */
    double t0 = js_now_ms();
    size_t tokens = 0;
    js_parse(&ctx, src, n, 0, &tokens);                  /* compile the iteration's bundle */
    free(src);
    build_wines();                                       /* WINE_DATA is built on each module load */
    OutBuf ob = {0};
    bool prev = false;
    RNode *list = c_wine_list();
    serialize(&ob, list, &prev);
    units = utf16_units(ob.p);
    hash = quick_hash(ob.p);
    double t1 = js_now_ms();
    if (it == 0 && dump) {
      FILE *f = fopen(dump, "wb");
      if (f) { fwrite(ob.p, 1, ob.n, f); fclose(f); }
    }
    if (units != 183778 || hash != 1177839858) {
      fprintf(stderr, "web-ssr: expected length 183778 and hash 1177839858, got %zu and %d\n", units, hash);
      ok = 0;
    }
    total += t1 - t0;
    free(ob.p);
    free_tracked();
    free_arena();
  }
  printf("web-ssr %d iterations %.3f ms (average %.3f ms)\n", iterations, total, total / iterations);
  printf("validation %s: html length %zu, quickHash %d\n", ok ? "ok" : "FAILED", units, hash);
  js_context_destroy(&ctx);
  free(bundle);
  return ok ? 0 : 1;
}
