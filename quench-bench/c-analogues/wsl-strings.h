/* The string conversions the WSL checker's messages build (toString and toDeclString of the classes, and
 * the JS operators they use: + on strings, Array.prototype.join, Number::toString). Strings live in the
 * arena, like the nodes they came from. Nodes whose toString is not needed by any message ported so far
 * stop with a fatal error (js_tostr_unsupported). */
#ifndef WSL_STRINGS_H
#define WSL_STRINGS_H

#include <stdarg.h>
#include "wsl-intrinsics.h"
#include "wsl-names.h"

/* CAT(s1, s2, ...): the strings concatenated, the count taken from the arguments */
#define CAT(...) cat((int)(sizeof((const char *[]){__VA_ARGS__}) / sizeof(const char *)), __VA_ARGS__)

/* s1 + s2 + ... (n strings) in the arena */
static const char *cat(int n, ...) {
  va_list ap;
  size_t len = 0;
  va_start(ap, n);
  for (int i = 0; i < n; i++) len += strlen(va_arg(ap, const char *));
  va_end(ap);
  char *s = arena_alloc(len + 1);
  size_t k = 0;
  va_start(ap, n);
  for (int i = 0; i < n; i++) {
    const char *p = va_arg(ap, const char *);
    size_t m = strlen(p);
    memcpy(s + k, p, m);
    k += m;
  }
  va_end(ap);
  s[len] = 0;
  return s;
}

/* Number::toString: the shortest decimal that reads back as v, in JS's layout */
static const char *js_num(double v) {
  if (v != v) return "NaN";
  if (v == 0) return "0";
  if (isinf(v)) return v < 0 ? "-Infinity" : "Infinity";
  char buf[64];
  int prec;
  for (prec = 1; prec <= 17; prec++) {
    snprintf(buf, sizeof buf, "%.*e", prec - 1, v);
    if (strtod(buf, NULL) == v) break;
  }
  char digits[32];
  int nd = 0;
  const char *q = buf + (v < 0 ? 1 : 0);
  for (; *q && *q != 'e'; q++)
    if (*q >= '0' && *q <= '9') digits[nd++] = *q;
  int n = atoi(q + 1) + 1;   /* JS n: the decimal point position */
  while (nd > 1 && digits[nd - 1] == '0') nd--;
  char out[64];
  char *o = out;
  if (v < 0) *o++ = '-';
  if (nd <= n && n <= 21) {
    memcpy(o, digits, nd); o += nd;
    for (int i = nd; i < n; i++) *o++ = '0';
  } else if (0 < n && n <= 21) {
    memcpy(o, digits, n); o += n;
    *o++ = '.';
    memcpy(o, digits + n, nd - n); o += nd - n;
  } else if (-6 < n && n <= 0) {
    *o++ = '0'; *o++ = '.';
    for (int i = 0; i < -n; i++) *o++ = '0';
    memcpy(o, digits, nd); o += nd;
  } else {
    *o++ = digits[0];
    if (nd > 1) { *o++ = '.'; memcpy(o, digits + 1, nd - 1); o += nd - 1; }
    o += sprintf(o, "e%c%d", n - 1 < 0 ? '-' : '+', n - 1 < 0 ? 1 - n : n - 1);
  }
  *o = 0;
  return cat(1, out);
}

static const char *js_tostr(const Ast *n);

/* Array.prototype.join(sep) of the toStrings of a list */
static const char *js_join(const AstList *l, const char *sep) {
  const char *s = "";
  for (int i = 0; i < l->n; i++) s = cat(3, s, i ? sep : "", js_tostr(l->v[i]));
  return s;
}

/* the name of a protocol reference or declaration (Protocol.name) */
static const char *proto_name(const Ast *p) {
  return p->tag == A_PROTOCOLDECL ? p->u.protocolDecl.name : p->u.nameOnly.name;
}

static const char *js_tostr_unsupported(const Ast *n) {
  fprintf(stderr, "toString is not ported for %s (at %d)\n", ast_class_name(n), n->origin);
  exit(9);
  return NULL;
}

/* Func.toDeclString (NativeFunc prefixes "native "; the shader type and the call form as in Func.js) */
static const char *js_decl(const Ast *f) {
  const char *r = "";
  if (f->u.func.shaderType) r = cat(2, r, cat(2, f->u.func.shaderType, " "));
  const char *tps = js_join(&f->u.func.typeParameters, ",");
  if (f->u.func.isCast)
    r = cat(5, r, "operator<", tps, "> ", js_tostr(f->u.func.returnType));
  else
    r = cat(5, r, js_tostr(f->u.func.returnType), " ", f->u.func.name, cat(3, "<", tps, ">"));
  if (f->tag == A_NATIVEFUNC) r = cat(2, "native ", r);
  return cat(3, r, "(", cat(3, js_join(&f->u.func.parameters, ","), ")", ""));
}

/* toString of any node a message prints: types, literal values, protocols, functions and their parts */
static const char *js_tostr(const Ast *n) {
  switch (n->tag) {
  case A_TYPEREF: {
    /* TypeRef.toString: a reference without a name (setTypeAndArguments) prints the type it names */
    if (!n->u.typeRef.name) return js_tostr(n->u.typeRef.type);
    if (!n->u.typeRef.typeArguments.n) return n->u.typeRef.name;
    return cat(4, n->u.typeRef.name, "<", js_join(&n->u.typeRef.typeArguments, ","), ">");
  }
  case A_NATIVETYPE:
    return cat(4, "native typedef ", n->u.nativeType.name, "<", cat(2, js_join(&n->u.nativeType.typeParameters, ","), ">"));
  case A_PTRTYPE:
    return cat(4, js_tostr(n->u.ref.elementType), "* ", n->u.ref.addressSpace, "");
  case A_ARRAYREFTYPE:
    return cat(4, js_tostr(n->u.ref.elementType), "[] ", n->u.ref.addressSpace, "");
  case A_ARRAYTYPE:
    return cat(4, js_tostr(n->u.arrayType.elementType), "[", js_tostr(n->u.arrayType.numElements), "]");
  case A_TYPEVARIABLE:
    if (!n->u.typeVariable.protocol) return n->u.typeVariable.name;
    return cat(3, n->u.typeVariable.name, ":", proto_name(n->u.typeVariable.protocol));
  case A_CONSTEXPRTYPEPARAM:
    return cat(3, js_tostr(n->u.named.type), " ", n->u.named.name);
  case A_STRUCTTYPE:
    return cat(6, "struct ", n->u.structType.name, "<", js_join(&n->u.structType.typeParameters, ","), "> { ",
               cat(2, js_join(&n->u.structType.fields, "; "), "; }"));
  case A_ENUMTYPE:
    return cat(7, "enum ", n->u.enumType.name, " : ", js_tostr(n->u.enumType.baseType), " { ",
               js_join(&n->u.enumType.members, ","), " }");
  case A_ENUMMEMBER: {
    const char *r = n->u.nameOnly.name;
    if (n->u.nameOnly.value) r = cat(3, r, " = ", js_tostr(n->u.nameOnly.value));
    return r;
  }
  case A_NULLTYPE:
    return "nullType";
  case A_INTLITERALTYPE: case A_UINTLITERALTYPE: case A_FLOATLITERALTYPE: case A_DOUBLELITERALTYPE: {
    const char *pref = n->tag == A_INTLITERALTYPE ? "int" : n->tag == A_UINTLITERALTYPE ? "uint"
                     : n->tag == A_FLOATLITERALTYPE ? "float" : "double";
    return cat(4, pref, "LiteralType<", js_num(lit_value(n)), ">");
  }
  case A_INTLITERAL: case A_UINTLITERAL: case A_FLOATLITERAL: case A_DOUBLELITERAL: {
    const char *pref = n->tag == A_INTLITERAL ? "int" : n->tag == A_UINTLITERAL ? "uint"
                     : n->tag == A_FLOATLITERAL ? "float" : "double";
    return cat(4, pref, "Literal<", js_num(lit_value(n)), ">");
  }
  case A_PROTOCOLREF:
    return n->u.nameOnly.name;
  case A_VARIABLEREF:
    return n->u.varRef.name;
  case A_IDENTITY:
    return CAT("(", js_tostr(n->u.identity.target), ")");
  case A_PROTOCOLDECL:
    return cat(5, "protocol ", n->u.protocolDecl.name, " { ", js_join(&n->u.protocolDecl.signatures, "; "), "; }");
  case A_TYPEDEF:
    return cat(7, "typedef ", n->u.typeDef.name, "<", js_join(&n->u.typeDef.typeParameters, ","), "> = ",
               js_tostr(n->u.typeDef.type), "");
  case A_FIELD:
    return cat(3, js_tostr(n->u.named.type), " ", n->u.named.name);
  case A_FUNCPARAMETER:
    if (!n->u.named.name) return js_tostr(n->u.named.type);
    return cat(3, js_tostr(n->u.named.type), " ", n->u.named.name);
  case A_FUNCDEF: case A_NATIVEFUNC: case A_PROTOCOLFUNCDECL:
    return js_decl(n);
  case A_MAKEARRAYREF: /* MakeArrayRefExpression.toString */
    return cat(5, "@", n->u.lvalueRef.numElements ? cat(3, "<<", js_tostr(n->u.lvalueRef.numElements), ">>") : "",
               "(", js_tostr(n->u.lvalueRef.lValue), ")");
  case A_MAKEPTR:
    return cat(3, "&(", js_tostr(n->u.lvalueRef.lValue), ")");
  case A_CONVERTPTR:
    return cat(3, "@(", js_tostr(n->u.lvalueRef.lValue), ")");
  case A_LOGICALNOT:
    return cat(3, "!(", js_tostr(n->u.notExpr.operand), ")");
  case A_LOGICAL:
    return cat(7, "(", js_tostr(n->u.logical.left), " ", n->u.logical.text, " ", js_tostr(n->u.logical.right), ")");
  case A_DEREF:
    return cat(3, "*(", js_tostr(n->u.deref.ptr), ")");
  case A_ASSIGN:
    return cat(3, js_tostr(n->u.assign.lhs), " = ", js_tostr(n->u.assign.rhs));
  case A_COMMA:
    return cat(3, "(", js_join(&n->u.comma.list, ","), ")");
  case A_RMW:
    return cat(10, "RMW(", js_tostr(n->u.rmw.lValue), ", anonVar<old>, anonVar<new>, ", js_tostr(n->u.rmw.newValueExp), ", ",
               js_tostr(n->u.rmw.resultExp), ")", "", "", "");
  case A_BOOLLITERAL:
    return n->u.boolLit.value ? "true" : "false";
  case A_NULLLITERAL:
    return "null";
  case A_ENUMLITERAL:
    return cat(3, n->u.enumLit.member->u.nameOnly.target->u.enumType.name, ".", n->u.enumLit.member->u.nameOnly.name);
  case A_DOT:   /* DotExpression.toString */
    return cat(5, "(", js_tostr(n->u.dot.base), ".", n->u.dot.fieldName, ")");
  case A_INDEX: /* IndexExpression.toString */
    return cat(5, "(", js_tostr(n->u.index.base), "[", js_tostr(n->u.index.index), "])");
  case A_CALL: { /* CallExpression.toString; the actual type arguments show once the call is resolved */
    const char *head = n->u.call.isCast ? cat(2, "operator ", js_tostr(n->u.call.returnType)) : n->u.call.name;
    const char *actual = n->u.call.func ? cat(3, "<<", js_join(&n->u.call.actualTA, ","), ">>") : "";
    return cat(6, head, "<", js_join(&n->u.call.typeArguments, ","), ">", actual,
               cat(3, "(", js_join(&n->u.call.argumentList, ","), ")"));
  }
  default:
    return js_tostr_unsupported(n);
  }
}

/* originString of a node: its file, and its line in that file (the lexer's lineNumber + 1). The standard
 * library's lines start at 28 (prepare() passes lineNumberOffset 27); the user text is the input being run. */
static const char *ORIGIN_TEXT = "";
static size_t ORIGIN_LEN = 0;
static const char *origin_string(const Ast *n) {
  const char *text = n->native ? INPUTS[0].text : ORIGIN_TEXT;
  size_t len = n->native ? (size_t)INPUTS[0].len : ORIGIN_LEN;
  int line = (n->native ? 27 : 0) + 1;
  for (size_t i = 0; i < (size_t)n->origin && i < len; i++) if (text[i] == '\n') line++;
  char num[16];
  snprintf(num, sizeof num, "%d", line);
  return CAT(n->native ? "/internal/stdlib" : "/internal/test", ":", num);
}

#endif
