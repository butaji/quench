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
 * ALSO PORTED: checkInheritedPropertiesAreIdentical (2320) and checkTypeParameter (default against constraint, 2344).
 * NOT PORTED: generic type alias references (their constraints),
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
  TsNode *d = prop->valueDeclaration ? prop->valueDeclaration : (prop->ndecls ? prop->decls[0] : NULL);
  if (prop->name.n >= 3 && memcmp(prop->name.s, "__@", 3) == 0) {
    /* a late-bound name: the type of its computed name (the nameType set by late binding); a type that is neither
     * a string or number literal nor a unique symbol gives never, as getLiteralTypeFromProperty does */
    TsNode *nm = d ? d->name : NULL;
    if (!nm || nm->kind != TSK_ComputedPropertyName) return NULL;
    TsType *t = cExprType(nm->expression);
    if (t == cErrorType) return NULL;
    return (t->flags & (TF_StringLiteral | TF_NumberLiteral | TF_UniqueESSymbol)) ? t : cNeverType;
  }
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

/* compareProperties (identity): the same accessibility, optionality and readonly flags, and identical types */
static bool cIsPropertyIdenticalTo(TsSymbol *s, TsSymbol *t) {
  if (s == t) return true;
  TsNode *sd = s->valueDeclaration, *td = t->valueDeclaration;
  bool sNonPublic = sd && (bModifierFlags(sd) & (TMF_Private | TMF_Protected));
  bool tNonPublic = td && (bModifierFlags(td) & (TMF_Private | TMF_Protected));
  if (sNonPublic != tNonPublic) return false;
  if (sNonPublic) {
    if (s->instTarget != t->instTarget) return false;
  } else if ((s->flags & SF_Optional) != (t->flags & SF_Optional)) return false;
  if (cIsReadonlyProp(s) != cIsReadonlyProp(t)) return false;
  TsType *a = cTypeOfSymbol(s), *b = cTypeOfSymbol(t);
  if (a == cErrorType || b == cErrorType) { cUnportedHit = true; return true; }
  return cIdentical(a, b);
}

/* checkInheritedPropertiesAreIdentical (89870s): an interface with several bases may not inherit a property of
 * the same name with different types (2320 at the name). Returns false when it reports. */
static bool cCheckInheritedPropertiesAreIdentical(TsType *type, TsNode *typeNode) {
  cResolveBaseTypes(type);
  if (type->nbases < 2) return true;
  /* seen: the declared properties first, then the first property of each name from the bases */
  int cap = 64, ns = 0;
  TsSymbol **seenSym = ts_arena_alloc(sizeof(TsSymbol *) * (size_t)cap);
  TsType **seenCont = ts_arena_alloc(sizeof(TsType *) * (size_t)cap);
  TsSymTable *own = cDeclaredMembers(type);
  for (int k = 0; own && k < own->n; k++) {
    TsSymbol *p = own->e[k].sym;
    if (cIsSignatureName(own->e[k].key) || !(p->flags & SF_Value)) continue;
    if (ns == cap) {
      TsSymbol **ns2 = ts_arena_alloc(sizeof(TsSymbol *) * (size_t)cap * 2);
      TsType **nc2 = ts_arena_alloc(sizeof(TsType *) * (size_t)cap * 2);
      memcpy(ns2, seenSym, sizeof(TsSymbol *) * (size_t)ns);
      memcpy(nc2, seenCont, sizeof(TsType *) * (size_t)ns);
      seenSym = ns2; seenCont = nc2; cap *= 2;
    }
    seenSym[ns] = p; seenCont[ns] = type; ns++;
  }
  bool ok = true;
  for (int b = 0; b < type->nbases; b++) {
    TsType *base = type->bases[b];
    if (base == cErrorType) continue;
    TsType *bwt = cGetTypeWithThisArgument(base, type->thisType);
    TsSymbol **props;
    int np = cPropertiesOf(bwt, &props);
    for (int i = 0; i < np; i++) {
      TsSymbol *p = props[i];
      int hit = -1;
      for (int q = 0; q < ns && hit < 0; q++)
        if (seenSym[q]->name.n == p->name.n && memcmp(seenSym[q]->name.s, p->name.s, (size_t)p->name.n) == 0) hit = q;
      if (hit < 0) {
        if (ns == cap) {
          TsSymbol **ns2 = ts_arena_alloc(sizeof(TsSymbol *) * (size_t)cap * 2);
          TsType **nc2 = ts_arena_alloc(sizeof(TsType *) * (size_t)cap * 2);
          memcpy(ns2, seenSym, sizeof(TsSymbol *) * (size_t)ns);
          memcpy(nc2, seenCont, sizeof(TsType *) * (size_t)ns);
          seenSym = ns2; seenCont = nc2; cap *= 2;
        }
        seenSym[ns] = p; seenCont[ns] = base; ns++;
      } else if (seenCont[hit] != type && !cIsPropertyIdenticalTo(seenSym[hit], p)) {
        ok = false;
        cDiagAdd(cSourceFileOf(typeNode), cNodeStart(typeNode), 2320, NULL);
      }
    }
  }
  return ok;
}

/* checkTypeParameter (85592): the default must satisfy the constraint, instantiated with the default for the
 * type parameter (2344 at the default) */
static void cCheckTypeParameterNode(TsNode *n) {
  if (n->kind != TSK_TypeParameter || !n->symbol) return;
  TsType *tp = cTypeOfTypeParameter(cGetMergedSymbol(n->symbol));
  TsNode *dn = cTypeParamDefaultNode(tp);
  if (!dn) return;
  TsType *def = cTypeFromNode(dn);
  if (def == cErrorType) return;
  TsType *c = cConstraintOf(tp);
  if (!c) return;
  TsMapper *m = cCreateTypeMapper(&tp, &def, 1);
  TsType *inst = cInstantiate(c, m);
  if (inst == cErrorType) return;
  inst = cGetTypeWithThisArgument(inst, def);
  cCheckTypeRelatedTo(def, inst, REL_Assignable, dn, 2344);
}

/* checkSourceElement over the type nodes (checkUnionOrIntersectionType, checkTypeLiteral, the other type node
 * checks): each type node is resolved with getTypeFromTypeNode, and a type literal checks its index constraints
 * (checkIndexConstraints of the literal's anonymous type, 89089, lazily). */
static void cCheckTypeNode(TsNode *n) {
  switch (n->kind) {
    case TSK_UnionType: case TSK_IntersectionType: case TSK_ArrayType: case TSK_TupleType: case TSK_TypeOperator:
    case TSK_IndexedAccessType: case TSK_ConditionalType: case TSK_MappedType: case TSK_LiteralType: case TSK_FunctionType:
    case TSK_ConstructorType: case TSK_TypeQuery: case TSK_TemplateLiteralType: case TSK_TypeReference: case TSK_ParenthesizedType:
      cTypeFromNode(n);
      break;
    case TSK_TypeLiteral: {
      TsType *t = cTypeFromNode(n);
      if (t != cErrorType && (t->flags & TF_Object)) cCheckIndexConstraints(t);
      break;
    }
    default: break;
  }
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
  if (!cCheckInheritedPropertiesAreIdentical(type, n->name ? n->name : n)) return;
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

/* checkTypePredicate (88840s): the type of a predicate `x is T` must be assignable to the type of its parameter
 * (2677 at the type node). A `this` predicate and an asserts predicate without a type are not checked here. */
static void cCheckTypePredicateNode(TsNode *n) {
  if (n->kind != TSK_TypePredicate || !n->type || !n->parent || n->parent->type != n || !n->parameterName) return;
  TsNode *fn = n->parent;
  switch (fn->kind) {
    case TSK_FunctionDeclaration: case TSK_FunctionExpression: case TSK_ArrowFunction: case TSK_MethodDeclaration:
    case TSK_Constructor: case TSK_GetAccessor: case TSK_SetAccessor: case TSK_FunctionType: case TSK_ConstructorType:
    case TSK_MethodSignature: case TSK_CallSignature: case TSK_ConstructSignature:
      break;
    default:
      return;
  }
  if (n->parameterName->kind != TSK_Identifier || !fn->parameters) return;
  TsNode *param = NULL;
  for (int i = 0; i < fn->parameters->n && !param; i++) {
    TsNode *p = fn->parameters->v[i];
    if (p->name && p->name->kind == TSK_Identifier && p->name->textLen == n->parameterName->textLen &&
        memcmp(p->name->text, n->parameterName->text, (size_t)p->name->textLen) == 0)
      param = p;
  }
  if (!param || !param->symbol) return;
  TsType *predType = cTypeFromNode(n->type);
  if (predType == cErrorType) return;
  TsType *paramType = cTypeOfSymbol(cGetMergedSymbol(param->symbol));
  if (paramType == cErrorType) return;
  cCheckTypeRelatedTo(predType, paramType, REL_Assignable, n->type, 2677);
}

/* checkTemplateLiteralType (86000s): each span type is assignable to the template constraint (string, number,
 * bigint, boolean, null or undefined) */
static void cCheckTemplateLiteralTypeNode(TsNode *n) {
  if (n->kind != TSK_TemplateLiteralType) return;
  TsType *templateConstraint = cGetUnionType((TsType *[]){cStringType, cNumberType, cBigIntType, cBooleanType, cNullType, cUndefinedType}, 6);
  for (int i = 0; n->spans && i < n->spans->n; i++) {
    TsNode *sp = n->spans->v[i];
    TsType *t = cTypeFromNode(sp->type);
    if (t != cErrorType) cCheckTypeRelatedTo(t, templateConstraint, REL_Assignable, sp->type, 2322);
  }
}

/* checkMappedType (88420s): the constraint of a mapped type is assignable to keyof any (string | number | symbol) */
static void cCheckMappedTypeNode(TsNode *n) {
  if (n->kind != TSK_MappedType) return;
  TsType *keyofConstraint = cGetUnionType((TsType *[]){cStringType, cNumberType, cESSymbolType}, 3);
  if (n->nameType) { (void)cUnported("mapped type with an as clause"); return; }
  TsType *t = cTypeFromNode(n);
  if (t == cErrorType || !(t->flags & TF_Object) || !(t->objectFlags & OF_Mapped) || t->mapConstraint == cErrorType) return;
  TsNode *tpc = n->typeParameter ? n->typeParameter->constraint : NULL;
  cCheckTypeRelatedTo(t->mapConstraint, keyofConstraint, REL_Assignable, tpc ? tpc : n, 2322);
}

/* checkIndexedAccessType (88390s): the index type is assignable to keyof of the object type (2536 otherwise) */
static void cCheckIndexedAccessNode(TsNode *n) {
  if (n->kind != TSK_IndexedAccessType) return;
  TsType *t = cTypeFromNode(n);
  if (t == cErrorType || !(t->flags & TF_IndexedAccess)) return;
  TsType *keys = cGetIndexType(t->iaObj);
  if (keys == cErrorType) return;
  cCheckTypeRelatedTo(t->iaIdx, keys, REL_Assignable, n->indexType, 2536);
}

/* the declaration checks of type nodes, one walk: predicates, template literal types, mapped types and indexed
 * access types (each returns at once for other kinds) */
static void cCheckDeclTypeNodes(TsNode *n) {
  cCheckTypePredicateNode(n);
  cCheckTemplateLiteralTypeNode(n);
  cCheckMappedTypeNode(n);
  cCheckIndexedAccessNode(n);
}

#endif
