/* WSL preparation passes of Prepare.js, up to the NameResolver: the Visitor's default child walk,
 * ConstexprFolder (become), cloneProgram (with the name table rebuilt), and
 * programWithUnnecessaryThingsRemoved with its NameFinder. The order of every walk and of every
 * insertion follows the JS (Visitor.js, NameFinder.js, Program.js), because the output order of the
 * reduced program depends on it. */
#ifndef WSL_PREP_H
#define WSL_PREP_H

#include "wsl-parse.h"
#include "wsl-clone.h"

typedef void (*ChildFn)(void *cx, Ast *child);

static void each_child_list(const AstList *l, ChildFn f, void *cx) {
  for (int i = 0; i < l->n; i++)
    if (l->v[i]) f(cx, l->v[i]);
}

/* The literal's GenericLiteralType.preferredType: a TypeRef with the literal's origin. Created on
 * first use; a clone never shares it, because the standard library is never visited. */
static Ast *literal_preferred(Ast *n) {
  Ast **slot;
  const char *name;
  switch (n->tag) {
  case A_INTLITERAL: slot = &n->u.intLit.preferred; name = "int"; break;
  case A_UINTLITERAL: slot = &n->u.intLit.preferred; name = "uint"; break;
  case A_FLOATLITERAL: slot = &n->u.fltLit.preferred; name = "float"; break;
  case A_DOUBLELITERAL: slot = &n->u.fltLit.preferred; name = "double"; break;
  default: return NULL;
  }
  if (!*slot) {
    Ast *t = ast_new(A_TYPEREF, n->origin);
    t->native = n->native;
    t->u.typeRef.name = name;
    *slot = t;
  }
  return *slot;
}

/* Visitor.js defaults: the children of a node, in the order the default visit method visits them.
 * Nodes that a pass overrides are handled by that pass before it falls back to this walk. */
static void visit_children(Ast *n, ChildFn f, void *cx) {
#define VC(x) do { Ast *_c = (x); if (_c) f(cx, _c); } while (0)
  switch (n->tag) {
  case A_FUNCDEF:
    VC(n->u.func.returnType);
    each_child_list(&n->u.func.typeParameters, f, cx);
    each_child_list(&n->u.func.parameters, f, cx);
    VC(n->u.func.body);
    break;
  case A_NATIVEFUNC:
  case A_PROTOCOLFUNCDECL:
    VC(n->u.func.returnType);
    each_child_list(&n->u.func.typeParameters, f, cx);
    each_child_list(&n->u.func.parameters, f, cx);
    break;
  case A_FUNCPARAMETER:
  case A_CONSTEXPRTYPEPARAM:
  case A_FIELD:
    VC(n->u.named.type);
    break;
  case A_BLOCK:
    each_child_list(&n->u.block.statements, f, cx);
    break;
  case A_COMMA:
    each_child_list(&n->u.comma.list, f, cx);
    break;
  case A_PROTOCOLDECL:
    each_child_list(&n->u.protocolDecl.extends, f, cx);
    each_child_list(&n->u.protocolDecl.signatures, f, cx);
    break;
  case A_TYPEREF:
    each_child_list(&n->u.typeRef.typeArguments, f, cx);
    break;
  case A_NATIVETYPE:
    each_child_list(&n->u.nativeType.typeParameters, f, cx);
    break;
  case A_TYPEDEF:
    each_child_list(&n->u.typeDef.typeParameters, f, cx);
    VC(n->u.typeDef.type);
    break;
  case A_STRUCTTYPE:
    each_child_list(&n->u.structType.typeParameters, f, cx);
    each_child_list(&n->u.structType.fields, f, cx);
    break;
  case A_TYPEVARIABLE:
    VC(n->u.typeVariable.protocol);
    break;
  case A_ENUMTYPE:
    VC(n->u.enumType.baseType);
    each_child_list(&n->u.enumType.members, f, cx);
    break;
  case A_ENUMMEMBER:
    VC(n->u.nameOnly.value);
    break;
  case A_PTRTYPE:
  case A_ARRAYREFTYPE:
    VC(n->u.ref.elementType);
    break;
  case A_ARRAYTYPE:
    VC(n->u.arrayType.elementType);
    VC(n->u.arrayType.numElements);
    break;
  case A_VARIABLEDECL:
    VC(n->u.varDecl.type);
    VC(n->u.varDecl.initializer);
    break;
  case A_ASSIGN:
    VC(n->u.assign.lhs);
    VC(n->u.assign.rhs);
    break;
  case A_RMW:
    /* the old and new value variables are AnonymousVariables, which have no children */
    VC(n->u.rmw.lValue);
    VC(n->u.rmw.newValueExp);
    VC(n->u.rmw.resultExp);
    break;
  case A_DEREF:
    VC(n->u.deref.ptr);
    break;
  case A_DOT:
    VC(n->u.dot.base);
    break;
  case A_INDEX:
    VC(n->u.index.base);
    VC(n->u.index.index);
    break;
  case A_MAKEPTR:
  case A_MAKEARRAYREF:
    VC(n->u.lvalueRef.lValue);
    break;
  case A_IF:
    VC(n->u.ifStmt.conditional);
    VC(n->u.ifStmt.body);
    VC(n->u.ifStmt.elseBody);
    break;
  case A_WHILE:
    VC(n->u.loop.conditional);
    VC(n->u.loop.body);
    break;
  case A_DOWHILE:
    VC(n->u.loop.body);
    VC(n->u.loop.conditional);
    break;
  case A_FOR:
    VC(n->u.forLoop.initialization);
    VC(n->u.forLoop.condition);
    VC(n->u.forLoop.increment);
    VC(n->u.forLoop.body);
    break;
  case A_SWITCHSTATEMENT:
    VC(n->u.switchStmt.value);
    each_child_list(&n->u.switchStmt.cases, f, cx);
    break;
  case A_SWITCHCASE:
    VC(n->u.switchCase.value);
    VC(n->u.switchCase.body);
    break;
  case A_RETURN:
    VC(n->u.ret.value);
    break;
  case A_INTLITERAL:
  case A_UINTLITERAL:
  case A_FLOATLITERAL:
  case A_DOUBLELITERAL:
    VC(literal_preferred(n));
    break;
  case A_CALL:
    each_child_list(&n->u.call.typeArguments, f, cx);
    each_child_list(&n->u.call.argumentList, f, cx);
    VC(n->u.call.returnType);
    break;
  case A_LOGICALNOT:
    VC(n->u.notExpr.operand);
    break;
  case A_LOGICAL:
    VC(n->u.logical.left);
    VC(n->u.logical.right);
    break;
  case A_IDENTITY:
    VC(n->u.identity.target);
    break;
  default:
    break;   /* no children: leaves, break, continue, trap, null, bool, variable refs, enum literal */
  }
#undef VC
}

/* Node.become: the node turns into an IdentityExpression for target (keeping its origin). */
static void ast_become(Ast *n, Ast *target) {
  n->tag = A_IDENTITY;
  n->u.identity.target = target;
}

/* ---- Program: statements, the name table, and Program.add without its checks ---- */
static const char *stmt_name(const Ast *st) {
  switch (st->tag) {
  case A_FUNCDEF: case A_NATIVEFUNC: return st->u.func.name;
  case A_TYPEDEF: return st->u.typeDef.name;
  case A_STRUCTTYPE: return st->u.structType.name;
  case A_ENUMTYPE: return st->u.enumType.name;
  case A_NATIVETYPE: return st->u.nativeType.name;
  case A_PROTOCOLDECL: return st->u.protocolDecl.name;
  default: return NULL;
  }
}

/* Program.add for a statement already known to be valid (clones and reductions of valid programs). */
static void prog_add_raw(Program *prog, Ast *st) {
  const char *name = stmt_name(st);
  if (name) {
    int found = 0;
    if (name_lookup(prog, name, &found) < 0)
      name_insert(prog, name, st->tag == A_FUNCDEF || st->tag == A_NATIVEFUNC);
  }
  list_push(&prog->statements, st);
}

/* cloneProgram: a new program with each statement cloned, added in order. */
static void prog_clone(Program *dst, const Program *src) {
  memset(dst, 0, sizeof *dst);
  for (int i = 0; i < src->statements.n; i++) prog_add_raw(dst, ast_clone(src->statements.v[i]));
}

/* The things the global NameContext holds under name (underlyingThings(Anything, name)), in add order. */
static void prog_things(const Program *prog, const char *name, AstList *out) {
  out->n = 0;
  for (int i = 0; i < prog->statements.n; i++) {
    Ast *st = prog->statements.v[i];
    const char *n = stmt_name(st);
    if (n && strcmp(n, name) == 0) list_push(out, st);
  }
}

/* ---- NameFinder (NameFinder.js): the names a program needs, in the order they are first added ---- */
#define NF_CAP 8192
typedef struct {
  const char *slots[NF_CAP];   /* hash set of names */
  const char *order[NF_CAP];   /* the set's iteration order (insertion order) */
  int nset;
  const char *work[NF_CAP];    /* the worklist, popped from the end */
  int nwork;
} NameFinder;

static void nf_add(NameFinder *nf, const char *name) {
  if (!name) { fprintf(stderr, "NameFinder: null name\n"); exit(5); }
  unsigned i = name_hash(name) & (NF_CAP - 1);
  while (nf->slots[i]) {
    if (strcmp(nf->slots[i], name) == 0) return;
    i = (i + 1) & (NF_CAP - 1);
  }
  nf->slots[i] = name;
  nf->order[nf->nset++] = name;
  nf->work[nf->nwork++] = name;
}

static void nf_add_concat(NameFinder *nf, const char *a, const char *b, const char *c) {
  char buf[512];
  snprintf(buf, sizeof buf, "%s%s%s", a, b, c);
  nf_add(nf, arena_strndup(buf, (int)strlen(buf)));
}

static void nf_visit_node(NameFinder *nf, Ast *n);
static void nf_child(void *cx, Ast *c) { nf_visit_node((NameFinder *)cx, c); }

/* NameFinder's visit methods; everything else is the default child walk. */
static void nf_visit_node(NameFinder *nf, Ast *n) {
  switch (n->tag) {
  case A_TYPEREF: nf_add(nf, n->u.typeRef.name); break;
  case A_PROTOCOLREF: nf_add(nf, n->u.nameOnly.name); break;
  case A_VARIABLEREF: nf_add(nf, n->u.varRef.name); break;
  case A_TYPEORVARIABLEREF: nf_add(nf, n->u.nameOnly.name); return;
  case A_DOT:
    nf_add_concat(nf, "operator.", n->u.dot.fieldName, "");
    nf_add_concat(nf, "operator.", n->u.dot.fieldName, "=");
    nf_add_concat(nf, "operator&.", n->u.dot.fieldName, "");
    break;
  case A_INDEX:
    nf_add(nf, "operator[]");
    nf_add(nf, "operator[]=");
    nf_add(nf, "operator&[]");
    break;
  case A_CALL: nf_add(nf, n->u.call.name); break;
  default: break;
  }
  visit_children(n, nf_child, nf);
}

static NameFinder NF_STATE;
static AstList PROG_THINGS;

/* programWithUnnecessaryThingsRemoved: the reduced program, in NameFinder set order. */
static void program_removed(Program *out, const Program *in) {
  NameFinder *nf = &NF_STATE;
  memset(nf, 0, sizeof *nf);
  PROG_THINGS.v = NULL;   /* the arena is reset between inputs: the list must not keep its storage */
  PROG_THINGS.n = PROG_THINGS.cap = 0;
  for (int i = 0; i < in->statements.n; i++) {
    Ast *st = in->statements.v[i];
    if (!st->native) nf_add(nf, stmt_name(st));
  }
  nf_add(nf, "operator cast");
  nf_add(nf, "void");
  nf_add(nf, "bool");
  nf_add(nf, "int");
  while (nf->nwork) {
    const char *name = nf->work[--nf->nwork];
    prog_things(in, name, &PROG_THINGS);
    for (int k = 0; k < PROG_THINGS.n; k++) nf_visit_node(nf, PROG_THINGS.v[k]);
  }
  memset(out, 0, sizeof *out);
  for (int k = 0; k < nf->nset; k++) {
    prog_things(in, nf->order[k], &PROG_THINGS);
    for (int j = 0; j < PROG_THINGS.n; j++) prog_add_raw(out, PROG_THINGS.v[j]);
  }
}

/* ---- ConstexprFolder: unary minus on a literal becomes the negated literal ----
 * The JS negConstexpr getter (CreateLiteral.js) drops the origin argument, so the folded literal has
 * no origin (origin -1, traced as "-"). */
static void fold_visit(Ast *n);
static void fold_child(void *cx, Ast *c) { (void)cx; fold_visit(c); }

static Ast *unify_node(Ast *n) {
  while (n->tag == A_IDENTITY) n = n->u.identity.target;
  return n;
}

static Ast *lit_new(AstTag tag, int origin, int native) {
  Ast *a = lit_expr_new(tag, origin);
  a->native = native;
  a->type->native = native;
  return a;
}

static void fold_visit(Ast *n) {
  visit_children(n, fold_child, NULL);
  if (n->tag != A_CALL || strcmp(n->u.call.name, "operator-") != 0) return;
  if (n->u.call.typeArguments.n != 0 || n->u.call.argumentList.n != 1) return;
  Ast *arg = unify_node(n->u.call.argumentList.v[0]);
  Ast *neg = NULL;
  switch (arg->tag) {
  case A_INTLITERAL:   /* (-value) | 0 */
    neg = lit_new(A_INTLITERAL, -1, n->native);
    neg->u.intLit.value = (long long)(int32_t)(uint32_t)(-arg->u.intLit.value);
    break;
  case A_UINTLITERAL:  /* (-value) >>> 0 */
    neg = lit_new(A_UINTLITERAL, -1, n->native);
    neg->u.intLit.value = (long long)(uint32_t)(-arg->u.intLit.value);
    break;
  case A_FLOATLITERAL:
  case A_DOUBLELITERAL: {  /* new IntLiteral(origin, -value): the value is kept as it is */
    double v = -arg->u.fltLit.value;
    if (v != (double)(long long)v) {
      fprintf(stderr, "fold: non-integral negated literal %g\n", v);
      exit(6);
    }
    neg = lit_new(A_INTLITERAL, -1, n->native);
    neg->u.intLit.value = (long long)v;
    break;
  }
  default:
    return;   /* booleans, enum literals and non-literals do not fold */
  }
  ast_become(n, neg);
}

static void fold_program(Program *prog) {
  for (int i = 0; i < prog->statements.n; i++) fold_visit(prog->statements.v[i]);
}

#endif
