/* The operations of the WSL tests (tools/wsl-tests-table.cjs writes the table in wsl-tests-table.h). One WslOp per
 * operation, in the order the test performs it; a WslTest is a test's name and its slice of WSL_TEST_OPS. Fields:
 *   OP_PREP   s/len: the program text of doPrep(text); the program is the next prepare id of the test (a)
 *   OP_ARG    a: ARG_* kind (int, uint, uint8, bool, float, double, enum: value d; res: the result b of an earlier call)
 *   OP_CALL   a: program id of the call; b: result id of the call; c: the number of pending OP_ARG; s: function name
 *   OP_CHECK  a: CHECK_* kind (int, uint, uint8, bool, float, double, enum, number); b: result id; c: 1 if expected is a
 *             boolean; d: the expected value
 *   OP_FBEGIN a: CLS_* class the thrown error must be an instance of; s: substring of its message (NULL for none)
 *   OP_FEND   the end of a checkFail block: one of its operations must have thrown
 *   OP_LEX    a: lexer id (doLex result); s/len: the text of doLex(text)
 *   OP_LEXCHECK a: lexer id; b: token index; c: expected index; s: expected kind; s2: expected text
 *   OP_BUF    a: buffer id (the test's new EBuffer); b: its size
 *   OP_BSET   a: buffer id; b: index; c: 1 if the value is a boolean; d: the value (a test's buffer.set)
 *   OP_BGET   a: buffer id; b: index; c: 0 number, 1 boolean, 2 undefined; d: the value (a test's buffer.get)
 *   OP_RESVAL a: result id; c: 0 number, 1 boolean, 2 null; d: the value (a test's read of result.value)
 *   OP_RGET   a: result id; b: index; c: 0 number, 1 boolean, 2 undefined; d: the value (a test's read of result.ePtr)
 *   OP_TYPED  an argument the test builds: a: buffer id; b: offset; c: TYPED_* kind; d: length (arrref); s: address
 *             space (ptr, arrref; "" for a struct); s2: element native type name, or the struct's name
 * The lexer's token i is the i-th token of its doLex result. */
#ifndef WSL_TESTS_H
#define WSL_TESTS_H

#include <math.h>
#include <stddef.h>

enum { OP_PREP, OP_ARG, OP_CALL, OP_CHECK, OP_FBEGIN, OP_FEND, OP_LEX, OP_LEXCHECK, OP_BUF, OP_BSET, OP_BGET, OP_RESVAL, OP_TYPED, OP_RGET };
enum { TYPED_PTR, TYPED_ARRREF, TYPED_STRUCT };
enum { ARG_INT, ARG_UINT, ARG_UINT8, ARG_BOOL, ARG_FLOAT, ARG_DOUBLE, ARG_ENUM, ARG_RES };
enum { CHECK_INT, CHECK_UINT, CHECK_UINT8, CHECK_BOOL, CHECK_FLOAT, CHECK_DOUBLE, CHECK_ENUM, CHECK_NUMBER };
enum { CLS_WTYPEERROR, CLS_WTRAPERROR, CLS_WSYNTAXERROR };

typedef struct {
  int op;
  int a, b, c;
  double d;
  const char *s;
  int len;      /* the length of s (OP_PREP, OP_LEX): the text may hold any bytes */
  const char *s2;
} WslOp;

typedef struct {
  const char *name;
  int first, count;   /* the test's slice of WSL_TEST_OPS */
} WslTest;

#endif
