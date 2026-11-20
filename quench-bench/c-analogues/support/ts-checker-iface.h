/*
 * Checker port, interface and type reference checks of declaration files (TypeScript 5.9.2 checker.ts,
 * lib/typescript.js line numbers in comments). These run on every interface, the declaration files included.
 *
 * PORTED:
 *   - checkInterfaceDeclaration (89909) for the first declaration of a merged interface: each base type is
 *     checked against the interface with the this argument (checkTypeAssignableTo, 2430 at the name), and
 *     checkIndexConstraints (89089): every property against the applicable index signatures (2411) and every
 *     index signature against the other applicable ones (2413). The property name types are the string literal
 *     types of the names (getLiteralTypeFromProperty), which the checker creates for every property.
 *   - checkTypeReferenceNode (86183) with checkTypeArgumentConstraints (86146): a reference to a generic class or
 *     interface checks each type argument against its instantiated constraint (2344). The heritage clauses of
 *     classes and interfaces are checked the same way (the type arguments of the base expression).
 * NOT PORTED: checkInheritedPropertiesAreIdentical (2320), generic type alias references (their constraints),
 *   the late-bound names of index constraints (a property with a computed name is skipped and reported as
 *   unported), checkIndexConstraints of classes (static index and class member names).
 */
#ifndef TS_CHECKER_IFACE_H
#define TS_CHECKER_IFACE_H

#include "ts-checker-class.h"

/* the interface declaration of a symbol that checkInterfaceDeclaration checks: the first one (getDeclarationOfKind) */
static TsNode *cFirstInterfaceDecl(TsSymbol *m) {
  for (int d = 0; d < m->ndecls; d++)
    if (m->decls[d]->kind == TSK_InterfaceDeclaration) return m->decls[d];
  return NULL;
}

/* the symbol that declares a node (getSymbolOfDeclaration), merged */
static TsSymbol *cDeclaringSymbolOf(TsNode *decl) {
  if (!decl || !decl->parent || !decl->parent->symbol) return NULL;
  return cGetMergedSymbol(decl->parent->symbol);
}

/* getIndexTypeOfType (exact key): the index info of an object type whose key type is keyType */
static CIndexInfo *cIndexInfoWithKey(TsType *o, TsType *keyType) {
  cResolveIndexInfos(o);
  for (int i = 0; i < o->nindex; i++)
    if (o->index[i].keyType == keyType) return &o->index[i];
  return NULL;
}

/* getApplicableIndexInfos: every index info whose key type takes the key type (isApplicableIndexType) */
#define CI_MaxApplicable 8
static int cApplicableIndexInfos(TsType *o, TsType *key, CIndexInfo **out) {
  cResolveIndexInfos(o);
  int n = 0;
  for (int i = 0; i < o->nindex && n < CI_MaxApplicable; i++) {
    CIndexInfo *info = &o->index[i];
    bool applies = info->keyType == cStringType
                       ? (key->flags & (TF_StringLiteral | TF_String | TF_NumberLiteral | TF_Number)) != 0
                       : (info->keyType == cNumberType && (key->flags & (TF_NumberLiteral | TF_Number)) != 0);
    if (applies) out[n++] = info;
  }
  return n;
}

/* the base types of an interface whose property or index signature the check looks up (some() of getBaseTypes) */
static bool cBaseHasPropAndIndex(TsType *d, BName prop, TsType *keyType) {
  cResolveBaseTypes(d);
  for (int b = 0; b < d->nbases; b++) {
    TsType *base = d->bases[b];
    if (base == cErrorType) continue;
    if (cPropertyOf(base, prop) && cIndexInfoWithKey(base, keyType)) return true;
  }
  return false;
}

static bool cBaseHasIndex(TsType *d, TsType *keyType, TsType *otherKey) {
  cResolveBaseTypes(d);
  for (int b = 0; b < d->nbases; b++) {
    TsType *base = d->bases[b];
    if (base == cErrorType) continue;
    if (cIndexInfoWithKey(base, keyType) && cIndexInfoWithKey(base, otherKey)) return true;
  }
  return false;
}

/* the declaration of the interface symbol (interfaceDeclaration in checkIndexConstraintFor*) when the type is an interface */
static TsNode *cInterfaceDeclOfType(TsType *type) {
  if (!type->symbol || !(type->symbol->flags & SF_Interface)) return NULL;
  return cFirstInterfaceDecl(cGetMergedSymbol(type->symbol));
}

/* the literal type of a property name (getLiteralTypeFromProperty); NULL for a late-bound name */
static TsType *cPropertyNameLiteral(TsSymbol *prop) {
  if (prop->name.n >= 3 && memcmp(prop->name.s, "__@", 3) == 0) return NULL;
  TsNode *d = prop->valueDeclaration ? prop->valueDeclaration : (prop->ndecls ? prop->decls[0] : NULL);
  if (d && d->name && d->name->kind == TSK_NumericLiteral) return cLiteralType(TF_NumberLiteral, d->name->text, d->name->textLen);
  return cStringLiteralType(prop->name.s, prop->name.n);
}

/* checkIndexConstraintForProperty (89119) */
static void cCheckIndexConstraintForProperty(TsType *type, TsSymbol *prop, TsType *propNameType, TsType *propType) {
  TsNode *declaration = prop->valueDeclaration;
  TsNode *name = declaration ? declaration->name : NULL;
  if (name && name->kind == TSK_PrivateIdentifier) return;
  CIndexInfo *infos[CI_MaxApplicable];
  int ni = cApplicableIndexInfos(type, propNameType, infos);
  if (ni == 0) return;
  TsNode *interfaceDeclaration = cInterfaceDeclOfType(type);
  TsSymbol *ownerSym = cGetMergedSymbol(type->symbol);
  TsNode *localPropDeclaration = (declaration && cDeclaringSymbolOf(declaration) == ownerSym) ? declaration : NULL;
  for (int k = 0; k < ni; k++) {
    CIndexInfo *info = infos[k];
    TsNode *localIndexDeclaration = (info->decl && cDeclaringSymbolOf(info->decl) == ownerSym) ? info->decl : NULL;
    TsNode *errorNode = localPropDeclaration ? localPropDeclaration : localIndexDeclaration;
    if (!errorNode && interfaceDeclaration &&
        !cBaseHasPropAndIndex(type, prop->name, info->keyType))
      errorNode = interfaceDeclaration;
    if (errorNode && !cIsRelatedWith(propType, info->type, REL_Assignable)) cDiagAdd(cSourceFileOf(errorNode), cNodeStart(errorNode), 2411, NULL);
  }
}

/* checkIndexConstraintForIndexSignature (89141) */
static void cCheckIndexConstraintForIndexSignature(TsType *type, CIndexInfo *checkInfo) {
  CIndexInfo *infos[CI_MaxApplicable];
  int ni = cApplicableIndexInfos(type, checkInfo->keyType, infos);
  TsNode *interfaceDeclaration = cInterfaceDeclOfType(type);
  TsSymbol *ownerSym = cGetMergedSymbol(type->symbol);
  TsNode *localCheckDeclaration = (checkInfo->decl && cDeclaringSymbolOf(checkInfo->decl) == ownerSym) ? checkInfo->decl : NULL;
  for (int k = 0; k < ni; k++) {
    CIndexInfo *info = infos[k];
    if (info == checkInfo) continue;
    TsNode *localIndexDeclaration = (info->decl && cDeclaringSymbolOf(info->decl) == ownerSym) ? info->decl : NULL;
    TsNode *errorNode = localCheckDeclaration ? localCheckDeclaration : localIndexDeclaration;
    if (!errorNode && interfaceDeclaration && !cBaseHasIndex(type, checkInfo->keyType, info->keyType)) errorNode = interfaceDeclaration;
    if (errorNode && !cIsRelatedWith(checkInfo->type, info->type, REL_Assignable)) cDiagAdd(cSourceFileOf(errorNode), cNodeStart(errorNode), 2413, NULL);
  }
}

/* checkIndexConstraints (89089) for an interface (the static and class member parts are not ported) */
static void cCheckIndexConstraints(TsType *type) {
  cResolveIndexInfos(type);
  if (type->nindex == 0) return;
  TsSymbol **props;
  int np = cPropertiesOf(type, &props);
  for (int i = 0; i < np; i++) {
    TsSymbol *p = props[i];
    TsType *nameType = cPropertyNameLiteral(p);
    if (!nameType) { cUnported("index constraint of a late-bound property"); continue; }
    cCheckIndexConstraintForProperty(type, p, nameType, cTypeOfSymbol(p));
  }
  if (type->nindex > 1)
    for (int i = 0; i < type->nindex; i++) cCheckIndexConstraintForIndexSignature(type, &type->index[i]);
}

/* checkInterfaceDeclaration (89909) for the first declaration of an interface: the base types and the index
 * constraints (the lazy diagnostics of the declaration). */
static void cCheckInterfaceDecl(TsNode *n) {
  if (n->kind != TSK_InterfaceDeclaration || !n->symbol) return;
  TsSymbol *sym = cGetMergedSymbol(n->symbol);
  if (cFirstInterfaceDecl(sym) != n) return;
  TsType *type = cDeclaredTypeOfClassOrInterface(sym);
  cResolveMembers(type);
  TsType *typeWithThis = cGetTypeWithThisArgument(type, NULL);
  cResolveBaseTypes(type);
  for (int b = 0; b < type->nbases; b++) {
    TsType *base = type->bases[b];
    if (base == cErrorType) continue;
    TsType *baseWithThis = cGetTypeWithThisArgument(base, type->thisType);
    if (baseWithThis == cErrorType) continue;
    cCheckTypeRelatedTo(typeWithThis, baseWithThis, REL_Assignable, n->name ? n->name : n, 2430);
  }
  cCheckIndexConstraints(type);
}

/* checkTypeArgumentConstraints (86146) for a reference to a generic class or interface: the type arguments of
 * the reference against the constraints of the type parameters, instantiated with the arguments. argNodes are
 * the written type arguments (error nodes), NULL when the arguments are defaults. */
static void cCheckRefTypeArgumentConstraints(TsType *ref, TsNode *argOwner) {
  if (!(ref->flags & TF_Object) || !(ref->objectFlags & OF_Reference)) return;
  TsType *tg = ref->target;
  if (!tg || tg->ntypeParams == 0 || tg->ntypeParams > ref->nargs) return;
  TsType **tps = tg->typeParams;
  int n = tg->ntypeParams;
  TsMapper *m = NULL;
  for (int i = 0; i < n; i++) {
    TsType *c = cConstraintOf(tps[i]);
    if (!c) continue;
    if (!m) m = cCreateTypeMapper(tps, ref->args, n);
    TsType *arg = ref->args[i];
    if (arg == cErrorType) continue;
    TsType *inst = cInstantiate(c, m);
    if (inst == cErrorType) continue;
    TsNode *errNode = argOwner && argOwner->typeArguments && i < argOwner->typeArguments->n ? argOwner->typeArguments->v[i] : argOwner;
    cCheckTypeRelatedTo(arg, inst, REL_Assignable, errNode, 2344);
  }
}

/* checkTypeReferenceNode (86183): the type arguments of a type reference node */
static void cCheckTypeReferenceArgs(TsNode *n) {
  if (n->kind != TSK_TypeReference || !n->typeArguments || n->typeArguments->n == 0) return;
  TsType *t = cTypeFromNode(n);
  if (t == cErrorType) return;
  cCheckRefTypeArgumentConstraints(t, n);
}

/* the heritage clauses of a class or interface: checkTypeReferenceNode of each base expression with type arguments */
static void cCheckHeritageArgs(TsNode *n) {
  if (!(n->kind == TSK_ClassDeclaration || n->kind == TSK_InterfaceDeclaration || n->kind == TSK_ClassExpression) || !n->symbol) return;
  for (int h = 0; n->heritageClauses && h < n->heritageClauses->n; h++) {
    TsNode *hc = n->heritageClauses->v[h];
    for (int e = 0; hc->types && e < hc->types->n; e++) {
      TsNode *ewta = hc->types->v[e];
      if (!ewta->typeArguments || ewta->typeArguments->n == 0 || !ewta->expression || ewta->expression->kind != TSK_Identifier) continue;
      TsSymbol *bs = cBaseSymbol(n, ewta->expression);
      if (!bs || !(bs->flags & (SF_Class | SF_Interface))) continue;
      TsType *ref = cTypeFromSymbolReference(bs, ewta);
      if (ref == cErrorType) continue;
      cCheckRefTypeArgumentConstraints(ref, ewta);
    }
  }
}

#endif
