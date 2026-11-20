/*
 * Checker port, part 5: the subsequent-declaration checks of checkVariableLikeDeclaration (87788).
 *
 * PORTED: for a variable, property signature or property declaration that is not the first
 * declaration of its merged symbol, the declared type must be identical to the type of the symbol
 * (2403 for variables, 2717 for properties, at the name); and the modifiers of all declarations
 * must be identical (2687, areDeclarationFlagsIdentical 87985-88000).
 * NOT PORTED: the first declaration's initializer check (checkExpressionCached and assignability of
 * the initializer), binding-element and binding-pattern checks, and checkExportsOnMergedDeclarations.
 * Declarations whose types are not in the ported subset are skipped and counted in cUnportedCount.
 */
#ifndef TS_CHECKER_VARS_H
#define TS_CHECKER_VARS_H

#include "ts-types.h"

static bool cIsVariableLike(TsNode *d) {
  return d->kind == TSK_VariableDeclaration || d->kind == TSK_Parameter || d->kind == TSK_PropertyDeclaration ||
         d->kind == TSK_PropertySignature || d->kind == TSK_BindingElement;
}

/* areDeclarationFlagsIdentical (87985) */
static bool cAreDeclarationFlagsIdentical(TsNode *l, TsNode *r) {
  if ((l->kind == TSK_Parameter && r->kind == TSK_VariableDeclaration) || (l->kind == TSK_VariableDeclaration && r->kind == TSK_Parameter))
    return true;
  if ((l->questionToken != NULL) != (r->questionToken != NULL)) return false;
  const uint32_t interesting = TMF_Private | TMF_Protected | TMF_Async | TMF_Abstract | TMF_Readonly | TMF_Static;
  return (bModifierFlags(l) & interesting) == (bModifierFlags(r) & interesting);
}

static void cCheckVariableLike(TsNode *node) {
  if (node->kind != TSK_VariableDeclaration && node->kind != TSK_PropertySignature && node->kind != TSK_PropertyDeclaration) return;
  if (!node->name) return;
  TsSymbol *sym = cGetMergedSymbol(node->symbol);
  if (!sym || (sym->flags & SF_Alias)) return;
  if (sym->valueDeclaration == node) {
    if (sym->ndecls > 1) {
      for (int d = 0; d < sym->ndecls; d++) {
        TsNode *o = sym->decls[d];
        if (o != node && cIsVariableLike(o) && !cAreDeclarationFlagsIdentical(o, node)) {
          cDiagAdd(cSourceFileOf(node), cNodeStart(node->name), 2687, NULL);
          break;
        }
      }
    }
    return;
  }
  TsType *type = cTypeOfSymbol(sym);
  TsType *declType;
  if (!node->type) declType = cUnported("declaration without a type annotation");
  else declType = cTypeFromNode(node->type);
  if (node->questionToken) declType = cAddOptionality(declType);
  if (type != cErrorType && declType != cErrorType && !cIsTypeIdenticalTo(type, declType) && !(sym->flags & SF_Assignment_)) {
    int code = node->kind == TSK_VariableDeclaration ? 2403 : 2717;
    cDiagAdd(cSourceFileOf(node), cNodeStart(node->name), code, NULL);
  }
  if (sym->valueDeclaration && !cAreDeclarationFlagsIdentical(node, sym->valueDeclaration))
    cDiagAdd(cSourceFileOf(node), cNodeStart(node->name), 2687, NULL);
}

#endif
