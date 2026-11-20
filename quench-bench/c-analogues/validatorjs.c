// jetstream-row: validatorjs
// jetstream-files: validatorjs/src/test.mjs
// iterations: 120
/* C analogue of JetStream's validatorjs row (validator.js 13.15 test suite).
 *
 * The JS row runs about 10,000 assertions through the validator.js library, whose
 * checks are regular expressions run by the JS engine. This port replaces each
 * library check with a hand-written C check that accepts the same strings, for the
 * validators and options listed below. The input strings and expected results are
 * taken from the test suite's valid/invalid lists: every ASCII-only literal for
 * these validators, with the option sets the port implements. Literals that are
 * built with repeat() or template expressions, that contain non-ASCII characters,
 * or that need locales, other options, or other validators are not included. That
 * leaves 776 assertions per run instead of 10,114, so the count is not the library's.
 *
 * Validators: isEmail (default options), isInt (allow_leading_zeroes, min, max),
 * isFloat (min, max), isAlpha, isAlphanumeric, isNumeric (no_symbols), isUUID
 * (version), isHexColor, isBoolean (loose), isEmpty (ignore_whitespace), isLength
 * and isByteLength (min, max), isIP (version), isJSON (objects and arrays only),
 * isMACAddress (eui, no_separators), isBase64 (urlSafe, padding) and isFQDN
 * (default options).
 *
 * main() runs all cases for ITERATIONS passes, prints the elapsed milliseconds and
 * the assertion count, and exits 1 when a case gives the wrong answer. */
#define _POSIX_C_SOURCE 200809L
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#pragma STDC FP_CONTRACT OFF

#define ITERATIONS 120

typedef struct {
  int has_min, has_max;
  double min, max;
  int version;      /* isUUID, isIP: 0 means any */
  int no_lead0;     /* isInt: allow_leading_zeroes is false */
  int no_symbols;   /* isNumeric */
  int loose;        /* isBoolean */
  int ignore_ws;    /* isEmpty */
  int eui;          /* isMACAddress: 0 means 48 or 64 */
  int no_sep;       /* isMACAddress */
  int url_safe;     /* isBase64 */
  int has_padding;  /* isBase64: padding given explicitly */
  int padding;
} Opts;

enum {
  F_EMAIL, F_INT, F_FLOAT, F_ALPHA, F_ALPHANUMERIC, F_NUMERIC, F_UUID, F_HEXCOLOR,
  F_BOOLEAN, F_EMPTY, F_LENGTH, F_BYTELENGTH, F_IP, F_JSON, F_MACADDRESS, F_BASE64,
  F_FQDN
};

typedef struct {
  int fn;
  Opts o;
  int expect;
  const char *s;
} Case;

static int failures;

/* ---- character classes (ASCII only, no locale) ---- */

static int is_digit(int c) { return c >= '0' && c <= '9'; }
static int is_upper(int c) { return c >= 'A' && c <= 'Z'; }
static int is_lower(int c) { return c >= 'a' && c <= 'z'; }
static int is_alpha_c(int c) { return is_upper(c) || is_lower(c); }
static int is_alnum_c(int c) { return is_alpha_c(c) || is_digit(c); }
static int is_hex(int c) { return is_digit(c) || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F'); }
static int is_space_c(int c) { return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\v' || c == '\f'; }

/* ---- shared pieces ---- */

/* Dotted-quad IPv4: four decimal parts, each 0..255, no leading zeros. */
static int ipv4_ok(const char *s, size_t n) {
  size_t i = 0;
  for (int part = 0; part < 4; part++) {
    size_t st = i;
    int v = 0;
    while (i < n && is_digit((unsigned char)s[i])) {
      v = v * 10 + (s[i] - '0');
      i++;
      if (i - st > 3) return 0;
    }
    if (i == st || v > 255) return 0;
    if (i - st > 1 && s[st] == '0') return 0;
    if (part < 3) {
      if (i >= n || s[i] != '.') return 0;
      i++;
    }
  }
  return i == n;
}

/* IPv6 (RFC 4291 text form): hex groups with at most one "::", an optional
 * dotted-quad tail counted as two groups, and an optional %zone of letters and
 * digits. */
static int ipv6_ok(const char *s, size_t n) {
  size_t i, body_len;
  int extra = 0, explicit_groups = 0, dbl = 0;
  char buf[64];
  for (i = 0; i < n; i++)
    if (s[i] == '%') break;
  if (i < n) {
    size_t k;
    if (i + 1 >= n) return 0;
    for (k = i + 1; k < n; k++)
      if (!is_alnum_c((unsigned char)s[k])) return 0;
    n = i;
  }
  if (n == 0) return 0;
  body_len = n;
  if (memchr(s, '.', n)) {
    size_t start = n;
    while (start > 0 && s[start - 1] != ':') start--;
    if (start == 0) return 0;
    if (!ipv4_ok(s + start, n - start)) return 0;
    /* replace the dotted quad by one "0" group, then count it as two groups */
    if (start + 1 > sizeof buf) return 0;
    memcpy(buf, s, start);
    buf[start] = '0';
    body_len = start + 1;
    extra = 1;
    s = buf;
  }
  if (body_len == 0) return 0;
  {
    size_t p = 0;
    if (s[0] == ':') {
      if (body_len < 2 || s[1] != ':') return 0;
      dbl = 1;
      p = 2;
    }
    while (p < body_len) {
      size_t st = p;
      while (p < body_len && is_hex((unsigned char)s[p])) p++;
      if (p == st || p - st > 4) return 0;
      explicit_groups++;
      if (p == body_len) break;
      if (s[p] != ':') return 0;
      p++;
      if (p < body_len && s[p] == ':') {
        if (dbl) return 0;
        dbl = 1;
        p++;
      } else if (p == body_len) {
        return 0;
      }
    }
  }
  {
    int total = explicit_groups + extra;
    return dbl ? total <= 7 : total == 8;
  }
}

/* Fully qualified domain name, default options: at least two labels, letters
 * and digits and inner hyphens, at most 63 characters a label, a letters-only
 * TLD of at least two characters, no trailing dot, no underscore. */
static int fqdn_ok(const char *s, size_t n) {
  size_t i = 0, last_st = 0, labels = 0;
  if (n == 0 || n > 253) return 0;
  for (;;) {
    size_t st = i;
    while (i < n && s[i] != '.') i++;
    size_t len = i - st;
    if (len == 0 || len > 63) return 0;
    if (s[st] == '-' || s[i - 1] == '-') return 0;
    for (size_t k = st; k < i; k++)
      if (!is_alnum_c((unsigned char)s[k]) && s[k] != '-') return 0;
    labels++;
    last_st = st;
    if (i == n) break;
    i++; /* skip the dot; a trailing dot leaves an empty label and fails above */
  }
  if (labels < 2) return 0;
  for (size_t k = last_st; k < n; k++)
    if (!is_alpha_c((unsigned char)s[k])) return 0;
  return n - last_st >= 2;
}

/* ---- validators ---- */

static int v_email(const char *s, size_t n) {
  size_t at;
  if (n == 0 || n > 254) return 0;
  if (s[0] == '"') {
    size_t i = 1;
    int closed = 0;
    while (i < n) {
      unsigned char c = (unsigned char)s[i];
      if (c == '\\') {
        if (i + 1 >= n || (unsigned char)s[i + 1] < 32 || (unsigned char)s[i + 1] > 126) return 0;
        i += 2;
        continue;
      }
      if (c == '"') {
        closed = 1;
        break;
      }
      if (c < 32 || c > 126) return 0;
      i++;
    }
    if (!closed || i + 1 >= n || s[i + 1] != '@') return 0;
    at = i + 1;
  } else {
    size_t i = 0;
    int prev_dot = 1; /* a leading dot is not allowed */
    while (i < n && s[i] != '@') {
      unsigned char c = (unsigned char)s[i];
      if (c == '.') {
        if (prev_dot) return 0;
        prev_dot = 1;
      } else if (is_alnum_c(c) || (c != 0 && strchr("!#$%&'*+/=\?^_`{|}~-", c))) {
        prev_dot = 0;
      } else {
        return 0;
      }
      i++;
    }
    if (i == n || prev_dot) return 0; /* no '@', or a trailing dot */
    at = i;
  }
  if (at == 0 || at > 64) return 0; /* local part is 1..64 characters */
  if (at + 1 >= n) return 0;
  {
    const char *dom = s + at + 1;
    size_t dn = n - at - 1;
    size_t last = dn;
    while (last > 0 && dom[last - 1] != '.') last--;
    if (dn - last < 2) return 0; /* the TLD needs two letters */
    return fqdn_ok(dom, dn);
  }
}

static int v_int(const char *s, size_t n, const Opts *o) {
  size_t i = 0;
  double v;
  if (i < n && (s[i] == '+' || s[i] == '-')) i++;
  if (i == n) return 0;
  for (size_t k = i; k < n; k++)
    if (!is_digit((unsigned char)s[k])) return 0;
  if (o->no_lead0 && n - i > 1 && s[i] == '0') return 0;
  v = strtod(s, NULL);
  if (o->has_min && v < o->min) return 0;
  if (o->has_max && v > o->max) return 0;
  return 1;
}

/* Float grammar of validator.js: optional sign, digits, optional fraction,
 * optional exponent; at least one digit in the mantissa. */
static int v_float(const char *s, size_t n, const Opts *o) {
  size_t i = 0, digits = 0;
  if (n == 0) return 0;
  if (s[i] == '+' || s[i] == '-') i++;
  while (i < n && is_digit((unsigned char)s[i])) {
    i++;
    digits++;
  }
  if (i < n && s[i] == '.') {
    i++;
    while (i < n && is_digit((unsigned char)s[i])) {
      i++;
      digits++;
    }
  }
  if (digits == 0) return 0;
  if (i < n && (s[i] == 'e' || s[i] == 'E')) {
    i++;
    if (i < n && (s[i] == '+' || s[i] == '-')) i++;
    if (i == n || !is_digit((unsigned char)s[i])) return 0;
    while (i < n && is_digit((unsigned char)s[i])) i++;
  }
  if (i != n) return 0;
  {
    double v = strtod(s, NULL);
    if (o->has_min && v < o->min) return 0;
    if (o->has_max && v > o->max) return 0;
  }
  return 1;
}

static int v_alpha(const char *s, size_t n, int with_digits) {
  if (n == 0) return 0;
  for (size_t i = 0; i < n; i++) {
    unsigned char c = (unsigned char)s[i];
    if (!(is_alpha_c(c) || (with_digits && is_digit(c)))) return 0;
  }
  return 1;
}

static int v_numeric(const char *s, size_t n, const Opts *o) {
  size_t i = 0, before = 0, after = 0;
  if (o->no_symbols) {
    if (n == 0) return 0;
    for (size_t k = 0; k < n; k++)
      if (!is_digit((unsigned char)s[k])) return 0;
    return 1;
  }
  if (i < n && (s[i] == '+' || s[i] == '-')) i++;
  while (i < n && is_digit((unsigned char)s[i])) {
    i++;
    before++;
  }
  if (i < n && s[i] == '.') {
    i++;
    while (i < n && is_digit((unsigned char)s[i])) {
      i++;
      after++;
    }
    /* "([0-9]*[.])?[0-9]+": a dot must be followed by at least one digit */
    return after > 0 && i == n;
  }
  return before > 0 && i == n;
}

static int v_uuid(const char *s, size_t n, const Opts *o) {
  if (n != 36) return 0;
  for (size_t i = 0; i < 36; i++) {
    int want_dash = (i == 8 || i == 13 || i == 18 || i == 23);
    if (want_dash) {
      if (s[i] != '-') return 0;
    } else if (!is_hex((unsigned char)s[i])) {
      return 0;
    }
  }
  {
    int v = s[14] - '0';
    int variant = s[19];
    if (o->version == 0) {
      if (v < 1 || v > 8) return 0;
    } else if (v != o->version) {
      return 0;
    }
    return variant == '8' || variant == '9' || variant == 'a' || variant == 'b' ||
           variant == 'A' || variant == 'B';
  }
}

static int v_hexcolor(const char *s, size_t n) {
  size_t i = 0;
  if (i < n && s[i] == '#') i++;
  size_t len = n - i;
  if (!(len == 3 || len == 4 || len == 6 || len == 8)) return 0;
  for (; i < n; i++)
    if (!is_hex((unsigned char)s[i])) return 0;
  return 1;
}

static int v_boolean(const char *s, size_t n, const Opts *o) {
  static const char *strict[] = {"true", "false", "1", "0"};
  static const char *loose[] = {"true", "false", "1", "0", "yes", "no"};
  size_t count = o->loose ? 6 : 4;
  for (size_t k = 0; k < count; k++) {
    const char *w = o->loose ? loose[k] : strict[k];
    if (strlen(w) != n) continue;
    size_t j;
    for (j = 0; j < n; j++) {
      unsigned char c = (unsigned char)s[j];
      if (o->loose && is_upper(c)) c = (unsigned char)(c + 32);
      if (c != (unsigned char)w[j]) break;
    }
    if (j == n) return 1;
  }
  return 0;
}

static int v_empty(const char *s, size_t n, const Opts *o) {
  if (!o->ignore_ws) return n == 0;
  for (size_t i = 0; i < n; i++)
    if (!is_space_c((unsigned char)s[i])) return 0;
  return 1;
}

/* Length in code units, which equals the byte count for these ASCII inputs. */
static int v_length(size_t n, const Opts *o) {
  double len = (double)n;
  if (o->has_min && len < o->min) return 0;
  if (o->has_max && len > o->max) return 0;
  return 1;
}

static int v_ip(const char *s, size_t n, const Opts *o) {
  if (o->version == 4) return ipv4_ok(s, n);
  if (o->version == 6) return ipv6_ok(s, n);
  return ipv4_ok(s, n) || ipv6_ok(s, n);
}

/* Recursive-descent JSON syntax check. Only object and array top-level values
 * are accepted, as validator.js does when allow_primitives is false. */
static int jp_value(const char *s, size_t n, size_t *i, int depth);

static void jp_ws(const char *s, size_t n, size_t *i) {
  while (*i < n && (s[*i] == ' ' || s[*i] == '\t' || s[*i] == '\n' || s[*i] == '\r')) (*i)++;
}

static int jp_string(const char *s, size_t n, size_t *i) {
  if (*i >= n || s[*i] != '"') return 0;
  (*i)++;
  while (*i < n) {
    unsigned char c = (unsigned char)s[*i];
    if (c == '"') {
      (*i)++;
      return 1;
    }
    if (c < 32) return 0;
    if (c == '\\') {
      if (*i + 1 >= n) return 0;
      char e = s[*i + 1];
      if (e == 'u') {
        if (*i + 6 > n) return 0;
        for (size_t k = 2; k < 6; k++)
          if (!is_hex((unsigned char)s[*i + k])) return 0;
        *i += 6;
        continue;
      }
      if (e == '\0' || !strchr("\"\\/bfnrt", e)) return 0;
      *i += 2;
      continue;
    }
    (*i)++;
  }
  return 0;
}

static int jp_number(const char *s, size_t n, size_t *i) {
  size_t st = *i;
  if (*i < n && s[*i] == '-') (*i)++;
  if (*i < n && s[*i] == '0') {
    (*i)++;
  } else if (*i < n && s[*i] >= '1' && s[*i] <= '9') {
    while (*i < n && is_digit((unsigned char)s[*i])) (*i)++;
  } else {
    return 0;
  }
  if (*i < n && s[*i] == '.') {
    (*i)++;
    if (*i >= n || !is_digit((unsigned char)s[*i])) return 0;
    while (*i < n && is_digit((unsigned char)s[*i])) (*i)++;
  }
  if (*i < n && (s[*i] == 'e' || s[*i] == 'E')) {
    (*i)++;
    if (*i < n && (s[*i] == '+' || s[*i] == '-')) (*i)++;
    if (*i >= n || !is_digit((unsigned char)s[*i])) return 0;
    while (*i < n && is_digit((unsigned char)s[*i])) (*i)++;
  }
  return *i > st;
}

static int jp_literal(const char *s, size_t n, size_t *i, const char *word) {
  size_t wl = strlen(word);
  if (*i + wl > n || memcmp(s + *i, word, wl) != 0) return 0;
  *i += wl;
  return 1;
}

static int jp_value(const char *s, size_t n, size_t *i, int depth) {
  if (depth > 256) return 0;
  jp_ws(s, n, i);
  if (*i >= n) return 0;
  switch (s[*i]) {
    case '{': {
      (*i)++;
      jp_ws(s, n, i);
      if (*i < n && s[*i] == '}') {
        (*i)++;
        return 1;
      }
      for (;;) {
        jp_ws(s, n, i);
        if (!jp_string(s, n, i)) return 0;
        jp_ws(s, n, i);
        if (*i >= n || s[*i] != ':') return 0;
        (*i)++;
        if (!jp_value(s, n, i, depth + 1)) return 0;
        jp_ws(s, n, i);
        if (*i < n && s[*i] == ',') {
          (*i)++;
          continue;
        }
        if (*i < n && s[*i] == '}') {
          (*i)++;
          return 1;
        }
        return 0;
      }
    }
    case '[': {
      (*i)++;
      jp_ws(s, n, i);
      if (*i < n && s[*i] == ']') {
        (*i)++;
        return 1;
      }
      for (;;) {
        if (!jp_value(s, n, i, depth + 1)) return 0;
        jp_ws(s, n, i);
        if (*i < n && s[*i] == ',') {
          (*i)++;
          continue;
        }
        if (*i < n && s[*i] == ']') {
          (*i)++;
          return 1;
        }
        return 0;
      }
    }
    case '"':
      return jp_string(s, n, i);
    case 't':
      return jp_literal(s, n, i, "true");
    case 'f':
      return jp_literal(s, n, i, "false");
    case 'n':
      return jp_literal(s, n, i, "null");
    default:
      return jp_number(s, n, i);
  }
}

static int v_json(const char *s, size_t n) {
  size_t i = 0;
  if (n == 0 || (s[0] != '{' && s[0] != '[')) return 0;
  if (!jp_value(s, n, &i, 0)) return 0;
  jp_ws(s, n, &i);
  return i == n;
}

/* MAC address: 6 or 8 two-digit groups separated by one of ":", "-" or " ", or
 * 3 or 4 four-digit groups separated by "."; no_separators gives 12 or 16 digits.
 * The separator must be the same throughout. */
static int v_mac(const char *s, size_t n, const Opts *o) {
  static const int sizes[2] = {48, 64};
  static const size_t octets[2] = {6, 8};
  int ok_any = 0;
  for (int e = 0; e < 2; e++) {
    size_t oc = octets[e];
    if (o->eui && o->eui != sizes[e]) continue;
    if (o->no_sep) {
      if (n != oc * 2) continue;
      int good = 1;
      for (size_t k = 0; k < n && good; k++) good = is_hex((unsigned char)s[k]);
      if (good) ok_any = 1;
      continue;
    }
    /* two-digit groups with one separator */
    if (n == oc * 3 - 1 && (s[2] == ':' || s[2] == '-' || s[2] == ' ')) {
      int good = 1;
      for (size_t g = 0; g < oc && good; g++) {
        size_t p = g * 3;
        if (g > 0 && s[p - 1] != s[2]) good = 0;
        if (!is_hex((unsigned char)s[p]) || !is_hex((unsigned char)s[p + 1])) good = 0;
      }
      if (good) ok_any = 1;
    }
    /* four-digit groups separated by dots */
    {
      size_t quads = oc / 2;
      if (n == quads * 5 - 1) {
        int good = 1;
        for (size_t g = 0; g < quads && good; g++) {
          size_t p = g * 5;
          if (g > 0 && s[p - 1] != '.') good = 0;
          for (size_t k = 0; k < 4 && good; k++)
            if (!is_hex((unsigned char)s[p + k])) good = 0;
        }
        if (good) ok_any = 1;
      }
    }
  }
  return ok_any;
}

/* Base64. The standard alphabet uses + and /, urlSafe uses - and _. Padding:
 * an explicit padding true requires '=' to complete a group of four; false forbids
 * '='; with no padding option the default is required for the standard alphabet
 * and optional for urlSafe. */
static int v_base64(const char *s, size_t n, const Opts *o) {
  enum { REQUIRED, FORBIDDEN, OPTIONAL };
  int mode;
  size_t data = 0, pads;
  if (o->has_padding) {
    mode = o->padding ? REQUIRED : FORBIDDEN;
  } else {
    mode = o->url_safe ? OPTIONAL : REQUIRED;
  }
  while (data < n && s[data] != '=') {
    unsigned char c = (unsigned char)s[data];
    int ok = is_alnum_c(c) || (o->url_safe ? (c == '-' || c == '_') : (c == '+' || c == '/'));
    if (!ok) return 0;
    data++;
  }
  pads = n - data;
  for (size_t k = data; k < n; k++)
    if (s[k] != '=') return 0;
  if (pads > 0) {
    if (mode == FORBIDDEN || pads > 2) return 0;
    return data % 4 == 4 - pads;
  }
  if (mode == REQUIRED) return data % 4 == 0;
  return data % 4 != 1;
}

static int check(const Case *c) {
  const char *s = c->s;
  size_t n = strlen(s);
  switch (c->fn) {
    case F_EMAIL: return v_email(s, n);
    case F_INT: return v_int(s, n, &c->o);
    case F_FLOAT: return v_float(s, n, &c->o);
    case F_ALPHA: return v_alpha(s, n, 0);
    case F_ALPHANUMERIC: return v_alpha(s, n, 1);
    case F_NUMERIC: return v_numeric(s, n, &c->o);
    case F_UUID: return v_uuid(s, n, &c->o);
    case F_HEXCOLOR: return v_hexcolor(s, n);
    case F_BOOLEAN: return v_boolean(s, n, &c->o);
    case F_EMPTY: return v_empty(s, n, &c->o);
    case F_LENGTH: return v_length(n, &c->o);
    case F_BYTELENGTH: return v_length(n, &c->o);
    case F_IP: return v_ip(s, n, &c->o);
    case F_JSON: return v_json(s, n);
    case F_MACADDRESS: return v_mac(s, n, &c->o);
    case F_BASE64: return v_base64(s, n, &c->o);
    case F_FQDN: return fqdn_ok(s, n);
    default: return 0;
  }
}

static double now_ms(void) {
  struct timespec ts;
  clock_gettime(CLOCK_MONOTONIC, &ts);
  return (double)ts.tv_sec * 1e3 + (double)ts.tv_nsec / 1e6;
}

/* ---- Cases, extracted from the test suite (see the header comment) ---- */

/* Generated by the vector extractor from validator.js test/validators.test.js. */
static const Case CASES[] = {
  {F_EMAIL, {}, 1, "foo@bar.com"},
  {F_EMAIL, {}, 1, "x@x.au"},
  {F_EMAIL, {}, 1, "foo@bar.com.au"},
  {F_EMAIL, {}, 1, "foo+bar@bar.com"},
  {F_EMAIL, {}, 1, "test123+ext@gmail.com"},
  {F_EMAIL, {}, 1, "some.name.midd.leNa.me.and.locality+extension@GoogleMail.com"},
  {F_EMAIL, {}, 1, "\"foobar\"@example.com"},
  {F_EMAIL, {}, 1, "\"foo\\@bar\"@example.com"},
  {F_EMAIL, {}, 1, "test@gmail.com"},
  {F_EMAIL, {}, 1, "test.1@gmail.com"},
  {F_EMAIL, {}, 1, "test@1337.com"},
  {F_EMAIL, {}, 0, "invalidemail@"},
  {F_EMAIL, {}, 0, "invalid.com"},
  {F_EMAIL, {}, 0, "@invalid.com"},
  {F_EMAIL, {}, 0, "foo@bar.com."},
  {F_EMAIL, {}, 0, "foo@_bar.com"},
  {F_EMAIL, {}, 0, "foo@bar.co.uk."},
  {F_EMAIL, {}, 0, "z@co.c"},
  {F_EMAIL, {}, 0, "multiple..dots@stillinvalid.com"},
  {F_EMAIL, {}, 0, "test123+invalid! sub_address@gmail.com"},
  {F_EMAIL, {}, 0, "gmail...ignores...dots...@gmail.com"},
  {F_EMAIL, {}, 0, "ends.with.dot.@gmail.com"},
  {F_EMAIL, {}, 0, "multiple..dots@gmail.com"},
  {F_EMAIL, {}, 0, "wrong()[]\",:;<>@@gmail.com"},
  {F_EMAIL, {}, 0, "\"wrong()[]\",:;<>@@gmail.com"},
  {F_EMAIL, {}, 0, "\"foobar@gmail.com"},
  {F_EMAIL, {}, 0, "\"foo\"bar@gmail.com"},
  {F_EMAIL, {}, 0, "foo\"bar\"@gmail.com"},
  {F_MACADDRESS, {}, 1, "ab:ab:ab:ab:ab:ab"},
  {F_MACADDRESS, {}, 1, "FF:FF:FF:FF:FF:FF"},
  {F_MACADDRESS, {}, 1, "01:02:03:04:05:ab"},
  {F_MACADDRESS, {}, 1, "01:AB:03:04:05:06"},
  {F_MACADDRESS, {}, 1, "A9 C5 D4 9F EB D3"},
  {F_MACADDRESS, {}, 1, "01 02 03 04 05 ab"},
  {F_MACADDRESS, {}, 1, "01-02-03-04-05-ab"},
  {F_MACADDRESS, {}, 1, "0102.0304.05ab"},
  {F_MACADDRESS, {}, 1, "ab:ab:ab:ab:ab:ab:ab:ab"},
  {F_MACADDRESS, {}, 1, "FF:FF:FF:FF:FF:FF:FF:FF"},
  {F_MACADDRESS, {}, 1, "01:02:03:04:05:06:07:ab"},
  {F_MACADDRESS, {}, 1, "01:AB:03:04:05:06:07:08"},
  {F_MACADDRESS, {}, 1, "A9 C5 D4 9F EB D3 B6 65"},
  {F_MACADDRESS, {}, 1, "01 02 03 04 05 06 07 ab"},
  {F_MACADDRESS, {}, 1, "01-02-03-04-05-06-07-ab"},
  {F_MACADDRESS, {}, 1, "0102.0304.0506.07ab"},
  {F_MACADDRESS, {}, 0, "abc"},
  {F_MACADDRESS, {}, 0, "01:02:03:04:05"},
  {F_MACADDRESS, {}, 0, "01:02:03:04:05:z0"},
  {F_MACADDRESS, {}, 0, "01:02:03:04::ab"},
  {F_MACADDRESS, {}, 0, "1:2:3:4:5:6"},
  {F_MACADDRESS, {}, 0, "AB:CD:EF:GH:01:02"},
  {F_MACADDRESS, {}, 0, "A9C5 D4 9F EB D3"},
  {F_MACADDRESS, {}, 0, "01-02 03:04 05 ab"},
  {F_MACADDRESS, {}, 0, "0102.03:04.05ab"},
  {F_MACADDRESS, {}, 0, "900f/dffs/sdea"},
  {F_MACADDRESS, {}, 0, "01:02:03:04:05:06:07"},
  {F_MACADDRESS, {}, 0, "01:02:03:04:05:06:07:z0"},
  {F_MACADDRESS, {}, 0, "01:02:03:04:05:06::ab"},
  {F_MACADDRESS, {}, 0, "1:2:3:4:5:6:7:8"},
  {F_MACADDRESS, {}, 0, "AB:CD:EF:GH:01:02:03:04"},
  {F_MACADDRESS, {}, 0, "A9C5 D4 9F EB D3 B6 65"},
  {F_MACADDRESS, {}, 0, "01-02 03:04 05 06 07 ab"},
  {F_MACADDRESS, {}, 0, "0102.03:04.0506.07ab"},
  {F_MACADDRESS, {}, 0, "900f/dffs/sdea/54gh"},
  {F_MACADDRESS, {.eui = 48}, 1, "ab:ab:ab:ab:ab:ab"},
  {F_MACADDRESS, {.eui = 48}, 1, "FF:FF:FF:FF:FF:FF"},
  {F_MACADDRESS, {.eui = 48}, 1, "01:02:03:04:05:ab"},
  {F_MACADDRESS, {.eui = 48}, 1, "01:AB:03:04:05:06"},
  {F_MACADDRESS, {.eui = 48}, 1, "A9 C5 D4 9F EB D3"},
  {F_MACADDRESS, {.eui = 48}, 1, "01 02 03 04 05 ab"},
  {F_MACADDRESS, {.eui = 48}, 1, "01-02-03-04-05-ab"},
  {F_MACADDRESS, {.eui = 48}, 1, "0102.0304.05ab"},
  {F_MACADDRESS, {.eui = 48}, 0, "ab:ab:ab:ab:ab:ab:ab:ab"},
  {F_MACADDRESS, {.eui = 48}, 0, "FF:FF:FF:FF:FF:FF:FF:FF"},
  {F_MACADDRESS, {.eui = 48}, 0, "01:02:03:04:05:06:07:ab"},
  {F_MACADDRESS, {.eui = 48}, 0, "01:AB:03:04:05:06:07:08"},
  {F_MACADDRESS, {.eui = 48}, 0, "A9 C5 D4 9F EB D3 B6 65"},
  {F_MACADDRESS, {.eui = 48}, 0, "01 02 03 04 05 06 07 ab"},
  {F_MACADDRESS, {.eui = 48}, 0, "01-02-03-04-05-06-07-ab"},
  {F_MACADDRESS, {.eui = 48}, 0, "0102.0304.0506.07ab"},
  {F_MACADDRESS, {.eui = 64}, 1, "ab:ab:ab:ab:ab:ab:ab:ab"},
  {F_MACADDRESS, {.eui = 64}, 1, "FF:FF:FF:FF:FF:FF:FF:FF"},
  {F_MACADDRESS, {.eui = 64}, 1, "01:02:03:04:05:06:07:ab"},
  {F_MACADDRESS, {.eui = 64}, 1, "01:AB:03:04:05:06:07:08"},
  {F_MACADDRESS, {.eui = 64}, 1, "A9 C5 D4 9F EB D3 B6 65"},
  {F_MACADDRESS, {.eui = 64}, 1, "01 02 03 04 05 06 07 ab"},
  {F_MACADDRESS, {.eui = 64}, 1, "01-02-03-04-05-06-07-ab"},
  {F_MACADDRESS, {.eui = 64}, 1, "0102.0304.0506.07ab"},
  {F_MACADDRESS, {.eui = 64}, 0, "ab:ab:ab:ab:ab:ab"},
  {F_MACADDRESS, {.eui = 64}, 0, "FF:FF:FF:FF:FF:FF"},
  {F_MACADDRESS, {.eui = 64}, 0, "01:02:03:04:05:ab"},
  {F_MACADDRESS, {.eui = 64}, 0, "01:AB:03:04:05:06"},
  {F_MACADDRESS, {.eui = 64}, 0, "A9 C5 D4 9F EB D3"},
  {F_MACADDRESS, {.eui = 64}, 0, "01 02 03 04 05 ab"},
  {F_MACADDRESS, {.eui = 64}, 0, "01-02-03-04-05-ab"},
  {F_MACADDRESS, {.eui = 64}, 0, "0102.0304.05ab"},
  {F_MACADDRESS, {.no_sep = 1}, 1, "abababababab"},
  {F_MACADDRESS, {.no_sep = 1}, 1, "FFFFFFFFFFFF"},
  {F_MACADDRESS, {.no_sep = 1}, 1, "0102030405ab"},
  {F_MACADDRESS, {.no_sep = 1}, 1, "01AB03040506"},
  {F_MACADDRESS, {.no_sep = 1}, 1, "abababababababab"},
  {F_MACADDRESS, {.no_sep = 1}, 1, "FFFFFFFFFFFFFFFF"},
  {F_MACADDRESS, {.no_sep = 1}, 1, "01020304050607ab"},
  {F_MACADDRESS, {.no_sep = 1}, 1, "01AB030405060708"},
  {F_MACADDRESS, {.no_sep = 1}, 0, "abc"},
  {F_MACADDRESS, {.no_sep = 1}, 0, "01:02:03:04:05"},
  {F_MACADDRESS, {.no_sep = 1}, 0, "01:02:03:04::ab"},
  {F_MACADDRESS, {.no_sep = 1}, 0, "1:2:3:4:5:6"},
  {F_MACADDRESS, {.no_sep = 1}, 0, "AB:CD:EF:GH:01:02"},
  {F_MACADDRESS, {.no_sep = 1}, 0, "ab:ab:ab:ab:ab:ab"},
  {F_MACADDRESS, {.no_sep = 1}, 0, "FF:FF:FF:FF:FF:FF"},
  {F_MACADDRESS, {.no_sep = 1}, 0, "01:02:03:04:05:ab"},
  {F_MACADDRESS, {.no_sep = 1}, 0, "01:AB:03:04:05:06"},
  {F_MACADDRESS, {.no_sep = 1}, 0, "0102030405"},
  {F_MACADDRESS, {.no_sep = 1}, 0, "01020304ab"},
  {F_MACADDRESS, {.no_sep = 1}, 0, "123456"},
  {F_MACADDRESS, {.no_sep = 1}, 0, "ABCDEFGH0102"},
  {F_MACADDRESS, {.no_sep = 1}, 0, "01:02:03:04:05:06:07"},
  {F_MACADDRESS, {.no_sep = 1}, 0, "01:02:03:04:05:06::ab"},
  {F_MACADDRESS, {.no_sep = 1}, 0, "1:2:3:4:5:6:7:8"},
  {F_MACADDRESS, {.no_sep = 1}, 0, "AB:CD:EF:GH:01:02:03:04"},
  {F_MACADDRESS, {.no_sep = 1}, 0, "ab:ab:ab:ab:ab:ab:ab:ab"},
  {F_MACADDRESS, {.no_sep = 1}, 0, "FF:FF:FF:FF:FF:FF:FF:FF"},
  {F_MACADDRESS, {.no_sep = 1}, 0, "01:02:03:04:05:06:07:ab"},
  {F_MACADDRESS, {.no_sep = 1}, 0, "01:AB:03:04:05:06:07:08"},
  {F_MACADDRESS, {.no_sep = 1}, 0, "01020304050607"},
  {F_MACADDRESS, {.no_sep = 1}, 0, "010203040506ab"},
  {F_MACADDRESS, {.no_sep = 1}, 0, "12345678"},
  {F_MACADDRESS, {.no_sep = 1}, 0, "ABCDEFGH01020304"},
  {F_MACADDRESS, {.no_sep = 1, .eui = 48}, 1, "abababababab"},
  {F_MACADDRESS, {.no_sep = 1, .eui = 48}, 1, "FFFFFFFFFFFF"},
  {F_MACADDRESS, {.no_sep = 1, .eui = 48}, 1, "0102030405ab"},
  {F_MACADDRESS, {.no_sep = 1, .eui = 48}, 1, "01AB03040506"},
  {F_MACADDRESS, {.no_sep = 1, .eui = 48}, 0, "abababababababab"},
  {F_MACADDRESS, {.no_sep = 1, .eui = 48}, 0, "FFFFFFFFFFFFFFFF"},
  {F_MACADDRESS, {.no_sep = 1, .eui = 48}, 0, "01020304050607ab"},
  {F_MACADDRESS, {.no_sep = 1, .eui = 48}, 0, "01AB030405060708"},
  {F_MACADDRESS, {.no_sep = 1, .eui = 64}, 1, "abababababababab"},
  {F_MACADDRESS, {.no_sep = 1, .eui = 64}, 1, "FFFFFFFFFFFFFFFF"},
  {F_MACADDRESS, {.no_sep = 1, .eui = 64}, 1, "01020304050607ab"},
  {F_MACADDRESS, {.no_sep = 1, .eui = 64}, 1, "01AB030405060708"},
  {F_MACADDRESS, {.no_sep = 1, .eui = 64}, 0, "abababababab"},
  {F_MACADDRESS, {.no_sep = 1, .eui = 64}, 0, "FFFFFFFFFFFF"},
  {F_MACADDRESS, {.no_sep = 1, .eui = 64}, 0, "0102030405ab"},
  {F_MACADDRESS, {.no_sep = 1, .eui = 64}, 0, "01AB03040506"},
  {F_FQDN, {}, 1, "domain.com"},
  {F_FQDN, {}, 1, "dom.plato"},
  {F_FQDN, {}, 1, "a.domain.co"},
  {F_FQDN, {}, 1, "foo--bar.com"},
  {F_FQDN, {}, 1, "xn--froschgrn-x9a.com"},
  {F_FQDN, {}, 1, "rebecca.blackfriday"},
  {F_FQDN, {}, 1, "1337.com"},
  {F_FQDN, {}, 0, "abc"},
  {F_FQDN, {}, 0, "256.0.0.0"},
  {F_FQDN, {}, 0, "_.com"},
  {F_FQDN, {}, 0, "*.some.com"},
  {F_FQDN, {}, 0, "s!ome.com"},
  {F_FQDN, {}, 0, "domain.com/"},
  {F_FQDN, {}, 0, "/more.com"},
  {F_FQDN, {}, 0, "example.0"},
  {F_FQDN, {}, 0, "192.168.0.9999"},
  {F_FQDN, {}, 0, "192.168.0"},
  {F_ALPHA, {}, 1, "abc"},
  {F_ALPHA, {}, 1, "ABC"},
  {F_ALPHA, {}, 1, "FoObar"},
  {F_ALPHA, {}, 0, "abc1"},
  {F_ALPHA, {}, 0, "  foo  "},
  {F_ALPHA, {}, 0, ""},
  {F_ALPHANUMERIC, {}, 1, "abc123"},
  {F_ALPHANUMERIC, {}, 1, "ABC11"},
  {F_ALPHANUMERIC, {}, 0, "abc "},
  {F_ALPHANUMERIC, {}, 0, "foo!!"},
  {F_NUMERIC, {}, 1, "123"},
  {F_NUMERIC, {}, 1, "00123"},
  {F_NUMERIC, {}, 1, "-00123"},
  {F_NUMERIC, {}, 1, "0"},
  {F_NUMERIC, {}, 1, "-0"},
  {F_NUMERIC, {}, 1, "+123"},
  {F_NUMERIC, {}, 1, "123.123"},
  {F_NUMERIC, {}, 1, "+000000"},
  {F_NUMERIC, {}, 0, " "},
  {F_NUMERIC, {}, 0, ""},
  {F_NUMERIC, {}, 0, "."},
  {F_NUMERIC, {.no_symbols = 1}, 1, "123"},
  {F_NUMERIC, {.no_symbols = 1}, 1, "00123"},
  {F_NUMERIC, {.no_symbols = 1}, 1, "0"},
  {F_NUMERIC, {.no_symbols = 1}, 0, "-0"},
  {F_NUMERIC, {.no_symbols = 1}, 0, "+000000"},
  {F_NUMERIC, {.no_symbols = 1}, 0, ""},
  {F_NUMERIC, {.no_symbols = 1}, 0, "+123"},
  {F_NUMERIC, {.no_symbols = 1}, 0, "123.123"},
  {F_NUMERIC, {.no_symbols = 1}, 0, "-00123"},
  {F_NUMERIC, {.no_symbols = 1}, 0, " "},
  {F_NUMERIC, {.no_symbols = 1}, 0, "."},
  {F_INT, {}, 1, "13"},
  {F_INT, {}, 1, "123"},
  {F_INT, {}, 1, "0"},
  {F_INT, {}, 1, "123"},
  {F_INT, {}, 1, "-0"},
  {F_INT, {}, 1, "+1"},
  {F_INT, {}, 1, "01"},
  {F_INT, {}, 1, "-01"},
  {F_INT, {}, 1, "000"},
  {F_INT, {}, 0, "100e10"},
  {F_INT, {}, 0, "123.123"},
  {F_INT, {}, 0, "   "},
  {F_INT, {}, 0, ""},
  {F_INT, {.no_lead0 = 1}, 1, "13"},
  {F_INT, {.no_lead0 = 1}, 1, "123"},
  {F_INT, {.no_lead0 = 1}, 1, "0"},
  {F_INT, {.no_lead0 = 1}, 1, "123"},
  {F_INT, {.no_lead0 = 1}, 1, "-0"},
  {F_INT, {.no_lead0 = 1}, 1, "+1"},
  {F_INT, {.no_lead0 = 1}, 0, "01"},
  {F_INT, {.no_lead0 = 1}, 0, "-01"},
  {F_INT, {.no_lead0 = 1}, 0, "000"},
  {F_INT, {.no_lead0 = 1}, 0, "100e10"},
  {F_INT, {.no_lead0 = 1}, 0, "123.123"},
  {F_INT, {.no_lead0 = 1}, 0, "   "},
  {F_INT, {.no_lead0 = 1}, 0, ""},
  {F_INT, {.no_lead0 = 0}, 1, "13"},
  {F_INT, {.no_lead0 = 0}, 1, "123"},
  {F_INT, {.no_lead0 = 0}, 1, "0"},
  {F_INT, {.no_lead0 = 0}, 1, "123"},
  {F_INT, {.no_lead0 = 0}, 1, "-0"},
  {F_INT, {.no_lead0 = 0}, 1, "+1"},
  {F_INT, {.no_lead0 = 0}, 1, "01"},
  {F_INT, {.no_lead0 = 0}, 1, "-01"},
  {F_INT, {.no_lead0 = 0}, 1, "000"},
  {F_INT, {.no_lead0 = 0}, 1, "-000"},
  {F_INT, {.no_lead0 = 0}, 1, "+000"},
  {F_INT, {.no_lead0 = 0}, 0, "100e10"},
  {F_INT, {.no_lead0 = 0}, 0, "123.123"},
  {F_INT, {.no_lead0 = 0}, 0, "   "},
  {F_INT, {.no_lead0 = 0}, 0, ""},
  {F_INT, {.has_min = 1, .min = 10}, 1, "15"},
  {F_INT, {.has_min = 1, .min = 10}, 1, "80"},
  {F_INT, {.has_min = 1, .min = 10}, 1, "99"},
  {F_INT, {.has_min = 1, .min = 10}, 0, "9"},
  {F_INT, {.has_min = 1, .min = 10}, 0, "6"},
  {F_INT, {.has_min = 1, .min = 10}, 0, "3.2"},
  {F_INT, {.has_min = 1, .min = 10}, 0, "a"},
  {F_INT, {.has_min = 1, .min = 10, .has_max = 1, .max = 15}, 1, "15"},
  {F_INT, {.has_min = 1, .min = 10, .has_max = 1, .max = 15}, 1, "11"},
  {F_INT, {.has_min = 1, .min = 10, .has_max = 1, .max = 15}, 1, "13"},
  {F_INT, {.has_min = 1, .min = 10, .has_max = 1, .max = 15}, 0, "9"},
  {F_INT, {.has_min = 1, .min = 10, .has_max = 1, .max = 15}, 0, "2"},
  {F_INT, {.has_min = 1, .min = 10, .has_max = 1, .max = 15}, 0, "17"},
  {F_INT, {.has_min = 1, .min = 10, .has_max = 1, .max = 15}, 0, "3.2"},
  {F_INT, {.has_min = 1, .min = 10, .has_max = 1, .max = 15}, 0, "33"},
  {F_INT, {.has_min = 1, .min = 10, .has_max = 1, .max = 15}, 0, "a"},
  {F_INT, {}, 1, "143"},
  {F_INT, {}, 1, "15"},
  {F_INT, {}, 1, "767777575"},
  {F_INT, {}, 0, "10.4"},
  {F_INT, {}, 0, "bar"},
  {F_INT, {}, 0, "10a"},
  {F_INT, {}, 0, "c44"},
  {F_FLOAT, {}, 1, "123"},
  {F_FLOAT, {}, 1, "123."},
  {F_FLOAT, {}, 1, "123.123"},
  {F_FLOAT, {}, 1, "-123.123"},
  {F_FLOAT, {}, 1, "-0.123"},
  {F_FLOAT, {}, 1, "+0.123"},
  {F_FLOAT, {}, 1, "0.123"},
  {F_FLOAT, {}, 1, ".0"},
  {F_FLOAT, {}, 1, "-.123"},
  {F_FLOAT, {}, 1, "+.123"},
  {F_FLOAT, {}, 1, "01.123"},
  {F_FLOAT, {}, 1, "-0.22250738585072011e-307"},
  {F_FLOAT, {}, 0, "+"},
  {F_FLOAT, {}, 0, "-"},
  {F_FLOAT, {}, 0, "  "},
  {F_FLOAT, {}, 0, ""},
  {F_FLOAT, {}, 0, "."},
  {F_FLOAT, {}, 0, ","},
  {F_FLOAT, {}, 0, "foo"},
  {F_FLOAT, {}, 0, "20.foo"},
  {F_FLOAT, {}, 0, "2020-01-06T14:31:00.135Z"},
  {F_FLOAT, {.has_min = 1, .min = 3.7}, 1, "3.888"},
  {F_FLOAT, {.has_min = 1, .min = 3.7}, 1, "3.92"},
  {F_FLOAT, {.has_min = 1, .min = 3.7}, 1, "4.5"},
  {F_FLOAT, {.has_min = 1, .min = 3.7}, 1, "50"},
  {F_FLOAT, {.has_min = 1, .min = 3.7}, 1, "3.7"},
  {F_FLOAT, {.has_min = 1, .min = 3.7}, 1, "3.71"},
  {F_FLOAT, {.has_min = 1, .min = 3.7}, 0, "3.6"},
  {F_FLOAT, {.has_min = 1, .min = 3.7}, 0, "3.69"},
  {F_FLOAT, {.has_min = 1, .min = 3.7}, 0, "3"},
  {F_FLOAT, {.has_min = 1, .min = 3.7}, 0, "1.5"},
  {F_FLOAT, {.has_min = 1, .min = 3.7}, 0, "a"},
  {F_FLOAT, {.has_min = 1, .min = 0.1, .has_max = 1, .max = 1.0}, 1, "0.1"},
  {F_FLOAT, {.has_min = 1, .min = 0.1, .has_max = 1, .max = 1.0}, 1, "1.0"},
  {F_FLOAT, {.has_min = 1, .min = 0.1, .has_max = 1, .max = 1.0}, 1, "0.15"},
  {F_FLOAT, {.has_min = 1, .min = 0.1, .has_max = 1, .max = 1.0}, 1, "0.33"},
  {F_FLOAT, {.has_min = 1, .min = 0.1, .has_max = 1, .max = 1.0}, 1, "0.57"},
  {F_FLOAT, {.has_min = 1, .min = 0.1, .has_max = 1, .max = 1.0}, 1, "0.7"},
  {F_FLOAT, {.has_min = 1, .min = 0.1, .has_max = 1, .max = 1.0}, 0, "0"},
  {F_FLOAT, {.has_min = 1, .min = 0.1, .has_max = 1, .max = 1.0}, 0, "0.0"},
  {F_FLOAT, {.has_min = 1, .min = 0.1, .has_max = 1, .max = 1.0}, 0, "a"},
  {F_FLOAT, {.has_min = 1, .min = 0.1, .has_max = 1, .max = 1.0}, 0, "1.3"},
  {F_FLOAT, {.has_min = 1, .min = 0.1, .has_max = 1, .max = 1.0}, 0, "0.05"},
  {F_FLOAT, {.has_min = 1, .min = 0.1, .has_max = 1, .max = 1.0}, 0, "5"},
  {F_FLOAT, {}, 1, "123"},
  {F_FLOAT, {}, 1, "123."},
  {F_FLOAT, {}, 1, "123.123"},
  {F_FLOAT, {}, 1, "-767.767"},
  {F_FLOAT, {}, 1, "+111.111"},
  {F_FLOAT, {}, 0, "ab565"},
  {F_FLOAT, {}, 0, "-,123"},
  {F_FLOAT, {}, 0, "+,123"},
  {F_FLOAT, {}, 0, "7866.t"},
  {F_FLOAT, {}, 0, "123,123"},
  {F_FLOAT, {}, 0, "123,"},
  {F_HEXCOLOR, {}, 1, "#ff0000ff"},
  {F_HEXCOLOR, {}, 1, "#ff0034"},
  {F_HEXCOLOR, {}, 1, "#CCCCCC"},
  {F_HEXCOLOR, {}, 1, "0f38"},
  {F_HEXCOLOR, {}, 1, "fff"},
  {F_HEXCOLOR, {}, 1, "#f00"},
  {F_HEXCOLOR, {}, 0, "#ff"},
  {F_HEXCOLOR, {}, 0, "fff0a"},
  {F_HEXCOLOR, {}, 0, "#ff12FG"},
  {F_EMPTY, {}, 1, ""},
  {F_EMPTY, {}, 0, " "},
  {F_EMPTY, {}, 0, "foo"},
  {F_EMPTY, {}, 0, "3"},
  {F_EMPTY, {.ignore_ws = 0}, 1, ""},
  {F_EMPTY, {.ignore_ws = 0}, 0, " "},
  {F_EMPTY, {.ignore_ws = 0}, 0, "foo"},
  {F_EMPTY, {.ignore_ws = 0}, 0, "3"},
  {F_EMPTY, {.ignore_ws = 1}, 1, ""},
  {F_EMPTY, {.ignore_ws = 1}, 1, " "},
  {F_EMPTY, {.ignore_ws = 1}, 0, "foo"},
  {F_EMPTY, {.ignore_ws = 1}, 0, "3"},
  {F_LENGTH, {.has_min = 1, .min = 2}, 1, "abc"},
  {F_LENGTH, {.has_min = 1, .min = 2}, 1, "de"},
  {F_LENGTH, {.has_min = 1, .min = 2}, 1, "abcd"},
  {F_LENGTH, {.has_min = 1, .min = 2}, 0, ""},
  {F_LENGTH, {.has_min = 1, .min = 2}, 0, "a"},
  {F_LENGTH, {.has_min = 1, .min = 2, .has_max = 1, .max = 3}, 1, "abc"},
  {F_LENGTH, {.has_min = 1, .min = 2, .has_max = 1, .max = 3}, 1, "de"},
  {F_LENGTH, {.has_min = 1, .min = 2, .has_max = 1, .max = 3}, 0, ""},
  {F_LENGTH, {.has_min = 1, .min = 2, .has_max = 1, .max = 3}, 0, "a"},
  {F_LENGTH, {.has_min = 1, .min = 2, .has_max = 1, .max = 3}, 0, "abcd"},
  {F_LENGTH, {.has_min = 1, .min = 2, .has_max = 1, .max = 3}, 0, ""},
  {F_LENGTH, {.has_min = 1, .min = 0, .has_max = 1, .max = 0}, 1, ""},
  {F_LENGTH, {.has_min = 1, .min = 0, .has_max = 1, .max = 0}, 0, "a"},
  {F_LENGTH, {.has_min = 1, .min = 0, .has_max = 1, .max = 0}, 0, "ab"},
  {F_BYTELENGTH, {.has_min = 1, .min = 2}, 1, "abc"},
  {F_BYTELENGTH, {.has_min = 1, .min = 2}, 1, "de"},
  {F_BYTELENGTH, {.has_min = 1, .min = 2}, 1, "abcd"},
  {F_BYTELENGTH, {.has_min = 1, .min = 2}, 0, ""},
  {F_BYTELENGTH, {.has_min = 1, .min = 2}, 0, "a"},
  {F_BYTELENGTH, {.has_min = 1, .min = 2, .has_max = 1, .max = 3}, 1, "abc"},
  {F_BYTELENGTH, {.has_min = 1, .min = 2, .has_max = 1, .max = 3}, 1, "de"},
  {F_BYTELENGTH, {.has_min = 1, .min = 2, .has_max = 1, .max = 3}, 0, ""},
  {F_BYTELENGTH, {.has_min = 1, .min = 2, .has_max = 1, .max = 3}, 0, "a"},
  {F_BYTELENGTH, {.has_min = 1, .min = 2, .has_max = 1, .max = 3}, 0, "abcd"},
  {F_BYTELENGTH, {.has_min = 1, .min = 0, .has_max = 1, .max = 0}, 1, ""},
  {F_BYTELENGTH, {.has_min = 1, .min = 0, .has_max = 1, .max = 0}, 0, "a"},
  {F_LENGTH, {.has_min = 1, .min = 2}, 1, "abc"},
  {F_LENGTH, {.has_min = 1, .min = 2}, 1, "de"},
  {F_LENGTH, {.has_min = 1, .min = 2}, 1, "abcd"},
  {F_LENGTH, {.has_min = 1, .min = 2}, 0, ""},
  {F_LENGTH, {.has_min = 1, .min = 2}, 0, "a"},
  {F_LENGTH, {.has_min = 1, .min = 2, .has_max = 1, .max = 3}, 1, "abc"},
  {F_LENGTH, {.has_min = 1, .min = 2, .has_max = 1, .max = 3}, 1, "de"},
  {F_LENGTH, {.has_min = 1, .min = 2, .has_max = 1, .max = 3}, 0, ""},
  {F_LENGTH, {.has_min = 1, .min = 2, .has_max = 1, .max = 3}, 0, "a"},
  {F_LENGTH, {.has_min = 1, .min = 2, .has_max = 1, .max = 3}, 0, "abcd"},
  {F_LENGTH, {.has_min = 1, .min = 2, .has_max = 1, .max = 3}, 0, ""},
  {F_LENGTH, {.has_max = 1, .max = 3}, 1, "abc"},
  {F_LENGTH, {.has_max = 1, .max = 3}, 1, "de"},
  {F_LENGTH, {.has_max = 1, .max = 3}, 1, "a"},
  {F_LENGTH, {.has_max = 1, .max = 3}, 1, ""},
  {F_LENGTH, {.has_max = 1, .max = 3}, 0, "abcd"},
  {F_LENGTH, {.has_max = 1, .max = 0}, 1, ""},
  {F_LENGTH, {.has_max = 1, .max = 0}, 0, "a"},
  {F_LENGTH, {.has_max = 1, .max = 0}, 0, "ab"},
  {F_LENGTH, {}, 1, "a"},
  {F_LENGTH, {}, 1, ""},
  {F_LENGTH, {}, 1, "asds"},
  {F_BYTELENGTH, {.has_min = 1, .min = 2}, 1, "abc"},
  {F_BYTELENGTH, {.has_min = 1, .min = 2}, 1, "de"},
  {F_BYTELENGTH, {.has_min = 1, .min = 2}, 1, "abcd"},
  {F_BYTELENGTH, {.has_min = 1, .min = 2}, 0, ""},
  {F_BYTELENGTH, {.has_min = 1, .min = 2}, 0, "a"},
  {F_BYTELENGTH, {.has_min = 1, .min = 2, .has_max = 1, .max = 3}, 1, "abc"},
  {F_BYTELENGTH, {.has_min = 1, .min = 2, .has_max = 1, .max = 3}, 1, "de"},
  {F_BYTELENGTH, {.has_min = 1, .min = 2, .has_max = 1, .max = 3}, 0, ""},
  {F_BYTELENGTH, {.has_min = 1, .min = 2, .has_max = 1, .max = 3}, 0, "a"},
  {F_BYTELENGTH, {.has_min = 1, .min = 2, .has_max = 1, .max = 3}, 0, "abcd"},
  {F_BYTELENGTH, {.has_max = 1, .max = 3}, 1, "abc"},
  {F_BYTELENGTH, {.has_max = 1, .max = 3}, 1, "de"},
  {F_BYTELENGTH, {.has_max = 1, .max = 3}, 1, "a"},
  {F_BYTELENGTH, {.has_max = 1, .max = 3}, 1, ""},
  {F_BYTELENGTH, {.has_max = 1, .max = 3}, 0, "abcd"},
  {F_BYTELENGTH, {.has_max = 1, .max = 0}, 1, ""},
  {F_BYTELENGTH, {.has_max = 1, .max = 0}, 0, "a"},
  {F_UUID, {}, 1, "9deb20fe-a6e0-355c-81ea-288b009e4f6d"},
  {F_UUID, {}, 1, "A987FBC9-4BED-4078-8F07-9141BA07C9F3"},
  {F_UUID, {}, 1, "A987FBC9-4BED-5078-AF07-9141BA07C9F3"},
  {F_UUID, {}, 1, "A987FBC9-4BED-6078-AF07-9141BA07C9F3"},
  {F_UUID, {}, 1, "018C544A-D384-7000-BB74-3B1738ABE43C"},
  {F_UUID, {}, 1, "A987FBC9-4BED-8078-AF07-9141BA07C9F3"},
  {F_UUID, {}, 0, ""},
  {F_UUID, {}, 0, "xxxA987FBC9-4BED-3078-CF07-9141BA07C9F3"},
  {F_UUID, {}, 0, "A987FBC9-4BED-3078-CF07-9141BA07C9F3"},
  {F_UUID, {}, 0, "A987FBC9-4BED-3078-CF07-9141BA07C9F3xxx"},
  {F_UUID, {}, 0, "A987FBC94BED3078CF079141BA07C9F3"},
  {F_UUID, {}, 0, "934859"},
  {F_UUID, {}, 0, "987FBC9-4BED-3078-CF07A-9141BA07C9F3"},
  {F_UUID, {}, 0, "AAAAAAAA-1111-1111-AAAG-111111111111"},
  {F_UUID, {.version = 1}, 1, "E034B584-7D89-11E9-9669-1AECF481A97B"},
  {F_UUID, {.version = 1}, 0, "xxxA987FBC9-4BED-3078-CF07-9141BA07C9F3"},
  {F_UUID, {.version = 1}, 0, "AAAAAAAA-1111-2222-AAAG"},
  {F_UUID, {.version = 1}, 0, "AAAAAAAA-1111-2222-AAAG-111111111111"},
  {F_UUID, {.version = 1}, 0, "A987FBC9-4BED-4078-8F07-9141BA07C9F3"},
  {F_UUID, {.version = 1}, 0, "A987FBC9-4BED-5078-AF07-9141BA07C9F3"},
  {F_UUID, {.version = 1}, 0, "018C544A-D384-7000-BB74-3B1738ABE43C"},
  {F_UUID, {.version = 2}, 1, "A987FBC9-4BED-2078-AF07-9141BA07C9F3"},
  {F_UUID, {.version = 2}, 0, ""},
  {F_UUID, {.version = 2}, 0, "xxxA987FBC9-4BED-3078-CF07-9141BA07C9F3"},
  {F_UUID, {.version = 2}, 0, "11111"},
  {F_UUID, {.version = 2}, 0, "AAAAAAAA-1111-1111-AAAG-111111111111"},
  {F_UUID, {.version = 2}, 0, "A987FBC9-4BED-2078-CF07-9141BA07C9F3"},
  {F_UUID, {.version = 2}, 0, "A987FBC9-4BED-4078-8F07-9141BA07C9F3"},
  {F_UUID, {.version = 2}, 0, "A987FBC9-4BED-5078-AF07-9141BA07C9F3"},
  {F_UUID, {.version = 2}, 0, "018C544A-D384-7000-BB74-3B1738ABE43C"},
  {F_UUID, {.version = 3}, 1, "9deb20fe-a6e0-355c-81ea-288b009e4f6d"},
  {F_UUID, {.version = 3}, 0, ""},
  {F_UUID, {.version = 3}, 0, "A987FBC9-4BED-3078-CF07-9141BA07C9F3"},
  {F_UUID, {.version = 3}, 0, "xxxA987FBC9-4BED-3078-CF07-9141BA07C9F3"},
  {F_UUID, {.version = 3}, 0, "934859"},
  {F_UUID, {.version = 3}, 0, "AAAAAAAA-1111-1111-AAAG-111111111111"},
  {F_UUID, {.version = 3}, 0, "A987FBC9-4BED-4078-8F07-9141BA07C9F3"},
  {F_UUID, {.version = 3}, 0, "A987FBC9-4BED-5078-AF07-9141BA07C9F3"},
  {F_UUID, {.version = 3}, 0, "018C544A-D384-7000-BB74-3B1738ABE43C"},
  {F_UUID, {.version = 4}, 1, "713ae7e3-cb32-45f9-adcb-7c4fa86b90c1"},
  {F_UUID, {.version = 4}, 1, "625e63f3-58f5-40b7-83a1-a72ad31acffb"},
  {F_UUID, {.version = 4}, 1, "57b73598-8764-4ad0-a76a-679bb6640eb1"},
  {F_UUID, {.version = 4}, 1, "9c858901-8a57-4791-81fe-4c455b099bc9"},
  {F_UUID, {.version = 4}, 0, ""},
  {F_UUID, {.version = 4}, 0, "xxxA987FBC9-4BED-3078-CF07-9141BA07C9F3"},
  {F_UUID, {.version = 4}, 0, "934859"},
  {F_UUID, {.version = 4}, 0, "AAAAAAAA-1111-1111-AAAG-111111111111"},
  {F_UUID, {.version = 4}, 0, "A987FBC9-4BED-5078-AF07-9141BA07C9F3"},
  {F_UUID, {.version = 4}, 0, "A987FBC9-4BED-3078-CF07-9141BA07C9F3"},
  {F_UUID, {.version = 4}, 0, "018C544A-D384-7000-BB74-3B1738ABE43C"},
  {F_UUID, {.version = 5}, 1, "987FBC97-4BED-5078-AF07-9141BA07C9F3"},
  {F_UUID, {.version = 5}, 1, "987FBC97-4BED-5078-BF07-9141BA07C9F3"},
  {F_UUID, {.version = 5}, 1, "987FBC97-4BED-5078-8F07-9141BA07C9F3"},
  {F_UUID, {.version = 5}, 1, "987FBC97-4BED-5078-9F07-9141BA07C9F3"},
  {F_UUID, {.version = 5}, 0, ""},
  {F_UUID, {.version = 5}, 0, "xxxA987FBC9-4BED-3078-CF07-9141BA07C9F3"},
  {F_UUID, {.version = 5}, 0, "934859"},
  {F_UUID, {.version = 5}, 0, "AAAAAAAA-1111-1111-AAAG-111111111111"},
  {F_UUID, {.version = 5}, 0, "9c858901-8a57-4791-81fe-4c455b099bc9"},
  {F_UUID, {.version = 5}, 0, "A987FBC9-4BED-3078-CF07-9141BA07C9F3"},
  {F_UUID, {.version = 5}, 0, "018C544A-D384-7000-BB74-3B1738ABE43C"},
  {F_UUID, {.version = 6}, 1, "1ef29908-cde1-69d0-be16-bfc8518a95f0"},
  {F_UUID, {.version = 6}, 0, "987FBC97-4BED-1078-AF07-9141BA07C9F3"},
  {F_UUID, {.version = 6}, 0, "987FBC97-4BED-2078-AF07-9141BA07C9F3"},
  {F_UUID, {.version = 6}, 0, "987FBC97-4BED-3078-AF07-9141BA07C9F3"},
  {F_UUID, {.version = 6}, 0, "987FBC97-4BED-4078-AF07-9141BA07C9F3"},
  {F_UUID, {.version = 6}, 0, "987FBC97-4BED-5078-AF07-9141BA07C9F3"},
  {F_UUID, {.version = 6}, 0, "018C544A-D384-7000-BB74-3B1738ABE43C"},
  {F_UUID, {.version = 6}, 0, "987FBC97-4BED-8078-AF07-9141BA07C9F3"},
  {F_UUID, {.version = 7}, 1, "018C544A-D384-7000-BB74-3B1738ABE43C"},
  {F_UUID, {.version = 7}, 0, ""},
  {F_UUID, {.version = 7}, 0, "xxxA987FBC9-4BED-3078-CF07-9141BA07C9F3"},
  {F_UUID, {.version = 7}, 0, "934859"},
  {F_UUID, {.version = 7}, 0, "AAAAAAAA-1111-1111-AAAG-111111111111"},
  {F_UUID, {.version = 7}, 0, "A987FBC9-4BED-5078-AF07-9141BA07C9F3"},
  {F_UUID, {.version = 7}, 0, "A987FBC9-4BED-3078-CF07-9141BA07C9F3"},
  {F_UUID, {.version = 7}, 0, "A987FBC9-4BED-6078-AF07-9141BA07C9F3"},
  {F_UUID, {.version = 7}, 0, "A987FBC9-4BED-8078-AF07-9141BA07C9F3"},
  {F_UUID, {.version = 7}, 0, "713ae7e3-cb32-45f9-adcb-7c4fa86b90c1"},
  {F_UUID, {.version = 7}, 0, "625e63f3-58f5-40b7-83a1-a72ad31acffb"},
  {F_UUID, {.version = 7}, 0, "57b73598-8764-4ad0-a76a-679bb6640eb1"},
  {F_UUID, {.version = 7}, 0, "9c858901-8a57-4791-81fe-4c455b099bc9"},
  {F_UUID, {.version = 8}, 1, "018C544A-D384-8000-BB74-3B1738ABE43C"},
  {F_UUID, {.version = 8}, 0, ""},
  {F_UUID, {.version = 8}, 0, "xxxA987FBC9-4BED-3078-CF07-9141BA07C9F3"},
  {F_UUID, {.version = 8}, 0, "934859"},
  {F_UUID, {.version = 8}, 0, "AAAAAAAA-1111-1111-AAAG-111111111111"},
  {F_UUID, {.version = 8}, 0, "A987FBC9-4BED-5078-AF07-9141BA07C9F3"},
  {F_UUID, {.version = 8}, 0, "A987FBC9-4BED-3078-CF07-9141BA07C9F3"},
  {F_UUID, {.version = 8}, 0, "A987FBC9-4BED-6078-AF07-9141BA07C9F3"},
  {F_UUID, {.version = 8}, 0, "A987FBC9-4BED-7078-AF07-9141BA07C9F3"},
  {F_UUID, {.version = 8}, 0, "713ae7e3-cb32-45f9-adcb-7c4fa86b90c1"},
  {F_UUID, {.version = 8}, 0, "625e63f3-58f5-40b7-83a1-a72ad31acffb"},
  {F_UUID, {.version = 8}, 0, "57b73598-8764-4ad0-a76a-679bb6640eb1"},
  {F_UUID, {.version = 8}, 0, "9c858901-8a57-4791-81fe-4c455b099bc9"},
  {F_JSON, {}, 1, "{ \"key\": \"value\" }"},
  {F_JSON, {}, 1, "{}"},
  {F_JSON, {}, 0, "{ key: \"value\" }"},
  {F_JSON, {}, 0, "{ 'key': 'value' }"},
  {F_JSON, {}, 0, "null"},
  {F_JSON, {}, 0, "1234"},
  {F_JSON, {}, 0, "\"nope\""},
  {F_BOOLEAN, {}, 1, "true"},
  {F_BOOLEAN, {}, 1, "false"},
  {F_BOOLEAN, {}, 1, "0"},
  {F_BOOLEAN, {}, 1, "1"},
  {F_BOOLEAN, {}, 0, "1.0"},
  {F_BOOLEAN, {}, 0, "0.0"},
  {F_BOOLEAN, {}, 0, "true "},
  {F_BOOLEAN, {}, 0, "False"},
  {F_BOOLEAN, {}, 0, "True"},
  {F_BOOLEAN, {}, 0, "yes"},
  {F_BOOLEAN, {.loose = 1}, 1, "true"},
  {F_BOOLEAN, {.loose = 1}, 1, "True"},
  {F_BOOLEAN, {.loose = 1}, 1, "TRUE"},
  {F_BOOLEAN, {.loose = 1}, 1, "false"},
  {F_BOOLEAN, {.loose = 1}, 1, "False"},
  {F_BOOLEAN, {.loose = 1}, 1, "FALSE"},
  {F_BOOLEAN, {.loose = 1}, 1, "0"},
  {F_BOOLEAN, {.loose = 1}, 1, "1"},
  {F_BOOLEAN, {.loose = 1}, 1, "yes"},
  {F_BOOLEAN, {.loose = 1}, 1, "Yes"},
  {F_BOOLEAN, {.loose = 1}, 1, "YES"},
  {F_BOOLEAN, {.loose = 1}, 1, "no"},
  {F_BOOLEAN, {.loose = 1}, 1, "No"},
  {F_BOOLEAN, {.loose = 1}, 1, "NO"},
  {F_BOOLEAN, {.loose = 1}, 0, "1.0"},
  {F_BOOLEAN, {.loose = 1}, 0, "0.0"},
  {F_BOOLEAN, {.loose = 1}, 0, "true "},
  {F_BOOLEAN, {.loose = 1}, 0, " false"},
  {F_BASE64, {}, 1, ""},
  {F_BASE64, {}, 1, "Zg=="},
  {F_BASE64, {}, 1, "Zm8="},
  {F_BASE64, {}, 1, "Zm9v"},
  {F_BASE64, {}, 1, "Zm9vYg=="},
  {F_BASE64, {}, 1, "Zm9vYmE="},
  {F_BASE64, {}, 1, "Zm9vYmFy"},
  {F_BASE64, {}, 1, "TG9yZW0gaXBzdW0gZG9sb3Igc2l0IGFtZXQsIGNvbnNlY3RldHVyIGFkaXBpc2NpbmcgZWxpdC4="},
  {F_BASE64, {}, 1, "Vml2YW11cyBmZXJtZW50dW0gc2VtcGVyIHBvcnRhLg=="},
  {F_BASE64, {}, 1, "U3VzcGVuZGlzc2UgbGVjdHVzIGxlbw=="},
  {F_BASE64, {}, 0, "12345"},
  {F_BASE64, {}, 0, "Vml2YW11cyBmZXJtZtesting123"},
  {F_BASE64, {}, 0, "Zg="},
  {F_BASE64, {}, 0, "Z==="},
  {F_BASE64, {}, 0, "Zm=8"},
  {F_BASE64, {}, 0, "=m9vYg=="},
  {F_BASE64, {}, 0, "Zm9vYmFy===="},
  {F_BASE64, {.url_safe = 1}, 1, ""},
  {F_BASE64, {.url_safe = 1}, 1, "bGFkaWVzIGFuZCBnZW50bGVtZW4sIHdlIGFyZSBmbG9hdGluZyBpbiBzcGFjZQ"},
  {F_BASE64, {.url_safe = 1}, 1, "1234"},
  {F_BASE64, {.url_safe = 1}, 1, "bXVtLW5ldmVyLXByb3Vk"},
  {F_BASE64, {.url_safe = 1}, 1, "PDw_Pz8-Pg"},
  {F_BASE64, {.url_safe = 1}, 1, "VGhpcyBpcyBhbiBlbmNvZGVkIHN0cmluZw"},
  {F_BASE64, {.url_safe = 1}, 0, " AA"},
  {F_BASE64, {.url_safe = 1}, 0, "\tAA"},
  {F_BASE64, {.url_safe = 1}, 0, "\015AA"},
  {F_BASE64, {.url_safe = 1}, 0, "\nAA"},
  {F_BASE64, {.url_safe = 1}, 0, "This+isa/bad+base64Url=="},
  {F_BASE64, {.url_safe = 1}, 0, "0K3RgtC+INC30LDQutC+0LTQuNGA0L7QstCw0L3QvdCw0Y8g0YHRgtGA0L7QutCw"},
  {F_BASE64, {.url_safe = 0, .padding = 1, .has_padding = 1}, 1, ""},
  {F_BASE64, {.url_safe = 0, .padding = 1, .has_padding = 1}, 1, "TWFu"},
  {F_BASE64, {.url_safe = 0, .padding = 1, .has_padding = 1}, 1, "TWE="},
  {F_BASE64, {.url_safe = 0, .padding = 1, .has_padding = 1}, 1, "TQ=="},
  {F_BASE64, {.url_safe = 0, .padding = 1, .has_padding = 1}, 1, "SGVsbG8="},
  {F_BASE64, {.url_safe = 0, .padding = 1, .has_padding = 1}, 1, "U29mdHdhcmU="},
  {F_BASE64, {.url_safe = 0, .padding = 1, .has_padding = 1}, 1, "YW55IGNhcm5hbCBwbGVhc3VyZS4="},
  {F_BASE64, {.url_safe = 0, .padding = 1, .has_padding = 1}, 0, "TWF"},
  {F_BASE64, {.url_safe = 0, .padding = 1, .has_padding = 1}, 0, "TWE==="},
  {F_BASE64, {.url_safe = 0, .padding = 1, .has_padding = 1}, 0, "SGVsbG8@"},
  {F_BASE64, {.url_safe = 0, .padding = 1, .has_padding = 1}, 0, "SGVsbG8==="},
  {F_BASE64, {.url_safe = 0, .padding = 1, .has_padding = 1}, 0, "SGVsb G8="},
  {F_BASE64, {.url_safe = 0, .padding = 1, .has_padding = 1}, 0, "===="},
  {F_BASE64, {.url_safe = 0, .padding = 0, .has_padding = 1}, 1, ""},
  {F_BASE64, {.url_safe = 0, .padding = 0, .has_padding = 1}, 1, "TWFu"},
  {F_BASE64, {.url_safe = 0, .padding = 0, .has_padding = 1}, 1, "TWE"},
  {F_BASE64, {.url_safe = 0, .padding = 0, .has_padding = 1}, 1, "TQ"},
  {F_BASE64, {.url_safe = 0, .padding = 0, .has_padding = 1}, 1, "SGVsbG8"},
  {F_BASE64, {.url_safe = 0, .padding = 0, .has_padding = 1}, 1, "U29mdHdhcmU"},
  {F_BASE64, {.url_safe = 0, .padding = 0, .has_padding = 1}, 1, "YW55IGNhcm5hbCBwbGVhc3VyZS4"},
  {F_BASE64, {.url_safe = 0, .padding = 0, .has_padding = 1}, 0, "TWE="},
  {F_BASE64, {.url_safe = 0, .padding = 0, .has_padding = 1}, 0, "TQ==="},
  {F_BASE64, {.url_safe = 0, .padding = 0, .has_padding = 1}, 0, "SGVsbG8@"},
  {F_BASE64, {.url_safe = 0, .padding = 0, .has_padding = 1}, 0, "SGVsbG8==="},
  {F_BASE64, {.url_safe = 0, .padding = 0, .has_padding = 1}, 0, "SGVsb G8"},
  {F_BASE64, {.url_safe = 0, .padding = 0, .has_padding = 1}, 0, "===="},
  {F_BASE64, {.url_safe = 1, .padding = 1, .has_padding = 1}, 1, ""},
  {F_BASE64, {.url_safe = 1, .padding = 1, .has_padding = 1}, 1, "SGVsbG8="},
  {F_BASE64, {.url_safe = 1, .padding = 1, .has_padding = 1}, 1, "U29mdHdhcmU="},
  {F_BASE64, {.url_safe = 1, .padding = 1, .has_padding = 1}, 1, "YW55IGNhcm5hbCBwbGVhc3VyZS4="},
  {F_BASE64, {.url_safe = 1, .padding = 1, .has_padding = 1}, 1, "SGVsbG8-"},
  {F_BASE64, {.url_safe = 1, .padding = 1, .has_padding = 1}, 1, "SGVsbG8_"},
  {F_BASE64, {.url_safe = 1, .padding = 1, .has_padding = 1}, 0, "SGVsbG8==="},
  {F_BASE64, {.url_safe = 1, .padding = 1, .has_padding = 1}, 0, "SGVsbG8@"},
  {F_BASE64, {.url_safe = 1, .padding = 1, .has_padding = 1}, 0, "SGVsb G8="},
  {F_BASE64, {.url_safe = 1, .padding = 1, .has_padding = 1}, 0, "===="},
  {F_BASE64, {.url_safe = 1, .padding = 0, .has_padding = 1}, 1, ""},
  {F_BASE64, {.url_safe = 1, .padding = 0, .has_padding = 1}, 1, "SGVsbG8"},
  {F_BASE64, {.url_safe = 1, .padding = 0, .has_padding = 1}, 1, "U29mdHdhcmU"},
  {F_BASE64, {.url_safe = 1, .padding = 0, .has_padding = 1}, 1, "YW55IGNhcm5hbCBwbGVhc3VyZS4"},
  {F_BASE64, {.url_safe = 1, .padding = 0, .has_padding = 1}, 1, "SGVsbG8-"},
  {F_BASE64, {.url_safe = 1, .padding = 0, .has_padding = 1}, 1, "SGVsbG8_"},
  {F_BASE64, {.url_safe = 1, .padding = 0, .has_padding = 1}, 0, "SGVsbG8="},
  {F_BASE64, {.url_safe = 1, .padding = 0, .has_padding = 1}, 0, "SGVsbG8==="},
  {F_BASE64, {.url_safe = 1, .padding = 0, .has_padding = 1}, 0, "SGVsbG8@"},
  {F_BASE64, {.url_safe = 1, .padding = 0, .has_padding = 1}, 0, "SGVsb G8"},
  {F_BASE64, {.url_safe = 1, .padding = 0, .has_padding = 1}, 0, "===="},
  {F_BASE64, {.url_safe = 0, .padding = 1, .has_padding = 1}, 1, ""},
  {F_BASE64, {.url_safe = 0, .padding = 1, .has_padding = 1}, 1, "TWFu"},
  {F_BASE64, {.url_safe = 0, .padding = 1, .has_padding = 1}, 1, "TWE="},
  {F_BASE64, {.url_safe = 0, .padding = 1, .has_padding = 1}, 1, "TQ=="},
  {F_BASE64, {.url_safe = 0, .padding = 1, .has_padding = 1}, 0, "TWE"},
  {F_BASE64, {.url_safe = 0, .padding = 1, .has_padding = 1}, 0, "TQ="},
  {F_BASE64, {.url_safe = 0, .padding = 1, .has_padding = 1}, 0, "TQ==="},
  {F_BASE64, {.url_safe = 1, .padding = 0, .has_padding = 1}, 1, ""},
  {F_BASE64, {.url_safe = 1, .padding = 0, .has_padding = 1}, 1, "SGVsbG8"},
  {F_BASE64, {.url_safe = 1, .padding = 0, .has_padding = 1}, 1, "SGVsbG8-"},
  {F_BASE64, {.url_safe = 1, .padding = 0, .has_padding = 1}, 1, "SGVsbG8_"},
  {F_BASE64, {.url_safe = 1, .padding = 0, .has_padding = 1}, 0, "SGVsbG8="},
  {F_BASE64, {.url_safe = 1, .padding = 0, .has_padding = 1}, 0, "SGVsbG8@"},
  {F_BASE64, {.url_safe = 1, .padding = 0, .has_padding = 1}, 0, "SGVsb G8"},
  {F_FQDN, {}, 1, "google.com"},
  {F_FQDN, {}, 0, "google.l33t"},
  {F_IP, {}, 1, "127.0.0.1"},
  {F_IP, {}, 1, "0.0.0.0"},
  {F_IP, {}, 1, "255.255.255.255"},
  {F_IP, {}, 1, "1.2.3.4"},
  {F_IP, {}, 1, "::1"},
  {F_IP, {}, 1, "2001:db8:0000:1:1:1:1:1"},
  {F_IP, {}, 1, "2001:db8:3:4::192.0.2.33"},
  {F_IP, {}, 1, "2001:41d0:2:a141::1"},
  {F_IP, {}, 1, "::ffff:127.0.0.1"},
  {F_IP, {}, 1, "::0000"},
  {F_IP, {}, 1, "0000::"},
  {F_IP, {}, 1, "1::"},
  {F_IP, {}, 1, "1111:1:1:1:1:1:1:1"},
  {F_IP, {}, 1, "fe80::a6db:30ff:fe98:e946"},
  {F_IP, {}, 1, "::"},
  {F_IP, {}, 1, "::8"},
  {F_IP, {}, 1, "::ffff:127.0.0.1"},
  {F_IP, {}, 1, "::ffff:255.255.255.255"},
  {F_IP, {}, 1, "::ffff:0:255.255.255.255"},
  {F_IP, {}, 1, "::2:3:4:5:6:7:8"},
  {F_IP, {}, 1, "::255.255.255.255"},
  {F_IP, {}, 1, "0:0:0:0:0:ffff:127.0.0.1"},
  {F_IP, {}, 1, "1:2:3:4:5:6:7::"},
  {F_IP, {}, 1, "1:2:3:4:5:6::8"},
  {F_IP, {}, 1, "1::7:8"},
  {F_IP, {}, 1, "1:2:3:4:5::7:8"},
  {F_IP, {}, 1, "1:2:3:4:5::8"},
  {F_IP, {}, 1, "1::6:7:8"},
  {F_IP, {}, 1, "1:2:3:4::6:7:8"},
  {F_IP, {}, 1, "1:2:3:4::8"},
  {F_IP, {}, 1, "1::5:6:7:8"},
  {F_IP, {}, 1, "1:2:3::5:6:7:8"},
  {F_IP, {}, 1, "1:2:3::8"},
  {F_IP, {}, 1, "1::4:5:6:7:8"},
  {F_IP, {}, 1, "1:2::4:5:6:7:8"},
  {F_IP, {}, 1, "1:2::8"},
  {F_IP, {}, 1, "1::3:4:5:6:7:8"},
  {F_IP, {}, 1, "1::8"},
  {F_IP, {}, 1, "fe80::7:8%eth0"},
  {F_IP, {}, 1, "fe80::7:8%1"},
  {F_IP, {}, 1, "64:ff9b::192.0.2.33"},
  {F_IP, {}, 1, "0:0:0:0:0:0:10.0.0.1"},
  {F_IP, {}, 0, "abc"},
  {F_IP, {}, 0, "256.0.0.0"},
  {F_IP, {}, 0, "0.0.0.256"},
  {F_IP, {}, 0, "26.0.0.256"},
  {F_IP, {}, 0, "0200.200.200.200"},
  {F_IP, {}, 0, "200.0200.200.200"},
  {F_IP, {}, 0, "200.200.0200.200"},
  {F_IP, {}, 0, "200.200.200.0200"},
  {F_IP, {}, 0, "::banana"},
  {F_IP, {}, 0, "banana::"},
  {F_IP, {}, 0, "::1banana"},
  {F_IP, {}, 0, "::1::"},
  {F_IP, {}, 0, "1:"},
  {F_IP, {}, 0, ":1"},
  {F_IP, {}, 0, ":1:1:1::2"},
  {F_IP, {}, 0, "1:1:1:1:1:1:1:1:1:1:1:1:1:1:1:1"},
  {F_IP, {}, 0, "::11111"},
  {F_IP, {}, 0, "11111:1:1:1:1:1:1:1"},
  {F_IP, {}, 0, "2001:db8:0000:1:1:1:1::1"},
  {F_IP, {}, 0, "0:0:0:0:0:0:ffff:127.0.0.1"},
  {F_IP, {}, 0, "0:0:0:0:ffff:127.0.0.1"},
  {F_IP, {}, 0, "BC:e4d5:c:e7b9::%40i0nccymtl9cwfKo.5vaeXLSGRMe:EDh2qs5wkhnPws5xQKqafjfAMm6wGFCJ.bVFsZfb"},
  {F_IP, {}, 0, "1dC:0DF8:62D:3AC::%KTatXocjaFVioS0RTNQl4mA.V151o0RSy.JIu-D-D8.d3171ZWsSJ7PK4YjkJCRN0F"},
  {F_IP, {}, 1, "127.0.0.1"},
  {F_IP, {}, 1, "0.0.0.0"},
  {F_IP, {}, 1, "255.255.255.255"},
  {F_IP, {}, 1, "1.2.3.4"},
  {F_IP, {}, 1, "::1"},
  {F_IP, {}, 1, "2001:db8:0000:1:1:1:1:1"},
  {F_IP, {}, 1, "2001:db8:3:4::192.0.2.33"},
  {F_IP, {}, 1, "2001:41d0:2:a141::1"},
  {F_IP, {}, 1, "::ffff:127.0.0.1"},
  {F_IP, {}, 1, "::0000"},
  {F_IP, {}, 1, "0000::"},
  {F_IP, {}, 1, "1::"},
  {F_IP, {}, 1, "1111:1:1:1:1:1:1:1"},
  {F_IP, {}, 1, "fe80::a6db:30ff:fe98:e946"},
  {F_IP, {}, 1, "::"},
  {F_IP, {}, 1, "::8"},
  {F_IP, {}, 1, "::ffff:127.0.0.1"},
  {F_IP, {}, 1, "::ffff:255.255.255.255"},
  {F_IP, {}, 1, "::ffff:0:255.255.255.255"},
  {F_IP, {}, 1, "::2:3:4:5:6:7:8"},
  {F_IP, {}, 1, "::255.255.255.255"},
  {F_IP, {}, 1, "0:0:0:0:0:ffff:127.0.0.1"},
  {F_IP, {}, 1, "1:2:3:4:5:6:7::"},
  {F_IP, {}, 1, "1:2:3:4:5:6::8"},
  {F_IP, {}, 1, "1::7:8"},
  {F_IP, {}, 1, "1:2:3:4:5::7:8"},
  {F_IP, {}, 1, "1:2:3:4:5::8"},
  {F_IP, {}, 1, "1::6:7:8"},
  {F_IP, {}, 1, "1:2:3:4::6:7:8"},
  {F_IP, {}, 1, "1:2:3:4::8"},
  {F_IP, {}, 1, "1::5:6:7:8"},
  {F_IP, {}, 1, "1:2:3::5:6:7:8"},
  {F_IP, {}, 1, "1:2:3::8"},
  {F_IP, {}, 1, "1::4:5:6:7:8"},
  {F_IP, {}, 1, "1:2::4:5:6:7:8"},
  {F_IP, {}, 1, "1:2::8"},
  {F_IP, {}, 1, "1::3:4:5:6:7:8"},
  {F_IP, {}, 1, "1::8"},
  {F_IP, {}, 1, "fe80::7:8%eth0"},
  {F_IP, {}, 1, "fe80::7:8%1"},
  {F_IP, {}, 1, "64:ff9b::192.0.2.33"},
  {F_IP, {}, 1, "0:0:0:0:0:0:10.0.0.1"},
  {F_IP, {}, 0, "abc"},
  {F_IP, {}, 0, "256.0.0.0"},
  {F_IP, {}, 0, "0.0.0.256"},
  {F_IP, {}, 0, "26.0.0.256"},
  {F_IP, {}, 0, "0200.200.200.200"},
  {F_IP, {}, 0, "200.0200.200.200"},
  {F_IP, {}, 0, "200.200.0200.200"},
  {F_IP, {}, 0, "200.200.200.0200"},
  {F_IP, {}, 0, "::banana"},
  {F_IP, {}, 0, "banana::"},
  {F_IP, {}, 0, "::1banana"},
  {F_IP, {}, 0, "::1::"},
  {F_IP, {}, 0, "1:"},
  {F_IP, {}, 0, ":1"},
  {F_IP, {}, 0, ":1:1:1::2"},
  {F_IP, {}, 0, "1:1:1:1:1:1:1:1:1:1:1:1:1:1:1:1"},
  {F_IP, {}, 0, "::11111"},
  {F_IP, {}, 0, "11111:1:1:1:1:1:1:1"},
  {F_IP, {}, 0, "2001:db8:0000:1:1:1:1::1"},
  {F_IP, {}, 0, "0:0:0:0:0:0:ffff:127.0.0.1"},
  {F_IP, {}, 0, "0:0:0:0:ffff:127.0.0.1"},
  {F_IP, {.version = 4}, 1, "127.0.0.1"},
  {F_IP, {.version = 4}, 1, "0.0.0.0"},
  {F_IP, {.version = 4}, 1, "255.255.255.255"},
  {F_IP, {.version = 4}, 1, "1.2.3.4"},
  {F_IP, {.version = 4}, 1, "255.0.0.1"},
  {F_IP, {.version = 4}, 1, "0.0.1.1"},
  {F_IP, {.version = 4}, 0, "::1"},
  {F_IP, {.version = 4}, 0, "2001:db8:0000:1:1:1:1:1"},
  {F_IP, {.version = 4}, 0, "::ffff:127.0.0.1"},
  {F_IP, {.version = 4}, 0, "137.132.10.01"},
  {F_IP, {.version = 4}, 0, "0.256.0.256"},
  {F_IP, {.version = 4}, 0, "255.256.255.256"},
  {F_IP, {.version = 6}, 1, "::1"},
  {F_IP, {.version = 6}, 1, "2001:db8:0000:1:1:1:1:1"},
  {F_IP, {.version = 6}, 1, "::ffff:127.0.0.1"},
  {F_IP, {.version = 6}, 1, "fe80::1234%1"},
  {F_IP, {.version = 6}, 1, "ff08::9abc%10"},
  {F_IP, {.version = 6}, 1, "ff08::9abc%interface10"},
  {F_IP, {.version = 6}, 1, "ff02::5678%pvc1.3"},
  {F_IP, {.version = 6}, 0, "127.0.0.1"},
  {F_IP, {.version = 6}, 0, "0.0.0.0"},
  {F_IP, {.version = 6}, 0, "255.255.255.255"},
  {F_IP, {.version = 6}, 0, "1.2.3.4"},
  {F_IP, {.version = 6}, 0, "::ffff:287.0.0.1"},
  {F_IP, {.version = 6}, 0, "%"},
  {F_IP, {.version = 6}, 0, "fe80::1234%"},
  {F_IP, {.version = 6}, 0, "fe80::1234%1%3%4"},
  {F_IP, {.version = 6}, 0, "fe80%fe80%"},
};

#define NCASES ((int)(sizeof CASES / sizeof CASES[0]))

int main(void) {
  int assertions = 0;
  double t0 = now_ms();
  for (int it = 0; it < ITERATIONS; it++) {
    assertions = 0;
    for (int k = 0; k < NCASES; k++) {
      const Case *c = &CASES[k];
      int got = check(c) != 0;
      assertions++;
      if (got != c->expect) {
        if (failures < 10)
          fprintf(stderr, "validatorjs: case %d (%s) expected %d\n", k, c->s, c->expect);
        failures++;
      }
    }
  }
  double ms = now_ms() - t0;
  printf("validatorjs %d iterations %.3f ms assertions %d %s\n", ITERATIONS, ms, assertions,
         failures ? "FAILED" : "ok");
  return failures ? 1 : 0;
}
