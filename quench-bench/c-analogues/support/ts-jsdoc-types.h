/* Types of the JSDoc parser port (see ts-jsdoc.h). Kept apart so the generated prototypes can use them. */
#ifndef TS_JSDOC_TYPES_H
#define TS_JSDOC_TYPES_H
/* ---- source-slice strings ---- */

typedef struct { int s, e; } JPiece;

typedef struct { JPiece *v; int n, cap; } JPieces;

typedef struct { TsNode **v; int n, cap; } JNodes;

/* an optional string: def == false is `undefined` (the JS parameters indentText / margin) */
typedef struct { int s, e; bool def; } JStr;

/* result of parseTagComments / parseTrailingTagComments: undefined, a string, or a NodeArray */
enum { JC_Undefined = 0, JC_String = 1, JC_Nodes = 2 };
typedef struct { int kind; int s, e; TsList *nodes; } JComment;

typedef struct { int pos, end; } CRange;

typedef struct { CRange *v; int n, cap; } CRanges;

typedef struct {
  int start;
  JPieces comments;
  JNodes parts;
  JNodes tags;
  bool hasTags;
  int tagsPos, tagsEnd;
  bool hasLinkEnd;
  int linkEnd;
  bool hasCommentsPos;
  int commentsPos;
} JsDocCtx;
static JsDocCtx *JD;
static bool hasDeprecatedTag;

typedef struct { int end, pos, fullStart, tokenStart, token; const char *tval; int tvalLen; uint32_t tflags; } JsRangeSave;

typedef struct { int target; int indent; TsNode *name; } JsChildArgs;
static JsChildArgs jsChildArgs;
static int jsIndentArg;

typedef struct { TsNode *name; bool isBracketed; } JBracketName;

#endif
