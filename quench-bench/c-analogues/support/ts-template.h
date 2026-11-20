/*
 * Template literal types and string mapping types (TypeScript 5.9.2 checker.ts: getTypeFromTemplateTypeNode,
 * getTemplateLiteralType, addSpans, getStringMappingType, getStringMappingTypeForGenericType; line numbers in
 * lib/typescript.js). Included by ts-types.h after the generic checks (cIsGenericIndex uses the pattern test).
 * PORTED: template literal types of literal, null, undefined, boolean and template spans (flattened as
 * addSpans does), the union cross product, `string` for all-string spans, the template literal cache by the
 * type list and the texts; string mapping types (Uppercase, Lowercase, Capitalize, Uncapitalize) of string
 * literals, of template literal types and of generic types, with the generic string mapping cache.
 * Texts are the source slices: a template or string with an escape, a non-ASCII case mapping, a bigint or a
 * number that is not an integer makes the type unported (cUnported), as does a cross product above 100000.
 */
#ifndef TS_TEMPLATE_H
#define TS_TEMPLATE_H

/* the template literal types created (templateLiteralTypes), and the string mapping types (stringMappingTypes) */
typedef struct { TsType *tpl; } CTplEntry;
static TsType **cTplList;
static int cTplN, cTplCap;
/* the template literal types live in the checker arena, which every checker run resets */
typedef struct { TsSymbol *sym; TsType *type; TsType *res; } CSmEntry;
static CSmEntry *cSmList;
static int cSmN, cSmCap;
/* the template and string mapping types live in the checker arena: every checker run starts with none */
static void cResetTemplateTypes(void) { cTplN = 0; cSmN = 0; }

static char *cArenaCat(const char *a, int na, const char *b, int nb) {
  char *r = ts_arena_alloc((size_t)na + (size_t)nb + 1);
  if (na) memcpy(r, a, (size_t)na);
  if (nb) memcpy(r + na, b, (size_t)nb);
  r[na + nb] = 0;
  return r;
}

static bool cHasEscapeOrNonAscii(const char *s, int n) {
  for (int i = 0; i < n; i++) if (s[i] == '\\' || (unsigned char)s[i] >= 0x80) return true;
  return false;
}

/* isPatternLiteralPlaceholderType / isPatternLiteralType (66905, 66919) */
static bool cIsPatternLiteralType(TsType *t);
static bool cIsPatternLiteralPlaceholderType(TsType *t) {
  if (t->flags & TF_Intersection) {
    for (int i = 0; i < t->ntypes; i++) if (cIsPatternLiteralPlaceholderType(t->types[i])) return true;
    return false;
  }
  return (t->flags & (TF_Any | TF_String | TF_Number | TF_BigInt)) != 0 || cIsPatternLiteralType(t);
}
static bool cIsPatternLiteralType(TsType *t) {
  if (t->flags & TF_TemplateLiteral) {
    for (int i = 0; i < t->ntypes; i++) if (!cIsPatternLiteralPlaceholderType(t->types[i])) return false;
    return true;
  }
  if (t->flags & TF_StringMapping) return cIsPatternLiteralPlaceholderType(t->smType);
  return false;
}

/* getTemplateStringForType (66604): the text a literal type contributes to a template */
static bool cTemplateStringForType(TsType *t, CStr *out) {
  if (t->flags & TF_StringLiteral) {
    if (cHasEscapeOrNonAscii(t->text, t->textLen)) { cUnported("template text with an escape"); return false; }
    out->s = t->text;
    out->n = t->textLen;
    return true;
  }
  if (t->flags & TF_NumberLiteral) {
    double v = t->litNum;
    if (v != floor(v) || fabs(v) >= 1e21) { cUnported("template text of a non-integer number"); return false; }
    char *b = ts_arena_alloc(32);
    out->n = snprintf(b, 32, "%.0f", v);
    out->s = b;
    return true;
  }
  if (t == cTrueType || t == cFalseType) {
    out->s = t == cTrueType ? "true" : "false";
    out->n = t == cTrueType ? 4 : 5;
    return true;
  }
  if (t->flags & TF_Null) { out->s = "null"; out->n = 4; return true; }
  if (t->flags & TF_Undefined) { out->s = "undefined"; out->n = 9; return true; }
  cUnported("template text of a bigint literal");
  return false;
}

static TsType *cGetTemplateLiteralType(CStr *texts, TsType **types, int n);

/* addSpans (66571): the spans of a template type, flattened; the generic ones are collected with their texts */
typedef struct {
  char *text; int tlen;
  TsType **newTypes; CStr *newTexts; int nn, cap;
} CSpanState;

static void cSpanPush(CSpanState *st, TsType *t, const char *s, int n) {
  if (st->nn == st->cap) {
    int nc = st->cap ? st->cap * 2 : 4;
    TsType **nt = ts_arena_alloc(sizeof(TsType *) * (size_t)nc);
    CStr *ns = ts_arena_alloc(sizeof(CStr) * (size_t)(nc + 1));
    if (st->nn) { memcpy(nt, st->newTypes, sizeof(TsType *) * (size_t)st->nn); memcpy(ns, st->newTexts, sizeof(CStr) * (size_t)st->nn); }
    st->newTypes = nt;
    st->newTexts = ns;
    st->cap = nc;
  }
  st->newTypes[st->nn] = t;
  st->newTexts[st->nn].s = s;
  st->newTexts[st->nn].n = n;
  st->nn++;
}

static bool cAddSpans(CStr *texts, TsType **types, int n, CSpanState *st) {
  for (int i = 0; i < n; i++) {
    TsType *t = types[i];
    if (t->flags & (TF_StringLiteral | TF_NumberLiteral | TF_BigIntLiteral | TF_BooleanLiteral | TF_Null | TF_Undefined)) {
      CStr s;
      if (!cTemplateStringForType(t, &s)) return false;
      st->text = cArenaCat(st->text, st->tlen, s.s, s.n);
      st->tlen += s.n;
      st->text = cArenaCat(st->text, st->tlen, texts[i + 1].s, texts[i + 1].n);
      st->tlen += texts[i + 1].n;
    } else if (t->flags & TF_TemplateLiteral) {
      st->text = cArenaCat(st->text, st->tlen, t->tlTexts[0].s, t->tlTexts[0].n);
      st->tlen += t->tlTexts[0].n;
      if (!cAddSpans(t->tlTexts, t->types, t->ntypes, st)) return false;
      st->text = cArenaCat(st->text, st->tlen, texts[i + 1].s, texts[i + 1].n);
      st->tlen += texts[i + 1].n;
    } else if (cIsGenericIndex(t) || cIsPatternLiteralPlaceholderType(t)) {
      cSpanPush(st, t, st->text, st->tlen);
      st->text = (char *)texts[i + 1].s;   /* the text restarts after this span (not copied) */
      st->tlen = texts[i + 1].n;
    } else {
      return false;
    }
  }
  return true;
}

/* getTemplateLiteralType (66551): texts has n + 1 entries for the n types */
static TsType *cGetTemplateLiteralType(CStr *texts, TsType **types, int n) {
  int ui = -1;
  for (int i = 0; i < n && ui < 0; i++) if (types[i]->flags & (TF_Never | TF_Union)) ui = i;
  if (ui >= 0) {
    if (types[ui]->flags & TF_Never) return cNeverType;   /* mapType of never */
    TsType *u = types[ui];
    long prod = 1;
    for (int i = 0; i < n; i++) prod *= (types[i]->flags & TF_Union) ? types[i]->ntypes : 1;
    if (prod > 100000) return cUnported("template literal cross product too large");
    TsType **parts = ts_arena_alloc(sizeof(TsType *) * (size_t)(u->ntypes ? u->ntypes : 1));
    for (int k = 0; k < u->ntypes; k++) {
      TsType **rep = cCopyTypes(types, n);
      rep[ui] = u->types[k];
      parts[k] = cGetTemplateLiteralType(texts, rep, n);
    }
    return cGetUnionType(parts, u->ntypes);
  }
  for (int i = 0; i < n; i++) if (types[i] == cErrorType) return cErrorType;
  CSpanState st;
  memset(&st, 0, sizeof st);
  st.text = (char *)texts[0].s;
  st.tlen = texts[0].n;
  if (!cAddSpans(texts, types, n, &st)) return cStringType;
  if (st.nn == 0) return cLiteralType(TF_StringLiteral, st.text, st.tlen);
  /* the last text */
  CStr *newTexts = ts_arena_alloc(sizeof(CStr) * (size_t)(st.nn + 1));
  if (st.nn) memcpy(newTexts, st.newTexts, sizeof(CStr) * (size_t)st.nn);
  newTexts[st.nn].s = st.text;
  newTexts[st.nn].n = st.tlen;
  bool allEmpty = true;
  for (int i = 0; i <= st.nn; i++) if (newTexts[i].n) allEmpty = false;
  if (allEmpty) {
    bool allString = true;
    for (int i = 0; i < st.nn; i++) if (!(st.newTypes[i]->flags & TF_String)) allString = false;
    if (allString) return cStringType;
    if (st.nn == 1 && cIsPatternLiteralType(st.newTypes[0])) return st.newTypes[0];
  }
  /* templateLiteralTypes: the same types and texts give the same type */
  for (int k = 0; k < cTplN; k++) {
    TsType *e = cTplList[k];
    if (e->ntypes != st.nn) continue;
    bool same = true;
    for (int i = 0; i < st.nn && same; i++) same = e->types[i] == st.newTypes[i];
    for (int i = 0; i <= st.nn && same; i++)
      same = e->tlTexts[i].n == newTexts[i].n && memcmp(e->tlTexts[i].s, newTexts[i].s, (size_t)newTexts[i].n) == 0;
    if (same) return e;
  }
  TsType *t = cNewType(TF_TemplateLiteral);
  t->tlTexts = newTexts;
  t->types = cCopyTypes(st.newTypes, st.nn);
  t->ntypes = st.nn;
  if (cTplN == cTplCap) {
    cTplCap = cTplCap ? cTplCap * 2 : 16;
    cTplList = realloc(cTplList, sizeof(TsType *) * (size_t)cTplCap);
    if (!cTplList) abort();
  }
  cTplList[cTplN++] = t;
  return t;
}

/* getTypeFromTemplateTypeNode (66541) */
static TsType *cGetTypeFromTemplateTypeNode(TsNode *n) {
  int ns = n->spans ? n->spans->n : 0;
  CStr *texts = ts_arena_alloc(sizeof(CStr) * (size_t)(ns + 1));
  TsType **types = ts_arena_alloc(sizeof(TsType *) * (size_t)(ns ? ns : 1));
  texts[0].s = n->head->text;
  texts[0].n = n->head->textLen;
  for (int i = 0; i < ns; i++) {
    TsNode *sp = n->spans->v[i];
    texts[i + 1].s = sp->literal->text;
    texts[i + 1].n = sp->literal->textLen;
    types[i] = cTypeFromNode(sp->type);
    if (types[i] == cErrorType) return cErrorType;
  }
  for (int i = 0; i <= ns; i++) if (cHasEscapeOrNonAscii(texts[i].s, texts[i].n)) return cUnported("template type with an escape");
  return cGetTemplateLiteralType(texts, types, ns);
}

/* the ASCII case mappings of applyStringMapping: 0 Uppercase, 1 Lowercase, 2 Capitalize, 3 Uncapitalize */
static int cStringMappingKind(TsSymbol *sym) {
  BName n = sym->name;
  if (n.n == 9 && memcmp(n.s, "Uppercase", 9) == 0) return 0;
  if (n.n == 9 && memcmp(n.s, "Lowercase", 9) == 0) return 1;
  if (n.n == 10 && memcmp(n.s, "Capitalize", 10) == 0) return 2;
  if (n.n == 12 && memcmp(n.s, "Uncapitalize", 12) == 0) return 3;
  return -1;
}

static CStr cMapCase(CStr s, int kind) {
  CStr r;
  char *b = ts_arena_alloc((size_t)s.n + 1);
  for (int i = 0; i < s.n; i++) {
    char c = s.s[i];
    bool upper = (kind == 0) || (kind == 2 && i == 0);
    bool lower = (kind == 1) || (kind == 3 && i == 0);
    if (upper && c >= 'a' && c <= 'z') c = (char)(c - 32);
    if (lower && c >= 'A' && c <= 'Z') c = (char)(c + 32);
    b[i] = c;
  }
  b[s.n] = 0;
  r.s = b;
  r.n = s.n;
  return r;
}

static TsType *cGetStringMappingTypeForGenericType(TsSymbol *sym, TsType *type);

/* getStringMappingType (66613) */
static TsType *cGetStringMappingType(TsSymbol *sym, TsType *type) {
  if (type->flags & (TF_Union | TF_Never)) {
    if (type->flags & TF_Never) return type;
    TsType **parts = ts_arena_alloc(sizeof(TsType *) * (size_t)(type->ntypes ? type->ntypes : 1));
    for (int i = 0; i < type->ntypes; i++) parts[i] = cGetStringMappingType(sym, type->types[i]);
    return cGetUnionType(parts, type->ntypes);
  }
  int kind = cStringMappingKind(sym);
  if (kind < 0) return cUnported("string mapping of another intrinsic");
  if (type->flags & TF_StringLiteral) {
    if (cHasEscapeOrNonAscii(type->text, type->textLen)) return cUnported("string mapping of a text with an escape");
    CStr s = { type->text, type->textLen };
    CStr m = cMapCase(s, kind);
    return cLiteralType(TF_StringLiteral, m.s, m.n);
  }
  if (type->flags & TF_TemplateLiteral) {
    /* applyTemplateStringMapping */
    int n = type->ntypes;
    CStr *texts = ts_arena_alloc(sizeof(CStr) * (size_t)(n + 1));
    TsType **types = ts_arena_alloc(sizeof(TsType *) * (size_t)(n ? n : 1));
    for (int i = 0; i <= n; i++) texts[i] = (kind == 0 || kind == 1) ? cMapCase(type->tlTexts[i], kind) : type->tlTexts[i];
    for (int i = 0; i < n; i++) types[i] = cGetStringMappingType(sym, type->types[i]);
    if (kind == 2 || kind == 3) {
      if (type->tlTexts[0].n) {
        texts[0] = cMapCase(type->tlTexts[0], kind);
      } else {
        /* the first type takes the mapping instead of the empty first text */
        if (n) types[0] = cGetStringMappingType(sym, type->types[0]);
      }
    }
    return cGetTemplateLiteralType(texts, types, n);
  }
  if ((type->flags & TF_StringMapping) && sym == type->symbol) return type;
  if ((type->flags & (TF_Any | TF_String | TF_StringMapping)) || cIsGenericIndex(type))
    return cGetStringMappingTypeForGenericType(sym, type);
  if (cIsPatternLiteralPlaceholderType(type)) {
    CStr e[2] = { { "", 0 }, { "", 0 } };
    return cGetStringMappingTypeForGenericType(sym, cGetTemplateLiteralType(e, &type, 1));
  }
  return type;
}

/* getStringMappingTypeForGenericType (66648) with createStringMappingType (66656) */
static TsType *cGetStringMappingTypeForGenericType(TsSymbol *sym, TsType *type) {
  for (int i = 0; i < cSmN; i++)
    if (cSmList[i].sym == sym && cSmList[i].type == type) return cSmList[i].res;
  TsType *r = cNewType(TF_StringMapping);
  r->symbol = sym;
  r->smType = type;
  if (cSmN == cSmCap) {
    cSmCap = cSmCap ? cSmCap * 2 : 16;
    cSmList = realloc(cSmList, sizeof(CSmEntry) * (size_t)cSmCap);
    if (!cSmList) abort();
  }
  cSmList[cSmN].sym = sym;
  cSmList[cSmN].type = type;
  cSmList[cSmN].res = r;
  cSmN++;
  return r;
}

#endif
