/* WSL checks and synthesis between name resolution and the checker (a C port of Prepare.js's steps):
 * TypeDefResolver (resolveTypeDefsInTypes/Protocols), RecursiveTypeChecker, SynthesizeStructAccessors,
 * SynthesizeEnumFunctions, and resolveNamesInFunctions. The Rewriter, Substitution, AutoWrapper and
 * TypeParameterRewriter that these use are ported here in the forms the passes need. */
#ifndef WSL_SYNTH_H
#define WSL_SYNTH_H

#include "wsl-names.h"

static void fatal_unsupported(const char *what, const Ast *n) {
  fprintf(stderr, "unsupported in stage 3/4: %s (%s at %d)\n", what, ast_class_name(n), n->origin);
  exit(9);
}

/* A node made from a source node's origin (the JS constructors take the source's origin token). */
static Ast *mk_from(AstTag tag, const Ast *src) {
  Ast *a = ast_new(tag, src->origin);
  a->native = src->native;
  return a;
}

/* ---- Rewriter / Substitution / AutoWrapper ----
 * r->n > 0: Substitution (the map olds -> news); r->aw: AutoWrapper; otherwise the plain Rewriter. */
typedef struct {
  Ast **olds;
  Ast **news;
  int n;
  int aw;
} Rw;

static Ast *rw(const Rw *r, Ast *n);

static Ast *rw_aw(Ast *n) {
  Rw a = {NULL, NULL, 0, 1};
  return rw(&a, n);
}

static Ast *rw_plain(Ast *n) {
  Rw a = {NULL, NULL, 0, 0};
  return rw(&a, n);
}

/* TypeRef.wrap(type) */
static Ast *typeref_wrap(Ast *t) {
  Ast *r = mk_from(A_TYPEREF, t);
  r->u.typeRef.name = thing_name(t);
  r->u.typeRef.type = t;
  return r;
}

/* VariableRef.wrap(variable) */
static Ast *variableref_wrap(Ast *v) {
  Ast *r = mk_from(A_VARIABLEREF, v);
  r->u.varRef.name = thing_name(v);
  r->u.varRef.variable = v;
  return r;
}

static Ast *rw(const Rw *r, Ast *n) {
  if (!n) return NULL;
  if (r->aw) {
    switch (n->tag) {
    case A_TYPEREF:
    case A_VARIABLEREF:
      return n;   /* AutoWrapper.visitTypeRef and visitVariableRef return the node itself */
    case A_TYPEVARIABLE:
    case A_STRUCTTYPE:
    case A_NATIVETYPE:
      return typeref_wrap(n);
    case A_CONSTEXPRTYPEPARAM:
    case A_FUNCPARAMETER:
    case A_VARIABLEDECL:
      return variableref_wrap(n);
    default:
      break;
    }
  }
  switch (n->tag) {
  case A_TYPEREF: {
    if (r->n) {
      /* Substitution.visitTypeRef: a type variable being replaced */
      for (int i = 0; i < r->n; i++) {
        if (r->olds[i] == n->u.typeRef.type) {
          if (n->u.typeRef.typeArguments.n) fatal_unsupported("type arguments on a type variable", n);
          return rw_aw(r->news[i]);
        }
      }
    }
    Ast *t = mk_from(A_TYPEREF, n);
    t->u.typeRef.name = n->u.typeRef.name;
    for (int i = 0; i < n->u.typeRef.typeArguments.n; i++)
      list_push(&t->u.typeRef.typeArguments, rw(r, n->u.typeRef.typeArguments.v[i]));
    t->u.typeRef.type = rw(r, n->u.typeRef.type);
    return t;
  }
  case A_PTRTYPE:
  case A_ARRAYREFTYPE: {
    Ast *t = mk_from(n->tag, n);
    t->u.ref.addressSpace = n->u.ref.addressSpace;
    t->u.ref.elementType = rw(r, n->u.ref.elementType);
    return t;
  }
  case A_ARRAYTYPE: {
    Ast *t = mk_from(A_ARRAYTYPE, n);
    t->u.arrayType.elementType = rw(r, n->u.arrayType.elementType);
    t->u.arrayType.numElements = rw(r, n->u.arrayType.numElements);
    return t;
  }
  case A_VARIABLEREF: {
    if (r->n) {
      for (int i = 0; i < r->n; i++) {
        if (r->olds[i] == n->u.varRef.variable) return rw_aw(r->news[i]);
      }
    }
    Ast *t = mk_from(A_VARIABLEREF, n);
    t->u.varRef.name = n->u.varRef.name;
    t->u.varRef.anon = n->u.varRef.anon;
    t->u.varRef.variable = n->u.varRef.variable;
    return t;
  }
  case A_INTLITERAL:
  case A_UINTLITERAL:
  case A_FLOATLITERAL:
  case A_DOUBLELITERAL: {
    /* Rewriter.visitGenericLiteral: always an IntLiteral with the same value; the preferred type is cloned */
    Ast *t = mk_from(A_INTLITERAL, n);
    /* Rewriter.visitGenericLiteralType: the source's literal type, its committed type visited too (AutoWrapper
     * returns the literal type as it is) */
    if (r->aw) t->type = n->type;
    else if (is_lit_type_tag(n->type->tag)) {
      t->type = lit_type_new(n->type->tag, t);
      if (n->type->type) t->type->type = rw(r, n->type->type);
    } else t->type = rw(r, n->type);   /* a typeref-wrapped literal (withType): an ordinary type */
    double v = (n->tag == A_INTLITERAL || n->tag == A_UINTLITERAL)
                   ? (n->u.intLit.isDbl ? n->u.intLit.dbl : (double)n->u.intLit.value) : n->u.fltLit.value;
    if (v != (double)(long long)v) fatal_unsupported("non-integral literal in a type", n);
    t->u.intLit.value = (long long)v;
    t->u.intLit.preferred = rw_plain(literal_preferred(n));
    return t;
  }
  case A_FUNCPARAMETER: {
    Ast *t = mk_from(A_FUNCPARAMETER, n);
    t->u.named.name = n->u.named.name;
    t->u.named.type = rw(r, n->u.named.type);
    return t;
  }
  case A_PROTOCOLFUNCDECL: {
    /* Rewriter.visitProtocolFuncDecl: a new node; possibleOverloads (live) and protocolDecl are shared */
    Ast *t = mk_from(A_PROTOCOLFUNCDECL, n);
    t->u.func.name = n->u.func.name;
    t->u.func.isCast = n->u.func.isCast;
    t->u.func.shaderType = n->u.func.shaderType;
    t->u.func.isRestricted = 0;
    t->u.func.returnType = rw(r, n->u.func.returnType);
    for (int i = 0; i < n->u.func.typeParameters.n; i++)
      list_push(&t->u.func.typeParameters, rw(r, n->u.func.typeParameters.v[i]));
    for (int i = 0; i < n->u.func.parameters.n; i++)
      list_push(&t->u.func.parameters, rw(r, n->u.func.parameters.v[i]));
    t->u.func.hasOv = n->u.func.hasOv;
    t->u.func.ov = n->u.func.ov;
    t->u.func.protocolDecl = n->u.func.protocolDecl;
    return t;
  }
  case A_TYPEVARIABLE:
  case A_TYPEDEF:
  case A_STRUCTTYPE:
  case A_NATIVETYPE:
  case A_ENUMTYPE:
  case A_CONSTEXPRTYPEPARAM:
  case A_PROTOCOLREF:
  case A_PROTOCOLDECL:
  case A_FUNCDEF:
  case A_NATIVEFUNC:
  case A_INTLITERALTYPE: case A_UINTLITERALTYPE: case A_FLOATLITERALTYPE: case A_DOUBLELITERALTYPE:
  case A_NULLTYPE:
    return n;   /* the Rewriter returns these as they are (AutoWrapper: the literal and null types) */
  case A_BOOLLITERAL:
    return n;
  case A_ENUMLITERAL: {   /* Rewriter.visitEnumLiteral: a new EnumLiteral of the same member */
    Ast *t = mk_from(A_ENUMLITERAL, n);
    t->u.enumLit.member = n->u.enumLit.member;
    return t;
  }
  default:
    fatal_unsupported("Rewriter", n);
  }
  return n;
}

/* TypeParameterRewriter: fresh type variables for a type's parameters. */
static Ast *tp_rewrite(const Ast *tp) {
  if (tp->tag == A_TYPEVARIABLE) {
    Ast *t = mk_from(A_TYPEVARIABLE, tp);
    t->u.typeVariable.name = tp->u.typeVariable.name;
    t->u.typeVariable.protocol = tp->u.typeVariable.protocol;
    return t;
  }
  if (tp->tag == A_CONSTEXPRTYPEPARAM) {
    Ast *t = mk_from(A_CONSTEXPRTYPEPARAM, tp);
    t->u.named.name = tp->u.named.name;
    t->u.named.type = rw_plain(tp->u.named.type);
    return t;
  }
  fatal_unsupported("TypeParameterRewriter", tp);
  return NULL;
}

static void tp_rewrite_list(const AstList *src, AstList *dst) {
  memset(dst, 0, sizeof *dst);
  for (int i = 0; i < src->n; i++) list_push(dst, tp_rewrite(src->v[i]));
}

/* ---- ResolveTypeDefs: TypeDefResolver ---- */
static Ast *TDR_STACK[256];
static int TDR_N;

static void tdr_visit(Ast *n);
static void tdr_child(void *cx, Ast *c) { (void)cx; tdr_visit(c); }

static void tdr_typeref(Ast *n) {
  for (int i = 0; i < TDR_N; i++) {
    if (TDR_STACK[i] == n) name_throw(n, "", "Recursive Type");
  }
  TDR_STACK[TDR_N++] = n;
  for (int i = 0; i < n->u.typeRef.typeArguments.n; i++) tdr_visit(n->u.typeRef.typeArguments.v[i]);
  Ast *type = n->u.typeRef.type;
  if (type && type->tag == A_TYPEDEF) {
    if (n->u.typeRef.typeArguments.n != type->u.typeDef.typeParameters.n)
      fatal_unsupported("argument/parameter mismatch", n);
    if (type->u.typeDef.typeParameters.n) fatal_unsupported("generic type definition", n);
    Ast *newType = rw_plain(type->u.typeDef.type);
    tdr_visit(newType);
    n->u.typeRef.name = NULL;   /* setTypeAndArguments */
    n->u.typeRef.type = newType;
    memset(&n->u.typeRef.typeArguments, 0, sizeof(AstList));
  }
  TDR_N--;
}

static void tdr_visit(Ast *n) {
  if (n->tag == A_TYPEREF) tdr_typeref(n);
  else visit_children(n, tdr_child, NULL);
}

/* ---- RecursiveTypeChecker (only checks) ---- */
static Ast *RC_STACK[256];
static int RC_N;

static void rc_visit(Ast *n);
static void rc_child(void *cx, Ast *c) { (void)cx; rc_visit(c); }

static void rc_visit(Ast *n) {
  switch (n->tag) {
  case A_FUNCDEF:
  case A_NATIVEFUNC:
  case A_PTRTYPE:
  case A_ARRAYREFTYPE:
    return;
  case A_STRUCTTYPE:
    for (int i = 0; i < RC_N; i++)
      if (RC_STACK[i] == n) name_throw(n, "", "Recursive Type");
    RC_STACK[RC_N++] = n;
    visit_children(n, rc_child, NULL);
    RC_N--;
    return;
  case A_TYPEREF:
    visit_children(n, rc_child, NULL);
    if (n->u.typeRef.type) rc_visit(n->u.typeRef.type);
    return;
  default:
    visit_children(n, rc_child, NULL);
    return;
  }
}

/* ---- synthesized natives: each is added to the program and to the global scope ---- */
static void add_synth(Program *p, NameCtx *global, Ast *f) {
  prog_add_raw(p, f);
  nc_add(global, f);
}

static Ast *mk_native(const Ast *src, const char *name, Ast *ret, AstList *tps, AstList *params) {
  Ast *f = mk_from(A_NATIVEFUNC, src);
  f->u.func.name = name;
  f->u.func.isCast = 0;
  f->u.func.shaderType = NULL;
  f->u.func.isRestricted = 0;
  f->u.func.returnType = ret;
  f->u.func.typeParameters = *tps;
  f->u.func.parameters = *params;
  return f;
}

/* FuncParameter(origin, null, type) */
static Ast *mk_param(const Ast *src, Ast *type) {
  Ast *p = mk_from(A_FUNCPARAMETER, src);
  p->u.named.name = NULL;
  p->u.named.type = type;
  return p;
}

static Ast *mk_typeref_named(const Ast *src, const char *name) {
  Ast *t = mk_from(A_TYPEREF, src);
  t->u.typeRef.name = name;
  return t;
}

/* TypeRef.instantiate(struct, the parameters as arguments) */
static Ast *struct_typeref(const Ast *st, const AstList *tps) {
  Ast *t = mk_from(A_TYPEREF, st);
  t->u.typeRef.name = st->u.structType.name;
  for (int i = 0; i < tps->n; i++) list_push(&t->u.typeRef.typeArguments, rw_aw(tps->v[i]));
  t->u.typeRef.type = (Ast *)st;
  return t;
}

static const char *concat3(const char *a, const char *b, const char *c) {
  char buf[600];
  snprintf(buf, sizeof buf, "%s%s%s", a, b, c);
  return arena_strndup(buf, (int)strlen(buf));
}

/* createFieldType: the field's type with the struct's parameters replaced by tps. */
static Ast *field_type(const Ast *st, const Ast *field, const AstList *tps) {
  Rw r = {st->u.structType.typeParameters.v, tps->v, st->u.structType.typeParameters.n, 0};
  return rw(&r, field->u.named.type);
}

/* setupImplementationData: the accessor's struct and field (the instance's type is made in fi_get_unique) */
static Ast *synth_accessor(Ast *st, const char *fname, Ast *f) {
  f->u.func.implStruct = st;
  f->u.func.implField = fname;
  return f;
}

static void synth_struct_accessors(Program *p, NameCtx *global, int count) {
  static const char *spaces[] = {"thread", "threadgroup", "device", "constant"};
  for (int s = 0; s < count; s++) {
    Ast *st = p->statements.v[s];
    if (st->tag != A_STRUCTTYPE) continue;
    for (int f = 0; f < st->u.structType.fields.n; f++) {
      Ast *field = st->u.structType.fields.v[f];
      const char *fname = field->u.named.name;
      AstList tps;
      /* the getter: operator.field */
      tp_rewrite_list(&st->u.structType.typeParameters, &tps);
      Ast *ret = field_type(st, field, &tps);
      AstList params = {0};
      list_push(&params, mk_param(field, struct_typeref(st, &tps)));
      add_synth(p, global, synth_accessor(st, fname, mk_native(field, concat3("operator.", fname, ""), ret, &tps, &params)));
      /* the setter: operator.field= */
      tp_rewrite_list(&st->u.structType.typeParameters, &tps);
      AstList setParams = {0};
      Ast *setRet = struct_typeref(st, &tps);
      list_push(&setParams, mk_param(field, struct_typeref(st, &tps)));
      list_push(&setParams, mk_param(field, field_type(st, field, &tps)));
      add_synth(p, global, synth_accessor(st, fname, mk_native(field, concat3("operator.", fname, "="), setRet, &tps, &setParams)));
      /* the anders: operator&.field, once per address space */
      for (int k = 0; k < 4; k++) {
        tp_rewrite_list(&st->u.structType.typeParameters, &tps);
        Ast *ptrRet = mk_from(A_PTRTYPE, field);
        ptrRet->u.ref.addressSpace = spaces[k];
        ptrRet->u.ref.elementType = field_type(st, field, &tps);
        AstList andParams = {0};
        Ast *ptrParam = mk_from(A_PTRTYPE, field);
        ptrParam->u.ref.addressSpace = spaces[k];
        ptrParam->u.ref.elementType = struct_typeref(st, &tps);
        list_push(&andParams, mk_param(field, ptrParam));
        add_synth(p, global, synth_accessor(st, fname, mk_native(field, concat3("operator&.", fname, ""), ptrRet, &tps, &andParams)));
      }
    }
  }
}

static void synth_enum_functions(Program *p, NameCtx *global, int count) {
  for (int s = 0; s < count; s++) {
    Ast *en = p->statements.v[s];
    if (en->tag != A_ENUMTYPE) continue;
    const char *ename = en->u.enumType.name;
    AstList none = {0}, params;
    /* operator==(E, E) -> bool */
    params = (AstList){0};
    list_push(&params, mk_param(en, mk_typeref_named(en, ename)));
    list_push(&params, mk_param(en, mk_typeref_named(en, ename)));
    add_synth(p, global, mk_native(en, "operator==", mk_typeref_named(en, "bool"), &none, &params));
    /* operator.value(E) -> baseType */
    params = (AstList){0};
    list_push(&params, mk_param(en, mk_typeref_named(en, ename)));
    add_synth(p, global, mk_native(en, "operator.value", rw_plain(en->u.enumType.baseType), &none, &params));
    /* operator cast(E) -> baseType */
    params = (AstList){0};
    list_push(&params, mk_param(en, mk_typeref_named(en, ename)));
    add_synth(p, global, mk_native(en, "operator cast", rw_plain(en->u.enumType.baseType), &none, &params));
    /* operator cast(baseType) -> E */
    params = (AstList){0};
    list_push(&params, mk_param(en, rw_plain(en->u.enumType.baseType)));
    add_synth(p, global, mk_native(en, "operator cast", mk_typeref_named(en, ename), &none, &params));
  }
}

/* resolveNamesInFunctions: the functions by name in the order the program's function map gives
 * (names by first addition, each name's overloads in addition order). */
/* The JS function map's order: each name in first-appearance order, then its functions in order. */
typedef void (*FuncFn)(void *cx, Ast *f);
static void each_func_in_map_order(Program *p, FuncFn fn, void *cx) {
  for (int i = 0; i < p->statements.n; i++) {
    Ast *first = p->statements.v[i];
    if (first->tag != A_FUNCDEF && first->tag != A_NATIVEFUNC) continue;
    const char *name = first->u.func.name;
    int seen = 0;
    for (int j = 0; j < i && !seen; j++) {
      Ast *o = p->statements.v[j];
      if ((o->tag == A_FUNCDEF || o->tag == A_NATIVEFUNC) && strcmp(o->u.func.name, name) == 0) seen = 1;
    }
    if (seen) continue;
    for (int k = i; k < p->statements.n; k++) {
      Ast *f = p->statements.v[k];
      if ((f->tag == A_FUNCDEF || f->tag == A_NATIVEFUNC) && strcmp(f->u.func.name, name) == 0)
        fn(cx, f);
    }
  }
}
static void nf_cb(void *cx, Ast *f) { nr_statement((NameCtx *)cx, f); }
static void names_functions(Program *p, NameCtx *global) { each_func_in_map_order(p, nf_cb, global); }

/* resolveTypeDefsInFunctions */
static void tdr_func_cb(void *cx, Ast *f) { (void)cx; tdr_visit(f); }
static void resolve_typedefs_in_functions(Program *p) { each_func_in_map_order(p, tdr_func_cb, NULL); }

/* ---- flattenProtocolExtends ---- */
static Ast *FL_STACK[256];
static int FL_N;

/* ProtocolDecl.add: sets the signature's protocol and appends it */
static void proto_add(Ast *proto, Ast *sig) {
  sig->u.func.protocolDecl = proto;
  list_push(&proto->u.protocolDecl.signatures, sig);
}

static void fl_flatten(Ast *proto) {
  if (!proto->u.protocolDecl.extends.n) return;
  for (int i = 0; i < FL_N; i++)
    if (FL_STACK[i] == proto) name_throw(proto, "", "Recursive Protocol");
  FL_STACK[FL_N++] = proto;
  for (int i = 0; i < proto->u.protocolDecl.extends.n; i++) {
    Ast *parent = proto->u.protocolDecl.extends.v[i]->u.nameOnly.target;
    fl_flatten(parent);
    Ast *from = parent->u.protocolDecl.typeVariable;
    Ast *to = proto->u.protocolDecl.typeVariable;
    Rw r = {&from, &to, 1, 0};
    for (int j = 0; j < parent->u.protocolDecl.signatures.n; j++)
      proto_add(proto, rw(&r, parent->u.protocolDecl.signatures.v[j]));
  }
  memset(&proto->u.protocolDecl.extends, 0, sizeof(AstList));
  FL_N--;
}

static void flatten_protocol_extends(Program *p) {
  for (int i = 0; i < p->statements.n; i++)
    if (p->statements.v[i]->tag == A_PROTOCOLDECL) fl_flatten(p->statements.v[i]);
}

#endif
