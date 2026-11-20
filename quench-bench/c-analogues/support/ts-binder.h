/*
 * Binder port (TypeScript 5.9.2 binder.ts, symbol part) for the typescript-lib analogue.
 *
 * PORTED: symbol creation and declaration merging (declareSymbol with the SymbolFlags includes and
 * excludes), symbol tables (locals, exports, members, globalExports), containers and block-scoped
 * containers, declarations of every TypeScript node kind the binder declares, module instance state
 * (including the export-specifier alias lookup), strict mode from the project options (strict: true,
 * so alwaysStrict), and the binder's own symbol counter (SourceFile.symbolCount).
 *
 * NOT PORTED (no effect on symbol counts, so they are left out for now):
 *   - control flow: flow nodes (createFlowNode), flow labels, the reachability checks and
 *     the flow fields of nodes. The flow-node count is therefore not produced yet.
 *   - binder diagnostics (bindDiagnostics), strict mode checks that only report errors,
 *     JavaScript-only assignment declarations, CommonJS handling and JSDoc binding (every file in
 *     the immer-tiny project is TypeScript, and the JSDoc nodes are not bound for TypeScript files).
 *   - the lazy re-check of alias targets through the parent chain for nodes not yet bound.
 *   - identifier escapes: names are compared on the raw identifier text (no \u escape decoding),
 *     and numeric literal names use the raw text.
 * Usage: call tsBindSourceFile(sf, fileName) after parsing sf with parseSourceFile; it sets
 * sf->binderSymbolCount. The arena is shared with the parser: reset it per file.
 */
#ifndef TS_BINDER_H
#define TS_BINDER_H

#include <string.h>
#include "ts-enums.h"

/* SymbolFlags (TypeScript 5.9.2 names and values) */
enum {
  SF_None = 0, SF_FunctionScopedVariable = 1, SF_BlockScopedVariable = 2, SF_Property = 4,
  SF_EnumMember = 8, SF_Function = 16, SF_Class = 32, SF_Interface = 64, SF_ConstEnum = 128,
  SF_RegularEnum = 256, SF_ValueModule = 512, SF_NamespaceModule = 1024, SF_TypeLiteral = 2048,
  SF_ObjectLiteral = 4096, SF_Method = 8192, SF_Constructor = 16384, SF_GetAccessor = 32768,
  SF_SetAccessor = 65536, SF_Signature = 131072, SF_TypeParameter = 262144, SF_TypeAlias = 524288,
  SF_ExportValue = 1048576, SF_Alias = 2097152, SF_Prototype = 4194304, SF_ExportStar = 8388608,
  SF_Optional = 16777216, SF_Assignment = 67108864,
  SF_Enum = 384, SF_Variable = 3, SF_Value = 111551, SF_Type = 788968, SF_Namespace = 1920,
  SF_Module = 1536, SF_Accessor = 98304
};

/* excludes (copied from binder.ts) */
enum {
  EX_InterfaceExcludes = 788872, EX_ClassExcludes = 899503, EX_TypeAliasExcludes = 788968,
  EX_EnumMemberExcludes = 900095, EX_RegularEnumExcludes = 899327, EX_ConstEnumExcludes = 899967,
  EX_FunctionExcludes = 110991, EX_ValueModuleExcludes = 110735, EX_NamespaceModuleExcludes = 0,
  EX_BlockScopedVariableExcludes = 111551, EX_ParameterExcludes = 111551,
  EX_FunctionScopedVariableExcludes = 111550, EX_PropertyExcludes = 0, EX_MethodExcludes = 103359,
  EX_GetAccessorExcludes = 46015, EX_SetAccessorExcludes = 78783, EX_AccessorExcludes = 13247,
  EX_TypeParameterExcludes = 526824, EX_AliasExcludes = 2097152, EX_All = -1
};

/* container flags (binder.ts ContainerFlags) */
enum {
  CF_IsContainer = 1, CF_IsBlockScopedContainer = 2, CF_IsControlFlowContainer = 4,
  CF_IsFunctionLike = 8, CF_IsFunctionExpression = 16, CF_HasLocals = 32, CF_IsInterface = 64,
  CF_IsObjectLiteralOrClassExpressionMethodOrAccessor = 128
};

/* a name is a byte string; s == NULL is the undefined name */
typedef struct { const char *s; int n; } BName;

typedef struct TsSymbol {
  uint32_t flags;
  BName name;
  struct TsSymTable *exports, *members, *globalExports;
  struct TsSymbol *exportSymbol, *parent;
  TsNode **decls;
  int ndecls, declCap;
  bool constEnumOnlyModule;
  /* checker (ts-checker.h) */
  TsNode *valueDeclaration;
  struct TsSymbol *mergedInto;   /* recordMergedSymbol: the symbol this one was merged into */
  struct TsType *declaredType, *type;
  int64_t lastAssignPos;         /* checker: lastAssignmentPos (negative: a definite assignment); valid when lastAssignSet */
  bool lastAssignSet;
  struct TsType *aliasHolder;    /* generic type aliases: the holder of their instantiations (ts-types.h) */
  struct TsType *uniqueType;     /* the unique symbol type of a const or readonly property symbol */
  struct TsSymbol *instTarget;   /* instantiated symbols: the symbol instantiated (instantiateSymbol) */
  struct TsMapper *instMapper;   /* instantiated symbols: the mapper */
  uint32_t checkFlags;
  uint32_t checkId;              /* getSymbolId: assigned on first use, used in unique symbol names */
  struct TsType *deferParent;    /* deferred union or intersection property (DeferredType): the containing type */
  struct TsType **deferTypes;    /* ... and the member property types */
  int nDeferTypes;
} TsSymbol;

typedef struct TsSymEntry { BName key; TsSymbol *sym; } TsSymEntry;
typedef struct TsSymTable { TsSymEntry *e; int cap, n; int *slot; int slotCap; } TsSymTable;

/* binder state (one file at a time) */
static TsNode *bFile;
static const char *bFileName;
static TsNode *bParent;
static TsNode *bContainer;
static bool bSeenThis;   /* seenThisKeyword: a this type was bound in the current interface */
static TsNode *bBlockScopeContainer;
static bool bInStrictMode;
static int32_t bSymbolCount;
static int bDepth;

static void *ts_arena_zalloc(size_t n) {
  void *p = ts_arena_alloc(n);
  memset(p, 0, n);
  return p;
}

/* ---- names ---- */
static BName bnC(const char *s) { BName b = { s, (int)strlen(s) }; return b; }
static BName bnUndef(void) { BName b = { NULL, 0 }; return b; }
static bool bnDef(BName b) { return b.s != NULL; }
static bool bnEq(BName a, BName b) { return a.n == b.n && (a.n == 0 || memcmp(a.s, b.s, (size_t)a.n) == 0); }

/* escapeLeadingUnderscores on raw text: a leading "__" gets one more "_" */
static BName bnEscapeText(const char *s, int n) {
  BName b = { s, n };
  if (n >= 2 && s[0] == '_' && s[1] == '_') {
    char *o = ts_arena_alloc((size_t)n + 1);
    o[0] = '_';
    memcpy(o + 1, s, (size_t)n);
    b.s = o;
    b.n = n + 1;
  }
  return b;
}

static BName bnConcat(const char *a, int na, const char *b, int nb) {
  char *o = ts_arena_alloc((size_t)(na + nb) + 1);
  memcpy(o, a, (size_t)na);
  memcpy(o + na, b, (size_t)nb);
  BName r = { o, na + nb };
  return r;
}

/* the value text of a literal: the parser stores string literal text without its quotes */
static void bLiteralValue(TsNode *lit, const char **s, int *n) {
  *s = lit->text;
  *n = lit->textLen;
}

static bool bIsStringOrNumericLiteralLike(TsNode *n) {
  return n && (n->kind == TSK_StringLiteral || n->kind == TSK_NoSubstitutionTemplateLiteral || n->kind == TSK_NumericLiteral);
}
static bool bIsSignedNumericLiteral(TsNode *n) {
  return n && n->kind == TSK_PrefixUnaryExpression && (n->extra == TSK_PlusToken || n->extra == TSK_MinusToken) &&
         n->operand && n->operand->kind == TSK_NumericLiteral;
}
static TsNode *bSkipParentheses(TsNode *n) {
  while (n && n->kind == TSK_ParenthesizedExpression) n = n->expression;
  return n;
}

/* ---- symbol tables ---- */
static uint32_t bHash(BName k) {
  uint32_t h = 2166136261u;
  for (int i = 0; i < k.n; i++) h = (h ^ (uint8_t)k.s[i]) * 16777619u;
  return h;
}

static TsSymTable *bTableNew(void) {
  TsSymTable *t = ts_arena_zalloc(sizeof(TsSymTable));
  t->cap = 8;
  t->e = ts_arena_zalloc(sizeof(TsSymEntry) * (size_t)t->cap);
  t->slotCap = 16;
  t->slot = ts_arena_zalloc(sizeof(int) * (size_t)t->slotCap);
  return t;
}

/* index of key k in t->e, or -1 */
static int bTableIndex(TsSymTable *t, BName k) {
  uint32_t mask = (uint32_t)t->slotCap - 1, i = bHash(k) & mask;
  while (t->slot[i]) {
    if (bnEq(t->e[t->slot[i] - 1].key, k)) return t->slot[i] - 1;
    i = (i + 1) & mask;
  }
  return -1;
}

static TsSymbol *bTableGet(TsSymTable *t, BName k) {
  int i = bTableIndex(t, k);
  return i >= 0 ? t->e[i].sym : NULL;
}

/* Entries keep insertion order (a JavaScript Map): iteration is t->e[0..n). */
static void bTableAppend(TsSymTable *t, BName k, TsSymbol *sym) {
  if (t->n == t->cap) {
    TsSymEntry *ne = ts_arena_zalloc(sizeof(TsSymEntry) * (size_t)t->cap * 2);
    memcpy(ne, t->e, sizeof(TsSymEntry) * (size_t)t->n);
    t->e = ne;
    t->cap *= 2;
  }
  if ((t->n + 1) * 2 > t->slotCap) {
    t->slotCap *= 2;
    t->slot = ts_arena_zalloc(sizeof(int) * (size_t)t->slotCap);
    uint32_t mask = (uint32_t)t->slotCap - 1;
    for (int j = 0; j < t->n; j++) {
      uint32_t i = bHash(t->e[j].key) & mask;
      while (t->slot[i]) i = (i + 1) & mask;
      t->slot[i] = j + 1;
    }
  }
  t->e[t->n].key = k;
  t->e[t->n].sym = sym;
  uint32_t mask = (uint32_t)t->slotCap - 1, i = bHash(k) & mask;
  while (t->slot[i]) i = (i + 1) & mask;
  t->slot[i] = ++t->n;
}

static void bTableSet(TsSymTable *t, BName k, TsSymbol *sym) {
  int i = bTableIndex(t, k);
  if (i >= 0) { t->e[i].sym = sym; return; }
  bTableAppend(t, k, sym);
}

static TsSymbol *bCreateSymbol(uint32_t flags, BName name) {
  bSymbolCount++;
#ifdef TS_BINDER_DEBUG
  fprintf(stderr, "symbol %d flags %u name %.*s\n", bSymbolCount, flags, name.s ? name.n : 9, name.s ? name.s : "(undef)");
#endif
  TsSymbol *s = ts_arena_zalloc(sizeof(TsSymbol));
  s->flags = flags;
  s->name = name;
  return s;
}

static void bAddDecl(TsSymbol *s, TsNode *node) {
  if (s->ndecls == s->declCap) {
    int cap = s->declCap ? s->declCap * 2 : 4;
    TsNode **d = ts_arena_alloc(sizeof(TsNode *) * (size_t)cap);
    if (s->ndecls) memcpy(d, s->decls, sizeof(TsNode *) * (size_t)s->ndecls);
    s->decls = d;
    s->declCap = cap;
  }
  for (int i = 0; i < s->ndecls; i++) if (s->decls[i] == node) return; /* appendIfUnique */
  s->decls[s->ndecls++] = node;
}

/* ---- modifiers and combined flags ---- */
static uint32_t bModifierFlags(TsNode *n) {
  uint32_t f = (uint32_t)modifiersToFlags(n->modifiers);
  if (n->flags & TNF_NestedNamespace) f |= TMF_Export;
  return f;
}
static bool bHasSyntacticModifier(TsNode *n, uint32_t flag) { return (bModifierFlags(n) & flag) != 0; }

static TsNode *bWalkUpBindingElementsAndPatterns(TsNode *binding) {
  TsNode *n = binding->parent;
  while (n->parent && n->parent->kind == TSK_BindingElement) n = n->parent->parent;
  return n->parent;
}

/* getCombinedFlags over the variable declaration / list / statement chain */
static uint32_t bCombinedFlags(TsNode *node, bool useModifiers) {
  if (node->kind == TSK_BindingElement) node = bWalkUpBindingElementsAndPatterns(node);
  uint32_t flags = useModifiers ? bModifierFlags(node) : (uint32_t)node->flags;
  if (node->kind == TSK_VariableDeclaration) node = node->parent;
  if (node && node->kind == TSK_VariableDeclarationList) {
    flags |= useModifiers ? bModifierFlags(node) : (uint32_t)node->flags;
    node = node->parent;
  }
  if (node && node->kind == TSK_VariableStatement) flags |= useModifiers ? bModifierFlags(node) : (uint32_t)node->flags;
  return flags;
}

static TsNode *bRootDeclaration(TsNode *n) {
  while (n->kind == TSK_BindingElement) n = n->parent->parent;
  return n;
}

static bool bIsBlockOrCatchScoped(TsNode *decl) {
  if ((bCombinedFlags(decl, false) & TNF_BlockScoped) != 0) return true;
  TsNode *root = bRootDeclaration(decl);
  return root->kind == TSK_VariableDeclaration && root->parent && root->parent->kind == TSK_CatchClause;
}

static bool bIsPartOfParameterDeclaration(TsNode *decl) { return bRootDeclaration(decl)->kind == TSK_Parameter; }

static bool bIsBindingPattern(TsNode *n) {
  return n && (n->kind == TSK_ObjectBindingPattern || n->kind == TSK_ArrayBindingPattern);
}

static bool bIsEnumConst(TsNode *n) { return (bCombinedFlags(n, true) & TMF_Const) != 0; }

static bool bIsFunctionLikeKind(int k) {
  switch (k) {
    case TSK_MethodSignature: case TSK_CallSignature: case TSK_JSDocSignature: case TSK_ConstructSignature:
    case TSK_IndexSignature: case TSK_FunctionType: case TSK_JSDocFunctionType: case TSK_ConstructorType:
    case TSK_MethodDeclaration: case TSK_Constructor: case TSK_GetAccessor: case TSK_SetAccessor:
    case TSK_FunctionDeclaration: case TSK_FunctionExpression: case TSK_ArrowFunction:
      return true;
  }
  return false;
}

static bool bIsEntityNameExpression(TsNode *n) {
  if (n->kind == TSK_Identifier) return true;
  return n->kind == TSK_PropertyAccessExpression && n->name && n->name->kind == TSK_Identifier &&
         bIsEntityNameExpression(n->expression);
}

static bool bIsAmbientModule(TsNode *n) {
  return n->kind == TSK_ModuleDeclaration && (n->name->kind == TSK_StringLiteral || (n->flags & TNF_GlobalAugmentation));
}

static bool bIsGlobalScopeAugmentation(TsNode *n) { return n->kind == TSK_ModuleDeclaration && (n->flags & TNF_GlobalAugmentation); }

static bool bIsParameterPropertyDeclaration(TsNode *n, TsNode *parent) {
  return bHasSyntacticModifier(n, TMF_Public | TMF_Private | TMF_Protected | TMF_Readonly | TMF_Override) &&
         parent->kind == TSK_Constructor;
}

/* ---- declaration names (getDeclarationName) ---- */
static TsNode *bContainingClass(TsNode *n) {
  for (TsNode *p = n->parent; p; p = p->parent)
    if (p->kind == TSK_ClassDeclaration || p->kind == TSK_ClassExpression) return p;
  return NULL;
}

static bool bHasDynamicName(TsNode *decl) {
  TsNode *name = decl->name;
  if (!name) return false;
  if (name->kind != TSK_ComputedPropertyName && name->kind != TSK_ElementAccessExpression) return false;
  TsNode *expr = name->kind == TSK_ElementAccessExpression ? bSkipParentheses(name->argumentExpression) : name->expression;
  return !bIsStringOrNumericLiteralLike(expr) && !bIsSignedNumericLiteral(expr);
}

static bool bIsPropertyNameLiteral(TsNode *n) {
  return n->kind == TSK_Identifier || n->kind == TSK_StringLiteral || n->kind == TSK_NoSubstitutionTemplateLiteral ||
         n->kind == TSK_NumericLiteral;
}

static bool bIsAliasableExpression(TsNode *e) { return bIsEntityNameExpression(e) || e->kind == TSK_ClassExpression; }

static BName bGetDeclarationName(TsNode *node);

/* identifier or literal name as a symbol-table key */
static BName bNameOfPropertyName(TsNode *name) {
  if (name->kind == TSK_Identifier) return bnEscapeText(name->text, name->textLen);
  const char *s;
  int n;
  bLiteralValue(name, &s, &n);
  return bnEscapeText(s, n);
}

static BName bGetDeclarationName(TsNode *node) {
  if (node->kind == TSK_ExportAssignment) return node->extra ? bnC("export=") : bnC("default");
  TsNode *name = node->name;
  if (name) {
    if (bIsAmbientModule(node)) {
      if (bIsGlobalScopeAugmentation(node)) return bnC("__global");
      const char *s;
      int n;
      bLiteralValue(name, &s, &n);
      char *q = ts_arena_alloc((size_t)n + 3);
      q[0] = '"';
      memcpy(q + 1, s, (size_t)n);
      q[n + 1] = '"';
      BName r = { q, n + 2 };
      return r;
    }
    if (name->kind == TSK_ComputedPropertyName) {
      TsNode *e = name->expression;
      if (bIsStringOrNumericLiteralLike(e)) return bNameOfPropertyName(e);
      if (bIsSignedNumericLiteral(e)) {
        const char *op = e->extra == TSK_MinusToken ? "-" : "+";
        return bnConcat(op, 1, e->operand->text, e->operand->textLen);
      }
      return bnUndef();
    }
    if (name->kind == TSK_PrivateIdentifier) {
      TsNode *cls = bContainingClass(node);
      if (!cls) return bnUndef();
      void *owner = cls->symbol ? (void *)cls->symbol : (void *)cls;
      char idbuf[40];
      int idn = snprintf(idbuf, sizeof idbuf, "__#%p@", owner);
      return bnConcat(idbuf, idn, name->text, name->textLen);
    }
    return bIsPropertyNameLiteral(name) ? bNameOfPropertyName(name) : bnUndef();
  }
  switch (node->kind) {
    case TSK_Constructor: return bnC("__constructor");
    case TSK_FunctionType: case TSK_CallSignature: return bnC("__call");
    case TSK_ConstructorType: case TSK_ConstructSignature: return bnC("__new");
    case TSK_IndexSignature: return bnC("__index");
    case TSK_ExportDeclaration: return bnC("__export");
    case TSK_SourceFile: return bnC("export=");
  }
  return bnUndef();
}

/* ---- declarations and symbols ---- */
/* setValueDeclaration (TypeScript sources only: the JS assignment-declaration cases do not apply) */
static void bSetValueDeclaration(TsSymbol *sym, TsNode *node) {
  if (!sym->valueDeclaration || (sym->valueDeclaration->kind != node->kind &&
      (sym->valueDeclaration->kind == TSK_ModuleDeclaration || sym->valueDeclaration->kind == TSK_Identifier)))
    sym->valueDeclaration = node;
}

static void bAddDeclarationToSymbol(TsSymbol *sym, TsNode *node, uint32_t symbolFlags) {
  sym->flags |= symbolFlags;
  node->symbol = sym;
  bAddDecl(sym, node);
  if (symbolFlags & SF_Value) bSetValueDeclaration(sym, node);
  if ((symbolFlags & (SF_Class | SF_Enum | SF_Module | SF_Variable)) && !sym->exports) sym->exports = bTableNew();
  if ((symbolFlags & (SF_Class | SF_Interface | SF_TypeLiteral | SF_ObjectLiteral)) && !sym->members) sym->members = bTableNew();
}

static TsSymbol *bDeclareSymbol(TsSymTable *table, TsSymbol *parent, TsNode *node, uint32_t includes, uint32_t excludes, bool isComputedName) {
  if (!table) { fprintf(stderr, "binder: no symbol table for a declaration\n"); abort(); }
  bool isDefaultExport = bHasSyntacticModifier(node, TMF_Default) ||
                         (node->kind == TSK_ExportSpecifier && node->name && node->name->textLen == 7 &&
                          memcmp(node->name->text, "default", 7) == 0);
  BName name;
  if (isComputedName) name = bnC("__computed");
  else if (isDefaultExport && parent) name = bnC("default");
  else name = bGetDeclarationName(node);
  TsSymbol *symbol;
  if (!bnDef(name)) {
    symbol = bCreateSymbol(0, bnC("__missing"));
  } else {
    symbol = bTableGet(table, name);
    if (!symbol) {
      symbol = bCreateSymbol(0, name);
      bTableSet(table, name, symbol);
    } else if (symbol->flags & excludes) {
      if (!((includes & SF_Variable) && (symbol->flags & SF_Assignment))) {
        /* duplicate declaration: diagnostics are not produced; a new symbol is created but not stored */
        symbol = bCreateSymbol(0, name);
      }
    }
  }
  bAddDeclarationToSymbol(symbol, node, includes);
  if (!symbol->parent) symbol->parent = parent;
  return symbol;
}

static bool bHasExportDeclarations(TsNode *node) {
  TsNode *body = node->kind == TSK_SourceFile ? node : (node->body && node->body->kind == TSK_ModuleBlock ? node->body : NULL);
  if (!body || !body->statements) return false;
  for (int i = 0; i < body->statements->n; i++) {
    TsNode *s = body->statements->v[i];
    if (s->kind == TSK_ExportDeclaration || s->kind == TSK_ExportAssignment) return true;
  }
  return false;
}

static void bSetExportContextFlag(TsNode *node) {
  if ((node->flags & TNF_Ambient) && !bHasExportDeclarations(node)) node->flags |= TNF_ExportContext;
  else node->flags &= ~(uint32_t)TNF_ExportContext;
}

static TsSymbol *bDeclareModuleMember(TsNode *node, uint32_t symbolFlags, uint32_t symbolExcludes) {
  bool hasExportModifier = (bCombinedFlags(node, true) & TMF_Export) != 0;
  if (symbolFlags & SF_Alias) {
    if (node->kind == TSK_ExportSpecifier || (node->kind == TSK_ImportEqualsDeclaration && hasExportModifier))
      return bDeclareSymbol(bContainer->symbol->exports, bContainer->symbol, node, symbolFlags, symbolExcludes, false);
    return bDeclareSymbol(bContainer->locals, NULL, node, symbolFlags, symbolExcludes, false);
  }
  if (!bIsAmbientModule(node) && (hasExportModifier || (bContainer->flags & TNF_ExportContext))) {
    if (!bContainer->locals || (bHasSyntacticModifier(node, TMF_Default) && !bnDef(bGetDeclarationName(node)))) {
      return bDeclareSymbol(bContainer->symbol->exports, bContainer->symbol, node, symbolFlags, symbolExcludes, false);
    }
    uint32_t exportKind = (symbolFlags & SF_Value) ? SF_ExportValue : 0;
    TsSymbol *local = bDeclareSymbol(bContainer->locals, NULL, node, exportKind, symbolExcludes, false);
    local->exportSymbol = bDeclareSymbol(bContainer->symbol->exports, bContainer->symbol, node, symbolFlags, symbolExcludes, false);
    node->localSymbol = local;
    return local;
  }
  return bDeclareSymbol(bContainer->locals, NULL, node, symbolFlags, symbolExcludes, false);
}

static TsSymbol *bDeclareClassMember(TsNode *node, uint32_t symbolFlags, uint32_t symbolExcludes) {
  if (bHasSyntacticModifier(node, TMF_Static))
    return bDeclareSymbol(bContainer->symbol->exports, bContainer->symbol, node, symbolFlags, symbolExcludes, false);
  return bDeclareSymbol(bContainer->symbol->members, bContainer->symbol, node, symbolFlags, symbolExcludes, false);
}

static TsSymbol *bDeclareSourceFileMember(TsNode *node, uint32_t symbolFlags, uint32_t symbolExcludes) {
  if (bFile->hasExternalModuleIndicator) return bDeclareModuleMember(node, symbolFlags, symbolExcludes);
  return bDeclareSymbol(bFile->locals, NULL, node, symbolFlags, symbolExcludes, false);
}

static TsSymbol *bDeclareSymbolAndAddToSymbolTable(TsNode *node, uint32_t symbolFlags, uint32_t symbolExcludes) {
  TsNode *c = bContainer;
  switch (c->kind) {
    case TSK_ModuleDeclaration: return bDeclareModuleMember(node, symbolFlags, symbolExcludes);
    case TSK_SourceFile: return bDeclareSourceFileMember(node, symbolFlags, symbolExcludes);
    case TSK_ClassExpression: case TSK_ClassDeclaration: return bDeclareClassMember(node, symbolFlags, symbolExcludes);
    case TSK_EnumDeclaration:
      return bDeclareSymbol(c->symbol->exports, c->symbol, node, symbolFlags, symbolExcludes, false);
    case TSK_TypeLiteral: case TSK_ObjectLiteralExpression: case TSK_InterfaceDeclaration: case TSK_JsxAttributes:
      return bDeclareSymbol(c->symbol->members, c->symbol, node, symbolFlags, symbolExcludes, false);
    case TSK_FunctionType: case TSK_ConstructorType: case TSK_CallSignature: case TSK_ConstructSignature:
    case TSK_IndexSignature: case TSK_MethodDeclaration: case TSK_MethodSignature: case TSK_Constructor:
    case TSK_GetAccessor: case TSK_SetAccessor: case TSK_FunctionDeclaration: case TSK_FunctionExpression:
    case TSK_ArrowFunction: case TSK_ClassStaticBlockDeclaration: case TSK_TypeAliasDeclaration: case TSK_MappedType:
      return bDeclareSymbol(c->locals, NULL, node, symbolFlags, symbolExcludes, false);
  }
  return NULL;
}

static TsSymbol *bBindAnonymousDeclaration(TsNode *node, uint32_t symbolFlags, BName name) {
  TsSymbol *symbol = bCreateSymbol(symbolFlags, name);
  if (symbolFlags & (SF_EnumMember | 106500 /* ClassMember */)) symbol->parent = bContainer->symbol;
  bAddDeclarationToSymbol(symbol, node, symbolFlags);
  return symbol;
}

static TsSymbol *bBindBlockScopedDeclaration(TsNode *node, uint32_t symbolFlags, uint32_t symbolExcludes) {
  switch (bBlockScopeContainer->kind) {
    case TSK_ModuleDeclaration:
      return bDeclareModuleMember(node, symbolFlags, symbolExcludes);
    case TSK_SourceFile:
      if (bContainer->hasExternalModuleIndicator) return bDeclareModuleMember(node, symbolFlags, symbolExcludes);
      /* fall through */
    default:
      if (!bBlockScopeContainer->locals) bBlockScopeContainer->locals = bTableNew();
      return bDeclareSymbol(bBlockScopeContainer->locals, NULL, node, symbolFlags, symbolExcludes, false);
  }
}

static TsSymbol *bBindPropertyOrMethodOrAccessor(TsNode *node, uint32_t symbolFlags, uint32_t symbolExcludes) {
  if (bHasDynamicName(node)) return bBindAnonymousDeclaration(node, symbolFlags, bnC("__computed"));
  return bDeclareSymbolAndAddToSymbolTable(node, symbolFlags, symbolExcludes);
}

/* ---- module instance state (getModuleInstanceState) ---- */
enum { MI_NonInstantiated = 0, MI_Instantiated = 1, MI_ConstEnumOnly = 2 };
static int bModuleInstanceStateCached(TsNode *n);

static bool bNodeHasName(TsNode *s, TsNode *name) {
  return s->name && s->name->kind == TSK_Identifier && s->name->textLen == name->textLen &&
         memcmp(s->name->text, name->text, (size_t)name->textLen) == 0;
}

static int bAliasTargetState(TsNode *spec) {
  TsNode *name = spec->propertyName ? spec->propertyName : spec->name;
  if (name->kind != TSK_Identifier) return MI_Instantiated;
  for (TsNode *p = spec->parent; p; p = p->parent) {
    if (p->kind == TSK_Block || p->kind == TSK_ModuleBlock || p->kind == TSK_SourceFile) {
      int found = -1;
      if (p->statements)
        for (int i = 0; i < p->statements->n; i++) {
          TsNode *s = p->statements->v[i];
          if (!bNodeHasName(s, name)) continue;
          int st = bModuleInstanceStateCached(s);
          if (found < 0 || st > found) found = st;
          if (found == MI_Instantiated) return found;
          if (s->kind == TSK_ImportEqualsDeclaration) found = MI_Instantiated;
        }
      if (found >= 0) return found;
    }
  }
  return MI_Instantiated;
}

static int bModuleInstanceStateWorker(TsNode *n) {
  switch (n->kind) {
    case TSK_InterfaceDeclaration: case TSK_TypeAliasDeclaration:
      return MI_NonInstantiated;
    case TSK_EnumDeclaration:
      if (bIsEnumConst(n)) return MI_ConstEnumOnly;
      break;
    case TSK_ImportDeclaration: case TSK_ImportEqualsDeclaration:
      if (!bHasSyntacticModifier(n, TMF_Export)) return MI_NonInstantiated;
      break;
    case TSK_ExportDeclaration:
      if (!n->moduleSpecifier && n->exportClause && n->exportClause->kind == TSK_NamedExports) {
        int state = MI_NonInstantiated;
        TsList *els = n->exportClause->elements;
        for (int i = 0; els && i < els->n; i++) {
          int st = bAliasTargetState(els->v[i]);
          if (st > state) state = st;
          if (state == MI_Instantiated) return state;
        }
        return state;
      }
      break;
    case TSK_ModuleBlock: {
      int state = MI_NonInstantiated;
      if (n->statements)
        for (int i = 0; i < n->statements->n; i++) {
          int cs = bModuleInstanceStateCached(n->statements->v[i]);
          if (cs == MI_NonInstantiated) continue;
          if (cs == MI_ConstEnumOnly) { state = MI_ConstEnumOnly; continue; }
          return MI_Instantiated;
        }
      return state;
    }
    case TSK_ModuleDeclaration:
      return n->body ? bModuleInstanceStateCached(n->body) : MI_Instantiated;
    default:
      break;
  }
  return MI_Instantiated;
}

static int bModuleInstanceStateCached(TsNode *n) {
  if (bDepth > 64) return MI_Instantiated; /* guard against alias cycles */
  bDepth++;
  int r = bModuleInstanceStateWorker(n);
  bDepth--;
  return r;
}

/* ---- container flags (getContainerFlags) ---- */
static bool bIsObjectLiteralOrClassExpressionMethodOrAccessor(TsNode *n) {
  return (n->kind == TSK_GetAccessor || n->kind == TSK_SetAccessor || n->kind == TSK_MethodDeclaration) && n->parent &&
         (n->parent->kind == TSK_ObjectLiteralExpression || n->parent->kind == TSK_ClassExpression);
}

static uint32_t bContainerFlags(TsNode *node) {
  switch (node->kind) {
    case TSK_ClassExpression: case TSK_ClassDeclaration: case TSK_EnumDeclaration: case TSK_ObjectLiteralExpression:
    case TSK_TypeLiteral: case TSK_JsxAttributes:
      return CF_IsContainer;
    case TSK_InterfaceDeclaration:
      return CF_IsContainer | CF_IsInterface;
    case TSK_ModuleDeclaration: case TSK_TypeAliasDeclaration: case TSK_MappedType: case TSK_IndexSignature:
      return CF_IsContainer | CF_HasLocals;
    case TSK_SourceFile:
      return CF_IsContainer | CF_IsControlFlowContainer | CF_HasLocals;
    case TSK_GetAccessor: case TSK_SetAccessor: case TSK_MethodDeclaration:
      if (bIsObjectLiteralOrClassExpressionMethodOrAccessor(node))
        return CF_IsContainer | CF_IsControlFlowContainer | CF_HasLocals | CF_IsFunctionLike | CF_IsObjectLiteralOrClassExpressionMethodOrAccessor;
      /* fall through */
    case TSK_Constructor: case TSK_FunctionDeclaration: case TSK_MethodSignature: case TSK_CallSignature:
    case TSK_JSDocSignature: case TSK_JSDocFunctionType: case TSK_FunctionType: case TSK_ConstructSignature:
    case TSK_ConstructorType: case TSK_ClassStaticBlockDeclaration:
      return CF_IsContainer | CF_IsControlFlowContainer | CF_HasLocals | CF_IsFunctionLike;
    case TSK_FunctionExpression: case TSK_ArrowFunction:
      return CF_IsContainer | CF_IsControlFlowContainer | CF_HasLocals | CF_IsFunctionLike | CF_IsFunctionExpression;
    case TSK_ModuleBlock:
      return CF_IsControlFlowContainer;
    case TSK_PropertyDeclaration:
      return node->initializer ? CF_IsControlFlowContainer : 0;
    case TSK_CatchClause: case TSK_ForStatement: case TSK_ForInStatement: case TSK_ForOfStatement: case TSK_CaseBlock:
      return CF_IsBlockScopedContainer | CF_HasLocals;
    case TSK_Block:
      return (node->parent && (bIsFunctionLikeKind(node->parent->kind) || node->parent->kind == TSK_ClassStaticBlockDeclaration))
                 ? 0 : (CF_IsBlockScopedContainer | CF_HasLocals);
  }
  return 0;
}

/* ---- children in source order (forEachChild) ---- */
typedef struct { TsNode **v; int n, cap; } BNodeVec;

static void bVecPush(BNodeVec *nv, TsNode *n) {
  if (!n) return;
  if (nv->n == nv->cap) {
    nv->cap = nv->cap ? nv->cap * 2 : 16;
    nv->v = realloc(nv->v, sizeof(TsNode *) * (size_t)nv->cap);
    if (!nv->v) abort();
  }
  nv->v[nv->n++] = n;
}

static void bVecPushList(BNodeVec *nv, TsList *l) {
  if (!l) return;
  for (int i = 0; i < l->n; i++) bVecPush(nv, l->v[i]);
}

static void bChildrenSorted(TsNode *n, BNodeVec *nv) {
  bVecPush(nv, n->name); bVecPush(nv, n->expression); bVecPush(nv, n->initializer); bVecPush(nv, n->type);
  bVecPush(nv, n->body); bVecPush(nv, n->questionToken); bVecPush(nv, n->exclamationToken);
  bVecPush(nv, n->asteriskToken); bVecPush(nv, n->dotDotDotToken); bVecPush(nv, n->literal); bVecPush(nv, n->left);
  bVecPush(nv, n->right); bVecPush(nv, n->operatorToken); bVecPush(nv, n->condition); bVecPush(nv, n->whenTrue);
  bVecPush(nv, n->whenFalse); bVecPush(nv, n->thenStatement); bVecPush(nv, n->elseStatement); bVecPush(nv, n->statement);
  bVecPush(nv, n->tag); bVecPush(nv, n->tagName); bVecPush(nv, n->templateNode); bVecPush(nv, n->qualifier);
  bVecPush(nv, n->constraint); bVecPush(nv, n->defaultType); bVecPush(nv, n->typeName); bVecPush(nv, n->argument);
  bVecPush(nv, n->block); bVecPush(nv, n->finallyBlock); bVecPush(nv, n->catchClause); bVecPush(nv, n->variableDeclaration);
  bVecPush(nv, n->label); bVecPush(nv, n->incrementor); bVecPush(nv, n->declaration); bVecPush(nv, n->moduleReference);
  bVecPush(nv, n->moduleSpecifier); bVecPush(nv, n->importClause); bVecPush(nv, n->namedBindings);
  bVecPush(nv, n->propertyName); bVecPush(nv, n->checkType); bVecPush(nv, n->extendsType); bVecPush(nv, n->trueType);
  bVecPush(nv, n->falseType); bVecPush(nv, n->objectType); bVecPush(nv, n->indexType); bVecPush(nv, n->elementType);
  bVecPush(nv, n->nameOrPath); bVecPush(nv, n->endOfFileToken); bVecPush(nv, n->head); bVecPush(nv, n->exportClause);
  bVecPush(nv, n->node1); bVecPush(nv, n->node2); bVecPush(nv, n->node3); bVecPush(nv, n->node4); bVecPush(nv, n->node5);
  bVecPush(nv, n->typeExpression); bVecPush(nv, n->readonlyToken); bVecPush(nv, n->nameType); bVecPush(nv, n->exprName);
  bVecPush(nv, n->attributes_node); bVecPush(nv, n->typeParameter); bVecPush(nv, n->parameterName);
  bVecPush(nv, n->assertsModifier); bVecPush(nv, n->equalsGreaterThanToken); bVecPush(nv, n->colonToken);
  bVecPush(nv, n->questionDotToken); bVecPush(nv, n->argumentExpression); bVecPush(nv, n->operand);
  bVecPush(nv, n->objectAssignmentInitializer); bVecPush(nv, n->equalsToken); bVecPush(nv, n->declarationList);
  bVecPush(nv, n->caseBlock); bVecPush(nv, n->awaitModifier);
  bVecPushList(nv, n->modifiers); bVecPushList(nv, n->typeParameters); bVecPushList(nv, n->parameters);
  bVecPushList(nv, n->members); bVecPushList(nv, n->statements); bVecPushList(nv, n->arguments);
  bVecPushList(nv, n->elements); bVecPushList(nv, n->properties); bVecPushList(nv, n->heritageClauses);
  bVecPushList(nv, n->types); bVecPushList(nv, n->declarations); bVecPushList(nv, n->typeArguments);
  bVecPushList(nv, n->clauses); bVecPushList(nv, n->spans); bVecPushList(nv, n->tokens); bVecPushList(nv, n->qualifiers);
  bVecPushList(nv, n->assertions); bVecPushList(nv, n->attributes); bVecPushList(nv, n->expressions);
  bVecPushList(nv, n->list1); bVecPushList(nv, n->list2);
  /* insertion sort by pos (stable); lists are already in order, so this is linear for them */
  for (int i = 1; i < nv->n; i++) {
    TsNode *x = nv->v[i];
    int j = i - 1;
    while (j >= 0 && nv->v[j]->pos > x->pos) { nv->v[j + 1] = nv->v[j]; j--; }
    nv->v[j + 1] = x;
  }
}

/* ---- binding ---- */
static void bBind(TsNode *node);

static void bBindEachChild(TsNode *node) {
  BNodeVec nv = { NULL, 0, 0 };
  bChildrenSorted(node, &nv);
  for (int i = 0; i < nv.n; i++) bBind(nv.v[i]);
  free(nv.v);
}

static void bBindEachFunctionsFirst(TsList *stmts) {
  if (!stmts) return;
  for (int i = 0; i < stmts->n; i++)
    if (stmts->v[i]->kind == TSK_FunctionDeclaration) bBind(stmts->v[i]);
  for (int i = 0; i < stmts->n; i++)
    if (stmts->v[i]->kind != TSK_FunctionDeclaration) bBind(stmts->v[i]);
}

static bool bIsUseStrictPrologueDirective(TsNode *stmt) {
  TsNode *e = stmt->expression;
  return e && e->textLen == 10 && memcmp(e->text, "use strict", 10) == 0;
}

static void bUpdateStrictModeStatementList(TsList *stmts) {
  if (bInStrictMode || !stmts) return;
  for (int i = 0; i < stmts->n; i++) {
    TsNode *s = stmts->v[i];
    if (!(s->kind == TSK_ExpressionStatement && s->expression && s->expression->kind == TSK_StringLiteral)) return;
    if (bIsUseStrictPrologueDirective(s)) { bInStrictMode = true; return; }
  }
}

static bool bIsDestructuringAssignmentNode(TsNode *n) {
  return n->kind == TSK_BinaryExpression && n->operatorToken->kind == TSK_EqualsToken &&
         (n->left->kind == TSK_ObjectLiteralExpression || n->left->kind == TSK_ArrayLiteralExpression);
}
/* ---- control flow (binder.ts flow part): flow nodes are counted, the graph is built as in TypeScript ---- */
enum {
  FF_Unreachable = 1, FF_Start = 2, FF_BranchLabel = 4, FF_LoopLabel = 8, FF_Assignment = 16,
  FF_TrueCondition = 32, FF_FalseCondition = 64, FF_SwitchClause = 128, FF_ArrayMutation = 256, FF_Call = 512,
  FF_ReduceLabel = 1024, FF_Referenced = 2048, FF_Shared = 4096
};

typedef struct TsFlow {
  uint32_t flags;
  TsNode *node;
  struct TsFlow *ante;          /* antecedent of a non-label flow node */
  struct TsFlow **labelAnts;    /* antecedent list of a label (BranchLabel / LoopLabel) */
  int nLabelAnts, capLabelAnts;
  struct TsFlow *target;        /* ReduceLabel: target and antecedents */
  struct TsFlow **redAnts;
  int nRedAnts;
  int clauseStart, clauseEnd;   /* SwitchClause: the clause range (empty for the implicit default of a switch) */
  struct CLoopCache *loopCache; /* checker: the loop label's types by reference key (flowLoopCaches) */
  uint8_t reach;                /* checker: reachability of a shared node (0 not known, 1 reachable, 2 not) */
} TsFlow;

/* the two unreachable nodes are created once by createBinder in TypeScript, so they are not counted */
static TsFlow bUnreachableFlowObj = { FF_Unreachable, NULL, NULL, NULL, 0, 0, NULL, NULL, 0 };
static TsFlow bReportedUnreachableFlowObj = { FF_Unreachable, NULL, NULL, NULL, 0, 0, NULL, NULL, 0 };
#define bUnreachableFlow (&bUnreachableFlowObj)
#define bReportedUnreachableFlow (&bReportedUnreachableFlowObj)

typedef struct BActiveLabel { struct BActiveLabel *next; BName name; TsFlow *breakTarget, *continueTarget; bool referenced; } BActiveLabel;

static int32_t bFlowCount;
static TsFlow *bCurFlow;
static TsFlow *bBreakTarget, *bContinueTarget, *bReturnTarget, *bTrueTarget, *bFalseTarget, *bExceptionTarget, *bPreSwitchCaseFlow;
static BActiveLabel *bActiveLabels;
static bool bHasExplicitReturn, bInReturnPosition, bHasFlowEffects, bInAssignmentPattern;

static TsFlow *bNewFlow(uint32_t flags, TsNode *node, TsFlow *ante) {
  bFlowCount++;
#ifdef TS_BINDER_DEBUG_FLOW
  fprintf(stderr, "F %u %d %d\n", flags, node ? node->kind : -1, node ? node->pos : -1);
#endif
  TsFlow *f = ts_arena_zalloc(sizeof(TsFlow));
  f->flags = flags;
  f->node = node;
  f->ante = ante;
  return f;
}
static TsFlow *bBranchLabel(void) { return bNewFlow(FF_BranchLabel, NULL, NULL); }
static TsFlow *bLoopLabel(void) { return bNewFlow(FF_LoopLabel, NULL, NULL); }
static void bSetReferenced(TsFlow *f) { f->flags |= (f->flags & FF_Referenced) ? FF_Shared : FF_Referenced; }

static bool bLabelContains(TsFlow *label, TsFlow *f) {
  for (int i = 0; i < label->nLabelAnts; i++) if (label->labelAnts[i] == f) return true;
  return false;
}
static void bAddAntecedent(TsFlow *label, TsFlow *ante) {
  if (!(ante->flags & FF_Unreachable) && !bLabelContains(label, ante)) {
    if (label->nLabelAnts == label->capLabelAnts) {
      label->capLabelAnts = label->capLabelAnts ? label->capLabelAnts * 2 : 4;
      TsFlow **v = ts_arena_alloc(sizeof(TsFlow *) * (size_t)label->capLabelAnts);
      if (label->nLabelAnts) memcpy(v, label->labelAnts, sizeof(TsFlow *) * (size_t)label->nLabelAnts);
      label->labelAnts = v;
    }
    label->labelAnts[label->nLabelAnts++] = ante;
    bSetReferenced(ante);
  }
}
static TsFlow *bFinishLabel(TsFlow *flow) {
  if (flow->nLabelAnts == 0) return bUnreachableFlow;
  if (flow->nLabelAnts == 1) return flow->labelAnts[0];
  return flow;
}
static TsFlow *bReduceLabel(TsFlow *target, TsFlow **ants, int n, TsFlow *ante) {
  TsFlow *f = bNewFlow(FF_ReduceLabel, NULL, ante);
  f->target = target;
  f->redAnts = ants;
  f->nRedAnts = n;
  return f;
}
static TsFlow *bFlowCondition(uint32_t flags, TsFlow *ante, TsNode *expr);
static TsFlow *bFlowSwitchClause(TsFlow *ante, TsNode *sw, int s, int e) {
  bSetReferenced(ante);
  TsFlow *f = bNewFlow(FF_SwitchClause, sw, ante);
  f->clauseStart = s;
  f->clauseEnd = e;
  return f;
}
static TsFlow *bFlowMutation(uint32_t flags, TsFlow *ante, TsNode *node) {
  bSetReferenced(ante);
  bHasFlowEffects = true;
  TsFlow *result = bNewFlow(flags, node, ante);
  if (bExceptionTarget) bAddAntecedent(bExceptionTarget, result);
  return result;
}
static TsFlow *bFlowCall(TsFlow *ante, TsNode *node) {
  bSetReferenced(ante);
  bHasFlowEffects = true;
  return bNewFlow(FF_Call, node, ante);
}

/* ---- predicates (typescript.js utilities) ---- */
static bool bIsStringLiteralLike(TsNode *n) { return n && (n->kind == TSK_StringLiteral || n->kind == TSK_NoSubstitutionTemplateLiteral); }
static bool bIsOptionalChain(TsNode *n) {
  if (!(n->flags & TNF_OptionalChain)) return false;
  int k = n->kind;
  return k == TSK_PropertyAccessExpression || k == TSK_ElementAccessExpression || k == TSK_CallExpression || k == TSK_NonNullExpression;
}
static bool bIsOptionalChainRoot(TsNode *n) { return bIsOptionalChain(n) && n->kind != TSK_NonNullExpression && n->questionDotToken; }
static bool bIsExpressionOfOptionalChainRoot(TsNode *n) { return bIsOptionalChainRoot(n->parent) && n->parent->expression == n; }
static bool bIsOutermostOptionalChain(TsNode *n) {
  return !bIsOptionalChain(n->parent) || bIsOptionalChainRoot(n->parent) || n != n->parent->expression;
}
static bool bIsNullishCoalesce(TsNode *n) { return n->kind == TSK_BinaryExpression && n->operatorToken->kind == TSK_QuestionQuestionToken; }
static bool bIsLogicalOrCoalescingBinaryOperator(int t) {
  return t == TSK_AmpersandAmpersandToken || t == TSK_BarBarToken || t == TSK_QuestionQuestionToken;
}
static bool bIsLogicalOrCoalescingAssignmentOperator(int t) {
  return t == TSK_BarBarEqualsToken || t == TSK_AmpersandAmpersandEqualsToken || t == TSK_QuestionQuestionEqualsToken;
}
static bool bIsLogicalOrCoalescingBinaryExpression(TsNode *n) {
  return n->kind == TSK_BinaryExpression && bIsLogicalOrCoalescingBinaryOperator(n->operatorToken->kind);
}
static bool bIsLogicalOrCoalescingAssignmentExpression(TsNode *n) {
  return n->kind == TSK_BinaryExpression && bIsLogicalOrCoalescingAssignmentOperator(n->operatorToken->kind);
}
static bool bIsAssignmentOperatorKind(int t) { return t >= TSK_EqualsToken && t <= TSK_CaretEqualsToken; }
static bool bIsDottedName(TsNode *n) {
  return n->kind == TSK_Identifier || n->kind == TSK_ThisKeyword || n->kind == TSK_SuperKeyword || n->kind == TSK_MetaProperty ||
         (n->kind == TSK_PropertyAccessExpression && bIsDottedName(n->expression)) ||
         (n->kind == TSK_ParenthesizedExpression && bIsDottedName(n->expression));
}
static bool bIsLogicalExpression(TsNode *n) {
  for (;;) {
    if (n->kind == TSK_ParenthesizedExpression) n = n->expression;
    else if (n->kind == TSK_PrefixUnaryExpression && n->extra == TSK_ExclamationToken) n = n->operand;
    else return bIsLogicalOrCoalescingBinaryExpression(n);
  }
}
static bool bIsLogicalAssignmentExpression(TsNode *n) { return bIsLogicalOrCoalescingAssignmentExpression(bSkipParentheses(n)); }
static bool bIsStatementCondition(TsNode *n) {
  TsNode *p = n->parent;
  switch (p->kind) {
    case TSK_IfStatement: case TSK_WhileStatement: case TSK_DoStatement: return p->expression == n;
    case TSK_ForStatement: case TSK_ConditionalExpression: return p->condition == n;
  }
  return false;
}
static bool bIsTopLevelLogicalExpression(TsNode *n) {
  while (n->parent->kind == TSK_ParenthesizedExpression || (n->parent->kind == TSK_PrefixUnaryExpression && n->parent->extra == TSK_ExclamationToken))
    n = n->parent;
  return !bIsStatementCondition(n) && !bIsLogicalExpression(n->parent) && !(bIsOptionalChain(n->parent) && n->parent->expression == n);
}
static bool bIsNarrowableReference(TsNode *n);
static bool bIsLeftHandSideKind(int k) {
  switch (k) {
    case TSK_PropertyAccessExpression: case TSK_ElementAccessExpression: case TSK_NewExpression: case TSK_CallExpression:
    case TSK_TaggedTemplateExpression: case TSK_ArrayLiteralExpression: case TSK_ParenthesizedExpression:
    case TSK_ObjectLiteralExpression: case TSK_ClassExpression: case TSK_FunctionExpression: case TSK_Identifier:
    case TSK_RegularExpressionLiteral: case TSK_NumericLiteral: case TSK_BigIntLiteral: case TSK_StringLiteral:
    case TSK_NoSubstitutionTemplateLiteral: case TSK_TemplateExpression: case TSK_FalseKeyword: case TSK_NullKeyword:
    case TSK_ThisKeyword: case TSK_TrueKeyword: case TSK_SuperKeyword: case TSK_NonNullExpression: case TSK_MetaProperty:
    case TSK_ImportKeyword:
      return true;
  }
  return false;
}
static bool bIsNarrowableReference(TsNode *n) {
  switch (n->kind) {
    case TSK_Identifier: case TSK_ThisKeyword: case TSK_SuperKeyword: case TSK_MetaProperty: return true;
    case TSK_PropertyAccessExpression: case TSK_ParenthesizedExpression: case TSK_NonNullExpression:
      return bIsNarrowableReference(n->expression);
    case TSK_ElementAccessExpression:
      return (bIsStringLiteralLike(n->argumentExpression) || n->argumentExpression->kind == TSK_NumericLiteral ||
              bIsEntityNameExpression(n->argumentExpression)) && bIsNarrowableReference(n->expression);
    case TSK_BinaryExpression:
      return (n->operatorToken->kind == TSK_CommaToken && bIsNarrowableReference(n->right)) ||
             (bIsAssignmentOperatorKind(n->operatorToken->kind) && bIsLeftHandSideKind(bSkipParentheses(n->left)->kind));
  }
  return false;
}
static bool bContainsNarrowableReference(TsNode *n);
static bool bIsNarrowingExpression(TsNode *n);
static bool bIsNarrowableOperand(TsNode *n) {
  if (n->kind == TSK_ParenthesizedExpression) return bIsNarrowableOperand(n->expression);
  if (n->kind == TSK_BinaryExpression) {
    if (n->operatorToken->kind == TSK_EqualsToken) return bIsNarrowableOperand(n->left);
    if (n->operatorToken->kind == TSK_CommaToken) return bIsNarrowableOperand(n->right);
  }
  return bContainsNarrowableReference(n);
}
static bool bContainsNarrowableReference(TsNode *n) {
  return bIsNarrowableReference(n) || (bIsOptionalChain(n) && bContainsNarrowableReference(n->expression));
}
static bool bHasNarrowableArgument(TsNode *n) {
  if (n->arguments)
    for (int i = 0; i < n->arguments->n; i++)
      if (bContainsNarrowableReference(n->arguments->v[i])) return true;
  if (n->expression->kind == TSK_PropertyAccessExpression && bContainsNarrowableReference(n->expression->expression)) return true;
  return false;
}
static bool bIsNarrowingTypeofOperands(TsNode *e1, TsNode *e2) {
  return e1->kind == TSK_TypeOfExpression && bIsNarrowableOperand(e1->expression) && bIsStringLiteralLike(e2);
}
static bool bIsNarrowingBinaryExpression(TsNode *n) {
  switch (n->operatorToken->kind) {
    case TSK_EqualsToken: case TSK_BarBarEqualsToken: case TSK_AmpersandAmpersandEqualsToken: case TSK_QuestionQuestionEqualsToken:
      return bContainsNarrowableReference(n->left);
    case TSK_EqualsEqualsToken: case TSK_ExclamationEqualsToken: case TSK_EqualsEqualsEqualsToken: case TSK_ExclamationEqualsEqualsToken: {
      TsNode *l = bSkipParentheses(n->left), *r = bSkipParentheses(n->right);
      return bIsNarrowableOperand(l) || bIsNarrowableOperand(r) || bIsNarrowingTypeofOperands(r, l) || bIsNarrowingTypeofOperands(l, r) ||
             ((r->kind == TSK_TrueKeyword || r->kind == TSK_FalseKeyword) && bIsNarrowingExpression(l)) ||
             ((l->kind == TSK_TrueKeyword || l->kind == TSK_FalseKeyword) && bIsNarrowingExpression(r));
    }
    case TSK_InstanceOfKeyword: return bIsNarrowableOperand(n->left);
    case TSK_InKeyword: return bIsNarrowingExpression(n->right);
    case TSK_CommaToken: return bIsNarrowingExpression(n->right);
  }
  return false;
}
static bool bIsNarrowingExpression(TsNode *n) {
  switch (n->kind) {
    case TSK_Identifier: case TSK_ThisKeyword: return true;
    case TSK_PropertyAccessExpression: case TSK_ElementAccessExpression: return bContainsNarrowableReference(n);
    case TSK_CallExpression: return bHasNarrowableArgument(n);
    case TSK_ParenthesizedExpression:
      return bIsNarrowingExpression(n->expression);
    case TSK_NonNullExpression: return bIsNarrowingExpression(n->expression);
    case TSK_BinaryExpression: return bIsNarrowingBinaryExpression(n);
    case TSK_PrefixUnaryExpression: return n->extra == TSK_ExclamationToken && bIsNarrowingExpression(n->operand);
    case TSK_TypeOfExpression: return bIsNarrowingExpression(n->expression);
  }
  return false;
}

/* getAssignmentTarget: the assignment, update or for-in/of statement that writes node, if any */
static TsNode *bGetAssignmentTarget(TsNode *node) {
  TsNode *p = node->parent;
  for (;;) {
    switch (p->kind) {
      case TSK_BinaryExpression:
        return bIsAssignmentOperatorKind(p->operatorToken->kind) && p->left == node ? p : NULL;
      case TSK_PrefixUnaryExpression: case TSK_PostfixUnaryExpression:
        return (p->extra == TSK_PlusPlusToken || p->extra == TSK_MinusMinusToken) ? p : NULL;
      case TSK_ForInStatement: case TSK_ForOfStatement:
        return p->initializer == node ? p : NULL;
      case TSK_ParenthesizedExpression: case TSK_ArrayLiteralExpression: case TSK_SpreadElement: case TSK_NonNullExpression:
        node = p;
        break;
      case TSK_SpreadAssignment:
        node = p->parent;
        break;
      case TSK_ShorthandPropertyAssignment:
        if (p->name != node) return NULL;
        node = p->parent;
        break;
      case TSK_PropertyAssignment:
        if (p->name == node) return NULL;
        node = p->parent;
        break;
      default:
        return NULL;
    }
    p = node->parent;
  }
}
static bool bIsAssignmentTarget(TsNode *n) { return bGetAssignmentTarget(n) != NULL; }

static bool bIsStatementButNotDeclarationKind(int k) {
  switch (k) {
    case TSK_BreakStatement: case TSK_ContinueStatement: case TSK_DebuggerStatement: case TSK_DoStatement:
    case TSK_ExpressionStatement: case TSK_EmptyStatement: case TSK_ForInStatement: case TSK_ForOfStatement:
    case TSK_ForStatement: case TSK_IfStatement: case TSK_LabeledStatement: case TSK_ReturnStatement:
    case TSK_SwitchStatement: case TSK_ThrowStatement: case TSK_TryStatement: case TSK_VariableStatement:
    case TSK_WhileStatement: case TSK_WithStatement: case TSK_NotEmittedStatement:
      return true;
  }
  return false;
}

/* checkUnreachable: reports (not counted) and moves the flow to the reported node when code is unreachable */
static bool bCheckUnreachable(TsNode *node) {
  if (!(bCurFlow->flags & FF_Unreachable)) return false;
  if (bCurFlow == bUnreachableFlow) {
    bool reportError = (bIsStatementButNotDeclarationKind(node->kind) && node->kind != TSK_EmptyStatement) ||
                       node->kind == TSK_ClassDeclaration ||
                       (node->kind == TSK_EnumDeclaration && !bIsEnumConst(node)) ||
                       (node->kind == TSK_ModuleDeclaration && bModuleInstanceStateCached(node) == MI_Instantiated);
    if (reportError) bCurFlow = bReportedUnreachableFlow;
  }
  return true;
}

static TsFlow *bFlowCondition(uint32_t flags, TsFlow *ante, TsNode *expr) {
  if (ante->flags & FF_Unreachable) return ante;
  if (!expr) return (flags & FF_TrueCondition) ? ante : bUnreachableFlow;
  if (((expr->kind == TSK_TrueKeyword && (flags & FF_FalseCondition)) || (expr->kind == TSK_FalseKeyword && (flags & FF_TrueCondition))) &&
      !bIsExpressionOfOptionalChainRoot(expr) && !bIsNullishCoalesce(expr->parent))
    return bUnreachableFlow;
  if (!bIsNarrowingExpression(expr)) return ante;
  bSetReferenced(ante);
  return bNewFlow(flags, expr, ante);
}

/* ---- binding of the flow graph ---- */
static void bBindAssignmentTargetFlow(TsNode *node);
static void bBindCondition(TsNode *node, TsFlow *trueT, TsFlow *falseT);
static void bDoWithConditional(void (*action)(TsNode *), TsNode *node, TsFlow *trueT, TsFlow *falseT);
static bool bBindChildrenFlowDispatch(TsNode *node);
static void bBindEachStatements(TsList *l);

static void bBindCondWrap(TsNode *node) { bBind(node); }

static void bDoWithConditional(void (*action)(TsNode *), TsNode *node, TsFlow *trueT, TsFlow *falseT) {
  TsFlow *savedT = bTrueTarget, *savedF = bFalseTarget;
  bTrueTarget = trueT;
  bFalseTarget = falseT;
  action(node);
  bTrueTarget = savedT;
  bFalseTarget = savedF;
}

static void bBindCondition(TsNode *node, TsFlow *trueT, TsFlow *falseT) {
  bDoWithConditional(bBindCondWrap, node, trueT, falseT);
  if (!node || (!bIsLogicalAssignmentExpression(node) && !bIsLogicalExpression(node) &&
                !(bIsOptionalChain(node) && bIsOutermostOptionalChain(node)))) {
    bAddAntecedent(trueT, bFlowCondition(FF_TrueCondition, bCurFlow, node));
    bAddAntecedent(falseT, bFlowCondition(FF_FalseCondition, bCurFlow, node));
  }
}

static void bBindIterative(TsNode *node, TsFlow *breakT, TsFlow *contT) {
  TsFlow *sb = bBreakTarget, *sc = bContinueTarget;
  bBreakTarget = breakT;
  bContinueTarget = contT;
  bBind(node);
  bBreakTarget = sb;
  bContinueTarget = sc;
}

static TsFlow *bSetContinueTarget(TsNode *node, TsFlow *target) {
  BActiveLabel *label = bActiveLabels;
  while (label && node->parent->kind == TSK_LabeledStatement) {
    label->continueTarget = target;
    label = label->next;
    node = node->parent;
  }
  return target;
}

static void bBindWhile(TsNode *node) {
  TsFlow *preWhile = bSetContinueTarget(node, bLoopLabel());
  TsFlow *preBody = bBranchLabel();
  TsFlow *postWhile = bBranchLabel();
  bAddAntecedent(preWhile, bCurFlow);
  bCurFlow = preWhile;
  bBindCondition(node->expression, preBody, postWhile);
  bCurFlow = bFinishLabel(preBody);
  bBindIterative(node->statement, postWhile, preWhile);
  bAddAntecedent(preWhile, bCurFlow);
  bCurFlow = bFinishLabel(postWhile);
}

static void bBindDo(TsNode *node) {
  TsFlow *preDo = bLoopLabel();
  TsFlow *preCond = bSetContinueTarget(node, bBranchLabel());
  TsFlow *postDo = bBranchLabel();
  bAddAntecedent(preDo, bCurFlow);
  bCurFlow = preDo;
  bBindIterative(node->statement, postDo, preCond);
  bAddAntecedent(preCond, bCurFlow);
  bCurFlow = bFinishLabel(preCond);
  bBindCondition(node->expression, preDo, postDo);
  bCurFlow = bFinishLabel(postDo);
}

static void bBindFor(TsNode *node) {
  TsFlow *preLoop = bSetContinueTarget(node, bLoopLabel());
  TsFlow *preBody = bBranchLabel();
  TsFlow *preInc = bBranchLabel();
  TsFlow *postLoop = bBranchLabel();
  bBind(node->initializer);
  bAddAntecedent(preLoop, bCurFlow);
  bCurFlow = preLoop;
  bBindCondition(node->condition, preBody, postLoop);
  bCurFlow = bFinishLabel(preBody);
  bBindIterative(node->statement, postLoop, preInc);
  bAddAntecedent(preInc, bCurFlow);
  bCurFlow = bFinishLabel(preInc);
  bBind(node->incrementor);
  bAddAntecedent(preLoop, bCurFlow);
  bCurFlow = bFinishLabel(postLoop);
}

static void bBindForInOf(TsNode *node) {
  TsFlow *preLoop = bSetContinueTarget(node, bLoopLabel());
  TsFlow *postLoop = bBranchLabel();
  bBind(node->expression);
  bAddAntecedent(preLoop, bCurFlow);
  bCurFlow = preLoop;
  if (node->kind == TSK_ForOfStatement) bBind(node->awaitModifier);
  bAddAntecedent(postLoop, bCurFlow);
  bBind(node->initializer);
  if (node->initializer->kind != TSK_VariableDeclarationList) bBindAssignmentTargetFlow(node->initializer);
  bBindIterative(node->statement, postLoop, preLoop);
  bAddAntecedent(preLoop, bCurFlow);
  bCurFlow = bFinishLabel(postLoop);
}

static void bBindIf(TsNode *node) {
  TsFlow *thenL = bBranchLabel(), *elseL = bBranchLabel(), *postIf = bBranchLabel();
  bBindCondition(node->expression, thenL, elseL);
  bCurFlow = bFinishLabel(thenL);
  bBind(node->thenStatement);
  bAddAntecedent(postIf, bCurFlow);
  bCurFlow = bFinishLabel(elseL);
  bBind(node->elseStatement);
  bAddAntecedent(postIf, bCurFlow);
  bCurFlow = bFinishLabel(postIf);
}

static void bBindReturnOrThrow(TsNode *node) {
  bool savedIn = bInReturnPosition;
  bInReturnPosition = true;
  bBind(node->expression);
  bInReturnPosition = savedIn;
  if (node->kind == TSK_ReturnStatement) {
    bHasExplicitReturn = true;
    if (bReturnTarget) bAddAntecedent(bReturnTarget, bCurFlow);
  }
  bCurFlow = bUnreachableFlow;
  bHasFlowEffects = true;
}

static BActiveLabel *bFindActiveLabel(const char *s, int n) {
  for (BActiveLabel *l = bActiveLabels; l; l = l->next)
    if (l->name.n == n && memcmp(l->name.s, s, (size_t)n) == 0) return l;
  return NULL;
}

static void bBindBreakOrContinueFlow(TsNode *node, TsFlow *breakT, TsFlow *contT) {
  TsFlow *flowLabel = node->kind == TSK_BreakStatement ? breakT : contT;
  if (flowLabel) {
    bAddAntecedent(flowLabel, bCurFlow);
    bCurFlow = bUnreachableFlow;
    bHasFlowEffects = true;
  }
}

static void bBindBreakOrContinue(TsNode *node) {
  bBind(node->label);
  if (node->label) {
    BActiveLabel *active = bFindActiveLabel(node->label->text, node->label->textLen);
    if (active) {
      active->referenced = true;
      bBindBreakOrContinueFlow(node, active->breakTarget, active->continueTarget);
    }
  } else {
    bBindBreakOrContinueFlow(node, bBreakTarget, bContinueTarget);
  }
}

static TsFlow **bConcat(TsFlow *a, TsFlow *b, int *n) {
  int na = a ? a->nLabelAnts : 0, nb = b ? b->nLabelAnts : 0;
  *n = na + nb;
  if (*n == 0) return NULL;
  TsFlow **v = ts_arena_alloc(sizeof(TsFlow *) * (size_t)*n);
  if (na) memcpy(v, a->labelAnts, sizeof(TsFlow *) * (size_t)na);
  if (nb) memcpy(v + na, b->labelAnts, sizeof(TsFlow *) * (size_t)nb);
  return v;
}

static void bBindTry(TsNode *node) {
  TsFlow *saveReturn = bReturnTarget, *saveExc = bExceptionTarget;
  TsFlow *normalExit = bBranchLabel();
  TsFlow *returnLabel = bBranchLabel();
  TsFlow *exceptionLabel = bBranchLabel();
  if (node->finallyBlock) bReturnTarget = returnLabel;
  bAddAntecedent(exceptionLabel, bCurFlow);
  bExceptionTarget = exceptionLabel;
  bBind(node->block);
  bAddAntecedent(normalExit, bCurFlow);
  if (node->catchClause) {
    bCurFlow = bFinishLabel(exceptionLabel);
    exceptionLabel = bBranchLabel();
    bAddAntecedent(exceptionLabel, bCurFlow);
    bExceptionTarget = exceptionLabel;
    bBind(node->catchClause);
    bAddAntecedent(normalExit, bCurFlow);
  }
  bReturnTarget = saveReturn;
  bExceptionTarget = saveExc;
  if (node->finallyBlock) {
    TsFlow *finallyLabel = bBranchLabel();
    /* concatenate(normalExit, exception, return) */
    /* concatenate of the antecedent lists; the temporary holder is not a flow node */
    int n1 = 0, n3 = 0;
    TsFlow **c1 = bConcat(normalExit, exceptionLabel, &n1);
    TsFlow tmp;
    memset(&tmp, 0, sizeof tmp);
    tmp.labelAnts = c1;
    tmp.nLabelAnts = n1;
    TsFlow **c2 = bConcat(&tmp, returnLabel, &n3);
    finallyLabel->labelAnts = c2;
    finallyLabel->nLabelAnts = n3;
    bCurFlow = finallyLabel;
    bBind(node->finallyBlock);
    if (bCurFlow->flags & FF_Unreachable) {
      bCurFlow = bUnreachableFlow;
    } else {
      if (bReturnTarget && returnLabel->nLabelAnts) {
        bAddAntecedent(bReturnTarget, bReduceLabel(finallyLabel, returnLabel->labelAnts, returnLabel->nLabelAnts, bCurFlow));
      }
      if (bExceptionTarget && exceptionLabel->nLabelAnts) {
        bAddAntecedent(bExceptionTarget, bReduceLabel(finallyLabel, exceptionLabel->labelAnts, exceptionLabel->nLabelAnts, bCurFlow));
      }
      bCurFlow = normalExit->nLabelAnts ? bReduceLabel(finallyLabel, normalExit->labelAnts, normalExit->nLabelAnts, bCurFlow)
                                        : bUnreachableFlow;
    }
  } else {
    bCurFlow = bFinishLabel(normalExit);
  }
}

static void bBindSwitch(TsNode *node) {
  TsFlow *post = bBranchLabel();
  bBind(node->expression);
  TsFlow *saveBreak = bBreakTarget, *savePre = bPreSwitchCaseFlow;
  bBreakTarget = post;
  bPreSwitchCaseFlow = bCurFlow;
  bBind(node->caseBlock);
  bAddAntecedent(post, bCurFlow);
  bool hasDefault = false;
  TsList *cl = node->caseBlock->clauses;
  for (int i = 0; cl && i < cl->n; i++) if (cl->v[i]->kind == TSK_DefaultClause) hasDefault = true;
  if (!hasDefault) bAddAntecedent(post, bFlowSwitchClause(bPreSwitchCaseFlow, node, 0, 0));
  bBreakTarget = saveBreak;
  bPreSwitchCaseFlow = savePre;
  bCurFlow = bFinishLabel(post);
}

static void bBindCaseBlock(TsNode *node) {
  TsList *clauses = node->clauses;
  bool isNarrowingSwitch = node->parent->expression->kind == TSK_TrueKeyword || bIsNarrowingExpression(node->parent->expression);
  TsFlow *fallthrough = bUnreachableFlow;
  for (int i = 0; i < clauses->n; i++) {
    int clauseStart = i;
    while (clauses->v[i]->statements->n == 0 && i + 1 < clauses->n) {
      if (fallthrough == bUnreachableFlow) bCurFlow = bPreSwitchCaseFlow;
      bBind(clauses->v[i]);
      i++;
    }
    TsFlow *preCase = bBranchLabel();
    bAddAntecedent(preCase, isNarrowingSwitch ? bFlowSwitchClause(bPreSwitchCaseFlow, node->parent, clauseStart, i + 1) : bPreSwitchCaseFlow);
    bAddAntecedent(preCase, fallthrough);
    bCurFlow = bFinishLabel(preCase);
    TsNode *clause = clauses->v[i];
    bBind(clause);
    fallthrough = bCurFlow;
  }
}

static void bBindCaseClause(TsNode *node) {
  TsFlow *saveCur = bCurFlow;
  bCurFlow = bPreSwitchCaseFlow;
  bBind(node->expression);
  bCurFlow = saveCur;
  bBindEachStatements(node->statements);
}

static void bMaybeBindExpressionFlowIfCall(TsNode *node) {
  if (node->kind == TSK_CallExpression) {
    if (node->expression->kind != TSK_SuperKeyword && bIsDottedName(node->expression)) bCurFlow = bFlowCall(bCurFlow, node);
  }
}

static void bBindLabeled(TsNode *node) {
  TsFlow *postStmt = bBranchLabel();
  BActiveLabel *lab = ts_arena_zalloc(sizeof(BActiveLabel));
  lab->next = bActiveLabels;
  lab->name.s = node->label->text;
  lab->name.n = node->label->textLen;
  lab->breakTarget = postStmt;
  bActiveLabels = lab;
  bBind(node->label);
  bBind(node->statement);
  bActiveLabels = bActiveLabels->next;
  bAddAntecedent(postStmt, bCurFlow);
  bCurFlow = bFinishLabel(postStmt);
}

static void bBindDestructuringTargetFlow(TsNode *node) {
  if (node->kind == TSK_BinaryExpression && node->operatorToken->kind == TSK_EqualsToken) bBindAssignmentTargetFlow(node->left);
  else bBindAssignmentTargetFlow(node);
}

static void bBindAssignmentTargetFlow(TsNode *node) {
  if (bIsNarrowableReference(node)) {
    bCurFlow = bFlowMutation(FF_Assignment, bCurFlow, node);
  } else if (node->kind == TSK_ArrayLiteralExpression) {
    for (int i = 0; node->elements && i < node->elements->n; i++) {
      TsNode *e = node->elements->v[i];
      if (e->kind == TSK_SpreadElement) bBindAssignmentTargetFlow(e->expression);
      else bBindDestructuringTargetFlow(e);
    }
  } else if (node->kind == TSK_ObjectLiteralExpression) {
    for (int i = 0; node->properties && i < node->properties->n; i++) {
      TsNode *p = node->properties->v[i];
      if (p->kind == TSK_PropertyAssignment) bBindDestructuringTargetFlow(p->initializer);
      else if (p->kind == TSK_ShorthandPropertyAssignment) bBindAssignmentTargetFlow(p->name);
      else if (p->kind == TSK_SpreadAssignment) bBindAssignmentTargetFlow(p->expression);
    }
  }
}

static void bBindLogicalLike(TsNode *node, TsFlow *trueT, TsFlow *falseT) {
  TsFlow *preRight = bBranchLabel();
  int op = node->operatorToken->kind;
  if (op == TSK_AmpersandAmpersandToken || op == TSK_AmpersandAmpersandEqualsToken) bBindCondition(node->left, preRight, falseT);
  else bBindCondition(node->left, trueT, preRight);
  bCurFlow = bFinishLabel(preRight);
  bBind(node->operatorToken);
  if (bIsLogicalOrCoalescingAssignmentOperator(op)) {
    bDoWithConditional(bBind, node->right, trueT, falseT);
    bBindAssignmentTargetFlow(node->left);
    bAddAntecedent(trueT, bFlowCondition(FF_TrueCondition, bCurFlow, node));
    bAddAntecedent(falseT, bFlowCondition(FF_FalseCondition, bCurFlow, node));
  } else {
    bBindCondition(node->right, trueT, falseT);
  }
}

static void bBindEachStatements(TsList *l) {
  for (int i = 0; l && i < l->n; i++) bBind(l->v[i]);
}

static void bBindBinaryFlow(TsNode *node);

/* the binary-expression trampoline of binder.ts, written as the equivalent recursion */
static void bMaybeBind2(TsNode *n) {
  if (n && n->kind == TSK_BinaryExpression && !bIsDestructuringAssignmentNode(n)) {
    /* the trampoline's onEnter: set the parent, bind the node's parts with it as parent, restore */
    n->parent = bParent;
    TsNode *saveParent = bParent;
    bParent = n;
    bBindBinaryFlow(n);
    bParent = saveParent;
    return;
  }
  bBind(n);
}

static void bBindBinaryFlow(TsNode *node) {
  bool skip = false;
  int op = node->operatorToken->kind;
  if (bIsLogicalOrCoalescingBinaryOperator(op) || bIsLogicalOrCoalescingAssignmentOperator(op)) {
    if (bIsTopLevelLogicalExpression(node)) {
      TsFlow *post = bBranchLabel();
      TsFlow *saveCur = bCurFlow;
      bool saveHas = bHasFlowEffects;
      bHasFlowEffects = false;
      bBindLogicalLike(node, post, post);
      bCurFlow = bHasFlowEffects ? bFinishLabel(post) : saveCur;
      if (!bHasFlowEffects) bHasFlowEffects = saveHas;
    } else {
      bBindLogicalLike(node, bTrueTarget, bFalseTarget);
    }
    skip = true;
  }
  if (!skip) {
    bMaybeBind2(node->left);
    if (op == TSK_CommaToken) bMaybeBindExpressionFlowIfCall(node->left);
    bBind(node->operatorToken);
    bMaybeBind2(node->right);
    if (op == TSK_CommaToken) bMaybeBindExpressionFlowIfCall(node->right);
    if (bIsAssignmentOperatorKind(op) && !bIsAssignmentTarget(node)) {
      bBindAssignmentTargetFlow(node->left);
      if (op == TSK_EqualsToken && node->left->kind == TSK_ElementAccessExpression) {
        if (bIsNarrowableOperand(node->left->expression)) bCurFlow = bFlowMutation(FF_ArrayMutation, bCurFlow, node);
      }
    }
  }
}

static void bBindDestructuringAssignmentFlow(TsNode *node) {
  if (bInAssignmentPattern) {
    bInAssignmentPattern = false;
    bBind(node->operatorToken);
    bBind(node->right);
    bInAssignmentPattern = true;
    bBind(node->left);
  } else {
    bInAssignmentPattern = true;
    bBind(node->left);
    bInAssignmentPattern = false;
    bBind(node->operatorToken);
    bBind(node->right);
  }
  bBindAssignmentTargetFlow(node->left);
}

static void bBindPrefixUnaryFlow(TsNode *node) {
  if (node->extra == TSK_ExclamationToken) {
    TsFlow *saveTrue = bTrueTarget;
    bTrueTarget = bFalseTarget;
    bFalseTarget = saveTrue;
    bBindEachChild(node);
    bFalseTarget = bTrueTarget;
    bTrueTarget = saveTrue;
  } else {
    bBindEachChild(node);
    if (node->extra == TSK_PlusPlusToken || node->extra == TSK_MinusMinusToken) bBindAssignmentTargetFlow(node->operand);
  }
}

static void bBindPostfixUnaryFlow(TsNode *node) {
  bBindEachChild(node);
  if (node->extra == TSK_PlusPlusToken || node->extra == TSK_MinusMinusToken) bBindAssignmentTargetFlow(node->operand);
}

static void bBindDeleteFlow(TsNode *node) {
  bBindEachChild(node);
  if (node->expression->kind == TSK_PropertyAccessExpression) bBindAssignmentTargetFlow(node->expression);
}

static void bBindConditionalFlow(TsNode *node) {
  TsFlow *trueL = bBranchLabel(), *falseL = bBranchLabel(), *postL = bBranchLabel();
  TsFlow *saveCur = bCurFlow;
  bool saveHas = bHasFlowEffects;
  bHasFlowEffects = false;
  bBindCondition(node->condition, trueL, falseL);
  bCurFlow = bFinishLabel(trueL);
  bBind(node->questionToken);
  bBind(node->whenTrue);
  bAddAntecedent(postL, bCurFlow);
  bCurFlow = bFinishLabel(falseL);
  bBind(node->colonToken);
  bBind(node->whenFalse);
  bAddAntecedent(postL, bCurFlow);
  bCurFlow = bHasFlowEffects ? bFinishLabel(postL) : saveCur;
  if (!bHasFlowEffects) bHasFlowEffects = saveHas;
}

static void bBindInitializedVariable(TsNode *node) {
  TsNode *name = node->kind != TSK_OmittedExpression ? node->name : NULL;
  if (bIsBindingPattern(name)) {
    for (int i = 0; name->elements && i < name->elements->n; i++) bBindInitializedVariable(name->elements->v[i]);
  } else {
    bCurFlow = bFlowMutation(FF_Assignment, bCurFlow, node);
  }
}

static void bBindVariableDeclarationFlow(TsNode *node) {
  bBindEachChild(node);
  if (node->initializer || node->parent->parent->kind == TSK_ForInStatement || node->parent->parent->kind == TSK_ForOfStatement)
    bBindInitializedVariable(node);
}

static void bBindInitializer(TsNode *node) {
  if (!node) return;
  TsFlow *entry = bCurFlow;
  bBind(node);
  if (entry == bUnreachableFlow || entry == bCurFlow) return;
  TsFlow *exit = bBranchLabel();
  bAddAntecedent(exit, entry);
  bAddAntecedent(exit, bCurFlow);
  bCurFlow = bFinishLabel(exit);
}

static void bBindBindingElementFlow(TsNode *node) {
  bBind(node->dotDotDotToken);
  bBind(node->propertyName);
  bBindInitializer(node->initializer);
  bBind(node->name);
}

static void bBindParameterFlow(TsNode *node) {
  bBindEachStatements(node->modifiers);
  bBind(node->dotDotDotToken);
  bBind(node->questionToken);
  bBind(node->type);
  bBindInitializer(node->initializer);
  bBind(node->name);
}

static void bBindOptionalExpression(TsNode *node, TsFlow *trueT, TsFlow *falseT) {
  bDoWithConditional(bBind, node, trueT, falseT);
  if (!bIsOptionalChain(node) || bIsOutermostOptionalChain(node)) {
    bAddAntecedent(trueT, bFlowCondition(FF_TrueCondition, bCurFlow, node));
    bAddAntecedent(falseT, bFlowCondition(FF_FalseCondition, bCurFlow, node));
  }
}

static void bBindOptionalChainRest(TsNode *node) {
  switch (node->kind) {
    case TSK_PropertyAccessExpression:
      bBind(node->questionDotToken);
      bBind(node->name);
      break;
    case TSK_ElementAccessExpression:
      bBind(node->questionDotToken);
      bBind(node->argumentExpression);
      break;
    case TSK_CallExpression:
      bBind(node->questionDotToken);
      bBindEachStatements(node->typeArguments);
      bBindEachStatements(node->arguments);
      break;
  }
}

static void bBindOptionalChain(TsNode *node, TsFlow *trueT, TsFlow *falseT) {
  TsFlow *preChain = bIsOptionalChainRoot(node) ? bBranchLabel() : NULL;
  bBindOptionalExpression(node->expression, preChain ? preChain : trueT, falseT);
  if (preChain) bCurFlow = bFinishLabel(preChain);
  bDoWithConditional(bBindOptionalChainRest, node, trueT, falseT);
  if (bIsOutermostOptionalChain(node)) {
    bAddAntecedent(trueT, bFlowCondition(FF_TrueCondition, bCurFlow, node));
    bAddAntecedent(falseT, bFlowCondition(FF_FalseCondition, bCurFlow, node));
  }
}

static void bBindOptionalChainFlow(TsNode *node) {
  if (bIsTopLevelLogicalExpression(node)) {
    TsFlow *post = bBranchLabel();
    TsFlow *saveCur = bCurFlow;
    bool saveHas = bHasFlowEffects;
    bBindOptionalChain(node, post, post);
    bCurFlow = bHasFlowEffects ? bFinishLabel(post) : saveCur;
    if (!bHasFlowEffects) bHasFlowEffects = saveHas;
  } else {
    bBindOptionalChain(node, bTrueTarget, bFalseTarget);
  }
}

static void bBindAccessOrNonNullFlow(TsNode *node) {
  if (bIsOptionalChain(node)) bBindOptionalChainFlow(node);
  else bBindEachChild(node);
}

static void bBindCallFlow(TsNode *node) {
  if (bIsOptionalChain(node)) {
    bBindOptionalChainFlow(node);
  } else {
    TsNode *expr = bSkipParentheses(node->expression);
    if (expr->kind == TSK_FunctionExpression || expr->kind == TSK_ArrowFunction) {
      bBindEachStatements(node->typeArguments);
      bBindEachStatements(node->arguments);
      bBind(node->expression);
    } else {
      bBindEachChild(node);
      if (node->expression->kind == TSK_SuperKeyword) bCurFlow = bFlowCall(bCurFlow, node);
    }
  }
  if (node->expression->kind == TSK_PropertyAccessExpression) {
    TsNode *pa = node->expression;
    if (pa->name->kind == TSK_Identifier && bIsNarrowableOperand(pa->expression) &&
        ((pa->name->textLen == 4 && memcmp(pa->name->text, "push", 4) == 0) ||
         (pa->name->textLen == 7 && memcmp(pa->name->text, "unshift", 7) == 0)))
      bCurFlow = bFlowMutation(FF_ArrayMutation, bCurFlow, node);
  }
}

/* the statement, expression and declaration dispatch of bindChildren (flow part) */
static bool bBindChildrenFlowDispatch(TsNode *node) {
  switch (node->kind) {
    case TSK_WhileStatement: bBindWhile(node); return true;
    case TSK_DoStatement: bBindDo(node); return true;
    case TSK_ForStatement: bBindFor(node); return true;
    case TSK_ForInStatement: case TSK_ForOfStatement: bBindForInOf(node); return true;
    case TSK_IfStatement: bBindIf(node); return true;
    case TSK_ReturnStatement: case TSK_ThrowStatement: bBindReturnOrThrow(node); return true;
    case TSK_BreakStatement: case TSK_ContinueStatement: bBindBreakOrContinue(node); return true;
    case TSK_TryStatement: bBindTry(node); return true;
    case TSK_SwitchStatement: bBindSwitch(node); return true;
    case TSK_CaseBlock: bBindCaseBlock(node); return true;
    case TSK_CaseClause: bBindCaseClause(node); return true;
    case TSK_ExpressionStatement: bBind(node->expression); bMaybeBindExpressionFlowIfCall(node->expression); return true;
    case TSK_LabeledStatement: bBindLabeled(node); return true;
    case TSK_PrefixUnaryExpression: bBindPrefixUnaryFlow(node); return true;
    case TSK_PostfixUnaryExpression: bBindPostfixUnaryFlow(node); return true;
    case TSK_BinaryExpression: bBindBinaryFlow(node); return true;
    case TSK_DeleteExpression: bBindDeleteFlow(node); return true;
    case TSK_ConditionalExpression: bBindConditionalFlow(node); return true;
    case TSK_VariableDeclaration: bBindVariableDeclarationFlow(node); return true;
    case TSK_PropertyAccessExpression: case TSK_ElementAccessExpression: bBindAccessOrNonNullFlow(node); return true;
    case TSK_CallExpression: bBindCallFlow(node); return true;
    case TSK_NonNullExpression: bBindAccessOrNonNullFlow(node); return true;
    case TSK_BindingElement: bBindBindingElementFlow(node); return true;
    case TSK_Parameter: bBindParameterFlow(node); return true;
    default: return false;
  }
}

static void bBindChildren(TsNode *node) {
  bool saveIAP = bInAssignmentPattern;
  bInAssignmentPattern = false;
  if (bCheckUnreachable(node)) {
    bBindEachChild(node);
    bInAssignmentPattern = saveIAP;
    return;
  }
  switch (node->kind) {
    case TSK_SourceFile:
      bBindEachFunctionsFirst(node->statements);
      bBind(node->endOfFileToken);
      break;
    case TSK_Block: case TSK_ModuleBlock:
      bBindEachFunctionsFirst(node->statements);
      break;
    case TSK_BinaryExpression:
      if (bIsDestructuringAssignmentNode(node)) {
        bInAssignmentPattern = saveIAP;
        bBindDestructuringAssignmentFlow(node);
        return;
      }
      bBindBinaryFlow(node);
      break;
    case TSK_ObjectLiteralExpression: case TSK_ArrayLiteralExpression: case TSK_PropertyAssignment: case TSK_SpreadElement:
      bInAssignmentPattern = saveIAP;
      bBindEachChild(node);
      break;
    default:
      if (!bBindChildrenFlowDispatch(node)) bBindEachChild(node);
      break;
  }
  bInAssignmentPattern = saveIAP;
}

static TsNode *bImmediatelyInvoked(TsNode *func) {
  if (func->kind == TSK_FunctionExpression || func->kind == TSK_ArrowFunction) {
    TsNode *prev = func, *parent = func->parent;
    while (parent->kind == TSK_ParenthesizedExpression) { prev = parent; parent = parent->parent; }
    if (parent->kind == TSK_CallExpression && parent->expression == prev) return parent;
  }
  return NULL;
}

static void bBindContainer(TsNode *node, uint32_t cf) {
  TsNode *saveContainer = bContainer, *saveBlock = bBlockScopeContainer;
  bool savedInReturn = bInReturnPosition;
  if (node->kind == TSK_ArrowFunction && node->body->kind != TSK_Block) bInReturnPosition = true;
  if (cf & CF_IsContainer) {
    bContainer = bBlockScopeContainer = node;
    if (cf & CF_HasLocals) node->locals = bTableNew();
  } else if (cf & CF_IsBlockScopedContainer) {
    bBlockScopeContainer = node;
    if (cf & CF_HasLocals) node->locals = NULL;
  }
  if (cf & CF_IsControlFlowContainer) {
    TsFlow *saveCur = bCurFlow, *saveBreak = bBreakTarget, *saveContinue = bContinueTarget, *saveReturn = bReturnTarget;
    TsFlow *saveExc = bExceptionTarget;
    BActiveLabel *saveActive = bActiveLabels;
    bool saveHasExplicitReturn = bHasExplicitReturn;
    bool isIIFE = ((cf & CF_IsFunctionExpression) && !(bModifierFlags(node) & TMF_Async) && !node->asteriskToken &&
                   bImmediatelyInvoked(node) != NULL) || node->kind == TSK_ClassStaticBlockDeclaration;
    if (!isIIFE) {
      bCurFlow = bNewFlow(FF_Start, NULL, NULL);
      if (cf & (CF_IsFunctionExpression | CF_IsObjectLiteralOrClassExpressionMethodOrAccessor)) bCurFlow->node = node;
    }
    bReturnTarget = (isIIFE || node->kind == TSK_Constructor) ? bBranchLabel() : NULL;
    bExceptionTarget = NULL;
    bBreakTarget = NULL;
    bContinueTarget = NULL;
    bActiveLabels = NULL;
    bHasExplicitReturn = false;
    bBindChildren(node);
    if (bReturnTarget) {
      bAddAntecedent(bReturnTarget, bCurFlow);
      bCurFlow = bFinishLabel(bReturnTarget);
    }
    if (!isIIFE) bCurFlow = saveCur;
    bBreakTarget = saveBreak;
    bContinueTarget = saveContinue;
    bReturnTarget = saveReturn;
    bExceptionTarget = saveExc;
    bActiveLabels = saveActive;
    bHasExplicitReturn = saveHasExplicitReturn;
  } else if (cf & CF_IsInterface) {
    /* the binder's interface container: ContainsThis is set when a this type was bound inside it (61839) */
    bool saveSeen = bSeenThis;
    bSeenThis = false;
    bBindChildren(node);
    node->flags = bSeenThis ? (node->flags | TNF_ContainsThis) : (node->flags & ~(uint32_t)TNF_ContainsThis);
    bSeenThis = saveSeen;
  } else {
    bBindChildren(node);
  }
  bInReturnPosition = savedInReturn;
  bContainer = saveContainer;
  bBlockScopeContainer = saveBlock;
}

static void bBindTypeParameter(TsNode *node) {
  if (node->parent->kind == TSK_InferType) {
    TsNode *ext = NULL;
    for (TsNode *a = node->parent; a && !ext; a = a->parent)
      if (a->parent && a->parent->kind == TSK_ConditionalType && a->parent->extendsType == a) ext = a->parent;
    TsNode *container2 = ext;   /* getInferTypeContainer: the conditional type whose extends type holds the infer */
    if (container2) {
      if (!container2->locals) container2->locals = bTableNew();
      bDeclareSymbol(container2->locals, NULL, node, SF_TypeParameter, EX_TypeParameterExcludes, false);
    } else {
      bBindAnonymousDeclaration(node, SF_TypeParameter, bGetDeclarationName(node));
    }
  } else {
    bDeclareSymbolAndAddToSymbolTable(node, SF_TypeParameter, EX_TypeParameterExcludes);
  }
}

static void bBindParameter(TsNode *node) {
  if (bIsBindingPattern(node->name)) {
    int idx = 0;
    for (int i = 0; node->parent->parameters && i < node->parent->parameters->n; i++)
      if (node->parent->parameters->v[i] == node) { idx = i; break; }
    char buf[24];
    int n = snprintf(buf, sizeof buf, "__%d", idx);
    bBindAnonymousDeclaration(node, SF_FunctionScopedVariable, bnConcat(buf, n, "", 0));
  } else {
    bDeclareSymbolAndAddToSymbolTable(node, SF_FunctionScopedVariable, EX_ParameterExcludes);
  }
  if (bIsParameterPropertyDeclaration(node, node->parent)) {
    TsNode *cls = node->parent->parent;
    bDeclareSymbol(cls->symbol->members, cls->symbol, node,
                   SF_Property | (node->questionToken ? SF_Optional : 0), EX_PropertyExcludes, false);
  }
}

static void bBindVariableDeclarationOrBindingElement(TsNode *node) {
  if (bIsBindingPattern(node->name)) return;
  if (bIsBlockOrCatchScoped(node)) bBindBlockScopedDeclaration(node, SF_BlockScopedVariable, EX_BlockScopedVariableExcludes);
  else if (bIsPartOfParameterDeclaration(node)) bDeclareSymbolAndAddToSymbolTable(node, SF_FunctionScopedVariable, EX_ParameterExcludes);
  else bDeclareSymbolAndAddToSymbolTable(node, SF_FunctionScopedVariable, EX_FunctionScopedVariableExcludes);
}

static void bBindClassLikeDeclaration(TsNode *node) {
  if (node->kind == TSK_ClassDeclaration) {
    bBindBlockScopedDeclaration(node, SF_Class, EX_ClassExcludes);
  } else {
    BName bindingName = node->name ? bnEscapeText(node->name->text, node->name->textLen) : bnC("__class");
    bBindAnonymousDeclaration(node, SF_Class, bindingName);
  }
  TsSymbol *symbol = node->symbol;
  TsSymbol *prototype = bCreateSymbol(SF_Property | SF_Prototype, bnC("prototype"));
  if (!symbol->exports) symbol->exports = bTableNew();
  bTableSet(symbol->exports, prototype->name, prototype);
  prototype->parent = symbol;
}

static void bBindFunctionOrConstructorType(TsNode *node) {
  TsSymbol *symbol = bCreateSymbol(SF_Signature, bGetDeclarationName(node));
  bAddDeclarationToSymbol(symbol, node, SF_Signature);
  TsSymbol *typeLiteral = bCreateSymbol(SF_TypeLiteral, bnC("__type"));
  bAddDeclarationToSymbol(typeLiteral, node, SF_TypeLiteral);
  typeLiteral->members = bTableNew();
  bTableSet(typeLiteral->members, symbol->name, symbol);
}

static void bBindExportAssignment(TsNode *node) {
  if (!bContainer->symbol || !bContainer->symbol->exports) {
    bBindAnonymousDeclaration(node, SF_Value, bGetDeclarationName(node));
  } else {
    uint32_t flags = bIsAliasableExpression(node->expression) ? SF_Alias : SF_Property;
    bDeclareSymbol(bContainer->symbol->exports, bContainer->symbol, node, flags, EX_All, false);
  }
}

static void bBindSourceFileAsExternalModule(void) {
  char *q = ts_arena_alloc(strlen(bFileName) + 3);
  size_t len = strlen(bFileName);
  /* removeFileExtension: drop the last extension */
  const char *dot = strrchr(bFileName, '.');
  if (dot && dot > bFileName) len = (size_t)(dot - bFileName);
  q[0] = '"';
  memcpy(q + 1, bFileName, len);
  q[len + 1] = '"';
  BName name = { q, (int)len + 2 };
  bBindAnonymousDeclaration(bFile, SF_ValueModule, name);
}

static void bBindSourceFileIfExternalModule(void) {
  bSetExportContextFlag(bFile);
  if (bFile->hasExternalModuleIndicator) bBindSourceFileAsExternalModule();
}

static bool bIsModuleAugmentationExternal(TsNode *node) {
  TsNode *p = node->parent;
  if (p->kind == TSK_SourceFile) return p->hasExternalModuleIndicator;
  if (p->kind == TSK_ModuleBlock) {
    TsNode *gp = p->parent, *ggp = gp ? gp->parent : NULL;
    return gp && bIsAmbientModule(gp) && ggp && ggp->kind == TSK_SourceFile && !ggp->hasExternalModuleIndicator;
  }
  return false;
}

static void bDeclareModuleSymbol(TsNode *node) {
  int state = bModuleInstanceStateCached(node);
  bool instantiated = state != MI_NonInstantiated;
  bDeclareSymbolAndAddToSymbolTable(node, instantiated ? SF_ValueModule : SF_NamespaceModule,
                                    instantiated ? EX_ValueModuleExcludes : EX_NamespaceModuleExcludes);
}

static void bBindModuleDeclaration(TsNode *node) {
  bSetExportContextFlag(node);
  if (bIsAmbientModule(node)) {
    if (bIsModuleAugmentationExternal(node)) bDeclareModuleSymbol(node);
    else bDeclareSymbolAndAddToSymbolTable(node, SF_ValueModule, EX_ValueModuleExcludes);
  } else {
    bDeclareModuleSymbol(node);
  }
}

static void bBindWorker(TsNode *node);

static void bBind(TsNode *node) {
  if (!node) return;
#ifdef TS_BINDER_TRACE_VISIT
  fprintf(stderr, "visit kind %d pos %d\n", node->kind, node->pos);
#endif
  node->parent = bParent;
  /* the flow node of a reference (bindWorker: identifiers in expressions, this and super; bindChildren: narrowable accesses) */
  if (bCurFlow && (node->kind == TSK_Identifier || node->kind == TSK_ThisKeyword || node->kind == TSK_SuperKeyword)) node->flowNode = bCurFlow;
  else if (bCurFlow && (node->kind == TSK_PropertyAccessExpression || node->kind == TSK_ElementAccessExpression) && bIsNarrowableReference(node))
    node->flowNode = bCurFlow;
  /* bindFunctionExpression and bindPropertyOrMethodOrAccessor: the flow at which a function value is created */
  if (bCurFlow && node->kind > TSK_LastToken &&
      (node->kind == TSK_FunctionExpression || node->kind == TSK_ArrowFunction ||
       ((node->kind == TSK_MethodDeclaration || node->kind == TSK_GetAccessor || node->kind == TSK_SetAccessor) &&
        (bContainerFlags(node) & CF_IsObjectLiteralOrClassExpressionMethodOrAccessor))))
    node->flowNode = bCurFlow;
  bool saveStrict = bInStrictMode;
  bBindWorker(node);
  TsNode *saveParent = bParent;
  if (node->kind > TSK_LastToken) {
    bParent = node;
    uint32_t cf = bContainerFlags(node);
    if (cf == 0) bBindChildren(node);
    else bBindContainer(node, cf);
  } else {
    if (node->kind == TSK_EndOfFileToken) bParent = node;
  }
  bParent = saveParent;
  bInStrictMode = saveStrict;
}

static void bBindWorker(TsNode *node) {
  switch (node->kind) {
    case TSK_ThisType: bSeenThis = true; return;
    case TSK_ClassExpression: case TSK_ClassDeclaration:
      bInStrictMode = true;
      bBindClassLikeDeclaration(node);
      return;
    case TSK_InterfaceDeclaration:
      bBindBlockScopedDeclaration(node, SF_Interface, EX_InterfaceExcludes);
      return;
    case TSK_TypeAliasDeclaration:
      bBindBlockScopedDeclaration(node, SF_TypeAlias, EX_TypeAliasExcludes);
      return;
    case TSK_EnumDeclaration:
      if (bIsEnumConst(node)) bBindBlockScopedDeclaration(node, SF_ConstEnum, EX_ConstEnumExcludes);
      else bBindBlockScopedDeclaration(node, SF_RegularEnum, EX_RegularEnumExcludes);
      return;
    case TSK_ModuleDeclaration:
      bBindModuleDeclaration(node);
      return;
    case TSK_TypeParameter:
      bBindTypeParameter(node);
      return;
    case TSK_Parameter:
      bBindParameter(node);
      return;
    case TSK_VariableDeclaration: case TSK_BindingElement:
      bBindVariableDeclarationOrBindingElement(node);
      return;
    case TSK_PropertyDeclaration: case TSK_PropertySignature: {
      bool isAutoAccessor = node->kind == TSK_PropertyDeclaration && bHasSyntacticModifier(node, TMF_Accessor);
      uint32_t includes = isAutoAccessor ? SF_Accessor : SF_Property;
      uint32_t excludes = isAutoAccessor ? EX_AccessorExcludes : EX_PropertyExcludes;
      bBindPropertyOrMethodOrAccessor(node, includes | (node->questionToken ? SF_Optional : 0), excludes);
      return;
    }
    case TSK_PropertyAssignment: case TSK_ShorthandPropertyAssignment:
      bBindPropertyOrMethodOrAccessor(node, SF_Property, EX_PropertyExcludes);
      return;
    case TSK_EnumMember:
      bBindPropertyOrMethodOrAccessor(node, SF_EnumMember, EX_EnumMemberExcludes);
      return;
    case TSK_CallSignature: case TSK_ConstructSignature: case TSK_IndexSignature:
      bDeclareSymbolAndAddToSymbolTable(node, SF_Signature, SF_None);
      return;
    case TSK_MethodDeclaration: case TSK_MethodSignature: {
      bool objLiteralMethod = node->kind == TSK_MethodDeclaration && node->parent && node->parent->kind == TSK_ObjectLiteralExpression;
      bBindPropertyOrMethodOrAccessor(node, SF_Method | (node->questionToken ? SF_Optional : 0),
                                      objLiteralMethod ? EX_PropertyExcludes : EX_MethodExcludes);
      return;
    }
    case TSK_FunctionDeclaration:
      if (bInStrictMode) bBindBlockScopedDeclaration(node, SF_Function, EX_FunctionExcludes);
      else bDeclareSymbolAndAddToSymbolTable(node, SF_Function, EX_FunctionExcludes);
      return;
    case TSK_Constructor:
      bDeclareSymbolAndAddToSymbolTable(node, SF_Constructor, SF_None);
      return;
    case TSK_GetAccessor:
      bBindPropertyOrMethodOrAccessor(node, SF_GetAccessor, EX_GetAccessorExcludes);
      return;
    case TSK_SetAccessor:
      bBindPropertyOrMethodOrAccessor(node, SF_SetAccessor, EX_SetAccessorExcludes);
      return;
    case TSK_FunctionType: case TSK_ConstructorType:
      bBindFunctionOrConstructorType(node);
      return;
    case TSK_TypeLiteral: case TSK_MappedType:
      bBindAnonymousDeclaration(node, SF_TypeLiteral, bnC("__type"));
      return;
    case TSK_ObjectLiteralExpression:
      bBindAnonymousDeclaration(node, SF_ObjectLiteral, bnC("__object"));
      return;
    case TSK_JsxAttributes:
      bBindAnonymousDeclaration(node, SF_ObjectLiteral, bnC("__jsxAttributes"));
      return;
    case TSK_JsxAttribute:
      bDeclareSymbolAndAddToSymbolTable(node, SF_Property, EX_PropertyExcludes);
      return;
    case TSK_FunctionExpression: case TSK_ArrowFunction: {
      BName name = node->name ? bnEscapeText(node->name->text, node->name->textLen) : bnC("__function");
      bBindAnonymousDeclaration(node, SF_Function, name);
      return;
    }
    case TSK_ImportEqualsDeclaration: case TSK_NamespaceImport: case TSK_ImportSpecifier: case TSK_ExportSpecifier:
      bDeclareSymbolAndAddToSymbolTable(node, SF_Alias, EX_AliasExcludes);
      return;
    case TSK_NamespaceExportDeclaration:
      /* the diagnostic cases are skipped; the declaration is kept only where TypeScript keeps it */
      if (bFile->hasExternalModuleIndicator && node->parent->kind == TSK_SourceFile && bFile->isDeclarationFile) {
        if (!bFile->symbol->globalExports) bFile->symbol->globalExports = bTableNew();
        bDeclareSymbol(bFile->symbol->globalExports, bFile->symbol, node, SF_Alias, EX_AliasExcludes, false);
      }
      return;
    case TSK_ImportClause:
      if (node->name) bDeclareSymbolAndAddToSymbolTable(node, SF_Alias, EX_AliasExcludes);
      return;
    case TSK_ExportDeclaration:
      if (!bContainer->symbol || !bContainer->symbol->exports) {
        bBindAnonymousDeclaration(node, SF_ExportStar, bGetDeclarationName(node));
      } else if (!node->exportClause) {
        bDeclareSymbol(bContainer->symbol->exports, bContainer->symbol, node, SF_ExportStar, SF_None, false);
      } else if (node->exportClause->kind == TSK_NamespaceExport) {
        bDeclareSymbol(bContainer->symbol->exports, bContainer->symbol, node->exportClause, SF_Alias, EX_AliasExcludes, false);
      }
      return;
    case TSK_ExportAssignment:
      bBindExportAssignment(node);
      return;
    case TSK_SourceFile:
      bUpdateStrictModeStatementList(node->statements);
      bBindSourceFileIfExternalModule();
      return;
    case TSK_Block:
      if (!(node->parent && (bIsFunctionLikeKind(node->parent->kind) || node->parent->kind == TSK_ClassStaticBlockDeclaration)))
        return;
      bUpdateStrictModeStatementList(node->statements);
      return;
    case TSK_ModuleBlock:
      bUpdateStrictModeStatementList(node->statements);
      return;
    default:
      return;
  }
}

/* binds a parsed source file and sets sf->binderSymbolCount (file.symbolCount in TypeScript) */
static void tsBindSourceFile(TsNode *file, const char *fileName) {
  bFile = file;
  bFileName = fileName;
  bParent = NULL;
  bContainer = NULL;
  bBlockScopeContainer = NULL;
  bSymbolCount = 0;
  bDepth = 0;
  /* setExternalModuleIndicator: any top-level import/export statement makes the file a module */
  bool ext = false;
  if (file->statements)
    for (int i = 0; i < file->statements->n && !ext; i++) {
      TsNode *s = file->statements->v[i];
      if (bHasSyntacticModifier(s, TMF_Export)) ext = true;
      else if (s->kind == TSK_ImportDeclaration || s->kind == TSK_ExportAssignment || s->kind == TSK_ExportDeclaration) ext = true;
      else if (s->kind == TSK_ImportEqualsDeclaration && s->moduleReference && s->moduleReference->kind == TSK_ExternalModuleReference) ext = true;
    }
  file->hasExternalModuleIndicator = ext;
  /* bindInStrictMode: alwaysStrict (strict: true) for non-declaration files, else the module indicator */
  bInStrictMode = file->isDeclarationFile ? ext : true;
  bFlowCount = 0;
  bCurFlow = NULL;
  bBreakTarget = bContinueTarget = bReturnTarget = bTrueTarget = bFalseTarget = bExceptionTarget = bPreSwitchCaseFlow = NULL;
  bActiveLabels = NULL;
  bHasFlowEffects = bInAssignmentPattern = bInReturnPosition = bHasExplicitReturn = false;
  bBind(file);
  file->binderSymbolCount = bSymbolCount;
  file->binderFlowCount = bFlowCount;
  bFile = NULL;
}

#endif
