/*
 * Checker port, part 2: module resolution, export lookup and identifier resolution
 * (TypeScript 5.9.2 checker.ts / program.ts; line numbers in lib/typescript.js).
 *
 * PORTED:
 *   - resolveExternalModuleName / resolveExternalModule (54001-54150) for the classic module
 *     resolution that this project's options select (no moduleResolution, module ES6 -> Classic):
 *     ambient "name" modules first, then <dir>/<name>.ts|.tsx|.d.ts walking up the directories
 *     for non-relative names, and the 2792 error when nothing is found. The host file list is the
 *     program's file list (exact, case-sensitive names), as the harness's fileExists does.
 *   - getExportsOfModule (54403): own exports plus `export *` targets, the "default" export is not
 *     re-exported by a star, and a name already present wins.
 *   - getExternalModuleMember error path (53382-53440): 2305 "has no exported member", 2614 when the
 *     module has a default export, 2724 when a spelling suggestion exists (getSpellingSuggestion).
 *   - identifier resolution for value names (resolveName, 54800-54900, value meaning only):
 *     block and function locals, module exports, enum members, class expression and function
 *     expression names, `arguments`, then globals. Unresolved value references report the
 *     getCannotFindNameDiagnosticForName code (2304, or 2580 for node names).
 * NOT PORTED: type-meaning name resolution (type references are not resolved yet), ambient
 * pattern modules ("*.css" style), `export =` modules, and alias targets (resolveAlias).
 */
#ifndef TS_CHECKER_NAMES_H
#define TS_CHECKER_NAMES_H

#include <math.h>
#include <ctype.h>
#include "ts-checker.h"

/* program files in getSourceFiles() order (set by the driver before cInitializeGlobals) */
static TsNode **cFiles;
static int cFileCount;

#define SF_ModuleMember 2623475u /* SymbolFlags.ModuleMember */
#define SF_EnumMemberMask 8u

/* ---- path helpers (normalizePath, getDirectoryPath) ---- */
static bool cIsRelativeName(const char *s, int n) {
  if (n >= 1 && s[0] == '/') return true; /* isRootedDiskPath */
  if (n < 1 || s[0] != '.') return false;   /* pathIsRelative: ^\.\.?($|[\\/]) */
  if (n == 1) return true;
  if (s[1] == '/' || s[1] == '\\') return true;
  return s[1] == '.' && (n == 2 || s[2] == '/' || s[2] == '\\');
}

/* normalizePath(combinePaths(dir, name)) for '/'-separated relative paths; out has room for 4096 */
static void cCombineNormalize(const char *dir, const char *name, int nlen, char *out) {
  char buf[4096];
  int bl = 0;
  if (name[0] == '/') { memcpy(buf, name, (size_t)nlen); bl = nlen; }
  else {
    int dl = (int)strlen(dir);
    if (dl) { memcpy(buf, dir, (size_t)dl); bl = dl; buf[bl++] = '/'; }
    memcpy(buf + bl, name, (size_t)nlen);
    bl += nlen;
  }
  buf[bl] = 0;
  /* split on '/', drop '.', pop on '..' */
  const char *seg[512];
  int ns = 0;
  char *p = buf;
  bool rooted = buf[0] == '/';
  if (rooted) p++;
  while (*p) {
    char *e = strchr(p, '/');
    if (e) *e = 0;
    if (strcmp(p, "") != 0 && strcmp(p, ".") != 0) {
      if (strcmp(p, "..") == 0) { if (ns) ns--; }
      else seg[ns++] = p;
    }
    if (!e) break;
    p = e + 1;
  }
  int o = 0;
  if (rooted) out[o++] = '/';
  for (int i = 0; i < ns; i++) {
    if (i) out[o++] = '/';
    int l = (int)strlen(seg[i]);
    memcpy(out + o, seg[i], (size_t)l);
    o += l;
  }
  out[o] = 0;
}

/* exact, case-sensitive lookup in the program's file list (fileExists of the harness host) */
static TsNode *cFileByName(const char *path) {
  for (int i = 0; i < cFileCount; i++)
    if (strcmp(cFiles[i]->fileName, path) == 0) return cFiles[i];
  return NULL;
}

/* tryFile for the classic extensions .ts, .tsx, .d.ts */
static TsNode *cTryClassicFile(const char *base) {
  static const char *exts[] = { ".ts", ".tsx", ".d.ts" };
  char path[4200];
  for (int e = 0; e < 3; e++) {
    snprintf(path, sizeof path, "%s%s", base, exts[e]);
    TsNode *f = cFileByName(path);
    if (f) return f;
  }
  return NULL;
}

/* classicNameResolver: relative names from the containing directory; others walk up the directories.
 * A .js or .jsx extension is replaced by the TypeScript extensions (tryAddingExtensions). */
static TsNode *cResolveClassic(const char *containingFile, const char *name, int nlen) {
  char dir[4096];
  const char *slash = strrchr(containingFile, '/');
  if (slash) { memcpy(dir, containingFile, (size_t)(slash - containingFile)); dir[slash - containingFile] = 0; }
  else dir[0] = 0;
  if (nlen > 3 && memcmp(name + nlen - 3, ".js", 3) == 0) nlen -= 3;
  else if (nlen > 4 && memcmp(name + nlen - 4, ".jsx", 4) == 0) nlen -= 4;
  char cand[4096];
  if (cIsRelativeName(name, nlen)) {
    cCombineNormalize(dir, name, nlen, cand);
    return cTryClassicFile(cand);
  }
  for (;;) {
    cCombineNormalize(dir, name, nlen, cand);
    TsNode *f = cTryClassicFile(cand);
    if (f) return f;
    if (!dir[0]) break;
    char *s = strrchr(dir, '/');
    if (s) *s = 0;
    else dir[0] = 0;
  }
  return NULL;
}

/* ---- module names ---- */
static BName cQuotedName(const char *s, int n) {
  BName q = bnConcat("\"", 1, s, n);
  return bnConcat(q.s, q.n, "\"", 1);
}

/* resolveExternalModuleName (53996) / resolveExternalModule (54004). Reports 2792 when reportErrors. */
static TsSymbol *cResolveExternalModuleName(TsNode *spec, bool reportErrors) {
  if (!spec || (spec->kind != TSK_StringLiteral && spec->kind != TSK_NoSubstitutionTemplateLiteral)) return NULL;
  TsNode *file = cSourceFileOf(spec);
  if (!cIsRelativeName(spec->text, spec->textLen)) {
    /* tryFindAmbientModule: the globals entry "name" with ValueModule */
    TsSymbol *amb = bTableGet(cGlobals, cQuotedName(spec->text, spec->textLen));
    if (amb && (amb->flags & SF_ValueModule)) return cGetMergedSymbol(amb);
  }
  TsNode *target = cResolveClassic(file->fileName, spec->text, spec->textLen);
  if (target) {
    /* a script file is not a module: File_0_is_not_a_module (2306) is not produced by this project */
    return target->symbol ? cGetMergedSymbol(target->symbol) : NULL;
  }
  if (reportErrors) {
    cDiagAdd(file, cNodeStart(spec), 2792, NULL); /* Cannot_find_module_0_Did_you_mean_... (classic) */
  }
  return NULL;
}

/* ---- exports of a module (getExportsOfModuleWorker, 54403) ---- */
typedef struct { TsSymbol **v; int n, cap; } CSymVec;

static bool cSymVecHas(CSymVec *v, TsSymbol *s) {
  for (int i = 0; i < v->n; i++) if (v->v[i] == s) return true;
  return false;
}

static void cSymVecPush(CSymVec *v, TsSymbol *s) {
  if (v->n == v->cap) {
    v->cap = v->cap ? v->cap * 2 : 16;
    v->v = realloc(v->v, sizeof(TsSymbol *) * (size_t)v->cap);
    if (!v->v) abort();
  }
  v->v[v->n++] = s;
}

static bool cIsDefaultName(BName n) { return n.n == 7 && memcmp(n.s, "default", 7) == 0; }

/* extendExportSymbols (54381): a name already in the target wins; default is not re-exported */
static void cExtendExportSymbols(TsSymTable *target, TsSymTable *source) {
  if (!source) return;
  for (int i = 0; i < source->n; i++) {
    BName id = source->e[i].key;
    if (cIsDefaultName(id)) continue;
    if (bTableIndex(target, id) < 0) bTableSet(target, id, source->e[i].sym);
  }
}

static TsSymTable *cVisitExports(TsSymbol *sym, CSymVec *visited) {
  if (!sym || !sym->exports) return NULL;
  if (cSymVecHas(visited, sym)) return NULL;
  cSymVecPush(visited, sym);
  TsSymTable *symbols = cTableCopy(sym->exports);
  int starIdx = bTableIndex(sym->exports, bnC("__export"));
  if (starIdx >= 0) {
    TsSymbol *stars = sym->exports->e[starIdx].sym;
    TsSymTable *nested = bTableNew();
    for (int d = 0; d < stars->ndecls; d++) {
      TsSymbol *resolved = cResolveExternalModuleName(stars->decls[d]->moduleSpecifier, false);
      cExtendExportSymbols(nested, cVisitExports(resolved, visited));
    }
    cExtendExportSymbols(symbols, nested);
  }
  return symbols;
}

static TsSymTable *cGetExportsOfModule(TsSymbol *mod) {
  CSymVec visited = { NULL, 0, 0 };
  TsSymTable *t = cVisitExports(mod, &visited);
  free(visited.v);
  return t;
}

/* ---- spelling suggestions (getSpellingSuggestion, 3489-3560) ---- */
static double cLevenshteinWithMax(const char *s1, int n1, const char *s2, int n2, double max) {
  double big = max + 0.01;
  double *prev = malloc(sizeof(double) * (size_t)(n2 + 1)), *cur = malloc(sizeof(double) * (size_t)(n2 + 1));
  for (int i = 0; i <= n2; i++) prev[i] = i;
  for (int i = 1; i <= n1; i++) {
    unsigned char c1 = (unsigned char)s1[i - 1];
    int minJ = (int)ceil(i > max ? i - max : 1);
    int maxJ = (int)floor(n2 > max + i ? max + i : n2);
    cur[0] = i;
    double colMin = i;
    for (int j = 1; j < minJ; j++) cur[j] = big;
    for (int j = minJ; j <= maxJ; j++) {
      unsigned char a = (unsigned char)s1[i - 1], b = (unsigned char)s2[j - 1];
      double subst = (tolower(a) == tolower(b)) ? prev[j - 1] + 0.1 : prev[j - 1] + 2;
      double dist;
      if (c1 == b) dist = prev[j - 1];
      else {
        dist = prev[j] + 1;
        if (cur[j - 1] + 1 < dist) dist = cur[j - 1] + 1;
        if (subst < dist) dist = subst;
      }
      cur[j] = dist;
      if (dist < colMin) colMin = dist;
    }
    for (int j = maxJ + 1; j <= n2; j++) cur[j] = big;
    if (colMin > max) { free(prev); free(cur); return -1; }
    double *tmp = prev; prev = cur; cur = tmp;
  }
  double res = prev[n2];
  free(prev);
  free(cur);
  return res > max ? -1 : res;
}

/* returns the best candidate index or -1 */
static int cSpellingSuggestion(BName name, TsSymbol **cands, int nc) {
  double maxLenDiff = 2 > floor(name.n * 0.34) ? 2 : floor(name.n * 0.34);
  double bestDistance = floor(name.n * 0.4) + 1;
  int best = -1;
  for (int i = 0; i < nc; i++) {
    BName cn = cands[i]->name;
    if (!bnDef(cn)) continue;
    if (fabs((double)(cn.n - name.n)) > maxLenDiff) continue;
    if (bnEq(cn, name)) continue;
    if (cn.n < 3) {
      /* candidateName.toLowerCase() !== name.toLowerCase() for short names */
      bool same = cn.n == name.n;
      for (int k = 0; same && k < cn.n; k++) same = tolower((unsigned char)cn.s[k]) == tolower((unsigned char)name.s[k]);
      if (!same) continue;
    }
    double d = cLevenshteinWithMax(name.s, name.n, cn.s, cn.n, bestDistance - 0.1);
    if (d < 0) continue;
    bestDistance = d;
    best = i;
  }
  return best;
}

/* ---- import and export specifiers (checkImportBinding / checkExportSpecifier -> getExternalModuleMember) ---- */
static void cCheckModuleMember(TsNode *decl, TsNode *spec) {
  TsSymbol *mod = cResolveExternalModuleName(decl->moduleSpecifier, false);
  if (!mod) return;
  TsNode *name = spec->propertyName ? spec->propertyName : spec->name;
  BName nm = bnEscapeText(name->text, name->textLen); /* escapedText of the identifier */
  TsSymTable *ex = cGetExportsOfModule(mod);
  if (bTableIndex(ex, nm) >= 0) return;
#ifdef TS_CHECKER_DEBUG
  fprintf(stderr, "NOEXPORT %.*s in %s:", nm.n, nm.s, mod->ndecls ? mod->decls[0]->fileName : "?");
  for (int i = 0; i < ex->n; i++) fprintf(stderr, " %.*s", ex->e[i].key.n, ex->e[i].key.s);
  fprintf(stderr, "\n");
#endif
  /* getSuggestedSymbolForNonexistentModule: candidates are the module's member symbols */
  TsSymbol **cands = malloc(sizeof(TsSymbol *) * (size_t)(ex->n + 1));
  int nc = 0;
  for (int i = 0; i < ex->n; i++)
    if (ex->e[i].sym->flags & SF_ModuleMember) cands[nc++] = ex->e[i].sym;
  int sug = cSpellingSuggestion(nm, cands, nc);
  free(cands);
  TsNode *file = cSourceFileOf(name);
  if (sug >= 0) {
    cDiagAdd(file, cNodeStart(name), 2724, NULL);
  } else if (bTableIndex(mod->exports, bnC("default")) >= 0) {
    cDiagAdd(file, cNodeStart(name), 2614, NULL);
  } else {
    cDiagAdd(file, cNodeStart(name), 2305, NULL);
  }
}

/* ---- identifier resolution (value meaning) ---- */
static bool cIsFunctionLikeNode(TsNode *n) {
  return bIsFunctionLikeKind(n->kind);
}

/* the binder's value symbol for `name` as seen from location (resolveName, value meaning only) */
static TsSymbol *cResolveValueName(TsNode *loc, BName name) {
  for (TsNode *n = loc; n; n = n->parent) {
    if (n->locals && !(n->kind == TSK_SourceFile && !n->hasExternalModuleIndicator)) {
      TsSymbol *s = bTableGet(n->locals, name);
      /* an import alias resolves to its target, or to unknownSymbol (all meanings) when unresolved */
      if (s && (s->flags & (SF_Value | SF_Alias))) return s;
    }
    switch (n->kind) {
      case TSK_SourceFile:
        if (!n->hasExternalModuleIndicator) goto globals;
        /* fall through */
      case TSK_ModuleDeclaration: {
        TsSymbol *ms = cGetMergedSymbol(n->symbol);
        if (ms && ms->exports) {
          TsSymbol *s = bTableGet(ms->exports, name);
          if (s && (s->flags & SF_ModuleMember)) return s;
        }
        break;
      }
      case TSK_EnumDeclaration:
        if (cGetMergedSymbol(n->symbol) && cGetMergedSymbol(n->symbol)->exports) {
          TsSymbol *s = bTableGet(cGetMergedSymbol(n->symbol)->exports, name);
          if (s && (s->flags & SF_EnumMember)) return s;
        }
        break;
      case TSK_ClassExpression: case TSK_FunctionExpression:
        if (n->name && bnEq(bnEscapeText(n->name->text, n->name->textLen), name) && n->symbol) return n->symbol;
        break;
      default: break;
    }
    if (cIsFunctionLikeNode(n) && n->kind != TSK_ArrowFunction && bnEq(name, bnC("arguments"))) return cArgumentsSymbol;
  }
globals:
  {
    TsSymbol *s = bTableGet(cGlobals, name);
    if (s && (s->flags & SF_Value)) return s;
  }
  return NULL;
}

/* does this identifier refer to a value (expression reference), not a declaration or type name? */
static bool cIsValueReference(TsNode *id) {
  TsNode *p = id->parent;
  if (!p) return false;
  switch (p->kind) {
    case TSK_PropertyAccessExpression: return p->expression == id;
    case TSK_QualifiedName: case TSK_LabeledStatement: case TSK_BreakStatement: case TSK_ContinueStatement:
    case TSK_ImportSpecifier: case TSK_ExportSpecifier: case TSK_ImportClause: case TSK_NamespaceImport:
    case TSK_ImportEqualsDeclaration: case TSK_NamespaceExportDeclaration: case TSK_TypeReference:
    case TSK_TypeParameter: case TSK_JsxAttribute: case TSK_MetaProperty:
      return false;
    case TSK_ExpressionWithTypeArguments:
      /* extends of a class is a value reference; implements and interface extends are type references (not resolved yet) */
      return p->parent && p->parent->kind == TSK_HeritageClause && p->parent->extra == TSK_ExtendsKeyword &&
             p->parent->parent && (p->parent->parent->kind == TSK_ClassDeclaration || p->parent->parent->kind == TSK_ClassExpression);
    case TSK_ShorthandPropertyAssignment: return p->name == id;
    case TSK_PropertyAssignment: return p->initializer == id;
    default: break;
  }
  if (p->name == id || p->propertyName == id) return false;
  if (p->kind >= TSK_FirstTypeNode && p->kind <= TSK_LastTypeNode) return false;
  return true;
}

/* getCannotFindNameDiagnosticForName: the code for an unresolved value name */
static int cCannotFindNameCode(TsNode *id) {
  const char *s = id->text;
  int n = id->textLen;
  if ((n == 8 && memcmp(s, "document", 8) == 0) || (n == 7 && memcmp(s, "console", 7) == 0)) return 2584;
  if (n == 7 && memcmp(s, "process", 7) == 0) return 2580;
  if (n == 7 && memcmp(s, "require", 7) == 0) return 2580;
  if (n == 6 && memcmp(s, "Buffer", 6) == 0) return 2580;
  if (n == 6 && memcmp(s, "module", 6) == 0) return 2580;
  return 2304;
}

static void cCheckIdentifierReference(TsNode *id) {
  if (!id->textLen) return;
  if (!cIsValueReference(id)) return;
  BName nm = bnEscapeText(id->text, id->textLen);
  if (cResolveValueName(id, nm)) return;
  cDiagAdd(cSourceFileOf(id), cNodeStart(id), cCannotFindNameCode(id), NULL);
}

/* ---- declaration-level module checks, then a walk of every node ---- */
static void cCheckModuleDeclaration(TsNode *n) {
  switch (n->kind) {
    case TSK_ImportDeclaration:
      cResolveExternalModuleName(n->moduleSpecifier, true);
      if (n->importClause && n->importClause->namedBindings && n->importClause->namedBindings->kind == TSK_NamedImports) {
        TsList *els = n->importClause->namedBindings->elements;
        for (int i = 0; els && i < els->n; i++) cCheckModuleMember(n, els->v[i]);
      }
      break;
    case TSK_ExportDeclaration:
      if (n->moduleSpecifier) {
        cResolveExternalModuleName(n->moduleSpecifier, true);
        if (n->exportClause && n->exportClause->kind == TSK_NamedExports) {
          TsList *els = n->exportClause->elements;
          for (int i = 0; els && i < els->n; i++) cCheckModuleMember(n, els->v[i]);
        }
      }
      break;
    case TSK_ImportEqualsDeclaration:
      if (n->moduleReference && n->moduleReference->kind == TSK_ExternalModuleReference)
        cResolveExternalModuleName(n->moduleReference->expression, true);
      break;
    default: break;
  }
}

static void cWalkNames(TsNode *n) {
  if (!n) return;
  cCheckModuleDeclaration(n);
  if (n->kind == TSK_Identifier) cCheckIdentifierReference(n);
  BNodeVec nv = { NULL, 0, 0 };
  bChildrenSorted(n, &nv);
  for (int i = 0; i < nv.n; i++) cWalkNames(nv.v[i]);
  free(nv.v);
}

#endif
