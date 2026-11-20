/* Intrinsics.js as C dispatch per NativeType kind. The JS attaches these properties to each native type
 * instance (isNumber, isInt, isSigned, isFloating, isPrimitive, canRepresent, createLiteral,
 * successorValue, valuesEqual, formatValueFrom<Literal>, defaultValue, size). The kind is read from the
 * NativeType's name once per query, and each property is a switch over it. A property the JS does not
 * define for a kind (a call would throw a TypeError there) is a fatal error here.
 * Number conversions follow the JS operators: js_int32 is `| 0`, js_uint32 is `>>> 0`. */
#ifndef WSL_INTRINSICS_H
#define WSL_INTRINSICS_H

#include <math.h>
#include <stdint.h>
#include <string.h>
#include "wsl-ast.h"

typedef enum { NT_NONE, NT_VOID, NT_INT32, NT_UINT32, NT_UINT8, NT_FLOAT32, NT_FLOAT64, NT_BOOL } NtKind;
typedef enum { LK_INT, LK_UINT, LK_FLOAT, LK_DOUBLE } LitKind;

static void nt_undefined(const char *what, NtKind k) {
  fprintf(stderr, "intrinsic %s is not defined for native kind %d\n", what, (int)k);
  exit(9);
}

/* The NativeType kind of a type node, NT_NONE for every other node and for native types without intrinsics. */
static NtKind nt_kind(const Ast *t) {
  if (t->tag != A_NATIVETYPE) return NT_NONE;
  const char *n = t->u.nativeType.name;
  if (!strcmp(n, "int32")) return NT_INT32;
  if (!strcmp(n, "uint32")) return NT_UINT32;
  if (!strcmp(n, "uint8")) return NT_UINT8;
  if (!strcmp(n, "float32")) return NT_FLOAT32;
  if (!strcmp(n, "float64")) return NT_FLOAT64;
  if (!strcmp(n, "bool")) return NT_BOOL;
  if (!strcmp(n, "void")) return NT_VOID;
  return NT_NONE;
}

/* ---- flags (Intrinsics.js sets these per type) ---- */
static int nt_is_int(NtKind k) { return k == NT_INT32 || k == NT_UINT32 || k == NT_UINT8; }
static int nt_is_floating(NtKind k) { return k == NT_FLOAT32 || k == NT_FLOAT64; }
static int nt_is_number(NtKind k) { return nt_is_int(k) || nt_is_floating(k); }
static int nt_is_signed(NtKind k) { return k == NT_INT32; }
/* uint8 is not marked primitive in Intrinsics.js, so it is not here either */
static int nt_is_primitive(NtKind k) {
  return k == NT_INT32 || k == NT_UINT32 || k == NT_FLOAT32 || k == NT_FLOAT64 || k == NT_BOOL;
}

/* Type.isNumber and friends for any type node (false for everything that is not a native kind) */
static int ty_is_number(const Ast *t) { return nt_is_number(nt_kind(t)); }
static int ty_is_int(const Ast *t) { return nt_is_int(nt_kind(t)); }
static int ty_is_signed(const Ast *t) { return nt_is_signed(nt_kind(t)); }
static int ty_is_floating(const Ast *t) { return nt_is_floating(nt_kind(t)); }

/* ---- number conversions ---- */
static double js_int32(double v) {
  if (!isfinite(v)) return 0;
  double m = fmod(trunc(v), 4294967296.0);
  if (m < 0) m += 4294967296.0;
  return (double)(int32_t)(uint32_t)m;
}

static double js_uint32(double v) {
  if (!isfinite(v)) return 0;
  double m = fmod(trunc(v), 4294967296.0);
  if (m < 0) m += 4294967296.0;
  return (double)(uint32_t)m;
}

/* `value & 0xff` on a number */
static double js_uint8(double v) { return (double)((int32_t)js_int32(v) & 0xff); }

/* isBitwiseEquivalent: the two doubles have the same bits */
static int same_bits(double a, double b) {
  uint64_t x, y;
  memcpy(&x, &a, 8);
  memcpy(&y, &b, 8);
  return x == y;
}

/* ---- the intrinsic properties ---- */
/* canRepresent(value) */
static int nt_can_represent(NtKind k, double v) {
  switch (k) {
  case NT_INT32: return same_bits(js_int32(v), v);
  case NT_UINT32: return same_bits(js_uint32(v), v);
  case NT_UINT8: return same_bits(js_uint8(v), v);
  case NT_FLOAT32: return same_bits((double)(float)v, v);
  case NT_FLOAT64: return 1;
  default: break;
  }
  nt_undefined("canRepresent", k);
  return 0;
}

/* createLiteral(origin, value) converts the value; the caller makes the IntLiteral with the native type */
static double nt_create_literal(NtKind k, double v) {
  switch (k) {
  case NT_INT32: return js_int32(v);
  case NT_UINT32: return js_uint32(v);
  case NT_UINT8: return js_uint8(v);
  default: break;
  }
  nt_undefined("createLiteral", k);
  return 0;
}

/* successorValue(value) */
static double nt_successor(NtKind k, double v) {
  switch (k) {
  case NT_INT32: return js_int32(v + 1);
  case NT_UINT32: return js_uint32(v + 1);
  case NT_UINT8: return js_uint8(v + 1);
  default: break;
  }
  nt_undefined("successorValue", k);
  return 0;
}

/* valuesEqual(a, b) */
static int nt_values_equal(NtKind k, double a, double b) {
  if (nt_is_int(k)) return a == b;
  nt_undefined("valuesEqual", k);
  return 0;
}

/* defaultValue */
static double nt_default_value(NtKind k) {
  if (nt_is_int(k)) return 0;
  nt_undefined("defaultValue", k);
  return 0;
}

/* size: the number of memory cells a value of the kind occupies */
static int nt_size(NtKind k) {
  return k == NT_VOID ? 0 : 1;
}

/* formatValueFrom<Literal>(value): the value as the native kind stores it. Int and uint literals format
 * for the integer kinds; float32 and float64 also format float and double literals. */
static double nt_format_value(NtKind k, LitKind lk, double v) {
  int fromFloat = lk == LK_FLOAT || lk == LK_DOUBLE;
  switch (k) {
  case NT_INT32: if (!fromFloat) return js_int32(v); break;
  case NT_UINT32: if (!fromFloat) return js_uint32(v); break;
  case NT_UINT8: if (!fromFloat) return js_uint8(v); break;
  case NT_FLOAT32: return fromFloat ? (double)(float)v : v;
  case NT_FLOAT64: return v;
  default: break;
  }
  nt_undefined("formatValue", k);
  return 0;
}

/* ---- literal values: a literal expression's value, or a literal type's through its owner ---- */
static double lit_value(const Ast *n) {
  if (n->tag == A_INTLITERALTYPE || n->tag == A_UINTLITERALTYPE || n->tag == A_FLOATLITERALTYPE ||
      n->tag == A_DOUBLELITERALTYPE)
    n = n->u.litType.owner;
  if (n->tag == A_INTLITERAL || n->tag == A_UINTLITERAL)
    return n->u.intLit.isDbl ? n->u.intLit.dbl : (double)n->u.intLit.value;
  return n->u.fltLit.value;
}

#endif
