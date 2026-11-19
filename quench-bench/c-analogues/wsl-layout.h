/* A port of StructLayoutBuilder.js: the sizes and field offsets of the struct types a function reaches, and the
 * layout data of a synthesized struct accessor (SynthesizeStructAccessors.js, didLayoutStructsInImplementationData).
 * inline() runs it on each function after LateChecker (_inlineFunction); the layout is a Visitor traversal with
 * the state _offset, run with VTrace.layout set (wsl-visit.h). The traversal is the Visitor's except:
 *   visitReferenceType: empty (the element type is not visited);
 *   visitStructType: a struct with a size is not visited again; otherwise its fields are laid out from offset 0,
 *     its size is the offset after them, and the offset is then advanced by the caller's offset (the odd
 *     `this._offset += oldOffset` of the JS, kept as it is);
 *   visitField: the field's type is visited, then offset = the current offset, which advances by the type's size;
 *   visitTypeRef: the type arguments, then the target type;
 *   visitCallExpression: the arguments, the instance type arguments, the native instance and the result type only.
 * The log of a layout (LSB_LOG) receives one entry per struct as it is laid out, and one per accessor instance.
 * The size of a type is the JS size getter: TypeRef (its target), StructType (its layout), NativeType (the kinds
 * of Intrinsics.js; an unknown native type has none), EnumType (its base type), ArrayType (element size times the
 * literal count), ReferenceType 1; any other type has no size (undefined). */
#ifndef WSL_LAYOUT_H
#define WSL_LAYOUT_H

static int64_t LSB_OFF;        /* StructLayoutBuilder._offset */
static Buf *LSB_LOG;           /* the layout log, or NULL */
static int LSB_NE;             /* entries in LSB_LOG */

static void lsb_entry(void) {
  if (!LSB_LOG) return;
  if (LSB_NE++) buf_str(LSB_LOG, "\n");
}

/* ArrayType.numElementsValue: the value of a literal count */
static int64_t arr_count(const Ast *t) {
  const Ast *ne = t->u.arrayType.numElements;
  if (ne->tag == A_NULLTYPE || ne->tag == A_NULLLITERAL) fatal_unsupported("null array count", t);
  int literal = ne->tag == A_INTLITERAL || ne->tag == A_UINTLITERAL || ne->tag == A_FLOATLITERAL ||
                ne->tag == A_DOUBLELITERAL || is_lit_type_tag(ne->tag);
  if (!literal) chk_error(cat(2, "numElements is not a literal: ", js_tostr(ne)));
  return (int64_t)lit_value(ne);
}

/* type.size: the value, with *ok 0 when the JS getter gives undefined (null). An unknown size inside an array
 * would give NaN in the JS; that is not reachable here and is not modelled. */
static int64_t ty_size(const Ast *t, int *ok) {
  *ok = 1;
  switch (t->tag) {
  case A_TYPEREF:
    if (t->u.typeRef.typeArguments.n) fatal_unsupported("size of a type instantiation", t);
    return ty_size(t->u.typeRef.type, ok);
  case A_STRUCTTYPE:
    *ok = t->hasSize;
    return t->size;
  case A_NATIVETYPE: {
    NtKind k = nt_kind(t);
    if (k == NT_NONE) { *ok = 0; return 0; }
    return nt_size(k);
  }
  case A_ENUMTYPE:
    return ty_size(t->u.enumType.baseType, ok);
  case A_ARRAYTYPE: {
    int eok;
    int64_t es = ty_size(t->u.arrayType.elementType, &eok);
    if (!eok) fatal_unsupported("array of a type without size", t);
    return es * arr_count(t);
  }
  case A_PTRTYPE:
  case A_ARRAYREFTYPE:
    return 1;
  default:
    *ok = 0;
    return 0;
  }
}

/* StructLayoutBuilder.visitStructType, before the fields: the size is kept when there is one; the offset is
 * saved and restarts at 0. Returns the saved offset. */
static int64_t lsb_struct_begin(Ast *n) {
  if (n->u.structType.typeParameters.n) chk_error(cat(2, "Cannot do layout for generic type: ", js_tostr(n)));
  int64_t old = LSB_OFF;
  LSB_OFF = 0;
  return old;
}

/* after the fields: size = the offset, then the saved offset is added back (the JS `this._offset += oldOffset`) */
static void lsb_struct_end(Ast *n, int64_t old) {
  n->size = LSB_OFF;
  n->hasSize = 1;
  LSB_OFF += old;
  if (!LSB_LOG) return;
  lsb_entry();
  buf_fmt(LSB_LOG, "struct %s size=%lld", n->u.structType.name, (long long)n->size);
  for (int i = 0; i < n->u.structType.fields.n; i++) {
    const Ast *f = n->u.structType.fields.v[i];
    buf_fmt(LSB_LOG, "\nfield %s offset=%lld", f->u.named.name, (long long)f->offset);
  }
}

/* StructLayoutBuilder.visitField, after the type: the offset is the current one, then the advance by the size */
static void lsb_field(Ast *n) {
  n->offset = LSB_OFF;
  n->hasOffset = 1;
  int ok;
  int64_t sz = ty_size(n->u.named.type, &ok);
  if (!ok) chk_error(cat(2, "Type does not have size: ", js_tostr(n->u.named.type)));
  LSB_OFF += sz;
}

/* NativeFuncInstance: the accessor's layout data (SynthesizeStructAccessors.js didLayoutStructsInImplementationData).
 * A native without implementation data has none, and nothing is done. */
static void lsb_native_inst(Ast *n) {
  Ast *st = n->u.func.implType;
  if (!st) return;
  if (!st->hasSize) fatal_unsupported("accessor struct without a size", n);
  const Ast *field = NULL;
  for (int i = 0; i < st->u.structType.fields.n && !field; i++)
    if (!strcmp(st->u.structType.fields.v[i]->u.named.name, n->u.func.implField)) field = st->u.structType.fields.v[i];
  if (!field) chk_error("Could not find field");
  int ok;
  ty_size(field->u.named.type, &ok);
  if (!ok) fatal_unsupported("accessor field without a size", n);
  if (!field->hasOffset) fatal_unsupported("accessor field without an offset", n);
  n->u.func.implOffset = field->offset;
  n->u.func.implStructSize = st->size;
  n->u.func.implFieldSize = ty_size(field->u.named.type, &ok);
  n->u.func.implHas = 1;
  if (!LSB_LOG) return;
  lsb_entry();
  buf_fmt(LSB_LOG, "impl %s offset=%lld structSize=%lld fieldSize=%lld", n->u.func.implField,
          (long long)n->u.func.implOffset, (long long)n->u.func.implStructSize, (long long)n->u.func.implFieldSize);
}

#endif
