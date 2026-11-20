/* WSL AST: one tag per class that WSL/Parse.js constructs, as a tagged union, plus the canonical
 * dump used to validate the C parser against the row's own Parse.js (tools/wsl-ast-dump.cjs).
 * Lists and nodes live in a bump arena (arena_reset between inputs). */
#ifndef WSL_AST_H
#define WSL_AST_H

#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef enum {
  A_FUNCDEF, A_NATIVEFUNC, A_PROTOCOLFUNCDECL,
  A_TYPEREF, A_PTRTYPE, A_ARRAYREFTYPE, A_ARRAYTYPE, A_TYPEVARIABLE,
  A_CONSTEXPRTYPEPARAM, A_TYPEORVARIABLEREF, A_PROTOCOLREF, A_TYPEDEF,
  A_STRUCTTYPE, A_FIELD, A_ENUMTYPE, A_ENUMMEMBER, A_PROTOCOLDECL, A_NATIVETYPE,
  A_FUNCPARAMETER, A_VARIABLEDECL, A_BLOCK, A_SWITCHSTATEMENT, A_SWITCHCASE,
  A_RETURN, A_IF, A_WHILE, A_DOWHILE, A_FOR, A_BREAK, A_CONTINUE, A_TRAP,
  A_COMMA, A_NULLLITERAL, A_BOOLLITERAL, A_VARIABLEREF, A_CALL, A_DOT, A_INDEX,
  A_DEREF, A_MAKEPTR, A_MAKEARRAYREF, A_LOGICALNOT, A_LOGICAL, A_ASSIGN, A_RMW,
  A_INTLITERAL, A_UINTLITERAL, A_FLOATLITERAL, A_DOUBLELITERAL,
  A_IDENTITY, A_ENUMLITERAL, A_ANONVAR,
  /* literal types (GenericLiteralType and NullType): the type of a literal expression, see lit_type_new */
  A_INTLITERALTYPE, A_UINTLITERALTYPE, A_FLOATLITERALTYPE, A_DOUBLELITERALTYPE, A_NULLTYPE,
  /* ConvertPtrToArrayRefExpression (made by the checker for @ on a pointer; becomes the expression) */
  A_CONVERTPTR,
  /* instances (FuncInstantiator.getUnique, wsl-inline.h): NativeFuncInstance; FunctionLikeBlock (the inliner) */
  A_NATIVEFUNCINST, A_FUNCTIONLIKEBLOCK
} AstTag;

typedef struct Ast Ast;
typedef struct EPtrV EPtrV;
typedef struct { Ast **v; int n, cap; } AstList;

/* The checker's results for a property access (PropertyAccessExpression fields, wsl-check.h). lv is
 * isLValue (1 or 0). The overload calls and result types are NULL when unresolved; errGet, errAnd and
 * errSet are the messages of the failed attempts. */
typedef struct {
  Ast *baseType, *resGet, *resAnd, *callGet, *callAnd, *callSet;
  const char *errGet, *errAnd, *errSet;
  int lv;
  const char *lvReason, *addrSpace;
} PropChk;

struct Ast {
  AstTag tag;
  int origin;                 /* index of the origin token in the source, -1 if none */
  int native;                 /* 1 if the origin kind is "native" (the standard library), else 0 */
  Ast *type;                  /* Value.type of a literal expression: its literal type node (NULL otherwise) */
  int64_t size; int hasSize;      /* StructType.size, set by the layout (wsl-layout.h); hasSize 0 is undefined */
  int64_t offset; int hasOffset;  /* Field.offset, set by the layout */
  EPtrV *ePtr;                    /* ePtr (wsl-ebuf.h): a parameter, variable, anonymous variable or box; FuncDef and CallExpression use ePtrAux */
  EPtrV *ePtrAux;                 /* returnEPtr (FuncDef) or resultEPtr (CallExpression) */
  union {
    /* FuncDef, NativeFunc, ProtocolFuncDecl */
    struct { const char *name; int isCast; const char *shaderType; int isRestricted;
             Ast *returnType; AstList typeParameters; AstList parameters; Ast *body;
             int hasOv; AstList *ov; Ast *protocolDecl;   /* ov: possibleOverloads (ProtocolFuncDecl), live; protocolDecl: ProtocolFuncDecl.protocolDecl */
             Ast *orig; Ast *uninstRet; int inlined;   /* orig: the generic function of an instance; uninstRet: uninstantiatedReturnType; inlined: Func.inlined */
             Ast *implStruct; const char *implField;   /* a synthesized struct accessor (SynthesizeStructAccessors): its struct and field */
             Ast *implType;   /* implType: an instance's implementationData.type (the struct instance of its type arguments) */
             int implHas; int64_t implOffset, implStructSize, implFieldSize; } func;   /* implOffset..: the accessor's layout data (didLayoutStructsInImplementationData) */
    /* TypeRef */
    struct { const char *name; AstList typeArguments; Ast *type; } typeRef;   /* type: resolved target */
    /* PtrType, ArrayRefType */
    struct { const char *addressSpace; Ast *elementType; } ref;
    /* ArrayType */
    struct { Ast *elementType; Ast *numElements; } arrayType;
    /* TypeVariable */
    struct { const char *name; Ast *protocol; } typeVariable;
    /* ConstexprTypeParameter, Field, FuncParameter */
    struct { const char *name; Ast *type; } named;
    /* TypeOrVariableRef, ProtocolRef, EnumMember (value in value) */
    struct { const char *name; Ast *value; Ast *target; } nameOnly;   /* target: ProtocolRef.protocolDecl */
    /* TypeDef */
    struct { const char *name; AstList typeParameters; Ast *type; } typeDef;
    /* StructType */
    struct { const char *name; AstList typeParameters; AstList fields; } structType;
    /* EnumType */
    struct { const char *name; Ast *baseType; AstList members; } enumType;
    /* ProtocolDecl */
    struct { const char *name; AstList extends; AstList signatures; Ast *typeVariable; } protocolDecl;
    /* NativeType */
    struct { const char *name; AstList typeParameters; } nativeType;
    /* VariableDecl */
    struct { const char *name; Ast *type; Ast *initializer; } varDecl;
    /* Block */
    struct { AstList statements; } block;
    /* SwitchStatement */
    struct { Ast *value; AstList cases; } switchStmt;
    /* SwitchCase */
    struct { Ast *value; Ast *body; } switchCase;
    /* Return */
    struct { Ast *value; Ast *func; } ret;   /* func: the enclosing top-level statement */
    /* IfStatement */
    struct { Ast *conditional; Ast *body; Ast *elseBody; } ifStmt;
    /* WhileLoop, DoWhileLoop (conditional and body are both set; dump order is per tag) */
    struct { Ast *conditional; Ast *body; } loop;
    /* ForLoop */
    struct { Ast *initialization; Ast *condition; Ast *increment; Ast *body; } forLoop;
    /* CommaExpression */
    struct { AstList list; } comma;
    /* BoolLiteral */
    struct { int value; } boolLit;
    /* VariableRef; anon: 0 named, 1 old value of a ReadModifyWrite, 2 new value */
    struct { const char *name; int anon; Ast *variable; } varRef;
    /* CallExpression; func, resultType, argTypes, actualTA: the checker's results (wsl-check.h) */
    struct { const char *name; AstList typeArguments; AstList argumentList;
             int hasOv; AstList *ov; int isCast; Ast *returnType;
             Ast *func; Ast *resultType; AstList argTypes; AstList actualTA;
             Ast *nfi; AstList instTA; } call;   /* nfi: nativeFuncInstance; instTA: instantiatedActualTypeArguments */
    /* DotExpression, IndexExpression: the checker's results are in pc (PropChk, wsl-check.h) */
    struct { const char *fieldName; Ast *base;
             int hasGet, hasSet, hasAnd; AstList *getOv, *setOv, *andOv; PropChk pc; } dot;
    struct { Ast *base; Ast *index;
             int hasGet, hasSet, hasAnd; AstList *getOv, *setOv, *andOv; PropChk pc; } index;
    /* DereferenceExpression; addrSpace is set by the checker */
    struct { Ast *ptr; const char *addrSpace; } deref;
    /* MakePtrExpression, MakeArrayRefExpression (numElements: set by the checker), ConvertPtrToArrayRefExpression */
    struct { Ast *lValue; Ast *numElements; } lvalueRef;
    /* LogicalNot */
    struct { Ast *operand; } notExpr;
    /* LogicalExpression */
    struct { const char *text; Ast *left; Ast *right; } logical;
    /* Assignment */
    struct { Ast *lhs; Ast *rhs; } assign;
    /* ReadModifyWriteExpression: oldVar and newVar are the anonymous variables its anon refs share */
    struct { Ast *lValue; Ast *newValueExp; Ast *resultExp; Ast *oldVar, *newVar; } rmw;
    /* IdentityExpression (a rewritten node): target */
    struct { Ast *target; } identity;
    /* EnumLiteral */
    struct { Ast *member; } enumLit;
    /* IntLiteral, UintLiteral. isDbl: the JS value is a double (a FloatLiteral or DoubleLiteral copied as an
     * IntLiteral by Rewriter.visitGenericLiteral, a FIXME there); then dbl holds it and value is unused. */
    struct { long long value; double dbl; int isDbl; Ast *preferred; } intLit;   /* preferred: GenericLiteralType.preferredType */
    /* FloatLiteral, DoubleLiteral: value is the double (float literals are rounded to float) */
    struct { double value; Ast *preferred; } fltLit;
    /* FunctionLikeBlock (the inliner's replacement of a call; ePtrAux is its returnEPtr) */
    struct { Ast *returnType; AstList argumentList; AstList parameters; Ast *body; } fblock;
    /* literal types: the literal expression whose type this is (its value and preferredType are read from it) */
    struct { Ast *owner; } litType;
  } u;
};

/* ---- arena ---- */
typedef struct { char *base; size_t used, cap; } Arena;
static Arena AST_ARENA;
static const char *js_num(double v);   /* wsl-strings.h: Number::toString */
static void *arena_alloc(size_t n) {
  n = (n + 15) & ~(size_t)15;
  if (!AST_ARENA.base || AST_ARENA.used + n > AST_ARENA.cap) {
    size_t cap = n > (512u << 20) ? n : (512u << 20);
    AST_ARENA.base = malloc(cap);   /* earlier chunks are abandoned; the arena is reset per input */
    if (!AST_ARENA.base) { fprintf(stderr, "out of memory\n"); exit(4); }
    AST_ARENA.used = 0;
    AST_ARENA.cap = cap;
  }
  void *p = AST_ARENA.base + AST_ARENA.used;
  AST_ARENA.used += n;
  return p;
}
static void arena_reset(void) { AST_ARENA.used = 0; }
/* The arena keeps the standard library below a mark; each input resets to the mark. */
static size_t arena_mark(void) { return AST_ARENA.used; }
static void arena_reset_to(size_t m) { AST_ARENA.used = m; }
static char *arena_strndup(const char *s, int n) {
  char *p = arena_alloc((size_t)n + 1);
  memcpy(p, s, n);
  p[n] = 0;
  return p;
}

static void list_push(AstList *l, Ast *n) {
  if (l->n == l->cap) {
    int cap = l->cap ? l->cap * 2 : 4;
    Ast **v = arena_alloc(sizeof(Ast *) * cap);
    if (l->n) memcpy(v, l->v, sizeof(Ast *) * l->n);
    l->v = v;
    l->cap = cap;
  }
  l->v[l->n++] = n;
}

/* The origin kind of the nodes being created: set by parse_program for the text it parses. */
static int AST_ORIGIN_NATIVE = 0;
static Ast *ast_new(AstTag tag, int origin) {
  Ast *a = arena_alloc(sizeof(Ast));
  memset(a, 0, sizeof *a);
  a->tag = tag;
  a->origin = origin;
  a->native = AST_ORIGIN_NATIVE;
  return a;
}

/* A literal expression's literal type (GenericLiteralType / NullType). The JS constructor makes it with
 * the literal; here the literal makes it with ast_new. Its value and preferredType are the owner's. */
/* the kinds lit_type_new makes: the four GenericLiteralTypes and NullType */
static int is_lit_type_tag(AstTag t) {
  return t == A_INTLITERALTYPE || t == A_UINTLITERALTYPE || t == A_FLOATLITERALTYPE || t == A_DOUBLELITERALTYPE || t == A_NULLTYPE;
}
static Ast *lit_type_new(AstTag kind, Ast *owner) {
  if (!is_lit_type_tag(kind)) { fprintf(stderr, "lit_type_new: not a literal type\n"); abort(); }
  Ast *t = ast_new(kind, owner->origin);
  t->native = owner->native;
  t->u.litType.owner = owner;
  return t;
}

/* A literal expression of tag `tag` with its type (NullLiteral or one of the four GenericLiterals). */
static Ast *lit_expr_new(AstTag tag, int origin) {
  Ast *a = ast_new(tag, origin);
  AstTag kind = tag == A_NULLLITERAL ? A_NULLTYPE
              : tag == A_INTLITERAL ? A_INTLITERALTYPE
              : tag == A_UINTLITERAL ? A_UINTLITERALTYPE
              : tag == A_FLOATLITERAL ? A_FLOATLITERALTYPE : A_DOUBLELITERALTYPE;
  a->type = lit_type_new(kind, a);
  return a;
}

/* ---- canonical dump (format: tools/wsl-ast-dump.cjs) ---- */
typedef struct { char *s; size_t n, cap; } Buf;
static void buf_put(Buf *b, const char *s, size_t n) {
  if (b->n + n + 1 > b->cap) {
    b->cap = (b->n + n + 1) * 2 + 1024;
    b->s = realloc(b->s, b->cap);
  }
  memcpy(b->s + b->n, s, n);
  b->n += n;
  b->s[b->n] = 0;
}
static void buf_str(Buf *b, const char *s) { buf_put(b, s, strlen(s)); }
static void buf_fmt(Buf *b, const char *fmt, ...) __attribute__((format(printf, 2, 3)));
static void buf_fmt(Buf *b, const char *fmt, ...) {
  char tmp[512];
  va_list ap;
  va_start(ap, fmt);
  int k = vsnprintf(tmp, sizeof tmp, fmt, ap);
  va_end(ap);
  buf_put(b, tmp, (size_t)k);
}
/* JSON.stringify of a plain ASCII name (names never contain quotes or control characters) */
static void buf_json(Buf *b, const char *s) {
  buf_str(b, "\"");
  buf_str(b, s);
  buf_str(b, "\"");
}
static void buf_scalar_str(Buf *b, const char *label, const char *s) {
  buf_fmt(b, " %s=", label);
  if (s) buf_json(b, s); else buf_str(b, "-");
}
static void buf_scalar_bool(Buf *b, const char *label, int v) { buf_fmt(b, " %s=%s", label, v ? "true" : "false"); }
static void buf_scalar_bits(Buf *b, const char *label, double v) {
  uint64_t bits;
  memcpy(&bits, &v, 8);
  buf_fmt(b, " %s=\"%016llx\"", label, (unsigned long long)bits);
}
static void buf_indent(Buf *b, int depth) {
  for (int i = 0; i < depth; i++) buf_str(b, " ");
}

/* --stage 2 and later: the references name resolution sets, after each node's own scalars and
 * children. A target is Class@n<origin> for a standard-library node, Class@u<origin> otherwise. */
static int REFS_ON = 0;
static const char *ast_class_name(const Ast *n);
static void buf_ref_target(Buf *b, const char *label, const Ast *t) {
  buf_fmt(b, " %s=", label);
  if (!t) { buf_str(b, "\"-\""); return; }
  buf_fmt(b, "\"%s@%c%d\"", ast_class_name(t), t->native ? 'n' : 'u', t->origin);
}
static void buf_ref_list(Buf *b, const char *label, const AstList *l, int has) {
  buf_fmt(b, " %s=", label);
  if (!has || !l) { buf_str(b, "\"-\""); return; }
  buf_str(b, "\"[");
  for (int i = 0; i < l->n; i++) {
    if (i) buf_str(b, ",");
    buf_fmt(b, "%s@%c%d", ast_class_name(l->v[i]), l->v[i]->native ? 'n' : 'u', l->v[i]->origin);
  }
  buf_str(b, "]\"");
}
static const Ast *pref_type(const Ast *p) { return p ? p->u.typeRef.type : NULL; }
static int EBUF_DUMP = 0;   /* --ebuf: the EPtrs of the nodes are printed after their scalars */
static const char *eptr_str(const EPtrV *p);
/* End of a node's own scalars: the reference scalars (if any), then the line break. */
static void end_scalars(Buf *b, const Ast *n) {
  if (REFS_ON) {
    switch (n->tag) {
    case A_TYPEREF: buf_ref_target(b, "type", n->u.typeRef.type); break;
    case A_VARIABLEREF: buf_ref_target(b, "variable", n->u.varRef.variable); break;
    case A_CALL: buf_ref_list(b, "overloads", n->u.call.ov, n->u.call.hasOv); break;
    case A_DOT:
      buf_ref_list(b, "getOverloads", n->u.dot.getOv, n->u.dot.hasGet);
      buf_ref_list(b, "setOverloads", n->u.dot.setOv, n->u.dot.hasSet);
      buf_ref_list(b, "andOverloads", n->u.dot.andOv, n->u.dot.hasAnd);
      break;
    case A_INDEX:
      buf_ref_list(b, "getOverloads", n->u.index.getOv, n->u.index.hasGet);
      buf_ref_list(b, "setOverloads", n->u.index.setOv, n->u.index.hasSet);
      buf_ref_list(b, "andOverloads", n->u.index.andOv, n->u.index.hasAnd);
      break;
    case A_PROTOCOLREF: buf_ref_target(b, "protocolDecl", n->u.nameOnly.target); break;
    case A_PROTOCOLFUNCDECL: buf_ref_list(b, "overloads", n->u.func.ov, n->u.func.hasOv); break;
    case A_RETURN: buf_ref_target(b, "func", n->u.ret.func); break;
    case A_ENUMLITERAL: buf_ref_target(b, "member", n->u.enumLit.member); break;
    case A_INTLITERAL:
    case A_UINTLITERAL: buf_ref_target(b, "preferred", pref_type(n->u.intLit.preferred)); break;
    case A_FLOATLITERAL:
    case A_DOUBLELITERAL: buf_ref_target(b, "preferred", pref_type(n->u.fltLit.preferred)); break;
    default: break;
    }
  }
  if (EBUF_DUMP) {
    if (n->ePtr) buf_scalar_str(b, "ePtr", eptr_str(n->ePtr));
    if (n->ePtrAux) buf_scalar_str(b, n->tag == A_CALL ? "resultEPtr" : "returnEPtr", eptr_str(n->ePtrAux));
  }
  buf_str(b, "\n");
}

static int DUMP_NO_ROLES = 0;   /* --props: no old/new role on a reference (the JS dump finds the role only in a read-modify-write it still holds) */
static void dump_node(Buf *b, const Ast *n, int depth);
static void dump_child(Buf *b, const Ast *n, int depth) {
  if (!n) { buf_indent(b, depth); buf_str(b, "-\n"); return; }
  dump_node(b, n, depth);
}
static void dump_list(Buf *b, const AstList *l, int depth) {
  buf_indent(b, depth);
  buf_fmt(b, "list %d\n", l->n);
  for (int i = 0; i < l->n; i++) dump_child(b, l->v[i], depth + 1);
}

static const char *ast_class_name(const Ast *n) {
  switch (n->tag) {
  case A_FUNCDEF: return "FuncDef";
  case A_NATIVEFUNC: return "NativeFunc";
  case A_PROTOCOLFUNCDECL: return "ProtocolFuncDecl";
  case A_TYPEREF: return "TypeRef";
  case A_PTRTYPE: return "PtrType";
  case A_ARRAYREFTYPE: return "ArrayRefType";
  case A_ARRAYTYPE: return "ArrayType";
  case A_TYPEVARIABLE: return "TypeVariable";
  case A_CONSTEXPRTYPEPARAM: return "ConstexprTypeParameter";
  case A_TYPEORVARIABLEREF: return "TypeOrVariableRef";
  case A_PROTOCOLREF: return "ProtocolRef";
  case A_TYPEDEF: return "TypeDef";
  case A_STRUCTTYPE: return "StructType";
  case A_FIELD: return "Field";
  case A_ENUMTYPE: return "EnumType";
  case A_ENUMMEMBER: return "EnumMember";
  case A_PROTOCOLDECL: return "ProtocolDecl";
  case A_NATIVETYPE: return "NativeType";
  case A_FUNCPARAMETER: return "FuncParameter";
  case A_VARIABLEDECL: return "VariableDecl";
  case A_BLOCK: return "Block";
  case A_SWITCHSTATEMENT: return "SwitchStatement";
  case A_SWITCHCASE: return "SwitchCase";
  case A_RETURN: return "Return";
  case A_IF: return "IfStatement";
  case A_WHILE: return "WhileLoop";
  case A_DOWHILE: return "DoWhileLoop";
  case A_FOR: return "ForLoop";
  case A_BREAK: return "Break";
  case A_CONTINUE: return "Continue";
  case A_TRAP: return "TrapStatement";
  case A_COMMA: return "CommaExpression";
  case A_NULLLITERAL: return "NullLiteral";
  case A_BOOLLITERAL: return "BoolLiteral";
  case A_VARIABLEREF: return "VariableRef";
  case A_CALL: return "CallExpression";
  case A_DOT: return "DotExpression";
  case A_INDEX: return "IndexExpression";
  case A_DEREF: return "DereferenceExpression";
  case A_MAKEPTR: return "MakePtrExpression";
  case A_MAKEARRAYREF: return "MakeArrayRefExpression";
  case A_LOGICALNOT: return "LogicalNot";
  case A_LOGICAL: return "LogicalExpression";
  case A_ASSIGN: return "Assignment";
  case A_RMW: return "ReadModifyWriteExpression";
  case A_INTLITERAL: return "IntLiteral";
  case A_UINTLITERAL: return "UintLiteral";
  case A_FLOATLITERAL: return "FloatLiteral";
  case A_DOUBLELITERAL: return "DoubleLiteral";
  case A_IDENTITY: return "IdentityExpression";
  case A_ENUMLITERAL: return "EnumLiteral";
  case A_ANONVAR: return "AnonymousVariable";
  case A_CONVERTPTR: return "ConvertPtrToArrayRefExpression";
  case A_NATIVEFUNCINST: return "NativeFuncInstance";
  case A_FUNCTIONLIKEBLOCK: return "FunctionLikeBlock";
  case A_INTLITERALTYPE: return "IntLiteralType";
  case A_UINTLITERALTYPE: return "UintLiteralType";
  case A_FLOATLITERALTYPE: return "FloatLiteralType";
  case A_DOUBLELITERALTYPE: return "DoubleLiteralType";
  case A_NULLTYPE: return "NullType";
  }
  return "?";
}

static void dump_node(Buf *b, const Ast *n, int depth) {
  buf_indent(b, depth);
  buf_str(b, ast_class_name(n));
  /* Each case lists scalars then children, in the order of tools/wsl-ast-dump.cjs. */
  switch (n->tag) {
  case A_FUNCDEF:
  case A_NATIVEFUNC:
  case A_PROTOCOLFUNCDECL:
  case A_NATIVEFUNCINST:
    buf_scalar_str(b, "name", n->u.func.name);
    buf_scalar_bool(b, "isCast", n->u.func.isCast);
    buf_scalar_str(b, "shaderType", n->u.func.shaderType);
    if (n->tag == A_FUNCDEF || n->tag == A_NATIVEFUNC) buf_scalar_bool(b, "isRestricted", n->u.func.isRestricted);
    end_scalars(b, n);
    dump_child(b, n->u.func.returnType, depth + 1);
    dump_list(b, &n->u.func.typeParameters, depth + 1);
    dump_list(b, &n->u.func.parameters, depth + 1);
    if (n->tag == A_FUNCDEF) dump_child(b, n->u.func.body, depth + 1);
    return;
  case A_TYPEREF:
    buf_scalar_str(b, "name", n->u.typeRef.name);
    end_scalars(b, n);
    dump_list(b, &n->u.typeRef.typeArguments, depth + 1);
    return;
  case A_PTRTYPE:
  case A_ARRAYREFTYPE:
    buf_scalar_str(b, "addressSpace", n->u.ref.addressSpace);
    end_scalars(b, n);
    dump_child(b, n->u.ref.elementType, depth + 1);
    return;
  case A_ARRAYTYPE:
    end_scalars(b, n);
    dump_child(b, n->u.arrayType.elementType, depth + 1);
    dump_child(b, n->u.arrayType.numElements, depth + 1);
    return;
  case A_TYPEVARIABLE:
    buf_scalar_str(b, "name", n->u.typeVariable.name);
    end_scalars(b, n);
    dump_child(b, n->u.typeVariable.protocol, depth + 1);
    return;
  case A_CONSTEXPRTYPEPARAM:
  case A_FIELD:
  case A_FUNCPARAMETER:
    buf_scalar_str(b, "name", n->u.named.name);
    end_scalars(b, n);
    dump_child(b, n->u.named.type, depth + 1);
    return;
  case A_TYPEORVARIABLEREF:
  case A_PROTOCOLREF:
    buf_scalar_str(b, "name", n->u.nameOnly.name);
    end_scalars(b, n);
    return;
  case A_TYPEDEF:
    buf_scalar_str(b, "name", n->u.typeDef.name);
    end_scalars(b, n);
    dump_list(b, &n->u.typeDef.typeParameters, depth + 1);
    dump_child(b, n->u.typeDef.type, depth + 1);
    return;
  case A_STRUCTTYPE:
    buf_scalar_str(b, "name", n->u.structType.name);
    end_scalars(b, n);
    dump_list(b, &n->u.structType.typeParameters, depth + 1);
    dump_list(b, &n->u.structType.fields, depth + 1);
    return;
  case A_ENUMTYPE:
    buf_scalar_str(b, "name", n->u.enumType.name);
    end_scalars(b, n);
    dump_child(b, n->u.enumType.baseType, depth + 1);
    dump_list(b, &n->u.enumType.members, depth + 1);
    return;
  case A_ENUMMEMBER:
    buf_scalar_str(b, "name", n->u.nameOnly.name);
    end_scalars(b, n);
    dump_child(b, n->u.nameOnly.value, depth + 1);
    return;
  case A_PROTOCOLDECL:
    buf_scalar_str(b, "name", n->u.protocolDecl.name);
    end_scalars(b, n);
    dump_list(b, &n->u.protocolDecl.extends, depth + 1);
    dump_list(b, &n->u.protocolDecl.signatures, depth + 1);
    return;
  case A_NATIVETYPE:
    buf_scalar_str(b, "name", n->u.nativeType.name);
    end_scalars(b, n);
    dump_list(b, &n->u.nativeType.typeParameters, depth + 1);
    return;
  case A_VARIABLEDECL:
    buf_scalar_str(b, "name", n->u.varDecl.name);
    end_scalars(b, n);
    dump_child(b, n->u.varDecl.type, depth + 1);
    dump_child(b, n->u.varDecl.initializer, depth + 1);
    return;
  case A_BLOCK:
    end_scalars(b, n);
    dump_list(b, &n->u.block.statements, depth + 1);
    return;
  case A_SWITCHSTATEMENT:
    end_scalars(b, n);
    dump_child(b, n->u.switchStmt.value, depth + 1);
    dump_list(b, &n->u.switchStmt.cases, depth + 1);
    return;
  case A_SWITCHCASE:
    end_scalars(b, n);
    dump_child(b, n->u.switchCase.value, depth + 1);
    dump_child(b, n->u.switchCase.body, depth + 1);
    return;
  case A_RETURN:
    end_scalars(b, n);
    dump_child(b, n->u.ret.value, depth + 1);
    return;
  case A_IF:
    end_scalars(b, n);
    dump_child(b, n->u.ifStmt.conditional, depth + 1);
    dump_child(b, n->u.ifStmt.body, depth + 1);
    dump_child(b, n->u.ifStmt.elseBody, depth + 1);
    return;
  case A_WHILE:
    end_scalars(b, n);
    dump_child(b, n->u.loop.conditional, depth + 1);
    dump_child(b, n->u.loop.body, depth + 1);
    return;
  case A_DOWHILE:
    end_scalars(b, n);
    dump_child(b, n->u.loop.body, depth + 1);
    dump_child(b, n->u.loop.conditional, depth + 1);
    return;
  case A_FOR:
    end_scalars(b, n);
    dump_child(b, n->u.forLoop.initialization, depth + 1);
    dump_child(b, n->u.forLoop.condition, depth + 1);
    dump_child(b, n->u.forLoop.increment, depth + 1);
    dump_child(b, n->u.forLoop.body, depth + 1);
    return;
  case A_BREAK:
  case A_CONTINUE:
  case A_TRAP:
  case A_NULLLITERAL:
    end_scalars(b, n);
    return;
  case A_COMMA:
    end_scalars(b, n);
    dump_list(b, &n->u.comma.list, depth + 1);
    return;
  case A_BOOLLITERAL:
    buf_scalar_bool(b, "value", n->u.boolLit.value);
    end_scalars(b, n);
    return;
  case A_VARIABLEREF:
    buf_scalar_str(b, "name", n->u.varRef.name);
    if (n->u.varRef.anon && !DUMP_NO_ROLES) buf_scalar_str(b, "anon", n->u.varRef.anon == 1 ? "old" : "new");
    else buf_str(b, " anon=-");
    end_scalars(b, n);
    return;
  case A_CALL:
    buf_scalar_str(b, "name", n->u.call.name);
    end_scalars(b, n);
    dump_list(b, &n->u.call.typeArguments, depth + 1);
    dump_list(b, &n->u.call.argumentList, depth + 1);
    if (REFS_ON) dump_child(b, n->u.call.returnType, depth + 1);
    return;
  case A_DOT:
    buf_scalar_str(b, "fieldName", n->u.dot.fieldName);
    end_scalars(b, n);
    dump_child(b, n->u.dot.base, depth + 1);
    return;
  case A_INDEX:
    end_scalars(b, n);
    dump_child(b, n->u.index.base, depth + 1);
    dump_child(b, n->u.index.index, depth + 1);
    return;
  case A_DEREF:
    end_scalars(b, n);
    dump_child(b, n->u.deref.ptr, depth + 1);
    return;
  case A_MAKEPTR:
  case A_CONVERTPTR:
  case A_MAKEARRAYREF:
    end_scalars(b, n);
    dump_child(b, n->u.lvalueRef.lValue, depth + 1);
    return;
  case A_LOGICALNOT:
    end_scalars(b, n);
    dump_child(b, n->u.notExpr.operand, depth + 1);
    return;
  case A_LOGICAL:
    buf_scalar_str(b, "text", n->u.logical.text);
    end_scalars(b, n);
    dump_child(b, n->u.logical.left, depth + 1);
    dump_child(b, n->u.logical.right, depth + 1);
    return;
  case A_ASSIGN:
    end_scalars(b, n);
    dump_child(b, n->u.assign.lhs, depth + 1);
    dump_child(b, n->u.assign.rhs, depth + 1);
    return;
  case A_IDENTITY:
    end_scalars(b, n);
    dump_child(b, n->u.identity.target, depth + 1);
    return;
  case A_ENUMLITERAL:
    end_scalars(b, n);
    return;
  case A_FUNCTIONLIKEBLOCK:
    end_scalars(b, n);
    dump_child(b, n->u.fblock.returnType, depth + 1);
    dump_list(b, &n->u.fblock.argumentList, depth + 1);
    dump_list(b, &n->u.fblock.parameters, depth + 1);
    dump_child(b, n->u.fblock.body, depth + 1);
    return;
  case A_ANONVAR:
    end_scalars(b, n);   /* a node line with no scalars; the references to it are named "anonVar" */
    return;   /* an AnonymousVariable is only ever a reference target */
  case A_INTLITERALTYPE: case A_UINTLITERALTYPE: case A_FLOATLITERALTYPE: case A_DOUBLELITERALTYPE:
  case A_NULLTYPE:
    fprintf(stderr, "literal types are not parse output; the dump does not print them\n");
    exit(9);
  case A_RMW:
    end_scalars(b, n);
    dump_child(b, n->u.rmw.lValue, depth + 1);
    dump_child(b, n->u.rmw.newValueExp, depth + 1);
    dump_child(b, n->u.rmw.resultExp, depth + 1);
    return;
  case A_INTLITERAL:
    buf_scalar_str(b, "kind", "int");
    if (n->u.intLit.isDbl) buf_fmt(b, " value=%s", js_num(n->u.intLit.dbl));
    else buf_fmt(b, " value=%lld", n->u.intLit.value);
    end_scalars(b, n);
    return;
  case A_UINTLITERAL:
    buf_scalar_str(b, "kind", "uint");
    buf_fmt(b, " value=%lld", n->u.intLit.value);
    end_scalars(b, n);
    return;
  case A_FLOATLITERAL:
    buf_scalar_str(b, "kind", "float");
    buf_scalar_bits(b, "bits", n->u.fltLit.value);
    end_scalars(b, n);
    return;
  case A_DOUBLELITERAL:
    buf_scalar_str(b, "kind", "double");
    buf_scalar_bits(b, "bits", n->u.fltLit.value);
    end_scalars(b, n);
    return;
  }
}

#endif
