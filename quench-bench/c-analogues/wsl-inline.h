/* The instantiation layer after resolveProperties (a C port of Rewriter.js, Substitution.js, AutoWrapper.js,
 * InstantiateImmediates.js, FuncInstantiator.js and the instance parts of StructType.instantiate).
 *
 * The JS Rewriter is one class with visitor methods; a Rewriter instance has its own mapping (_mapNode and
 * _getMapping) and its subclasses change a few methods. Here one RX holds the mode flags of the instance:
 *   aw       AutoWrapper: the leaf types and parameters are wrapped, the references are returned as they are
 *   subst    Substitution: a type or variable in the map is replaced by its argument (wrapped with aw)
 *   immed    InstantiateImmediates: a struct or native type reference is instantiated, reference types are kept
 *   preserve InstantiationInstantiateImmediates: a call keeps its raw type arguments and argument types
 *   inst     the InstantiationSubstitution call hook: a protocol signature is re-resolved, a native callee gets
 *            its instance (nativeFuncInstance)
 * FuncInstantiator.getUnique returns the instance of a function for a list of type arguments, cached per
 * function and argument list, in the order of the JS Map (a function's first getUnique call, then its
 * instances in creation order). Synthesized natives' instantiateImplementation hooks are not ported: their
 * implementation data is not part of the dump, and fatal_unsupported is raised if one is instantiated. */
#ifndef WSL_INLINE_H
#define WSL_INLINE_H

#include "wsl-props.h"

typedef struct RX {
  int aw, subst, immed, preserve, inst;
  Ast **mf, **mt; int mn, mcap;     /* _mapNode / _getMapping (the latest mapping of a key wins) */
  Ast **params, **args; int np;     /* Substitution's map (params -> args) */
} RX;

static Ast *rx_visit(RX *x, Ast *n);
static RX *rx_new(int aw, int subst, int immed, int preserve, int inst) {
  RX *x = arena_alloc(sizeof(RX));
  memset(x, 0, sizeof *x);
  x->aw = aw; x->subst = subst; x->immed = immed; x->preserve = preserve; x->inst = inst;
  return x;
}

static void rx_map(RX *x, Ast *from, Ast *to) {   /* _mapNode */
  if (x->mn == x->mcap) {
    int cap = x->mcap ? x->mcap * 2 : 16;
    Ast **nf = arena_alloc(sizeof(Ast *) * cap), **nt = arena_alloc(sizeof(Ast *) * cap);
    if (x->mn) { memcpy(nf, x->mf, sizeof(Ast *) * x->mn); memcpy(nt, x->mt, sizeof(Ast *) * x->mn); }
    x->mf = nf; x->mt = nt; x->mcap = cap;
  }
  x->mf[x->mn] = from;
  x->mt[x->mn++] = to;
}

static Ast *rx_get(const RX *x, Ast *v) {   /* _getMapping: the mapping of v, or v itself */
  for (int i = x->mn - 1; i >= 0; i--) if (x->mf[i] == v) return x->mt[i];
  return v;
}

/* AutoWrapper of a node (replacement.visit(new AutoWrapper())) */
static Ast *rx_aw(Ast *n) { return rx_visit(rx_new(1, 0, 0, 0, 0), n); }

static AstList rx_list(RX *x, const AstList *src) {
  AstList d = {NULL, 0, 0};
  for (int i = 0; i < src->n; i++) list_push(&d, rx_visit(x, src->v[i]));
  return d;
}

/* Substitution's map lookup by key: the replacement, or NULL */
static Ast *rx_repl(const RX *x, Ast *key) {
  for (int i = 0; i < x->np; i++) if (x->params[i] == key) return x->args[i];
  return NULL;
}

/* FuncInstantiator: the instance table. Each entry is one instance of one function. */
typedef struct { Ast *func; Ast **targs; int nt; Ast *inst; } FInst;
static FInst *FI_TAB;
static int FI_N, FI_CAP;
static Ast **FI_FUNCS;          /* functions in the order of their first getUnique call (the JS Map keys) */
static int FI_NF, FI_FCAP;
static Ast *fi_get_unique(Ast *func, Ast **targs, int nt);

/* StructType.instantiate(typeArguments): a new struct with the fields substituted (when there are arguments)
 * and instantiated immediately. typeArguments is NULL-able only by the caller's choice. */
static Ast *struct_instantiate(Ast *st, Ast **targs, int nt) {
  if (nt != st->u.structType.typeParameters.n)
    name_throw(st, "", "Wrong number of type arguments to instantiation");
  /* typeArguments is an array in JS even when it is empty, so the substitution is always made */
  RX *sub = rx_new(0, 1, 0, 0, 0);
  sub->params = st->u.structType.typeParameters.v;
  sub->np = st->u.structType.typeParameters.n;
  sub->args = targs;
  AstList tps = {NULL, 0, 0};
  RX *imm = rx_new(0, 0, 1, 0, 0);
  Ast *r = mk_from(A_STRUCTTYPE, st);
  r->u.structType.name = st->u.structType.name;
  r->u.structType.typeParameters = tps;
  for (int i = 0; i < st->u.structType.fields.n; i++) {
    Ast *f = st->u.structType.fields.v[i];
    Ast *nf = f;
    nf = rx_visit(sub, nf);
    nf = rx_visit(imm, nf);
    list_push(&r->u.structType.fields, nf);
  }
  return r;
}

static Ast *rx_visit(RX *x, Ast *n) {
  if (!n) return NULL;
  /* AutoWrapper overrides: references are returned as they are, the leaves are wrapped */
  if (x->aw) {
    switch (n->tag) {
    case A_TYPEREF: case A_VARIABLEREF: case A_BOOLLITERAL: return n;
    case A_TYPEVARIABLE: case A_STRUCTTYPE: case A_NATIVETYPE: return typeref_wrap(n);
    case A_CONSTEXPRTYPEPARAM: case A_FUNCPARAMETER: case A_VARIABLEDECL: return variableref_wrap(n);
    case A_INTLITERALTYPE: case A_UINTLITERALTYPE: case A_FLOATLITERALTYPE: case A_DOUBLELITERALTYPE:
    case A_NULLTYPE:
      return n;
    default: break;
    }
  }
  switch (n->tag) {
  /* identity: the Rewriter returns these as they are */
  case A_FUNCDEF: case A_NATIVEFUNC: case A_NATIVEFUNCINST: case A_NATIVETYPE: case A_TYPEDEF:
  case A_STRUCTTYPE: case A_CONSTEXPRTYPEPARAM: case A_PROTOCOLREF: case A_PROTOCOLDECL:
  case A_ENUMTYPE: case A_TYPEVARIABLE: case A_BOOLLITERAL:
  case A_INTLITERALTYPE: case A_UINTLITERALTYPE: case A_FLOATLITERALTYPE: case A_DOUBLELITERALTYPE:
  case A_NULLTYPE:
    return n;

  case A_FUNCPARAMETER: {
    Ast *t = mk_from(A_FUNCPARAMETER, n);
    t->u.named.name = n->u.named.name;
    t->u.named.type = rx_visit(x, n->u.named.type);
    rx_map(x, n, t);
    t->ePtr = n->ePtr;   /* Rewriter.visitFuncParameter */
    return t;
  }
  case A_VARIABLEDECL: {
    Ast *t = mk_from(A_VARIABLEDECL, n);
    t->u.varDecl.name = n->u.varDecl.name;
    t->u.varDecl.type = rx_visit(x, n->u.varDecl.type);
    t->u.varDecl.initializer = rx_visit(x, n->u.varDecl.initializer);
    rx_map(x, n, t);
    t->ePtr = n->ePtr;   /* Rewriter.visitVariableDecl */
    return t;
  }
  case A_ANONVAR: {
    Ast *t = mk_from(A_ANONVAR, n);
    t->type = rx_visit(x, n->type);
    rx_map(x, n, t);
    t->ePtr = n->ePtr;   /* Rewriter.visitAnonymousVariable */
    return t;
  }
  case A_BLOCK: {
    Ast *t = mk_from(A_BLOCK, n);
    for (int i = 0; i < n->u.block.statements.n; i++) list_push(&t->u.block.statements, rx_visit(x, n->u.block.statements.v[i]));
    return t;
  }
  case A_COMMA: {
    Ast *t = mk_from(A_COMMA, n);
    t->u.comma.list = rx_list(x, &n->u.comma.list);
    return t;
  }
  case A_FIELD: {
    Ast *t = mk_from(A_FIELD, n);
    t->u.named.name = n->u.named.name;
    t->u.named.type = rx_visit(x, n->u.named.type);
    return t;
  }
  case A_ENUMMEMBER: {
    Ast *t = mk_from(A_ENUMMEMBER, n);
    t->u.nameOnly.name = n->u.nameOnly.name;
    t->u.nameOnly.value = rx_visit(x, n->u.nameOnly.value);
    return t;
  }
  case A_PROTOCOLFUNCDECL: {
    Ast *t = mk_from(A_PROTOCOLFUNCDECL, n);
    t->u.func.name = n->u.func.name;
    t->u.func.isCast = n->u.func.isCast;
    t->u.func.shaderType = n->u.func.shaderType;
    t->u.func.returnType = rx_visit(x, n->u.func.returnType);
    t->u.func.typeParameters = rx_list(x, &n->u.func.typeParameters);
    t->u.func.parameters = rx_list(x, &n->u.func.parameters);
    t->u.func.hasOv = n->u.func.hasOv;
    t->u.func.ov = n->u.func.ov;
    t->u.func.protocolDecl = n->u.func.protocolDecl;
    return t;
  }
  case A_TYPEREF: {
    if (x->subst) {   /* Substitution.visitTypeRef: a replaced type variable */
      Ast *repl = rx_repl(x, n->u.typeRef.type);
      if (repl) {
        if (n->u.typeRef.typeArguments.n) name_throw(n, "", "Unexpected type arguments on type variable");
        return rx_aw(repl);
      }
    }
    Ast *t = mk_from(A_TYPEREF, n);
    t->u.typeRef.name = n->u.typeRef.name;
    t->u.typeRef.typeArguments = rx_list(x, &n->u.typeRef.typeArguments);
    t->u.typeRef.type = rx_visit(x, n->u.typeRef.type);
    if (!x->immed) return t;
    /* InstantiateImmediates.visitTypeRef */
    Ast *ty = t->u.typeRef.type;
    if (!ty) return t;
    if (ty->tag == A_STRUCTTYPE) {
      Ast *inst = struct_instantiate(ty, t->u.typeRef.typeArguments.v, t->u.typeRef.typeArguments.n);
      return typeref_wrap(inst);
    }
    if (ty->tag == A_NATIVETYPE) {
      if (t->u.typeRef.typeArguments.n)
        fatal_unsupported("native type instance (NativeTypeInstance) in an instantiation", n);
      return typeref_wrap(ty);
    }
    if (t->u.typeRef.typeArguments.n) fatal_unsupported("type arguments on a type without instantiate", n);
    return t;
  }
  case A_PTRTYPE: case A_ARRAYREFTYPE: {
    if (x->immed) return n;   /* InstantiateImmediates.visitReferenceType */
    Ast *t = mk_from(n->tag, n);
    t->u.ref.addressSpace = n->u.ref.addressSpace;
    t->u.ref.elementType = rx_visit(x, n->u.ref.elementType);
    return t;
  }
  case A_ARRAYTYPE: {
    Ast *t = mk_from(A_ARRAYTYPE, n);
    t->u.arrayType.elementType = rx_visit(x, n->u.arrayType.elementType);
    t->u.arrayType.numElements = rx_visit(x, n->u.arrayType.numElements);
    return t;
  }
  case A_VARIABLEREF: {
    if (x->subst) {   /* Substitution.visitVariableRef */
      Ast *repl = rx_repl(x, n->u.varRef.variable);
      if (repl) return rx_aw(repl);
    }
    Ast *t = mk_from(A_VARIABLEREF, n);
    t->u.varRef.name = n->u.varRef.name;
    t->u.varRef.anon = n->u.varRef.anon;
    t->u.varRef.variable = rx_get(x, n->u.varRef.variable);
    return t;
  }
  case A_INTLITERAL: case A_UINTLITERAL: case A_FLOATLITERAL: case A_DOUBLELITERAL: {
    /* Rewriter.visitGenericLiteral: an IntLiteral with the same value (a float literal holds its double) */
    if (!n->type) fatal_unsupported("literal without a type", n);
    Ast *t = mk_from(A_INTLITERAL, n);
    /* the literal type: the same node under AutoWrapper (visitGenericLiteralType returns it), else a new one whose
     * type is the rewritten committed type (result.type = Node.visit(node.type, this)) */
    if (x->aw) t->type = n->type;
    else if (is_lit_type_tag(n->type->tag)) {
      t->type = lit_type_new(n->type->tag, t);
      if (n->type->type) t->type->type = rx_visit(x, n->type->type);
    } else t->type = rx_visit(x, n->type);   /* a typeref-wrapped literal (withType): an ordinary type */
    if (n->tag == A_INTLITERAL || n->tag == A_UINTLITERAL) {
      t->u.intLit.value = n->u.intLit.value;
      t->u.intLit.dbl = n->u.intLit.dbl;
      t->u.intLit.isDbl = n->u.intLit.isDbl;
    } else {
      t->u.intLit.isDbl = 1;
      t->u.intLit.dbl = n->u.fltLit.value;
    }
    t->u.intLit.preferred = rx_visit(x, literal_preferred(n));   /* the literal type's preferredType, visited by this rewriter */
    t->ePtr = n->ePtr;   /* Rewriter.visitGenericLiteral */
    return t;
  }
  case A_NULLLITERAL: {
    Ast *t = lit_expr_new(A_NULLLITERAL, n->origin);
    t->native = n->native;
    /* visitNullLiteral: result.type = node.type.visit(this); AutoWrapper returns the NullType as it is */
    if (x->aw) t->type = n->type;
    else t->type->type = rx_visit(x, n->type->type);
    t->ePtr = n->ePtr;   /* Rewriter.visitNullLiteral */
    return t;
  }
  case A_ENUMLITERAL: {
    Ast *t = mk_from(A_ENUMLITERAL, n);
    t->u.enumLit.member = n->u.enumLit.member;
    t->ePtr = n->ePtr;   /* Rewriter.visitEnumLiteral */
    return t;
  }
  case A_ASSIGN: {
    Ast *t = mk_from(A_ASSIGN, n);
    t->u.assign.lhs = rx_visit(x, n->u.assign.lhs);
    t->u.assign.rhs = rx_visit(x, n->u.assign.rhs);
    t->type = rx_visit(x, n->type);
    return t;
  }
  case A_RMW: {
    Ast *t = mk_from(A_RMW, n);
    t->u.rmw.lValue = rx_visit(x, n->u.rmw.lValue);
    t->u.rmw.oldVar = rx_visit(x, n->u.rmw.oldVar);
    t->u.rmw.newVar = rx_visit(x, n->u.rmw.newVar);
    t->u.rmw.newValueExp = rx_visit(x, n->u.rmw.newValueExp);
    t->u.rmw.resultExp = rx_visit(x, n->u.rmw.resultExp);
    return t;
  }
  case A_DEREF: {
    Ast *t = mk_from(A_DEREF, n);
    t->u.deref.ptr = rx_visit(x, n->u.deref.ptr);
    t->type = rx_visit(x, n->type);
    t->u.deref.addrSpace = n->u.deref.addrSpace;
    return t;
  }
  case A_DOT: case A_INDEX: {
    /* Rewriter._handlePropertyAccessExpression: the overloads, the types and the calls are copied, then updateCalls */
    Ast *base = prop_base(n);
    Ast *t = shallow_copy(n);
    prop_set_base(t, rx_visit(x, base));
    if (n->tag == A_INDEX) t->u.index.index = rx_visit(x, n->u.index.index);
    PropChk *p = prop_chk(t);
    const PropChk *o = prop_chk(n);
    p->baseType = rx_visit(x, o->baseType);
    p->callGet = rx_visit(x, o->callGet);
    p->resGet = rx_visit(x, o->resGet);
    p->callAnd = rx_visit(x, o->callAnd);
    p->resAnd = rx_visit(x, o->resAnd);
    p->callSet = rx_visit(x, o->callSet);
    prop_update_calls(t);
    return t;
  }
  case A_MAKEPTR: case A_MAKEARRAYREF: case A_CONVERTPTR: {
    Ast *t = mk_from(n->tag, n);
    t->u.lvalueRef.lValue = rx_visit(x, n->u.lvalueRef.lValue);
    t->u.lvalueRef.numElements = n->tag == A_MAKEARRAYREF ? rx_visit(x, n->u.lvalueRef.numElements) : NULL;
    t->ePtr = n->ePtr;   /* the Rewriter's MakePtr, MakeArrayRef and ConvertPtrToArrayRef copy the EPtr */
    return t;
  }
  case A_RETURN: {
    Ast *t = mk_from(A_RETURN, n);
    t->u.ret.value = rx_visit(x, n->u.ret.value);
    return t;
  }
  case A_CONTINUE: case A_BREAK: case A_TRAP:
    return mk_from(n->tag, n);
  case A_CALL: {
    Ast *t = mk_from(A_CALL, n);
    t->u.call.name = n->u.call.name;
    /* the preserving call (InstantiationInstantiateImmediates) takes its type arguments raw, the others visit them */
    t->u.call.argumentList = rx_list(x, &n->u.call.argumentList);
    /* processDerivedCallData */
    t->u.call.func = n->u.call.func;
    t->u.call.nfi = n->u.call.nfi;
    t->u.call.hasOv = n->u.call.hasOv;
    t->u.call.ov = n->u.call.ov;
    if (x->preserve) {
      /* InstantiationInstantiateImmediates: the raw actual types, argument types and cast type are kept */
      t->u.call.actualTA = n->u.call.actualTA;
      t->u.call.argTypes = n->u.call.argTypes;
      t->u.call.typeArguments = n->u.call.typeArguments;
      t->u.call.instTA = rx_list(x, &n->u.call.instTA);
      if (n->u.call.isCast) { t->u.call.isCast = 1; t->u.call.returnType = n->u.call.returnType; }
    } else {
      t->u.call.typeArguments = rx_list(x, &n->u.call.typeArguments);
      t->u.call.instTA = rx_list(x, &n->u.call.instTA);
      t->u.call.actualTA = rx_list(x, &n->u.call.actualTA);
      t->u.call.argTypes = rx_list(x, &n->u.call.argTypes);
      if (n->u.call.isCast) {
        t->u.call.isCast = 1;
        t->u.call.returnType = rx_visit(x, n->u.call.returnType);
      }
    }
    t->u.call.resultType = rx_visit(x, n->u.call.resultType);
    t->ePtrAux = n->ePtrAux;   /* processDerivedCallData: the resultEPtr */
    if (x->inst) {   /* InstantiationSubstitution.visitCallExpression */
      Ast *f = t->u.call.func;
      if (f && f->tag == A_PROTOCOLFUNCDECL) {
        Overload ov = resolve_overload(t->u.call.ov, t->u.call.typeArguments.v, t->u.call.typeArguments.n,
                                       t->u.call.argTypes.v, t->u.call.argTypes.n, t->u.call.returnType, 0);
        if (!ov.func) name_throw(n, "Could not resolve protocol signature function call during instantiation: ", t->u.call.name);
        resolve_to_overload(t, &ov);
        t->u.call.instTA = t->u.call.actualTA;   /* resolveToOverload: instantiatedActualTypeArguments = actualTypeArguments */
        f = t->u.call.func;
      }
      if (f && f->tag == A_NATIVEFUNC) t->u.call.nfi = fi_get_unique(f, t->u.call.actualTA.v, t->u.call.actualTA.n);
    }
    return t;
  }
  case A_FUNCTIONLIKEBLOCK:
    fatal_unsupported("FunctionLikeBlock in an instantiation", n);
    return n;
  case A_LOGICALNOT: {
    Ast *t = mk_from(A_LOGICALNOT, n);
    t->u.notExpr.operand = rx_visit(x, n->u.notExpr.operand);
    t->ePtr = n->ePtr;   /* Rewriter.visitLogicalNot */
    return t;
  }
  case A_LOGICAL: {
    Ast *t = mk_from(A_LOGICAL, n);
    t->u.logical.text = n->u.logical.text;
    t->u.logical.left = rx_visit(x, n->u.logical.left);
    t->u.logical.right = rx_visit(x, n->u.logical.right);
    t->ePtr = n->ePtr;   /* Rewriter.visitLogicalExpression */
    return t;
  }
  case A_IF: {
    Ast *t = mk_from(A_IF, n);
    t->u.ifStmt.conditional = rx_visit(x, n->u.ifStmt.conditional);
    t->u.ifStmt.body = rx_visit(x, n->u.ifStmt.body);
    t->u.ifStmt.elseBody = rx_visit(x, n->u.ifStmt.elseBody);
    return t;
  }
  case A_WHILE: {
    Ast *t = mk_from(A_WHILE, n);
    t->u.loop.conditional = rx_visit(x, n->u.loop.conditional);
    t->u.loop.body = rx_visit(x, n->u.loop.body);
    return t;
  }
  case A_DOWHILE: {
    Ast *t = mk_from(A_DOWHILE, n);
    t->u.loop.body = rx_visit(x, n->u.loop.body);
    t->u.loop.conditional = rx_visit(x, n->u.loop.conditional);
    return t;
  }
  case A_FOR: {
    Ast *t = mk_from(A_FOR, n);
    t->u.forLoop.initialization = rx_visit(x, n->u.forLoop.initialization);
    t->u.forLoop.condition = rx_visit(x, n->u.forLoop.condition);
    t->u.forLoop.increment = rx_visit(x, n->u.forLoop.increment);
    t->u.forLoop.body = rx_visit(x, n->u.forLoop.body);
    return t;
  }
  case A_SWITCHSTATEMENT: {
    Ast *t = mk_from(A_SWITCHSTATEMENT, n);
    t->u.switchStmt.value = rx_visit(x, n->u.switchStmt.value);
    t->u.switchStmt.cases = rx_list(x, &n->u.switchStmt.cases);
    t->type = rx_visit(x, n->type);
    return t;
  }
  case A_SWITCHCASE: {
    Ast *t = mk_from(A_SWITCHCASE, n);
    t->u.switchCase.value = rx_visit(x, n->u.switchCase.value);
    t->u.switchCase.body = rx_visit(x, n->u.switchCase.body);
    return t;
  }
  case A_IDENTITY: {   /* new IdentityExpression(target.visit): the origin is the target's */
    Ast *tg = rx_visit(x, n->u.identity.target);
    Ast *t = mk_from(A_IDENTITY, tg);
    t->u.identity.target = tg;
    return t;
  }
  default:
    fatal_unsupported("instantiation Rewriter", n);
  }
  return n;
}

/* ---- FuncInstantiator.getUnique ---- */
static void fi_note_func(Ast *func) {
  for (int i = 0; i < FI_NF; i++) if (FI_FUNCS[i] == func) return;
  if (FI_NF == FI_FCAP) {
    int cap = FI_FCAP ? FI_FCAP * 2 : 64;
    Ast **nf = malloc(sizeof(Ast *) * cap);
    if (FI_NF) memcpy(nf, FI_FUNCS, sizeof(Ast *) * FI_NF);
    free(FI_FUNCS);
    FI_FUNCS = nf; FI_FCAP = cap;
  }
  FI_FUNCS[FI_NF++] = func;
}

static Ast *fi_get_unique(Ast *func, Ast **targs, int nt) {
  fi_note_func(func);
  /* the instance for these arguments, if there is one */
  for (int i = 0; i < FI_N; i++) {
    if (FI_TAB[i].func != func || FI_TAB[i].nt != nt) continue;
    int ok = 1;
    for (int k = nt; k--;) {
      if (!ut_equals(FI_TAB[i].targs[k], targs[k])) { ok = 0; break; }
    }
    if (ok) return FI_TAB[i].inst;
  }
  int tpn = func->u.func.typeParameters.n;
  if (tpn != nt) fatal_unsupported("type argument count of an instantiation", func);
  RX *sub = rx_new(0, 1, 0, 0, 1);   /* InstantiationSubstitution: the call hook is on */
  sub->params = func->u.func.typeParameters.v;
  sub->np = tpn;
  sub->args = targs;
  RX *imm = rx_new(0, 0, 1, 1, 1);
  Ast *inst;
  if (func->tag == A_FUNCDEF) {
    Ast *ret = rx_visit(sub, func->u.func.returnType);
    ret = rx_visit(imm, ret);
    AstList params = {NULL, 0, 0};
    for (int i = 0; i < func->u.func.parameters.n; i++) list_push(&params, rx_visit(sub, func->u.func.parameters.v[i]));
    for (int i = 0; i < params.n; i++) params.v[i] = rx_visit(imm, params.v[i]);
    Ast *body = rx_visit(sub, func->u.func.body);
    body = rx_visit(imm, body);
    inst = mk_from(A_FUNCDEF, func);
    inst->u.func.name = func->u.func.name;
    inst->u.func.isCast = func->u.func.isCast;
    inst->u.func.shaderType = func->u.func.shaderType;
    inst->u.func.isRestricted = 0;
    inst->u.func.returnType = ret;
    inst->u.func.parameters = params;
    inst->u.func.body = body;
    inst->u.func.orig = func;
  } else if (func->tag == A_NATIVEFUNC) {
    /* NativeFunc.instantiateImplementation is not ported (see the header comment) */
    if (func->u.func.hasOv) fatal_unsupported("native with an instantiation hook", func);
    Ast *ret = rx_visit(sub, func->u.func.returnType);
    ret = rx_visit(imm, ret);
    AstList params = {NULL, 0, 0};
    for (int i = 0; i < func->u.func.parameters.n; i++) list_push(&params, rx_visit(sub, func->u.func.parameters.v[i]));
    for (int i = 0; i < params.n; i++) params.v[i] = rx_visit(imm, params.v[i]);
    inst = mk_from(A_NATIVEFUNCINST, func);
    inst->u.func.name = func->u.func.name;
    inst->u.func.isCast = func->u.func.isCast;
    inst->u.func.shaderType = func->u.func.shaderType;
    inst->u.func.returnType = ret;
    inst->u.func.parameters = params;
    inst->u.func.orig = func;
    /* NativeFunc.instantiateImplementation of a synthesized accessor: implementationData = {type, fieldName}, where
     * type is the struct instantiated with the type arguments (the layout data is not part of this port yet) */
    if (func->u.func.implStruct) {
      inst->u.func.implType = struct_instantiate(func->u.func.implStruct, targs, nt);
      inst->u.func.implField = func->u.func.implField;
    }
  } else {
    fatal_unsupported("getUnique of a non-function", func);
    return NULL;
  }
  /* uninstantiatedReturnType: the return type with the substitution alone */
  inst->u.func.uninstRet = rx_visit(sub, func->u.func.returnType);
  if (FI_N == FI_CAP) {
    int cap = FI_CAP ? FI_CAP * 2 : 256;
    FInst *nt2 = malloc(sizeof(FInst) * cap);
    if (FI_N) memcpy(nt2, FI_TAB, sizeof(FInst) * FI_N);
    free(FI_TAB);
    FI_TAB = nt2; FI_CAP = cap;
  }
  FI_TAB[FI_N].func = func;
  FI_TAB[FI_N].targs = arena_alloc(sizeof(Ast *) * (nt ? nt : 1));
  for (int i = 0; i < nt; i++) FI_TAB[FI_N].targs[i] = targs[i];
  FI_TAB[FI_N].nt = nt;
  FI_TAB[FI_N].inst = inst;
  FI_N++;
  return inst;
}

/* inline()'s first loop, without the inlining: each non-generic function of the program, in the order of the
 * JS program.functions Map (a name at its first statement, then every function of that name in order) gets
 * getUnique(func, []). */
static int fi_name_eq(const char *a, const char *b) { return (!a || !b) ? a == b : strcmp(a, b) == 0; }
/* the non-generic functions of inline()'s first loop, in the order of the JS program.functions Map (a name at its
 * first statement, then every function of that name in order); out must hold the program's statement count */
static int top_funcs(const Program *p, Ast **out) {
  int nout = 0;
  for (int k = 0; k < p->statements.n; k++) {
    Ast *s = p->statements.v[k];
    if (s->tag != A_FUNCDEF && s->tag != A_NATIVEFUNC) continue;
    int first = 1;
    for (int j = 0; j < k && first; j++) {
      Ast *o = p->statements.v[j];
      if ((o->tag == A_FUNCDEF || o->tag == A_NATIVEFUNC) && fi_name_eq(o->u.func.name, s->u.func.name)) first = 0;
    }
    if (!first) continue;
    for (int j = k; j < p->statements.n; j++) {
      Ast *f = p->statements.v[j];
      if ((f->tag != A_FUNCDEF && f->tag != A_NATIVEFUNC) || !fi_name_eq(f->u.func.name, s->u.func.name)) continue;
      if (f->u.func.typeParameters.n == 0) out[nout++] = f;
    }
  }
  return nout;
}

static void fi_instantiate_top(const Program *p) {
  Ast **fs = arena_alloc(sizeof(Ast *) * (p->statements.n + 1));
  int n = top_funcs(p, fs);
  for (int i = 0; i < n; i++) fi_get_unique(fs[i], NULL, 0);
}

static void fi_reset(void) { FI_N = 0; FI_NF = 0; }

#endif
