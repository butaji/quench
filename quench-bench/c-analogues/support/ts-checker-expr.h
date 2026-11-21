/*
 * Checker port, expression checks of assignments (TypeScript 5.9.2 checker.ts, checkBinaryLikeExpression for
 * `=` (lib/typescript.js line numbers in comments)).
 *
 * PORTED:
 *   - checkAssignmentOperator for an element access target `this[K]` with K a unique symbol: the target type is
 *     the declared type of the class member (checkElementAccessExpression with a late-bound name);
 *   - checkTypeAssignableToAndOptionallyElaborate for an object literal source (elaborateObjectLiteral,
 *     elaborateElementwise): each property assignment whose name is a property of the target is checked
 *     against that property's type, and a failing one is reported with 2322 at the property name. The
 *     property types come from cExprType, the ported expression forms; a property whose type is not ported
 *     is skipped (no diagnostic is reported for it).
 * NOT PORTED: the whole-literal 2322 (reported at the target when no property is elaborated), excess property
 * checks, contextual typing of literals, and every other assignment target.
 */
#ifndef TS_CHECKER_EXPR_H
#define TS_CHECKER_EXPR_H

#include "ts-checker-class.h"

/* the property that `this[K]` names, K a unique symbol (NULL when the target is another form) */
static TsSymbol *cThisElementProperty(TsNode *left) {
  if (!left->expression || left->expression->kind != TSK_ThisKeyword || !left->argumentExpression) return NULL;
  TsNode *k = left->argumentExpression;
  if (!bIsEntityNameExpression(k)) return NULL;
  TsType *kt = cEntityNameType(left, k);
  if (kt == cErrorType || !(kt->flags & TF_UniqueESSymbol)) return NULL;
  TsType *cls = cEnclosingClassType(left);
  if (!cls) return NULL;
  return cPropertyOf(cls, (BName){ kt->text, kt->textLen });
}

/* elaborateObjectLiteral: a failing property of the literal is reported at its name (2322) */
static void cElaborateObjectLiteral(TsNode *lit, TsType *target) {
  if (!lit->properties) return;
  for (int i = 0; i < lit->properties->n; i++) {
    TsNode *p = lit->properties->v[i];
    if (p->kind != TSK_PropertyAssignment || !p->name || p->name->kind != TSK_Identifier || !p->initializer) continue;
    TsSymbol *tp = cPropertyOf(target, bnEscapeText(p->name->text, p->name->textLen));
    if (!tp) continue;
    TsType *src = cExprType(p->initializer);
    if (src == cErrorType) continue; /* not ported: not reported */
    if (!cIsRelatedWith(src, cTypeOfSymbol(tp), REL_Assignable))
      cDiagAdd(cSourceFileOf(p->name), cNodeStart(p->name), 2322, NULL);
  }
}

/* checkAssignmentOperator: the target `this[K]` and an object literal source */
static void cCheckAssignmentOperator(TsNode *bin) {
  TsNode *left = bin->left, *right = bin->right;
  if (!left || !right || left->kind != TSK_ElementAccessExpression || right->kind != TSK_ObjectLiteralExpression) return;
  TsSymbol *prop = cThisElementProperty(left);
  if (!prop) return;
  TsType *target = cTypeOfSymbol(prop);
  if (target == cErrorType) return;
  cElaborateObjectLiteral(right, target);
}

/* the expression checks run on every node of a source file */
static void cCheckExpressionNode(TsNode *n) {
  if (n->kind == TSK_BinaryExpression && n->operatorToken && n->operatorToken->kind == TSK_EqualsToken)
    cCheckAssignmentOperator(n);
}

/* checkSourceElement over the declarations and type nodes of a file: the type annotation of every declaration
 * is resolved with getTypeFromTypeNode (checkVariableLikeDeclaration, checkParameter, checkPropertyDeclaration,
 * checkSignatureDeclaration, checkTypeReferenceNode), and every type alias declares its type. */
static void cCheckDeclarationTypes(TsNode *n) {
  switch (n->kind) {
    case TSK_Parameter: case TSK_VariableDeclaration: case TSK_PropertyDeclaration: case TSK_PropertySignature:
    case TSK_MethodDeclaration: case TSK_MethodSignature: case TSK_FunctionDeclaration: case TSK_ArrowFunction:
    case TSK_FunctionExpression: case TSK_GetAccessor: case TSK_SetAccessor: case TSK_CallSignature:
    case TSK_ConstructSignature: case TSK_IndexSignature: case TSK_FunctionType: case TSK_ConstructorType:
      if (n->type) cTypeFromNode(n->type);
      break;
    case TSK_TypeReference:
      cTypeFromNode(n);
      break;
    case TSK_TypeAliasDeclaration:
      if (n->symbol) cDeclaredTypeOfTypeAlias(cGetMergedSymbol(n->symbol));
      break;
    default: break;
  }
}

#endif
