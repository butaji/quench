/*
 * BERT tokenizer as configured in tokenizer.json:
 *   normalizer:    BertNormalizer (clean_text, handle_chinese_chars, lowercase,
 *                  strip_accents: null -> follows lowercase)
 *   pre_tokenizer: BertPreTokenizer (whitespace, then punctuation split)
 *   model:         WordPiece (continuing prefix "##", max chars per word, [UNK])
 *   post:          TemplateProcessing "single" template ([CLS] $A [SEP])
 *   added tokens:  matched on the raw text before normalization when normalized=false
 *
 * Limits: case mapping covers ASCII, Latin-1, Latin Extended-A (pairs), Greek
 * and Cyrillic basic letters; accent stripping covers Latin-1 letters only.
 * Those are the characters the benchmark inputs and common English text use.
 */
#define _POSIX_C_SOURCE 200809L

#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "tokenizer.h"

#define FAIL(...)                       \
  do {                                  \
    set_err(err, errlen, __VA_ARGS__);  \
    return -1;                          \
  } while (0)

static void set_err(char *err, size_t errlen, const char *fmt, ...) {
  if (!err || errlen == 0) return;
  va_list ap;
  va_start(ap, fmt);
  vsnprintf(err, errlen, fmt, ap);
  va_end(ap);
}

/* ------------------------------------------------------------------------ */
/* JSON (DOM)                                                               */
/* ------------------------------------------------------------------------ */

typedef enum { J_NULL, J_BOOL, J_NUM, J_STR, J_ARR, J_OBJ } jtype;

typedef struct jval {
  jtype type;
  double num;
  int b;
  char *str;
  struct jval *items;
  char **keys; /* object members only; parallel to items */
  size_t n, cap;
} jval;

typedef struct jparser {
  const char *p, *end;
  int bad;
} jparser;

static void jv_free(jval *v) {
  if (!v) return;
  free(v->str);
  for (size_t i = 0; i < v->n; i++) {
    if (v->keys) free(v->keys[i]);
    jv_free(&v->items[i]);
  }
  free(v->keys);
  free(v->items);
}

static void jv_destroy(jval *v) {
  jv_free(v);
  free(v);
}

static void jp_ws(jparser *j) {
  while (j->p < j->end && (*j->p == ' ' || *j->p == '\t' || *j->p == '\n' || *j->p == '\r')) j->p++;
}

static void utf8_put(char **buf, size_t *len, size_t *cap, uint32_t cp) {
  char tmp[4];
  int n;
  if (cp < 0x80) { tmp[0] = (char)cp; n = 1; }
  else if (cp < 0x800) { tmp[0] = (char)(0xC0 | (cp >> 6)); tmp[1] = (char)(0x80 | (cp & 0x3F)); n = 2; }
  else if (cp < 0x10000) {
    tmp[0] = (char)(0xE0 | (cp >> 12)); tmp[1] = (char)(0x80 | ((cp >> 6) & 0x3F));
    tmp[2] = (char)(0x80 | (cp & 0x3F)); n = 3;
  } else {
    tmp[0] = (char)(0xF0 | (cp >> 18)); tmp[1] = (char)(0x80 | ((cp >> 12) & 0x3F));
    tmp[2] = (char)(0x80 | ((cp >> 6) & 0x3F)); tmp[3] = (char)(0x80 | (cp & 0x3F)); n = 4;
  }
  if (*len + (size_t)n + 1 > *cap) {
    size_t nc = *cap ? *cap * 2 : 64;
    while (nc < *len + (size_t)n + 1) nc *= 2;
    char *nb = realloc(*buf, nc);
    if (!nb) return;
    *buf = nb;
    *cap = nc;
  }
  memcpy(*buf + *len, tmp, (size_t)n);
  *len += (size_t)n;
  (*buf)[*len] = 0;
}

static int jp_hex4(const char *p, uint32_t *v) {
  uint32_t x = 0;
  for (int i = 0; i < 4; i++) {
    char c = p[i];
    x <<= 4;
    if (c >= '0' && c <= '9') x |= (uint32_t)(c - '0');
    else if (c >= 'a' && c <= 'f') x |= (uint32_t)(c - 'a' + 10);
    else if (c >= 'A' && c <= 'F') x |= (uint32_t)(c - 'A' + 10);
    else return -1;
  }
  *v = x;
  return 0;
}

/* Parses a string starting at the opening quote. Returns malloc'd UTF-8. */
static char *jp_string(jparser *j) {
  if (j->p >= j->end || *j->p != '"') { j->bad = 1; return NULL; }
  j->p++;
  char *buf = NULL;
  size_t len = 0, cap = 0;
  while (j->p < j->end && *j->p != '"') {
    unsigned char c = (unsigned char)*j->p;
    if (c == '\\') {
      j->p++;
      if (j->p >= j->end) { j->bad = 1; break; }
      char e = *j->p++;
      switch (e) {
        case '"': utf8_put(&buf, &len, &cap, '"'); break;
        case '\\': utf8_put(&buf, &len, &cap, '\\'); break;
        case '/': utf8_put(&buf, &len, &cap, '/'); break;
        case 'b': utf8_put(&buf, &len, &cap, '\b'); break;
        case 'f': utf8_put(&buf, &len, &cap, '\f'); break;
        case 'n': utf8_put(&buf, &len, &cap, '\n'); break;
        case 'r': utf8_put(&buf, &len, &cap, '\r'); break;
        case 't': utf8_put(&buf, &len, &cap, '\t'); break;
        case 'u': {
          uint32_t cp;
          if (j->end - j->p < 4 || jp_hex4(j->p, &cp)) { j->bad = 1; break; }
          j->p += 4;
          if (cp >= 0xD800 && cp <= 0xDBFF && j->end - j->p >= 6 && j->p[0] == '\\' && j->p[1] == 'u') {
            uint32_t lo;
            if (!jp_hex4(j->p + 2, &lo) && lo >= 0xDC00 && lo <= 0xDFFF) {
              cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
              j->p += 6;
            }
          }
          utf8_put(&buf, &len, &cap, cp);
          break;
        }
        default: j->bad = 1; break;
      }
      if (j->bad) break;
    } else {
      /* Raw bytes (including UTF-8 multi-byte sequences) are copied unchanged. */
      if (len + 2 > cap) {
        size_t nc = cap ? cap * 2 : 64;
        char *nb = realloc(buf, nc);
        if (!nb) { j->bad = 1; break; }
        buf = nb;
        cap = nc;
      }
      buf[len++] = (char)c;
      buf[len] = 0;
      j->p++;
    }
  }
  if (j->p >= j->end || *j->p != '"') { j->bad = 1; free(buf); return NULL; }
  j->p++;
  if (!buf) buf = calloc(1, 1);
  return buf;
}

static jval *jp_value(jparser *j);

static int jv_push(jval *v, char *key, jval *item) {
  if (v->n == v->cap) {
    size_t nc = v->cap ? v->cap * 2 : 8;
    jval *ni = realloc(v->items, nc * sizeof(jval));
    if (!ni) return -1;
    v->items = ni;
    if (v->type == J_OBJ) {
      char **nk = realloc(v->keys, nc * sizeof(char *));
      if (!nk) return -1;
      v->keys = nk;
    }
    v->cap = nc;
  }
  if (v->keys) v->keys[v->n] = key;
  v->items[v->n++] = *item;
  return 0;
}

static jval *jp_value(jparser *j) {
  jp_ws(j);
  if (j->p >= j->end) { j->bad = 1; return NULL; }
  jval *v = calloc(1, sizeof *v);
  if (!v) { j->bad = 1; return NULL; }
  char c = *j->p;
  if (c == '{' || c == '[') {
    int obj = c == '{';
    v->type = obj ? J_OBJ : J_ARR;
    j->p++;
    jp_ws(j);
    if (j->p < j->end && *j->p == (obj ? '}' : ']')) { j->p++; return v; }
    for (;;) {
      char *key = NULL;
      if (obj) {
        jp_ws(j);
        key = jp_string(j);
        if (j->bad) { free(key); break; }
        jp_ws(j);
        if (j->p >= j->end || *j->p != ':') { free(key); j->bad = 1; break; }
        j->p++;
      }
      jval *item = jp_value(j);
      if (j->bad || !item) { free(key); free(item); j->bad = 1; break; }
      if (jv_push(v, key, item)) { j->bad = 1; free(item); break; }
      free(item);  /* the struct was copied into the parent array */
      jp_ws(j);
      if (j->p < j->end && *j->p == ',') { j->p++; continue; }
      if (j->p < j->end && *j->p == (obj ? '}' : ']')) { j->p++; break; }
      j->bad = 1;
      break;
    }
    return v;
  }
  if (c == '"') {
    v->type = J_STR;
    v->str = jp_string(j);
    if (j->bad) { v->str = NULL; }
    return v;
  }
  if (c == 't' && j->end - j->p >= 4 && !memcmp(j->p, "true", 4)) { j->p += 4; v->type = J_BOOL; v->b = 1; return v; }
  if (c == 'f' && j->end - j->p >= 5 && !memcmp(j->p, "false", 5)) { j->p += 5; v->type = J_BOOL; return v; }
  if (c == 'n' && j->end - j->p >= 4 && !memcmp(j->p, "null", 4)) { j->p += 4; v->type = J_NULL; return v; }
  if (c == '-' || (c >= '0' && c <= '9')) {
    char *endp = NULL;
    v->num = strtod(j->p, &endp);
    if (!endp || endp == j->p || endp > j->end) { j->bad = 1; return v; }
    j->p = endp;
    v->type = J_NUM;
    return v;
  }
  j->bad = 1;
  return v;
}

/* Parses a whole document. Returns NULL on error. */
static jval *json_parse(const char *text, size_t len) {
  jparser j = {text, text + len, 0};
  jval *v = jp_value(&j);
  jp_ws(&j);
  if (j.bad || j.p != j.end) {
    jv_destroy(v);
    return NULL;
  }
  return v;
}

static const jval *jv_get(const jval *obj, const char *key) {
  if (!obj || obj->type != J_OBJ) return NULL;
  for (size_t i = 0; i < obj->n; i++)
    if (strcmp(obj->keys[i], key) == 0) return &obj->items[i];
  return NULL;
}

static char *read_file(const char *path, size_t *len) {
  FILE *f = fopen(path, "rb");
  if (!f) return NULL;
  if (fseek(f, 0, SEEK_END) != 0) { fclose(f); return NULL; }
  long sz = ftell(f);
  rewind(f);
  if (sz < 0) { fclose(f); return NULL; }
  char *buf = malloc((size_t)sz + 1);
  if (!buf) { fclose(f); return NULL; }
  if (fread(buf, 1, (size_t)sz, f) != (size_t)sz) { fclose(f); free(buf); return NULL; }
  fclose(f);
  buf[sz] = 0;
  *len = (size_t)sz;
  return buf;
}

/* ------------------------------------------------------------------------ */
/* Vocabulary and tokenizer state                                           */
/* ------------------------------------------------------------------------ */

typedef struct added_tok {
  char *content;
  size_t clen;
  int64_t id;
} added_tok;

typedef struct tmpl_item {
  int is_seq;
  size_t n;
  int64_t ids[4];
} tmpl_item;

struct bert_tokenizer {
  char **vkeys;
  int32_t *vvals;
  size_t vcap;
  int32_t unk_id, pad_id;
  char *cont_prefix;
  size_t max_chars;
  int lowercase, strip_accents, clean_text, chinese;
  added_tok *added;
  size_t n_added;
  tmpl_item *tmpl;
  size_t n_tmpl;
};

static uint32_t bytes_hash(const char *s, size_t n) {
  uint32_t h = 2166136261u;
  for (size_t i = 0; i < n; i++) {
    h ^= (uint8_t)s[i];
    h *= 16777619u;
  }
  return h;
}

static int vocab_find(const bert_tokenizer *t, const char *s, size_t n) {
  size_t i = bytes_hash(s, n) & (t->vcap - 1);
  while (t->vkeys[i]) {
    if (strlen(t->vkeys[i]) == n && memcmp(t->vkeys[i], s, n) == 0) return t->vvals[i];
    i = (i + 1) & (t->vcap - 1);
  }
  return -1;
}

static int vocab_add(bert_tokenizer *t, const char *s, int32_t id) {
  size_t n = strlen(s);
  size_t i = bytes_hash(s, n) & (t->vcap - 1);
  while (t->vkeys[i]) {
    if (strcmp(t->vkeys[i], s) == 0) { t->vvals[i] = id; return 0; }
    i = (i + 1) & (t->vcap - 1);
  }
  t->vkeys[i] = malloc(n + 1);
  if (!t->vkeys[i]) return -1;
  memcpy(t->vkeys[i], s, n + 1);
  t->vvals[i] = id;
  return 0;
}

void bert_tokenizer_free(bert_tokenizer *t) {
  if (!t) return;
  if (t->vkeys)
    for (size_t i = 0; i < t->vcap; i++) free(t->vkeys[i]);
  free(t->vkeys);
  free(t->vvals);
  free(t->cont_prefix);
  for (size_t i = 0; i < t->n_added; i++) free(t->added[i].content);
  free(t->added);
  free(t->tmpl);
  free(t);
}

int32_t bert_tokenizer_pad_id(const bert_tokenizer *t) { return t->pad_id; }

static int jv_bool(const jval *v, int def) {
  return (v && v->type == J_BOOL) ? v->b : def;
}

int bert_tokenizer_load(const char *path, bert_tokenizer **out, char *err, size_t errlen) {
  *out = NULL;
  size_t len;
  char *text = read_file(path, &len);
  if (!text) FAIL("cannot read %s", path);
  jval *root = json_parse(text, len);
  free(text);
  if (!root) FAIL("%s: invalid JSON", path);

  bert_tokenizer *t = calloc(1, sizeof *t);
  if (!t) { jv_destroy(root); FAIL("out of memory"); }
  int rc = -1;

  const jval *model = jv_get(root, "model");
  const jval *mtype = jv_get(model, "type");
  const jval *vocab = jv_get(model, "vocab");
  if (!mtype || mtype->type != J_STR || strcmp(mtype->str, "WordPiece") != 0) {
    set_err(err, errlen, "tokenizer model is not WordPiece"); goto done;
  }
  if (!vocab || vocab->type != J_OBJ) { set_err(err, errlen, "tokenizer vocab missing"); goto done; }

  /* Vocabulary table. */
  t->vcap = 16;
  while (t->vcap < 2 * vocab->n) t->vcap <<= 1;
  t->vkeys = calloc(t->vcap, sizeof(char *));
  t->vvals = malloc(t->vcap * sizeof(int32_t));
  if (!t->vkeys || !t->vvals) { set_err(err, errlen, "out of memory"); goto done; }
  for (size_t i = 0; i < vocab->n; i++) {
    const jval *id = &vocab->items[i];
    if (id->type != J_NUM) { set_err(err, errlen, "vocab id not a number"); goto done; }
    if (vocab_add(t, vocab->keys[i], (int32_t)id->num)) { set_err(err, errlen, "out of memory"); goto done; }
  }

  const jval *unk = jv_get(model, "unk_token");
  const jval *pref = jv_get(model, "continuing_subword_prefix");
  const jval *maxc = jv_get(model, "max_input_chars_per_word");
  if (!unk || unk->type != J_STR) { set_err(err, errlen, "unk_token missing"); goto done; }
  if (!pref || pref->type != J_STR) { set_err(err, errlen, "continuing_subword_prefix missing"); goto done; }
  t->cont_prefix = malloc(strlen(pref->str) + 1);
  if (!t->cont_prefix) { set_err(err, errlen, "out of memory"); goto done; }
  strcpy(t->cont_prefix, pref->str);
  t->max_chars = (maxc && maxc->type == J_NUM) ? (size_t)maxc->num : 100;
  int unk_found = vocab_find(t, unk->str, strlen(unk->str));
  if (unk_found < 0) { set_err(err, errlen, "unk token not in vocab"); goto done; }
  t->unk_id = unk_found;
  int pad_found = vocab_find(t, "[PAD]", 5);
  t->pad_id = pad_found >= 0 ? pad_found : 0;

  /* Normalizer. */
  const jval *norm = jv_get(root, "normalizer");
  const jval *ntype = jv_get(norm, "type");
  if (!ntype || ntype->type != J_STR || strcmp(ntype->str, "BertNormalizer") != 0) {
    set_err(err, errlen, "only BertNormalizer is supported"); goto done;
  }
  t->clean_text = jv_bool(jv_get(norm, "clean_text"), 1);
  t->chinese = jv_bool(jv_get(norm, "handle_chinese_chars"), 1);
  t->lowercase = jv_bool(jv_get(norm, "lowercase"), 1);
  const jval *sa = jv_get(norm, "strip_accents");
  t->strip_accents = (sa && sa->type == J_BOOL) ? sa->b : t->lowercase;

  /* Pre-tokenizer. */
  const jval *pre = jv_get(root, "pre_tokenizer");
  const jval *ptype = jv_get(pre, "type");
  if (!ptype || ptype->type != J_STR || strcmp(ptype->str, "BertPreTokenizer") != 0) {
    set_err(err, errlen, "only BertPreTokenizer is supported"); goto done;
  }

  /* Added tokens (matched before normalization when not normalized). */
  const jval *added = jv_get(root, "added_tokens");
  if (added && added->type == J_ARR && added->n) {
    t->added = calloc(added->n, sizeof(added_tok));
    if (!t->added) { set_err(err, errlen, "out of memory"); goto done; }
    for (size_t i = 0; i < added->n; i++) {
      const jval *a = &added->items[i];
      const jval *content = jv_get(a, "content");
      const jval *aid = jv_get(a, "id");
      const jval *normalized = jv_get(a, "normalized");
      if (!content || content->type != J_STR || !aid || aid->type != J_NUM) continue;
      if (jv_bool(normalized, 0)) continue;
      added_tok *d = &t->added[t->n_added];
      d->content = malloc(strlen(content->str) + 1);
      if (!d->content) { set_err(err, errlen, "out of memory"); goto done; }
      strcpy(d->content, content->str);
      d->clen = strlen(content->str);
      d->id = (int64_t)aid->num;
      t->n_added++;
    }
  }

  /* Post-processor: single-sequence template. */
  const jval *post = jv_get(root, "post_processor");
  const jval *single = jv_get(post, "single");
  const jval *special = jv_get(post, "special_tokens");
  if (!single || single->type != J_ARR) { set_err(err, errlen, "post_processor.single missing"); goto done; }
  t->tmpl = calloc(single->n ? single->n : 1, sizeof(tmpl_item));
  if (!t->tmpl) { set_err(err, errlen, "out of memory"); goto done; }
  for (size_t i = 0; i < single->n; i++) {
    const jval *item = &single->items[i];
    const jval *st = jv_get(item, "SpecialToken");
    const jval *sq = jv_get(item, "Sequence");
    if (sq) {
      t->tmpl[t->n_tmpl].is_seq = 1;
      t->n_tmpl++;
    } else if (st) {
      const jval *sid = jv_get(st, "id");
      const jval *entry = (sid && sid->type == J_STR) ? jv_get(special, sid->str) : NULL;
      const jval *ids = jv_get(entry, "ids");
      if (!ids || ids->type != J_ARR || ids->n > 4) { set_err(err, errlen, "template special token unresolved"); goto done; }
      tmpl_item *ti = &t->tmpl[t->n_tmpl++];
      ti->n = ids->n;
      for (size_t k = 0; k < ids->n; k++) ti->ids[k] = (int64_t)ids->items[k].num;
    } else {
      set_err(err, errlen, "unsupported template item"); goto done;
    }
  }
  rc = 0;

done:
  jv_destroy(root);
  if (rc) bert_tokenizer_free(t);
  else *out = t;
  return rc;
}

/* ------------------------------------------------------------------------ */
/* Character classes and normalization                                      */
/* ------------------------------------------------------------------------ */

static int is_ws(uint32_t c) {
  return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == 0xA0 || c == 0x1680 ||
         (c >= 0x2000 && c <= 0x200A) || c == 0x202F || c == 0x205F || c == 0x3000;
}

static int is_control(uint32_t c) {
  if (c == '\t' || c == '\n' || c == '\r') return 0;
  if (c < 0x20 || (c >= 0x7F && c <= 0x9F)) return 1;      /* Cc */
  if (c == 0xAD || (c >= 0x200B && c <= 0x200F) || (c >= 0x202A && c <= 0x202E) ||
      (c >= 0x2060 && c <= 0x2064) || c == 0xFEFF)            /* Cf */
    return 1;
  return 0;
}

static int is_punct(uint32_t c) {
  if ((c >= 33 && c <= 47) || (c >= 58 && c <= 64) || (c >= 91 && c <= 96) || (c >= 123 && c <= 126))
    return 1;
  if (c == 0xA1 || c == 0xA7 || c == 0xAB || c == 0xB6 || c == 0xB7 || c == 0xBB || c == 0xBF) return 1;
  if ((c >= 0x2010 && c <= 0x2027) || (c >= 0x2030 && c <= 0x205E)) return 1;
  if ((c >= 0x3001 && c <= 0x3003) || (c >= 0x3008 && c <= 0x3011) || (c >= 0x3014 && c <= 0x301F))
    return 1;
  if (c >= 0xFF01 && c <= 0xFF0F) return 1;
  return 0;
}

static int is_cjk(uint32_t c) {
  return (c >= 0x4E00 && c <= 0x9FFF) || (c >= 0x3400 && c <= 0x4DBF) ||
         (c >= 0x20000 && c <= 0x2A6DF) || (c >= 0x2A700 && c <= 0x2B73F) ||
         (c >= 0x2B740 && c <= 0x2B81F) || (c >= 0x2B820 && c <= 0x2CEAF) ||
         (c >= 0xF900 && c <= 0xFAFF) || (c >= 0x2F800 && c <= 0x2FA1F);
}

static uint32_t lower_cp(uint32_t c) {
  if (c >= 'A' && c <= 'Z') return c + 32;
  if (c < 0x80) return c;
  if (c >= 0xC0 && c <= 0xDE && c != 0xD7) return c + 32;
  if (c >= 0x100 && c <= 0x17F) {
    if ((c >= 0x100 && c <= 0x137) || (c >= 0x14A && c <= 0x177)) return (c % 2 == 0) ? c + 1 : c;
    if ((c >= 0x139 && c <= 0x148) || (c >= 0x179 && c <= 0x17E)) return (c % 2 == 1) ? c + 1 : c;
    if (c == 0x178) return 0xFF;
    return c;
  }
  if (c >= 0x391 && c <= 0x3A9 && c != 0x3A2) return c + 32;
  if (c >= 0x410 && c <= 0x42F) return c + 32;
  if (c >= 0x400 && c <= 0x40F) return c + 80;
  return c;
}

/* Base letter of a lowercase Latin-1 letter (U+00E0..U+00FF), 0 if none. */
static uint32_t base_letter(uint32_t c) {
  static const char tbl[32] = {
    'a', 'a', 'a', 'a', 'a', 'a', 0, 'c', 'e', 'e', 'e', 'e', 'i', 'i', 'i', 'i',
    0, 'n', 'o', 'o', 'o', 'o', 'o', 0, 0, 'u', 'u', 'u', 'u', 'y', 0, 'y'};
  if (c >= 0xE0 && c <= 0xFF) return (uint32_t)(unsigned char)tbl[c - 0xE0];
  return 0;
}

static size_t utf8_decode_one(const char *s, size_t n, uint32_t *cp) {
  unsigned char c = (unsigned char)s[0];
  size_t need = c < 0x80 ? 1 : (c >> 5) == 6 ? 2 : (c >> 4) == 14 ? 3 : (c >> 3) == 30 ? 4 : 1;
  if (need > n) { *cp = 0xFFFD; return 1; }
  if (need == 1) {
    *cp = c >= 0x80 ? 0xFFFD : c;
    return 1;
  }
  uint32_t v = c & (0xFF >> (need + 1));
  for (size_t i = 1; i < need; i++) {
    unsigned char cc = (unsigned char)s[i];
    if ((cc & 0xC0) != 0x80) { *cp = 0xFFFD; return 1; }
    v = (v << 6) | (cc & 0x3F);
  }
  *cp = v;
  return need;
}

/* Growable array of code points. */
typedef struct cpbuf {
  uint32_t *v;
  size_t n, cap;
} cpbuf;

static int cp_push(cpbuf *b, uint32_t c) {
  if (b->n == b->cap) {
    size_t nc = b->cap ? b->cap * 2 : 64;
    uint32_t *nv = realloc(b->v, nc * sizeof(uint32_t));
    if (!nv) return -1;
    b->v = nv;
    b->cap = nc;
  }
  b->v[b->n++] = c;
  return 0;
}

/* Growable array of token ids. */
typedef struct idbuf {
  int64_t *v;
  size_t n, cap;
} idbuf;

static int id_push(idbuf *b, int64_t id) {
  if (b->n == b->cap) {
    size_t nc = b->cap ? b->cap * 2 : 64;
    int64_t *nv = realloc(b->v, nc * sizeof(int64_t));
    if (!nv) return -1;
    b->v = nv;
    b->cap = nc;
  }
  b->v[b->n++] = id;
  return 0;
}

/* Normalizes and pre-tokenizes one segment, appending WordPiece ids. */
static int encode_segment(const bert_tokenizer *t, const char *s, size_t n, idbuf *ids,
                          char *err, size_t errlen) {
  cpbuf cps = {0};
  /* Decode, clean, space CJK, lowercase, strip accents. */
  for (size_t i = 0; i < n;) {
    uint32_t c;
    i += utf8_decode_one(s + i, n - i, &c);
    if (t->clean_text) {
      if (c == 0 || c == 0xFFFD || is_control(c)) continue;
      if (is_ws(c)) c = ' ';
    }
    if (t->chinese && is_cjk(c)) {
      if (cp_push(&cps, ' ') || cp_push(&cps, c) || cp_push(&cps, ' ')) { free(cps.v); FAIL("out of memory"); }
      continue;
    }
    if (t->lowercase) c = lower_cp(c);
    if (t->strip_accents) {
      if (c >= 0x300 && c <= 0x36F) continue;   /* combining marks */
      uint32_t b = base_letter(c);
      if (b) c = b;
    }
    if (cp_push(&cps, c)) { free(cps.v); FAIL("out of memory"); }
  }

  /* Whitespace and punctuation split; WordPiece per word. */
  size_t wstart = 0, i = 0;
  char *buf = NULL;
  size_t bcap = 0;
  while (i <= cps.n) {
    int end_word = i == cps.n || cps.v[i] == ' ' || is_punct(cps.v[i]);
    if (end_word) {
      if (i > wstart) {
        size_t wlen = i - wstart;
        int bad = wlen > t->max_chars;
        size_t wpos = wstart;
        size_t first_id_count = ids->n;
        while (!bad && wpos < i) {
          size_t endp = i;
          int found = -1;
          size_t pos_len = 0;
          while (wpos < endp) {
            /* Build "##"? + UTF-8 of cps[wpos..endp). */
            size_t need = (wpos > wstart ? 2 : 0) + (endp - wpos) * 4 + 1;
            if (need > bcap) {
              char *nb = realloc(buf, need);
              if (!nb) { free(buf); free(cps.v); FAIL("out of memory"); }
              buf = nb;
              bcap = need;
            }
            pos_len = 0;
            if (wpos > wstart) {
              memcpy(buf, t->cont_prefix, strlen(t->cont_prefix));
              pos_len = strlen(t->cont_prefix);
            }
            for (size_t k = wpos; k < endp; k++) {
              uint32_t c = cps.v[k];
              if (c < 0x80) buf[pos_len++] = (char)c;
              else if (c < 0x800) { buf[pos_len++] = (char)(0xC0 | (c >> 6)); buf[pos_len++] = (char)(0x80 | (c & 0x3F)); }
              else if (c < 0x10000) {
                buf[pos_len++] = (char)(0xE0 | (c >> 12)); buf[pos_len++] = (char)(0x80 | ((c >> 6) & 0x3F));
                buf[pos_len++] = (char)(0x80 | (c & 0x3F));
              } else {
                buf[pos_len++] = (char)(0xF0 | (c >> 18)); buf[pos_len++] = (char)(0x80 | ((c >> 12) & 0x3F));
                buf[pos_len++] = (char)(0x80 | ((c >> 6) & 0x3F)); buf[pos_len++] = (char)(0x80 | (c & 0x3F));
              }
            }
            int id = vocab_find(t, buf, pos_len);
            if (id >= 0) { found = id; break; }
            endp--;
          }
          if (found < 0) { bad = 1; break; }
          if (id_push(ids, found)) { free(buf); free(cps.v); FAIL("out of memory"); }
          wpos = endp;
        }
        if (bad) {
          ids->n = first_id_count;
          if (id_push(ids, t->unk_id)) { free(buf); free(cps.v); FAIL("out of memory"); }
        }
      }
      if (i < cps.n && cps.v[i] != ' ') {
        /* Punctuation is its own word. */
        char pbuf[8];
        uint32_t c = cps.v[i];
        size_t pl = c < 0x80 ? 1 : c < 0x800 ? 2 : c < 0x10000 ? 3 : 4;
        if (pl == 1) pbuf[0] = (char)c;
        else if (pl == 2) { pbuf[0] = (char)(0xC0 | (c >> 6)); pbuf[1] = (char)(0x80 | (c & 0x3F)); }
        else if (pl == 3) { pbuf[0] = (char)(0xE0 | (c >> 12)); pbuf[1] = (char)(0x80 | ((c >> 6) & 0x3F)); pbuf[2] = (char)(0x80 | (c & 0x3F)); }
        else { pbuf[0] = (char)(0xF0 | (c >> 18)); pbuf[1] = (char)(0x80 | ((c >> 12) & 0x3F)); pbuf[2] = (char)(0x80 | ((c >> 6) & 0x3F)); pbuf[3] = (char)(0x80 | (c & 0x3F)); }
        int id = vocab_find(t, pbuf, pl);
        if (id_push(ids, id >= 0 ? id : t->unk_id)) { free(buf); free(cps.v); FAIL("out of memory"); }
      }
      wstart = i + 1;
    }
    i++;
  }
  free(buf);
  free(cps.v);
  return 0;
}

/* Encodes one text to token ids including the template specials. */
static int encode_text(const bert_tokenizer *t, const char *text, idbuf *out, char *err, size_t errlen) {
  idbuf body = {0};
  size_t len = strlen(text);
  size_t seg_start = 0, pos = 0;
  int ok = 0;
  while (pos < len) {
    int matched = 0;
    for (size_t a = 0; a < t->n_added; a++) {
      const added_tok *at = &t->added[a];
      if (at->clen && len - pos >= at->clen && memcmp(text + pos, at->content, at->clen) == 0) {
        if (encode_segment(t, text + seg_start, pos - seg_start, &body, err, errlen)) goto out;
        if (id_push(&body, at->id)) { set_err(err, errlen, "out of memory"); goto out; }
        pos += at->clen;
        seg_start = pos;
        matched = 1;
        break;
      }
    }
    if (!matched) {
      uint32_t c;
      pos += utf8_decode_one(text + pos, len - pos, &c);
    }
  }
  if (encode_segment(t, text + seg_start, len - seg_start, &body, err, errlen)) goto out;

  /* Template: specials and the sequence. */
  for (size_t k = 0; k < t->n_tmpl; k++) {
    const tmpl_item *ti = &t->tmpl[k];
    if (ti->is_seq) {
      for (size_t j = 0; j < body.n; j++)
        if (id_push(out, body.v[j])) { set_err(err, errlen, "out of memory"); goto out; }
    } else {
      for (size_t j = 0; j < ti->n; j++)
        if (id_push(out, ti->ids[j])) { set_err(err, errlen, "out of memory"); goto out; }
    }
  }
  ok = 1;
out:
  free(body.v);
  return ok ? 0 : -1;
}

int bert_tokenizer_encode_batch(const bert_tokenizer *t, const char *const *texts, size_t n,
                                int64_t **ids, int64_t **mask, size_t *seq_len,
                                char *err, size_t errlen) {
  *ids = NULL;
  *mask = NULL;
  *seq_len = 0;
  idbuf *seqs = calloc(n ? n : 1, sizeof(idbuf));
  if (!seqs) FAIL("out of memory");
  size_t L = 0;
  for (size_t b = 0; b < n; b++) {
    if (encode_text(t, texts[b], &seqs[b], err, errlen)) goto fail;
    if (seqs[b].n > L) L = seqs[b].n;
  }
  size_t cells = (n * L) != 0 ? n * L : 1;
  int64_t *oi = malloc(cells * sizeof(int64_t));
  int64_t *om = malloc(cells * sizeof(int64_t));
  if (!oi || !om) { free(oi); free(om); set_err(err, errlen, "out of memory"); goto fail; }
  for (size_t b = 0; b < n; b++) {
    for (size_t j = 0; j < L; j++) {
      size_t k = b * L + j;
      if (j < seqs[b].n) {
        oi[k] = seqs[b].v[j];
        om[k] = 1;
      } else {
        oi[k] = t->pad_id;
        om[k] = 0;
      }
    }
  }
  for (size_t b = 0; b < n; b++) free(seqs[b].v);
  free(seqs);
  *ids = oi;
  *mask = om;
  *seq_len = L;
  return 0;
fail:
  for (size_t b = 0; b < n; b++) free(seqs[b].v);
  free(seqs);
  return -1;
}

/* ------------------------------------------------------------------------ */
/* config.json id2label                                                     */
/* ------------------------------------------------------------------------ */

void bert_labels_free(bert_labels *l) {
  if (!l) return;
  for (size_t i = 0; i < l->n; i++) free(l->names[i]);
  free(l->names);
  l->names = NULL;
  l->n = 0;
}

int bert_load_id2label(const char *path, bert_labels *out, char *err, size_t errlen) {
  memset(out, 0, sizeof *out);
  size_t len;
  char *text = read_file(path, &len);
  if (!text) FAIL("cannot read %s", path);
  jval *root = json_parse(text, len);
  free(text);
  if (!root) FAIL("%s: invalid JSON", path);
  const jval *m = jv_get(root, "id2label");
  if (!m || m->type != J_OBJ || m->n == 0) { jv_destroy(root); FAIL("config.json has no id2label"); }
  size_t maxid = 0;
  for (size_t i = 0; i < m->n; i++) {
    long id = strtol(m->keys[i], NULL, 10);
    if (id < 0 || id > 100000) { jv_destroy(root); FAIL("bad label id"); }
    if ((size_t)id + 1 > maxid) maxid = (size_t)id + 1;
  }
  out->names = calloc(maxid, sizeof(char *));
  out->n = maxid;
  if (!out->names) { out->n = 0; jv_destroy(root); FAIL("out of memory"); }
  for (size_t i = 0; i < m->n; i++) {
    long id = strtol(m->keys[i], NULL, 10);
    const jval *v = &m->items[i];
    if (v->type != J_STR) continue;
    out->names[id] = malloc(strlen(v->str) + 1);
    if (out->names[id]) strcpy(out->names[id], v->str);
  }
  jv_destroy(root);
  return 0;
}
