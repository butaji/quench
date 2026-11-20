/*
 * Checker port (TypeScript 5.9.2 checker.ts, lib/typescript.js line numbers in comments) for the
 * typescript-lib analogue. The checker is ported function by function as far as the immer-tiny
 * program needs it; ts-checker.h says exactly what is ported and what is not.
 *
 * PORTED (step a, program initialization):
 *   - createSymbol / cloneSymbol / recordMergedSymbol / getMergedSymbol with the transient flag and
 *     the checker symbol counter (checker symbolCount);
 *   - getExcludedSymbolFlags, mergeSymbol, reportMergeSymbolError, mergeSymbolTable with the
 *     amalgamated duplicate-identifier pass of initializeTypeChecker (6200 diagnostics for files
 *     with 8 or more conflicting names, per-name 2300/2451 otherwise);
 *   - initializeTypeChecker: global-script symbol tables merged into globals in program order,
 *     `declare global` augmentations (moduleAugmentations of top-level global augmentations),
 *     UMD globalExports, and the intrinsic symbols created by createTypeChecker.
 * Program order is the order of program.getSourceFiles() (see ts-checker-expected.h).
 */
#ifndef TS_CHECKER_H
#define TS_CHECKER_H

#include <stdio.h>
#include "ts-binder.h"

#define SF_Transient 33554432u   /* SymbolFlags.Transient */
#define SF_Assignment_ 67108864u /* SymbolFlags.Assignment */

/* ---- diagnostics (createDiagnosticForNode / diagnostics.add) ---- */
typedef struct CDiag {
  TsNode *file;     /* SourceFile */
  int32_t start;    /* after trivia, as createDiagnosticForNode computes it */
  int code;
  char *msg;
} CDiag;

static CDiag *cDiags;
static int cDiagCount, cDiagCap;

/* a line that holds a `// @ts-ignore` or `// @ts-expect-error` comment (commentDirectiveRegExpSingleLine) */
static bool cLineHasDirective(const char *t, int32_t s, int32_t e) {
  for (int32_t i = s; i + 1 < e; i++) {
    if (t[i] != '/' || t[i + 1] != '/') continue;
    int32_t j = i + 2;
    if (j < e && t[j] == '/') j++;
    while (j < e && (t[j] == ' ' || t[j] == '\t')) j++;
    if (j < e && t[j] == '@') {
      if (e - j >= 10 && memcmp(t + j, "@ts-ignore", 10) == 0) return true;
      if (e - j >= 16 && memcmp(t + j, "@ts-expect-error", 16) == 0) return true;
    }
    return false;
  }
  return false;
}

/* markPrecedingCommentDirectiveLine: a directive on the lines above the diagnostic's line, through blank and
 * comment-only lines, suppresses the diagnostic */
static bool cSuppressedByDirective(TsNode *file, int32_t start) {
  const char *t = file->text;
  int32_t cur = start;
  while (cur > 0 && t[cur - 1] != '\n' && t[cur - 1] != '\r') cur--;
  while (cur > 0) {
    int32_t ce = cur - 1;                       /* the line break before cur: content ends before it */
    if (ce > 0 && t[ce] == '\n' && t[ce - 1] == '\r') ce--;
    int32_t s = ce;
    while (s > 0 && t[s - 1] != '\n' && t[s - 1] != '\r') s--;
    if (cLineHasDirective(t, s, ce)) return true;
    int32_t j = s;
    while (j < ce && (t[j] == ' ' || t[j] == '\t')) j++;
    if (j < ce && !(t[j] == '/' && j + 1 < ce && t[j + 1] == '/')) return false;
    cur = s;
  }
  return false;
}

static void cDiagAdd(TsNode *file, int32_t start, int code, char *msg) {
  if (file && cSuppressedByDirective(file, start)) return;
  if (cDiagCount == cDiagCap) {
    cDiagCap = cDiagCap ? cDiagCap * 2 : 256;
    cDiags = realloc(cDiags, sizeof(CDiag) * (size_t)cDiagCap);
    if (!cDiags) abort();
  }
  cDiags[cDiagCount].file = file;
  cDiags[cDiagCount].start = start;
  cDiags[cDiagCount].code = code;
  cDiags[cDiagCount].msg = msg;
  cDiagCount++;
}

/* ---- source file helpers ---- */
static TsNode *cSourceFileOf(TsNode *n) {
  while (n && n->kind != TSK_SourceFile) n = n->parent;
  return n;
}

/* skipTrivia: whitespace, line terminators, comments; a BOM and a shebang at the start */
static int32_t cSkipTrivia(const char *t, int len, int32_t pos) {
  if (pos == 0 && len >= 3 && (unsigned char)t[0] == 0xEF && (unsigned char)t[1] == 0xBB && (unsigned char)t[2] == 0xBF) pos = 3;
  if (pos == 0 && len >= 2 && t[0] == '#' && t[1] == '!') {
    while (pos < len && t[pos] != '\n' && t[pos] != '\r') pos++;
  }
  while (pos < len) {
    char c = t[pos];
    if (c == ' ' || c == '\t' || c == '\v' || c == '\f' || c == '\n' || c == '\r') { pos++; continue; }
    if (c == '/' && pos + 1 < len && t[pos + 1] == '/') {
      pos += 2;
      while (pos < len && t[pos] != '\n' && t[pos] != '\r') pos++;
      continue;
    }
    if (c == '/' && pos + 1 < len && t[pos + 1] == '*') {
      pos += 2;
      while (pos < len && !(t[pos] == '*' && pos + 1 < len && t[pos + 1] == '/')) pos++;
      pos = pos < len ? pos + 2 : len;
      continue;
    }
    break;
  }
  return pos;
}

static int32_t cNodeStart(TsNode *n) {
  TsNode *sf = cSourceFileOf(n);
  return cSkipTrivia(sf->text, sf->textLen, n->pos);
}

/* getLineAndCharacterOfPosition: 1-based line, 1-based column counted in UTF-16 units */
static void cLineCol(TsNode *sf, int32_t pos, int *line, int *col) {
  const char *t = sf->text;
  int ln = 1, lineStart = 0;
  for (int i = 0; i < pos; i++) {
    if (t[i] == '\n') { ln++; lineStart = i + 1; }
    else if (t[i] == '\r') {
      if (i + 1 < pos && t[i + 1] == '\n') continue;
      ln++; lineStart = i + 1;
    }
  }
  int units = 0;
  for (int i = lineStart; i < pos; i++) {
    unsigned char c = (unsigned char)t[i];
    if ((c & 0xC0) == 0x80) continue;             /* continuation byte */
    units += (c >= 0xF0) ? 2 : 1;                 /* 4-byte sequences are a surrogate pair */
  }
  *line = ln;
  *col = units + 1;
}

/* ---- symbols ---- */
static int cSymbolCount;            /* checker symbolCount: every createSymbol call */
static TsSymTable *cGlobals;        /* globals */
static TsSymbol *cGlobalThisSymbol; /* globalThisSymbol */
static TsSymbol *cUndefinedSymbol;  /* undefinedSymbol */
static TsSymbol *cArgumentsSymbol;  /* argumentsSymbol */

/* createSymbol (52194): a checker symbol is transient */
static TsSymbol *cCreateSymbol(uint32_t flags, BName name) {
  cSymbolCount++;
#ifdef TS_CHECKER_DEBUG
  fprintf(stderr, "CSYM %d %u %.*s\n", cSymbolCount, flags, name.s ? name.n : 9, name.s ? name.s : "(undef)");
#endif
  TsSymbol *s = ts_arena_zalloc(sizeof(TsSymbol));
  s->flags = flags | SF_Transient;
  s->name = name;
  return s;
}

/* getExcludedSymbolFlags (52211) */
static uint32_t cExcludedFlags(uint32_t f) {
  uint32_t r = 0;
  if (f & SF_BlockScopedVariable) r |= EX_BlockScopedVariableExcludes;
  if (f & SF_FunctionScopedVariable) r |= EX_FunctionScopedVariableExcludes;
  if (f & SF_Property) r |= EX_PropertyExcludes;
  if (f & SF_EnumMember) r |= EX_EnumMemberExcludes;
  if (f & SF_Function) r |= EX_FunctionExcludes;
  if (f & SF_Class) r |= EX_ClassExcludes;
  if (f & SF_Interface) r |= EX_InterfaceExcludes;
  if (f & SF_RegularEnum) r |= EX_RegularEnumExcludes;
  if (f & SF_ConstEnum) r |= EX_ConstEnumExcludes;
  if (f & SF_ValueModule) r |= EX_ValueModuleExcludes;
  if (f & SF_Method) r |= EX_MethodExcludes;
  if (f & SF_GetAccessor) r |= EX_GetAccessorExcludes;
  if (f & SF_SetAccessor) r |= EX_SetAccessorExcludes;
  if (f & SF_TypeParameter) r |= EX_TypeParameterExcludes;
  if (f & SF_TypeAlias) r |= EX_TypeAliasExcludes;
  if (f & SF_Alias) r |= EX_AliasExcludes;
  return r;
}

/* recordMergedSymbol (52231): source remembers the symbol it was merged into */
static void cRecordMergedSymbol(TsSymbol *target, TsSymbol *source) { source->mergedInto = target; }

/* getMergedSymbol (54467) */
static TsSymbol *cGetMergedSymbol(TsSymbol *s) { return s && s->mergedInto ? s->mergedInto : s; }

static void cAddDeclRaw(TsSymbol *s, TsNode *node) {
  if (s->ndecls == s->declCap) {
    int cap = s->declCap ? s->declCap * 2 : 4;
    TsNode **d = ts_arena_alloc(sizeof(TsNode *) * (size_t)cap);
    if (s->ndecls) memcpy(d, s->decls, sizeof(TsNode *) * (size_t)s->ndecls);
    s->decls = d;
    s->declCap = cap;
  }
  s->decls[s->ndecls++] = node;
}

static TsSymTable *cTableCopy(TsSymTable *src) {
  TsSymTable *t = bTableNew();
  for (int i = 0; i < src->n; i++) bTableSet(t, src->e[i].key, src->e[i].sym);
  return t;
}

/* cloneSymbol (52238) */
static TsSymbol *cCloneSymbol(TsSymbol *sym) {
  TsSymbol *result = cCreateSymbol(sym->flags, sym->name);
  for (int i = 0; i < sym->ndecls; i++) cAddDeclRaw(result, sym->decls[i]);
  result->parent = sym->parent;
  if (sym->valueDeclaration) result->valueDeclaration = sym->valueDeclaration;
  if (sym->constEnumOnlyModule) result->constEnumOnlyModule = true;
  if (sym->members) result->members = cTableCopy(sym->members);
  if (sym->exports) result->exports = cTableCopy(sym->exports);
  cRecordMergedSymbol(result, sym);
  return result;
}

/* setValueDeclaration (18825) for TypeScript declarations */
static void cSetValueDeclaration(TsSymbol *sym, TsNode *node) { bSetValueDeclaration(sym, node); }

/* ---- duplicate-identifier bookkeeping (amalgamatedDuplicates, 52290-52330) ---- */
typedef struct CConflict {
  BName name;
  bool isBlockScoped;
  TsNode **firstLocs, **secondLocs;
  int nFirst, capFirst, nSecond, capSecond;
} CConflict;

typedef struct CDupPair {
  TsNode *firstFile, *secondFile;
  CConflict **conf;
  int nConf, capConf;
} CDupPair;

static CDupPair **cDupPairs;
static int cNDupPairs, cCapDupPairs;
static bool cAmalgamate;            /* amalgamatedDuplicates is set during initializeTypeChecker */

static void cPushUnique(TsNode ***v, int *n, int *cap, TsNode *node) {
  for (int i = 0; i < *n; i++) if ((*v)[i] == node) return;
  if (*n == *cap) {
    int nc = *cap ? *cap * 2 : 4;
    TsNode **nv = ts_arena_alloc(sizeof(TsNode *) * (size_t)nc);
    if (*n) memcpy(nv, *v, sizeof(TsNode *) * (size_t)*n);
    *v = nv;
    *cap = nc;
  }
  (*v)[(*n)++] = node;
}

static void cAddDuplicateLocations(TsNode ***locs, int *n, int *cap, TsSymbol *sym) {
  for (int i = 0; i < sym->ndecls; i++) cPushUnique(locs, n, cap, sym->decls[i]);
}

static TsNode *cFileOfSymbolDecl(TsSymbol *sym) { return sym->ndecls ? cSourceFileOf(sym->decls[0]) : NULL; }

static CDupPair *cDupPairFor(TsNode *first, TsNode *second) {
  for (int i = 0; i < cNDupPairs; i++)
    if (cDupPairs[i]->firstFile == first && cDupPairs[i]->secondFile == second) return cDupPairs[i];
  if (cNDupPairs == cCapDupPairs) {
    cCapDupPairs = cCapDupPairs ? cCapDupPairs * 2 : 8;
    cDupPairs = realloc(cDupPairs, sizeof(CDupPair *) * (size_t)cCapDupPairs);
    if (!cDupPairs) abort();
  }
  CDupPair *p = calloc(1, sizeof(CDupPair));
  p->firstFile = first;
  p->secondFile = second;
  cDupPairs[cNDupPairs++] = p;
  return p;
}

static CConflict *cConflictFor(CDupPair *p, BName name, bool isBlockScoped) {
  for (int i = 0; i < p->nConf; i++)
    if (bnEq(p->conf[i]->name, name)) return p->conf[i];
  if (p->nConf == p->capConf) {
    p->capConf = p->capConf ? p->capConf * 2 : 16;
    p->conf = realloc(p->conf, sizeof(CConflict *) * (size_t)p->capConf);
    if (!p->conf) abort();
  }
  CConflict *c = calloc(1, sizeof(CConflict));
  c->name = name;
  c->isBlockScoped = isBlockScoped;
  p->conf[p->nConf++] = c;
  return c;
}

/* Map keyed by file pair: a pair is first when its path compares less (comparePaths, ordinal here) */
static int cPathCmp(TsNode *a, TsNode *b) { return strcmp(a->fileName, b->fileName); }

/* addDuplicateDeclarationError (52326 region): the error is on the name of the declaration */
static void cAddDuplicateDeclarationError(TsNode *node, int code, const char *symName, int nRelated) {
  (void)nRelated;
  TsNode *errNode = node->name ? node->name : node;
  char *msg = NULL;
  (void)symName;
  cDiagAdd(cSourceFileOf(errNode), cNodeStart(errNode), code, msg);
}

/* reportMergeSymbolError (inside mergeSymbol, 52286) */
static void cReportMergeSymbolError(TsSymbol *target, TsSymbol *source) {
  bool isEitherEnum = (target->flags & SF_Enum) || (source->flags & SF_Enum);
  bool isEitherBlockScoped = (target->flags & SF_BlockScopedVariable) || (source->flags & SF_BlockScopedVariable);
  int code = isEitherEnum ? 2567 : isEitherBlockScoped ? 2451 : 2300;
  TsNode *sourceFile = source->ndecls ? cFileOfSymbolDecl(source) : NULL;
  TsNode *targetFile = target->ndecls ? cFileOfSymbolDecl(target) : NULL;
  if (sourceFile && targetFile && cAmalgamate && !isEitherEnum && sourceFile != targetFile) {
    TsNode *first = cPathCmp(sourceFile, targetFile) < 0 ? sourceFile : targetFile;
    TsNode *second = first == sourceFile ? targetFile : sourceFile;
    CDupPair *p = cDupPairFor(first, second);
    CConflict *c = cConflictFor(p, source->name, isEitherBlockScoped);
    cAddDuplicateLocations(&c->firstLocs, &c->nFirst, &c->capFirst, source);
    cAddDuplicateLocations(&c->secondLocs, &c->nSecond, &c->capSecond, target);
  } else {
    for (int i = 0; i < target->ndecls; i++) cAddDuplicateDeclarationError(target->decls[i], code, "", source->ndecls);
    for (int i = 0; i < source->ndecls; i++) cAddDuplicateDeclarationError(source->decls[i], code, "", target->ndecls);
  }
}

/* mergeSymbol (52249) */
static TsSymbol *cMergeSymbol(TsSymbol *target, TsSymbol *source, bool unidirectional);

/* mergeSymbolTable (52360) */
static void cMergeSymbolTable(TsSymTable *target, TsSymTable *source, bool unidirectional, TsSymbol *mergedParent) {
  for (int i = 0; i < source->n; i++) {
    BName id = source->e[i].key;
    TsSymbol *sourceSymbol = source->e[i].sym;
    TsSymbol *targetSymbol = bTableGet(target, id);
    TsSymbol *merged = targetSymbol ? cMergeSymbol(targetSymbol, sourceSymbol, unidirectional) : cGetMergedSymbol(sourceSymbol);
    if (mergedParent && targetSymbol) merged->parent = mergedParent;
    bTableSet(target, id, merged);
  }
}

static TsSymbol *cMergeSymbol(TsSymbol *target, TsSymbol *source, bool unidirectional) {
  if (!(target->flags & cExcludedFlags(source->flags)) || ((source->flags | target->flags) & SF_Assignment_)) {
    if (source == target) return target;
    if (!(target->flags & SF_Transient)) {
      TsSymbol *resolvedTarget = target; /* resolveSymbol: aliases are not in the global scripts of this project */
      if (!(resolvedTarget->flags & cExcludedFlags(source->flags)) || ((source->flags | resolvedTarget->flags) & SF_Assignment_)) {
        target = cCloneSymbol(resolvedTarget);
      } else {
        cReportMergeSymbolError(target, source);
        return source;
      }
    }
    if ((source->flags & SF_ValueModule) && (target->flags & SF_ValueModule) && target->constEnumOnlyModule && !source->constEnumOnlyModule)
      target->constEnumOnlyModule = false;
    target->flags |= source->flags;
    if (source->valueDeclaration) cSetValueDeclaration(target, source->valueDeclaration);
    for (int i = 0; i < source->ndecls; i++) cAddDeclRaw(target, source->decls[i]);
    if (source->members) {
      if (!target->members) target->members = bTableNew();
      cMergeSymbolTable(target->members, source->members, unidirectional, NULL);
    }
    if (source->exports) {
      if (!target->exports) target->exports = bTableNew();
      cMergeSymbolTable(target->exports, source->exports, unidirectional, target);
    }
    if (!unidirectional) cRecordMergedSymbol(target, source);
  } else if (target->flags & SF_NamespaceModule) {
    /* Cannot_augment_module_0_with_value_exports_because_it_resolves_to_a_non_module_entity: not in this project */
    if (target != cGlobalThisSymbol) { fprintf(stderr, "checker: unported namespace augmentation\n"); abort(); }
  } else {
    cReportMergeSymbolError(target, source);
  }
  return target;
}

/* ---- initializeTypeChecker (93043), the part that merges globals ---- */
typedef struct CAugmentation { TsNode *name; } CAugmentation;   /* moduleAugmentations entries */

/* createTypeChecker intrinsic symbols, in creation order (51029-51037, 51547, 51699) */
static void cCreateIntrinsicSymbols(void) {
  cGlobals = bTableNew();
  cUndefinedSymbol = cCreateSymbol(SF_Property, bnC("undefined"));
  cGlobalThisSymbol = cCreateSymbol(SF_Module, bnC("globalThis"));
  cGlobalThisSymbol->checkFlags = 8; /* Readonly */
  cGlobalThisSymbol->exports = cGlobals;
  cGlobalThisSymbol->ndecls = 0;
  bTableSet(cGlobals, cGlobalThisSymbol->name, cGlobalThisSymbol);
  cArgumentsSymbol = cCreateSymbol(SF_Property, bnC("arguments"));
  cCreateSymbol(SF_Property, bnC("require"));
  cCreateSymbol(SF_Property, bnC("unknown"));
  cCreateSymbol(0, bnC("__resolving__"));
  cCreateSymbol(SF_TypeLiteral, bnC("__type"));
}

/* Module augmentations of one file: names of top-level `declare global` (the ones that merge into
 * globals; other ambient module augmentations are not in this project). */
static void cCollectGlobalAugmentations(TsNode *sf, CAugmentation **out, int *n, int *cap) {
  if (!sf->statements) return;
  for (int i = 0; i < sf->statements->n; i++) {
    TsNode *s = sf->statements->v[i];
    if (s->kind != TSK_ModuleDeclaration || !(s->flags & TNF_GlobalAugmentation)) continue;
    if (*n == *cap) {
      *cap = *cap ? *cap * 2 : 4;
      *out = realloc(*out, sizeof(CAugmentation) * (size_t)*cap);
    }
    (*out)[(*n)++].name = s->name;
  }
}

/* initializeTypeChecker (93043-93095): program files in order; file.locals of scripts into globals */
static void cInitializeGlobals(TsNode **files, int nFiles) {
  cSymbolCount = 0;
  cAmalgamate = true;
  cCreateIntrinsicSymbols();
  CAugmentation *aug = NULL;
  int nAug = 0, capAug = 0;
  for (int f = 0; f < nFiles; f++) {
    TsNode *file = files[f];
    if (!file->hasExternalModuleIndicator) {
      TsSymbol *fileGlobalThis = bTableGet(file->locals, bnC("globalThis"));
      if (fileGlobalThis && fileGlobalThis->ndecls) {
        for (int d = 0; d < fileGlobalThis->ndecls; d++)
          cDiagAdd(file, cNodeStart(fileGlobalThis->decls[d]->name), 2397, NULL); /* Declaration name conflicts with built-in global identifier 'globalThis' */
      }
      cMergeSymbolTable(cGlobals, file->locals, false, NULL);
    }
    cCollectGlobalAugmentations(file, &aug, &nAug, &capAug);
    if (file->symbol && file->symbol->globalExports) {
      TsSymTable *src = file->symbol->globalExports;
      for (int i = 0; i < src->n; i++)
        if (bTableIndex(cGlobals, src->e[i].key) < 0) bTableSet(cGlobals, src->e[i].key, src->e[i].sym);
    }
  }
  for (int a = 0; a < nAug; a++) {
    TsNode *moduleDecl = aug[a].name->parent;
    if (moduleDecl->symbol->ndecls && moduleDecl->symbol->decls[0] != moduleDecl) continue;
    cMergeSymbolTable(cGlobals, moduleDecl->symbol->exports, false, NULL);
  }
  cAmalgamate = false;
  /* addUndefinedToGlobalsOrErrorOnRedeclaration (52422) */
  if (bTableIndex(cGlobals, cUndefinedSymbol->name) < 0) bTableSet(cGlobals, cUndefinedSymbol->name, cUndefinedSymbol);
  free(aug);
}

/* The amalgamated pass at the end of the merge loop: one 6200 per file pair with eight or more
 * conflicting names, otherwise one error per conflicting declaration. */
static void cReportAmalgamatedDuplicates(void) {
  for (int p = 0; p < cNDupPairs; p++) {
    CDupPair *d = cDupPairs[p];
    if (d->nConf < 8) {
      for (int c = 0; c < d->nConf; c++) {
        CConflict *cf = d->conf[c];
        int code = cf->isBlockScoped ? 2451 : 2300;
        for (int i = 0; i < cf->nFirst; i++) cAddDuplicateDeclarationError(cf->firstLocs[i], code, "", cf->nSecond);
        for (int i = 0; i < cf->nSecond; i++) cAddDuplicateDeclarationError(cf->secondLocs[i], code, "", cf->nFirst);
      }
    } else {
      size_t len = 0;
      for (int c = 0; c < d->nConf; c++) len += d->conf[c]->name.n + 2;
      char *list = malloc(len + 1), *o = list;
      list[0] = 0;
      for (int c = 0; c < d->nConf; c++) {
        if (c) { *o++ = ','; *o++ = ' '; }
        memcpy(o, d->conf[c]->name.s, (size_t)d->conf[c]->name.n);
        o += d->conf[c]->name.n;
      }
      *o = 0;
      const char *prefix = "Definitions of the following identifiers conflict with those in another file: ";
      for (int k = 0; k < 2; k++) {
        TsNode *file = k == 0 ? d->firstFile : d->secondFile;
        char *msg = malloc(strlen(prefix) + strlen(list) + 1);
        strcpy(msg, prefix);
        strcat(msg, list);
        cDiagAdd(file, cSkipTrivia(file->text, file->textLen, 0), 6200, msg);
      }
      free(list);
    }
  }
}

#endif
