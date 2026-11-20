/* A port of Inliner.js (a Rewriter subclass), VisitingSet.js and Inline.js (inline, _inlineFunction).
 * inline() runs over the non-generic functions of the program, in the order of inline()'s first loop: each
 * function's instance is made, then _inlineFunction: unless it is native or already inlined, LateChecker, the
 * layout and EBufferBuilder run on it (wsl-visit.h), then it is rewritten by the Inliner. The Inliner rewrites
 * each node the way Rewriter.js does (the copies of the EPtrs included); a call to a non-native function is
 * replaced by a FunctionLikeBlock of the callee's (instance's) parameters and body, after the callee is inlined
 * itself. VisitingSet guards the calls: a callee that is already being inlined is a "Recursive Func" error.
 * The Rewriter's mapping (_mapNode: the parameters, variables and anonymous variables of the rewritten function)
 * is the RX map of wsl-inline.h. */
#ifndef WSL_INLINER_H
#define WSL_INLINER_H

typedef struct { Ast **v; int n, cap; } VSet;   /* VisitingSet: the functions being inlined */
static VSet *INL_VIS;                            /* the set of the current inline() call (shared by its nested calls) */

static void vset_push(VSet *s, Ast *f) {
  if (s->n == s->cap) {
    int cap = s->cap ? s->cap * 2 : 16;
    Ast **nv = arena_alloc(sizeof(Ast *) * cap);
    if (s->n) memcpy(nv, s->v, sizeof(Ast *) * s->n);
    s->v = nv;
    s->cap = cap;
  }
  s->v[s->n++] = f;
}

static void vset_pop(VSet *s, Ast *f) {
  for (int i = s->n - 1; i >= 0; i--)
    if (s->v[i] == f) {
      for (int k = i; k + 1 < s->n; k++) s->v[k] = s->v[k + 1];
      s->n--;
      return;
    }
}

static int vset_has(const VSet *s, Ast *f) {
  for (int i = 0; i < s->n; i++) if (s->v[i] == f) return 1;
  return 0;
}

static void inline_function(Ast *func);

/* Rewriter.visit(node) for a list (a null child stays null) */
static AstList inl_list(RX *m, const AstList *src);
static Ast *inl_visit(RX *m, Ast *n);

static AstList inl_list(RX *m, const AstList *src) {
  AstList d = {NULL, 0, 0};
  for (int i = 0; i < src->n; i++) list_push(&d, inl_visit(m, src->v[i]));
  return d;
}

/* Inliner.visitCallExpression, after Rewriter.visitCallExpression built the result (rt). */
static Ast *inl_call(Ast *n, Ast *rt) {
  if (rt->u.call.nfi) return rt;   /* a native callee keeps its call */
  Ast *callee = n->u.call.func;
  if (vset_has(INL_VIS, callee)) wtype_error(origin_string(callee), "Recursive Func");
  vset_push(INL_VIS, callee);
  Ast *func = fi_get_unique(rt->u.call.func, rt->u.call.actualTA.v, rt->u.call.actualTA.n);
  if (func->tag == A_NATIVEFUNC || func->tag == A_NATIVEFUNCINST) fatal_unsupported("Unexpected native func", func);
  inline_function(func);
  Ast *blk = mk_from(A_FUNCTIONLIKEBLOCK, rt);
  blk->u.fblock.returnType = func->u.func.returnType;
  blk->u.fblock.argumentList = rt->u.call.argumentList;
  blk->u.fblock.parameters = func->u.func.parameters;
  blk->u.fblock.body = func->u.func.body;
  blk->ePtrAux = rt->ePtrAux;   /* returnEPtr = result.resultEPtr */
  vset_pop(INL_VIS, callee);
  return blk;
}

/* Rewriter.visit(n) for a node of a function body (Rewriter.js, with the Inliner's call), for the mapping m */
static Ast *inl_visit(RX *m, Ast *n) {
  if (!n) return NULL;
  switch (n->tag) {
  /* identity: the Rewriter returns these as they are */
  case A_FUNCDEF: case A_NATIVEFUNC: case A_NATIVEFUNCINST: case A_NATIVETYPE: case A_TYPEDEF:
  case A_STRUCTTYPE: case A_CONSTEXPRTYPEPARAM: case A_PROTOCOLREF: case A_PROTOCOLDECL:
  case A_ENUMTYPE: case A_TYPEVARIABLE: case A_BOOLLITERAL:
    return n;

  case A_FUNCPARAMETER: {
    Ast *t = mk_from(A_FUNCPARAMETER, n);
    t->u.named.name = n->u.named.name;
    t->u.named.type = inl_visit(m, n->u.named.type);
    rx_map(m, n, t);
    t->ePtr = n->ePtr;
    return t;
  }
  case A_VARIABLEDECL: {
    Ast *t = mk_from(A_VARIABLEDECL, n);
    t->u.varDecl.name = n->u.varDecl.name;
    t->u.varDecl.type = inl_visit(m, n->u.varDecl.type);
    t->u.varDecl.initializer = inl_visit(m, n->u.varDecl.initializer);
    rx_map(m, n, t);
    t->ePtr = n->ePtr;
    return t;
  }
  case A_BLOCK: {
    Ast *t = mk_from(A_BLOCK, n);
    for (int i = 0; i < n->u.block.statements.n; i++) list_push(&t->u.block.statements, inl_visit(m, n->u.block.statements.v[i]));
    return t;
  }
  case A_COMMA: {
    Ast *t = mk_from(A_COMMA, n);
    t->u.comma.list = inl_list(m, &n->u.comma.list);
    return t;
  }
  case A_TYPEREF: {
    Ast *t = mk_from(A_TYPEREF, n);
    t->u.typeRef.name = n->u.typeRef.name;
    t->u.typeRef.typeArguments = inl_list(m, &n->u.typeRef.typeArguments);
    t->u.typeRef.type = inl_visit(m, n->u.typeRef.type);
    return t;
  }
  case A_ENUMLITERAL: {
    Ast *t = mk_from(A_ENUMLITERAL, n);
    t->u.enumLit.member = n->u.enumLit.member;
    t->ePtr = n->ePtr;
    return t;
  }
  case A_PTRTYPE: case A_ARRAYREFTYPE: {
    Ast *t = mk_from(n->tag, n);
    t->u.ref.addressSpace = n->u.ref.addressSpace;
    t->u.ref.elementType = inl_visit(m, n->u.ref.elementType);
    return t;
  }
  case A_ARRAYTYPE: {
    Ast *t = mk_from(A_ARRAYTYPE, n);
    t->u.arrayType.elementType = inl_visit(m, n->u.arrayType.elementType);
    t->u.arrayType.numElements = inl_visit(m, n->u.arrayType.numElements);
    return t;
  }
  case A_ASSIGN: {
    Ast *t = mk_from(A_ASSIGN, n);
    t->u.assign.lhs = inl_visit(m, n->u.assign.lhs);
    t->u.assign.rhs = inl_visit(m, n->u.assign.rhs);
    t->type = inl_visit(m, n->type);
    return t;
  }
  case A_RMW: {
    Ast *t = mk_from(A_RMW, n);
    t->u.rmw.lValue = inl_visit(m, n->u.rmw.lValue);
    t->u.rmw.oldVar = inl_visit(m, n->u.rmw.oldVar);
    t->u.rmw.newVar = inl_visit(m, n->u.rmw.newVar);
    t->u.rmw.newValueExp = inl_visit(m, n->u.rmw.newValueExp);
    t->u.rmw.resultExp = inl_visit(m, n->u.rmw.resultExp);
    return t;
  }
  case A_DEREF: {
    Ast *t = mk_from(A_DEREF, n);
    t->u.deref.ptr = inl_visit(m, n->u.deref.ptr);
    t->type = inl_visit(m, n->type);
    t->u.deref.addrSpace = n->u.deref.addrSpace;
    return t;
  }
  case A_DOT: case A_INDEX: {
    /* Rewriter._handlePropertyAccessExpression: as the instantiation Rewriter (wsl-inline.h) copies it */
    Ast *base = prop_base(n);
    Ast *t = shallow_copy(n);
    prop_set_base(t, inl_visit(m, base));
    if (n->tag == A_INDEX) t->u.index.index = inl_visit(m, n->u.index.index);
    PropChk *p = prop_chk(t);
    const PropChk *o = prop_chk(n);
    p->baseType = inl_visit(m, o->baseType);
    p->callGet = inl_visit(m, o->callGet);
    p->resGet = inl_visit(m, o->resGet);
    p->callAnd = inl_visit(m, o->callAnd);
    p->resAnd = inl_visit(m, o->resAnd);
    p->callSet = inl_visit(m, o->callSet);
    prop_update_calls(t);
    return t;
  }
  case A_MAKEPTR: case A_CONVERTPTR: case A_MAKEARRAYREF: {
    Ast *t = mk_from(n->tag, n);
    t->u.lvalueRef.lValue = inl_visit(m, n->u.lvalueRef.lValue);
    if (n->tag == A_MAKEARRAYREF && n->u.lvalueRef.numElements) t->u.lvalueRef.numElements = inl_visit(m, n->u.lvalueRef.numElements);
    t->ePtr = n->ePtr;
    return t;
  }
  case A_VARIABLEREF: {
    Ast *t = mk_from(A_VARIABLEREF, n);
    t->u.varRef.name = n->u.varRef.name;
    t->u.varRef.anon = n->u.varRef.anon;
    t->u.varRef.variable = rx_get(m, n->u.varRef.variable);
    return t;
  }
  case A_RETURN: {
    Ast *t = mk_from(A_RETURN, n);
    t->u.ret.value = inl_visit(m, n->u.ret.value);
    return t;
  }
  case A_CONTINUE: case A_BREAK: case A_TRAP:
    return mk_from(n->tag, n);
  case A_INTLITERAL: case A_UINTLITERAL: case A_FLOATLITERAL: case A_DOUBLELITERAL: {
    /* Rewriter.visitGenericLiteral: an IntLiteral with the same value; its type is visited (a new literal type of the
     * same class, whose type and preferredType are visited) and the EPtr is copied */
    if (!n->type) fatal_unsupported("literal without a type", n);
    Ast *t = mk_from(A_INTLITERAL, n);
    if (is_lit_type_tag(n->type->tag)) {
      t->type = lit_type_new(n->type->tag, t);
      if (n->type->type) t->type->type = inl_visit(m, n->type->type);
    } else
      t->type = inl_visit(m, n->type);   /* a typeref-wrapped literal (withType): an ordinary type */
    if (n->tag == A_INTLITERAL || n->tag == A_UINTLITERAL) {
      t->u.intLit.value = n->u.intLit.value;
      t->u.intLit.dbl = n->u.intLit.dbl;
      t->u.intLit.isDbl = n->u.intLit.isDbl;
    } else {
      t->u.intLit.isDbl = 1;
      t->u.intLit.dbl = n->u.fltLit.value;
    }
    t->u.intLit.preferred = inl_visit(m, literal_preferred(n));
    t->ePtr = n->ePtr;
    return t;
  }
  case A_NULLLITERAL: {
    Ast *t = lit_expr_new(A_NULLLITERAL, n->origin);
    t->native = n->native;
    t->type->type = inl_visit(m, n->type->type);
    t->ePtr = n->ePtr;
    return t;
  }
  case A_CALL: {
    /* Rewriter.visitCallExpression and processDerivedCallData, then the Inliner's call */
    Ast *t = mk_from(A_CALL, n);
    t->u.call.name = n->u.call.name;
    t->u.call.typeArguments = inl_list(m, &n->u.call.typeArguments);
    t->u.call.argumentList = inl_list(m, &n->u.call.argumentList);
    t->u.call.actualTA = inl_list(m, &n->u.call.actualTA);
    t->u.call.argTypes = inl_list(m, &n->u.call.argTypes);
    t->u.call.instTA = inl_list(m, &n->u.call.instTA);
    t->u.call.func = n->u.call.func;
    t->u.call.nfi = n->u.call.nfi;
    t->u.call.hasOv = n->u.call.hasOv;
    t->u.call.ov = n->u.call.ov;
    if (n->u.call.isCast) {
      t->u.call.isCast = 1;
      t->u.call.returnType = inl_visit(m, n->u.call.returnType);
    }
    t->u.call.resultType = inl_visit(m, n->u.call.resultType);
    t->ePtrAux = n->ePtrAux;
    return inl_call(n, t);
  }
  case A_LOGICALNOT: {
    Ast *t = mk_from(A_LOGICALNOT, n);
    t->u.notExpr.operand = inl_visit(m, n->u.notExpr.operand);
    t->ePtr = n->ePtr;
    return t;
  }
  case A_LOGICAL: {
    Ast *t = mk_from(A_LOGICAL, n);
    t->u.logical.text = n->u.logical.text;
    t->u.logical.left = inl_visit(m, n->u.logical.left);
    t->u.logical.right = inl_visit(m, n->u.logical.right);
    t->ePtr = n->ePtr;
    return t;
  }
  case A_IF: {
    Ast *t = mk_from(A_IF, n);
    t->u.ifStmt.conditional = inl_visit(m, n->u.ifStmt.conditional);
    t->u.ifStmt.body = inl_visit(m, n->u.ifStmt.body);
    t->u.ifStmt.elseBody = inl_visit(m, n->u.ifStmt.elseBody);
    return t;
  }
  case A_WHILE: {
    Ast *t = mk_from(A_WHILE, n);
    t->u.loop.conditional = inl_visit(m, n->u.loop.conditional);
    t->u.loop.body = inl_visit(m, n->u.loop.body);
    return t;
  }
  case A_DOWHILE: {
    Ast *t = mk_from(A_DOWHILE, n);
    t->u.loop.body = inl_visit(m, n->u.loop.body);
    t->u.loop.conditional = inl_visit(m, n->u.loop.conditional);
    return t;
  }
  case A_FOR: {
    Ast *t = mk_from(A_FOR, n);
    t->u.forLoop.initialization = inl_visit(m, n->u.forLoop.initialization);
    t->u.forLoop.condition = inl_visit(m, n->u.forLoop.condition);
    t->u.forLoop.increment = inl_visit(m, n->u.forLoop.increment);
    t->u.forLoop.body = inl_visit(m, n->u.forLoop.body);
    return t;
  }
  case A_SWITCHSTATEMENT: {
    Ast *t = mk_from(A_SWITCHSTATEMENT, n);
    t->u.switchStmt.value = inl_visit(m, n->u.switchStmt.value);
    t->u.switchStmt.cases = inl_list(m, &n->u.switchStmt.cases);
    t->type = inl_visit(m, n->type);
    return t;
  }
  case A_SWITCHCASE: {
    Ast *t = mk_from(A_SWITCHCASE, n);
    t->u.switchCase.value = inl_visit(m, n->u.switchCase.value);
    t->u.switchCase.body = inl_visit(m, n->u.switchCase.body);
    return t;
  }
  case A_ANONVAR: {
    Ast *t = mk_from(A_ANONVAR, n);
    t->type = inl_visit(m, n->type);
    rx_map(m, n, t);
    t->ePtr = n->ePtr;
    return t;
  }
  case A_IDENTITY: {   /* new IdentityExpression(target.visit): the origin is the target's */
    Ast *tg = inl_visit(m, n->u.identity.target);
    Ast *t = mk_from(A_IDENTITY, tg);
    t->u.identity.target = tg;
    return t;
  }
  case A_INTLITERALTYPE: case A_UINTLITERALTYPE: case A_FLOATLITERALTYPE: case A_DOUBLELITERALTYPE:
  case A_NULLTYPE: {
    /* visitGenericLiteralType and visitNullType: a new type of the same class and value (the value is read from its
     * owner, so the owner is a copy of the literal with the visited preferredType), whose type is visited */
    Ast *o = n->u.litType.owner;
    Ast *o2 = mk_from(o->tag, o);
    o2->u = o->u;
    o2->type = NULL;
    Ast *t = lit_type_new(n->tag, o2);
    t->type = inl_visit(m, n->type);
    if (n->tag != A_NULLTYPE) {
      Ast *pref = inl_visit(m, literal_preferred(o));
      if (o->tag == A_INTLITERAL || o->tag == A_UINTLITERAL) o2->u.intLit.preferred = pref;
      else o2->u.fltLit.preferred = pref;
    }
    return t;
  }
  case A_FUNCTIONLIKEBLOCK: {   /* visitFunctionLikeBlock: the returnEPtr is copied */
    Ast *t = mk_from(A_FUNCTIONLIKEBLOCK, n);
    t->u.fblock.returnType = inl_visit(m, n->u.fblock.returnType);
    t->u.fblock.argumentList = inl_list(m, &n->u.fblock.argumentList);
    t->u.fblock.parameters = inl_list(m, &n->u.fblock.parameters);
    t->u.fblock.body = inl_visit(m, n->u.fblock.body);
    t->ePtrAux = n->ePtrAux;
    return t;
  }
  default:
    fatal_unsupported("inliner rewrite", n);
  }
  return n;
}

/* _inlineFunction(program, func, visiting) */
static void inline_function(Ast *func) {
  if (func->u.func.typeParameters.n) chk_error("Cannot inline function that has type parameters");
  if (func->tag != A_FUNCDEF || func->u.func.inlined) return;   /* a native (isNative) or inlined function */
  static Buf scratch;
  VTrace lt = {&scratch, 0, 1, 0, 0};
  vt_node(&lt, func);
  scratch.n = 0;
  LSB_OFF = 0;
  VTrace st = {&scratch, 0, 0, 1, 0};
  vt_node(&st, func);
  scratch.n = 0;
  VTrace et = {&scratch, 0, 0, 0, 1};
  vt_node(&et, func);
  scratch.n = 0;
  /* func.rewrite(new Inliner): the return type, the parameters, then the body */
  RX *m = rx_new(0, 0, 0, 0, 0);
  func->u.func.returnType = inl_visit(m, func->u.func.returnType);
  AstList ps = {NULL, 0, 0};
  for (int i = 0; i < func->u.func.parameters.n; i++) list_push(&ps, inl_visit(m, func->u.func.parameters.v[i]));
  func->u.func.parameters = ps;
  func->u.func.body = inl_visit(m, func->u.func.body);
  func->u.func.inlined = 1;
}

/* inline(program): every non-generic function, in inline()'s first loop order, with its own VisitingSet */
static void inline_program(const Program *p) {
  Ast **fs = arena_alloc(sizeof(Ast *) * (p->statements.n + 1));
  int n = top_funcs(p, fs);
  for (int i = 0; i < n; i++) {
    Ast *inst = fi_get_unique(fs[i], NULL, 0);
    VSet vs = {NULL, 0, 0};
    vset_push(&vs, inst);
    INL_VIS = &vs;
    inline_function(inst);
  }
  INL_VIS = NULL;
}

/* --inline: the instances in the order of the funcInstantiator's _instances Map, each as a header and its tree with
 * its EPtrs, then "buffers N" and the buffers (as --ebuf). The text has no trailing newline. */
static void inline_dump(Buf *out) {
  Buf body = {0};
  int k = 0;
  EBUF_DUMP = 1;
  for (int f = 0; f < FI_NF; f++)
    for (int i = 0; i < FI_N; i++) {
      if (FI_TAB[i].func != FI_FUNCS[f]) continue;
      buf_fmt(&body, "instance %d %s %d\n", k++, ast_class_name(FI_TAB[i].func), FI_TAB[i].nt);
      dump_node(&body, FI_TAB[i].inst, 1);
    }
  EBUF_DUMP = 0;
  buf_fmt(&body, "buffers %d\n", EBUF_NALL);
  for (int j = 0; j < EBUF_NALL; j++) {
    buf_str(&body, ebuf_str(EBUF_ALL[j]));
    buf_str(&body, "\n");
  }
  if (body.n && body.s[body.n - 1] == '\n') body.n--;
  buf_put(out, body.s, body.n);
}

/* ---- name-based resolution (NameContext.resolveFuncOverload, Inline.js resolveInlinedFunction) ---- */
/* The global NameContext of the program being prepared (program.globalNameContext); set by the driver. */
static NameCtx *PROG_GLOBAL_NC;

/* NameContext.resolveFuncOverload(name, typeArguments, argumentTypes, returnType, allowEntryPoint): a name with no
 * function resolves to no candidates (failures []). */
static Overload resolve_func_overload(const char *name, Ast **targs, int ntargs, Ast **args, int nargs, Ast *ret,
                                      int allowEntry) {
  NameEntry *e = nc_get(PROG_GLOBAL_NC, K_FUNC, name);
  if (!e) return overload_fail(NULL);
  return resolve_overload(&e->funcs, targs, ntargs, args, nargs, ret, allowEntry);
}

/* resolveInlinedFunction(program, name, typeArguments, argumentTypes, allowEntryPoint): the instance of the matched
 * function, inlined with its own VisitingSet, or NULL with the candidates' failures in failOut. */
static Ast *resolve_inlined_function(const char *name, Ast **targs, int ntargs, Ast **args, int nargs, int allowEntry,
                                     Overload *failOut) {
  Overload o = resolve_func_overload(name, targs, ntargs, args, nargs, NULL, allowEntry);
  if (!o.func) {
    *failOut = o;
    return NULL;
  }
  Ast *func = fi_get_unique(o.func, o.targs, o.ntargs);
  VSet *saved = INL_VIS;
  VSet vs = {NULL, 0, 0};
  vset_push(&vs, o.func);
  INL_VIS = &vs;
  inline_function(func);
  INL_VIS = saved;
  return func;
}

#endif
