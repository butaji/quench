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
  A_IDENTITY, A_ENUMLITERAL
} AstTag;

typedef struct Ast Ast;
typedef struct { Ast **v; int n, cap; } AstList;

struct Ast {
  AstTag tag;
  int origin;                 /* index of the origin token in the source, -1 if none */
  int native;                 /* 1 if the origin kind is "native" (the standard library), else 0 */
  union {
    /* FuncDef, NativeFunc, ProtocolFuncDecl */
    struct { const char *name; int isCast; const char *shaderType; int isRestricted;
             Ast *returnType; AstList typeParameters; AstList parameters; Ast *body;
             int hasOv; AstList ov; } func;   /* ov: possibleOverloads (ProtocolFuncDecl) */
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
    /* CallExpression */
    struct { const char *name; AstList typeArguments; AstList argumentList;
             int hasOv; AstList ov; int isCast; Ast *returnType; } call;
    /* DotExpression */
    struct { const char *fieldName; Ast *base;
             int hasGet, hasSet, hasAnd; AstList getOv, setOv, andOv; } dot;
    /* IndexExpression */
    struct { Ast *base; Ast *index;
             int hasGet, hasSet, hasAnd; AstList getOv, setOv, andOv; } index;
    /* DereferenceExpression */
    struct { Ast *ptr; } deref;
    /* MakePtrExpression, MakeArrayRefExpression */
    struct { Ast *lValue; } lvalueRef;
    /* LogicalNot */
    struct { Ast *operand; } notExpr;
    /* LogicalExpression */
    struct { const char *text; Ast *left; Ast *right; } logical;
    /* Assignment */
    struct { Ast *lhs; Ast *rhs; } assign;
    /* ReadModifyWriteExpression */
    struct { Ast *lValue; Ast *newValueExp; Ast *resultExp; } rmw;
    /* IdentityExpression (a rewritten node): target */
    struct { Ast *target; } identity;
    /* EnumLiteral */
    struct { Ast *member; } enumLit;
    /* IntLiteral, UintLiteral */
    struct { long long value; Ast *preferred; } intLit;   /* preferred: GenericLiteralType.preferredType */
    /* FloatLiteral, DoubleLiteral: value is the double (float literals are rounded to float) */
    struct { double value; Ast *preferred; } fltLit;
  } u;
};

/* ---- arena ---- */
typedef struct { char *base; size_t used, cap; } Arena;
static Arena AST_ARENA;
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
    buf_scalar_str(b, "name", n->u.func.name);
    buf_scalar_bool(b, "isCast", n->u.func.isCast);
    buf_scalar_str(b, "shaderType", n->u.func.shaderType);
    if (n->tag != A_PROTOCOLFUNCDECL) buf_scalar_bool(b, "isRestricted", n->u.func.isRestricted);
    buf_str(b, "\n");
    dump_child(b, n->u.func.returnType, depth + 1);
    dump_list(b, &n->u.func.typeParameters, depth + 1);
    dump_list(b, &n->u.func.parameters, depth + 1);
    if (n->tag == A_FUNCDEF) dump_child(b, n->u.func.body, depth + 1);
    return;
  case A_TYPEREF:
    buf_scalar_str(b, "name", n->u.typeRef.name);
    buf_str(b, "\n");
    dump_list(b, &n->u.typeRef.typeArguments, depth + 1);
    return;
  case A_PTRTYPE:
  case A_ARRAYREFTYPE:
    buf_scalar_str(b, "addressSpace", n->u.ref.addressSpace);
    buf_str(b, "\n");
    dump_child(b, n->u.ref.elementType, depth + 1);
    return;
  case A_ARRAYTYPE:
    buf_str(b, "\n");
    dump_child(b, n->u.arrayType.elementType, depth + 1);
    dump_child(b, n->u.arrayType.numElements, depth + 1);
    return;
  case A_TYPEVARIABLE:
    buf_scalar_str(b, "name", n->u.typeVariable.name);
    buf_str(b, "\n");
    dump_child(b, n->u.typeVariable.protocol, depth + 1);
    return;
  case A_CONSTEXPRTYPEPARAM:
  case A_FIELD:
  case A_FUNCPARAMETER:
    buf_scalar_str(b, "name", n->u.named.name);
    buf_str(b, "\n");
    dump_child(b, n->u.named.type, depth + 1);
    return;
  case A_TYPEORVARIABLEREF:
  case A_PROTOCOLREF:
    buf_scalar_str(b, "name", n->u.nameOnly.name);
    buf_str(b, "\n");
    return;
  case A_TYPEDEF:
    buf_scalar_str(b, "name", n->u.typeDef.name);
    buf_str(b, "\n");
    dump_list(b, &n->u.typeDef.typeParameters, depth + 1);
    dump_child(b, n->u.typeDef.type, depth + 1);
    return;
  case A_STRUCTTYPE:
    buf_scalar_str(b, "name", n->u.structType.name);
    buf_str(b, "\n");
    dump_list(b, &n->u.structType.typeParameters, depth + 1);
    dump_list(b, &n->u.structType.fields, depth + 1);
    return;
  case A_ENUMTYPE:
    buf_scalar_str(b, "name", n->u.enumType.name);
    buf_str(b, "\n");
    dump_child(b, n->u.enumType.baseType, depth + 1);
    dump_list(b, &n->u.enumType.members, depth + 1);
    return;
  case A_ENUMMEMBER:
    buf_scalar_str(b, "name", n->u.nameOnly.name);
    buf_str(b, "\n");
    dump_child(b, n->u.nameOnly.value, depth + 1);
    return;
  case A_PROTOCOLDECL:
    buf_scalar_str(b, "name", n->u.protocolDecl.name);
    buf_str(b, "\n");
    dump_list(b, &n->u.protocolDecl.extends, depth + 1);
    dump_list(b, &n->u.protocolDecl.signatures, depth + 1);
    return;
  case A_NATIVETYPE:
    buf_scalar_str(b, "name", n->u.nativeType.name);
    buf_str(b, "\n");
    dump_list(b, &n->u.nativeType.typeParameters, depth + 1);
    return;
  case A_VARIABLEDECL:
    buf_scalar_str(b, "name", n->u.varDecl.name);
    buf_str(b, "\n");
    dump_child(b, n->u.varDecl.type, depth + 1);
    dump_child(b, n->u.varDecl.initializer, depth + 1);
    return;
  case A_BLOCK:
    buf_str(b, "\n");
    dump_list(b, &n->u.block.statements, depth + 1);
    return;
  case A_SWITCHSTATEMENT:
    buf_str(b, "\n");
    dump_child(b, n->u.switchStmt.value, depth + 1);
    dump_list(b, &n->u.switchStmt.cases, depth + 1);
    return;
  case A_SWITCHCASE:
    buf_str(b, "\n");
    dump_child(b, n->u.switchCase.value, depth + 1);
    dump_child(b, n->u.switchCase.body, depth + 1);
    return;
  case A_RETURN:
    buf_str(b, "\n");
    dump_child(b, n->u.ret.value, depth + 1);
    return;
  case A_IF:
    buf_str(b, "\n");
    dump_child(b, n->u.ifStmt.conditional, depth + 1);
    dump_child(b, n->u.ifStmt.body, depth + 1);
    dump_child(b, n->u.ifStmt.elseBody, depth + 1);
    return;
  case A_WHILE:
    buf_str(b, "\n");
    dump_child(b, n->u.loop.conditional, depth + 1);
    dump_child(b, n->u.loop.body, depth + 1);
    return;
  case A_DOWHILE:
    buf_str(b, "\n");
    dump_child(b, n->u.loop.body, depth + 1);
    dump_child(b, n->u.loop.conditional, depth + 1);
    return;
  case A_FOR:
    buf_str(b, "\n");
    dump_child(b, n->u.forLoop.initialization, depth + 1);
    dump_child(b, n->u.forLoop.condition, depth + 1);
    dump_child(b, n->u.forLoop.increment, depth + 1);
    dump_child(b, n->u.forLoop.body, depth + 1);
    return;
  case A_BREAK:
  case A_CONTINUE:
  case A_TRAP:
  case A_NULLLITERAL:
    buf_str(b, "\n");
    return;
  case A_COMMA:
    buf_str(b, "\n");
    dump_list(b, &n->u.comma.list, depth + 1);
    return;
  case A_BOOLLITERAL:
    buf_scalar_bool(b, "value", n->u.boolLit.value);
    buf_str(b, "\n");
    return;
  case A_VARIABLEREF:
    buf_scalar_str(b, "name", n->u.varRef.name);
    if (n->u.varRef.anon) buf_scalar_str(b, "anon", n->u.varRef.anon == 1 ? "old" : "new");
    else buf_str(b, " anon=-");
    buf_str(b, "\n");
    return;
  case A_CALL:
    buf_scalar_str(b, "name", n->u.call.name);
    buf_str(b, "\n");
    dump_list(b, &n->u.call.typeArguments, depth + 1);
    dump_list(b, &n->u.call.argumentList, depth + 1);
    return;
  case A_DOT:
    buf_scalar_str(b, "fieldName", n->u.dot.fieldName);
    buf_str(b, "\n");
    dump_child(b, n->u.dot.base, depth + 1);
    return;
  case A_INDEX:
    buf_str(b, "\n");
    dump_child(b, n->u.index.base, depth + 1);
    dump_child(b, n->u.index.index, depth + 1);
    return;
  case A_DEREF:
    buf_str(b, "\n");
    dump_child(b, n->u.deref.ptr, depth + 1);
    return;
  case A_MAKEPTR:
  case A_MAKEARRAYREF:
    buf_str(b, "\n");
    dump_child(b, n->u.lvalueRef.lValue, depth + 1);
    return;
  case A_LOGICALNOT:
    buf_str(b, "\n");
    dump_child(b, n->u.notExpr.operand, depth + 1);
    return;
  case A_LOGICAL:
    buf_scalar_str(b, "text", n->u.logical.text);
    buf_str(b, "\n");
    dump_child(b, n->u.logical.left, depth + 1);
    dump_child(b, n->u.logical.right, depth + 1);
    return;
  case A_ASSIGN:
    buf_str(b, "\n");
    dump_child(b, n->u.assign.lhs, depth + 1);
    dump_child(b, n->u.assign.rhs, depth + 1);
    return;
  case A_IDENTITY:
    buf_str(b, "\n");
    dump_child(b, n->u.identity.target, depth + 1);
    return;
  case A_ENUMLITERAL:
    buf_str(b, "\n");
    return;
  case A_RMW:
    buf_str(b, "\n");
    dump_child(b, n->u.rmw.lValue, depth + 1);
    dump_child(b, n->u.rmw.newValueExp, depth + 1);
    dump_child(b, n->u.rmw.resultExp, depth + 1);
    return;
  case A_INTLITERAL:
    buf_scalar_str(b, "kind", "int");
    buf_fmt(b, " value=%lld\n", n->u.intLit.value);
    return;
  case A_UINTLITERAL:
    buf_scalar_str(b, "kind", "uint");
    buf_fmt(b, " value=%lld\n", n->u.intLit.value);
    return;
  case A_FLOATLITERAL:
    buf_scalar_str(b, "kind", "float");
    buf_scalar_bits(b, "bits", n->u.fltLit.value);
    buf_str(b, "\n");
    return;
  case A_DOUBLELITERAL:
    buf_scalar_str(b, "kind", "double");
    buf_scalar_bits(b, "bits", n->u.fltLit.value);
    buf_str(b, "\n");
    return;
  }
}

#endif
