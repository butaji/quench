/* WSL cloneProgram: a C form of WSL/CloneProgram.js and StatementCloner.js (via Rewriter.js).
 * The clone is a structural copy of the parse tree; the row's clone also drops nothing that the
 * dump can see, so the clone of a parsed program dumps identically (checked by --clone in
 * wsl-ast-dump.c against tools/wsl-ast-dump.cjs). Strings are immutable and shared. */
#ifndef WSL_CLONE_H
#define WSL_CLONE_H

#include "wsl-ast.h"

static Ast *ast_clone(const Ast *n);

static void list_clone(AstList *dst, const AstList *src) {
  dst->v = NULL;
  dst->n = 0;
  dst->cap = 0;
  for (int i = 0; i < src->n; i++) list_push(dst, ast_clone(src->v[i]));
}

static Ast *ast_clone(const Ast *n) {
  if (!n) return NULL;
  Ast *c = arena_alloc(sizeof(Ast));
  memcpy(c, n, sizeof(Ast));
  if (n->type) {
    AstTag tt = n->type->tag;
    if (tt == A_INTLITERALTYPE || tt == A_UINTLITERALTYPE || tt == A_FLOATLITERALTYPE || tt == A_DOUBLELITERALTYPE || tt == A_NULLTYPE) {
      /* a literal's type is its own node; its committed type (commit_ctx_nodes) is visited too */
      c->type = lit_type_new(tt, c);
      if (n->type->type) c->type->type = ast_clone(n->type->type);
    } else {
      c->type = ast_clone(n->type);   /* a typeref wrap (withType literals): an ordinary type node */
    }
  }
  switch (n->tag) {
  case A_FUNCDEF:
  case A_NATIVEFUNC:
  case A_PROTOCOLFUNCDECL:
    c->u.func.returnType = ast_clone(n->u.func.returnType);
    list_clone(&c->u.func.typeParameters, &n->u.func.typeParameters);
    list_clone(&c->u.func.parameters, &n->u.func.parameters);
    c->u.func.body = ast_clone(n->u.func.body);
    break;
  case A_TYPEREF:
    list_clone(&c->u.typeRef.typeArguments, &n->u.typeRef.typeArguments);
    break;
  case A_PTRTYPE:
  case A_ARRAYREFTYPE:
    c->u.ref.elementType = ast_clone(n->u.ref.elementType);
    break;
  case A_ARRAYTYPE:
    c->u.arrayType.elementType = ast_clone(n->u.arrayType.elementType);
    c->u.arrayType.numElements = ast_clone(n->u.arrayType.numElements);
    break;
  case A_TYPEVARIABLE:
    c->u.typeVariable.protocol = ast_clone(n->u.typeVariable.protocol);
    break;
  case A_CONSTEXPRTYPEPARAM:
  case A_FIELD:
  case A_FUNCPARAMETER:
    c->u.named.type = ast_clone(n->u.named.type);
    break;
  case A_TYPEDEF:
    list_clone(&c->u.typeDef.typeParameters, &n->u.typeDef.typeParameters);
    c->u.typeDef.type = ast_clone(n->u.typeDef.type);
    break;
  case A_STRUCTTYPE:
    list_clone(&c->u.structType.typeParameters, &n->u.structType.typeParameters);
    list_clone(&c->u.structType.fields, &n->u.structType.fields);
    break;
  case A_ENUMTYPE:
    c->u.enumType.baseType = ast_clone(n->u.enumType.baseType);
    list_clone(&c->u.enumType.members, &n->u.enumType.members);
    break;
  case A_ENUMMEMBER:
    c->u.nameOnly.value = ast_clone(n->u.nameOnly.value);
    break;
  case A_PROTOCOLDECL:
    list_clone(&c->u.protocolDecl.extends, &n->u.protocolDecl.extends);
    list_clone(&c->u.protocolDecl.signatures, &n->u.protocolDecl.signatures);
    break;
  case A_NATIVETYPE:
    list_clone(&c->u.nativeType.typeParameters, &n->u.nativeType.typeParameters);
    break;
  case A_VARIABLEDECL:
    c->u.varDecl.type = ast_clone(n->u.varDecl.type);
    c->u.varDecl.initializer = ast_clone(n->u.varDecl.initializer);
    break;
  case A_BLOCK:
    list_clone(&c->u.block.statements, &n->u.block.statements);
    break;
  case A_SWITCHSTATEMENT:
    c->u.switchStmt.value = ast_clone(n->u.switchStmt.value);
    list_clone(&c->u.switchStmt.cases, &n->u.switchStmt.cases);
    break;
  case A_SWITCHCASE:
    c->u.switchCase.value = ast_clone(n->u.switchCase.value);
    c->u.switchCase.body = ast_clone(n->u.switchCase.body);
    break;
  case A_RETURN:
    c->u.ret.value = ast_clone(n->u.ret.value);
    break;
  case A_IF:
    c->u.ifStmt.conditional = ast_clone(n->u.ifStmt.conditional);
    c->u.ifStmt.body = ast_clone(n->u.ifStmt.body);
    c->u.ifStmt.elseBody = ast_clone(n->u.ifStmt.elseBody);
    break;
  case A_WHILE:
  case A_DOWHILE:
    c->u.loop.conditional = ast_clone(n->u.loop.conditional);
    c->u.loop.body = ast_clone(n->u.loop.body);
    break;
  case A_FOR:
    c->u.forLoop.initialization = ast_clone(n->u.forLoop.initialization);
    c->u.forLoop.condition = ast_clone(n->u.forLoop.condition);
    c->u.forLoop.increment = ast_clone(n->u.forLoop.increment);
    c->u.forLoop.body = ast_clone(n->u.forLoop.body);
    break;
  case A_COMMA:
    list_clone(&c->u.comma.list, &n->u.comma.list);
    break;
  case A_CALL:
    list_clone(&c->u.call.typeArguments, &n->u.call.typeArguments);
    list_clone(&c->u.call.argumentList, &n->u.call.argumentList);
    break;
  case A_DOT:
    c->u.dot.base = ast_clone(n->u.dot.base);
    break;
  case A_INDEX:
    c->u.index.base = ast_clone(n->u.index.base);
    c->u.index.index = ast_clone(n->u.index.index);
    break;
  case A_DEREF:
    c->u.deref.ptr = ast_clone(n->u.deref.ptr);
    break;
  case A_MAKEPTR:
    c->u.lvalueRef.lValue = ast_clone(n->u.lvalueRef.lValue);
    break;
  case A_MAKEARRAYREF:   /* Rewriter.visitMakeArrayRefExpression: the numElements (set by the checker) is visited too */
    c->u.lvalueRef.lValue = ast_clone(n->u.lvalueRef.lValue);
    c->u.lvalueRef.numElements = ast_clone(n->u.lvalueRef.numElements);
    break;
  case A_LOGICALNOT:
    c->u.notExpr.operand = ast_clone(n->u.notExpr.operand);
    break;
  case A_LOGICAL:
    c->u.logical.left = ast_clone(n->u.logical.left);
    c->u.logical.right = ast_clone(n->u.logical.right);
    break;
  case A_ASSIGN:
    c->u.assign.lhs = ast_clone(n->u.assign.lhs);
    c->u.assign.rhs = ast_clone(n->u.assign.rhs);
    break;
  case A_IDENTITY:
    c->u.identity.target = ast_clone(n->u.identity.target);
    break;
  case A_INTLITERAL:
  case A_UINTLITERAL:
    c->u.intLit.preferred = ast_clone(n->u.intLit.preferred);
    break;
  case A_FLOATLITERAL:
  case A_DOUBLELITERAL:
    c->u.fltLit.preferred = ast_clone(n->u.fltLit.preferred);
    break;
  case A_RMW:
    c->u.rmw.lValue = ast_clone(n->u.rmw.lValue);
    c->u.rmw.newValueExp = ast_clone(n->u.rmw.newValueExp);
    c->u.rmw.resultExp = ast_clone(n->u.rmw.resultExp);
    break;
  default:
    break;   /* leaves, and nodes with no child pointers */
  }
  return c;
}

/* cloneProgram: the statements of prog, cloned in order, into a new list. */
static void statements_clone(AstList *dst, const AstList *src) { list_clone(dst, src); }

#endif
