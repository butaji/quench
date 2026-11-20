/*
 * Checker port, class base types (TypeScript 5.9.2 checker.ts, checkClassLikeDeclaration, 84.. and
 * issueMemberSpecificError, 84.., lib/typescript.js line numbers in comments).
 *
 * PORTED: for a class with an extends clause, the instance type of the class must be assignable to its base
 * type (checkTypeAssignableTo with no error node). When it is not, issueMemberSpecificError reports 2416 at the
 * name of each member whose type is not assignable to the same member of the base type (static members are
 * skipped, and members with a name the checker does not resolve are skipped); only when no member is reported
 * is the class reported as incorrectly extending its base (2415, at the class name).
 * NOT PORTED: checkKindsOfPropertyMemberOverrides (2424, 2425, 2426, 2610, 2611: no such diagnostic occurs in
 * the immer-tiny program), the base type accessibility and constructor-type checks, and the type arguments
 * constraints of the base expression.
 */
#ifndef TS_CHECKER_CLASS_H
#define TS_CHECKER_CLASS_H

#include "ts-relate.h"

/* the property name of a class member as getPropertyOfType sees it: identifiers, string and numeric names,
 * and late-bound names (their unique symbol names). Returns false for names that have no property key. */
static bool cMemberKey(TsNode *m, BName *key) {
  TsNode *nm = m->name;
  if (!nm) return false;
  if (nm->kind == TSK_ComputedPropertyName) {
    TsType *nt = cLateNameType(m);
    if (!nt || nt == cErrorType) return false;
    *key = (BName){ nt->text, nt->textLen };
    return true;
  }
  if (nm->kind == TSK_Identifier || nm->kind == TSK_PrivateIdentifier || nm->kind == TSK_NumericLiteral) {
    *key = bnEscapeText(nm->text, nm->textLen);
    return true;
  }
  if (nm->kind == TSK_StringLiteral && nm->textLen >= 2) {
    *key = bnEscapeText(nm->text + 1, nm->textLen - 2);
    return true;
  }
  return false;
}

/* issueMemberSpecificError (84..): 2416 for each member whose type is not assignable to the base member's type.
 * Returns true when at least one member was reported. */
static bool cIssueMemberSpecificError(TsNode *n, TsType *typeWithThis, TsType *baseWithThis) {
  bool issued = false;
  for (int m = 0; n->members && m < n->members->n; m++) {
    TsNode *mem = n->members->v[m];
    if (bHasSyntacticModifier(mem, TMF_Static)) continue;
    BName key;
    if (!cMemberKey(mem, &key)) continue;
    TsSymbol *prop = cPropertyOf(typeWithThis, key);
    TsSymbol *baseProp = cPropertyOf(baseWithThis, key);
    if (!prop || !baseProp) continue;
    if (!cCheckTypeRelatedTo(cTypeOfSymbol(prop), cTypeOfSymbol(baseProp), REL_Assignable, mem->name, 2416)) issued = true;
  }
  return issued;
}

/* checkClassLikeDeclaration: the base type check of a class (the first base type of the extends clause) */
static void cCheckClassBase(TsNode *n) {
  if (!(n->kind == TSK_ClassDeclaration || n->kind == TSK_ClassExpression) || !n->symbol) return;
  TsType *cls = cDeclaredTypeOfClassOrInterface(cGetMergedSymbol(n->symbol));
  cResolveMembers(cls);
  if (cls->nbases == 0) return;
  TsType *base = cls->bases[0];
  if (base == cErrorType) return;
  if (cIsRelatedWith(cls, base, REL_Assignable)) return;
  if (!cIssueMemberSpecificError(n, cls, base))
    cCheckTypeRelatedTo(cls, base, REL_Assignable, n->name ? n->name : n, 2415);
}

#endif
