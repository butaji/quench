/*
 * AST for the TypeScript parser port. A node is one struct with the union of the properties the
 * TypeScript factory gives its node kinds (named like TypeScript's properties). Unused properties
 * stay NULL. Node lists are NodeArrays: element pointers plus pos/end, as in TypeScript.
 * Memory comes from an arena that is reset after each source file.
 */
#ifndef TS_AST_H
#define TS_AST_H

#include <stdint.h>
#include <stdlib.h>
#include <stdbool.h>
#include "ts-enums.h"

typedef struct TsNode TsNode;

typedef struct TsList {        /* NodeArray */
  TsNode **v;
  int n;
  int pos, end;
  bool hasTrailingComma;
  bool isMissingList;
} TsList;

struct TsNode {
  uint16_t kind;
  uint32_t flags;             /* NodeFlags */
  int32_t pos, end;
  uint32_t tokenFlags;        /* for identifiers and literals */
  bool hasPrecedingJSDocLeadingAsterisks_unused;
  const char *text;           /* identifier / literal / keyword text (raw slice) */
  int32_t textLen;
  /* child nodes (TypeScript property names) */
  TsNode *name, *expression, *initializer, *type, *body, *questionToken, *exclamationToken;
  TsNode *asteriskToken, *dotDotDotToken, *literal, *left, *right, *operatorToken, *condition;
  TsNode *whenTrue, *whenFalse, *thenStatement, *elseStatement, *statement, *tag, *tagName;
  TsNode *templateNode, *qualifier, *constraint, *defaultType, *typeName, *argument, *block;
  TsNode *finallyBlock, *catchClause, *variableDeclaration, *label, *incrementor, *declaration;
  TsNode *moduleReference, *moduleSpecifier, *importClause, *namedBindings, *propertyName;
  TsNode *checkType, *extendsType, *trueType, *falseType, *objectType, *indexType, *elementType;
  TsNode *nameOrPath, *endOfFileToken, *head, *exportClause, *node1, *node2, *node3, *node4, *node5;
  TsNode *typeExpression, *readonlyToken, *nameType, *exprName;
  TsNode *attributes_node, *typeParameter, *parameterName, *assertsModifier, *equalsGreaterThanToken;
  TsNode *colonToken, *questionDotToken, *argumentExpression, *operand, *tagName_unused, *objectAssignmentInitializer;
  TsNode *equalsToken, *declarationList, *caseBlock, *awaitModifier, *propertyName_unused;
  /* node lists (TypeScript property names) */
  TsList *modifiers, *typeParameters, *parameters, *members, *statements, *arguments, *elements;
  TsList *properties, *heritageClauses, *types, *declarations, *typeArguments, *clauses, *spans;
  TsList *jsDoc, *tokens, *qualifiers, *assertions, *attributes, *expressions;
  TsList *list1, *list2;
  int32_t extra;              /* small per-kind payload (operator kind, flags, token number) */
  /* binder (ts-binder.h) */
  TsNode *parent;
  struct TsSymbol *symbol, *localSymbol;
  struct TsSymTable *locals;
  bool hasExternalModuleIndicator, isDeclarationFile;
  int32_t binderSymbolCount;  /* SourceFile: file.symbolCount set by the binder */
  int32_t binderFlowCount;    /* SourceFile: flow nodes created by the binder */
  const char *fileName;       /* SourceFile: program file name */
  struct TsType *resolvedType; /* checker: getTypeFromTypeNode cache (links.resolvedType) */
  uint8_t assignMarked;        /* checker: the assignments of the symbols of this function or file are marked (AssignmentsMarked) */
  struct TsType *inferredReturn; /* checker: the inferred return type of a declaration (ts-relate.h) */
  uint8_t inferState;
  uint8_t retLiteral;         /* checker: a return of the declaration has a literal type (before the widening of cInferReturnType) */
  struct TsFlow *flowNode;    /* binder: the flow node of an identifier, this, super or narrowable access (binder.ts flowNode) */
  struct TsType *exprType;    /* checker: checkExpressionCached result (links.resolvedType of expressions) */
  uint8_t exprState;          /* 0 not checked, 1 being checked, 2 done */
  struct CSig *resolvedSig;   /* checker: links.resolvedSignature of a call or new expression (chosen candidate instance) */
  uint8_t checkState;         /* checker: checkSourceElement of a statement or declaration (0 not, 1 being, 2 done) */
  /* enums (ts-types.h): EnumDeclaration: EnumValuesComputed; EnumMember: its computed value */
  uint8_t enumValuesComputed, enumValueDone, enumValueKind;   /* kind: 0 undefined, 1 number, 2 string */
  uint8_t condState;   /* ConditionalType: being resolved (ts-conditional.h) */
  double enumValueNum;
  const char *enumValueStr;
  int32_t enumValueLen;
};

/* ---- arena ---- */
typedef struct TsArenaBlock { struct TsArenaBlock *next; size_t used, cap; char data[]; } TsArenaBlock;
static TsArenaBlock *ts_arena_head;

static void *ts_arena_alloc(size_t n) {
  n = (n + 15) & ~(size_t)15;
  if (!ts_arena_head || ts_arena_head->used + n > ts_arena_head->cap) {
    size_t cap = n > (1u << 20) ? n : (1u << 20);
    TsArenaBlock *b = malloc(sizeof(TsArenaBlock) + cap);
    if (!b) abort();
    b->next = ts_arena_head;
    b->used = 0;
    b->cap = cap;
    ts_arena_head = b;
  }
  void *p = ts_arena_head->data + ts_arena_head->used;
  ts_arena_head->used += n;
  return p;
}

static void ts_arena_reset(void) {
  while (ts_arena_head) {
    TsArenaBlock *n = ts_arena_head->next;
    free(ts_arena_head);
    ts_arena_head = n;
  }
}

/* ---- node stack: parser list building uses a stack discipline (mark, push, take) ---- */
static TsNode **ts_stk;
static int ts_stk_n, ts_stk_cap;
static void ts_stk_push(TsNode *n) {
  if (ts_stk_n == ts_stk_cap) {
    ts_stk_cap = ts_stk_cap ? ts_stk_cap * 2 : 256;
    ts_stk = realloc(ts_stk, sizeof(TsNode *) * ts_stk_cap);
    if (!ts_stk) abort();
  }
  ts_stk[ts_stk_n++] = n;
}

/* createNodeArray(elements from mark) */
static TsList *ts_list_take(int mark, int pos, int end, bool trailingComma) {
  TsList *l = ts_arena_alloc(sizeof(TsList));
  int n = ts_stk_n - mark;
  l->n = n;
  l->v = ts_arena_alloc(sizeof(TsNode *) * (n ? n : 1));
  for (int i = 0; i < n; i++) l->v[i] = ts_stk[mark + i];
  ts_stk_n = mark;
  l->pos = pos;
  l->end = end;
  l->hasTrailingComma = trailingComma;
  return l;
}

static TsList *ts_list_empty(int pos, int end) {
  TsList *l = ts_arena_alloc(sizeof(TsList));
  l->v = NULL;
  l->n = 0;
  l->pos = pos;
  l->end = end;
  return l;
}

static TsList *ts_list_single(TsNode *a, int pos, int end) {
  TsList *l = ts_arena_alloc(sizeof(TsList));
  l->v = ts_arena_alloc(sizeof(TsNode *));
  l->v[0] = a;
  l->n = 1;
  l->pos = pos;
  l->end = end;
  return l;
}

#endif
