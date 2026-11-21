/* WSL name resolution: a C port of NameContext.js, NameResolver.js and ResolveNames.js. A NameCtx
 * is a scope (a hash map from names to things, with a delegate for the enclosing scope); the
 * resolver is a function over (scope, node). Each resolved reference is written into the node
 * (TypeRef.type, VariableRef.variable, the overload lists of calls, property accesses and protocol
 * declarations, Return.func, the literal preferred types, and Node.become for enum members and casts).
 * Order of scopes, of insertions and of visits follows the JS exactly. */
#ifndef WSL_NAMES_H
#define WSL_NAMES_H

#include "wsl-prep.h"

enum { K_ANY = 0, K_TYPE, K_VALUE, K_FUNC, K_PROTOCOL };

/* The kind of a thing, as the JS class hierarchy gives it (Type, Value, Func, Protocol). */
static int thing_kind(const Ast *t) {
  switch (t->tag) {
  case A_TYPEDEF: case A_STRUCTTYPE: case A_ENUMTYPE: case A_NATIVETYPE: case A_TYPEVARIABLE:
    return K_TYPE;
  case A_FUNCPARAMETER: case A_VARIABLEDECL: case A_CONSTEXPRTYPEPARAM:
    return K_VALUE;
  case A_FUNCDEF: case A_NATIVEFUNC: case A_PROTOCOLFUNCDECL:
    return K_FUNC;
  case A_PROTOCOLDECL:
    return K_PROTOCOL;
  default:
    return K_ANY;
  }
}

static const char *thing_name(const Ast *t) {
  switch (t->tag) {
  case A_FUNCDEF: case A_NATIVEFUNC: case A_PROTOCOLFUNCDECL: return t->u.func.name;
  case A_TYPEDEF: return t->u.typeDef.name;
  case A_STRUCTTYPE: return t->u.structType.name;
  case A_ENUMTYPE: return t->u.enumType.name;
  case A_NATIVETYPE: return t->u.nativeType.name;
  case A_PROTOCOLDECL: return t->u.protocolDecl.name;
  case A_TYPEVARIABLE: return t->u.typeVariable.name;
  case A_FUNCPARAMETER: case A_CONSTEXPRTYPEPARAM: return t->u.named.name;
  case A_VARIABLEDECL: return t->u.varDecl.name;
  default: return NULL;
  }
}

/* A name's entry: a function name holds every overload (in add order); anything else holds one thing. */
typedef struct NameEntry {
  const char *name;
  Ast *thing;
  int isFunc;
  AstList funcs;
  struct NameEntry *next;
} NameEntry;

typedef struct NameCtx {
  struct NameCtx *delegate;
  NameEntry *buckets[64];
  Ast *current;   /* the top-level statement being resolved (set on the global scope) */
} NameCtx;

static int ent_kind(const NameEntry *e) { return e->isFunc ? K_FUNC : thing_kind(e->thing); }

static NameCtx *nc_new(NameCtx *delegate) {
  NameCtx *c = arena_alloc(sizeof(NameCtx));
  memset(c, 0, sizeof *c);
  c->delegate = delegate;
  return c;
}

static NameEntry *nc_find_local(NameCtx *c, const char *name) {
  for (NameEntry *e = c->buckets[name_hash(name) & 63]; e; e = e->next)
    if (strcmp(e->name, name) == 0) return e;
  return NULL;
}

static void nc_insert_entry(NameCtx *c, NameEntry *e) {
  unsigned b = name_hash(e->name) & 63;
  e->next = c->buckets[b];
  c->buckets[b] = e;
}

/* The source position of a node, as the JS error messages print it (origin:line). */
static const char *CUR_ORIGIN, *CUR_TEXT;
static int CUR_LEN, CUR_LINEOFF, CUR_NATIVE;

static void name_throw(const Ast *t, const char *prefix, const char *name) {
  char buf[600];
  Lexer L = {CUR_TEXT, CUR_LEN, 0, CUR_NATIVE, CUR_ORIGIN, CUR_LINEOFF, NULL};
  snprintf(buf, sizeof buf, "Type error at %s:%d: %s%s", CUR_ORIGIN, lex_line_of(&L, t->origin), prefix, name);
  wsl_throw(0, "WTypeError", buf);
}

/* NameContext.add */
static void nc_add(NameCtx *c, Ast *t) {
  const char *name = thing_name(t);
  if (!name) return;
  NameEntry *e = nc_find_local(c, name);
  if (thing_kind(t) == K_FUNC) {
    if (e) {
      if (!e->isFunc) name_throw(t, "Cannot reuse type name for function: ", name);
      list_push(&e->funcs, t);
      return;
    }
    e = arena_alloc(sizeof(NameEntry));
    memset(e, 0, sizeof *e);
    e->name = name;
    e->isFunc = 1;
    list_push(&e->funcs, t);
    nc_insert_entry(c, e);
    return;
  }
  if (e) name_throw(t, "Duplicate name: ", name);
  e = arena_alloc(sizeof(NameEntry));
  memset(e, 0, sizeof *e);
  e->name = name;
  e->thing = t;
  nc_insert_entry(c, e);
}

/* NameContext.get: a wrong-kind result is null and does not reach the delegate. */
static NameEntry *nc_get(NameCtx *c, int kind, const char *name) {
  NameEntry *r = nc_find_local(c, name);
  if (!r && c->delegate) return nc_get(c->delegate, kind, name);
  if (r && kind != K_ANY && ent_kind(r) != kind) return NULL;
  return r;
}

static Ast *nc_current(NameCtx *c) {
  if (c->current) return c->current;
  if (c->delegate) return nc_current(c->delegate);
  return NULL;
}

/* The global scope of a program: its statements, added in order. */
static NameCtx *program_global_ctx(const Program *p) {
  NameCtx *g = nc_new(NULL);
  for (int i = 0; i < p->statements.n; i++) nc_add(g, p->statements.v[i]);
  return g;
}

/* ---- the NameResolver ---- */
static void nr_visit(NameCtx *c, Ast *n);
static void nr_child(void *cx, Ast *child) { nr_visit((NameCtx *)cx, child); }

static const AstList *type_parameters_of(const Ast *t) {
  switch (t->tag) {
  case A_TYPEDEF: return &t->u.typeDef.typeParameters;
  case A_STRUCTTYPE: return &t->u.structType.typeParameters;
  case A_NATIVETYPE: return &t->u.nativeType.typeParameters;
  default: return NULL;   /* Type's default: no type parameters */
  }
}

/* instanceof Type: every node class that extends Type (TypeRef, the pointer and array types, and the declared types). */
static int is_type_node(const Ast *t) {
  switch (t->tag) {
  case A_TYPEREF: case A_PTRTYPE: case A_ARRAYREFTYPE: case A_ARRAYTYPE: case A_TYPEVARIABLE:
  case A_TYPEDEF: case A_STRUCTTYPE: case A_ENUMTYPE: case A_NATIVETYPE:
    return 1;
  default:
    return 0;
  }
}

/* _visitTypeParametersAndBuildNameContext: each parameter is added, then visited in the outer scope. */
static NameCtx *nr_type_params_ctx(NameCtx *outer, const AstList *tps) {
  NameCtx *c = nc_new(outer);
  for (int i = 0; i < tps->n; i++) {
    nc_add(c, tps->v[i]);
    nr_visit(outer, tps->v[i]);
  }
  return c;
}

/* visitFunc (also used for native functions): return type and parameters in the type-parameter scope. */
static void nr_func(NameCtx *outer, Ast *n) {
  NameCtx *c = nr_type_params_ctx(outer, &n->u.func.typeParameters);
  nr_visit(c, n->u.func.returnType);
  for (int i = 0; i < n->u.func.parameters.n; i++) nr_visit(c, n->u.func.parameters.v[i]);
}

static void nr_funcdef(NameCtx *outer, Ast *n) {
  NameCtx *ctp = nr_type_params_ctx(outer, &n->u.func.typeParameters);
  nr_visit(ctp, n->u.func.returnType);
  NameCtx *cps = nc_new(ctp);
  for (int i = 0; i < n->u.func.parameters.n; i++) {
    Ast *p = n->u.func.parameters.v[i];
    nr_visit(ctp, p);
    nc_add(cps, p);
  }
  nr_visit(cps, n->u.func.body);
}

/* _handlePropertyAccess: the overloads of the get, set and and accessors of a dot or index. */
static void nr_property(NameCtx *c, Ast *n, const char *getName, const char *setName, const char *andName,
                        int *hasGet, int *hasSet, int *hasAnd, AstList **getOv, AstList **setOv, AstList **andOv) {
  NameEntry *g = nc_get(c, K_FUNC, getName);
  NameEntry *s = nc_get(c, K_FUNC, setName);
  NameEntry *a = nc_get(c, K_FUNC, andName);
  *hasGet = g != NULL;
  *hasSet = s != NULL;
  *hasAnd = a != NULL;
  /* the overload lists are live: a later addition to a name shows in every reference to it */
  *getOv = g ? &g->funcs : NULL;
  *setOv = s ? &s->funcs : NULL;
  *andOv = a ? &a->funcs : NULL;
  if (!g && !a) {
    char msg[600];
    snprintf(msg, sizeof msg, "Cannot find either %s or %s", getName, andName);
    name_throw(n, "", msg);
  }
}

static void nr_type_args(NameCtx *c, AstList *args) {
  for (int i = 0; i < args->n; i++) {
    Ast *a = args->v[i];
    if (a->tag != A_TYPEORVARIABLEREF) continue;
    NameEntry *e = nc_get(c, K_ANY, a->u.nameOnly.name);
    if (!e) wsl_throw(0, "TypeError", "Cannot read properties of null (reading 'kind')");
    Ast *r;
    if (ent_kind(e) == K_VALUE) {
      r = ast_new(A_VARIABLEREF, a->origin);
      r->native = a->native;
      r->u.varRef.name = a->u.nameOnly.name;
    } else if (ent_kind(e) == K_TYPE) {
      r = ast_new(A_TYPEREF, a->origin);
      r->native = a->native;
      r->u.typeRef.name = a->u.nameOnly.name;
    } else {
      wsl_throw(0, "WTypeError", "Type argument resolved to wrong kind of thing");
    }
    args->v[i] = r;
  }
}

static void nr_protocol_func_decl(NameCtx *c, Ast *n) {
  nr_func(c, n);
  NameEntry *e = nc_get(c, K_FUNC, n->u.func.name);
  if (!e) {
    char msg[600];
    snprintf(msg, sizeof msg, "Cannot find any functions named %s", n->u.func.name);
    name_throw(n, "", msg);
  }
  n->u.func.hasOv = 1;
  n->u.func.ov = &e->funcs;
}

static void nr_type_ref(NameCtx *c, Ast *n) {
  nr_type_args(c, &n->u.typeRef.typeArguments);
  Ast *type = n->u.typeRef.type;
  if (!type) {
    NameEntry *e = nc_get(c, K_TYPE, n->u.typeRef.name);
    if (!e) {
      char msg[600];
      snprintf(msg, sizeof msg, "Could not find type named %s", n->u.typeRef.name);
      name_throw(n, "", msg);
    }
    type = e->thing;
    n->u.typeRef.type = type;
  }
  const AstList *tps = type_parameters_of(type);
  int ntp = tps ? tps->n : 0;
  AstList *args = &n->u.typeRef.typeArguments;
  if (ntp != args->n) {
    char msg[600];
    snprintf(msg, sizeof msg, "Wrong number of type arguments (passed %d, expected %d)", args->n, ntp);
    name_throw(n, "", msg);
  }
  for (int i = 0; i < ntp; i++) {
    int parameterIsType = tps->v[i]->tag == A_TYPEVARIABLE;
    int argumentIsType = is_type_node(args->v[i]);
    nr_visit(c, args->v[i]);
    if (parameterIsType && !argumentIsType) {
      char msg[600];
      snprintf(msg, sizeof msg, "Expected type, but got value at argument #%d", i);
      name_throw(n, "", msg);
    }
    if (!parameterIsType && argumentIsType) {
      char msg[600];
      snprintf(msg, sizeof msg, "Expected value, but got type at argument #%d", i);
      name_throw(n, "", msg);
    }
  }
}

static void nr_variable_ref(NameCtx *c, Ast *n) {
  if (n->u.varRef.variable) return;
  NameEntry *e = nc_get(c, K_VALUE, n->u.varRef.name);
  if (!e) {
    char msg[600];
    snprintf(msg, sizeof msg, "Could not find variable named %s", n->u.varRef.name);
    name_throw(n, "", msg);
  }
  n->u.varRef.variable = e->thing;
}

static void nr_dot(NameCtx *c, Ast *n) {
  Ast *base = n->u.dot.base;
  if (base->tag == A_VARIABLEREF) {
    NameEntry *e = nc_get(c, K_TYPE, base->u.varRef.name);
    if (e && e->thing->tag == A_ENUMTYPE) {
      Ast *member = NULL;
      for (int i = 0; i < e->thing->u.enumType.members.n; i++) {
        Ast *m = e->thing->u.enumType.members.v[i];
        if (strcmp(m->u.nameOnly.name, n->u.dot.fieldName) == 0) { member = m; break; }
      }
      if (!member) {
        char msg[600];
        snprintf(msg, sizeof msg, "Enum %s does not have a member named %s", e->thing->u.enumType.name, n->u.dot.fieldName);
        name_throw(n, "", msg);
      }
      Ast *lit = ast_new(A_ENUMLITERAL, n->origin);
      lit->native = n->native;
      lit->u.enumLit.member = member;
      ast_become(n, lit);
      return;
    }
  }
  char get[300], set[300], and[300];
  snprintf(get, sizeof get, "operator.%s", n->u.dot.fieldName);
  snprintf(set, sizeof set, "operator.%s=", n->u.dot.fieldName);
  snprintf(and, sizeof and, "operator&.%s", n->u.dot.fieldName);
  nr_property(c, n, arena_strndup(get, (int)strlen(get)), arena_strndup(set, (int)strlen(set)),
              arena_strndup(and, (int)strlen(and)), &n->u.dot.hasGet, &n->u.dot.hasSet, &n->u.dot.hasAnd,
              &n->u.dot.getOv, &n->u.dot.setOv, &n->u.dot.andOv);
  nr_visit(c, n->u.dot.base);
}

static void nr_index(NameCtx *c, Ast *n) {
  nr_property(c, n, "operator[]", "operator[]=", "operator&[]", &n->u.index.hasGet, &n->u.index.hasSet,
              &n->u.index.hasAnd, &n->u.index.getOv, &n->u.index.setOv, &n->u.index.andOv);
  nr_visit(c, n->u.index.base);
  nr_visit(c, n->u.index.index);
}

static void nr_call(NameCtx *c, Ast *n) {
  nr_type_args(c, &n->u.call.typeArguments);
  NameEntry *funcs = nc_get(c, K_FUNC, n->u.call.name);
  if (funcs) {
    n->u.call.hasOv = 1;
    n->u.call.ov = &funcs->funcs;
  } else {
    const char *orig = n->u.call.name;
    NameEntry *t = nc_get(c, K_TYPE, n->u.call.name);
    if (!t) {
      char msg[600];
      snprintf(msg, sizeof msg, "Cannot find any function or type named \"%s\"", n->u.call.name);
      name_throw(n, "", msg);
    }
    /* Node.becomeCast: the return type is a TypeRef with the old name and arguments */
    Ast *rt = ast_new(A_TYPEREF, n->origin);
    rt->native = n->native;
    rt->u.typeRef.name = n->u.call.name;
    rt->u.typeRef.typeArguments = n->u.call.typeArguments;
    rt->u.typeRef.type = t->thing;
    n->u.call.returnType = rt;
    n->u.call.name = "operator cast";
    n->u.call.isCast = 1;
    memset(&n->u.call.typeArguments, 0, sizeof(AstList));
    NameEntry *cast = nc_get(c, K_FUNC, "operator cast");
    if (!cast) {
      char msg[600];
      snprintf(msg, sizeof msg, "Cannot find any operator cast implementations in cast to %s", orig);
      name_throw(n, "", msg);
    }
    n->u.call.hasOv = 1;
    n->u.call.ov = &cast->funcs;
  }
  each_child_list(&n->u.call.typeArguments, nr_child, c);
  each_child_list(&n->u.call.argumentList, nr_child, c);
  if (n->u.call.returnType) nr_visit(c, n->u.call.returnType);
}

/* The NameResolver's visit methods; everything else is the default child walk in the same scope. */
static void nr_visit(NameCtx *c, Ast *n) {
  switch (n->tag) {
  case A_FUNCDEF:
    nr_funcdef(c, n);
    return;
  case A_NATIVEFUNC:
    nr_func(c, n);
    return;
  case A_PROTOCOLFUNCDECL:
    nr_protocol_func_decl(c, n);
    return;
  case A_BLOCK: {
    NameCtx *inner = nc_new(c);
    for (int i = 0; i < n->u.block.statements.n; i++) nr_visit(inner, n->u.block.statements.v[i]);
    return;
  }
  case A_IF:
    nr_visit(c, n->u.ifStmt.conditional);
    nr_visit(nc_new(c), n->u.ifStmt.body);
    if (n->u.ifStmt.elseBody) nr_visit(nc_new(c), n->u.ifStmt.elseBody);
    return;
  case A_WHILE:
    nr_visit(c, n->u.loop.conditional);
    nr_visit(nc_new(c), n->u.loop.body);
    return;
  case A_DOWHILE:
    nr_visit(nc_new(c), n->u.loop.body);
    nr_visit(c, n->u.loop.conditional);
    return;
  case A_FOR: {
    NameCtx *inner = nc_new(c);
    if (n->u.forLoop.initialization) nr_visit(inner, n->u.forLoop.initialization);
    if (n->u.forLoop.condition) nr_visit(inner, n->u.forLoop.condition);
    if (n->u.forLoop.increment) nr_visit(inner, n->u.forLoop.increment);
    nr_visit(inner, n->u.forLoop.body);
    return;
  }
  case A_PROTOCOLDECL: {
    each_child_list(&n->u.protocolDecl.extends, nr_child, c);
    if (!n->u.protocolDecl.typeVariable) {
      Ast *tv = ast_new(A_TYPEVARIABLE, n->origin);
      tv->native = n->native;
      tv->u.typeVariable.name = n->u.protocolDecl.name;
      n->u.protocolDecl.typeVariable = tv;
    }
    NameCtx *inner = nc_new(c);
    nc_add(inner, n->u.protocolDecl.typeVariable);
    each_child_list(&n->u.protocolDecl.signatures, nr_child, inner);
    return;
  }
  case A_PROTOCOLREF: {
    NameEntry *e = nc_get(c, K_PROTOCOL, n->u.nameOnly.name);
    if (!e) {
      char msg[600];
      snprintf(msg, sizeof msg, "Could not find protocol named %s", n->u.nameOnly.name);
      name_throw(n, "", msg);
    }
    n->u.nameOnly.target = e->thing;
    return;
  }
  case A_TYPEDEF: {
    NameCtx *inner = nc_new(c);
    for (int i = 0; i < n->u.typeDef.typeParameters.n; i++) {
      Ast *tp = n->u.typeDef.typeParameters.v[i];
      nr_visit(c, tp);
      nc_add(inner, tp);
    }
    nr_visit(inner, n->u.typeDef.type);
    return;
  }
  case A_STRUCTTYPE: {
    NameCtx *inner = nc_new(c);
    for (int i = 0; i < n->u.structType.typeParameters.n; i++) {
      Ast *tp = n->u.structType.typeParameters.v[i];
      nr_visit(c, tp);
      nc_add(inner, tp);
    }
    each_child_list(&n->u.structType.fields, nr_child, inner);
    return;
  }
  case A_TYPEREF:
    nr_type_ref(c, n);
    return;
  case A_PTRTYPE:
  case A_ARRAYREFTYPE:
    nr_visit(nc_new(c), n->u.ref.elementType);
    return;
  case A_VARIABLEDECL: {
    nc_add(c, n);
    nr_visit(c, n->u.varDecl.type);
    if (n->u.varDecl.initializer) nr_visit(c, n->u.varDecl.initializer);
    return;
  }
  case A_VARIABLEREF:
    nr_variable_ref(c, n);
    return;
  case A_RETURN:
    n->u.ret.func = nc_current(c);
    if (n->u.ret.value) nr_visit(c, n->u.ret.value);
    return;
  case A_DOT:
    nr_dot(c, n);
    return;
  case A_INDEX:
    nr_index(c, n);
    return;
  case A_CALL:
    nr_call(c, n);
    return;
  default:
    visit_children(n, nr_child, c);
    return;
  }
}

/* doStatement: the statement is current on the global scope while it is resolved. */
static void nr_statement(NameCtx *global, Ast *st) {
  global->current = st;
  nr_visit(global, st);
  global->current = NULL;
}

/* resolveNamesInTypes and resolveNamesInProtocols: the statements of each kind, in program order. */
static void names_types(const Program *p, NameCtx *global) {
  for (int i = 0; i < p->statements.n; i++) {
    Ast *st = p->statements.v[i];
    if (st->tag == A_TYPEDEF || st->tag == A_STRUCTTYPE || st->tag == A_ENUMTYPE || st->tag == A_NATIVETYPE)
      nr_statement(global, st);
  }
}

static void names_protocols(const Program *p, NameCtx *global) {
  for (int i = 0; i < p->statements.n; i++) {
    Ast *st = p->statements.v[i];
    if (st->tag == A_PROTOCOLDECL) nr_statement(global, st);
  }
}

#endif
