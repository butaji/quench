/*
 * TypeScript 5.9.2 parser, ported from namespace Parser in lib/typescript.js (lines 33083-39965),
 * function by function. Names follow the JS (parseX, isX, nextToken, ...). Notes:
 *  - nodeCount is incremented by every factory call (countNode in the JS), so every node creation
 *    below calls newNode(); tokens, identifiers and speculative (later discarded) nodes count too.
 *  - Syntax cursor / incremental reuse is not used by a fresh parse and is not ported.
 *  - Parse errors are only counted (positions are kept for TypeScript's same-position dedup);
 *    the messages are not produced, and speculation rollback restores the count.
 *  - Speculation: tryParse = scanner tryScan + parser state rollback on failure;
 *    lookAhead = always rolled back.
 */
#ifndef TS_PARSER_H
#define TS_PARSER_H

#include <stdio.h>
#include <string.h>
#include "ts-ast.h"
#include "ts-scanner.h"


typedef intptr_t (*TsCb)(void);
typedef TsNode *(*TsParseFn)(void);

/* ParsingContext (typescript.js ParsingContext enum) */
enum {
  PC_SourceElements, PC_BlockStatements, PC_SwitchClauses, PC_SwitchClauseStatements, PC_TypeMembers,
  PC_ClassMembers, PC_EnumMembers, PC_HeritageClauseElement, PC_VariableDeclarations,
  PC_ObjectBindingElements, PC_ArrayBindingElements, PC_ArgumentExpressions, PC_ObjectLiteralMembers,
  PC_JsxAttributes, PC_JsxChildren, PC_ArrayLiteralMembers, PC_Parameters, PC_JSDocParameters,
  PC_RestProperties, PC_TypeParameters, PC_TypeArguments, PC_TupleElementTypes, PC_HeritageClauses,
  PC_ImportOrExportSpecifiers, PC_ImportAttributes, PC_JSDocComment, PC_Count
};

/* ContextFlags of the parser (typescript.js contextFlags values, same as NodeFlags bits). */
enum {
  CF_Yield = 16384, CF_DisallowIn = 8192, CF_Decorator = 32768, CF_Await = 65536,
  CF_DisallowConditionalTypes = 131072, CF_Ambient = 33554432,
};

typedef struct TsParser {
  int languageVersion;
  int scriptKind;
  int languageVariant;
  int contextFlags;
  int sourceFlags;
  int currentToken;
  long nodeCount;
  long identifierCount;
  int *errs;                 /* parseDiagnostics: start positions (messages not kept) */
  int nerr, cerr;
  bool errBeforeNextFinishedNode;
  int parsingContext;
  bool topLevel;
  int jsDocParsingMode;
} TsParser;

static TsParser P;
#include "ts-protos.h"

/* countNode: every node the factory creates */
static TsNode *newNode(int kind) {
  P.nodeCount++;
  TsNode *n = ts_arena_alloc(sizeof(TsNode));
  memset(n, 0, sizeof *n);
  n->kind = (uint16_t)kind;
  n->pos = 0;
  n->end = 0;
  return n;
}

/* ---- errors ---- */
static void parseErrorAtPosition(int start) {
  if (P.nerr == 0 || P.errs[P.nerr - 1] != start) {
    if (P.nerr == P.cerr) {
      P.cerr = P.cerr ? P.cerr * 2 : 64;
      P.errs = realloc(P.errs, sizeof(int) * P.cerr);
      if (!P.errs) abort();
    }
    P.errs[P.nerr++] = start;
  }
  P.errBeforeNextFinishedNode = true;
}
static void parseErrorAtCurrentToken(void) { parseErrorAtPosition(TS.tokenStart); }
static void parseErrorAt(int start) { parseErrorAtPosition(start); }

/* scanner errors: the scanner's own error2 reports at the scanner position */
static void scanError(void) { parseErrorAtPosition(TS.pos); }

/* ---- flags and context ---- */
static void setContextFlag(bool val, int flag) {
  if (val) P.contextFlags |= flag;
  else P.contextFlags &= ~flag;
}
static void setDisallowInContext(bool v) { setContextFlag(v, CF_DisallowIn); }
static void setYieldContext(bool v) { setContextFlag(v, CF_Yield); }
static void setDecoratorContext(bool v) { setContextFlag(v, CF_Decorator); }
static void setAwaitContext(bool v) { setContextFlag(v, CF_Await); }

/* doOutsideOfContext / doInsideOfContext: run cb with the given context flags cleared / set */
static intptr_t doOutsideOfContext(int context, TsCb cb) {
  int toClear = context & P.contextFlags;
  if (toClear) {
    setContextFlag(false, toClear);
    intptr_t r = cb();
    setContextFlag(true, toClear);
    return r;
  }
  return cb();
}
static intptr_t doInsideOfContext(int context, TsCb cb) {
  int toSet = context & ~P.contextFlags;
  if (toSet) {
    setContextFlag(true, toSet);
    intptr_t r = cb();
    setContextFlag(false, toSet);
    return r;
  }
  return cb();
}
static intptr_t allowInAnd(TsCb cb) { return doOutsideOfContext(CF_DisallowIn, cb); }
static intptr_t disallowInAnd(TsCb cb) { return doInsideOfContext(CF_DisallowIn, cb); }
static intptr_t allowConditionalTypesAnd(TsCb cb) { return doOutsideOfContext(CF_DisallowConditionalTypes, cb); }
static intptr_t disallowConditionalTypesAnd(TsCb cb) { return doInsideOfContext(CF_DisallowConditionalTypes, cb); }
static intptr_t doInYieldContext(TsCb cb) { return doInsideOfContext(CF_Yield, cb); }
static intptr_t doInDecoratorContext(TsCb cb) { return doInsideOfContext(CF_Decorator, cb); }
static intptr_t doInAwaitContext(TsCb cb) { return doInsideOfContext(CF_Await, cb); }
static intptr_t doOutsideOfAwaitContext(TsCb cb) { return doOutsideOfContext(CF_Await, cb); }
static intptr_t doInYieldAndAwaitContext(TsCb cb) { return doInsideOfContext(CF_Yield | CF_Await, cb); }
static intptr_t doOutsideOfYieldAndAwaitContext(TsCb cb) { return doOutsideOfContext(CF_Yield | CF_Await, cb); }

static bool inContext(int flags) { return (P.contextFlags & flags) != 0; }
static bool inYieldContext(void) { return inContext(CF_Yield); }
static bool inDisallowInContext(void) { return inContext(CF_DisallowIn); }
static bool inDisallowConditionalTypesContext(void) { return inContext(CF_DisallowConditionalTypes); }
static bool inDecoratorContext(void) { return inContext(CF_Decorator); }
static bool inAwaitContext(void) { return inContext(CF_Await); }

/* ---- tokens ---- */
static int getNodePos(void) { return TS.fullStart; }
static bool hasPrecedingJSDocComment(void) { return (TS.tflags & TF_PrecedingJSDocComment) != 0; }
static int token(void) { return P.currentToken; }

static int nextTokenWithoutCheck(void) { return P.currentToken = ts_scan(); }

static bool isKeywordKind(int k) { return k >= TSK_FirstKeyword && k <= TSK_LastKeyword; }

static int nextToken(void) {
  if (isKeywordKind(P.currentToken) && (TS.tflags & (TF_UnicodeEscape | TF_ExtendedUnicodeEscape))) {
    parseErrorAtPosition(TS.tokenStart);
  }
  return nextTokenWithoutCheck();
}
static int nextTokenJSDoc(void) { return P.currentToken = ts_scanJsDocToken(); }
static int reScanGreaterToken(void) { return P.currentToken = ts_reScanGreaterToken(); }
static int reScanSlashToken(void) { return P.currentToken = ts_reScanSlashToken(); }
static int reScanTemplateToken(void) { return P.currentToken = ts_reScanTemplateToken(); }
static int reScanLessThanToken(void) { return P.currentToken = ts_reScanLessThanToken(); }
static int reScanHashToken(void) { return P.currentToken = ts_reScanHashToken(); }

/* speculationHelper: kind 0 = tryParse (keeps scanner and errors on success), 1 = lookAhead */
static intptr_t speculationHelper(TsCb cb, bool isLookahead) {
  int saveToken = P.currentToken;
  int saveErr = P.nerr;
  bool saveErrBefore = P.errBeforeNextFinishedNode;
  TsScanState ss = ts_scanner_save();
  intptr_t result = cb();
  if (!result || isLookahead) ts_scanner_restore(&ss);
  if (!result || isLookahead) {
    P.currentToken = saveToken;
    P.nerr = saveErr;
    P.errBeforeNextFinishedNode = saveErrBefore;
  }
  return result;
}
static intptr_t lookAhead(TsCb cb) { return speculationHelper(cb, true); }
static intptr_t tryParse(TsCb cb) { return speculationHelper(cb, false); }

static bool isBindingIdentifier(void) {
  if (token() == TSK_Identifier) return true;
  return token() > TSK_LastReservedWord;
}

static bool isIdentifier2(void) {
  if (token() == TSK_Identifier) return true;
  if (token() == TSK_YieldKeyword && inYieldContext()) return false;
  if (token() == TSK_AwaitKeyword && inAwaitContext()) return false;
  return token() > TSK_LastReservedWord;
}

static bool parseExpected(int kind) {
  if (token() == kind) {
    nextToken();
    return true;
  }
  parseErrorAtCurrentToken();
  return false;
}

static bool parseExpectedNoAdvance(int kind) {
  if (token() == kind) return true;
  parseErrorAtCurrentToken();
  return false;
}

static bool parseOptional(int t) {
  if (token() == t) {
    nextToken();
    return true;
  }
  return false;
}

static TsNode *parseTokenNode(void) {
  int pos = getNodePos();
  int kind = token();
  nextToken();
  TsNode *n = newNode(kind);
  return finishNode(n, pos, -1);
}

static TsNode *parseOptionalToken(int t) {
  if (token() == t) return parseTokenNode();
  return NULL;
}

static bool canParseSemicolon(void) {
  if (token() == TSK_SemicolonToken) return true;
  return token() == TSK_CloseBraceToken || token() == TSK_EndOfFileToken || (TS.tflags & TF_PrecedingLineBreak) != 0;
}

static bool tryParseSemicolon(void) {
  if (!canParseSemicolon()) return false;
  if (token() == TSK_SemicolonToken) nextToken();
  return true;
}

static bool parseSemicolon(void) { return tryParseSemicolon() || parseExpected(TSK_SemicolonToken); }

/* createNodeArray(elements) is ts_list_take; the end defaults to the scanner's full start */
static TsList *createNodeArrayFrom(int mark, int pos, int end, bool trailingComma) {
  if (end < 0) end = TS.fullStart;
  return ts_list_take(mark, pos, end, trailingComma);
}

/* finishNode: setTextRangePosEnd(node, pos, end ?? getTokenFullStart()), context flags, error flag */
static TsNode *finishNode(TsNode *node, int pos, int end) {
  node->pos = pos;
  node->end = end < 0 ? TS.fullStart : end;
  if (P.contextFlags) node->flags |= (uint32_t)P.contextFlags;
  if (P.errBeforeNextFinishedNode) {
    P.errBeforeNextFinishedNode = false;
    node->flags |= TNF_ThisNodeHasError;
  }
  return node;
}

/* identifiers: the text is a raw slice of the source (escapes are not decoded) */
static TsNode *newIdentifier(const char *text, int len, int originalKeyword) {
  TsNode *n = newNode(TSK_Identifier);
  n->text = text;
  n->textLen = len;
  n->extra = originalKeyword;
  return n;
}

/* createMissingNode(kind, reportAtCurrentPosition, ...) */
static TsNode *createMissingNode(int kind, bool reportAtCurrentPosition) {
  if (reportAtCurrentPosition) parseErrorAtPosition(TS.fullStart);
  else parseErrorAtCurrentToken();
  int pos = getNodePos();
  TsNode *result;
  if (kind == TSK_Identifier) {
    result = newIdentifier("", 0, 0);
  } else if (kind == TSK_TemplateHead || kind == TSK_TemplateMiddle || kind == TSK_TemplateTail || kind == TSK_NoSubstitutionTemplateLiteral) {
    result = newNode(kind);
  } else if (kind == TSK_NumericLiteral || kind == TSK_StringLiteral) {
    result = newNode(kind);
  } else if (kind == TSK_MissingDeclaration) {
    result = newNode(kind);
  } else {
    result = newNode(kind);
  }
  return finishNode(result, pos, -1);
}

/* parseExpectedToken(t): token node if present, else a missing token node */
static TsNode *parseExpectedToken(int t) {
  TsNode *n = parseOptionalToken(t);
  if (n) return n;
  return createMissingNode(t, false);
}

/* createIdentifier(isIdentifier, ...) */
static TsNode *createIdentifier(bool isIdentifier) {
  if (isIdentifier) {
    P.identifierCount++;
    int pos = (TS.tflags & TF_PrecedingJSDocLeadingAsterisks) ? TS.tokenStart : getNodePos();
    int originalKeywordKind = token();
    const char *text = TS.tval;
    int len = TS.tvalLen;
    nextTokenWithoutCheck();
    return finishNode(newIdentifier(text, len, originalKeywordKind), pos, -1);
  }
  if (token() == TSK_PrivateIdentifier) {
    parseErrorAtCurrentToken();
    return createIdentifier(true);
  }
  if (token() == TSK_Unknown && ts_tryScan_reScanInvalidIdentifier()) {
    return createIdentifier(true);
  }
  P.identifierCount++;
  bool reportAtCurrentPosition = token() == TSK_EndOfFileToken;
  return createMissingNode(TSK_Identifier, reportAtCurrentPosition);
}

static TsNode *parseBindingIdentifier(void) { return createIdentifier(isBindingIdentifier()); }
static TsNode *parseIdentifier(void) { return createIdentifier(isIdentifier2()); }
static TsNode *parseIdentifierName(void) { return createIdentifier(tokenIsIdentifierOrKeyword(token())); }
static TsNode *parseIdentifierNameErrorOnUnicodeEscapeSequence(void) {
  if (TS.tflags & (TF_UnicodeEscape | TF_ExtendedUnicodeEscape)) parseErrorAtCurrentToken();
  return createIdentifier(tokenIsIdentifierOrKeyword(token()));
}


/* ======================= part 2: lists, names, templates, literals, type references ======================= */


/* parseList(kind, parseElement): elements are gathered on the node stack */
static TsList *parseList(int kind, TsParseFn parseElement);
static TsList *parseDelimitedList(int kind, TsParseFn parseElement, bool considerSemicolonAsDelimiter);

static TsNode *parseListElement(TsParseFn parseElement) { return parseElement(); }

static TsList *parseList(int kind, TsParseFn parseElement) {
  int saveParsingContext = P.parsingContext;
  P.parsingContext |= 1 << kind;
  int mark = ts_stk_n;
  int listPos = getNodePos();
  while (!isListTerminator(kind)) {
    if (isListElement2(kind, false)) {
      ts_stk_push(parseListElement(parseElement));
      continue;
    }
    if (abortParsingListOrMoveToNextToken(kind)) break;
  }
  P.parsingContext = saveParsingContext;
  return createNodeArrayFrom(mark, listPos, -1, false);
}

/* parseDelimitedList: returns NULL is never produced without syntax-cursor reuse */
static TsList *parseDelimitedList(int kind, TsParseFn parseElement, bool considerSemicolonAsDelimiter) {
  int saveParsingContext = P.parsingContext;
  P.parsingContext |= 1 << kind;
  int mark = ts_stk_n;
  int listPos = getNodePos();
  int commaStart = -1;
  for (;;) {
    if (isListElement2(kind, false)) {
      int startPos = TS.fullStart;
      ts_stk_push(parseListElement(parseElement));
      commaStart = TS.tokenStart;
      if (parseOptional(TSK_CommaToken)) continue;
      commaStart = -1;
      if (isListTerminator(kind)) break;
      parseExpected(TSK_CommaToken);
      if (considerSemicolonAsDelimiter && token() == TSK_SemicolonToken && !(TS.tflags & TF_PrecedingLineBreak)) {
        nextToken();
      }
      if (startPos == TS.fullStart) nextToken();
      continue;
    }
    if (isListTerminator(kind)) break;
    if (abortParsingListOrMoveToNextToken(kind)) break;
  }
  P.parsingContext = saveParsingContext;
  return createNodeArrayFrom(mark, listPos, -1, commaStart >= 0);
}

static TsList *createMissingList(void) {
  TsList *l = ts_list_empty(getNodePos(), TS.fullStart);
  l->isMissingList = true;
  return l;
}

static TsList *parseBracketedList(int kind, TsParseFn parseElement, int open, int close) {
  if (parseExpected(open)) {
    TsList *result = parseDelimitedList(kind, parseElement, false);
    parseExpected(close);
    return result;
  }
  return createMissingList();
}

static TsNode *parseRightSideOfDot(bool allowIdentifierNames, bool allowPrivateIdentifiers, bool allowUnicodeEscapeSequenceInIdentifierName);

/* parseEntityName(allowReservedWords) */
static TsNode *parseEntityName(bool allowReservedWords) {
  int pos = getNodePos();
  TsNode *entity = allowReservedWords ? parseIdentifierName() : parseIdentifier();
  while (parseOptional(TSK_DotToken)) {
    if (token() == TSK_LessThanToken) break;
    TsNode *q = newNode(TSK_QualifiedName);
    q->left = entity;
    q->right = parseRightSideOfDot(allowReservedWords, false, true);
    entity = finishNode(q, pos, -1);
  }
  return entity;
}

/* parseRightSideOfDot */
static TsNode *parseRightSideOfDot(bool allowIdentifierNames, bool allowPrivateIdentifiers, bool allowUnicodeEscapeSequenceInIdentifierName) {
  if ((TS.tflags & TF_PrecedingLineBreak) && tokenIsIdentifierOrKeyword(token())) {
    bool matchesPattern = lookAhead(cb_nextTokenIsIdentifierOrKeywordOnSameLine) != 0;
    if (matchesPattern) return createMissingNode(TSK_Identifier, true);
  }
  if (token() == TSK_PrivateIdentifier) {
    TsNode *node = parsePrivateIdentifier();
    return allowPrivateIdentifiers ? node : createMissingNode(TSK_Identifier, true);
  }
  if (allowIdentifierNames) {
    return allowUnicodeEscapeSequenceInIdentifierName ? parseIdentifierName() : parseIdentifierNameErrorOnUnicodeEscapeSequence();
  }
  return parseIdentifier();
}

/* parseTemplateSpans / parseTemplateExpression / parseTemplateType / parseTemplateTypeSpans */

static TsNode *parseLiteralOfTemplateSpan(bool isTaggedTemplate) {
  if (token() == TSK_CloseBraceToken) {
    reScanTemplateToken();
    return parseTemplateMiddleOrTemplateTail();
  }
  return parseExpectedToken(TSK_TemplateTail);
}

static TsNode *parseTemplateSpan(bool isTaggedTemplate) {
  int pos = getNodePos();
  TsNode *n = newNode(TSK_TemplateSpan);
  n->expression = (TsNode *)allowInAnd(cb_parseExpression);
  n->literal = parseLiteralOfTemplateSpan(isTaggedTemplate);
  return finishNode(n, pos, -1);
}

static TsNode *parseTemplateTypeSpan(void) {
  int pos = getNodePos();
  TsNode *n = newNode(TSK_TemplateLiteralTypeSpan);
  n->type = parseType();
  n->literal = parseLiteralOfTemplateSpan(false);
  return finishNode(n, pos, -1);
}

/* parseTemplateSpans: loop while the previous literal was a middle */
static TsList *parseTemplateSpansCore(bool isTaggedTemplate, bool typeSpans) {
  int pos = getNodePos();
  int mark = ts_stk_n;
  TsNode *node;
  do {
    node = typeSpans ? parseTemplateTypeSpan() : parseTemplateSpan(isTaggedTemplate);
    ts_stk_push(node);
  } while (node->literal->kind == TSK_TemplateMiddle);
  return createNodeArrayFrom(mark, pos, -1, false);
}

static TsNode *parseTemplateHead(bool isTaggedTemplate);

static TsNode *parseTemplateExpression(bool isTaggedTemplate) {
  int pos = getNodePos();
  TsNode *head = parseTemplateHead(isTaggedTemplate);
  TsNode *n = newNode(TSK_TemplateExpression);
  n->head = head;
  n->spans = parseTemplateSpansCore(isTaggedTemplate, false);
  return finishNode(n, pos, -1);
}

static TsNode *parseTemplateType(void) {
  int pos = getNodePos();
  TsNode *head = parseTemplateHead(false);
  TsNode *n = newNode(TSK_TemplateLiteralType);
  n->head = head;
  n->spans = parseTemplateSpansCore(false, true);
  return finishNode(n, pos, -1);
}

/* parseTemplateMiddleOrTemplateTail / parseTemplateHead */
static TsNode *parseTemplateMiddleOrTemplateTail(void) { return parseLiteralLikeNode(token()); }

static TsNode *parseTemplateHead(bool isTaggedTemplate) {
  if (!isTaggedTemplate && (TS.tflags & 26656)) reScanTemplateToken();
  return parseLiteralLikeNode(token());
}

/* parseLiteralLikeNode: string, numeric, bigint, template and regex literals */
static bool isTemplateLiteralKind(int k) { return k >= TSK_NoSubstitutionTemplateLiteral && k <= TSK_TemplateTail; }
static bool isLiteralKind(int k) { return k >= TSK_FirstLiteralToken && k <= TSK_LastLiteralToken; }

static TsNode *parseLiteralLikeNode(int kind) {
  int pos = getNodePos();
  TsNode *node = newNode(kind);
  node->text = TS.tval;
  node->textLen = TS.tvalLen;
  node->tokenFlags = TS.tflags;
  if (kind == TSK_NumericLiteral) node->tokenFlags = TS.tflags & 25584;
  nextToken();
  if (TS.tflags & TF_Unterminated) node->tokenFlags |= TF_Unterminated;
  return finishNode(node, pos, -1);
}

static TsNode *parseLiteralNode(void) { return parseLiteralLikeNode(token()); }

static TsNode *parseTypeReference(void);

/* ======================= part 3: parameters, type members, tuples, function types ======================= */

/* callback adapters: doXContext(cb) takes an intptr_t thunk; these return node pointers as intptr_t */
static intptr_t cb_parseExpression(void) { return (intptr_t)parseExpression(); }
static intptr_t cb_parseType(void) { return (intptr_t)parseType(); }
static intptr_t cb_parseTypeOrTypePredicate(void) { return (intptr_t)parseTypeOrTypePredicate(); }
static intptr_t cb_parseUnaryExpressionOrHigher(void) { return (intptr_t)parseUnaryExpressionOrHigher(); }
static intptr_t cb_parseModifiersDecorators(void) { return (intptr_t)parseModifiers(true, false, false); }
static intptr_t cb_parseModifiersPlain(void) { return (intptr_t)parseModifiers(false, false, false); }
static intptr_t cb_parseStatement(void) { return (intptr_t)parseStatement(); }
static intptr_t cb_parseIdentifierName(void) { return (intptr_t)parseIdentifierName(); }

static TsNode *parseParameterWorker(bool inOuterAwaitContext, bool allowAmbiguity);

static TsNode *parseParameter(bool inOuterAwaitContext) { return parseParameterWorker(inOuterAwaitContext, true); }
static TsNode *parseParameterForSpeculation(bool inOuterAwaitContext) { return parseParameterWorker(inOuterAwaitContext, false); }
static TsNode *parseParameterDefaultFalse(void) { return parseParameter(false); }
static TsNode *parseParameterForSpeculationFalse(void) { return parseParameterForSpeculation(false); }

static TsNode *parseParameterWorker(bool inOuterAwaitContext, bool allowAmbiguity) {
  int pos = getNodePos();
  bool hasJSDoc = hasPrecedingJSDocComment();
  TsList *modifiers = (TsList *)(inOuterAwaitContext ? doInAwaitContext(cb_parseModifiersDecorators)
                                                       : doOutsideOfAwaitContext(cb_parseModifiersDecorators));
  if (token() == TSK_ThisKeyword) {
    TsNode *node2 = newNode(TSK_Parameter);
    node2->modifiers = modifiers;
    node2->name = createIdentifier(true);
    node2->type = parseTypeAnnotation();
    TsNode *modifier = (modifiers && modifiers->n) ? modifiers->v[0] : NULL;
    if (modifier) parseErrorAtPosition(modifier->pos);
    return withJSDoc(finishNode(node2, pos, -1), hasJSDoc);
  }
  bool savedTopLevel = P.topLevel;
  P.topLevel = false;
  TsNode *dotDotDotToken = parseOptionalToken(TSK_DotDotDotToken);
  if (!allowAmbiguity && !isParameterNameStart()) return NULL;
  TsNode *node = newNode(TSK_Parameter);
  node->modifiers = modifiers;
  node->dotDotDotToken = dotDotDotToken;
  node->name = parseNameOfParameter(modifiers);
  node->questionToken = parseOptionalToken(TSK_QuestionToken);
  node->type = parseTypeAnnotation();
  node->initializer = parseInitializer();
  finishNode(node, pos, -1);
  node = withJSDoc(node, hasJSDoc);
  P.topLevel = savedTopLevel;
  return node;
}

/* parseNameOfParameter: a modifier keyword used as the name is skipped when the name is empty */
static TsNode *parseNameOfParameter(TsList *modifiers) {
  TsNode *name = parseIdentifierOrPattern();
  if (name->end - name->pos == 0 && !(modifiers && modifiers->n) && isModifierKind(token())) {
    nextToken();
  }
  return name;
}

static bool isParameterNameStart(void) {
  return isBindingIdentifier() || token() == TSK_OpenBracketToken || token() == TSK_OpenBraceToken;
}

static TsNode *parseJSDocParameter(void);

static bool shouldParseReturnType(int returnToken, bool isType) {
  if (returnToken == TSK_EqualsGreaterThanToken) {
    parseExpected(returnToken);
    return true;
  } else if (parseOptional(TSK_ColonToken)) {
    return true;
  } else if (isType && token() == TSK_EqualsGreaterThanToken) {
    parseErrorAtCurrentToken();
    nextToken();
    return true;
  }
  return false;
}

static TsNode *parseReturnType(int returnToken, bool isType) {
  if (shouldParseReturnType(returnToken, isType)) {
    return (TsNode *)allowConditionalTypesAnd(cb_parseTypeOrTypePredicate);
  }
  return NULL;
}

static TsNode *parseParameterSpecArg(bool allowAmbiguity) {
  (void)allowAmbiguity;
  return NULL;
}

/* parseParametersWorker(flags, allowAmbiguity): flags 1 yield, 2 await, 32 JSDoc parameters */
static TsList *parseParametersWorker(int flags, bool allowAmbiguity) {
  bool savedYieldContext = inYieldContext();
  bool savedAwaitContext = inAwaitContext();
  setYieldContext((flags & 1) != 0);
  setAwaitContext((flags & 2) != 0);
  TsList *parameters;
  if (flags & 32) {
    parameters = parseDelimitedList(PC_JSDocParameters, parseJSDocParameter, false);
  } else if (allowAmbiguity) {
    parameters = parseDelimitedList(PC_Parameters, parseParameterDefaultFalse, false);
  } else {
    parameters = parseDelimitedList(PC_Parameters, parseParameterForSpeculationFalse, false);
  }
  setYieldContext(savedYieldContext);
  setAwaitContext(savedAwaitContext);
  return parameters;
}

static TsList *parseParameters(int flags) {
  if (!parseExpected(TSK_OpenParenToken)) return createMissingList();
  TsList *parameters = parseParametersWorker(flags, true);
  parseExpected(TSK_CloseParenToken);
  return parameters;
}

static void parseTypeMemberSemicolon(void) {
  if (parseOptional(TSK_CommaToken)) return;
  parseSemicolon();
}

static TsNode *parseSignatureMember(int kind) {
  int pos = getNodePos();
  bool hasJSDoc = hasPrecedingJSDocComment();
  if (kind == TSK_ConstructSignature) parseExpected(TSK_NewKeyword);
  TsList *typeParameters = parseTypeParameters();
  TsList *parameters = parseParameters(4);
  TsNode *type = parseReturnType(TSK_ColonToken, true);
  parseTypeMemberSemicolon();
  TsNode *node = newNode(kind);
  node->typeParameters = typeParameters;
  node->parameters = parameters;
  node->type = type;
  return withJSDoc(finishNode(node, pos, -1), hasJSDoc);
}

static bool isUnambiguouslyIndexSignature(void) {
  nextToken();
  if (token() == TSK_DotDotDotToken || token() == TSK_CloseBracketToken) return true;
  if (isModifierKind(token())) {
    nextToken();
    if (isIdentifier2()) return true;
  } else if (!isIdentifier2()) {
    return false;
  } else {
    nextToken();
  }
  if (token() == TSK_ColonToken || token() == TSK_CommaToken) return true;
  if (token() != TSK_QuestionToken) return false;
  nextToken();
  return token() == TSK_ColonToken || token() == TSK_CommaToken || token() == TSK_CloseBracketToken;
}

static bool isIndexSignature(void) {
  return token() == TSK_OpenBracketToken && lookAhead(cb_isUnambiguouslyIndexSignature) != 0;
}

/* ======================= part 4: import types, non-array types, union/intersection, predicates ======================= */

static TsNode *parseImportType(void) {
  P.sourceFlags |= 4194304;
  int pos = getNodePos();
  bool isTypeOf = parseOptional(TSK_TypeOfKeyword);
  parseExpected(TSK_ImportKeyword);
  parseExpected(TSK_OpenParenToken);
  TsNode *type = parseType();
  TsList *attributes = NULL;
  if (parseOptional(TSK_CommaToken)) {
    parseExpected(TSK_OpenBraceToken);
    int currentToken2 = token();
    if (currentToken2 == TSK_WithKeyword || currentToken2 == TSK_AssertKeyword) {
      nextToken();
    } else {
      parseErrorAtCurrentToken();
    }
    parseExpected(TSK_ColonToken);
    attributes = parseImportAttributes(currentToken2, true);
    parseOptional(TSK_CommaToken);
    parseExpected(TSK_CloseBraceToken);
  }
  parseExpected(TSK_CloseParenToken);
  TsNode *qualifier = parseOptional(TSK_DotToken) ? parseEntityNameOfTypeReference() : NULL;
  TsList *typeArguments = parseTypeArgumentsOfTypeReference();
  TsNode *n = newNode(TSK_ImportType);
  n->argument = type;
  n->attributes_node = NULL; n->attributes = attributes;
  n->qualifier = qualifier;
  n->typeArguments = typeArguments;
  n->extra = isTypeOf;
  return finishNode(n, pos, -1);
}

static bool nextTokenIsNumericOrBigIntLiteral(void) {
  nextToken();
  return token() == TSK_NumericLiteral || token() == TSK_BigIntLiteral;
}

static intptr_t cb_nextTokenIsNumericOrBigIntLiteral(void) { return nextTokenIsNumericOrBigIntLiteral(); }
static intptr_t cb_parseKeywordAndNoDot(void) { return (intptr_t)parseKeywordAndNoDot(); }
static intptr_t cb_isStartOfTypeOfImportType(void) { return isStartOfTypeOfImportType(); }
static intptr_t cb_isStartOfMappedType(void) { return isStartOfMappedType(); }
static intptr_t cb_nextTokenIsStartOfType(void) { return nextTokenIsStartOfType(); }
static intptr_t cb_isStartOfParenthesizedOrFunctionType(void) { return isStartOfParenthesizedOrFunctionType(); }

static TsNode *parseNonArrayType(void) {
  switch (token()) {
    case TSK_AnyKeyword: case TSK_UnknownKeyword: case TSK_StringKeyword: case TSK_NumberKeyword:
    case TSK_BigIntKeyword: case TSK_SymbolKeyword: case TSK_BooleanKeyword: case TSK_UndefinedKeyword:
    case TSK_NeverKeyword: case TSK_ObjectKeyword: {
      TsNode *k = (TsNode *)tryParse(cb_parseKeywordAndNoDot);
      return k ? k : parseTypeReference();
    }
    case TSK_AsteriskEqualsToken:
      ts_reScanAsteriskEqualsToken();
      /* falls through */
    case TSK_AsteriskToken:
      return parseJSDocAllType();
    case TSK_QuestionQuestionToken:
      ts_reScanQuestionToken();
      /* falls through */
    case TSK_QuestionToken:
      return parseJSDocUnknownOrNullableType();
    case TSK_FunctionKeyword:
      return parseJSDocFunctionType();
    case TSK_ExclamationToken:
      return parseJSDocNonNullableType();
    case TSK_NoSubstitutionTemplateLiteral: case TSK_StringLiteral: case TSK_NumericLiteral:
    case TSK_BigIntLiteral: case TSK_TrueKeyword: case TSK_FalseKeyword: case TSK_NullKeyword:
      return parseLiteralTypeNode(false);
    case TSK_MinusToken:
      return lookAhead(cb_nextTokenIsNumericOrBigIntLiteral) ? parseLiteralTypeNode(true) : parseTypeReference();
    case TSK_VoidKeyword:
      return parseTokenNode();
    case TSK_ThisKeyword: {
      TsNode *thisKeyword = parseThisTypeNode();
      if (token() == TSK_IsKeyword && !(TS.tflags & TF_PrecedingLineBreak)) {
        return parseThisTypePredicate(thisKeyword);
      }
      return thisKeyword;
    }
    case TSK_TypeOfKeyword:
      return lookAhead(cb_isStartOfTypeOfImportType) ? parseImportType() : parseTypeQuery();
    case TSK_OpenBraceToken:
      return lookAhead(cb_isStartOfMappedType) ? parseMappedType() : parseTypeLiteral();
    case TSK_OpenBracketToken:
      return parseTupleType();
    case TSK_OpenParenToken:
      return parseParenthesizedType();
    case TSK_ImportKeyword:
      return parseImportType();
    case TSK_AssertsKeyword:
      return lookAhead(cb_nextTokenIsIdentifierOrKeywordOnSameLine) ? parseAssertsTypePredicate() : parseTypeReference();
    case TSK_TemplateHead:
      return parseTemplateType();
    default:
      return parseTypeReference();
  }
}

static bool isStartOfType(bool inStartOfParameter) {
  switch (token()) {
    case TSK_AnyKeyword: case TSK_UnknownKeyword: case TSK_StringKeyword: case TSK_NumberKeyword:
    case TSK_BigIntKeyword: case TSK_BooleanKeyword: case TSK_ReadonlyKeyword: case TSK_SymbolKeyword:
    case TSK_UniqueKeyword: case TSK_VoidKeyword: case TSK_UndefinedKeyword: case TSK_NullKeyword:
    case TSK_ThisKeyword: case TSK_TypeOfKeyword: case TSK_NeverKeyword: case TSK_OpenBraceToken:
    case TSK_OpenBracketToken: case TSK_LessThanToken: case TSK_BarToken: case TSK_AmpersandToken:
    case TSK_NewKeyword: case TSK_StringLiteral: case TSK_NumericLiteral: case TSK_BigIntLiteral:
    case TSK_TrueKeyword: case TSK_FalseKeyword: case TSK_ObjectKeyword: case TSK_AsteriskToken:
    case TSK_QuestionToken: case TSK_ExclamationToken: case TSK_DotDotDotToken: case TSK_InferKeyword:
    case TSK_ImportKeyword: case TSK_AssertsKeyword: case TSK_NoSubstitutionTemplateLiteral:
    case TSK_TemplateHead:
      return true;
    case TSK_FunctionKeyword:
      return !inStartOfParameter;
    case TSK_MinusToken:
      return !inStartOfParameter && lookAhead(cb_nextTokenIsNumericOrBigIntLiteral) != 0;
    case TSK_OpenParenToken:
      return !inStartOfParameter && lookAhead(cb_isStartOfParenthesizedOrFunctionType) != 0;
    default:
      return isIdentifier2();
  }
}

static bool isStartOfParenthesizedOrFunctionType(void) {
  nextToken();
  return token() == TSK_CloseParenToken || isStartOfParameter(false) || isStartOfType(false);
}

static TsNode *parsePostfixTypeOrHigher(void) {
  int pos = getNodePos();
  TsNode *type = parseNonArrayType();
  while (!(TS.tflags & TF_PrecedingLineBreak)) {
    switch (token()) {
      case TSK_ExclamationToken: {
        nextToken();
        TsNode *n = newNode(TSK_JSDocNonNullableType);
        n->type = type;
        n->extra = 1;  /* postfix */
        type = finishNode(n, pos, -1);
        break;
      }
      case TSK_QuestionToken: {
        if (lookAhead(cb_nextTokenIsStartOfType)) return type;
        nextToken();
        TsNode *n = newNode(TSK_JSDocNullableType);
        n->type = type;
        n->extra = 1;
        type = finishNode(n, pos, -1);
        break;
      }
      case TSK_OpenBracketToken:
        parseExpected(TSK_OpenBracketToken);
        if (isStartOfType(false)) {
          TsNode *indexType = parseType();
          parseExpected(TSK_CloseBracketToken);
          TsNode *n = newNode(TSK_IndexedAccessType);
          n->objectType = type;
          n->indexType = indexType;
          type = finishNode(n, pos, -1);
        } else {
          parseExpected(TSK_CloseBracketToken);
          TsNode *n = newNode(TSK_ArrayType);
          n->elementType = type;
          type = finishNode(n, pos, -1);
        }
        break;
      default:
        return type;
    }
  }
  return type;
}

static TsNode *parseTypeOperator(int operator_) {
  int pos = getNodePos();
  parseExpected(operator_);
  TsNode *n = newNode(TSK_TypeOperator);
  n->extra = operator_;
  n->type = parseTypeOperatorOrHigher();
  return finishNode(n, pos, -1);
}

static TsNode *tryParseConstraintOfInferType(void);
static intptr_t cb_tryParseConstraintOfInferType(void) { return (intptr_t)tryParseConstraintOfInferType(); }
static intptr_t cb_parseTypeOp(void) { return (intptr_t)parseType(); }

static TsNode *tryParseConstraintOfInferType(void) {
  if (parseOptional(TSK_ExtendsKeyword)) {
    TsNode *constraint = (TsNode *)disallowConditionalTypesAnd(cb_parseTypeOp);
    if (inDisallowConditionalTypesContext() || token() != TSK_QuestionToken) return constraint;
  }
  return NULL;
}

static TsNode *parseTypeParameterOfInferType(void) {
  int pos = getNodePos();
  TsNode *name = parseIdentifier();
  TsNode *constraint = (TsNode *)tryParse(cb_tryParseConstraintOfInferType);
  TsNode *node = newNode(TSK_TypeParameter);
  node->name = name;
  node->constraint = constraint;
  return finishNode(node, pos, -1);
}

static TsNode *parseInferType(void) {
  int pos = getNodePos();
  parseExpected(TSK_InferKeyword);
  TsNode *n = newNode(TSK_InferType);
  n->typeParameter = parseTypeParameterOfInferType();
  return finishNode(n, pos, -1);
}

static TsNode *parseTypeOperatorOrHigher(void) {
  int operator_ = token();
  switch (operator_) {
    case TSK_KeyOfKeyword:
    case TSK_UniqueKeyword:
    case TSK_ReadonlyKeyword:
      return parseTypeOperator(operator_);
    case TSK_InferKeyword:
      return parseInferType();
  }
  return (TsNode *)allowConditionalTypesAnd(cb_parsePostfixTypeOrHigher);
}

static TsNode *parseFunctionOrConstructorTypeToError(bool isInUnionType) {
  if (isStartOfFunctionTypeOrConstructorType()) {
    TsNode *type = parseFunctionOrConstructorType();
    parseErrorAtPosition(type->pos);
    return type;
  }
  return NULL;
}

/* parseUnionOrIntersectionType(operator, parseConstituentType, createTypeNode) */
static TsNode *parseUnionOrIntersectionType(int operator_, TsParseFn parseConstituentType, int createKind) {
  int pos = getNodePos();
  bool isUnionType = operator_ == TSK_BarToken;
  bool hasLeadingOperator = parseOptional(operator_);
  TsNode *type = NULL;
  if (hasLeadingOperator) type = parseFunctionOrConstructorTypeToError(isUnionType);
  if (!type) type = parseConstituentType();
  if (token() == operator_ || hasLeadingOperator) {
    int mark = ts_stk_n;
    ts_stk_push(type);
    while (parseOptional(operator_)) {
      TsNode *t = parseFunctionOrConstructorTypeToError(isUnionType);
      ts_stk_push(t ? t : parseConstituentType());
    }
    TsNode *n = newNode(createKind);
    n->types = createNodeArrayFrom(mark, pos, -1, false);
    type = finishNode(n, pos, -1);
  }
  return type;
}

static TsNode *parseIntersectionTypeOrHigher(void) {
  return parseUnionOrIntersectionType(TSK_AmpersandToken, parseTypeOperatorOrHigher, TSK_IntersectionType);
}
static TsNode *parseUnionTypeOrHigher(void) {
  return parseUnionOrIntersectionType(TSK_BarToken, parseIntersectionTypeOrHigher, TSK_UnionType);
}

static bool nextTokenIsNewKeyword(void) {
  nextToken();
  return token() == TSK_NewKeyword;
}
static intptr_t cb_nextTokenIsNewKeyword(void) { return nextTokenIsNewKeyword(); }

static bool isStartOfFunctionTypeOrConstructorType(void) {
  if (token() == TSK_LessThanToken) return true;
  if (token() == TSK_OpenParenToken && lookAhead(cb_isUnambiguouslyStartOfFunctionType)) return true;
  return token() == TSK_NewKeyword || (token() == TSK_AbstractKeyword && lookAhead(cb_nextTokenIsNewKeyword));
}

static bool skipParameterStart(void) {
  if (isModifierKind(token())) parseModifiers(false, false, false);
  if (isIdentifier2() || token() == TSK_ThisKeyword) {
    nextToken();
    return true;
  }
  if (token() == TSK_OpenBracketToken || token() == TSK_OpenBraceToken) {
    int previousErrorCount = P.nerr;
    parseIdentifierOrPattern();
    return previousErrorCount == P.nerr;
  }
  return false;
}

static bool isUnambiguouslyStartOfFunctionType(void) {
  nextToken();
  if (token() == TSK_CloseParenToken || token() == TSK_DotDotDotToken) return true;
  if (skipParameterStart()) {
    if (token() == TSK_ColonToken || token() == TSK_CommaToken || token() == TSK_QuestionToken || token() == TSK_EqualsToken) return true;
    if (token() == TSK_CloseParenToken) {
      nextToken();
      if (token() == TSK_EqualsGreaterThanToken) return true;
    }
  }
  return false;
}
static intptr_t cb_isUnambiguouslyStartOfFunctionType(void) { return isUnambiguouslyStartOfFunctionType(); }

static TsNode *parseTypePredicatePrefix(void) {
  TsNode *id = parseIdentifier();
  if (token() == TSK_IsKeyword && !(TS.tflags & TF_PrecedingLineBreak)) {
    nextToken();
    return id;
  }
  return NULL;
}
static intptr_t cb_parseTypePredicatePrefix(void) { return (intptr_t)parseTypePredicatePrefix(); }

static TsNode *parseTypeOrTypePredicate(void) {
  int pos = getNodePos();
  TsNode *typePredicateVariable = isIdentifier2() ? (TsNode *)tryParse(cb_parseTypePredicatePrefix) : NULL;
  TsNode *type = parseType();
  if (typePredicateVariable) {
    TsNode *n = newNode(TSK_TypePredicate);
    n->parameterName = typePredicateVariable;
    n->type = type;
    return finishNode(n, pos, -1);
  }
  return type;
}

static TsNode *parseAssertsTypePredicate(void) {
  int pos = getNodePos();
  TsNode *assertsModifier = parseExpectedToken(TSK_AssertsKeyword);
  TsNode *parameterName = token() == TSK_ThisKeyword ? parseThisTypeNode() : parseIdentifier();
  TsNode *type = parseOptional(TSK_IsKeyword) ? parseType() : NULL;
  TsNode *n = newNode(TSK_TypePredicate);
  n->assertsModifier = assertsModifier;
  n->parameterName = parameterName;
  n->type = type;
  return finishNode(n, pos, -1);
}

/* parseType: TypeExcludesFlags (81920) are cleared while parsing a type */
static TsNode *parseType(void) {
  if (P.contextFlags & 81920) return (TsNode *)doOutsideOfContext(81920, cb_parseType);
  if (isStartOfFunctionTypeOrConstructorType()) return parseFunctionOrConstructorType();
  int pos = getNodePos();
  TsNode *type = parseUnionTypeOrHigher();
  if (!inDisallowConditionalTypesContext() && !(TS.tflags & TF_PrecedingLineBreak) && parseOptional(TSK_ExtendsKeyword)) {
    TsNode *extendsType = (TsNode *)disallowConditionalTypesAnd(cb_parseType);
    parseExpected(TSK_QuestionToken);
    TsNode *trueType = (TsNode *)allowConditionalTypesAnd(cb_parseType);
    parseExpected(TSK_ColonToken);
    TsNode *falseType = (TsNode *)allowConditionalTypesAnd(cb_parseType);
    TsNode *n = newNode(TSK_ConditionalType);
    n->checkType = type;
    n->extendsType = extendsType;
    n->trueType = trueType;
    n->falseType = falseType;
    return finishNode(n, pos, -1);
  }
  return type;
}

static TsNode *parseTypeAnnotation(void) {
  return parseOptional(TSK_ColonToken) ? parseType() : NULL;
}

/* ======================= part 5: expression starts, assignment, yield, arrow functions ======================= */

static bool isStartOfLeftHandSideExpression(void) {
  switch (token()) {
    case TSK_ThisKeyword: case TSK_SuperKeyword: case TSK_NullKeyword: case TSK_TrueKeyword:
    case TSK_FalseKeyword: case TSK_NumericLiteral: case TSK_BigIntLiteral: case TSK_StringLiteral:
    case TSK_NoSubstitutionTemplateLiteral: case TSK_TemplateHead: case TSK_OpenParenToken:
    case TSK_OpenBracketToken: case TSK_OpenBraceToken: case TSK_FunctionKeyword: case TSK_ClassKeyword:
    case TSK_NewKeyword: case TSK_SlashToken: case TSK_SlashEqualsToken: case TSK_Identifier:
      return true;
    case TSK_ImportKeyword:
      return lookAhead(cb_nextTokenIsOpenParenOrLessThanOrDot) != 0;
    default:
      return isIdentifier2();
  }
}

static bool isBinaryOperator2(void);

static bool isStartOfExpression(void) {
  if (isStartOfLeftHandSideExpression()) return true;
  switch (token()) {
    case TSK_PlusToken: case TSK_MinusToken: case TSK_TildeToken: case TSK_ExclamationToken:
    case TSK_DeleteKeyword: case TSK_TypeOfKeyword: case TSK_VoidKeyword: case TSK_PlusPlusToken:
    case TSK_MinusMinusToken: case TSK_LessThanToken: case TSK_AwaitKeyword: case TSK_YieldKeyword:
    case TSK_PrivateIdentifier: case TSK_AtToken:
      return true;
    default:
      if (isBinaryOperator2()) return true;
      return isIdentifier2();
  }
}

static bool isStartOfExpressionStatement(void) {
  return token() != TSK_OpenBraceToken && token() != TSK_FunctionKeyword && token() != TSK_ClassKeyword &&
         token() != TSK_AtToken && isStartOfExpression();
}

static TsNode *makeBinaryExpression(TsNode *left, TsNode *operatorToken, TsNode *right, int pos);
static TsNode *parseAssignmentExpressionOrHigher(bool allowReturnTypeInArrowFunction);

/* parseExpression: comma-separated assignment expressions */
static TsNode *parseExpression(void) {
  bool saveDecoratorContext = inDecoratorContext();
  if (saveDecoratorContext) setDecoratorContext(false);
  int pos = getNodePos();
  TsNode *expr = parseAssignmentExpressionOrHigher(true);
  TsNode *operatorToken;
  while ((operatorToken = parseOptionalToken(TSK_CommaToken)) != NULL) {
    expr = makeBinaryExpression(expr, operatorToken, parseAssignmentExpressionOrHigher(true), pos);
  }
  if (saveDecoratorContext) setDecoratorContext(true);
  return expr;
}

static TsNode *parseInitializer(void) {
  return parseOptional(TSK_EqualsToken) ? parseAssignmentExpressionOrHigher(true) : NULL;
}

static bool isYieldExpression2(void);
static TsNode *parseYieldExpression(void);
static TsNode *tryParseParenthesizedArrowFunctionExpression(bool allowReturnTypeInArrowFunction);
static TsNode *tryParseAsyncSimpleArrowFunctionExpression(bool allowReturnTypeInArrowFunction);
static TsNode *parseBinaryExpressionOrHigher(int precedence);
static bool isAssignmentOperator(int k);
static TsNode *parseConditionalExpressionRest(TsNode *leftOperand, int pos, bool allowReturnTypeInArrowFunction);

static TsNode *parseAssignmentExpressionOrHigher(bool allowReturnTypeInArrowFunction) {
  if (isYieldExpression2()) return parseYieldExpression();
  TsNode *arrowExpression = tryParseParenthesizedArrowFunctionExpression(allowReturnTypeInArrowFunction);
  if (!arrowExpression) arrowExpression = tryParseAsyncSimpleArrowFunctionExpression(allowReturnTypeInArrowFunction);
  if (arrowExpression) return arrowExpression;
  int pos = getNodePos();
  bool hasJSDoc = hasPrecedingJSDocComment();
  TsNode *expr = parseBinaryExpressionOrHigher(0 /* Lowest */);
  if (expr->kind == TSK_Identifier && token() == TSK_EqualsGreaterThanToken) {
    return parseSimpleArrowFunctionExpression(pos, expr, allowReturnTypeInArrowFunction, hasJSDoc, NULL);
  }
  if (isLeftHandSideExpression(expr) && isAssignmentOperator(reScanGreaterToken())) {
    TsNode *op = parseTokenNode();
    return makeBinaryExpression(expr, op, parseAssignmentExpressionOrHigher(allowReturnTypeInArrowFunction), pos);
  }
  return parseConditionalExpressionRest(expr, pos, allowReturnTypeInArrowFunction);
}

static bool isYieldExpression2(void) {
  if (token() == TSK_YieldKeyword) {
    if (inYieldContext()) return true;
    return lookAhead(cb_nextTokenIsIdentifierOrKeywordOrLiteralOnSameLine) != 0;
  }
  return false;
}

static bool nextTokenIsIdentifierOnSameLine(void) {
  nextToken();
  return !(TS.tflags & TF_PrecedingLineBreak) && isIdentifier2();
}

static TsNode *parseYieldExpression(void) {
  int pos = getNodePos();
  nextToken();
  TsNode *n = newNode(TSK_YieldExpression);
  if (!(TS.tflags & TF_PrecedingLineBreak) && (token() == TSK_AsteriskToken || isStartOfExpression())) {
    n->asteriskToken = parseOptionalToken(TSK_AsteriskToken);
    n->expression = parseAssignmentExpressionOrHigher(true);
  }
  return finishNode(n, pos, -1);
}

static TsNode *parseArrowFunctionExpressionBody(bool isAsync, bool allowReturnTypeInArrowFunction);

static TsNode *parseSimpleArrowFunctionExpression(int pos, TsNode *identifier, bool allowReturnTypeInArrowFunction, bool hasJSDoc, TsNode *asyncModifier) {
  TsNode *parameter = newNode(TSK_Parameter);
  parameter->name = identifier;
  finishNode(parameter, identifier->pos, -1);
  TsList *parameters = ts_list_single(parameter, parameter->pos, parameter->end);
  TsNode *equalsGreaterThanToken = parseExpectedToken(TSK_EqualsGreaterThanToken);
  TsNode *body = parseArrowFunctionExpressionBody(asyncModifier != NULL, allowReturnTypeInArrowFunction);
  TsNode *node = newNode(TSK_ArrowFunction);
  node->modifiers = asyncModifier ? ts_list_single(asyncModifier, asyncModifier->pos, asyncModifier->end) : NULL;
  node->parameters = parameters;
  node->equalsGreaterThanToken = equalsGreaterThanToken;
  node->body = body;
  return withJSDoc(finishNode(node, pos, -1), hasJSDoc);
}

/* isParenthesizedArrowFunctionExpression tri-state */
enum { TRI_False = 0, TRI_True = 1, TRI_Unknown = 2 };

static TsNode *parseParenthesizedArrowFunctionExpression(bool allowAmbiguity, bool allowReturnTypeInArrowFunction);
static TsNode *parsePossibleParenthesizedArrowFunctionExpression(bool allowReturnTypeInArrowFunction);
static intptr_t cb_parsePossibleParenthesizedArrowFunctionExpressionTrue(void) { return (intptr_t)parsePossibleParenthesizedArrowFunctionExpression(true); }
static intptr_t cb_parsePossibleParenthesizedArrowFunctionExpressionFalse(void) { return (intptr_t)parsePossibleParenthesizedArrowFunctionExpression(false); }

static TsNode *tryParseParenthesizedArrowFunctionExpression(bool allowReturnTypeInArrowFunction) {
  int triState = isParenthesizedArrowFunctionExpression();
  if (triState == TRI_False) return NULL;
  if (triState == TRI_True) return parseParenthesizedArrowFunctionExpression(true, true);
  return (TsNode *)tryParse(allowReturnTypeInArrowFunction ? cb_parsePossibleParenthesizedArrowFunctionExpressionTrue : cb_parsePossibleParenthesizedArrowFunctionExpressionFalse);
}

static int isParenthesizedArrowFunctionExpressionWorker(void);
static intptr_t cb_isParenthesizedArrowFunctionExpressionWorker(void) { return isParenthesizedArrowFunctionExpressionWorker(); }

static int isParenthesizedArrowFunctionExpression(void) {
  if (token() == TSK_OpenParenToken || token() == TSK_LessThanToken || token() == TSK_AsyncKeyword) {
    return (int)lookAhead(cb_isParenthesizedArrowFunctionExpressionWorker);
  }
  if (token() == TSK_EqualsGreaterThanToken) return TRI_True;
  return TRI_False;
}

static int isParenthesizedArrowFunctionExpressionWorker(void) {
  if (token() == TSK_AsyncKeyword) {
    nextToken();
    if (TS.tflags & TF_PrecedingLineBreak) return TRI_False;
    if (token() != TSK_OpenParenToken && token() != TSK_LessThanToken) return TRI_False;
  }
  int first2 = token();
  int second = nextToken();
  if (first2 == TSK_OpenParenToken) {
    if (second == TSK_CloseParenToken) {
      int third = nextToken();
      switch (third) {
        case TSK_EqualsGreaterThanToken:
        case TSK_ColonToken:
        case TSK_OpenBraceToken:
          return TRI_True;
        default:
          return TRI_False;
      }
    }
    if (second == TSK_OpenBracketToken || second == TSK_OpenBraceToken) return TRI_Unknown;
      if (second == TSK_DotDotDotToken) return TRI_True;
      if (isModifierKind(second) && second != TSK_AsyncKeyword && lookAhead(cb_nextTokenIsIdentifier) != 0) {
        if (nextToken() == TSK_AsKeyword) return TRI_False;
        return TRI_True;
      }
      if (!isIdentifier2() && second != TSK_ThisKeyword) return TRI_False;
      switch (nextToken()) {
        case TSK_ColonToken:
          return TRI_True;
        case TSK_QuestionToken:
          nextToken();
          if (token() == TSK_ColonToken || token() == TSK_CommaToken || token() == TSK_EqualsToken || token() == TSK_CloseParenToken) return TRI_True;
          return TRI_False;
        case TSK_CommaToken:
        case TSK_EqualsToken:
        case TSK_CloseParenToken:
          return TRI_Unknown;
      }
      return TRI_False;
    } else {
      /* first token is '<': JSX (.tsx) is not a case for .ts / .d.ts files */
      if (!isIdentifier2() && token() != TSK_ConstKeyword) return TRI_False;
      return TRI_Unknown;
    }
}

static intptr_t cb_isUnParenthesizedAsyncArrowFunctionWorker(void);

/* parsePossibleParenthesizedArrowFunctionExpression: positions that failed are remembered */
/* notParenthesizedArrow: token positions where a parenthesized arrow function failed (a set, reset per file) */
static int *notParenSetItems;
static int notParenSetN, notParenSetCap;
static bool notParenSetHas(int pos) {
  for (int i = 0; i < notParenSetN; i++) if (notParenSetItems[i] == pos) return true;
  return false;
}
static void notParenSetAdd(int pos) {
  if (notParenSetHas(pos)) return;
  if (notParenSetN == notParenSetCap) {
    notParenSetCap = notParenSetCap ? notParenSetCap * 2 : 256;
    notParenSetItems = realloc(notParenSetItems, sizeof(int) * notParenSetCap);
    if (!notParenSetItems) abort();
  }
  notParenSetItems[notParenSetN++] = pos;
}

static TsNode *parsePossibleParenthesizedArrowFunctionExpression(bool allowReturnTypeInArrowFunction) {
  int tokenPos = TS.tokenStart;
  if (notParenSetHas(tokenPos)) return NULL;
  TsNode *result = parseParenthesizedArrowFunctionExpression(false, allowReturnTypeInArrowFunction);
  if (!result) notParenSetAdd(tokenPos);
  return result;
}

static TsNode *tryParseAsyncSimpleArrowFunctionExpression(bool allowReturnTypeInArrowFunction) {
  if (token() == TSK_AsyncKeyword) {
    if (lookAhead(cb_isUnParenthesizedAsyncArrowFunctionWorker) == TRI_True) {
      int pos = getNodePos();
      bool hasJSDoc = hasPrecedingJSDocComment();
      TsNode *asyncModifier = parseModifiersForArrowFunction();
      TsNode *expr = parseBinaryExpressionOrHigher(0 /* Lowest */);
      return parseSimpleArrowFunctionExpression(pos, expr, allowReturnTypeInArrowFunction, hasJSDoc, asyncModifier);
    }
  }
  return NULL;
}

static int isUnParenthesizedAsyncArrowFunctionWorker(void) {
  if (token() == TSK_AsyncKeyword) {
    nextToken();
    if ((TS.tflags & TF_PrecedingLineBreak) || token() == TSK_EqualsGreaterThanToken) return TRI_False;
    TsNode *expr = parseBinaryExpressionOrHigher(0 /* Lowest */);
    if (!(TS.tflags & TF_PrecedingLineBreak) && expr->kind == TSK_Identifier && token() == TSK_EqualsGreaterThanToken) return TRI_True;
  }
  return TRI_False;
}
static intptr_t cb_isUnParenthesizedAsyncArrowFunctionWorker(void) { return isUnParenthesizedAsyncArrowFunctionWorker(); }

static bool isAsyncModifierNode(TsNode *n) { return n->kind == TSK_AsyncKeyword; }
static bool isJSDocFunctionType(const TsNode *n) { return n->kind == TSK_JSDocFunctionType; }
static bool isAssignmentOperator(int t) { return t >= TSK_FirstAssignment && t <= TSK_LastAssignment; }
static bool isLeftHandSideExpressionKind(int kind) {
  switch (kind) {
    case TSK_PropertyAccessExpression: case TSK_ElementAccessExpression: case TSK_NewExpression: case TSK_CallExpression:
    case TSK_JsxElement: case TSK_JsxSelfClosingElement: case TSK_JsxFragment: case TSK_TaggedTemplateExpression:
    case TSK_ArrayLiteralExpression: case TSK_ParenthesizedExpression: case TSK_ObjectLiteralExpression:
    case TSK_ClassExpression: case TSK_FunctionExpression: case TSK_Identifier: case TSK_PrivateIdentifier:
    case TSK_RegularExpressionLiteral: case TSK_NumericLiteral: case TSK_BigIntLiteral: case TSK_StringLiteral:
    case TSK_NoSubstitutionTemplateLiteral: case TSK_TemplateExpression: case TSK_FalseKeyword:
    case TSK_NullKeyword: case TSK_ThisKeyword: case TSK_TrueKeyword: case TSK_SuperKeyword:
    case TSK_NonNullExpression: case TSK_ExpressionWithTypeArguments: case TSK_MetaProperty:
    case TSK_ImportKeyword: case TSK_MissingDeclaration:
      return true;
    default:
      return false;
  }
}
static bool isLeftHandSideExpression(const TsNode *n) { return isLeftHandSideExpressionKind(n->kind); }

static TsNode *parseParenthesizedArrowFunctionExpression(bool allowAmbiguity, bool allowReturnTypeInArrowFunction) {
  int pos = getNodePos();
  bool hasJSDoc = hasPrecedingJSDocComment();
  TsList *modifiers = (TsList *)0;
  TsNode *m = parseModifiersForArrowFunction();
  modifiers = m ? ts_list_single(m, m->pos, m->end) : NULL;
  bool hasAsync = false;
  if (modifiers) for (int i = 0; i < modifiers->n; i++) if (isAsyncModifierNode(modifiers->v[i])) hasAsync = true;
  int isAsync = hasAsync ? 2 /* Await */ : 0 /* None */;
  TsList *typeParameters = parseTypeParameters();
  TsList *parameters;
  if (!parseExpected(TSK_OpenParenToken)) {
    if (!allowAmbiguity) return NULL;
    parameters = createMissingList();
  } else {
    if (!allowAmbiguity) {
      TsList *maybeParameters = parseParametersWorker(isAsync, allowAmbiguity);
      if (!maybeParameters) return NULL;
      parameters = maybeParameters;
    } else {
      parameters = parseParametersWorker(isAsync, allowAmbiguity);
    }
    if (!parseExpected(TSK_CloseParenToken) && !allowAmbiguity) return NULL;
  }
  bool hasReturnColon = token() == TSK_ColonToken;
  TsNode *type = parseReturnType(TSK_ColonToken, false);
  if (type && !allowAmbiguity && typeHasArrowFunctionBlockingParseError(type)) return NULL;
  TsNode *unwrappedType = type;
  while (unwrappedType && unwrappedType->kind == TSK_ParenthesizedType) unwrappedType = unwrappedType->type;
  bool hasJSDocFunctionType = unwrappedType && isJSDocFunctionType(unwrappedType);
  if (!allowAmbiguity && token() != TSK_EqualsGreaterThanToken && (hasJSDocFunctionType || token() != TSK_OpenBraceToken)) return NULL;
  int lastToken = token();
  TsNode *equalsGreaterThanToken = parseExpectedToken(TSK_EqualsGreaterThanToken);
  TsNode *body = NULL;
  if (lastToken == TSK_EqualsGreaterThanToken || lastToken == TSK_OpenBraceToken) {
    body = parseArrowFunctionExpressionBody(hasAsync, allowReturnTypeInArrowFunction);
  }
  if (!allowReturnTypeInArrowFunction && hasReturnColon) {
    if (token() != TSK_ColonToken) return NULL;
  }
  TsNode *node = newNode(TSK_ArrowFunction);
  node->modifiers = modifiers;
  node->typeParameters = typeParameters;
  node->parameters = parameters;
  node->type = type;
  node->equalsGreaterThanToken = equalsGreaterThanToken;
  node->body = body;
  return withJSDoc(finishNode(node, pos, -1), hasJSDoc);
}

static TsNode *parseArrowFunctionExpressionBody(bool isAsync, bool allowReturnTypeInArrowFunction) {
  if (token() == TSK_OpenBraceToken) return parseFunctionBlock(isAsync ? 2 : 0, 0);
  if (token() != TSK_SemicolonToken && token() != TSK_FunctionKeyword && token() != TSK_ClassKeyword &&
      isStartOfStatement() && !isStartOfExpressionStatement()) {
    return parseFunctionBlock(16 /* IgnoreMissingOpenBrace */ | (isAsync ? 2 : 0), 0);
  }
  bool savedYieldContext = inYieldContext();
  setYieldContext(false);
  bool savedTopLevel = P.topLevel;
  P.topLevel = false;
  TsNode *node;
  if (isAsync) {
    node = (TsNode *)doInAwaitContext(allowReturnTypeInArrowFunction ? cb_parseAssignmentExpressionTrue : cb_parseAssignmentExpressionFalse);
  } else {
    node = (TsNode *)doOutsideOfAwaitContext(allowReturnTypeInArrowFunction ? cb_parseAssignmentExpressionTrue : cb_parseAssignmentExpressionFalse);
  }
  P.topLevel = savedTopLevel;
  setYieldContext(savedYieldContext);
  return node;
}

static TsNode *parseConditionalExpressionRest(TsNode *leftOperand, int pos, bool allowReturnTypeInArrowFunction) {
  TsNode *questionToken = parseOptionalToken(TSK_QuestionToken);
  if (!questionToken) return leftOperand;
  TsNode *whenTrue = (TsNode *)doOutsideOfContext(CF_DisallowIn | 0, cb_parseAssignmentExpressionFalseDisallowIn);
  TsNode *colonToken = parseExpectedToken(TSK_ColonToken);
  TsNode *whenFalse = nodeIsPresent(colonToken) ? parseAssignmentExpressionOrHigher(allowReturnTypeInArrowFunction)
                                                 : createMissingNode(TSK_Identifier, false);
  TsNode *n = newNode(TSK_ConditionalExpression);
  n->condition = leftOperand;
  n->questionToken = questionToken;
  n->whenTrue = whenTrue;
  n->colonToken = colonToken;
  n->whenFalse = whenFalse;
  return finishNode(n, pos, -1);
}

static TsNode *parseBinaryExpressionOrHigher(int precedence) {
  int pos = getNodePos();
  TsNode *leftOperand = parseUnaryExpressionOrHigher();
  return parseBinaryExpressionRest(precedence, leftOperand, pos);
}

/* ======================= part 6: binary, unary, update, left-hand-side, super ======================= */

/* OperatorPrecedence values used by getBinaryOperatorPrecedence (typescript.js) */
enum { OP_Lowest = 0, OP_Coalesce = 5, OP_LogicalOR = 5, OP_LogicalAND = 6, OP_BitwiseOR = 7, OP_BitwiseXOR = 8,
       OP_BitwiseAND = 9, OP_Equality = 10, OP_Relational = 11, OP_Shift = 12, OP_Additive = 13,
       OP_Multiplicative = 14, OP_Exponentiation = 15 };

static int getBinaryOperatorPrecedence(int kind) {
  switch (kind) {
    case TSK_QuestionQuestionToken: return OP_Coalesce;
    case TSK_BarBarToken: return OP_LogicalOR;
    case TSK_AmpersandAmpersandToken: return OP_LogicalAND;
    case TSK_BarToken: return OP_BitwiseOR;
    case TSK_CaretToken: return OP_BitwiseXOR;
    case TSK_AmpersandToken: return OP_BitwiseAND;
    case TSK_EqualsEqualsToken: case TSK_ExclamationEqualsToken:
    case TSK_EqualsEqualsEqualsToken: case TSK_ExclamationEqualsEqualsToken:
      return OP_Equality;
    case TSK_LessThanToken: case TSK_GreaterThanToken: case TSK_LessThanEqualsToken: case TSK_GreaterThanEqualsToken:
    case TSK_InstanceOfKeyword: case TSK_InKeyword: case TSK_AsKeyword: case TSK_SatisfiesKeyword:
      return OP_Relational;
    case TSK_LessThanLessThanToken: case TSK_GreaterThanGreaterThanToken: case TSK_GreaterThanGreaterThanGreaterThanToken:
      return OP_Shift;
    case TSK_PlusToken: case TSK_MinusToken: return OP_Additive;
    case TSK_AsteriskToken: case TSK_SlashToken: case TSK_PercentToken: return OP_Multiplicative;
    case TSK_AsteriskAsteriskToken: return OP_Exponentiation;
  }
  return -1;
}

static bool isInOrOfKeyword(int t) { return t == TSK_InKeyword || t == TSK_OfKeyword; }

static bool isBinaryOperator2(void) {
  if (inDisallowInContext() && token() == TSK_InKeyword) return false;
  return getBinaryOperatorPrecedence(token()) > 0;
}

static TsNode *makeBinaryExpression(TsNode *left, TsNode *operatorToken, TsNode *right, int pos) {
  TsNode *n = newNode(TSK_BinaryExpression);
  n->left = left;
  n->operatorToken = operatorToken;
  n->right = right;
  return finishNode(n, pos, -1);
}

static TsNode *makeSatisfiesExpression(TsNode *left, TsNode *right) {
  TsNode *n = newNode(TSK_SatisfiesExpression);
  n->expression = left;
  n->type = right;
  return finishNode(n, left->pos, -1);
}

static TsNode *makeAsExpression(TsNode *left, TsNode *right) {
  TsNode *n = newNode(TSK_AsExpression);
  n->expression = left;
  n->type = right;
  return finishNode(n, left->pos, -1);
}

static TsNode *parseBinaryExpressionRest(int precedence, TsNode *leftOperand, int pos) {
  for (;;) {
    reScanGreaterToken();
    int newPrecedence = getBinaryOperatorPrecedence(token());
    bool consumeCurrentOperator = token() == TSK_AsteriskAsteriskToken ? newPrecedence >= precedence : newPrecedence > precedence;
    if (!consumeCurrentOperator) break;
    if (token() == TSK_InKeyword && inDisallowInContext()) break;
    if (token() == TSK_AsKeyword || token() == TSK_SatisfiesKeyword) {
      if (TS.tflags & TF_PrecedingLineBreak) break;
      int keywordKind = token();
      nextToken();
      leftOperand = keywordKind == TSK_SatisfiesKeyword ? makeSatisfiesExpression(leftOperand, parseType())
                                                        : makeAsExpression(leftOperand, parseType());
    } else {
      TsNode *op = parseTokenNode();
      leftOperand = makeBinaryExpression(leftOperand, op, parseBinaryExpressionOrHigher(newPrecedence), pos);
    }
  }
  return leftOperand;
}

static TsNode *parseSimpleUnaryExpression(void);
static TsNode *parseUpdateExpression(void);
static TsNode *parseLeftHandSideExpressionOrHigher(void);
static TsNode *parseMemberExpressionOrHigher(void);
static TsNode *parseSuperExpression(void);
static TsNode *parseTypeAssertion(void);
static TsNode *parseAwaitExpression(void);

static intptr_t cb_parseSimpleUnaryExpression(void) { return (intptr_t)parseSimpleUnaryExpression(); }
static intptr_t cb_parseLeftHandSideExpressionOrHigher(void) { return (intptr_t)parseLeftHandSideExpressionOrHigher(); }

/* nextTokenAnd(parseSimpleUnaryExpression): advance, then parse the operand */
static TsNode *nextTokenAndSimpleUnary(void) {
  nextToken();
  return parseSimpleUnaryExpression();
}

static TsNode *parsePrefixUnaryExpression(void) {
  int pos = getNodePos();
  int op = token();
  TsNode *operand = nextTokenAndSimpleUnary();
  TsNode *n = newNode(TSK_PrefixUnaryExpression);
  n->extra = op;
  n->operand = operand;
  return finishNode(n, pos, -1);
}

static TsNode *parseDeleteExpression(void) {
  int pos = getNodePos();
  TsNode *operand = nextTokenAndSimpleUnary();
  TsNode *n = newNode(TSK_DeleteExpression);
  n->expression = operand;
  return finishNode(n, pos, -1);
}

static TsNode *parseTypeOfExpression(void) {
  int pos = getNodePos();
  TsNode *operand = nextTokenAndSimpleUnary();
  TsNode *n = newNode(TSK_TypeOfExpression);
  n->expression = operand;
  return finishNode(n, pos, -1);
}

static TsNode *parseVoidExpression(void) {
  int pos = getNodePos();
  TsNode *operand = nextTokenAndSimpleUnary();
  TsNode *n = newNode(TSK_VoidExpression);
  n->expression = operand;
  return finishNode(n, pos, -1);
}

static bool isAwaitExpression2(void) {
  if (token() == TSK_AwaitKeyword) {
    if (inAwaitContext()) return true;
    return lookAhead(cb_nextTokenIsIdentifierOrKeywordOrLiteralOnSameLine) != 0;
  }
  return false;
}

static TsNode *parseAwaitExpression(void) {
  int pos = getNodePos();
  TsNode *operand = nextTokenAndSimpleUnary();
  TsNode *n = newNode(TSK_AwaitExpression);
  n->expression = operand;
  return finishNode(n, pos, -1);
}

static bool isUpdateExpression(void) {
  switch (token()) {
    case TSK_PlusToken: case TSK_MinusToken: case TSK_TildeToken: case TSK_ExclamationToken:
    case TSK_DeleteKeyword: case TSK_TypeOfKeyword: case TSK_VoidKeyword: case TSK_AwaitKeyword:
      return false;
    case TSK_LessThanToken:
      /* JSX is not used by .ts / .d.ts files */
      return false;
    default:
      return true;
  }
}

static TsNode *parseUnaryExpressionOrHigher(void) {
  if (isUpdateExpression()) {
    int pos = getNodePos();
    TsNode *updateExpression = parseUpdateExpression();
    return token() == TSK_AsteriskAsteriskToken
               ? parseBinaryExpressionRest(getBinaryOperatorPrecedence(token()), updateExpression, pos)
               : updateExpression;
  }
  TsNode *simpleUnaryExpression = parseSimpleUnaryExpression();
  if (token() == TSK_AsteriskAsteriskToken) parseErrorAtPosition(simpleUnaryExpression->pos);
  return simpleUnaryExpression;
}

static TsNode *parseSimpleUnaryExpression(void) {
  switch (token()) {
    case TSK_PlusToken: case TSK_MinusToken: case TSK_TildeToken: case TSK_ExclamationToken:
      return parsePrefixUnaryExpression();
    case TSK_DeleteKeyword:
      return parseDeleteExpression();
    case TSK_TypeOfKeyword:
      return parseTypeOfExpression();
    case TSK_VoidKeyword:
      return parseVoidExpression();
    case TSK_LessThanToken:
      return parseTypeAssertion();
    case TSK_AwaitKeyword:
      if (isAwaitExpression2()) return parseAwaitExpression();
      return parseUpdateExpression();
    default:
      return parseUpdateExpression();
  }
}

static TsNode *parseUpdateExpression(void) {
  if (token() == TSK_PlusPlusToken || token() == TSK_MinusMinusToken) {
    int pos = getNodePos();
    int op = token();
    nextToken();
    TsNode *operand = parseLeftHandSideExpressionOrHigher();
    TsNode *n = newNode(TSK_PrefixUnaryExpression);
    n->extra = op;
    n->operand = operand;
    return finishNode(n, pos, -1);
  }
  TsNode *expression = parseLeftHandSideExpressionOrHigher();
  if ((token() == TSK_PlusPlusToken || token() == TSK_MinusMinusToken) && !(TS.tflags & TF_PrecedingLineBreak)) {
    int op = token();
    nextToken();
    TsNode *n = newNode(TSK_PostfixUnaryExpression);
    n->operand = expression;
    n->extra = op;
    return finishNode(n, expression->pos, -1);
  }
  return expression;
}


static TsNode *parseLeftHandSideExpressionOrHigher(void) {
  int pos = getNodePos();
  TsNode *expression;
  if (token() == TSK_ImportKeyword) {
    if (lookAhead(cb_nextTokenIsOpenParenOrLessThan)) {
      P.sourceFlags |= 4194304; /* PossiblyContainsDynamicImport */
      expression = parseTokenNode();
    } else if (lookAhead(cb_nextTokenIsDot)) {
      nextToken();
      nextToken();
      TsNode *mp = newNode(TSK_MetaProperty);
      mp->extra = TSK_ImportKeyword;
      mp->name = parseIdentifierName();
      expression = finishNode(mp, pos, -1);
      if (mp->name->textLen == 5 && memcmp(mp->name->text, "defer", 5) == 0) {
        if (token() == TSK_OpenParenToken || token() == TSK_LessThanToken) P.sourceFlags |= 4194304;
      } else {
        P.sourceFlags |= 8388608; /* PossiblyContainsImportMeta */
      }
    } else {
      expression = parseMemberExpressionOrHigher();
    }
  } else {
    expression = token() == TSK_SuperKeyword ? parseSuperExpression() : parseMemberExpressionOrHigher();
  }
  return parseCallExpressionRest(pos, expression);
}

static TsNode *parseMemberExpressionOrHigher(void) {
  int pos = getNodePos();
  TsNode *expression = parsePrimaryExpression();
  return parseMemberExpressionRest(pos, expression, true);
}

static TsNode *parseSuperExpression(void) {
  int pos = getNodePos();
  TsNode *expression = parseTokenNode();
  if (token() == TSK_LessThanToken) {
    int startPos = getNodePos();
    TsList *typeArguments = (TsList *)tryParse(cb_parseTypeArgumentsInExpression);
    if (typeArguments) {
      parseErrorAtPosition(startPos);
      if (!isTemplateStartOfTaggedTemplate()) {
        TsNode *n = newNode(TSK_ExpressionWithTypeArguments);
        n->expression = expression;
        n->typeArguments = typeArguments;
        expression = n;
      }
    }
  }
  if (token() == TSK_OpenParenToken || token() == TSK_DotToken || token() == TSK_OpenBracketToken) return expression;
  parseExpectedToken(TSK_DotToken);
  TsNode *pae = newNode(TSK_PropertyAccessExpression);
  pae->expression = expression;
  pae->name = parseRightSideOfDot(true, true, true);
  return finishNode(pae, pos, -1);
}

/* ======================= part 7: type assertions, member and call expression rest ======================= */

#define TNF_OptionalChain 64

static TsNode *parseTypeAssertion(void) {
  int pos = getNodePos();
  parseExpected(TSK_LessThanToken);
  TsNode *type = parseType();
  parseExpected(TSK_GreaterThanToken);
  TsNode *expression = parseSimpleUnaryExpression();
  TsNode *n = newNode(TSK_TypeAssertionExpression);
  n->type = type;
  n->expression = expression;
  return finishNode(n, pos, -1);
}

static bool isTemplateStartOfTaggedTemplate(void) {
  return token() == TSK_NoSubstitutionTemplateLiteral || token() == TSK_TemplateHead;
}

static bool nextTokenIsIdentifierOrKeywordOrOpenBracketOrTemplate(void) {
  nextToken();
  return tokenIsIdentifierOrKeyword(token()) || token() == TSK_OpenBracketToken || isTemplateStartOfTaggedTemplate();
}
static intptr_t cb_nextTokenIsIdentifierOrKeywordOrOpenBracketOrTemplate(void) { return nextTokenIsIdentifierOrKeywordOrOpenBracketOrTemplate(); }

static bool isStartOfOptionalPropertyOrElementAccessChain(void) {
  return token() == TSK_QuestionDotToken && lookAhead(cb_nextTokenIsIdentifierOrKeywordOrOpenBracketOrTemplate) != 0;
}

static bool tryReparseOptionalChain(TsNode *node) {
  if (node->flags & TNF_OptionalChain) return true;
  if (node->kind == TSK_NonNullExpression) {
    TsNode *expr = node->expression;
    while (expr->kind == TSK_NonNullExpression && !(expr->flags & TNF_OptionalChain)) expr = expr->expression;
    if (expr->flags & TNF_OptionalChain) {
      while (node->kind == TSK_NonNullExpression) {
        node->flags |= TNF_OptionalChain;
        node = node->expression;
      }
      return true;
    }
  }
  return false;
}

static TsNode *parseRightSideOfDotAll(void) { return parseRightSideOfDot(true, true, true); }

static TsNode *parsePropertyAccessExpressionRest(int pos, TsNode *expression, TsNode *questionDotToken) {
  TsNode *name = parseRightSideOfDot(true, true, true);
  bool isOptionalChain2 = questionDotToken != NULL || tryReparseOptionalChain(expression);
  TsNode *propertyAccess = newNode(TSK_PropertyAccessExpression);
  propertyAccess->expression = expression;
  propertyAccess->questionDotToken = questionDotToken;
  propertyAccess->name = name;
  if (isOptionalChain2) propertyAccess->flags |= TNF_OptionalChain;
  if (isOptionalChain2 && name->kind == TSK_PrivateIdentifier) parseErrorAtPosition(name->pos);
  if (expression->kind == TSK_ExpressionWithTypeArguments && expression->typeArguments) parseErrorAtPosition(expression->typeArguments->pos - 1);
  return finishNode(propertyAccess, pos, -1);
}

static TsNode *parseArgumentExpressionForElement(void);

static TsNode *parseElementAccessExpressionRest(int pos, TsNode *expression, TsNode *questionDotToken) {
  TsNode *argumentExpression;
  if (token() == TSK_CloseBracketToken) {
    argumentExpression = createMissingNode(TSK_Identifier, true);
  } else {
    TsNode *argument = (TsNode *)allowInAnd(cb_parseExpression);
    argumentExpression = argument;
  }
  parseExpected(TSK_CloseBracketToken);
  TsNode *n = newNode(TSK_ElementAccessExpression);
  n->expression = expression;
  n->questionDotToken = questionDotToken;
  n->argumentExpression = argumentExpression;
  if (questionDotToken || tryReparseOptionalChain(expression)) n->flags |= TNF_OptionalChain;
  return finishNode(n, pos, -1);
}

static TsNode *parseTaggedTemplateRest(int pos, TsNode *tag, TsNode *questionDotToken, TsList *typeArguments) {
  TsNode *tmpl;
  if (token() == TSK_NoSubstitutionTemplateLiteral) {
    reScanTemplateToken();
    tmpl = parseLiteralNode();
  } else {
    tmpl = parseTemplateExpression(true);
  }
  TsNode *tagExpression = newNode(TSK_TaggedTemplateExpression);
  tagExpression->tag = tag;
  tagExpression->typeArguments = typeArguments;
  tagExpression->templateNode = tmpl;
  if (questionDotToken || (tag->flags & TNF_OptionalChain)) tagExpression->flags |= TNF_OptionalChain;
  tagExpression->questionDotToken = questionDotToken;
  return finishNode(tagExpression, pos, -1);
}

static TsNode *parseMemberExpressionRest(int pos, TsNode *expression, bool allowOptionalChain) {
  for (;;) {
    TsNode *questionDotToken = NULL;
    bool isPropertyAccess = false;
    if (allowOptionalChain && isStartOfOptionalPropertyOrElementAccessChain()) {
      questionDotToken = parseExpectedToken(TSK_QuestionDotToken);
      isPropertyAccess = tokenIsIdentifierOrKeyword(token());
    } else {
      isPropertyAccess = parseOptional(TSK_DotToken);
    }
    if (isPropertyAccess) {
      expression = parsePropertyAccessExpressionRest(pos, expression, questionDotToken);
      continue;
    }
    if ((questionDotToken || !inDecoratorContext()) && parseOptional(TSK_OpenBracketToken)) {
      expression = parseElementAccessExpressionRest(pos, expression, questionDotToken);
      continue;
    }
    if (isTemplateStartOfTaggedTemplate()) {
      expression = (!questionDotToken && expression->kind == TSK_ExpressionWithTypeArguments)
                       ? parseTaggedTemplateRest(pos, expression->expression, questionDotToken, expression->typeArguments)
                       : parseTaggedTemplateRest(pos, expression, questionDotToken, NULL);
      continue;
    }
    if (!questionDotToken) {
      if (token() == TSK_ExclamationToken && !(TS.tflags & TF_PrecedingLineBreak)) {
        nextToken();
        TsNode *n = newNode(TSK_NonNullExpression);
        n->expression = expression;
        expression = finishNode(n, pos, -1);
        continue;
      }
      TsList *typeArguments = (TsList *)tryParse(cb_parseTypeArgumentsInExpression);
      if (typeArguments) {
        TsNode *n = newNode(TSK_ExpressionWithTypeArguments);
        n->expression = expression;
        n->typeArguments = typeArguments;
        expression = finishNode(n, pos, -1);
        continue;
      }
    }
    return expression;
  }
}

static TsNode *parseCallExpressionRest(int pos, TsNode *expression) {
  for (;;) {
    expression = parseMemberExpressionRest(pos, expression, true);
    TsList *typeArguments = NULL;
    TsNode *questionDotToken = parseOptionalToken(TSK_QuestionDotToken);
    if (questionDotToken) {
      typeArguments = (TsList *)tryParse(cb_parseTypeArgumentsInExpression);
      if (isTemplateStartOfTaggedTemplate()) {
        expression = parseTaggedTemplateRest(pos, expression, questionDotToken, typeArguments);
        continue;
      }
    }
    if (typeArguments || token() == TSK_OpenParenToken) {
      if (!questionDotToken && expression->kind == TSK_ExpressionWithTypeArguments) {
        typeArguments = expression->typeArguments;
        expression = expression->expression;
      }
      TsList *argumentList = parseArgumentList();
      TsNode *callExpr = newNode(TSK_CallExpression);
      callExpr->expression = expression;
      callExpr->questionDotToken = questionDotToken;
      callExpr->typeArguments = typeArguments;
      callExpr->arguments = argumentList;
      if (questionDotToken || tryReparseOptionalChain(expression)) callExpr->flags |= TNF_OptionalChain;
      expression = finishNode(callExpr, pos, -1);
      continue;
    }
    if (questionDotToken) {
      TsNode *name = createMissingNode(TSK_Identifier, false);
      TsNode *pae = newNode(TSK_PropertyAccessExpression);
      pae->expression = expression;
      pae->questionDotToken = questionDotToken;
      pae->name = name;
      pae->flags |= TNF_OptionalChain;
      expression = finishNode(pae, pos, -1);
    }
    break;
  }
  return expression;
}

static TsNode *parseArgumentExpression(void);

static TsList *parseArgumentList(void) {
  parseExpected(TSK_OpenParenToken);
  TsList *result = parseDelimitedList(PC_ArgumentExpressions, parseArgumentExpression, false);
  parseExpected(TSK_CloseParenToken);
  return result;
}

static bool canFollowTypeArgumentsInExpression(void) {
  switch (token()) {
    case TSK_OpenParenToken: case TSK_NoSubstitutionTemplateLiteral: case TSK_TemplateHead:
      return true;
    case TSK_LessThanToken: case TSK_GreaterThanToken: case TSK_PlusToken: case TSK_MinusToken:
      return false;
  }
  return (TS.tflags & TF_PrecedingLineBreak) || isBinaryOperator2() || !isStartOfExpression();
}

static TsList *parseTypeArgumentsInExpression(void) {
  if ((P.contextFlags & TNF_JavaScriptFile) != 0) return NULL;
  if (reScanLessThanToken() != TSK_LessThanToken) return NULL;
  nextToken();
  TsList *typeArguments = parseDelimitedList(PC_TypeArguments, parseType, false);
  if (reScanGreaterToken() != TSK_GreaterThanToken) return NULL;
  nextToken();
  return typeArguments && canFollowTypeArgumentsInExpression() ? typeArguments : NULL;
}
static intptr_t cb_parseTypeArgumentsInExpression(void) { return (intptr_t)parseTypeArgumentsInExpression(); }

/* ======================= part 8: primary expressions, literals, object and array literals, blocks ======================= */

static bool nextTokenIsFunctionKeywordOnSameLine(void) {
  nextToken();
  return token() == TSK_FunctionKeyword && !(TS.tflags & TF_PrecedingLineBreak);
}
static intptr_t cb_nextTokenIsFunctionKeywordOnSameLine(void) { return nextTokenIsFunctionKeywordOnSameLine(); }

static TsNode *parsePrimaryExpression(void) {
  switch (token()) {
    case TSK_NoSubstitutionTemplateLiteral:
      if (TS.tflags & 26656) reScanTemplateToken();
      return parseLiteralNode();
    case TSK_NumericLiteral: case TSK_BigIntLiteral: case TSK_StringLiteral:
      return parseLiteralNode();
    case TSK_ThisKeyword: case TSK_SuperKeyword: case TSK_NullKeyword: case TSK_TrueKeyword: case TSK_FalseKeyword:
      return parseTokenNode();
    case TSK_OpenParenToken:
      return parseParenthesizedExpression();
    case TSK_OpenBracketToken:
      return parseArrayLiteralExpression();
    case TSK_OpenBraceToken:
      return parseObjectLiteralExpression();
    case TSK_AsyncKeyword:
      if (!lookAhead(cb_nextTokenIsFunctionKeywordOnSameLine)) break;
      return parseFunctionExpression();
    case TSK_AtToken:
      return parseDecoratedExpression();
    case TSK_ClassKeyword:
      return parseClassExpression();
    case TSK_FunctionKeyword:
      return parseFunctionExpression();
    case TSK_NewKeyword:
      return parseNewExpressionOrNewDotTarget();
    case TSK_SlashToken: case TSK_SlashEqualsToken:
      if (reScanSlashToken() == TSK_RegularExpressionLiteral) return parseLiteralNode();
      break;
    case TSK_TemplateHead:
      return parseTemplateExpression(false);
    case TSK_PrivateIdentifier:
      return parsePrivateIdentifier();
  }
  return parseIdentifier();
}

static TsNode *parseParenthesizedExpression(void) {
  int pos = getNodePos();
  bool hasJSDoc = hasPrecedingJSDocComment();
  parseExpected(TSK_OpenParenToken);
  TsNode *expression = (TsNode *)allowInAnd(cb_parseExpression);
  parseExpected(TSK_CloseParenToken);
  TsNode *n = newNode(TSK_ParenthesizedExpression);
  n->expression = expression;
  return withJSDoc(finishNode(n, pos, -1), hasJSDoc);
}

static TsNode *parseSpreadElement(void) {
  int pos = getNodePos();
  parseExpected(TSK_DotDotDotToken);
  TsNode *expression = parseAssignmentExpressionOrHigher(true);
  TsNode *n = newNode(TSK_SpreadElement);
  n->expression = expression;
  return finishNode(n, pos, -1);
}

static TsNode *parseArgumentOrArrayLiteralElement(void) {
  if (token() == TSK_DotDotDotToken) return parseSpreadElement();
  if (token() == TSK_CommaToken) return finishNode(newNode(TSK_OmittedExpression), getNodePos(), -1);
  return parseAssignmentExpressionOrHigher(true);
}

static intptr_t cb_parseArgumentOrArrayLiteralElement(void) { return (intptr_t)parseArgumentOrArrayLiteralElement(); }
static intptr_t cb_parseArgumentOrArrayLiteralElementDisallowIn(void) { return 0; }

static TsNode *parseArgumentExpression(void) {
  return (TsNode *)doOutsideOfContext(CF_DisallowIn | CF_Decorator, cb_parseArgumentOrArrayLiteralElement);
}
static TsNode *parseArgumentExpressionForElement(void) { return parseArgumentExpression(); }

static TsNode *parseArrayLiteralExpression(void) {
  int pos = getNodePos();
  int openBracketPosition = TS.tokenStart;
  bool openBracketParsed = parseExpected(TSK_OpenBracketToken);
  bool multiLine = (TS.tflags & TF_PrecedingLineBreak) != 0;
  (void)openBracketPosition;
  TsList *elements = parseDelimitedList(PC_ArrayLiteralMembers, parseArgumentOrArrayLiteralElement, false);
  parseExpectedMatchingBrackets(TSK_CloseBracketToken, openBracketParsed);
  TsNode *n = newNode(TSK_ArrayLiteralExpression);
  n->elements = elements;
  n->extra = multiLine;
  return finishNode(n, pos, -1);
}


static TsNode *parseObjectLiteralElement(void) {
  int pos = getNodePos();
  bool hasJSDoc = hasPrecedingJSDocComment();
  if (parseOptionalToken(TSK_DotDotDotToken)) {
    TsNode *expression = parseAssignmentExpressionOrHigher(true);
    TsNode *n = newNode(TSK_SpreadAssignment);
    n->expression = expression;
    return withJSDoc(finishNode(n, pos, -1), hasJSDoc);
  }
  TsList *modifiers = parseModifiers(true, false, false);
  if (parseContextualModifier(TSK_GetKeyword)) return parseAccessorDeclaration(pos, hasJSDoc, modifiers, TSK_GetAccessor, 0);
  if (parseContextualModifier(TSK_SetKeyword)) return parseAccessorDeclaration(pos, hasJSDoc, modifiers, TSK_SetAccessor, 0);
  TsNode *asteriskToken = parseOptionalToken(TSK_AsteriskToken);
  bool tokenIsIdentifier = isIdentifier2();
  TsNode *name = parsePropertyName();
  TsNode *questionToken = parseOptionalToken(TSK_QuestionToken);
  TsNode *exclamationToken = parseOptionalToken(TSK_ExclamationToken);
  if (asteriskToken || token() == TSK_OpenParenToken || token() == TSK_LessThanToken) {
    return parseMethodDeclaration(pos, hasJSDoc, modifiers, asteriskToken, name, questionToken, exclamationToken, 0);
  }
  TsNode *node;
  bool isShorthandPropertyAssignment2 = tokenIsIdentifier && token() != TSK_ColonToken;
  if (isShorthandPropertyAssignment2) {
    TsNode *equalsToken = parseOptionalToken(TSK_EqualsToken);
    TsNode *objectAssignmentInitializer = equalsToken ? (TsNode *)allowInAnd(cb_parseAssignmentExpressionTrue) : NULL;
    node = newNode(TSK_ShorthandPropertyAssignment);
    node->name = name;
    node->objectAssignmentInitializer = objectAssignmentInitializer;
    node->equalsToken = equalsToken;
  } else {
    parseExpected(TSK_ColonToken);
    TsNode *initializer = (TsNode *)allowInAnd(cb_parseAssignmentExpressionTrue);
    node = newNode(TSK_PropertyAssignment);
    node->name = name;
    node->initializer = initializer;
  }
  node->modifiers = modifiers;
  node->questionToken = questionToken;
  node->exclamationToken = exclamationToken;
  return withJSDoc(finishNode(node, pos, -1), hasJSDoc);
}

static TsNode *parseObjectLiteralExpression(void) {
  int pos = getNodePos();
  int openBracePosition = TS.tokenStart;
  bool openBraceParsed = parseExpected(TSK_OpenBraceToken);
  bool multiLine = (TS.tflags & TF_PrecedingLineBreak) != 0;
  TsList *properties = parseDelimitedList(PC_ObjectLiteralMembers, parseObjectLiteralElement, true);
  parseExpectedMatchingBrackets(TSK_CloseBraceToken, openBraceParsed);
  (void)openBracePosition;
  TsNode *n = newNode(TSK_ObjectLiteralExpression);
  n->properties = properties;
  n->extra = multiLine;
  return finishNode(n, pos, -1);
}

static TsNode *parseOptionalBindingIdentifier(void) {
  return isBindingIdentifier() ? parseBindingIdentifier() : NULL;
}
static intptr_t cb_parseOptionalBindingIdentifier(void) { return (intptr_t)parseOptionalBindingIdentifier(); }

static TsNode *parseFunctionExpression(void) {
  bool savedDecoratorContext = inDecoratorContext();
  setDecoratorContext(false);
  int pos = getNodePos();
  bool hasJSDoc = hasPrecedingJSDocComment();
  TsList *modifiers = parseModifiers(false, false, false);
  parseExpected(TSK_FunctionKeyword);
  TsNode *asteriskToken = parseOptionalToken(TSK_AsteriskToken);
  int isGenerator = asteriskToken ? 1 : 0;
  bool hasAsync = false;
  if (modifiers) for (int i = 0; i < modifiers->n; i++) if (modifiers->v[i]->kind == TSK_AsyncKeyword) hasAsync = true;
  int isAsync = hasAsync ? 2 : 0;
  TsNode *name;
  if (isGenerator && isAsync) name = (TsNode *)doInYieldAndAwaitContext(cb_parseOptionalBindingIdentifier);
  else if (isGenerator) name = (TsNode *)doInYieldContext(cb_parseOptionalBindingIdentifier);
  else if (isAsync) name = (TsNode *)doInAwaitContext(cb_parseOptionalBindingIdentifier);
  else name = parseOptionalBindingIdentifier();
  TsList *typeParameters = parseTypeParameters();
  TsList *parameters = parseParameters(isGenerator | isAsync);
  TsNode *type = parseReturnType(TSK_ColonToken, false);
  TsNode *body = parseFunctionBlock(isGenerator | isAsync, 0);
  setDecoratorContext(savedDecoratorContext);
  TsNode *node = newNode(TSK_FunctionExpression);
  node->modifiers = modifiers;
  node->asteriskToken = asteriskToken;
  node->name = name;
  node->typeParameters = typeParameters;
  node->parameters = parameters;
  node->type = type;
  node->body = body;
  return withJSDoc(finishNode(node, pos, -1), hasJSDoc);
}

static TsNode *parseNewExpressionOrNewDotTarget(void) {
  int pos = getNodePos();
  parseExpected(TSK_NewKeyword);
  if (parseOptional(TSK_DotToken)) {
    TsNode *name = parseIdentifierName();
    TsNode *mp = newNode(TSK_MetaProperty);
    mp->extra = TSK_NewKeyword;
    mp->name = name;
    return finishNode(mp, pos, -1);
  }
  int expressionPos = getNodePos();
  TsNode *expression = parseMemberExpressionRest(expressionPos, parsePrimaryExpression(), false);
  TsList *typeArguments = NULL;
  if (expression->kind == TSK_ExpressionWithTypeArguments) {
    typeArguments = expression->typeArguments;
    expression = expression->expression;
  }
  if (token() == TSK_QuestionDotToken) parseErrorAtCurrentToken();
  TsList *argumentList = token() == TSK_OpenParenToken ? parseArgumentList() : NULL;
  TsNode *n = newNode(TSK_NewExpression);
  n->expression = expression;
  n->typeArguments = typeArguments;
  n->arguments = argumentList;
  return finishNode(n, pos, -1);
}

/* parseBlock(ignoreMissingOpenBrace, ...) */
static TsNode *parseBlock(bool ignoreMissingOpenBrace) {
  int pos = getNodePos();
  bool hasJSDoc = hasPrecedingJSDocComment();
  int openBracePosition = TS.tokenStart;
  bool openBraceParsed = parseExpected(TSK_OpenBraceToken);
  (void)openBracePosition;
  if (openBraceParsed || ignoreMissingOpenBrace) {
    bool multiLine = (TS.tflags & TF_PrecedingLineBreak) != 0;
    TsList *statements = parseList(PC_BlockStatements, parseStatementFn);
    parseExpectedMatchingBrackets(TSK_CloseBraceToken, openBraceParsed);
    TsNode *result = newNode(TSK_Block);
    result->statements = statements;
    result->extra = multiLine;
    result = withJSDoc(finishNode(result, pos, -1), hasJSDoc);
    if (token() == TSK_EqualsToken) {
      parseErrorAtCurrentToken();
      nextToken();
    }
    return result;
  }
  TsList *statements = createMissingList();
  TsNode *result = newNode(TSK_Block);
  result->statements = statements;
  return withJSDoc(finishNode(result, pos, -1), hasJSDoc);
}

static TsNode *parseFunctionBlock(int flags, int diagnosticMessage) {
  bool savedYieldContext = inYieldContext();
  setYieldContext((flags & 1) != 0);
  bool savedAwaitContext = inAwaitContext();
  setAwaitContext((flags & 2) != 0);
  bool savedTopLevel = P.topLevel;
  P.topLevel = false;
  bool saveDecoratorContext = inDecoratorContext();
  if (saveDecoratorContext) setDecoratorContext(false);
  TsNode *block = parseBlock((flags & 16) != 0);
  if (saveDecoratorContext) setDecoratorContext(true);
  P.topLevel = savedTopLevel;
  setYieldContext(savedYieldContext);
  setAwaitContext(savedAwaitContext);
  return block;
}

/* ======================= part 9: statements ======================= */

static void parseExpectedMatchingBrackets(int closeKind, bool openParsed) {
  if (token() == closeKind) {
    nextToken();
    return;
  }
  parseErrorAtCurrentToken();
  (void)openParsed;
}

static TsNode *parseStatement(void);
static TsNode *parseStatementFn(void) { return parseStatement(); }
static TsNode *parseVariableDeclarationList(bool inForStatementInitializer);
static bool nextTokenIsUsingKeywordThenBindingIdentifierOrStartOfObjectDestructuringOnSameLine(void);
static intptr_t cb_nextTokenIsUsingKeywordThenBindingIdentifierOrStartOfObjectDestructuringOnSameLine(void) {
  return nextTokenIsUsingKeywordThenBindingIdentifierOrStartOfObjectDestructuringOnSameLine();
}
static TsNode *parseCatchClause(void);

static TsNode *parseEmptyStatement(void) {
  int pos = getNodePos();
  bool hasJSDoc = hasPrecedingJSDocComment();
  parseExpected(TSK_SemicolonToken);
  return withJSDoc(finishNode(newNode(TSK_EmptyStatement), pos, -1), hasJSDoc);
}

static TsNode *parseIfStatement(void) {
  int pos = getNodePos();
  bool hasJSDoc = hasPrecedingJSDocComment();
  parseExpected(TSK_IfKeyword);
  int openParenPosition = TS.tokenStart;
  (void)openParenPosition;
  bool openParenParsed = parseExpected(TSK_OpenParenToken);
  TsNode *expression = (TsNode *)allowInAnd(cb_parseExpression);
  parseExpectedMatchingBrackets(TSK_CloseParenToken, openParenParsed);
  TsNode *thenStatement = parseStatement();
  TsNode *elseStatement = parseOptional(TSK_ElseKeyword) ? parseStatement() : NULL;
  TsNode *n = newNode(TSK_IfStatement);
  n->expression = expression;
  n->thenStatement = thenStatement;
  n->elseStatement = elseStatement;
  return withJSDoc(finishNode(n, pos, -1), hasJSDoc);
}

static TsNode *parseDoStatement(void) {
  int pos = getNodePos();
  bool hasJSDoc = hasPrecedingJSDocComment();
  parseExpected(TSK_DoKeyword);
  TsNode *statement = parseStatement();
  parseExpected(TSK_WhileKeyword);
  bool openParenParsed = parseExpected(TSK_OpenParenToken);
  TsNode *expression = (TsNode *)allowInAnd(cb_parseExpression);
  parseExpectedMatchingBrackets(TSK_CloseParenToken, openParenParsed);
  parseOptional(TSK_SemicolonToken);
  TsNode *n = newNode(TSK_DoStatement);
  n->statement = statement;
  n->expression = expression;
  return withJSDoc(finishNode(n, pos, -1), hasJSDoc);
}

static TsNode *parseWhileStatement(void) {
  int pos = getNodePos();
  bool hasJSDoc = hasPrecedingJSDocComment();
  parseExpected(TSK_WhileKeyword);
  bool openParenParsed = parseExpected(TSK_OpenParenToken);
  TsNode *expression = (TsNode *)allowInAnd(cb_parseExpression);
  parseExpectedMatchingBrackets(TSK_CloseParenToken, openParenParsed);
  TsNode *statement = parseStatement();
  TsNode *n = newNode(TSK_WhileStatement);
  n->expression = expression;
  n->statement = statement;
  return withJSDoc(finishNode(n, pos, -1), hasJSDoc);
}

static TsNode *parseForOrForInOrForOfStatement(void) {
  int pos = getNodePos();
  bool hasJSDoc = hasPrecedingJSDocComment();
  parseExpected(TSK_ForKeyword);
  TsNode *awaitToken = parseOptionalToken(TSK_AwaitKeyword);
  parseExpected(TSK_OpenParenToken);
  TsNode *initializer = NULL;
  if (token() != TSK_SemicolonToken) {
    if (token() == TSK_VarKeyword || token() == TSK_LetKeyword || token() == TSK_ConstKeyword || token() == TSK_UsingKeyword ||
        (token() == TSK_AwaitKeyword && lookAhead(cb_nextTokenIsUsingKeywordThenBindingIdentifierOrStartOfObjectDestructuringOnSameLine))) {
      initializer = parseVariableDeclarationList(true);
    } else {
      initializer = (TsNode *)disallowInAnd(cb_parseExpression);
    }
  }
  TsNode *node;
  bool isOf = awaitToken ? parseExpected(TSK_OfKeyword) : parseOptional(TSK_OfKeyword);
  if (isOf) {
    TsNode *expression = (TsNode *)allowInAnd(cb_parseAssignmentExpressionTrue);
    parseExpected(TSK_CloseParenToken);
    node = newNode(TSK_ForOfStatement);
    node->awaitModifier = awaitToken;
    node->initializer = initializer;
    node->expression = expression;
    node->statement = parseStatement();
  } else if (parseOptional(TSK_InKeyword)) {
    TsNode *expression = (TsNode *)allowInAnd(cb_parseExpression);
    parseExpected(TSK_CloseParenToken);
    node = newNode(TSK_ForInStatement);
    node->initializer = initializer;
    node->expression = expression;
    node->statement = parseStatement();
  } else {
    parseExpected(TSK_SemicolonToken);
    TsNode *condition = (token() != TSK_SemicolonToken && token() != TSK_CloseParenToken) ? (TsNode *)allowInAnd(cb_parseExpression) : NULL;
    parseExpected(TSK_SemicolonToken);
    TsNode *incrementor = token() != TSK_CloseParenToken ? (TsNode *)allowInAnd(cb_parseExpression) : NULL;
    parseExpected(TSK_CloseParenToken);
    node = newNode(TSK_ForStatement);
    node->initializer = initializer;
    node->condition = condition;
    node->incrementor = incrementor;
    node->statement = parseStatement();
  }
  return withJSDoc(finishNode(node, pos, -1), hasJSDoc);
}

static TsNode *parseBreakOrContinueStatement(int kind) {
  int pos = getNodePos();
  bool hasJSDoc = hasPrecedingJSDocComment();
  parseExpected(kind == TSK_BreakStatement ? TSK_BreakKeyword : TSK_ContinueKeyword);
  TsNode *label = canParseSemicolon() ? NULL : parseIdentifier();
  parseSemicolon();
  TsNode *n = newNode(kind);
  n->label = label;
  return withJSDoc(finishNode(n, pos, -1), hasJSDoc);
}

static TsNode *parseReturnStatement(void) {
  int pos = getNodePos();
  bool hasJSDoc = hasPrecedingJSDocComment();
  parseExpected(TSK_ReturnKeyword);
  TsNode *expression = canParseSemicolon() ? NULL : (TsNode *)allowInAnd(cb_parseExpression);
  parseSemicolon();
  TsNode *n = newNode(TSK_ReturnStatement);
  n->expression = expression;
  return withJSDoc(finishNode(n, pos, -1), hasJSDoc);
}

static TsNode *parseWithStatement(void) {
  int pos = getNodePos();
  bool hasJSDoc = hasPrecedingJSDocComment();
  parseExpected(TSK_WithKeyword);
  bool openParenParsed = parseExpected(TSK_OpenParenToken);
  TsNode *expression = (TsNode *)allowInAnd(cb_parseExpression);
  parseExpectedMatchingBrackets(TSK_CloseParenToken, openParenParsed);
  TsNode *statement = (TsNode *)doInsideOfContext(67108864 /* InWithStatement */, cb_parseStatement);
  TsNode *n = newNode(TSK_WithStatement);
  n->expression = expression;
  n->statement = statement;
  return withJSDoc(finishNode(n, pos, -1), hasJSDoc);
}

static TsNode *parseCaseClause(void) {
  int pos = getNodePos();
  bool hasJSDoc = hasPrecedingJSDocComment();
  parseExpected(TSK_CaseKeyword);
  TsNode *expression = (TsNode *)allowInAnd(cb_parseExpression);
  parseExpected(TSK_ColonToken);
  TsList *statements = parseList(PC_SwitchClauseStatements, parseStatementFn);
  TsNode *n = newNode(TSK_CaseClause);
  n->expression = expression;
  n->statements = statements;
  return withJSDoc(finishNode(n, pos, -1), hasJSDoc);
}

static TsNode *parseDefaultClause(void) {
  int pos = getNodePos();
  parseExpected(TSK_DefaultKeyword);
  parseExpected(TSK_ColonToken);
  TsList *statements = parseList(PC_SwitchClauseStatements, parseStatementFn);
  TsNode *n = newNode(TSK_DefaultClause);
  n->statements = statements;
  return finishNode(n, pos, -1);
}

static TsNode *parseCaseOrDefaultClause(void) {
  return token() == TSK_CaseKeyword ? parseCaseClause() : parseDefaultClause();
}

static TsNode *parseCaseBlock(void) {
  int pos = getNodePos();
  parseExpected(TSK_OpenBraceToken);
  TsList *clauses = parseList(PC_SwitchClauses, parseCaseOrDefaultClause);
  parseExpected(TSK_CloseBraceToken);
  TsNode *n = newNode(TSK_CaseBlock);
  n->clauses = clauses;
  return finishNode(n, pos, -1);
}

static TsNode *parseSwitchStatement(void) {
  int pos = getNodePos();
  bool hasJSDoc = hasPrecedingJSDocComment();
  parseExpected(TSK_SwitchKeyword);
  parseExpected(TSK_OpenParenToken);
  TsNode *expression = (TsNode *)allowInAnd(cb_parseExpression);
  parseExpected(TSK_CloseParenToken);
  TsNode *caseBlock = parseCaseBlock();
  TsNode *n = newNode(TSK_SwitchStatement);
  n->expression = expression;
  n->caseBlock = caseBlock;
  return withJSDoc(finishNode(n, pos, -1), hasJSDoc);
}

static TsNode *parseThrowStatement(void) {
  int pos = getNodePos();
  bool hasJSDoc = hasPrecedingJSDocComment();
  parseExpected(TSK_ThrowKeyword);
  TsNode *expression = (TS.tflags & TF_PrecedingLineBreak) ? NULL : (TsNode *)allowInAnd(cb_parseExpression);
  if (!expression) {
    P.identifierCount++;
    expression = finishNode(newIdentifier("", 0, 0), getNodePos(), -1);
  }
  if (!tryParseSemicolon()) parseErrorAtPosition(expression->pos);
  TsNode *n = newNode(TSK_ThrowStatement);
  n->expression = expression;
  return withJSDoc(finishNode(n, pos, -1), hasJSDoc);
}

static TsNode *parseTryStatement(void) {
  int pos = getNodePos();
  bool hasJSDoc = hasPrecedingJSDocComment();
  parseExpected(TSK_TryKeyword);
  TsNode *tryBlock = parseBlock(false);
  TsNode *catchClause = token() == TSK_CatchKeyword ? parseCatchClause() : NULL;
  TsNode *finallyBlock = NULL;
  if (!catchClause || token() == TSK_FinallyKeyword) {
    parseExpected(TSK_FinallyKeyword);
    finallyBlock = parseBlock(false);
  }
  TsNode *n = newNode(TSK_TryStatement);
  n->block = tryBlock;
  n->catchClause = catchClause;
  n->finallyBlock = finallyBlock;
  return withJSDoc(finishNode(n, pos, -1), hasJSDoc);
}

static TsNode *parseCatchClause(void) {
  int pos = getNodePos();
  parseExpected(TSK_CatchKeyword);
  TsNode *variableDeclaration = NULL;
  if (parseOptional(TSK_OpenParenToken)) {
    variableDeclaration = parseVariableDeclaration(false);
    parseExpected(TSK_CloseParenToken);
  }
  TsNode *block = parseBlock(false);
  TsNode *n = newNode(TSK_CatchClause);
  n->variableDeclaration = variableDeclaration;
  n->block = block;
  return finishNode(n, pos, -1);
}

/* ======================= part 10: expression statements, declaration lookahead, parseStatement ======================= */

static bool nextTokenIsIdentifierOrKeywordOnSameLine(void) {
  nextToken();
  return tokenIsIdentifierOrKeyword(token()) && !(TS.tflags & TF_PrecedingLineBreak);
}
static bool nextTokenIsClassKeywordOnSameLine(void) {
  nextToken();
  return token() == TSK_ClassKeyword && !(TS.tflags & TF_PrecedingLineBreak);
}
static bool nextTokenIsIdentifierOrKeywordOrLiteralOnSameLine(void) {
  nextToken();
  return (tokenIsIdentifierOrKeyword(token()) || token() == TSK_NumericLiteral || token() == TSK_BigIntLiteral ||
          token() == TSK_StringLiteral) && !(TS.tflags & TF_PrecedingLineBreak);
}
static bool nextTokenIsIdentifierOrStringLiteralOnSameLine(void) {
  nextToken();
  return !(TS.tflags & TF_PrecedingLineBreak) && (isIdentifier2() || token() == TSK_StringLiteral);
}
static bool nextTokenIsStringLiteral(void) {
  nextToken();
  return token() == TSK_StringLiteral;
}
static bool nextTokenIsOpenParen(void) { return nextToken() == TSK_OpenParenToken; }
static bool nextTokenIsIdentifierCb(void) { return nextTokenIsIdentifier(); }

static intptr_t cb_nextTokenIsIdentifierOrKeywordOrLiteralOnSameLine(void) { return nextTokenIsIdentifierOrKeywordOrLiteralOnSameLine(); }
static intptr_t cb_nextTokenIsIdentifierOrKeywordOnSameLine(void) { return nextTokenIsIdentifierOrKeywordOnSameLine(); }
static intptr_t cb_nextTokenIsClassKeywordOnSameLine(void) { return nextTokenIsClassKeywordOnSameLine(); }
static intptr_t cb_nextTokenIsOpenParen(void) { return nextTokenIsOpenParen(); }

/* parseExpressionOrLabeledStatement */
static TsNode *parseExpressionOrLabeledStatement(void) {
  int pos = getNodePos();
  bool hasJSDoc = hasPrecedingJSDocComment();
  TsNode *node;
  bool hasParen = token() == TSK_OpenParenToken;
  TsNode *expression = (TsNode *)allowInAnd(cb_parseExpression);
  if (expression->kind == TSK_Identifier && parseOptional(TSK_ColonToken)) {
    node = newNode(TSK_LabeledStatement);
    node->label = expression;
    node->statement = parseStatement();
  } else {
    if (!tryParseSemicolon()) parseErrorAtPosition(expression->pos);
    node = newNode(TSK_ExpressionStatement);
    node->expression = expression;
    if (hasParen) hasJSDoc = false;
  }
  return withJSDoc(finishNode(node, pos, -1), hasJSDoc);
}

static bool isDeclareModifierNode(TsNode *m) { return m->kind == TSK_DeclareKeyword; }

static bool isDeclaration2(void) {
  for (;;) {
    switch (token()) {
      case TSK_VarKeyword: case TSK_LetKeyword: case TSK_ConstKeyword: case TSK_FunctionKeyword:
      case TSK_ClassKeyword: case TSK_EnumKeyword:
        return true;
      case TSK_UsingKeyword:
        return isUsingDeclaration();
      case TSK_AwaitKeyword:
        return isAwaitUsingDeclaration();
      case TSK_InterfaceKeyword: case TSK_TypeKeyword: case TSK_DeferKeyword:
        return nextTokenIsIdentifierOnSameLine();
      case TSK_ModuleKeyword: case TSK_NamespaceKeyword:
        return nextTokenIsIdentifierOrStringLiteralOnSameLine();
      case TSK_AbstractKeyword: case TSK_AccessorKeyword: case TSK_AsyncKeyword: case TSK_DeclareKeyword:
      case TSK_PrivateKeyword: case TSK_ProtectedKeyword: case TSK_PublicKeyword: case TSK_ReadonlyKeyword: {
        int previousToken = token();
        nextToken();
        if (TS.tflags & TF_PrecedingLineBreak) return false;
        if (previousToken == TSK_DeclareKeyword && token() == TSK_TypeKeyword) return true;
        continue;
      }
      case TSK_GlobalKeyword:
        nextToken();
        return token() == TSK_OpenBraceToken || token() == TSK_Identifier || token() == TSK_ExportKeyword;
      case TSK_ImportKeyword:
        nextToken();
        return token() == TSK_DeferKeyword || token() == TSK_StringLiteral || token() == TSK_AsteriskToken || token() == TSK_OpenBraceToken || isIdentifier2();
      case TSK_ExportKeyword: {
        int currentToken2 = nextToken();
        if (currentToken2 == TSK_TypeKeyword) currentToken2 = lookAhead(cb_nextToken);
        if (currentToken2 == TSK_EqualsToken || currentToken2 == TSK_AsteriskToken || currentToken2 == TSK_OpenBraceToken ||
            currentToken2 == TSK_DefaultKeyword || currentToken2 == TSK_AsKeyword) {
          return true;
        }
        continue;
      }
      case TSK_StaticKeyword:
        nextToken();
        continue;
      default:
        return false;
    }
  }
}

static bool isStartOfDeclaration(void) { return lookAhead(cb_isDeclaration2) != 0; }

static bool isStartOfStatement(void) {
  switch (token()) {
    case TSK_AtToken: case TSK_SemicolonToken: case TSK_OpenBraceToken: case TSK_VarKeyword: case TSK_LetKeyword:
    case TSK_UsingKeyword: case TSK_FunctionKeyword: case TSK_ClassKeyword: case TSK_EnumKeyword: case TSK_IfKeyword:
    case TSK_DoKeyword: case TSK_WhileKeyword: case TSK_ForKeyword: case TSK_ContinueKeyword: case TSK_BreakKeyword:
    case TSK_ReturnKeyword: case TSK_WithKeyword: case TSK_SwitchKeyword: case TSK_ThrowKeyword: case TSK_TryKeyword:
    case TSK_DebuggerKeyword: case TSK_CatchKeyword: case TSK_FinallyKeyword:
      return true;
    case TSK_ImportKeyword:
      return isStartOfDeclaration() || lookAhead(cb_nextTokenIsOpenParenOrLessThanOrDot) != 0;
    case TSK_ConstKeyword: case TSK_ExportKeyword:
      return isStartOfDeclaration();
    case TSK_AsyncKeyword: case TSK_DeclareKeyword: case TSK_InterfaceKeyword: case TSK_ModuleKeyword:
    case TSK_NamespaceKeyword: case TSK_TypeKeyword: case TSK_GlobalKeyword: case TSK_DeferKeyword:
      return true;
    case TSK_AccessorKeyword: case TSK_PublicKeyword: case TSK_PrivateKeyword: case TSK_ProtectedKeyword:
    case TSK_StaticKeyword: case TSK_ReadonlyKeyword:
      return isStartOfDeclaration() || !lookAhead(cb_nextTokenIsIdentifierOrKeywordOnSameLine);
    default:
      return isStartOfExpression();
  }
}

static bool nextTokenIsBindingIdentifierOrStartOfDestructuring(void) {
  nextToken();
  return isBindingIdentifier() || token() == TSK_OpenBraceToken || token() == TSK_OpenBracketToken;
}
static bool isLetDeclaration(void) { return lookAhead(cb_nextTokenIsBindingIdentifierOrStartOfDestructuring) != 0; }

static bool nextTokenIsEqualsOrSemicolonOrColonToken(void) {
  nextToken();
  return token() == TSK_EqualsToken || token() == TSK_SemicolonToken || token() == TSK_ColonToken;
}

static bool nextTokenIsBindingIdentifierOrStartOfDestructuringOnSameLine(bool disallowOf) {
  nextToken();
  if (disallowOf && token() == TSK_OfKeyword) return lookAhead(cb_nextTokenIsEqualsOrSemicolonOrColonToken) != 0;
  return (isBindingIdentifier() || token() == TSK_OpenBraceToken) && !(TS.tflags & TF_PrecedingLineBreak);
}
static intptr_t cb_nextTokenIsBindingIdentifierOrStartOfDestructuringOnSameLineFalse(void) { return nextTokenIsBindingIdentifierOrStartOfDestructuringOnSameLine(false); }
static intptr_t cb_nextTokenIsBindingIdentifierOrStartOfDestructuringOnSameLineDisallowOf(void) { return nextTokenIsBindingIdentifierOrStartOfDestructuringOnSameLine(true); }

static bool isUsingDeclaration(void) {
  return lookAhead(cb_nextTokenIsBindingIdentifierOrStartOfDestructuringOnSameLineFalse) != 0;
}

static bool nextTokenIsUsingKeywordThenBindingIdentifierOrStartOfObjectDestructuringOnSameLine(void) {
  if (nextToken() == TSK_UsingKeyword) return nextTokenIsBindingIdentifierOrStartOfDestructuringOnSameLine(false);
  return false;
}

static bool isAwaitUsingDeclaration(void) {
  return lookAhead(cb_nextTokenIsUsingKeywordThenBindingIdentifierOrStartOfObjectDestructuringOnSameLine) != 0;
}

static TsNode *parseDeclaration(void);
static TsNode *parseVariableStatement(int pos, bool hasJSDoc, TsList *modifiers);
static TsNode *parseFunctionDeclaration(int pos, bool hasJSDoc, TsList *modifiers);
static TsNode *parseClassDeclaration(int pos, bool hasJSDoc, TsList *modifiers);

static TsNode *parseStatement(void) {
  switch (token()) {
    case TSK_SemicolonToken:
      return parseEmptyStatement();
    case TSK_OpenBraceToken:
      return parseBlock(false);
    case TSK_VarKeyword:
      return parseVariableStatement(getNodePos(), hasPrecedingJSDocComment(), NULL);
    case TSK_LetKeyword:
      if (isLetDeclaration()) return parseVariableStatement(getNodePos(), hasPrecedingJSDocComment(), NULL);
      break;
    case TSK_AwaitKeyword:
      if (isAwaitUsingDeclaration()) return parseVariableStatement(getNodePos(), hasPrecedingJSDocComment(), NULL);
      break;
    case TSK_UsingKeyword:
      if (isUsingDeclaration()) return parseVariableStatement(getNodePos(), hasPrecedingJSDocComment(), NULL);
      break;
    case TSK_FunctionKeyword:
      return parseFunctionDeclaration(getNodePos(), hasPrecedingJSDocComment(), NULL);
    case TSK_ClassKeyword:
      return parseClassDeclaration(getNodePos(), hasPrecedingJSDocComment(), NULL);
    case TSK_IfKeyword:
      return parseIfStatement();
    case TSK_DoKeyword:
      return parseDoStatement();
    case TSK_WhileKeyword:
      return parseWhileStatement();
    case TSK_ForKeyword:
      return parseForOrForInOrForOfStatement();
    case TSK_ContinueKeyword:
      return parseBreakOrContinueStatement(TSK_ContinueStatement);
    case TSK_BreakKeyword:
      return parseBreakOrContinueStatement(TSK_BreakStatement);
    case TSK_ReturnKeyword:
      return parseReturnStatement();
    case TSK_WithKeyword:
      return parseWithStatement();
    case TSK_SwitchKeyword:
      return parseSwitchStatement();
    case TSK_ThrowKeyword:
      return parseThrowStatement();
    case TSK_TryKeyword: case TSK_CatchKeyword: case TSK_FinallyKeyword:
      return parseTryStatement();
    case TSK_DebuggerKeyword:
      return parseDebuggerStatement();
    case TSK_AtToken:
      return parseDeclaration();
    case TSK_AsyncKeyword: case TSK_InterfaceKeyword: case TSK_TypeKeyword: case TSK_ModuleKeyword:
    case TSK_NamespaceKeyword: case TSK_DeclareKeyword: case TSK_ConstKeyword: case TSK_EnumKeyword:
    case TSK_ExportKeyword: case TSK_ImportKeyword: case TSK_PrivateKeyword: case TSK_ProtectedKeyword:
    case TSK_PublicKeyword: case TSK_AbstractKeyword: case TSK_AccessorKeyword: case TSK_StaticKeyword:
    case TSK_ReadonlyKeyword: case TSK_GlobalKeyword:
      if (isStartOfDeclaration()) return parseDeclaration();
      break;
    default:
      break;
  }
  return parseExpressionOrLabeledStatement();
}

static TsNode *parseDebuggerStatement(void) {
  int pos = getNodePos();
  bool hasJSDoc = hasPrecedingJSDocComment();
  parseExpected(TSK_DebuggerKeyword);
  parseSemicolon();
  return withJSDoc(finishNode(newNode(TSK_DebuggerStatement), pos, -1), hasJSDoc);
}

/* ======================= part 11: declarations, binding patterns, variables, functions, members ======================= */


/* modifiersToFlags: ModifierFlags of a modifier list (typescript.js modifiersToFlags) */
static int modifiersToFlags(TsList *modifiers) {
  int flags = 0;
  if (!modifiers) return 0;
  for (int i = 0; i < modifiers->n; i++) {
    switch (modifiers->v[i]->kind) {
      case TSK_StaticKeyword: flags |= TMF_Static; break;
      case TSK_PublicKeyword: flags |= TMF_Public; break;
      case TSK_ProtectedKeyword: flags |= TMF_Protected; break;
      case TSK_PrivateKeyword: flags |= TMF_Private; break;
      case TSK_AbstractKeyword: flags |= TMF_Abstract; break;
      case TSK_AccessorKeyword: flags |= TMF_Accessor; break;
      case TSK_ExportKeyword: flags |= TMF_Export; break;
      case TSK_DeclareKeyword: flags |= TMF_Ambient; break;
      case TSK_ConstKeyword: flags |= TMF_Const; break;
      case TSK_DefaultKeyword: flags |= TMF_Default; break;
      case TSK_AsyncKeyword: flags |= TMF_Async; break;
      case TSK_ReadonlyKeyword: flags |= TMF_Readonly; break;
      case TSK_OverrideKeyword: flags |= TMF_Override; break;
      case TSK_InKeyword: flags |= TMF_In; break;
      case TSK_OutKeyword: flags |= TMF_Out; break;
      default: break;
    }
  }
  return flags;
}

static TsNode *parseDeclarationWorker(int pos, bool hasJSDoc, TsList *modifiersIn);
static intptr_t cb_parseDeclarationWorkerNone(void) { return 0; }

static TsNode *parseDeclaration(void) {
  int pos = getNodePos();
  bool hasJSDoc = hasPrecedingJSDocComment();
  TsList *modifiers = parseModifiers(true, false, false);
  bool isAmbient = false;
  if (modifiers) for (int i = 0; i < modifiers->n; i++) if (isDeclareModifierNode(modifiers->v[i])) isAmbient = true;
  if (isAmbient) {
    if (modifiers) for (int i = 0; i < modifiers->n; i++) modifiers->v[i]->flags |= TNF_Ambient;
    int toSet = CF_Ambient & ~P.contextFlags;
    if (toSet) setContextFlag(true, toSet);
    TsNode *r = parseDeclarationWorker(pos, hasJSDoc, modifiers);
    if (toSet) setContextFlag(false, toSet);
    return r;
  }
  return parseDeclarationWorker(pos, hasJSDoc, modifiers);
}

static TsNode *parseInterfaceDeclaration(int pos, bool hasJSDoc, TsList *modifiers);
static TsNode *parseTypeAliasDeclaration(int pos, bool hasJSDoc, TsList *modifiers);
static TsNode *parseEnumDeclaration(int pos, bool hasJSDoc, TsList *modifiers);
static TsNode *parseModuleDeclaration(int pos, bool hasJSDoc, TsList *modifiers);
static TsNode *parseImportDeclarationOrImportEqualsDeclaration(int pos, bool hasJSDoc, TsList *modifiers);
static TsNode *parseExportAssignment(int pos, bool hasJSDoc, TsList *modifiers);
static TsNode *parseNamespaceExportDeclaration(int pos, bool hasJSDoc, TsList *modifiers);
static TsNode *parseExportDeclaration(int pos, bool hasJSDoc, TsList *modifiers);

static TsNode *parseDeclarationWorker(int pos, bool hasJSDoc, TsList *modifiersIn) {
  switch (token()) {
    case TSK_VarKeyword: case TSK_LetKeyword: case TSK_ConstKeyword: case TSK_UsingKeyword: case TSK_AwaitKeyword:
      return parseVariableStatement(pos, hasJSDoc, modifiersIn);
    case TSK_FunctionKeyword:
      return parseFunctionDeclaration(pos, hasJSDoc, modifiersIn);
    case TSK_ClassKeyword:
      return parseClassDeclaration(pos, hasJSDoc, modifiersIn);
    case TSK_InterfaceKeyword:
      return parseInterfaceDeclaration(pos, hasJSDoc, modifiersIn);
    case TSK_TypeKeyword:
      return parseTypeAliasDeclaration(pos, hasJSDoc, modifiersIn);
    case TSK_EnumKeyword:
      return parseEnumDeclaration(pos, hasJSDoc, modifiersIn);
    case TSK_GlobalKeyword: case TSK_ModuleKeyword: case TSK_NamespaceKeyword:
      return parseModuleDeclaration(pos, hasJSDoc, modifiersIn);
    case TSK_ImportKeyword:
      return parseImportDeclarationOrImportEqualsDeclaration(pos, hasJSDoc, modifiersIn);
    case TSK_ExportKeyword:
      nextToken();
      switch (token()) {
        case TSK_DefaultKeyword:
        case TSK_EqualsToken:
          return parseExportAssignment(pos, hasJSDoc, modifiersIn);
        case TSK_AsKeyword:
          return parseNamespaceExportDeclaration(pos, hasJSDoc, modifiersIn);
        default:
          return parseExportDeclaration(pos, hasJSDoc, modifiersIn);
      }
    default:
      if (modifiersIn) {
        TsNode *missing = createMissingNode(TSK_MissingDeclaration, true);
        missing->pos = pos;
        missing->modifiers = modifiersIn;
        return missing;
      }
      return NULL;
  }
}

static bool nextTokenIsFromKeywordOrEqualsToken(void) {
  nextToken();
  return token() == TSK_FromKeyword || token() == TSK_EqualsToken;
}

static TsNode *parseFunctionBlockOrSemicolon(int flags, int diagnosticMessage) {
  if (token() != TSK_OpenBraceToken) {
    if (flags & 4 /* Type */) {
      parseTypeMemberSemicolon();
      return NULL;
    }
    if (canParseSemicolon()) {
      parseSemicolon();
      return NULL;
    }
  }
  return parseFunctionBlock(flags, diagnosticMessage);
}

static TsNode *parseArrayBindingElement(void) {
  int pos = getNodePos();
  if (token() == TSK_CommaToken) return finishNode(newNode(TSK_OmittedExpression), pos, -1);
  TsNode *dotDotDotToken = parseOptionalToken(TSK_DotDotDotToken);
  TsNode *name = parseIdentifierOrPattern();
  TsNode *initializer = parseInitializer();
  TsNode *n = newNode(TSK_BindingElement);
  n->dotDotDotToken = dotDotDotToken;
  n->name = name;
  n->initializer = initializer;
  return finishNode(n, pos, -1);
}

static TsNode *parseObjectBindingElement(void) {
  int pos = getNodePos();
  TsNode *dotDotDotToken = parseOptionalToken(TSK_DotDotDotToken);
  bool tokenIsIdentifier = isBindingIdentifier();
  TsNode *propertyName = parsePropertyName();
  TsNode *name;
  if (tokenIsIdentifier && token() != TSK_ColonToken) {
    name = propertyName;
    propertyName = NULL;
  } else {
    parseExpected(TSK_ColonToken);
    name = parseIdentifierOrPattern();
  }
  TsNode *initializer = parseInitializer();
  TsNode *n = newNode(TSK_BindingElement);
  n->dotDotDotToken = dotDotDotToken;
  n->propertyName = propertyName;
  n->name = name;
  n->initializer = initializer;
  return finishNode(n, pos, -1);
}

static TsNode *parseObjectBindingPattern(void) {
  int pos = getNodePos();
  parseExpected(TSK_OpenBraceToken);
  TsList *elements = (TsList *)allowInAnd(cb_parseObjectBindingElementList);
  parseExpected(TSK_CloseBraceToken);
  TsNode *n = newNode(TSK_ObjectBindingPattern);
  n->elements = elements;
  return finishNode(n, pos, -1);
}

static TsNode *parseArrayBindingPattern(void) {
  int pos = getNodePos();
  parseExpected(TSK_OpenBracketToken);
  TsList *elements = (TsList *)allowInAnd(cb_parseArrayBindingElementList);
  parseExpected(TSK_CloseBracketToken);
  TsNode *n = newNode(TSK_ArrayBindingPattern);
  n->elements = elements;
  return finishNode(n, pos, -1);
}

static intptr_t cb_parseObjectBindingElementList(void) { return (intptr_t)parseDelimitedList(PC_ObjectBindingElements, parseObjectBindingElement, false); }
static intptr_t cb_parseArrayBindingElementList(void) { return (intptr_t)parseDelimitedList(PC_ArrayBindingElements, parseArrayBindingElement, false); }

static bool isBindingIdentifierOrPrivateIdentifierOrPattern(void) {
  return token() == TSK_OpenBraceToken || token() == TSK_OpenBracketToken || token() == TSK_PrivateIdentifier || isBindingIdentifier();
}

static TsNode *parseIdentifierOrPattern(void) {
  if (token() == TSK_OpenBracketToken) return parseArrayBindingPattern();
  if (token() == TSK_OpenBraceToken) return parseObjectBindingPattern();
  return parseBindingIdentifier();
}

static TsNode *parseVariableDeclaration(bool allowExclamation) {
  int pos = getNodePos();
  bool hasJSDoc = hasPrecedingJSDocComment();
  TsNode *name = parseIdentifierOrPattern();
  TsNode *exclamationToken = NULL;
  if (allowExclamation && name->kind == TSK_Identifier && token() == TSK_ExclamationToken && !(TS.tflags & TF_PrecedingLineBreak)) {
    exclamationToken = parseTokenNode();
  }
  TsNode *type = parseTypeAnnotation();
  TsNode *initializer = isInOrOfKeyword(token()) ? NULL : parseInitializer();
  TsNode *n = newNode(TSK_VariableDeclaration);
  n->name = name;
  n->exclamationToken = exclamationToken;
  n->type = type;
  n->initializer = initializer;
  return withJSDoc(finishNode(n, pos, -1), hasJSDoc);
}
static TsNode *parseVariableDeclarationAllowExclamation(void) { return parseVariableDeclaration(true); }
static TsNode *parseVariableDeclarationNoExclamation(void) { return parseVariableDeclaration(false); }

static bool canFollowContextualOfKeyword(void) {
  return nextTokenIsIdentifier() && nextToken() == TSK_CloseParenToken;
}
static intptr_t cb_canFollowContextualOfKeyword(void) { return canFollowContextualOfKeyword(); }


static TsNode *parseVariableDeclarationList(bool inForStatementInitializer) {
  int pos = getNodePos();
  int flags = 0;
  switch (token()) {
    case TSK_VarKeyword: break;
    case TSK_LetKeyword: flags |= 1; break;
    case TSK_ConstKeyword: flags |= 2; break;
    case TSK_UsingKeyword: flags |= 4; break;
    case TSK_AwaitKeyword:
      flags |= 6;
      nextToken();
      break;
    default:
      break;
  }
  nextToken();
  TsList *declarations;
  if (token() == TSK_OfKeyword && lookAhead(cb_canFollowContextualOfKeyword)) {
    declarations = createMissingList();
  } else {
    bool savedDisallowIn = inDisallowInContext();
    setDisallowInContext(inForStatementInitializer);
    declarations = parseDelimitedList(PC_VariableDeclarations,
                                      inForStatementInitializer ? parseVariableDeclarationNoExclamation : parseVariableDeclarationAllowExclamation, false);
    setDisallowInContext(savedDisallowIn);
  }
  TsNode *n = newNode(TSK_VariableDeclarationList);
  n->declarations = declarations;
  n->extra = flags;
  n->flags |= (uint32_t)flags; /* NodeFlags.Let / Const / Using: the binder reads them */
  return finishNode(n, pos, -1);
}

static TsNode *parseVariableStatement(int pos, bool hasJSDoc, TsList *modifiers) {
  TsNode *declarationList = parseVariableDeclarationList(false);
  parseSemicolon();
  TsNode *n = newNode(TSK_VariableStatement);
  n->modifiers = modifiers;
  n->declarationList = declarationList;
  return withJSDoc(finishNode(n, pos, -1), hasJSDoc);
}

static TsNode *parseFunctionDeclaration(int pos, bool hasJSDoc, TsList *modifiers) {
  bool savedAwaitContext = inAwaitContext();
  int modifierFlags = modifiersToFlags(modifiers);
  parseExpected(TSK_FunctionKeyword);
  TsNode *asteriskToken = parseOptionalToken(TSK_AsteriskToken);
  TsNode *name = (modifierFlags & TMF_Default) ? parseOptionalBindingIdentifier() : parseBindingIdentifier();
  int isGenerator = asteriskToken ? 1 : 0;
  int isAsync = (modifierFlags & TMF_Async) ? 2 : 0;
  TsList *typeParameters = parseTypeParameters();
  if (modifierFlags & TMF_Export) setAwaitContext(true);
  TsList *parameters = parseParameters(isGenerator | isAsync);
  TsNode *type = parseReturnType(TSK_ColonToken, false);
  TsNode *body = parseFunctionBlockOrSemicolon(isGenerator | isAsync, 0);
  setAwaitContext(savedAwaitContext);
  TsNode *n = newNode(TSK_FunctionDeclaration);
  n->modifiers = modifiers;
  n->asteriskToken = asteriskToken;
  n->name = name;
  n->typeParameters = typeParameters;
  n->parameters = parameters;
  n->type = type;
  n->body = body;
  return withJSDoc(finishNode(n, pos, -1), hasJSDoc);
}


/* ======================= part 12: decorators, modifiers, class members, classes, interfaces, enums, modules ======================= */

static TsNode *parseDecoratorExpression(void) {
  if (inAwaitContext() && token() == TSK_AwaitKeyword) {
    int pos = getNodePos();
    TsNode *awaitExpression = parseIdentifier();
    nextToken();
    TsNode *memberExpression = parseMemberExpressionRest(pos, awaitExpression, true);
    return parseCallExpressionRest(pos, memberExpression);
  }
  return parseLeftHandSideExpressionOrHigher();
}
static intptr_t cb_parseDecoratorExpression(void) { return (intptr_t)parseDecoratorExpression(); }

static TsNode *tryParseDecorator(void) {
  int pos = getNodePos();
  if (!parseOptional(TSK_AtToken)) return NULL;
  TsNode *expression = (TsNode *)doInDecoratorContext(cb_parseDecoratorExpression);
  TsNode *n = newNode(TSK_Decorator);
  n->expression = expression;
  return finishNode(n, pos, -1);
}

static bool nextTokenIsOpenBrace(void) { return nextToken() == TSK_OpenBraceToken; }
static intptr_t cb_nextTokenIsOpenBrace(void) { return nextTokenIsOpenBrace(); }

static bool nextTokenIsOnSameLineAndCanFollowModifier(void);
static bool canFollowModifier(void);
static intptr_t cb_nextTokenIsOnSameLineAndCanFollowModifier(void) { return nextTokenIsOnSameLineAndCanFollowModifier(); }

static TsNode *tryParseModifier(bool hasSeenStaticModifier, bool permitConstAsModifier, bool stopOnStartOfClassStaticBlock) {
  int pos = getNodePos();
  int kind = token();
  if (token() == TSK_ConstKeyword && permitConstAsModifier) {
    if (!tryParse(cb_nextTokenIsOnSameLineAndCanFollowModifier)) return NULL;
  } else if (stopOnStartOfClassStaticBlock && token() == TSK_StaticKeyword && lookAhead(cb_nextTokenIsOpenBrace)) {
    return NULL;
  } else if (hasSeenStaticModifier && token() == TSK_StaticKeyword) {
    return NULL;
  } else {
    if (!parseAnyContextualModifier()) return NULL;
  }
  return finishNode(newNode(kind), pos, -1);
}

/* append to a list being built on the node stack: returns the mark taken by the caller */
static TsList *parseModifiers(bool allowDecorators, bool permitConstAsModifier, bool stopOnStartOfClassStaticBlock) {
  int pos = getNodePos();
  int mark = ts_stk_n;
  TsNode *decorator, *modifier;
  bool hasSeenStaticModifier = false, hasLeadingModifier = false, hasTrailingDecorator = false;
  if (allowDecorators && token() == TSK_AtToken) {
    while ((decorator = tryParseDecorator()) != NULL) ts_stk_push(decorator);
  }
  while ((modifier = tryParseModifier(hasSeenStaticModifier, permitConstAsModifier, stopOnStartOfClassStaticBlock)) != NULL) {
    if (modifier->kind == TSK_StaticKeyword) hasSeenStaticModifier = true;
    ts_stk_push(modifier);
    hasLeadingModifier = true;
  }
  if (hasLeadingModifier && allowDecorators && token() == TSK_AtToken) {
    while ((decorator = tryParseDecorator()) != NULL) {
      ts_stk_push(decorator);
      hasTrailingDecorator = true;
    }
  }
  if (hasTrailingDecorator) {
    while ((modifier = tryParseModifier(hasSeenStaticModifier, permitConstAsModifier, stopOnStartOfClassStaticBlock)) != NULL) {
      if (modifier->kind == TSK_StaticKeyword) hasSeenStaticModifier = true;
      ts_stk_push(modifier);
    }
  }
  if (ts_stk_n == mark) return NULL;
  return createNodeArrayFrom(mark, pos, -1, false);
}

static TsNode *parseModifiersForArrowFunction(void) {
  if (token() == TSK_AsyncKeyword) {
    int pos = getNodePos();
    nextToken();
    return finishNode(newNode(TSK_AsyncKeyword), pos, -1);
  }
  return NULL;
}

static bool isClassMemberModifier(int idToken) {
  switch (idToken) {
    case TSK_PublicKeyword: case TSK_PrivateKeyword: case TSK_ProtectedKeyword: case TSK_StaticKeyword:
    case TSK_OverrideKeyword: case TSK_ReadonlyKeyword: case TSK_AccessorKeyword:
      return true;
  }
  return false;
}

static TsNode *parseClassStaticBlockDeclaration(int pos, bool hasJSDoc, TsList *modifiers);
static TsNode *parseIndexSignatureDeclaration(int pos, bool hasJSDoc, TsList *modifiers);
static TsNode *parsePropertyOrMethodDeclaration(int pos, bool hasJSDoc, TsList *modifiers);
static TsNode *tryParseConstructorDeclaration(int pos, bool hasJSDoc, TsList *modifiers);
static TsNode *parseDecoratedExpression(void);
static TsNode *parseClassDeclarationOrExpression(int pos, bool hasJSDoc, TsList *modifiers, int kind);

static TsNode *parseClassElement(void) {
  int pos = getNodePos();
  bool hasJSDoc = hasPrecedingJSDocComment();
  if (token() == TSK_SemicolonToken) {
    nextToken();
    return withJSDoc(finishNode(newNode(TSK_SemicolonClassElement), pos, -1), hasJSDoc);
  }
  TsList *modifiers = parseModifiers(true, true, true);
  if (token() == TSK_StaticKeyword && lookAhead(cb_nextTokenIsOpenBrace)) {
    return parseClassStaticBlockDeclaration(pos, hasJSDoc, modifiers);
  }
  if (parseContextualModifier(TSK_GetKeyword)) return parseAccessorDeclaration(pos, hasJSDoc, modifiers, TSK_GetAccessor, 0);
  if (parseContextualModifier(TSK_SetKeyword)) return parseAccessorDeclaration(pos, hasJSDoc, modifiers, TSK_SetAccessor, 0);
  if (token() == TSK_ConstructorKeyword || token() == TSK_StringLiteral) {
    TsNode *constructorDeclaration = tryParseConstructorDeclaration(pos, hasJSDoc, modifiers);
    if (constructorDeclaration) return constructorDeclaration;
  }
  if (isIndexSignature()) return parseIndexSignatureDeclaration(pos, hasJSDoc, modifiers);
  if (tokenIsIdentifierOrKeyword(token()) || token() == TSK_StringLiteral || token() == TSK_NumericLiteral ||
      token() == TSK_BigIntLiteral || token() == TSK_AsteriskToken || token() == TSK_OpenBracketToken) {
    bool isAmbient = false;
    if (modifiers) for (int i = 0; i < modifiers->n; i++) if (isDeclareModifierNode(modifiers->v[i])) isAmbient = true;
    if (isAmbient) {
      for (int i = 0; i < modifiers->n; i++) modifiers->v[i]->flags |= TNF_Ambient;
      int toSet = CF_Ambient & ~P.contextFlags;
      if (toSet) setContextFlag(true, toSet);
      TsNode *r = parsePropertyOrMethodDeclaration(pos, hasJSDoc, modifiers);
      if (toSet) setContextFlag(false, toSet);
      return r;
    }
    return parsePropertyOrMethodDeclaration(pos, hasJSDoc, modifiers);
  }
  if (modifiers) {
    TsNode *name = createMissingNode(TSK_Identifier, true);
    return parsePropertyDeclaration(pos, hasJSDoc, modifiers, name, NULL);
  }
  parseErrorAtCurrentToken();  /* Debug.fail in the JS: unreachable for well-formed input */
  return NULL;
}

static TsNode *parseDecoratedExpression(void) {
  int pos = getNodePos();
  bool hasJSDoc = hasPrecedingJSDocComment();
  TsList *modifiers = parseModifiers(true, false, false);
  if (token() == TSK_ClassKeyword) return parseClassDeclarationOrExpression(pos, hasJSDoc, modifiers, TSK_ClassExpression);
  TsNode *missing = createMissingNode(TSK_MissingDeclaration, true);
  missing->pos = pos;
  missing->modifiers = modifiers;
  return missing;
}

static TsNode *parseClassExpression(void) {
  return parseClassDeclarationOrExpression(getNodePos(), hasPrecedingJSDocComment(), NULL, TSK_ClassExpression);
}

static TsNode *parseClassDeclaration(int pos, bool hasJSDoc, TsList *modifiers) {
  return parseClassDeclarationOrExpression(pos, hasJSDoc, modifiers, TSK_ClassDeclaration);
}

static bool isImplementsClause(void) {
  return token() == TSK_ImplementsKeyword && lookAhead(cb_nextTokenIsIdentifierOrKeyword) != 0;
}

static bool isExportModifierNode(TsNode *m) { return m->kind == TSK_ExportKeyword; }

static TsNode *parseClassDeclarationOrExpression(int pos, bool hasJSDoc, TsList *modifiers, int kind) {
  bool savedAwaitContext = inAwaitContext();
  parseExpected(TSK_ClassKeyword);
  TsNode *name = NULL;
  if (isBindingIdentifier() && !isImplementsClause()) name = createIdentifier(isBindingIdentifier());
  TsList *typeParameters = parseTypeParameters();
  bool hasExport = false;
  if (modifiers) for (int i = 0; i < modifiers->n; i++) if (isExportModifierNode(modifiers->v[i])) hasExport = true;
  if (hasExport) setAwaitContext(true);
  TsList *heritageClauses = parseHeritageClauses();
  TsList *members;
  if (parseExpected(TSK_OpenBraceToken)) {
    members = parseClassMembers();
    parseExpected(TSK_CloseBraceToken);
  } else {
    members = createMissingList();
  }
  setAwaitContext(savedAwaitContext);
  TsNode *node = newNode(kind);
  node->modifiers = modifiers;
  node->name = name;
  node->typeParameters = typeParameters;
  node->heritageClauses = heritageClauses;
  node->members = members;
  return withJSDoc(finishNode(node, pos, -1), hasJSDoc);
}

static TsNode *parseHeritageClause(void) {
  int pos = getNodePos();
  int tok = token();
  nextToken();
  TsList *types = parseDelimitedList(PC_HeritageClauseElement, parseExpressionWithTypeArguments, false);
  TsNode *n = newNode(TSK_HeritageClause);
  n->extra = tok;
  n->types = types;
  return finishNode(n, pos, -1);
}

static TsList *tryParseTypeArguments(void) {
  return token() == TSK_LessThanToken ? parseBracketedList(PC_TypeArguments, parseType, TSK_LessThanToken, TSK_GreaterThanToken) : NULL;
}

static TsNode *parseExpressionWithTypeArguments(void) {
  int pos = getNodePos();
  TsNode *expression = parseLeftHandSideExpressionOrHigher();
  if (expression->kind == TSK_ExpressionWithTypeArguments) return expression;
  TsList *typeArguments = tryParseTypeArguments();
  TsNode *n = newNode(TSK_ExpressionWithTypeArguments);
  n->expression = expression;
  n->typeArguments = typeArguments;
  return finishNode(n, pos, -1);
}

static bool isHeritageClause2(void) { return token() == TSK_ExtendsKeyword || token() == TSK_ImplementsKeyword; }

static TsList *parseHeritageClauses(void) {
  if (isHeritageClause2()) return parseList(PC_HeritageClauses, parseHeritageClause);
  return NULL;
}

static TsList *parseClassMembers(void) { return parseList(PC_ClassMembers, parseClassElement); }

static TsNode *parseInterfaceDeclaration(int pos, bool hasJSDoc, TsList *modifiers) {
  parseExpected(TSK_InterfaceKeyword);
  TsNode *name = parseIdentifier();
  TsList *typeParameters = parseTypeParameters();
  TsList *heritageClauses = parseHeritageClauses();
  TsList *members = parseObjectTypeMembers();
  TsNode *node = newNode(TSK_InterfaceDeclaration);
  node->modifiers = modifiers;
  node->name = name;
  node->typeParameters = typeParameters;
  node->heritageClauses = heritageClauses;
  node->members = members;
  return withJSDoc(finishNode(node, pos, -1), hasJSDoc);
}

static TsNode *parseTypeAliasDeclaration(int pos, bool hasJSDoc, TsList *modifiers) {
  parseExpected(TSK_TypeKeyword);
  if (TS.tflags & TF_PrecedingLineBreak) parseErrorAtCurrentToken();
  TsNode *name = parseIdentifier();
  TsList *typeParameters = parseTypeParameters();
  parseExpected(TSK_EqualsToken);
  TsNode *type = token() == TSK_IntrinsicKeyword ? (TsNode *)tryParse(cb_parseKeywordAndNoDot) : NULL;
  if (!type) type = parseType();
  parseSemicolon();
  TsNode *node = newNode(TSK_TypeAliasDeclaration);
  node->modifiers = modifiers;
  node->name = name;
  node->typeParameters = typeParameters;
  node->type = type;
  return withJSDoc(finishNode(node, pos, -1), hasJSDoc);
}

static TsNode *parseEnumMember(void) {
  int pos = getNodePos();
  bool hasJSDoc = hasPrecedingJSDocComment();
  TsNode *name = parsePropertyName();
  TsNode *initializer = (TsNode *)allowInAnd(cb_parseInitializer);
  TsNode *n = newNode(TSK_EnumMember);
  n->name = name;
  n->initializer = initializer;
  return withJSDoc(finishNode(n, pos, -1), hasJSDoc);
}

static TsNode *parseEnumDeclaration(int pos, bool hasJSDoc, TsList *modifiers) {
  parseExpected(TSK_EnumKeyword);
  TsNode *name = parseIdentifier();
  TsList *members;
  if (parseExpected(TSK_OpenBraceToken)) {
    members = (TsList *)doOutsideOfYieldAndAwaitContext(cb_parseEnumMemberList);
    parseExpected(TSK_CloseBraceToken);
  } else {
    members = createMissingList();
  }
  TsNode *node = newNode(TSK_EnumDeclaration);
  node->modifiers = modifiers;
  node->name = name;
  node->members = members;
  return withJSDoc(finishNode(node, pos, -1), hasJSDoc);
}

static TsNode *parseModuleBlock(void) {
  int pos = getNodePos();
  TsList *statements;
  if (parseExpected(TSK_OpenBraceToken)) {
    statements = parseList(PC_BlockStatements, parseStatementFn);
    parseExpected(TSK_CloseBraceToken);
  } else {
    statements = createMissingList();
  }
  TsNode *n = newNode(TSK_ModuleBlock);
  n->statements = statements;
  return finishNode(n, pos, -1);
}

static TsNode *parseModuleOrNamespaceDeclaration(int pos, bool hasJSDoc, TsList *modifiers, int flags);

static TsNode *parseModuleOrNamespaceDeclaration(int pos, bool hasJSDoc, TsList *modifiers, int flags) {
  int namespaceFlag = flags & 32;
  TsNode *name = (flags & 8) ? parseIdentifierName() : parseIdentifier();
  TsNode *body;
  if (parseOptional(TSK_DotToken)) body = parseModuleOrNamespaceDeclaration(getNodePos(), false, NULL, 8 | namespaceFlag);
  else body = parseModuleBlock();
  TsNode *node = newNode(TSK_ModuleDeclaration);
  node->modifiers = modifiers;
  node->name = name;
  node->body = body;
  node->extra = flags;
  return withJSDoc(finishNode(node, pos, -1), hasJSDoc);
}

static TsNode *parseAmbientExternalModuleDeclaration(int pos, bool hasJSDoc, TsList *modifiersIn) {
  int flags = 0;
  TsNode *name;
  if (token() == TSK_GlobalKeyword) {
    name = parseIdentifier();
    flags |= 2048;
  } else {
    name = parseLiteralNode();
  }
  TsNode *body = NULL;
  if (token() == TSK_OpenBraceToken) body = parseModuleBlock();
  else parseSemicolon();
  TsNode *node = newNode(TSK_ModuleDeclaration);
  node->modifiers = modifiersIn;
  node->name = name;
  node->body = body;
  node->extra = flags;
  node->flags |= (uint32_t)flags; /* NodeFlags.GlobalAugmentation (2048) */
  return withJSDoc(finishNode(node, pos, -1), hasJSDoc);
}

static TsNode *parseModuleDeclaration(int pos, bool hasJSDoc, TsList *modifiersIn) {
  int flags = 0;
  if (token() == TSK_GlobalKeyword) {
    return parseAmbientExternalModuleDeclaration(pos, hasJSDoc, modifiersIn);
  } else if (parseOptional(TSK_NamespaceKeyword)) {
    flags |= 32;
  } else {
    parseExpected(TSK_ModuleKeyword);
    if (token() == TSK_StringLiteral) return parseAmbientExternalModuleDeclaration(pos, hasJSDoc, modifiersIn);
  }
  return parseModuleOrNamespaceDeclaration(pos, hasJSDoc, modifiersIn, flags);
}

static bool isExternalModuleReference2(void) {
  return token() == TSK_RequireKeyword && lookAhead(cb_nextTokenIsOpenParen) != 0;
}

static bool nextTokenIsSlash(void) { return nextToken() == TSK_SlashToken; }

static TsNode *parseNamespaceExportDeclaration(int pos, bool hasJSDoc, TsList *modifiers) {
  parseExpected(TSK_AsKeyword);
  parseExpected(TSK_NamespaceKeyword);
  TsNode *name = parseIdentifier();
  parseSemicolon();
  TsNode *node = newNode(TSK_NamespaceExportDeclaration);
  node->name = name;
  node->modifiers = modifiers;
  return withJSDoc(finishNode(node, pos, -1), hasJSDoc);
}

/* ======================= part 13: import and export declarations, source file ======================= */

static TsNode *parseImportClause(TsNode *identifier, int pos, int phaseModifier, bool skipJsDocLeadingAsterisks);

static TsNode *tryParseImportClause(TsNode *identifier, int pos, int phaseModifier, bool skipJsDocLeadingAsterisks) {
  TsNode *importClause = NULL;
  if (identifier || token() == TSK_AsteriskToken || token() == TSK_OpenBraceToken) {
    importClause = parseImportClause(identifier, pos, phaseModifier, skipJsDocLeadingAsterisks);
    parseExpected(TSK_FromKeyword);
  }
  return importClause;
}

static TsList *tryParseImportAttributes(void) {
  int currentToken2 = token();
  if ((currentToken2 == TSK_WithKeyword || currentToken2 == TSK_AssertKeyword) && !(TS.tflags & TF_PrecedingLineBreak)) {
    return parseImportAttributes(currentToken2, false);
  }
  return NULL;
}

static TsNode *parseImportAttribute(void) {
  int pos = getNodePos();
  TsNode *name = tokenIsIdentifierOrKeyword(token()) ? parseIdentifierName() : parseLiteralLikeNode(TSK_StringLiteral);
  parseExpected(TSK_ColonToken);
  TsNode *value = parseAssignmentExpressionOrHigher(true);
  TsNode *n = newNode(TSK_ImportAttribute);
  n->name = name;
  n->initializer = value;
  return finishNode(n, pos, -1);
}

/* parseImportAttributes(token2, skipKeyword): returns the attributes node list (NodeArray) */
static TsList *parseImportAttributes(int token2, bool skipKeyword) {
  int pos = getNodePos();
  if (!skipKeyword) parseExpected(token2);
  if (parseExpected(TSK_OpenBraceToken)) {
    bool multiLine = (TS.tflags & TF_PrecedingLineBreak) != 0;
    TsList *elements = parseDelimitedList(PC_ImportAttributes, parseImportAttribute, true);
    parseExpected(TSK_CloseBraceToken);
    TsNode *n = newNode(TSK_ImportAttributes);
    n->elements = elements;
    n->extra = multiLine;
    n->tokenFlags = token2;
    finishNode(n, pos, -1);
    return ts_list_single(n, pos, n->end);
  }
  TsList *elements = ts_list_empty(getNodePos(), TS.fullStart);
  TsNode *n = newNode(TSK_ImportAttributes);
  n->elements = elements;
  n->tokenFlags = token2;
  finishNode(n, pos, -1);
  return ts_list_single(n, pos, n->end);
}

static bool tokenAfterImportedIdentifierDefinitelyProducesImportDeclaration(void) {
  return token() == TSK_CommaToken || token() == TSK_FromKeyword;
}

static TsNode *parseImportEqualsDeclaration(int pos, bool hasJSDoc, TsList *modifiers, TsNode *identifier, bool isTypeOnly) {
  parseExpected(TSK_EqualsToken);
  TsNode *moduleReference = parseModuleReference();
  parseSemicolon();
  TsNode *node = newNode(TSK_ImportEqualsDeclaration);
  node->modifiers = modifiers;
  node->extra = isTypeOnly;
  node->name = identifier;
  node->moduleReference = moduleReference;
  return withJSDoc(finishNode(node, pos, -1), hasJSDoc);
}


static TsNode *parseImportDeclarationOrImportEqualsDeclaration(int pos, bool hasJSDoc, TsList *modifiers) {
  parseExpected(TSK_ImportKeyword);
  int afterImportPos = TS.fullStart;
  TsNode *identifier = NULL;
  if (isIdentifier2()) identifier = parseIdentifier();
  int phaseModifier = 0;
  bool isType = identifier && identifier->textLen == 4 && memcmp(identifier->text, "type", 4) == 0;
  bool isDefer = identifier && identifier->textLen == 5 && memcmp(identifier->text, "defer", 5) == 0;
  if (isType && (token() != TSK_FromKeyword || (isIdentifier2() && lookAhead(cb_nextTokenIsFromKeywordOrEqualsToken)))) {
    phaseModifier = TSK_TypeKeyword;
    identifier = isIdentifier2() ? parseIdentifier() : NULL;
  } else if (isDefer && (token() == TSK_FromKeyword ? !lookAhead(cb_nextTokenIsStringLiteral) : token() == TSK_FromKeyword ? 0 : (token() == TSK_FromKeyword))) {
    phaseModifier = TSK_DeferKeyword;
    identifier = isIdentifier2() ? parseIdentifier() : NULL;
  }
  if (identifier && !tokenAfterImportedIdentifierDefinitelyProducesImportDeclaration() && phaseModifier != TSK_DeferKeyword) {
    return parseImportEqualsDeclaration(pos, hasJSDoc, modifiers, identifier, phaseModifier == TSK_TypeKeyword);
  }
  TsNode *importClause = tryParseImportClause(identifier, afterImportPos, phaseModifier, false);
  TsNode *moduleSpecifier = parseModuleSpecifier();
  TsList *attributes = tryParseImportAttributes();
  parseSemicolon();
  TsNode *node = newNode(TSK_ImportDeclaration);
  node->modifiers = modifiers;
  node->importClause = importClause;
  node->moduleSpecifier = moduleSpecifier;
  node->attributes = attributes;
  return withJSDoc(finishNode(node, pos, -1), hasJSDoc);
}

static TsNode *parseImportClause(TsNode *identifier, int pos, int phaseModifier, bool skipJsDocLeadingAsterisks) {
  TsNode *namedBindings = NULL;
  if (!identifier || parseOptional(TSK_CommaToken)) {
    if (token() == TSK_AsteriskToken) namedBindings = parseNamespaceImport();
    else namedBindings = parseNamedImportsOrExports(TSK_NamedImports);
  }
  TsNode *n = newNode(TSK_ImportClause);
  n->extra = phaseModifier;
  n->name = identifier;
  n->namedBindings = namedBindings;
  return finishNode(n, pos, -1);
}

static TsNode *parseModuleReference(void) {
  return isExternalModuleReference2() ? parseExternalModuleReference() : parseEntityName(false);
}

static TsNode *parseExternalModuleReference(void) {
  int pos = getNodePos();
  parseExpected(TSK_RequireKeyword);
  parseExpected(TSK_OpenParenToken);
  TsNode *expression = parseModuleSpecifier();
  parseExpected(TSK_CloseParenToken);
  TsNode *n = newNode(TSK_ExternalModuleReference);
  n->expression = expression;
  return finishNode(n, pos, -1);
}

static TsNode *parseModuleSpecifier(void) {
  if (token() == TSK_StringLiteral) return parseLiteralNode();
  return parseExpression();
}

static TsNode *parseNamespaceImport(void) {
  int pos = getNodePos();
  parseExpected(TSK_AsteriskToken);
  parseExpected(TSK_AsKeyword);
  TsNode *name = parseIdentifier();
  TsNode *n = newNode(TSK_NamespaceImport);
  n->name = name;
  return finishNode(n, pos, -1);
}

static bool canParseModuleExportName(void) {
  return tokenIsIdentifierOrKeyword(token()) || token() == TSK_StringLiteral;
}

static TsNode *parseModuleExportNameIdent(void) { return parseIdentifierName(); }

static TsNode *parseModuleExportName(TsParseFn parseName) {
  return token() == TSK_StringLiteral ? parseLiteralNode() : parseName();
}

static TsNode *parseNamedImportsOrExports(int kind) {
  int pos = getNodePos();
  TsNode *node = newNode(kind);
  if (kind == TSK_NamedImports) {
    node->elements = parseBracketedList(PC_ImportOrExportSpecifiers, parseImportSpecifier, TSK_OpenBraceToken, TSK_CloseBraceToken);
  } else {
    node->elements = parseBracketedList(PC_ImportOrExportSpecifiers, parseExportSpecifier, TSK_OpenBraceToken, TSK_CloseBraceToken);
  }
  return finishNode(node, pos, -1);
}

static TsNode *parseExportSpecifier(void);
static TsNode *parseImportSpecifier(void);

static TsNode *parseExportSpecifier(void) {
  bool hasJSDoc = hasPrecedingJSDocComment();
  return withJSDoc(parseImportOrExportSpecifier(TSK_ExportSpecifier), hasJSDoc);
}

static TsNode *parseImportSpecifier(void) { return parseImportOrExportSpecifier(TSK_ImportSpecifier); }

/* checkIdentifierIsKeyword / Start / End are captured by parseNameWithKeywordCheck in the JS */
typedef struct { bool isKeyword; int start, end; } KeywordCheck;
static KeywordCheck kwCheck;

static TsNode *parseNameWithKeywordCheck(void) {
  kwCheck.isKeyword = isKeywordKind(token()) && !isIdentifier2();
  kwCheck.start = TS.tokenStart;
  kwCheck.end = TS.pos;
  return parseIdentifierName();
}

static TsNode *parseImportOrExportSpecifier(int kind) {
  int pos = getNodePos();
  kwCheck.isKeyword = isKeywordKind(token()) && !isIdentifier2();
  kwCheck.start = TS.tokenStart;
  kwCheck.end = TS.pos;
  bool checkIdentifierIsKeyword = kwCheck.isKeyword;
  int checkIdentifierStart = kwCheck.start;
  int checkIdentifierEnd = kwCheck.end;
  bool isTypeOnly = false;
  TsNode *propertyName = NULL;
  bool canParseAsKeyword = true;
  TsNode *name = parseModuleExportName(parseModuleExportNameIdent);
  if (name->kind == TSK_Identifier && name->textLen == 4 && memcmp(name->text, "type", 4) == 0) {
    if (token() == TSK_AsKeyword) {
      TsNode *firstAs = parseIdentifierName();
      if (token() == TSK_AsKeyword) {
        TsNode *secondAs = parseIdentifierName();
        if (canParseModuleExportName()) {
          isTypeOnly = true;
          propertyName = firstAs;
          name = parseModuleExportName(parseNameWithKeywordCheck);
          canParseAsKeyword = false;
        } else {
          propertyName = name;
          name = secondAs;
          canParseAsKeyword = false;
        }
      } else if (canParseModuleExportName()) {
        propertyName = name;
        canParseAsKeyword = false;
        name = parseModuleExportName(parseNameWithKeywordCheck);
      } else {
        isTypeOnly = true;
        name = firstAs;
      }
    } else if (canParseModuleExportName()) {
      isTypeOnly = true;
      name = parseModuleExportName(parseNameWithKeywordCheck);
    }
  }
  if (canParseAsKeyword && token() == TSK_AsKeyword) {
    propertyName = name;
    parseExpected(TSK_AsKeyword);
    name = parseModuleExportName(parseNameWithKeywordCheck);
  }
  /* the captured keyword check comes from parseNameWithKeywordCheck when it ran last */
  checkIdentifierIsKeyword = kwCheck.isKeyword;
  checkIdentifierStart = kwCheck.start;
  checkIdentifierEnd = kwCheck.end;
  if (kind == TSK_ImportSpecifier) {
    if (name->kind != TSK_Identifier) {
      parseErrorAtPosition(name->pos);
      TsNode *missing = createMissingNode(TSK_Identifier, false);
      missing->pos = name->pos;
      missing->end = name->pos;
      name = missing;
    } else if (checkIdentifierIsKeyword) {
      parseErrorAtPosition(checkIdentifierStart);
    }
  }
  TsNode *n = newNode(kind);
  n->extra = isTypeOnly;
  n->propertyName = propertyName;
  n->name = name;
  (void)checkIdentifierEnd;
  return finishNode(n, pos, -1);
}

static TsNode *parseNamespaceExport(int pos) {
  TsNode *n = newNode(TSK_NamespaceExport);
  n->name = parseModuleExportName(parseModuleExportNameIdent);
  return finishNode(n, pos, -1);
}

static TsNode *parseExportDeclaration(int pos, bool hasJSDoc, TsList *modifiers) {
  bool savedAwaitContext = inAwaitContext();
  setAwaitContext(true);
  TsNode *exportClause = NULL;
  TsNode *moduleSpecifier = NULL;
  TsList *attributes = NULL;
  bool isTypeOnly = parseOptional(TSK_TypeKeyword);
  int namespaceExportPos = getNodePos();
  if (parseOptional(TSK_AsteriskToken)) {
    if (parseOptional(TSK_AsKeyword)) exportClause = parseNamespaceExport(namespaceExportPos);
    parseExpected(TSK_FromKeyword);
    moduleSpecifier = parseModuleSpecifier();
  } else {
    exportClause = parseNamedImportsOrExports(TSK_NamedExports);
    if (token() == TSK_FromKeyword || (token() == TSK_StringLiteral && !(TS.tflags & TF_PrecedingLineBreak))) {
      parseExpected(TSK_FromKeyword);
      moduleSpecifier = parseModuleSpecifier();
    }
  }
  int currentToken2 = token();
  if (moduleSpecifier && (currentToken2 == TSK_WithKeyword || currentToken2 == TSK_AssertKeyword) && !(TS.tflags & TF_PrecedingLineBreak)) {
    attributes = parseImportAttributes(currentToken2, false);
  }
  parseSemicolon();
  setAwaitContext(savedAwaitContext);
  TsNode *node = newNode(TSK_ExportDeclaration);
  node->modifiers = modifiers;
  node->extra = isTypeOnly;
  node->exportClause = exportClause;
  node->moduleSpecifier = moduleSpecifier;
  node->attributes = attributes;
  return withJSDoc(finishNode(node, pos, -1), hasJSDoc);
}

static TsNode *parseExportAssignment(int pos, bool hasJSDoc, TsList *modifiers) {
  bool savedAwaitContext = inAwaitContext();
  setAwaitContext(true);
  bool isExportEquals = false;
  if (parseOptional(TSK_EqualsToken)) isExportEquals = true;
  else parseExpected(TSK_DefaultKeyword);
  TsNode *expression = parseAssignmentExpressionOrHigher(true);
  parseSemicolon();
  setAwaitContext(savedAwaitContext);
  TsNode *node = newNode(TSK_ExportAssignment);
  node->modifiers = modifiers;
  node->extra = isExportEquals;
  node->expression = expression;
  return withJSDoc(finishNode(node, pos, -1), hasJSDoc);
}

/* parseSourceFileWorker (typescript.js line 33372) */
static TsList *parseSourceFileStatements(void) {
  return parseList(PC_SourceElements, parseStatementFn);
}

/* ======================= part 14: predicates, property names, contextual modifiers, list machinery ======================= */

static bool tokenIsIdentifierOrKeyword(int t) { return t >= TSK_Identifier; }
static bool tokenIsIdentifierOrKeywordOrGreaterThan(int t) { return t == TSK_GreaterThanToken || tokenIsIdentifierOrKeyword(t); }
static bool isModifierKind(int t) {
  switch (t) {
    case TSK_AbstractKeyword: case TSK_AccessorKeyword: case TSK_AsyncKeyword: case TSK_ConstKeyword:
    case TSK_DeclareKeyword: case TSK_DefaultKeyword: case TSK_ExportKeyword: case TSK_InKeyword:
    case TSK_PublicKeyword: case TSK_PrivateKeyword: case TSK_ProtectedKeyword: case TSK_ReadonlyKeyword:
    case TSK_StaticKeyword: case TSK_OutKeyword: case TSK_OverrideKeyword:
      return true;
  }
  return false;
}
static bool nodeIsMissing(const TsNode *n) {
  if (!n) return true;
  return n->pos == n->end && n->pos >= 0 && n->kind != TSK_EndOfFileToken;
}
static bool nodeIsPresent(const TsNode *n) { return !nodeIsMissing(n); }
static bool isLiteralPropertyName(void) {
  return tokenIsIdentifierOrKeyword(token()) || token() == TSK_StringLiteral || token() == TSK_NumericLiteral || token() == TSK_BigIntLiteral;
}
static bool isImportAttributeName2(void) {
  return tokenIsIdentifierOrKeyword(token()) || token() == TSK_StringLiteral;
}
static bool nextTokenIsIdentifier(void) {
  nextToken();
  return isIdentifier2();
}
static bool nextTokenIsIdentifierOrKeyword(void) {
  nextToken();
  return tokenIsIdentifierOrKeyword(token());
}
static bool nextTokenIsIdentifierOrKeywordOrGreaterThan(void) {
  nextToken();
  return tokenIsIdentifierOrKeywordOrGreaterThan(token());
}
static bool nextTokenIsOpenParenOrLessThan(void) {
  nextToken();
  return token() == TSK_OpenParenToken || token() == TSK_LessThanToken;
}
static bool nextTokenIsDot(void) { return nextToken() == TSK_DotToken; }
static bool nextTokenIsOpenParenOrLessThanOrDot(void) {
  switch (nextToken()) {
    case TSK_OpenParenToken: case TSK_LessThanToken: case TSK_DotToken:
      return true;
  }
  return false;
}
static bool nextTokenIsStartOfExpression(void) {
  nextToken();
  return isStartOfExpression();
}
static bool nextTokenIsStartOfType(void) {
  nextToken();
  return isStartOfType(false);
}
static bool isValidHeritageClauseObjectLiteral(void) {
  if (nextToken() == TSK_CloseBraceToken) {
    int next = nextToken();
    return next == TSK_CommaToken || next == TSK_OpenBraceToken || next == TSK_ExtendsKeyword || next == TSK_ImplementsKeyword;
  }
  return true;
}
static bool isHeritageClauseExtendsOrImplementsKeyword(void) {
  if (token() == TSK_ImplementsKeyword || token() == TSK_ExtendsKeyword) return lookAhead(cb_nextTokenIsStartOfExpression) != 0;
  return false;
}

/* property names */
static TsNode *parseComputedPropertyName(void) {
  int pos = getNodePos();
  parseExpected(TSK_OpenBracketToken);
  TsNode *expression = (TsNode *)allowInAnd(cb_parseExpression);
  parseExpected(TSK_CloseBracketToken);
  TsNode *n = newNode(TSK_ComputedPropertyName);
  n->expression = expression;
  return finishNode(n, pos, -1);
}

static TsNode *parsePrivateIdentifier(void) {
  int pos = getNodePos();
  TsNode *node = newNode(TSK_PrivateIdentifier);
  node->text = TS.tval;
  node->textLen = TS.tvalLen;
  nextToken();
  return finishNode(node, pos, -1);
}

static TsNode *parsePropertyNameWorker(bool allowComputedPropertyNames) {
  if (token() == TSK_StringLiteral || token() == TSK_NumericLiteral || token() == TSK_BigIntLiteral) return parseLiteralNode();
  if (allowComputedPropertyNames && token() == TSK_OpenBracketToken) return parseComputedPropertyName();
  if (token() == TSK_PrivateIdentifier) return parsePrivateIdentifier();
  return parseIdentifierName();
}
static TsNode *parsePropertyName(void) { return parsePropertyNameWorker(true); }

/* contextual modifiers */
static bool parseContextualModifier(int t) {
  return token() == t && tryParse(cb_nextTokenCanFollowModifier) != 0;
}
static bool canFollowModifier(void) {
  return token() == TSK_OpenBracketToken || token() == TSK_OpenBraceToken || token() == TSK_AsteriskToken ||
         token() == TSK_DotDotDotToken || isLiteralPropertyName();
}
static bool canFollowGetOrSetKeyword(void) { return token() == TSK_OpenBracketToken || isLiteralPropertyName(); }
static bool canFollowExportModifier(void) {
  return token() == TSK_AtToken || (token() != TSK_AsteriskToken && token() != TSK_AsKeyword && token() != TSK_OpenBraceToken && canFollowModifier());
}
static bool nextTokenCanFollowExportModifier(void) {
  nextToken();
  return canFollowExportModifier();
}
static bool nextTokenCanFollowDefaultKeyword(void) {
  nextToken();
  return token() == TSK_ClassKeyword || token() == TSK_FunctionKeyword || token() == TSK_InterfaceKeyword ||
         token() == TSK_AtToken || (token() == TSK_AbstractKeyword && lookAhead(cb_nextTokenIsClassKeywordOnSameLine) != 0) ||
         (token() == TSK_AsyncKeyword && lookAhead(cb_nextTokenIsFunctionKeywordOnSameLine) != 0);
}
static bool nextTokenIsOnSameLineAndCanFollowModifier(void) {
  nextToken();
  if (TS.tflags & TF_PrecedingLineBreak) return false;
  return canFollowModifier();
}
static bool nextTokenCanFollowModifier(void) {
  switch (token()) {
    case TSK_ConstKeyword:
      return nextToken() == TSK_EnumKeyword;
    case TSK_ExportKeyword:
      nextToken();
      if (token() == TSK_DefaultKeyword) return lookAhead(cb_nextTokenCanFollowDefaultKeyword) != 0;
      if (token() == TSK_TypeKeyword) return lookAhead(cb_nextTokenCanFollowExportModifier) != 0;
      return canFollowExportModifier();
    case TSK_DefaultKeyword:
      return nextTokenCanFollowDefaultKeyword();
    case TSK_StaticKeyword:
      nextToken();
      return canFollowModifier();
    case TSK_GetKeyword:
    case TSK_SetKeyword:
      nextToken();
      return canFollowGetOrSetKeyword();
    default:
      return nextTokenIsOnSameLineAndCanFollowModifier();
  }
}
static intptr_t cb_nextTokenCanFollowModifier(void) { return nextTokenCanFollowModifier(); }
static intptr_t cb_nextTokenCanFollowExportModifier(void) { return nextTokenCanFollowExportModifier(); }
static intptr_t cb_nextTokenCanFollowDefaultKeyword(void) { return nextTokenCanFollowDefaultKeyword(); }
static bool parseAnyContextualModifier(void) {
  return isModifierKind(token()) && tryParse(cb_nextTokenCanFollowModifier) != 0;
}

/* list machinery: no syntax cursor, so currentNode is always absent */
static bool isStartOfParameter(bool isJSDocParameter) {
  return token() == TSK_DotDotDotToken || isBindingIdentifierOrPrivateIdentifierOrPattern() || isModifierKind(token()) ||
         token() == TSK_AtToken || isStartOfType(!isJSDocParameter);
}
static bool isClassMemberStart(void);
static bool isTypeMemberStart(void);
static bool isStartOfMappedType(void) {
  nextToken();
  if (token() == TSK_PlusToken || token() == TSK_MinusToken) return nextToken() == TSK_ReadonlyKeyword;
  if (token() == TSK_ReadonlyKeyword) nextToken();
  return token() == TSK_OpenBracketToken && nextToken() == TSK_Identifier && nextToken() == TSK_InKeyword;
}

static bool isListElement2(int parsingContext2, bool inErrorRecovery) {
  switch (parsingContext2) {
    case PC_SourceElements: case PC_BlockStatements: case PC_SwitchClauseStatements:
      return !(token() == TSK_SemicolonToken && inErrorRecovery) && isStartOfStatement();
    case PC_SwitchClauses:
      return token() == TSK_CaseKeyword || token() == TSK_DefaultKeyword;
    case PC_TypeMembers:
      return lookAhead(cb_isTypeMemberStart) != 0;
    case PC_ClassMembers:
      return lookAhead(cb_isClassMemberStart) != 0 || (token() == TSK_SemicolonToken && !inErrorRecovery);
    case PC_EnumMembers:
      return token() == TSK_OpenBracketToken || isLiteralPropertyName();
    case PC_ObjectLiteralMembers:
      switch (token()) {
        case TSK_OpenBracketToken: case TSK_AsteriskToken: case TSK_DotDotDotToken: case TSK_DotToken:
          return true;
        default:
          return isLiteralPropertyName();
      }
    case PC_RestProperties:
      return isLiteralPropertyName();
    case PC_ObjectBindingElements:
      return token() == TSK_OpenBracketToken || token() == TSK_DotDotDotToken || isLiteralPropertyName();
    case PC_ImportAttributes:
      return isImportAttributeName2();
    case PC_HeritageClauseElement:
      if (token() == TSK_OpenBraceToken) return lookAhead(cb_isValidHeritageClauseObjectLiteral) != 0;
      if (!inErrorRecovery) return isStartOfLeftHandSideExpression() && !isHeritageClauseExtendsOrImplementsKeyword();
      return isIdentifier2() && !isHeritageClauseExtendsOrImplementsKeyword();
    case PC_VariableDeclarations:
      return isBindingIdentifierOrPrivateIdentifierOrPattern();
    case PC_ArrayBindingElements:
      return token() == TSK_CommaToken || token() == TSK_DotDotDotToken || isBindingIdentifierOrPrivateIdentifierOrPattern();
    case PC_ArrayLiteralMembers:
      switch (token()) {
        case TSK_CommaToken: case TSK_DotToken:
          return true;
      }
      /* falls through */
    case PC_ArgumentExpressions:
      return token() == TSK_DotDotDotToken || isStartOfExpression();
    case PC_JsxChildren:
      return true;
    case PC_Parameters:
      return isStartOfParameter(false);
    case PC_JSDocParameters:
      return isStartOfParameter(true);
    case PC_TypeParameters:
      return token() == TSK_InKeyword || token() == TSK_ConstKeyword || isIdentifier2();
    case PC_TypeArguments: case PC_TupleElementTypes:
      return token() == TSK_CommaToken || isStartOfType(false);
    case PC_HeritageClauses:
      return isHeritageClause2();
    case PC_ImportOrExportSpecifiers:
      if (token() == TSK_FromKeyword && lookAhead(cb_nextTokenIsStringLiteral)) return false;
      if (token() == TSK_StringLiteral) return true;
      return tokenIsIdentifierOrKeyword(token());
    case PC_JsxAttributes:
      return tokenIsIdentifierOrKeyword(token()) || token() == TSK_OpenBraceToken;
    case PC_JSDocComment:
      return true;
  }
  return false;
}
static intptr_t cb_isTypeMemberStart(void) { return isTypeMemberStart(); }
static intptr_t cb_isClassMemberStart(void) { return isClassMemberStart(); }
static intptr_t cb_isValidHeritageClauseObjectLiteral(void) { return isValidHeritageClauseObjectLiteral(); }

static bool isVariableDeclaratorListTerminator(void) {
  if (canParseSemicolon()) return true;
  if (isInOrOfKeyword(token())) return true;
  if (token() == TSK_EqualsGreaterThanToken) return true;
  return false;
}

static bool isListTerminator(int kind) {
  if (token() == TSK_EndOfFileToken) return true;
  switch (kind) {
    case PC_BlockStatements: case PC_SwitchClauses: case PC_TypeMembers: case PC_ClassMembers:
    case PC_EnumMembers: case PC_ObjectLiteralMembers: case PC_ObjectBindingElements:
    case PC_ImportOrExportSpecifiers: case PC_ImportAttributes:
      return token() == TSK_CloseBraceToken;
    case PC_SwitchClauseStatements:
      return token() == TSK_CloseBraceToken || token() == TSK_CaseKeyword || token() == TSK_DefaultKeyword;
    case PC_HeritageClauseElement:
      return token() == TSK_OpenBraceToken || token() == TSK_ExtendsKeyword || token() == TSK_ImplementsKeyword;
    case PC_VariableDeclarations:
      return isVariableDeclaratorListTerminator();
    case PC_TypeParameters:
      return token() == TSK_GreaterThanToken || token() == TSK_OpenParenToken || token() == TSK_OpenBraceToken ||
             token() == TSK_ExtendsKeyword || token() == TSK_ImplementsKeyword;
    case PC_ArgumentExpressions:
      return token() == TSK_CloseParenToken || token() == TSK_SemicolonToken;
    case PC_ArrayLiteralMembers: case PC_TupleElementTypes: case PC_ArrayBindingElements:
      return token() == TSK_CloseBracketToken;
    case PC_JSDocParameters: case PC_Parameters: case PC_RestProperties:
      return token() == TSK_CloseParenToken || token() == TSK_CloseBracketToken;
    case PC_TypeArguments:
      return token() != TSK_CommaToken;
    case PC_HeritageClauses:
      return token() == TSK_OpenBraceToken || token() == TSK_CloseBraceToken;
    case PC_JsxAttributes:
      return token() == TSK_GreaterThanToken || token() == TSK_SlashToken;
    case PC_JsxChildren:
      return token() == TSK_LessThanToken && lookAhead(cb_nextTokenIsSlash) != 0;
    default:
      return false;
  }
}


/* ======================= part 15: type references, JSDoc type helpers, type members, mapped and tuple types ======================= */

static void parseErrorAtRange(TsNode *range, int message) { (void)message; parseErrorAtPosition(range->pos); }

/* withJSDoc(node, hasJSDoc): ParseNone never sets hasPrecedingJSDocComment, so only ParseAll reaches addJSDocComment */
static TsNode *addJSDocComment(TsNode *node);
static TsNode *withJSDoc(TsNode *node, bool hasJSDoc) {
  if (!hasJSDoc) return node;
  return addJSDocComment(node);
}

static bool isKeyword(int k) { return k >= TSK_FirstKeyword && k <= TSK_LastKeyword; }
static bool isJSDocNullableType(const TsNode *n) { return n->kind == TSK_JSDocNullableType; }
static bool isSetAccessorDeclaration(const TsNode *n) { return n->kind == TSK_SetAccessor; }
static void setTextRange(TsNode *n, const TsNode *range) { n->pos = range->pos; n->end = range->end; }

/* abortParsingListOrMoveToNextToken(kind): the error is counted; then recovery decides */
static bool isInSomeParsingContext(void) {
  for (int kind = 0; kind < PC_Count; kind++) {
    if (P.parsingContext & (1 << kind)) {
      if (isListElement2(kind, true) || isListTerminator(kind)) return true;
    }
  }
  return false;
}

static bool abortParsingListOrMoveToNextToken(int kind) {
  (void)kind;
  parseErrorAtCurrentToken();
  if (isInSomeParsingContext()) return true;
  nextToken();
  return false;
}

static TsNode *parseEntityNameOfTypeReference(void) { return parseEntityName(true); }

static TsList *parseTypeArgumentsOfTypeReference(void) {
  if (!(TS.tflags & TF_PrecedingLineBreak) && reScanLessThanToken() == TSK_LessThanToken) {
    return parseBracketedList(PC_TypeArguments, parseType, TSK_LessThanToken, TSK_GreaterThanToken);
  }
  return NULL;
}

static TsNode *parseTypeReference(void) {
  int pos = getNodePos();
  TsNode *typeName = parseEntityNameOfTypeReference();
  TsList *typeArguments = parseTypeArgumentsOfTypeReference();
  TsNode *n = newNode(TSK_TypeReference);
  n->typeName = typeName;
  n->typeArguments = typeArguments;
  return finishNode(n, pos, -1);
}

static bool typeHasArrowFunctionBlockingParseError(const TsNode *node) {
  switch (node->kind) {
    case TSK_TypeReference:
      return nodeIsMissing(node->typeName);
    case TSK_FunctionType:
    case TSK_ConstructorType:
      return (node->parameters == NULL || node->parameters->isMissingList) || typeHasArrowFunctionBlockingParseError(node->type);
    case TSK_ParenthesizedType:
      return typeHasArrowFunctionBlockingParseError(node->type);
    default:
      return false;
  }
}

static TsNode *parseThisTypePredicate(TsNode *lhs) {
  nextToken();
  TsNode *n = newNode(TSK_TypePredicate);
  n->parameterName = lhs;
  n->type = parseType();
  return finishNode(n, lhs->pos, -1);
}

static TsNode *parseThisTypeNode(void) {
  int pos = getNodePos();
  nextToken();
  return finishNode(newNode(TSK_ThisType), pos, -1);
}

static TsNode *parseJSDocAllType(void) {
  int pos = getNodePos();
  nextToken();
  return finishNode(newNode(TSK_JSDocAllType), pos, -1);
}

static TsNode *parseJSDocNonNullableType(void) {
  int pos = getNodePos();
  nextToken();
  TsNode *n = newNode(TSK_JSDocNonNullableType);
  n->type = parseNonArrayType();
  return finishNode(n, pos, -1);
}

static TsNode *parseJSDocUnknownOrNullableType(void) {
  int pos = getNodePos();
  nextToken();
  if (token() == TSK_CommaToken || token() == TSK_CloseBraceToken || token() == TSK_CloseParenToken ||
      token() == TSK_GreaterThanToken || token() == TSK_EqualsToken || token() == TSK_BarToken) {
    return finishNode(newNode(TSK_JSDocUnknownType), pos, -1);
  }
  TsNode *n = newNode(TSK_JSDocNullableType);
  n->type = parseType();
  return finishNode(n, pos, -1);
}

static TsNode *parseJSDocFunctionType(void) {
  int pos = getNodePos();
  bool hasJSDoc = hasPrecedingJSDocComment();
  if (tryParse(cb_nextTokenIsOpenParen)) {
    TsList *parameters = parseParameters(4 /* Type */ | 32 /* JSDoc */);
    TsNode *type = parseReturnType(TSK_ColonToken, false);
    TsNode *n = newNode(TSK_JSDocFunctionType);
    n->parameters = parameters;
    n->type = type;
    return withJSDoc(finishNode(n, pos, -1), hasJSDoc);
  }
  TsNode *n = newNode(TSK_TypeReference);
  n->typeName = parseIdentifierName();
  return finishNode(n, pos, -1);
}

static TsNode *parseJSDocType(void);

static TsNode *parseJSDocParameter(void) {
  int pos = getNodePos();
  TsNode *name = NULL;
  if (token() == TSK_ThisKeyword || token() == TSK_NewKeyword) {
    name = parseIdentifierName();
    parseExpected(TSK_ColonToken);
  }
  TsNode *n = newNode(TSK_Parameter);
  n->name = name;
  n->type = parseJSDocType();
  return finishNode(n, pos, -1);
}

static TsNode *parseJSDocType(void) {
  TS.skipJsDocLeadingAsterisks = true;
  int pos = getNodePos();
  if (parseOptional(TSK_ModuleKeyword)) {
    TsNode *moduleTag = newNode(TSK_JSDocNamepathType);
    for (;;) {
      switch (token()) {
        case TSK_CloseBraceToken:
        case TSK_EndOfFileToken:
        case TSK_CommaToken:
        case TSK_WhitespaceTrivia:
          goto terminate;
        default:
          nextTokenJSDoc();
      }
    }
  terminate:
    TS.skipJsDocLeadingAsterisks = false;
    return finishNode(moduleTag, pos, -1);
  }
  bool hasDotDotDot = parseOptional(TSK_DotDotDotToken);
  TsNode *type = parseTypeOrTypePredicate();
  TS.skipJsDocLeadingAsterisks = false;
  if (hasDotDotDot) {
    TsNode *v = newNode(TSK_JSDocVariadicType);
    v->type = type;
    type = finishNode(v, pos, -1);
  }
  if (token() == TSK_EqualsToken) {
    nextToken();
    TsNode *o = newNode(TSK_JSDocOptionalType);
    o->type = type;
    return finishNode(o, pos, -1);
  }
  return type;
}

static TsNode *parseTypeQuery(void) {
  int pos = getNodePos();
  parseExpected(TSK_TypeOfKeyword);
  TsNode *entityName = parseEntityName(true);
  TsList *typeArguments = !(TS.tflags & TF_PrecedingLineBreak) ? tryParseTypeArguments() : NULL;
  TsNode *n = newNode(TSK_TypeQuery);
  n->exprName = entityName;
  n->typeArguments = typeArguments;
  return finishNode(n, pos, -1);
}

static TsNode *parseTypeParameter(void) {
  int pos = getNodePos();
  TsList *modifiers = parseModifiers(false, true, false);
  TsNode *name = parseIdentifier();
  TsNode *constraint = NULL, *expression = NULL;
  if (parseOptional(TSK_ExtendsKeyword)) {
    if (isStartOfType(false) || !isStartOfExpression()) {
      constraint = parseType();
    } else {
      expression = parseUnaryExpressionOrHigher();
    }
  }
  TsNode *defaultType = parseOptional(TSK_EqualsToken) ? parseType() : NULL;
  TsNode *n = newNode(TSK_TypeParameter);
  n->modifiers = modifiers;
  n->name = name;
  n->constraint = constraint;
  n->defaultType = defaultType;
  n->expression = expression;
  return finishNode(n, pos, -1);
}

static TsList *parseTypeParameters(void) {
  if (token() == TSK_LessThanToken) {
    return parseBracketedList(PC_TypeParameters, parseTypeParameter, TSK_LessThanToken, TSK_GreaterThanToken);
  }
  return NULL;
}

static bool isStartOfTypeOfImportType(void) {
  nextToken();
  return token() == TSK_ImportKeyword;
}

static TsNode *parseKeywordAndNoDot(void) {
  TsNode *n = parseTokenNode();
  return token() == TSK_DotToken ? NULL : n;
}

static TsNode *parseLiteralTypeNode(bool negative) {
  int pos = getNodePos();
  if (negative) nextToken();
  TsNode *expression = (token() == TSK_TrueKeyword || token() == TSK_FalseKeyword || token() == TSK_NullKeyword)
                         ? parseTokenNode() : parseLiteralLikeNode(token());
  if (negative) {
    TsNode *p = newNode(TSK_PrefixUnaryExpression);
    p->operand = expression;
    p->extra = TSK_MinusToken;
    expression = finishNode(p, pos, -1);
  }
  TsNode *n = newNode(TSK_LiteralType);
  n->literal = expression;
  return finishNode(n, pos, -1);
}


/* ---- type members ---- */


static TsNode *parseIndexSignatureDeclaration(int pos, bool hasJSDoc, TsList *modifiers) {
  TsList *parameters = parseBracketedList(PC_Parameters, (TsParseFn)parseParameterCb, TSK_OpenBracketToken, TSK_CloseBracketToken);
  TsNode *type = parseTypeAnnotation();
  parseTypeMemberSemicolon();
  TsNode *n = newNode(TSK_IndexSignature);
  n->modifiers = modifiers;
  n->parameters = parameters;
  n->type = type;
  return withJSDoc(finishNode(n, pos, -1), hasJSDoc);
}

static TsNode *parsePropertyOrMethodSignature(int pos, bool hasJSDoc, TsList *modifiers) {
  TsNode *name = parsePropertyName();
  TsNode *questionToken = parseOptionalToken(TSK_QuestionToken);
  TsNode *node;
  if (token() == TSK_OpenParenToken || token() == TSK_LessThanToken) {
    TsList *typeParameters = parseTypeParameters();
    TsList *parameters = parseParameters(4 /* Type */);
    TsNode *type = parseReturnType(TSK_ColonToken, true);
    node = newNode(TSK_MethodSignature);
    node->modifiers = modifiers;
    node->name = name;
    node->questionToken = questionToken;
    node->typeParameters = typeParameters;
    node->parameters = parameters;
    node->type = type;
  } else {
    TsNode *type = parseTypeAnnotation();
    node = newNode(TSK_PropertySignature);
    node->modifiers = modifiers;
    node->name = name;
    node->questionToken = questionToken;
    node->type = type;
    if (token() == TSK_EqualsToken) node->initializer = parseInitializer();
  }
  parseTypeMemberSemicolon();
  return withJSDoc(finishNode(node, pos, -1), hasJSDoc);
}

static bool isTypeMemberStart(void) {
  if (token() == TSK_OpenParenToken || token() == TSK_LessThanToken || token() == TSK_GetKeyword || token() == TSK_SetKeyword) {
    return true;
  }
  bool idToken = false;
  while (isModifierKind(token())) {
    idToken = true;
    nextToken();
  }
  if (token() == TSK_OpenBracketToken) return true;
  if (isLiteralPropertyName()) {
    idToken = true;
    nextToken();
  }
  if (idToken) {
    return token() == TSK_OpenParenToken || token() == TSK_LessThanToken || token() == TSK_QuestionToken ||
           token() == TSK_ColonToken || token() == TSK_CommaToken || canParseSemicolon();
  }
  return false;
}

static TsNode *parseTypeMember(void) {
  if (token() == TSK_OpenParenToken || token() == TSK_LessThanToken) {
    return parseSignatureMember(TSK_CallSignature);
  }
  if (token() == TSK_NewKeyword && lookAhead(cb_nextTokenIsOpenParenOrLessThan)) {
    return parseSignatureMember(TSK_ConstructSignature);
  }
  int pos = getNodePos();
  bool hasJSDoc = hasPrecedingJSDocComment();
  TsList *modifiers = parseModifiers(false, false, false);
  if (parseContextualModifier(TSK_GetKeyword)) {
    return parseAccessorDeclaration(pos, hasJSDoc, modifiers, TSK_GetAccessor, 4 /* Type */);
  }
  if (parseContextualModifier(TSK_SetKeyword)) {
    return parseAccessorDeclaration(pos, hasJSDoc, modifiers, TSK_SetAccessor, 4 /* Type */);
  }
  if (isIndexSignature()) {
    return parseIndexSignatureDeclaration(pos, hasJSDoc, modifiers);
  }
  return parsePropertyOrMethodSignature(pos, hasJSDoc, modifiers);
}

static TsList *parseObjectTypeMembers(void) {
  TsList *members;
  if (parseExpected(TSK_OpenBraceToken)) {
    members = parseList(PC_TypeMembers, parseTypeMember);
    parseExpected(TSK_CloseBraceToken);
  } else {
    members = createMissingList();
  }
  return members;
}

static TsNode *parseTypeLiteral(void) {
  int pos = getNodePos();
  TsNode *n = newNode(TSK_TypeLiteral);
  n->members = parseObjectTypeMembers();
  return finishNode(n, pos, -1);
}

/* ---- mapped types ---- */

static TsNode *parseMappedTypeParameter(void) {
  int pos = getNodePos();
  TsNode *name = parseIdentifierName();
  parseExpected(TSK_InKeyword);
  TsNode *type = parseType();
  TsNode *n = newNode(TSK_TypeParameter);
  n->name = name;
  n->constraint = type;
  return finishNode(n, pos, -1);
}

static TsNode *parseMappedType(void) {
  int pos = getNodePos();
  parseExpected(TSK_OpenBraceToken);
  TsNode *readonlyToken = NULL;
  if (token() == TSK_ReadonlyKeyword || token() == TSK_PlusToken || token() == TSK_MinusToken) {
    readonlyToken = parseTokenNode();
    if (readonlyToken->kind != TSK_ReadonlyKeyword) {
      parseExpected(TSK_ReadonlyKeyword);
    }
  }
  parseExpected(TSK_OpenBracketToken);
  TsNode *typeParameter = parseMappedTypeParameter();
  TsNode *nameType = parseOptional(TSK_AsKeyword) ? parseType() : NULL;
  parseExpected(TSK_CloseBracketToken);
  TsNode *questionToken = NULL;
  if (token() == TSK_QuestionToken || token() == TSK_PlusToken || token() == TSK_MinusToken) {
    questionToken = parseTokenNode();
    if (questionToken->kind != TSK_QuestionToken) {
      parseExpected(TSK_QuestionToken);
    }
  }
  TsNode *type = parseTypeAnnotation();
  parseSemicolon();
  TsList *members = parseList(PC_TypeMembers, parseTypeMember);
  parseExpected(TSK_CloseBraceToken);
  TsNode *n = newNode(TSK_MappedType);
  n->readonlyToken = readonlyToken;
  n->typeParameter = typeParameter;
  n->nameType = nameType;
  n->questionToken = questionToken;
  n->type = type;
  n->members = members;
  return finishNode(n, pos, -1);
}

/* ---- tuple, parenthesized, function and constructor types ---- */

static TsNode *parseTupleElementType(void) {
  int pos = getNodePos();
  if (parseOptional(TSK_DotDotDotToken)) {
    TsNode *r = newNode(TSK_RestType);
    r->type = parseType();
    return finishNode(r, pos, -1);
  }
  TsNode *type = parseType();
  if (isJSDocNullableType(type) && type->pos == type->type->pos) {
    TsNode *node = newNode(TSK_OptionalType);
    node->type = type->type;
    setTextRange(node, type);
    node->flags = type->flags;
    return node;
  }
  return type;
}

static bool isNextTokenColonOrQuestionColon(void) {
  return nextToken() == TSK_ColonToken || (token() == TSK_QuestionToken && nextToken() == TSK_ColonToken);
}

static bool isTupleElementName(void) {
  if (token() == TSK_DotDotDotToken) {
    return tokenIsIdentifierOrKeyword(nextToken()) && isNextTokenColonOrQuestionColon();
  }
  return tokenIsIdentifierOrKeyword(token()) && isNextTokenColonOrQuestionColon();
}

static TsNode *parseTupleElementNameOrTupleElementType(void) {
  if (lookAhead(cb_isTupleElementName)) {
    int pos = getNodePos();
    bool hasJSDoc = hasPrecedingJSDocComment();
    TsNode *dotDotDotToken = parseOptionalToken(TSK_DotDotDotToken);
    TsNode *name = parseIdentifierName();
    TsNode *questionToken = parseOptionalToken(TSK_QuestionToken);
    parseExpected(TSK_ColonToken);
    TsNode *type = parseTupleElementType();
    TsNode *node = newNode(TSK_NamedTupleMember);
    node->dotDotDotToken = dotDotDotToken;
    node->name = name;
    node->questionToken = questionToken;
    node->type = type;
    return withJSDoc(finishNode(node, pos, -1), hasJSDoc);
  }
  return parseTupleElementType();
}

static TsNode *parseTupleType(void) {
  int pos = getNodePos();
  TsNode *n = newNode(TSK_TupleType);
  n->elements = parseBracketedList(PC_TupleElementTypes, parseTupleElementNameOrTupleElementType, TSK_OpenBracketToken, TSK_CloseBracketToken);
  return finishNode(n, pos, -1);
}

static TsNode *parseParenthesizedType(void) {
  int pos = getNodePos();
  parseExpected(TSK_OpenParenToken);
  TsNode *type = parseType();
  parseExpected(TSK_CloseParenToken);
  TsNode *n = newNode(TSK_ParenthesizedType);
  n->type = type;
  return finishNode(n, pos, -1);
}

static TsList *parseModifiersForConstructorType(void) {
  TsList *modifiers = NULL;
  if (token() == TSK_AbstractKeyword) {
    int pos = getNodePos();
    nextToken();
    TsNode *modifier = finishNode(newNode(TSK_AbstractKeyword), pos, -1);
    modifiers = ts_list_single(modifier, pos, TS.fullStart);
  }
  return modifiers;
}

static TsNode *parseFunctionOrConstructorType(void) {
  int pos = getNodePos();
  bool hasJSDoc = hasPrecedingJSDocComment();
  TsList *modifiers = parseModifiersForConstructorType();
  bool isConstructorType = parseOptional(TSK_NewKeyword);
  TsList *typeParameters = parseTypeParameters();
  TsList *parameters = parseParameters(4 /* Type */);
  TsNode *type = parseReturnType(TSK_EqualsGreaterThanToken, false);
  TsNode *node = newNode(isConstructorType ? TSK_ConstructorType : TSK_FunctionType);
  node->modifiers = isConstructorType ? modifiers : NULL;
  node->typeParameters = typeParameters;
  node->parameters = parameters;
  node->type = type;
  return withJSDoc(finishNode(node, pos, -1), hasJSDoc);
}

static TsNode *parseParameterCb(void) { return parseParameter(false); }

/* ---- class members: constructors, methods, properties, accessors, static blocks ---- */

static bool parseConstructorName(void);

static intptr_t parseConstructorStringLiteral(void) {
  TsNode *literalNode = parseLiteralNode();
  bool isConstructor = literalNode->textLen == 11 && memcmp(literalNode->text, "constructor", 11) == 0;
  return isConstructor ? (intptr_t)literalNode : 0;
}
static intptr_t cb_parseConstructorStringLiteral(void) { return parseConstructorStringLiteral(); }

static bool parseConstructorName(void) {
  if (token() == TSK_ConstructorKeyword) {
    return parseExpected(TSK_ConstructorKeyword);
  }
  if (token() == TSK_StringLiteral && lookAhead(cb_nextToken) == TSK_OpenParenToken) {
    return tryParse(cb_parseConstructorStringLiteral) != 0;
  }
  return false;
}

typedef struct { int pos; bool hasJSDoc; TsList *modifiers; } CtorArgs;
static CtorArgs ctorArgs[64];
static int ctorArgsN;

static TsNode *tryParseConstructorDeclarationImpl(int pos, bool hasJSDoc, TsList *modifiers) {
  if (!parseConstructorName()) return NULL;
  TsList *typeParameters = parseTypeParameters();
  TsList *parameters = parseParameters(0 /* None */);
  TsNode *type = parseReturnType(TSK_ColonToken, false);
  TsNode *body = parseFunctionBlockOrSemicolon(0 /* None */, 0);
  TsNode *node = newNode(TSK_Constructor);
  node->modifiers = modifiers;
  node->parameters = parameters;
  node->body = body;
  node->typeParameters = typeParameters;
  node->type = type;
  return withJSDoc(finishNode(node, pos, -1), hasJSDoc);
}

/* tryParseConstructorDeclaration(pos, hasJSDoc, modifiers): the callback reads its arguments on entry */
static intptr_t tryParseConstructorCb(void) {
  CtorArgs a = ctorArgs[ctorArgsN - 1];
  return (intptr_t)tryParseConstructorDeclarationImpl(a.pos, a.hasJSDoc, a.modifiers);
}
static TsNode *tryParseConstructorDeclaration(int pos, bool hasJSDoc, TsList *modifiers) {
  ctorArgs[ctorArgsN++] = (CtorArgs){pos, hasJSDoc, modifiers};
  TsNode *r = (TsNode *)tryParse(cb_tryParseConstructorCb);
  ctorArgsN--;
  return r;
}

static bool anyModifierIsAsync(TsList *modifiers) {
  if (!modifiers) return false;
  for (int i = 0; i < modifiers->n; i++) if (isAsyncModifierNode(modifiers->v[i])) return true;
  return false;
}

static TsNode *parseMethodDeclaration(int pos, bool hasJSDoc, TsList *modifiers, TsNode *asteriskToken, TsNode *name,
                                      TsNode *questionToken, TsNode *exclamationToken, int diagnosticMessage) {
  int isGenerator = asteriskToken ? 1 /* Yield */ : 0 /* None */;
  int isAsync = anyModifierIsAsync(modifiers) ? 2 /* Await */ : 0 /* None */;
  TsList *typeParameters = parseTypeParameters();
  TsList *parameters = parseParameters(isGenerator | isAsync);
  TsNode *type = parseReturnType(TSK_ColonToken, false);
  TsNode *body = parseFunctionBlockOrSemicolon(isGenerator | isAsync, diagnosticMessage);
  TsNode *node = newNode(TSK_MethodDeclaration);
  node->modifiers = modifiers;
  node->asteriskToken = asteriskToken;
  node->name = name;
  node->questionToken = questionToken;
  node->typeParameters = typeParameters;
  node->parameters = parameters;
  node->type = type;
  node->body = body;
  node->exclamationToken = exclamationToken;
  return withJSDoc(finishNode(node, pos, -1), hasJSDoc);
}

static void parseSemicolonAfterPropertyName(TsNode *name, TsNode *type, TsNode *initializer) {
  (void)name;
  if (token() == TSK_AtToken && !(TS.tflags & TF_PrecedingLineBreak)) {
    parseErrorAtCurrentToken();
    return;
  }
  if (token() == TSK_OpenParenToken) {
    parseErrorAtCurrentToken();
    nextToken();
    return;
  }
  if (type && !canParseSemicolon()) {
    parseErrorAtCurrentToken();
    return;
  }
  if (tryParseSemicolon()) return;
  if (initializer) {
    parseErrorAtCurrentToken();
    return;
  }
  parseErrorAtCurrentToken();
}

static TsNode *parsePropertyDeclaration(int pos, bool hasJSDoc, TsList *modifiers, TsNode *name, TsNode *questionToken) {
  TsNode *exclamationToken = (!questionToken && !(TS.tflags & TF_PrecedingLineBreak)) ? parseOptionalToken(TSK_ExclamationToken) : NULL;
  TsNode *type = parseTypeAnnotation();
  TsNode *initializer = (TsNode *)doOutsideOfContext(CF_Yield | CF_Await | CF_DisallowIn, cb_parseInitializer);
  parseSemicolonAfterPropertyName(name, type, initializer);
  TsNode *node = newNode(TSK_PropertyDeclaration);
  node->modifiers = modifiers;
  node->name = name;
  node->questionToken = questionToken ? questionToken : exclamationToken;
  node->type = type;
  node->initializer = initializer;
  return withJSDoc(finishNode(node, pos, -1), hasJSDoc);
}

static TsNode *parsePropertyOrMethodDeclaration(int pos, bool hasJSDoc, TsList *modifiers) {
  TsNode *asteriskToken = parseOptionalToken(TSK_AsteriskToken);
  TsNode *name = parsePropertyName();
  TsNode *questionToken = parseOptionalToken(TSK_QuestionToken);
  if (asteriskToken || token() == TSK_OpenParenToken || token() == TSK_LessThanToken) {
    return parseMethodDeclaration(pos, hasJSDoc, modifiers, asteriskToken, name, questionToken, NULL, 0);
  }
  return parsePropertyDeclaration(pos, hasJSDoc, modifiers, name, questionToken);
}

static TsNode *parseAccessorDeclaration(int pos, bool hasJSDoc, TsList *modifiers, int kind, int flags) {
  TsNode *name = parsePropertyName();
  TsList *typeParameters = parseTypeParameters();
  TsList *parameters = parseParameters(0 /* None */);
  TsNode *type = parseReturnType(TSK_ColonToken, false);
  TsNode *body = parseFunctionBlockOrSemicolon(flags, 0);
  TsNode *node = newNode(kind);
  node->modifiers = modifiers;
  node->name = name;
  node->parameters = parameters;
  node->body = body;
  node->typeParameters = typeParameters;
  node->type = type;
  return withJSDoc(finishNode(node, pos, -1), hasJSDoc);
}

static bool isClassMemberStart(void) {
  int idToken = -1;
  if (token() == TSK_AtToken) return true;
  while (isModifierKind(token())) {
    idToken = token();
    if (isClassMemberModifier(idToken)) return true;
    nextToken();
  }
  if (token() == TSK_AsteriskToken) return true;
  if (isLiteralPropertyName()) {
    idToken = token();
    nextToken();
  }
  if (token() == TSK_OpenBracketToken) return true;
  if (idToken != -1) {
    if (!isKeyword(idToken) || idToken == TSK_SetKeyword || idToken == TSK_GetKeyword) return true;
    switch (token()) {
      case TSK_OpenParenToken:
      case TSK_LessThanToken:
      case TSK_ExclamationToken:
      case TSK_ColonToken:
      case TSK_EqualsToken:
      case TSK_QuestionToken:
        return true;
      default:
        return canParseSemicolon();
    }
  }
  return false;
}

static TsNode *parseClassStaticBlockBody(void) {
  bool savedYieldContext = inYieldContext();
  bool savedAwaitContext = inAwaitContext();
  setYieldContext(false);
  setAwaitContext(true);
  TsNode *body = parseBlock(false);
  setYieldContext(savedYieldContext);
  setAwaitContext(savedAwaitContext);
  return body;
}

static TsNode *parseClassStaticBlockDeclaration(int pos, bool hasJSDoc, TsList *modifiers) {
  parseExpectedToken(TSK_StaticKeyword);
  TsNode *body = parseClassStaticBlockBody();
  TsNode *n = newNode(TSK_ClassStaticBlockDeclaration);
  n->body = body;
  n = withJSDoc(finishNode(n, pos, -1), hasJSDoc);
  n->modifiers = modifiers;
  return n;
}


/* ======================= part 16: source file ======================= */

/* initializeState + parseSourceFile setup: the parser state is reset for every file */
static void parserInitState(int scriptKind, int jsDocParsingMode, bool isDeclarationFile) {
  P.languageVersion = 99; /* ESNext */
  P.scriptKind = scriptKind;
  P.languageVariant = (scriptKind == 2 || scriptKind == 4) ? 1 : 0; /* JSX / TSX */
  P.contextFlags = isDeclarationFile ? CF_Ambient : 0;
  P.sourceFlags = 0;
  P.currentToken = 0;
  P.nodeCount = 0;
  P.identifierCount = 0;
  P.nerr = 0;
  P.errBeforeNextFinishedNode = false;
  P.parsingContext = 0;
  P.topLevel = true;
  P.jsDocParsingMode = jsDocParsingMode;
  TS.jsDocParsingMode = jsDocParsingMode;
  notParenSetN = 0;
  ctorArgsN = 0;
  ts_stk_n = 0;
  TS.skipJsDocLeadingAsterisks = 0;
}

static bool isDeclarationFileName(const char *name) {
  size_t n = strlen(name);
  return (n >= 5 && strcmp(name + n - 5, ".d.ts") == 0) || (n >= 6 && (strcmp(name + n - 6, ".d.mts") == 0 || strcmp(name + n - 6, ".d.cts") == 0));
}

/* parseSourceFile(fileName, sourceText, ...): returns the SourceFile node; P.nodeCount and
 * P.identifierCount hold the counts that TypeScript stores on the SourceFile. */
static TsNode *parseSourceFile(const char *fileName, const char *text, int len, int scriptKind, int jsDocParsingMode) {
  ts_setText(text, len);
  bool isDeclarationFile = isDeclarationFileName(fileName);
  parserInitState(scriptKind, jsDocParsingMode, isDeclarationFile);
  if (isDeclarationFile) P.contextFlags |= CF_Ambient;
  P.sourceFlags = P.contextFlags;
  nextToken();
  TsList *statements = parseList(PC_SourceElements, parseStatementFn);
  bool endHasJSDoc = hasPrecedingJSDocComment();
  TsNode *endOfFileToken = withJSDoc(parseTokenNode(), endHasJSDoc);
  TsNode *sf = newNode(TSK_SourceFile);
  sf->statements = statements;
  sf->endOfFileToken = endOfFileToken;
  sf->flags = (uint32_t)P.sourceFlags;
  sf->pos = 0;
  sf->end = len;
  sf->isDeclarationFile = isDeclarationFile;
  sf->text = text;
  sf->textLen = len;
  sf->fileName = fileName;
  return sf;
}

#include "ts-jsdoc.h"

#endif
