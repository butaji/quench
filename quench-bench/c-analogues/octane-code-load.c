// jetstream-row: octane-code-load
// jetstream-files: Octane/code-first-load.js
// iterations: 120
// jetstream-driver: deterministic=true exports=MockElement
/* C analogue of JetStream's octane-code-load row (Octane/code-first-load.js). Each iteration
 * builds two sources from the file's BASE_JS (Closure base.js) and JQUERY_JS (jQuery 1.7.2)
 * strings, exactly as runClosure and runJQuery do: a salt prefix, the cache-busting rename of
 * "goog" or "jQuery" with the Jenkins hash of the salt's digits (same double arithmetic as the
 * JS), and the result. The JS row evaluates each source; this analogue parses each source into
 * an AST with the shared parser, as often as the row evaluates it (two sources per iteration).
 * Execution of the sources is not part of the analogue. Building the strings is timed, as it is
 * inside the JS runIteration. Expected counts: node 22 with the row's own parser (tokens: true)
 * on the iteration-0 texts, closure 2311 nodes and 2956 tokens, jQuery 32846 and 42837; the
 * counts are checked on the first iteration. Usage: octane-code-load [root], default ../jetstream. */
#include "js-parser-analogue.h"

/* ---- growable string buffer ---- */
typedef struct { char *p; size_t n, cap; } Buf;
static void buf_add(Buf *b, const char *s, size_t n) {
  if (b->n + n + 1 > b->cap) {
    b->cap = (b->n + n + 1) * 2;
    b->p = realloc(b->p, b->cap);
    if (!b->p) { fprintf(stderr, "out of memory\n"); exit(2); }
  }
  memcpy(b->p + b->n, s, n);
  b->n += n;
  b->p[b->n] = 0;
}
static void buf_str(Buf *b, const char *s) { buf_add(b, s, strlen(s)); }
static void buf_int(Buf *b, int v) { char t[16]; snprintf(t, sizeof t, "%d", v); buf_str(b, t); }

/* ---- the file's string constants ---- */
/* Decodes a JavaScript string literal whose opening quote is just before p: escapes, line
 * continuations and \uXXXX/\xXX are decoded as the engine does. Returns a malloc'd buffer. */
static char *decode_literal(const char *p, size_t *out_n) {
  const char *e = p;
  while (*e != '"') {
    if (!*e) { fprintf(stderr, "unterminated string constant\n"); exit(2); }
    e += (*e == '\\') ? 2 : 1;
  }
  char *out = malloc((size_t)(e - p) + 1);
  size_t o = 0;
  const char *i = p;
  while (i < e) {
    if (*i != '\\') { out[o++] = *i++; continue; }
    i++;
    if (*i == '\r') { i++; if (*i == '\n') i++; continue; }
    if (*i == '\n') { i++; continue; }
    unsigned c = (unsigned char)*i;
    if (c == 'n') { out[o++] = '\n'; i++; }
    else if (c == 't') { out[o++] = '\t'; i++; }
    else if (c == 'r') { out[o++] = '\r'; i++; }
    else if (c == 'b') { out[o++] = '\b'; i++; }
    else if (c == 'f') { out[o++] = '\f'; i++; }
    else if (c == 'v') { out[o++] = '\v'; i++; }
    else if (c == '0' && !(i[1] >= '0' && i[1] <= '9')) { out[o++] = 0; i++; }
    else if (c == 'x' || c == 'u') {
      int digits = c == 'x' ? 2 : 4;
      unsigned cp = 0;
      i++;
      for (int k = 0; k < digits; k++, i++) {
        int d = *i >= '0' && *i <= '9' ? *i - '0' : (*i | 0x20) >= 'a' && (*i | 0x20) <= 'f' ? (*i | 0x20) - 'a' + 10 : -1;
        if (d < 0) { fprintf(stderr, "bad escape in string constant\n"); exit(2); }
        cp = cp * 16 + (unsigned)d;
      }
      if (cp < 0x80) out[o++] = (char)cp;
      else if (cp < 0x800) { out[o++] = (char)(0xC0 | (cp >> 6)); out[o++] = (char)(0x80 | (cp & 0x3F)); }
      else { out[o++] = (char)(0xE0 | (cp >> 12)); out[o++] = (char)(0x80 | ((cp >> 6) & 0x3F)); out[o++] = (char)(0x80 | (cp & 0x3F)); }
    } else { out[o++] = (char)c; i++; }  /* identity escape: \" \' \\ \- and so on */
  }
  out[o] = 0;
  *out_n = o;
  return out;
}
static const char *find_after(const char *hay, const char *needle) {
  const char *p = strstr(hay, needle);
  if (!p) { fprintf(stderr, "missing %s\n", needle); exit(2); }
  return p + strlen(needle);
}

/* ---- cache busting, as in code-first-load.js (jenkinsHash, cacheBust) ---- */
static int32_t to_i32(double d) {
  if (!isfinite(d)) return 0;
  double m = fmod(trunc(d), 4294967296.0);
  if (m < 0) m += 4294967296.0;
  return (int32_t)(uint32_t)m;
}
static double jenkins_hash(const int *key, int len) {
  double hash = 0;
  for (int i = 0; i < len; i++) {
    hash += key[i];
    hash += (double)(int32_t)((uint32_t)to_i32(hash) << 10);
    int32_t h = to_i32(hash);
    hash = (double)(h ^ (h >> 6));
  }
  hash += (double)(int32_t)((uint32_t)to_i32(hash) << 3);
  int32_t h = to_i32(hash);
  hash = (double)(h ^ (h >> 11));
  hash += (double)(int32_t)((uint32_t)to_i32(hash) << 15);
  return hash;
}
/* str.replace(new RegExp(old, "g"), rep): every occurrence, left to right. */
static char *replace_all(const char *s, size_t n, const char *old, const char *rep, size_t *out_n) {
  size_t ol = strlen(old), rl = strlen(rep), count = 0;
  for (const char *p = s; (p = strstr(p, old)) != NULL; p += ol) count++;
  char *out = malloc(n + count * (rl - ol + 1) + 1);
  size_t o = 0;
  const char *p = s, *q;
  while ((q = strstr(p, old)) != NULL) {
    memcpy(out + o, p, (size_t)(q - p)); o += (size_t)(q - p);
    memcpy(out + o, rep, rl); o += rl;
    p = q + ol;
  }
  size_t rest = n - (size_t)(p - s);
  memcpy(out + o, p, rest); o += rest;
  out[o] = 0;
  *out_n = o;
  return out;
}
static char *cache_bust(const char *s, size_t n, const char *old, int salt, size_t *out_n) {
  char digits[16];
  snprintf(digits, sizeof digits, "%d", salt);
  int keys[16], k = 0;
  for (const char *d = digits; *d; d++) keys[k++] = *d - '0';
  double hash = fabs(jenkins_hash(keys, k));
  char rep[64], tmp[32];
  int t = 0;
  unsigned long long v = (unsigned long long)hash;
  do { tmp[t++] = "0123456789abcdefghijklmnopqrstuvwxyz"[v % 36]; v /= 36; } while (v);
  snprintf(rep, sizeof rep, "%s", old);
  for (int i = t - 1, j = (int)strlen(old); i >= 0; i--) rep[j++] = tmp[i];
  rep[strlen(old) + t] = 0;
  return replace_all(s, n, old, rep, out_n);
}

typedef struct {
  char *base_js, *jquery_js, *jq_head, *jq_tail;
  size_t base_n, jquery_n, jq_head_n, jq_tail_n;
} Sources;

/* runClosure: src = "var googsalt=" + salt + ";" + BASE_JS + "(function(){...})();", then cacheBust */
static char *closure_source(const Sources *s, int salt, size_t *out_n) {
  Buf b = {0};
  buf_str(&b, "var googsalt="); buf_int(&b, salt); buf_str(&b, ";");
  buf_add(&b, s->base_js, s->base_n);
  buf_str(&b, "(function(){return goog.cloneObject(googsalt);})();");
  size_t n;
  char *out = cache_bust(b.p, b.n, "goog", salt, &n);
  free(b.p);
  *out_n = n;
  return out;
}
/* runJQuery: src = windowmock prefix + "var jQuerySalt=" + salt + ";" + JQUERY_JS + tail, then cacheBust */
static char *jquery_source(const Sources *s, int salt, size_t *out_n) {
  Buf b = {0};
  buf_add(&b, s->jq_head, s->jq_head_n);
  buf_str(&b, "var jQuerySalt="); buf_int(&b, salt); buf_str(&b, ";");
  buf_add(&b, s->jquery_js, s->jquery_n);
  buf_add(&b, s->jq_tail, s->jq_tail_n);
  size_t n;
  char *out = cache_bust(b.p, b.n, "jQuery", salt, &n);
  free(b.p);
  *out_n = n;
  return out;
}

static void load_sources(const char *root, Sources *s) {
  char path[4096];
  snprintf(path, sizeof path, "%s/Octane/code-first-load.js", root);
  size_t flen;
  char *file = js_read_file(path, &flen);
  s->base_js = decode_literal(find_after(file, "var BASE_JS = \""), &s->base_n);
  s->jquery_js = decode_literal(find_after(file, "var JQUERY_JS = \""), &s->jquery_n);
  /* the two literals of runJQuery that surround JQUERY_JS (joined with line continuations) */
  const char *head = strstr(file, "var src = \"var windowmock");
  if (!head) { fprintf(stderr, "missing runJQuery source\n"); exit(2); }
  s->jq_head = decode_literal(head + strlen("var src = \""), &s->jq_head_n);
  s->jq_tail = decode_literal(find_after(file, "\"(function(){return windowmock"), &s->jq_tail_n);
  /* the tail literal starts after the quote, so re-add the "(function(){return windowmock" text */
  char *tail = malloc(s->jq_tail_n + 64);
  memcpy(tail, "(function(){return windowmock", 29);
  memcpy(tail + 29, s->jq_tail, s->jq_tail_n);
  free(s->jq_tail);
  s->jq_tail = tail;
  s->jq_tail_n += 29;
  free(file);
}

int main(int argc, char **argv) {
  const char *root = argc > 2 ? argv[2] : "../jetstream";
  const int iterations = argc > 1 ? atoi(argv[1]) : 120;
  Sources s;
  load_sources(root, &s);
  JsContext ctx;
  js_context_init(&ctx);
  double total = 0;
  int ok = 1;
  for (int it = 0; it < iterations; it++) {
    double t0 = js_now_ms();
    size_t n;
    char *src = closure_source(&s, 2 * it, &n);
    size_t tokens = 0;
    JsNode *file = js_parse(&ctx, src, n, 0, &tokens);
    if (it == 0) {
      size_t nodes = js_count_nodes(file);
      if (nodes != 2311 || tokens != 2956) { fprintf(stderr, "closure: nodes %zu tokens %zu\n", nodes, tokens); ok = 0; }
    }
    free(src);
    src = jquery_source(&s, 2 * it + 1, &n);
    file = js_parse(&ctx, src, n, 0, &tokens);
    if (it == 0) {
      size_t nodes = js_count_nodes(file);
      if (nodes != 32846 || tokens != 42837) { fprintf(stderr, "jquery: nodes %zu tokens %zu\n", nodes, tokens); ok = 0; }
    }
    free(src);
    total += js_now_ms() - t0;
  }
  printf("octane-code-load %d iterations %.3f ms (average %.3f ms)\n", iterations, total, total / iterations);
  printf("validation %s: node and token counts of the iteration-0 closure and jQuery sources match the JS parser\n", ok ? "ok" : "FAILED");
  js_context_destroy(&ctx);
  return ok ? 0 : 1;
}
