/* A port of LateChecker.js: the checks run on each function before it is inlined (inline() in Inline.js).
 * LateChecker extends Visitor and overrides two methods: visitReferenceType (a pointer or array reference
 * to a non-primitive element type is an error unless its address space is "thread"; the element type is not
 * visited) and visitFuncDef (the shader signature check, then the base visit). The traversal itself is the
 * Visitor traversal of wsl-visit.h, run with VTrace.late set, so the order of the checks is the order of
 * the visits. The JS getters are read as truthy tests: isPrimitive is undefined (falsy) for every node class
 * that does not define it (see late_is_primitive). */
#ifndef WSL_LATE_H
#define WSL_LATE_H

/* node.isPrimitive, as a truthy value (1 or 0). A TypeRef with type arguments throws (its getter), as does a
 * missing type; both are unreachable for an instance, which has no uninstantiated types. */
static int late_is_primitive(const Ast *t) {
  if (!t) {
    fprintf(stderr, "isPrimitive of a missing type\n");
    exit(9);
  }
  switch (t->tag) {
  case A_TYPEREF:
    if (t->u.typeRef.typeArguments.n) chk_error(cat(2, "Cannot determine if an uninstantiated type is primitive: ", js_tostr(t)));
    return late_is_primitive(t->u.typeRef.type);
  case A_STRUCTTYPE: {   /* StructType.isPrimitive: result &= field.type.isPrimitive over every field (no short circuit) */
    int r = 1;
    for (int i = 0; i < t->u.structType.fields.n; i++) r &= late_is_primitive(t->u.structType.fields.v[i]->u.named.type);
    return r;
  }
  case A_ENUMTYPE: return 1;
  case A_NATIVETYPE: return nt_is_primitive(nt_kind(t));   /* the flag Intrinsics.js sets */
  case A_ARRAYTYPE: return late_is_primitive(t->u.arrayType.elementType);
  case A_NULLTYPE:
  case A_INTLITERALTYPE: case A_UINTLITERALTYPE: case A_FLOATLITERALTYPE: case A_DOUBLELITERALTYPE:
    return 1;
  default:
    return 0;   /* PtrType, ArrayRefType, TypeVariable, ProtocolRef (its ProtocolDecl has no getter), ...: undefined */
  }
}

/* LateChecker.visitReferenceType */
static void late_reference(const Ast *n) {
  if (!strcmp(n->u.ref.addressSpace, "thread")) return;
  if (!late_is_primitive(n->u.ref.elementType))
    wtype_error(origin_string(n), cat(2, "Illegal pointer to non-primitive type: ", js_tostr(n)));
}

/* LateChecker.assertPrimitive(type), with the function's origin */
static void late_assert_primitive(const Ast *func, const Ast *type) {
  if (!late_is_primitive(type))
    wtype_error(origin_string(func), cat(2, "Shader signature cannot include non-primitive type: ", js_tostr(type)));
}

/* LateChecker._checkShaderType */
static void late_shader(Ast *n) {
  const char *o = origin_string(n);
  Ast *rt = n->u.func.returnType;
  late_assert_primitive(n, rt);
  if (ut_unify_node(rt)->tag != A_STRUCTTYPE)
    wtype_error(o, cat(3, "Vertex shader ", n->u.func.name, " must return a struct."));
  const char *st = n->u.func.shaderType;
  if (!strcmp(st, "vertex")) {
    for (int i = 0; i < n->u.func.parameters.n; i++) {
      Ast *p = n->u.func.parameters.v[i];
      if (ut_unify_node(p->u.named.type)->tag == A_STRUCTTYPE) late_assert_primitive(n, p->u.named.type);
      else if (ut_unify_node(p->u.named.type)->tag != A_ARRAYREFTYPE)
        wtype_error(o, cat(4, n->u.func.name, " accepts a parameter ", p->u.named.name, " which isn't a struct and isn't an ArrayRef."));
    }
  } else if (!strcmp(st, "fragment")) {
    for (int i = 0; i < n->u.func.parameters.n; i++) {
      Ast *p = n->u.func.parameters.v[i];
      if (!strcmp(p->u.named.name, "stageIn")) {
        if (ut_unify_node(p->u.named.type)->tag != A_STRUCTTYPE)
          wtype_error(o, cat(3, "Fragment entry points' stageIn parameter (of ", n->u.func.name, ") must be a struct type."));
        late_assert_primitive(n, p->u.named.type);
      } else if (ut_unify_node(p->u.named.type)->tag != A_ARRAYREFTYPE)
        wtype_error(o, cat(3, "Fragment entry point's ", p->u.named.name, " parameter is not an array reference."));
    }
  } else
    chk_error(cat(2, "Bad shader type: ", st));
}

#endif
