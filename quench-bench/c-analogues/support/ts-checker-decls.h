/*
 * Checker port, part 3: declaration checks that report on the merged declarations of a symbol
 * (TypeScript 5.9.2 checker.ts; line numbers in lib/typescript.js).
 *
 * PORTED:
 *   - checkTypeForDuplicateIndexSignatures (85939): index signatures of an object type (interface,
 *     class or type literal, merged across declarations through getMergedSymbol) grouped by the
 *     type ids of their key types; each declaration of a group of more than one is an error (2374).
 *     Key types are the type ids of getTypeFromTypeNode for keyword types, unions (forEachType),
 *     type references to type aliases (followed to their aliased type node) and to other symbols
 *     (their merged symbol), and imported names (alias resolution through the module exports).
 *   - the type-meaning name lookup needed by the key types (resolveName with Type meaning).
 * NOT PORTED: generic type alias instantiation, type-level intrinsic types other than keywords.
 */
#ifndef TS_CHECKER_DECLS_H
#define TS_CHECKER_DECLS_H

#include "ts-checker-names.h"

#define SF_TypeMeaning 788968u /* SymbolFlags.Type */

/* ---- type-meaning name resolution (resolveName with Type meaning, value-less part) ---- */
static TsSymbol *cResolveNameMeaning(TsNode *loc, BName name, uint32_t meaning);
static TsSymbol *cResolveTypeName(TsNode *loc, BName name);

/* resolveAlias for an import specifier: the exported symbol of the module, or NULL when unresolved */
static TsSymbol *cEntityNameSymbol(TsNode *loc, TsNode *name, uint32_t meaning);
#define CE_AnyMeaning (SF_Value | SF_Type | SF_Namespace)

static TsSymbol *cResolveImportAlias(TsSymbol *alias, int depth) {
  if (depth > 8 || !alias->ndecls) return NULL;
  TsNode *spec = alias->decls[0];
  if (spec->kind == TSK_ImportEqualsDeclaration) {
    /* export import X = A.B (resolveAlias of an import-equals declaration) */
    if (!spec->moduleReference || spec->moduleReference->kind == TSK_ExternalModuleReference) return NULL;
    return cEntityNameSymbol(spec, spec->moduleReference, CE_AnyMeaning);
  }
  if (spec->kind != TSK_ImportSpecifier) return NULL;
  TsNode *importDecl = spec->parent && spec->parent->parent ? spec->parent->parent->parent : NULL;
  if (!importDecl || importDecl->kind != TSK_ImportDeclaration) return NULL;
  TsSymbol *mod = cResolveExternalModuleName(importDecl->moduleSpecifier, false);
  if (!mod) return NULL;
  TsNode *name = spec->propertyName ? spec->propertyName : spec->name;
  TsSymTable *ex = cGetExportsOfModule(mod);
  int i = bTableIndex(ex, bnEscapeText(name->text, name->textLen));
  if (i < 0) return NULL;
  TsSymbol *s = ex->e[i].sym;
  if (s->flags & SF_Alias) return cResolveImportAlias(s, depth + 1);
  return cGetMergedSymbol(s);
}

static TsSymbol *cResolveTypeName(TsNode *loc, BName name) { return cResolveNameMeaning(loc, name, SF_TypeMeaning); }

/* resolveName with the meaning mask: type names, or namespace names for the left side of a qualified name */
static TsSymbol *cResolveNameMeaning(TsNode *loc, BName name, uint32_t meaning) {
  for (TsNode *n = loc; n; n = n->parent) {
    if (n->locals && !(n->kind == TSK_SourceFile && !n->hasExternalModuleIndicator)) {
      TsSymbol *s = bTableGet(n->locals, name);
      if (s && (s->flags & (meaning | SF_Alias))) return s;
    }
    switch (n->kind) {
      case TSK_SourceFile:
        if (!n->hasExternalModuleIndicator) goto globals;
        /* fall through */
      case TSK_ModuleDeclaration: {
        /* getSymbolOfDeclaration: the merged symbol (declarations of the namespace in other files) */
        TsSymbol *ms = cGetMergedSymbol(n->symbol);
        if (ms && ms->exports) {
          TsSymbol *s = bTableGet(ms->exports, name);
          if (s && (s->flags & (meaning | SF_Alias))) return s;
        }
        break;
      }
      case TSK_ClassDeclaration: case TSK_ClassExpression: case TSK_InterfaceDeclaration: {
        TsSymbol *ms = cGetMergedSymbol(n->symbol);
        if (ms && ms->members) {
          TsSymbol *s = bTableGet(ms->members, name);
          if (s && (s->flags & meaning)) return s;
        }
        break;
      }
      default: break;
    }
  }
globals:
  {
    TsSymbol *s = bTableGet(cGlobals, name);
    if (s && (s->flags & (meaning | SF_Alias))) return s;
  }
  return NULL;
}

/* ---- type identity keys for index signature parameters ---- */
typedef struct { int kind; const void *ptr; } CTypeKey;
typedef struct { CTypeKey *v; int n, cap; } CKeyVec;

static void cKeyPush(CKeyVec *kv, CTypeKey k) {
  for (int i = 0; i < kv->n; i++) if (kv->v[i].kind == k.kind && kv->v[i].ptr == k.ptr) return;
  if (kv->n == kv->cap) {
    kv->cap = kv->cap ? kv->cap * 2 : 8;
    kv->v = realloc(kv->v, sizeof(CTypeKey) * (size_t)kv->cap);
    if (!kv->v) abort();
  }
  kv->v[kv->n++] = k;
}

/* forEachType over the type of a type node: a union contributes its members */
static void cTypeKeysOfNode(TsNode *t, CKeyVec *out, int depth) {
  if (!t || depth > 16) return;
  switch (t->kind) {
    case TSK_ParenthesizedType:
      cTypeKeysOfNode(t->type, out, depth + 1);
      return;
    case TSK_UnionType:
      for (int i = 0; t->types && i < t->types->n; i++) cTypeKeysOfNode(t->types->v[i], out, depth + 1);
      return;
    case TSK_TypeReference: {
      TsNode *tn = t->typeName;
      if (!tn || tn->kind != TSK_Identifier) break;
      TsSymbol *s = cResolveTypeName(t, bnEscapeText(tn->text, tn->textLen));
      if (!s) { CTypeKey k = { 0, t }; cKeyPush(out, k); return; }
      if (s->flags & SF_Alias) s = cResolveImportAlias(s, 0);
      if (!s) { CTypeKey k = { 0, t }; cKeyPush(out, k); return; }
      if ((s->flags & SF_TypeAlias) && s->ndecls && s->decls[0]->kind == TSK_TypeAliasDeclaration && !s->decls[0]->typeParameters) {
        cTypeKeysOfNode(s->decls[0]->type, out, depth + 1);
        return;
      }
      CTypeKey k = { 0, cGetMergedSymbol(s) };
      cKeyPush(out, k);
      return;
    }
    default:
      break;
  }
  /* keyword types: one id per keyword (intrinsic types); everything else is its own type */
  switch (t->kind) {
    case TSK_AnyKeyword: case TSK_BigIntKeyword: case TSK_BooleanKeyword: case TSK_NeverKeyword:
    case TSK_NumberKeyword: case TSK_ObjectKeyword: case TSK_StringKeyword: case TSK_SymbolKeyword:
    case TSK_UndefinedKeyword: case TSK_UnknownKeyword: {
      CTypeKey k = { t->kind, NULL };
      cKeyPush(out, k);
      return;
    }
    default: break;
  }
  CTypeKey k = { 0, t };
  cKeyPush(out, k);
}

/* checkTypeForDuplicateIndexSignatures (85939): errors on every declaration of a repeated key type */
static void cCheckTypeForDuplicateIndexSignatures(TsNode *node) {
  TsSymbol *sym = cGetMergedSymbol(node->symbol);
  if (!sym) return;
  if (node->kind == TSK_InterfaceDeclaration && sym->ndecls > 0 && sym->decls[0] != node) return;
  TsSymbol *idx = sym->members ? bTableGet(sym->members, bnC("__index")) : NULL;
  if (!idx || !idx->ndecls) return;
  /* one group per key type id, in first-seen order */
  typedef struct { CTypeKey key; int nd, cap; TsNode **decls; } Group;
  Group *groups = NULL;
  int ng = 0, capg = 0;
  for (int d = 0; d < idx->ndecls; d++) {
    TsNode *decl = idx->decls[d];
    if (decl->kind != TSK_IndexSignature || !decl->parameters || decl->parameters->n != 1 || !decl->parameters->v[0]->type)
      continue;
    CKeyVec keys = { NULL, 0, 0 };
    cTypeKeysOfNode(decl->parameters->v[0]->type, &keys, 0);
    for (int k = 0; k < keys.n; k++) {
      int g = -1;
      for (int i = 0; i < ng && g < 0; i++)
        if (groups[i].key.kind == keys.v[k].kind && groups[i].key.ptr == keys.v[k].ptr) g = i;
      if (g < 0) {
        if (ng == capg) {
          capg = capg ? capg * 2 : 8;
          groups = realloc(groups, sizeof(Group) * (size_t)capg);
          if (!groups) abort();
        }
        g = ng++;
        groups[g].key = keys.v[k];
        groups[g].nd = 0;
        groups[g].cap = 0;
        groups[g].decls = NULL;
      }
      int dup = 0;
      for (int i = 0; i < groups[g].nd; i++) if (groups[g].decls[i] == decl) dup = 1;
      if (dup) continue;
      if (groups[g].nd == groups[g].cap) {
        groups[g].cap = groups[g].cap ? groups[g].cap * 2 : 4;
        groups[g].decls = realloc(groups[g].decls, sizeof(TsNode *) * (size_t)groups[g].cap);
      }
      groups[g].decls[groups[g].nd++] = decl;
    }
    free(keys.v);
  }
  for (int g = 0; g < ng; g++) {
    if (groups[g].nd <= 1) continue;
    for (int i = 0; i < groups[g].nd; i++) {
      TsNode *decl = groups[g].decls[i];
      cDiagAdd(cSourceFileOf(decl), cNodeStart(decl), 2374, NULL); /* Duplicate_index_signature_for_type_0 */
    }
  }
  for (int g = 0; g < ng; g++) free(groups[g].decls);
  free(groups);
}

/* the declarations that call checkTypeForDuplicateIndexSignatures: checkTypeLiteral, checkClassLikeDeclaration,
 * checkInterfaceDeclaration (86234, 89471, 89941) */
static void cCheckDeclaresObjectType(TsNode *n) {
  switch (n->kind) {
    case TSK_TypeLiteral: case TSK_ClassDeclaration: case TSK_ClassExpression: case TSK_InterfaceDeclaration:
      cCheckTypeForDuplicateIndexSignatures(n);
      break;
    default: break;
  }
}

/* a walk over every node (the order of checking does not change the set of diagnostics) */
static void cWalkAll(TsNode *n, void (*fn)(TsNode *)) {
  if (!n) return;
  fn(n);
  BNodeVec nv = { NULL, 0, 0 };
  bChildrenSorted(n, &nv);
  for (int i = 0; i < nv.n; i++) cWalkAll(nv.v[i], fn);
  free(nv.v);
}

#endif
