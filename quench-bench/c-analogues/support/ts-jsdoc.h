/*
 * JSDoc parser, ported from typescript.js (namespace Parser.JSDocParser, lines 38917-39982) and
 * the comment-range helpers it uses (iterateCommentRanges, getJSDocCommentRanges).
 * The JS builds comment text as arrays of strings. Every string of TypeScript's JSDoc parser is a
 * slice of the source text, so a piece here is a source range [s, e). Only lengths and the
 * newline/whitespace tests on those strings affect the tree, so that is what is kept.
 * Node creation follows the JS factory calls one for one, so nodeCount matches.
 */
#ifndef TS_JSDOC_H
#define TS_JSDOC_H

#include "ts-jsdoc-types.h"
#include "ts-jsdoc-protos.h"




static void jp_push(JPieces *l, int s, int e) {
  if (l->n == l->cap) {
    l->cap = l->cap ? l->cap * 2 : 16;
    l->v = realloc(l->v, sizeof(JPiece) * l->cap);
    if (!l->v) abort();
  }
  l->v[l->n].s = s;
  l->v[l->n].e = e;
  l->n++;
}

static void jn_push(JNodes *l, TsNode *n) {
  if (l->n == l->cap) {
    l->cap = l->cap ? l->cap * 2 : 8;
    l->v = realloc(l->v, sizeof(TsNode *) * l->cap);
    if (!l->v) abort();
  }
  l->v[l->n++] = n;
}

static bool jsIsWs(char c) { return c == ' ' || c == '\t' || c == '\v' || c == '\f' || c == '\n' || c == '\r'; }

/* length of the joined pieces after trimEnd() */
static int jp_trimmedLen(const JPieces *l) {
  int total = 0;
  for (int i = 0; i < l->n; i++) total += l->v[i].e - l->v[i].s;
  int t = total;
  for (int i = l->n - 1; i >= 0; i--) {
    for (int k = l->v[i].e - 1; k >= l->v[i].s; k--) {
      if (!jsIsWs(TS.text[k])) return t;
      t--;
    }
  }
  return t;
}

/* removeLeadingNewlines: drops leading pieces that are exactly "\n" or "\r" */
static void jp_removeLeadingNewlines(JPieces *l) {
  while (l->n && l->v[0].e - l->v[0].s == 1 && (TS.text[l->v[0].s] == '\n' || TS.text[l->v[0].s] == '\r')) {
    memmove(l->v, l->v + 1, sizeof(JPiece) * (l->n - 1));
    l->n--;
  }
}

/* removeTrailingWhitespace */
static void jp_removeTrailingWhitespace(JPieces *l) {
  while (l->n) {
    JPiece *last = &l->v[l->n - 1];
    int len = last->e - last->s;
    int t = len;
    for (int k = last->e - 1; k >= last->s && jsIsWs(TS.text[k]); k--) t--;
    if (t == 0) {
      l->n--;
    } else if (t < len) {
      last->e = last->s + t;
      break;
    } else {
      break;
    }
  }
}

/* ---- comment ranges: iterateCommentRanges with reduce = true ---- */

static void cr_push(CRanges *l, int pos, int end) {
  if (l->n == l->cap) {
    l->cap = l->cap ? l->cap * 2 : 8;
    l->v = realloc(l->v, sizeof(CRange) * l->cap);
    if (!l->v) abort();
  }
  l->v[l->n].pos = pos;
  l->v[l->n].end = end;
  l->n++;
}

static bool jsIsLineBreakAt(int c) { return c == '\n' || c == '\r' || c == 0x2028 || c == 0x2029; }

static void iterateCommentRanges(const char *text, int len, int pos, bool trailing, CRanges *out) {
  bool hasPending = false;
  int pendingPos = 0, pendingEnd = 0;
  bool collecting = trailing;
  if (pos == 0) {
    collecting = true;
    if (len >= 2 && text[0] == '#' && text[1] == '!') {
      while (pos < len && text[pos] != '\n' && text[pos] != '\r') pos++;
    }
  }
  while (pos >= 0 && pos < len) {
    char ch = text[pos];
    if (ch == '\r' || ch == '\n') {
      if (ch == '\r' && pos + 1 < len && text[pos + 1] == '\n') pos++;
      pos++;
      if (trailing) break;
      collecting = true;
      continue;
    }
    if (ch == ' ' || ch == '\t' || ch == '\v' || ch == '\f') {
      pos++;
      continue;
    }
    if (ch != '/') break;
    char next = pos + 1 < len ? text[pos + 1] : 0;
    if (next != '/' && next != '*') break;
    int startPos = pos;
    pos += 2;
    if (next == '/') {
      while (pos < len && !jsIsLineBreakAt((unsigned char)text[pos])) pos++;
    } else {
      while (pos < len) {
        if (text[pos] == '*' && pos + 1 < len && text[pos + 1] == '/') {
          pos += 2;
          break;
        }
        pos++;
      }
    }
    if (collecting) {
      if (hasPending) cr_push(out, pendingPos, pendingEnd);
      pendingPos = startPos;
      pendingEnd = pos;
      hasPending = true;
    }
  }
  if (hasPending) cr_push(out, pendingPos, pendingEnd);
}

/* getJSDocCommentRanges(node, text): leading (and for some kinds trailing) comments at node.pos
 * that start with a double asterisk and not a slash after it, and end within the node. */
static void getJSDocCommentRanges(const TsNode *node, CRanges *out) {
  CRanges all = {0};
  int len = TS.len;
  bool withTrailing = node->kind == TSK_Parameter || node->kind == TSK_TypeParameter ||
                      node->kind == TSK_FunctionExpression || node->kind == TSK_ArrowFunction ||
                      node->kind == TSK_ParenthesizedExpression || node->kind == TSK_VariableDeclaration ||
                      node->kind == TSK_ExportSpecifier;
  if (withTrailing) iterateCommentRanges(TS.text, len, node->pos, true, &all);
  iterateCommentRanges(TS.text, len, node->pos, false, &all);
  for (int i = 0; i < all.n; i++) {
    CRange c = all.v[i];
    if (c.end <= node->end && c.pos + 3 < len && TS.text[c.pos + 1] == '*' && TS.text[c.pos + 2] == '*' &&
        TS.text[c.pos + 3] != '/') {
      cr_push(out, c.pos, c.end);
    }
  }
  free(all.v);
}

/* ---- JSDoc parser state (the JS closures of parseJSDocCommentWorker) ---- */

/* scanRange: the scanner works on [start, start + len) until the callback returns */
static void jsScanRangeBegin(JsRangeSave *sv, int start, int len) {
  sv->end = TS.end; sv->pos = TS.pos; sv->fullStart = TS.fullStart; sv->tokenStart = TS.tokenStart;
  sv->token = TS.token; sv->tval = TS.tval; sv->tvalLen = TS.tvalLen; sv->tflags = TS.tflags;
  TS.end = start + len;
  TS.pos = start;
  TS.fullStart = start;
  TS.tokenStart = start;
  TS.token = TSK_Unknown;
  TS.tval = "";
  TS.tvalLen = 0;
  TS.tflags = 0;
}
static void jsScanRangeEnd(const JsRangeSave *sv) {
  TS.end = sv->end; TS.pos = sv->pos; TS.fullStart = sv->fullStart; TS.tokenStart = sv->tokenStart;
  TS.token = sv->token; TS.tval = sv->tval; TS.tvalLen = sv->tvalLen; TS.tflags = sv->tflags;
}

static int nextJSDocCommentTextToken(bool inBackticks) { return P.currentToken = ts_scanJSDocCommentTextToken(inBackticks); }

static bool parseOptionalJsdoc(int t) {
  if (token() == t) {
    nextTokenJSDoc();
    return true;
  }
  return false;
}

static bool parseExpectedJSDoc(int kind) {
  if (token() == kind) {
    nextTokenJSDoc();
    return true;
  }
  parseErrorAtCurrentToken();
  return false;
}

/* parseExpectedTokenJSDoc(kind): a token node either way */
static TsNode *parseExpectedTokenJSDoc(int kind) {
  if (token() == kind) {
    int pos = getNodePos();
    TsNode *n = newNode(kind);
    nextTokenJSDoc();
    return finishNode(n, pos, -1);
  }
  return createMissingNode(kind, false);
}

/* linkType codes of parseJSDocLinkPrefix: 1 link, 2 linkcode, 3 linkplain, 0 none */
static int jsLinkKind(const char *t, int n) {
  if (n == 4 && memcmp(t, "link", 4) == 0) return 1;
  if (n == 8 && memcmp(t, "linkcode", 8) == 0) return 2;
  if (n == 9 && memcmp(t, "linkplain", 9) == 0) return 3;
  return 0;
}

static bool jsNodeTextIs(const TsNode *n, const char *s) {
  int len = (int)strlen(s);
  return n->textLen == len && memcmp(n->text, s, len) == 0;
}

static JStr jsStrOf(int s, int e) { JStr r = {s, e, true}; return r; }
static JStr jsStrUndef(void) { JStr r = {0, 0, false}; return r; }

/* str.slice(margin) for a source slice */
static JStr jsSliceFrom(JStr str, int margin) {
  int len = str.e - str.s;
  int m = margin < 0 ? 0 : (margin > len ? len : margin);
  return jsStrOf(str.s + m, str.e);
}

static bool isNextNonwhitespaceTokenEndOfFile(void) {
  for (;;) {
    nextTokenJSDoc();
    if (token() == TSK_EndOfFileToken) return true;
    if (!(token() == TSK_WhitespaceTrivia || token() == TSK_NewLineTrivia)) return false;
  }
}
static intptr_t cb_isNextNonwhitespaceTokenEndOfFile(void) { return isNextNonwhitespaceTokenEndOfFile(); }

static void skipWhitespace(void) {
  if (token() == TSK_WhitespaceTrivia || token() == TSK_NewLineTrivia) {
    if (lookAhead(cb_isNextNonwhitespaceTokenEndOfFile)) return;
  }
  while (token() == TSK_WhitespaceTrivia || token() == TSK_NewLineTrivia) nextTokenJSDoc();
}

/* skipWhitespaceOrAsterisk(): the indentation after the last line break, or "" */
static JStr skipWhitespaceOrAsterisk(void) {
  if (token() == TSK_WhitespaceTrivia || token() == TSK_NewLineTrivia) {
    if (lookAhead(cb_isNextNonwhitespaceTokenEndOfFile)) return jsStrOf(TS.pos, TS.pos);
  }
  bool precedingLineBreak = (TS.tflags & TF_PrecedingLineBreak) != 0;
  bool seenLineBreak = false;
  int indS = TS.pos, indE = TS.pos;
  while ((precedingLineBreak && token() == TSK_AsteriskToken) || token() == TSK_WhitespaceTrivia || token() == TSK_NewLineTrivia) {
    if (indS == indE) indS = TS.tokenStart;
    indE = TS.pos;
    if (token() == TSK_NewLineTrivia) {
      precedingLineBreak = true;
      seenLineBreak = true;
      indS = TS.pos;
      indE = TS.pos;
    } else if (token() == TSK_AsteriskToken) {
      precedingLineBreak = false;
    }
    nextTokenJSDoc();
  }
  return seenLineBreak ? jsStrOf(indS, indE) : jsStrOf(TS.pos, TS.pos);
}

/* ---- callbacks with arguments: the arguments are kept in a stack and read on entry ---- */
static intptr_t cb_parseJSDocType(void) { return (intptr_t)parseJSDocType(); }

static TsNode *parseChildParameterOrPropertyTag(int target, int indent3, TsNode *name);
static intptr_t cb_jsChildTag(void) {
  JsChildArgs a = jsChildArgs;
  return (intptr_t)parseChildParameterOrPropertyTag(a.target, a.indent, a.name);
}
static TsNode *tryParseChildTagArgs(int target, int indent3, TsNode *name) {
  JsChildArgs saved = jsChildArgs;
  jsChildArgs = (JsChildArgs){target, indent3, name};
  TsNode *r = (TsNode *)tryParse(cb_jsChildTag);
  jsChildArgs = saved;
  return r;
}

static TsNode *parseJSDocType(void);
static TsNode *parseJSDocTypeExpression(bool mayOmitBraces);
static TsNode *parseTag(int margin);
static TsNode *parseJSDocIdentifierName(bool hasMessage);
static TsNode *parseJSDocEntityName(void);
static JComment parseTrailingTagComments(int pos, int end2, int margin, JStr indentText);
static JComment parseTagComments(int indent3, JStr initialMargin);
static TsNode *parseJSDocLink(int start2);
static int parseJSDocLinkPrefix(void);
static intptr_t cb_parseJSDocLinkPrefix(void) { return parseJSDocLinkPrefix(); }
static TsNode *parseTypeTag(int start2, TsNode *tagName, bool hasIndent, int indent3, JStr indentText);
static TsNode *parseParameterOrPropertyTag(int start2, TsNode *tagName, int target, int indent3);
static TsNode *parseTemplateTag(int start2, TsNode *tagName, int indent3, JStr indentText);
static TsNode *parseThisTag(int start2, TsNode *tagName, int margin, JStr indentText);
static TsNode *parseJSDocTypeNameWithNamespace(bool nested);
static bool isObjectOrObjectArrayTypeReference(const TsNode *node);

/* ---- type expressions ---- */

static TsNode *parseJSDocTypeExpression(bool mayOmitBraces) {
  int pos = getNodePos();
  bool hasBrace = mayOmitBraces ? parseOptional(TSK_OpenBraceToken) : parseExpected(TSK_OpenBraceToken);
  TsNode *type = (TsNode *)doInsideOfContext(TNF_JSDoc, cb_parseJSDocType);
  if (!mayOmitBraces || hasBrace) parseExpectedJSDoc(TSK_CloseBraceToken);
  TsNode *result = newNode(TSK_JSDocTypeExpression);
  result->type = type;
  return finishNode(result, pos, -1);
}

static TsNode *parseJSDocNameReference(void) {
  int pos = getNodePos();
  bool hasBrace = parseOptional(TSK_OpenBraceToken);
  int p2 = getNodePos();
  TsNode *entityName = parseEntityName(false);
  while (token() == TSK_PrivateIdentifier) {
    reScanHashToken();
    nextTokenJSDoc();
    TsNode *m = newNode(TSK_JSDocMemberName);
    m->left = entityName;
    m->right = parseIdentifier();
    entityName = finishNode(m, p2, -1);
  }
  if (hasBrace) parseExpectedJSDoc(TSK_CloseBraceToken);
  TsNode *result = newNode(TSK_JSDocNameReference);
  result->name = entityName;
  return finishNode(result, pos, -1);
}

static TsNode *tryParseTypeExpression(void) {
  skipWhitespaceOrAsterisk();
  return token() == TSK_OpenBraceToken ? parseJSDocTypeExpression(false) : NULL;
}

/* parseJSDocLinkPrefix: skips to "{@link", returns the link kind (0 if none) */
static int parseJSDocLinkPrefix(void) {
  skipWhitespaceOrAsterisk();
  if (token() == TSK_OpenBraceToken && nextTokenJSDoc() == TSK_AtToken && tokenIsIdentifierOrKeyword(nextTokenJSDoc())) {
    int k = jsLinkKind(TS.tval, TS.tvalLen);
    if (k) return k;
  }
  return 0;
}

static TsNode *parseJSDocLinkName(void) {
  if (tokenIsIdentifierOrKeyword(token())) {
    int pos = getNodePos();
    TsNode *name = parseIdentifierName();
    while (parseOptional(TSK_DotToken)) {
      TsNode *right = token() == TSK_PrivateIdentifier ? createMissingNode(TSK_Identifier, false) : parseIdentifierName();
      TsNode *q = newNode(TSK_QualifiedName);
      q->left = name;
      q->right = right;
      name = finishNode(q, pos, -1);
    }
    while (token() == TSK_PrivateIdentifier) {
      reScanHashToken();
      nextTokenJSDoc();
      TsNode *m = newNode(TSK_JSDocMemberName);
      m->left = name;
      m->right = parseIdentifier();
      name = finishNode(m, pos, -1);
    }
    return name;
  }
  return NULL;
}

/* parseJSDocLink(start): an inline {@link ...} tag, or NULL */
static TsNode *parseJSDocLink(int start2) {
  int linkType = (int)tryParse(cb_parseJSDocLinkPrefix);
  if (!linkType) return NULL;
  nextTokenJSDoc();
  skipWhitespace();
  TsNode *name = parseJSDocLinkName();
  while (token() != TSK_CloseBraceToken && token() != TSK_NewLineTrivia && token() != TSK_EndOfFileToken) {
    nextTokenJSDoc();
  }
  TsNode *n = newNode(linkType == 1 ? TSK_JSDocLink : linkType == 2 ? TSK_JSDocLinkCode : TSK_JSDocLinkPlain);
  n->name = name;
  return finishNode(n, start2, TS.pos);
}

/* ---- identifiers and entity names ---- */

static TsNode *parseJSDocIdentifierName(bool hasMessage) {
  if (!tokenIsIdentifierOrKeyword(token())) {
    return createMissingNode(TSK_Identifier, !hasMessage);
  }
  P.identifierCount++;
  int start2 = TS.tokenStart;
  int end2 = TS.pos;
  int originalKeywordKind = token();
  const char *text = TS.tval;
  int len = TS.tvalLen;
  TsNode *result = finishNode(newIdentifier(text, len, originalKeywordKind), start2, end2);
  nextTokenJSDoc();
  return result;
}

static TsNode *parseJSDocEntityName(void) {
  TsNode *entity = parseJSDocIdentifierName(false);
  if (parseOptional(TSK_OpenBracketToken)) parseExpected(TSK_CloseBracketToken);
  while (parseOptional(TSK_DotToken)) {
    TsNode *name = parseJSDocIdentifierName(false);
    if (parseOptional(TSK_OpenBracketToken)) parseExpected(TSK_CloseBracketToken);
    TsNode *q = newNode(TSK_QualifiedName);
    q->left = entity;
    q->right = name;
    entity = finishNode(q, entity->pos, -1);
  }
  return entity;
}

static TsNode *parsePropertyAccessEntityNameExpression(void) {
  int pos = getNodePos();
  TsNode *node = parseJSDocIdentifierName(false);
  while (parseOptional(TSK_DotToken)) {
    TsNode *name = parseJSDocIdentifierName(false);
    TsNode *p = newNode(TSK_PropertyAccessExpression);
    p->expression = node;
    p->name = name;
    node = finishNode(p, pos, -1);
  }
  return node;
}

static TsNode *parseJSDocTypeNameWithNamespace(bool nested) {
  int start2 = TS.tokenStart;
  if (!tokenIsIdentifierOrKeyword(token())) return NULL;
  TsNode *typeNameOrNamespaceName = parseJSDocIdentifierName(false);
  if (parseOptional(TSK_DotToken)) {
    TsNode *body = parseJSDocTypeNameWithNamespace(true);
    TsNode *ns = newNode(TSK_ModuleDeclaration);
    ns->name = typeNameOrNamespaceName;
    ns->body = body;
    ns->extra = nested ? 8 : 0;
    return finishNode(ns, start2, -1);
  }
  return typeNameOrNamespaceName;
}

/* ---- tag helpers ---- */

static bool isObjectOrObjectArrayTypeReference(const TsNode *node) {
  switch (node->kind) {
    case TSK_ObjectKeyword:
      return true;
    case TSK_ArrayType:
      return isObjectOrObjectArrayTypeReference(node->elementType);
    default:
      return node->kind == TSK_TypeReference && node->typeName && node->typeName->kind == TSK_Identifier &&
             jsNodeTextIs(node->typeName, "Object") && node->typeArguments == NULL;
  }
}

static bool escapedTextsEqual(TsNode *a, TsNode *b) {
  while (a->kind != TSK_Identifier || b->kind != TSK_Identifier) {
    if (a->kind != TSK_Identifier && b->kind != TSK_Identifier && a->right && b->right &&
        a->right->textLen == b->right->textLen && memcmp(a->right->text, b->right->text, a->right->textLen) == 0) {
      a = a->left;
      b = b->left;
    } else {
      return false;
    }
  }
  return a->textLen == b->textLen && memcmp(a->text, b->text, a->textLen) == 0;
}

/* ---- tag bodies ---- */


static JBracketName parseBracketNameInPropertyAndParamTag(void) {
  bool isBracketed = parseOptionalJsdoc(TSK_OpenBracketToken);
  if (isBracketed) skipWhitespace();
  bool isBackquoted = parseOptionalJsdoc(TSK_BacktickToken);
  TsNode *name = parseJSDocEntityName();
  if (isBackquoted) parseExpectedTokenJSDoc(TSK_BacktickToken);
  if (isBracketed) {
    skipWhitespace();
    if (parseOptionalToken(TSK_EqualsToken)) parseExpression();
    parseExpected(TSK_CloseBracketToken);
  }
  JBracketName r = {name, isBracketed};
  return r;
}

static void addTag(TsNode *tag) {
  if (!tag) return;
  if (!JD->hasTags) {
    JD->hasTags = true;
    JD->tags.n = 0;
    JD->tagsPos = tag->pos;
  }
  jn_push(&JD->tags, tag);
  JD->tagsEnd = tag->end;
}

static bool someTag(int kind) {
  if (!JD) return false;
  for (int i = 0; i < JD->tags.n; i++) if (JD->tags.v[i]->kind == kind) return true;
  return false;
}

/* parseNestedTypeLiteral: child tags of an object-typed @param/@property become a type literal */
static TsNode *parseNestedTypeLiteral(TsNode *typeExpression, TsNode *name, int target, int indent3) {
  (void)name;
  if (typeExpression && isObjectOrObjectArrayTypeReference(typeExpression->type)) {
    int pos = getNodePos();
    JNodes children = {0};
    TsNode *child;
    while ((child = tryParseChildTagArgs(target, indent3, name)) != NULL) {
      if (child->kind == TSK_JSDocParameterTag || child->kind == TSK_JSDocPropertyTag) {
        jn_push(&children, child);
      } else if (child->kind == TSK_JSDocTemplateTag) {
        parseErrorAtPosition(child->tagName ? child->tagName->pos : child->pos);
      }
    }
    if (children.n) {
      TsNode *literal = newNode(TSK_JSDocTypeLiteral);
      literal->extra = typeExpression->type->kind == TSK_ArrayType;
      literal = finishNode(literal, pos, -1);
      TsNode *te = newNode(TSK_JSDocTypeExpression);
      te->type = literal;
      free(children.v);
      return finishNode(te, pos, -1);
    }
    free(children.v);
  }
  return NULL;
}

static TsNode *parseParameterOrPropertyTag(int start2, TsNode *tagName, int target, int indent3) {
  TsNode *typeExpression = tryParseTypeExpression();
  bool isNameFirst = typeExpression == NULL;
  skipWhitespaceOrAsterisk();
  JBracketName bn = parseBracketNameInPropertyAndParamTag();
  JStr indentText = skipWhitespaceOrAsterisk();
  if (isNameFirst && !lookAhead(cb_parseJSDocLinkPrefix)) typeExpression = tryParseTypeExpression();
  JComment comment = parseTrailingTagComments(start2, getNodePos(), indent3, indentText);
  TsNode *nested = parseNestedTypeLiteral(typeExpression, bn.name, target, indent3);
  if (nested) {
    typeExpression = nested;
    isNameFirst = true;
  }
  TsNode *n = newNode(target == 1 ? TSK_JSDocPropertyTag : TSK_JSDocParameterTag);
  n->tagName = tagName;
  n->name = bn.name;
  n->extra = bn.isBracketed;
  n->typeExpression = typeExpression;
  (void)comment;
  return finishNode(n, start2, -1);
}

static TsNode *parseReturnTag(int start2, TsNode *tagName, int indent3, JStr indentText) {
  if (someTag(TSK_JSDocReturnTag)) parseErrorAtPosition(tagName->pos);
  TsNode *typeExpression = tryParseTypeExpression();
  parseTrailingTagComments(start2, getNodePos(), indent3, indentText);
  TsNode *n = newNode(TSK_JSDocReturnTag);
  n->tagName = tagName;
  n->typeExpression = typeExpression;
  return finishNode(n, start2, -1);
}

static TsNode *parseTypeTag(int start2, TsNode *tagName, bool hasIndent, int indent3, JStr indentText) {
  if (someTag(TSK_JSDocTypeTag)) parseErrorAtPosition(tagName->pos);
  TsNode *typeExpression = parseJSDocTypeExpression(true);
  if (hasIndent && indentText.def) parseTrailingTagComments(start2, getNodePos(), indent3, indentText);
  TsNode *n = newNode(TSK_JSDocTypeTag);
  n->tagName = tagName;
  n->typeExpression = typeExpression;
  return finishNode(n, start2, -1);
}

static TsNode *parseSeeTag(int start2, TsNode *tagName, bool hasIndent, int indent3, JStr indentText) {
  bool isMarkdownOrJSDocLink = token() == TSK_OpenBracketToken || lookAhead(cb_jsSeeLinkLookahead) != 0;
  TsNode *nameExpression = isMarkdownOrJSDocLink ? NULL : parseJSDocNameReference();
  if (hasIndent && indentText.def) parseTrailingTagComments(start2, getNodePos(), indent3, indentText);
  TsNode *n = newNode(TSK_JSDocSeeTag);
  n->tagName = tagName;
  n->name = nameExpression;
  return finishNode(n, start2, -1);
}

static TsNode *parseAuthorNameAndEmailNode(void) {
  JPieces comments = {0};
  bool inEmail = false;
  int token2 = token();
  while (token2 != TSK_EndOfFileToken && token2 != TSK_NewLineTrivia) {
    if (token2 == TSK_LessThanToken) {
      inEmail = true;
    } else if (token2 == TSK_AtToken && !inEmail) {
      break;
    } else if (token2 == TSK_GreaterThanToken && inEmail) {
      jp_push(&comments, TS.tokenStart, TS.pos);
      ts_resetTokenState(TS.pos);
      break;
    }
    jp_push(&comments, TS.tokenStart, TS.pos);
    token2 = nextTokenJSDoc();
  }
  free(comments.v);
  return newNode(TSK_JSDocText);
}

static TsNode *parseExpressionWithTypeArgumentsForAugments(void) {
  bool usedBrace = parseOptional(TSK_OpenBraceToken);
  int pos = getNodePos();
  TsNode *expression = parsePropertyAccessEntityNameExpression();
  TS.skipJsDocLeadingAsterisks = 1;
  TsList *typeArguments = tryParseTypeArguments();
  TS.skipJsDocLeadingAsterisks = 0;
  TsNode *node = newNode(TSK_ExpressionWithTypeArguments);
  node->expression = expression;
  node->typeArguments = typeArguments;
  TsNode *res = finishNode(node, pos, -1);
  if (usedBrace) {
    skipWhitespace();
    parseExpected(TSK_CloseBraceToken);
  }
  return res;
}

static TsNode *parseAuthorTag(int start2, TsNode *tagName, int indent3, JStr indentText) {
  int commentStart = getNodePos();
  TsNode *textOnly = parseAuthorNameAndEmailNode();
  int commentEnd = TS.fullStart;
  JComment comments2 = parseTrailingTagComments(start2, commentEnd, indent3, indentText);
  if (comments2.kind == JC_Undefined) commentEnd = TS.fullStart;
  finishNode(textOnly, commentStart, commentEnd);
  TsNode *n = newNode(TSK_JSDocAuthorTag);
  n->tagName = tagName;
  return finishNode(n, start2, -1);
}

static TsNode *parseSimpleTagNode(int start2, int kind, TsNode *tagName, int margin, JStr indentText) {
  parseTrailingTagComments(start2, getNodePos(), margin, indentText);
  TsNode *n = newNode(kind);
  n->tagName = tagName;
  return finishNode(n, start2, -1);
}

static TsNode *parseImplementsTag(int start2, TsNode *tagName, int margin, JStr indentText) {
  TsNode *className = parseExpressionWithTypeArgumentsForAugments();
  parseTrailingTagComments(start2, getNodePos(), margin, indentText);
  TsNode *n = newNode(TSK_JSDocImplementsTag);
  n->tagName = tagName;
  n->expression = className;
  return finishNode(n, start2, -1);
}

static TsNode *parseAugmentsTag(int start2, TsNode *tagName, int margin, JStr indentText) {
  TsNode *className = parseExpressionWithTypeArgumentsForAugments();
  parseTrailingTagComments(start2, getNodePos(), margin, indentText);
  TsNode *n = newNode(TSK_JSDocAugmentsTag);
  n->tagName = tagName;
  n->expression = className;
  return finishNode(n, start2, -1);
}

static TsNode *parseSatisfiesTag(int start2, TsNode *tagName, int margin, JStr indentText, bool hasIndent) {
  TsNode *typeExpression = parseJSDocTypeExpression(false);
  if (hasIndent && indentText.def) parseTrailingTagComments(start2, getNodePos(), margin, indentText);
  TsNode *n = newNode(TSK_JSDocSatisfiesTag);
  n->tagName = tagName;
  n->typeExpression = typeExpression;
  return finishNode(n, start2, -1);
}

static TsNode *parseImportTag(int start2, TsNode *tagName, int margin, JStr indentText, bool hasIndent) {
  int afterImportTagPos = TS.fullStart;
  TsNode *identifier = NULL;
  if (isIdentifier2()) identifier = parseIdentifier();
  TsNode *importClause = tryParseImportClause(identifier, afterImportTagPos, TSK_TypeKeyword, true);
  TsNode *moduleSpecifier = parseModuleSpecifier();
  TsList *attributes = tryParseImportAttributes();
  if (hasIndent && indentText.def) parseTrailingTagComments(start2, getNodePos(), margin, indentText);
  TsNode *n = newNode(TSK_JSDocImportTag);
  n->tagName = tagName;
  n->importClause = importClause;
  n->moduleSpecifier = moduleSpecifier;
  n->attributes = attributes;
  return finishNode(n, start2, -1);
}

static TsNode *parseThisOrEnumTag(int kind, int start2, TsNode *tagName, int margin, JStr indentText) {
  TsNode *typeExpression = parseJSDocTypeExpression(true);
  skipWhitespace();
  parseTrailingTagComments(start2, getNodePos(), margin, indentText);
  TsNode *n = newNode(kind);
  n->tagName = tagName;
  n->typeExpression = typeExpression;
  return finishNode(n, start2, -1);
}

static TsNode *parseThisTag(int start2, TsNode *tagName, int margin, JStr indentText) {
  return parseThisOrEnumTag(TSK_JSDocThisTag, start2, tagName, margin, indentText);
}

static TsNode *parseEnumTag(int start2, TsNode *tagName, int margin, JStr indentText) {
  return parseThisOrEnumTag(TSK_JSDocEnumTag, start2, tagName, margin, indentText);
}

static TsNode *parseThrowsTag(int start2, TsNode *tagName, int margin, JStr indentText) {
  TsNode *typeExpression = tryParseTypeExpression();
  parseTrailingTagComments(start2, getNodePos(), margin, indentText);
  TsNode *n = newNode(TSK_JSDocThrowsTag);
  n->tagName = tagName;
  n->typeExpression = typeExpression;
  return finishNode(n, start2, -1);
}

/* parseChildParameterOrPropertyTag(target, indent, name): the next child tag of a typedef or callback */
static TsNode *parseChildParameterOrPropertyTag(int target, int indent3, TsNode *name) {
  bool canParseTag = true;
  bool seenAsterisk = false;
  for (;;) {
    switch (nextTokenJSDoc()) {
      case TSK_AtToken:
        if (canParseTag) {
          TsNode *child = tryParseChildTag(target, indent3);
          if (child && (child->kind == TSK_JSDocParameterTag || child->kind == TSK_JSDocPropertyTag) && name &&
              (child->name->kind == TSK_Identifier || !escapedTextsEqual(name, child->name->left))) {
            return NULL;
          }
          return child;
        }
        seenAsterisk = false;
        break;
      case TSK_NewLineTrivia:
        canParseTag = true;
        seenAsterisk = false;
        break;
      case TSK_AsteriskToken:
        if (seenAsterisk) canParseTag = false;
        seenAsterisk = true;
        break;
      case TSK_Identifier:
        canParseTag = false;
        break;
      case TSK_EndOfFileToken:
        return NULL;
      default:
        break;
    }
  }
}

/* tryParseChildTag(target, indent): @type, @prop/@property, @param, @template and @this children */
static TsNode *tryParseChildTag(int target, int indent3) {
  int start2 = TS.fullStart;
  nextTokenJSDoc();
  TsNode *tagName = parseJSDocIdentifierName(false);
  JStr indentText = skipWhitespaceOrAsterisk();
  int t;
  if (jsNodeTextIs(tagName, "type")) {
    return target == 1 ? parseTypeTag(start2, tagName, false, 0, jsStrUndef()) : NULL;
  } else if (jsNodeTextIs(tagName, "prop") || jsNodeTextIs(tagName, "property")) {
    t = 1;
  } else if (jsNodeTextIs(tagName, "arg") || jsNodeTextIs(tagName, "argument") || jsNodeTextIs(tagName, "param")) {
    t = 2 | 4;
  } else if (jsNodeTextIs(tagName, "template")) {
    return parseTemplateTag(start2, tagName, indent3, indentText);
  } else if (jsNodeTextIs(tagName, "this")) {
    return parseThisTag(start2, tagName, indent3, indentText);
  } else {
    return NULL;
  }
  if (!(target & t)) return NULL;
  return parseParameterOrPropertyTag(start2, tagName, target, indent3);
}

static TsNode *parseChildPropertyTag(int indent3) { return parseChildParameterOrPropertyTag(1, indent3, NULL); }

/* parseTypedefTag */
static TsNode *parseTypedefTag(int start2, TsNode *tagName, int indent3, JStr indentText) {
  TsNode *typeExpression = tryParseTypeExpression();
  skipWhitespaceOrAsterisk();
  TsNode *fullName = parseJSDocTypeNameWithNamespace(false);
  skipWhitespace();
  JComment comment = parseTagComments(indent3, jsStrUndef());
  jsIndentArg = indent3;
  int end2 = -1;
  bool hasEnd2 = false;
  if (!typeExpression || isObjectOrObjectArrayTypeReference(typeExpression->type)) {
    TsNode *childTypeTag = NULL;
    bool hasChildren = false;
    JNodes props = {0};
    TsNode *child;
    while ((jsIndentArg = indent3, child = (TsNode *)tryParse(cb_jsChildPropertyTagTypedef)) != NULL) {
      if (child->kind == TSK_JSDocTemplateTag) break;
      hasChildren = true;
      if (child->kind == TSK_JSDocTypeTag) {
        if (childTypeTag) {
          parseErrorAtCurrentToken();
          break;
        }
        childTypeTag = child;
      } else {
        jn_push(&props, child);
      }
    }
    free(props.v);
    if (hasChildren) {
      bool isArrayType = typeExpression && typeExpression->type->kind == TSK_ArrayType;
      TsNode *jsdocTypeLiteral = newNode(TSK_JSDocTypeLiteral);
      jsdocTypeLiteral->extra = isArrayType;
      if (childTypeTag && childTypeTag->typeExpression && !isObjectOrObjectArrayTypeReference(childTypeTag->typeExpression->type)) {
        typeExpression = childTypeTag->typeExpression;
      } else {
        typeExpression = finishNode(jsdocTypeLiteral, start2, -1);
      }
      end2 = typeExpression->end;
      hasEnd2 = true;
    }
  }
  if ((hasEnd2 && end2) || comment.kind != JC_Undefined) {
    end2 = getNodePos();
  } else {
    TsNode *e = fullName ? fullName : typeExpression ? typeExpression : tagName;
    end2 = e->end;
  }
  if (comment.kind == JC_Undefined) comment = parseTrailingTagComments(start2, end2, indent3, indentText);
  TsNode *typedefTag = newNode(TSK_JSDocTypedefTag);
  typedefTag->tagName = tagName;
  typedefTag->typeExpression = typeExpression;
  typedefTag->name = fullName;
  return finishNode(typedefTag, start2, end2);
}

static intptr_t cb_jsChildPropertyTagTypedef(void) { return (intptr_t)parseChildPropertyTag(jsIndentArg); }

/* parseCallbackTagParameters(indent) */
static void parseCallbackTagParameters(int indent3, JNodes *params) {
  TsNode *child;
  while ((jsIndentArg = indent3, child = (TsNode *)tryParse(cb_jsChildCallbackParam)) != NULL) {
    if (child->kind == TSK_JSDocTemplateTag) {
      parseErrorAtPosition(child->tagName ? child->tagName->pos : child->pos);
      break;
    }
    jn_push(params, child);
  }
}

static intptr_t cb_jsChildCallbackParam(void) { return (intptr_t)parseChildParameterOrPropertyTag(4, jsIndentArg, NULL); }
static intptr_t cb_jsSignatureReturnTag(void) {
  if (parseOptionalJsdoc(TSK_AtToken)) {
    TsNode *tag = parseTag(jsIndentArg);
    if (tag && tag->kind == TSK_JSDocReturnTag) return (intptr_t)tag;
  }
  return 0;
}

/* parseJSDocSignature(start, indent) */
static TsNode *parseJSDocSignature(int start2, int indent3) {
  JNodes parameters = {0};
  parseCallbackTagParameters(indent3, &parameters);
  jsIndentArg = indent3;
  TsNode *returnTag = (TsNode *)tryParse(cb_jsSignatureReturnTag);
  free(parameters.v);
  TsNode *n = newNode(TSK_JSDocSignature);
  n->typeParameters = NULL;
  n->type = returnTag;
  return finishNode(n, start2, -1);
}

static TsNode *parseCallbackTag(int start2, TsNode *tagName, int indent3, JStr indentText) {
  TsNode *fullName = parseJSDocTypeNameWithNamespace(false);
  skipWhitespace();
  JComment comment = parseTagComments(indent3, jsStrUndef());
  TsNode *typeExpression = parseJSDocSignature(start2, indent3);
  if (comment.kind == JC_Undefined) comment = parseTrailingTagComments(start2, getNodePos(), indent3, indentText);
  int end2 = comment.kind != JC_Undefined ? getNodePos() : typeExpression->end;
  TsNode *n = newNode(TSK_JSDocCallbackTag);
  n->tagName = tagName;
  n->typeExpression = typeExpression;
  n->name = fullName;
  return finishNode(n, start2, end2);
}

static TsNode *parseOverloadTag(int start2, TsNode *tagName, int indent3, JStr indentText) {
  skipWhitespace();
  JComment comment = parseTagComments(indent3, jsStrUndef());
  TsNode *typeExpression = parseJSDocSignature(start2, indent3);
  if (comment.kind == JC_Undefined) comment = parseTrailingTagComments(start2, getNodePos(), indent3, indentText);
  int end2 = comment.kind != JC_Undefined ? getNodePos() : typeExpression->end;
  TsNode *n = newNode(TSK_JSDocOverloadTag);
  n->tagName = tagName;
  n->typeExpression = typeExpression;
  return finishNode(n, start2, end2);
}

/* template tags */
static TsNode *parseTemplateTagTypeParameter(void) {
  int typeParameterPos = getNodePos();
  bool isBracketed = parseOptionalJsdoc(TSK_OpenBracketToken);
  if (isBracketed) skipWhitespace();
  TsList *modifiers = parseModifiers(false, true, false);
  TsNode *name = parseJSDocIdentifierName(true);
  TsNode *defaultType = NULL;
  if (isBracketed) {
    skipWhitespace();
    parseExpected(TSK_EqualsToken);
    defaultType = (TsNode *)doInsideOfContext(TNF_JSDoc, cb_parseJSDocType);
    parseExpected(TSK_CloseBracketToken);
  }
  if (nodeIsMissing(name)) return NULL;
  TsNode *n = newNode(TSK_TypeParameter);
  n->modifiers = modifiers;
  n->name = name;
  n->defaultType = defaultType;
  return finishNode(n, typeParameterPos, -1);
}

static TsList *parseTemplateTagTypeParameters(void) {
  int pos = getNodePos();
  JNodes typeParameters = {0};
  do {
    skipWhitespace();
    TsNode *node = parseTemplateTagTypeParameter();
    if (node) jn_push(&typeParameters, node);
    skipWhitespaceOrAsterisk();
  } while (parseOptionalJsdoc(TSK_CommaToken));
  TsList *l = ts_list_empty(pos, -1);
  l->n = typeParameters.n;
  l->v = typeParameters.v;
  l->end = TS.fullStart;
  return l;
}

static TsNode *parseTemplateTag(int start2, TsNode *tagName, int indent3, JStr indentText) {
  TsNode *constraint = token() == TSK_OpenBraceToken ? parseJSDocTypeExpression(false) : NULL;
  TsList *typeParameters = parseTemplateTagTypeParameters();
  parseTrailingTagComments(start2, getNodePos(), indent3, indentText);
  TsNode *n = newNode(TSK_JSDocTemplateTag);
  n->tagName = tagName;
  n->constraint = constraint;
  n->typeParameters = typeParameters;
  return finishNode(n, start2, -1);
}

static TsNode *parseUnknownTag(int start2, TsNode *tagName, int indent3, JStr indentText) {
  parseTrailingTagComments(start2, getNodePos(), indent3, indentText);
  TsNode *n = newNode(TSK_JSDocTag); /* createJSDocUnknownTag makes a JSDocTag node */
  n->tagName = tagName;
  return finishNode(n, start2, -1);
}

static intptr_t cb_jsSeeLinkLookahead(void) {
  return nextTokenJSDoc() == TSK_AtToken && tokenIsIdentifierOrKeyword(nextTokenJSDoc()) && jsLinkKind(TS.tval, TS.tvalLen) != 0;
}

/* parseTrailingTagComments and parseTagComments: the comment after a tag, with its inline links */
static JComment parseTagComments(int indent3, JStr initialMargin);

static JComment parseTrailingTagComments(int pos, int end2, int margin, JStr indentText) {
  if (!indentText.def || indentText.s == indentText.e) margin += end2 - pos;
  return parseTagComments(margin, jsSliceFrom(indentText, margin));
}

static JComment parseTagComments(int indent3, JStr initialMargin) {
  int commentsPos2 = getNodePos();
  JPieces comments2 = {0};
  JNodes parts2 = {0};
  bool hasLinkEnd2 = false;
  int linkEnd2 = 0;
  int state = 0; /* BeginningOfLine */
  bool hasMargin = false;
  int margin = 0;
#define TAGC_PUSH(S, E)                                 \
  do {                                                  \
    if (!hasMargin || margin == 0) {                    \
      margin = indent3;                                 \
      hasMargin = true;                                 \
    }                                                   \
    jp_push(&comments2, (S), (E));                      \
    indent3 += (E) - (S);                               \
  } while (0)
  if (initialMargin.def) {
    if (initialMargin.s != initialMargin.e) TAGC_PUSH(initialMargin.s, initialMargin.e);
    state = 1; /* SawAsterisk */
  }
  int tok = token();
  for (;;) {
    bool brk = false;
    switch (tok) {
      case TSK_NewLineTrivia:
        state = 0;
        jp_push(&comments2, TS.tokenStart, TS.pos);
        indent3 = 0;
        break;
      case TSK_AtToken:
        ts_resetTokenState(TS.pos - 1);
        brk = true;
        break;
      case TSK_EndOfFileToken:
        brk = true;
        break;
      case TSK_WhitespaceTrivia: {
        int wl = TS.pos - TS.tokenStart;
        if (hasMargin && indent3 + wl > margin) {
          jp_push(&comments2, TS.tokenStart + (margin - indent3), TS.pos);
          state = 2; /* SavingComments */
        }
        indent3 += wl;
        break;
      }
      case TSK_OpenBraceToken: {
        state = 2;
        int commentEnd = TS.fullStart;
        int linkStart = TS.pos - 1;
        (void)linkStart;
        TsNode *link = parseJSDocLink(linkStart);
        if (link) {
          TsNode *t = newNode(TSK_JSDocText);
          jn_push(&parts2, finishNode(t, hasLinkEnd2 ? linkEnd2 : commentsPos2, commentEnd));
          jn_push(&parts2, link);
          comments2.n = 0;
          linkEnd2 = TS.pos;
          hasLinkEnd2 = true;
        } else {
          TAGC_PUSH(TS.tokenStart, TS.pos);
        }
        break;
      }
      case TSK_BacktickToken:
        state = state == 3 ? 2 : 3;
        TAGC_PUSH(TS.tokenStart, TS.pos);
        break;
      case TSK_JSDocCommentTextToken:
        if (state != 3) state = 2;
        TAGC_PUSH(TS.tokenStart, TS.pos);
        break;
      case TSK_AsteriskToken:
        if (state == 0) {
          state = 1;
          indent3 += 1;
          break;
        }
        /* falls through */
      default:
        if (state != 3) state = 2;
        TAGC_PUSH(TS.tokenStart, TS.pos);
        break;
    }
    if (brk) break;
    if (state == 2 || state == 3) tok = nextJSDocCommentTextToken(state == 3);
    else tok = nextTokenJSDoc();
  }
#undef TAGC_PUSH
  jp_removeLeadingNewlines(&comments2);
  int trimmed = jp_trimmedLen(&comments2);
  JComment r = {JC_Undefined, 0, 0, NULL};
  if (parts2.n) {
    if (trimmed) {
      TsNode *t = newNode(TSK_JSDocText);
      jn_push(&parts2, finishNode(t, hasLinkEnd2 ? linkEnd2 : commentsPos2, -1));
    }
    TsList *l = ts_list_empty(commentsPos2, TS.pos);
    l->n = parts2.n;
    l->v = parts2.v;
    r.kind = JC_Nodes;
    r.nodes = l;
  } else if (trimmed) {
    r.kind = JC_String;
  }
  free(comments2.v);
  return r;
}

/* parseTag(margin): an @tag of the comment; returns the tag node (NULL for a tag that is not one) */
static TsNode *parseTag(int margin) {
  int start2 = TS.tokenStart;
  nextTokenJSDoc();
  TsNode *tagName = parseJSDocIdentifierName(false);
  JStr indentText = skipWhitespaceOrAsterisk();
  if (jsNodeTextIs(tagName, "author")) return parseAuthorTag(start2, tagName, margin, indentText);
  if (jsNodeTextIs(tagName, "implements")) return parseImplementsTag(start2, tagName, margin, indentText);
  if (jsNodeTextIs(tagName, "augments") || jsNodeTextIs(tagName, "extends")) return parseAugmentsTag(start2, tagName, margin, indentText);
  if (jsNodeTextIs(tagName, "class") || jsNodeTextIs(tagName, "constructor")) return parseSimpleTagNode(start2, TSK_JSDocClassTag, tagName, margin, indentText);
  if (jsNodeTextIs(tagName, "public")) return parseSimpleTagNode(start2, TSK_JSDocPublicTag, tagName, margin, indentText);
  if (jsNodeTextIs(tagName, "private")) return parseSimpleTagNode(start2, TSK_JSDocPrivateTag, tagName, margin, indentText);
  if (jsNodeTextIs(tagName, "protected")) return parseSimpleTagNode(start2, TSK_JSDocProtectedTag, tagName, margin, indentText);
  if (jsNodeTextIs(tagName, "readonly")) return parseSimpleTagNode(start2, TSK_JSDocReadonlyTag, tagName, margin, indentText);
  if (jsNodeTextIs(tagName, "override")) return parseSimpleTagNode(start2, TSK_JSDocOverrideTag, tagName, margin, indentText);
  if (jsNodeTextIs(tagName, "deprecated")) {
    hasDeprecatedTag = true;
    return parseSimpleTagNode(start2, TSK_JSDocDeprecatedTag, tagName, margin, indentText);
  }
  if (jsNodeTextIs(tagName, "this")) return parseThisTag(start2, tagName, margin, indentText);
  if (jsNodeTextIs(tagName, "enum")) return parseEnumTag(start2, tagName, margin, indentText);
  if (jsNodeTextIs(tagName, "arg") || jsNodeTextIs(tagName, "argument") || jsNodeTextIs(tagName, "param")) {
    return parseParameterOrPropertyTag(start2, tagName, 2, margin);
  }
  if (jsNodeTextIs(tagName, "return") || jsNodeTextIs(tagName, "returns")) return parseReturnTag(start2, tagName, margin, indentText);
  if (jsNodeTextIs(tagName, "template")) return parseTemplateTag(start2, tagName, margin, indentText);
  if (jsNodeTextIs(tagName, "type")) return parseTypeTag(start2, tagName, true, margin, indentText);
  if (jsNodeTextIs(tagName, "typedef")) return parseTypedefTag(start2, tagName, margin, indentText);
  if (jsNodeTextIs(tagName, "callback")) return parseCallbackTag(start2, tagName, margin, indentText);
  if (jsNodeTextIs(tagName, "overload")) return parseOverloadTag(start2, tagName, margin, indentText);
  if (jsNodeTextIs(tagName, "satisfies")) return parseSatisfiesTag(start2, tagName, margin, indentText, true);
  if (jsNodeTextIs(tagName, "see")) return parseSeeTag(start2, tagName, true, margin, indentText);
  if (jsNodeTextIs(tagName, "exception") || jsNodeTextIs(tagName, "throws")) return parseThrowsTag(start2, tagName, margin, indentText);
  if (jsNodeTextIs(tagName, "import")) return parseImportTag(start2, tagName, margin, indentText, true);
  return parseUnknownTag(start2, tagName, margin, indentText);
}

/* ---- the comment: parseJSDocCommentWorker and its scan (doJSDocScan) ---- */

static bool isJSDocLikeTextAt(int start) {
  int len = TS.len;
  return start + 3 < len && TS.text[start + 1] == '*' && TS.text[start + 2] == '*' && TS.text[start + 3] != '/';
}

static TsNode *doJSDocScan(void) {
  JsDocCtx *c = JD;
  int state = 1; /* SawAsterisk */
  bool hasMargin = false;
  int margin = 0;
  int indent3;
  {
    int lf = -1;
    for (int i = c->start; i >= 0; i--) {
      if (TS.text[i] == '\n') {
        lf = i;
        break;
      }
    }
    indent3 = c->start - (lf + 1) + 4;
  }
#define JD_PUSH(S, E)                                   \
  do {                                                  \
    if (!hasMargin || margin == 0) {                    \
      margin = indent3;                                 \
      hasMargin = true;                                 \
    }                                                   \
    jp_push(&c->comments, (S), (E));                    \
    indent3 += (E) - (S);                               \
  } while (0)

  nextTokenJSDoc();
  while (parseOptionalJsdoc(TSK_WhitespaceTrivia)) {}
  if (parseOptionalJsdoc(TSK_NewLineTrivia)) {
    state = 0;
    indent3 = 0;
  }
  for (;;) {
    bool brk = false;
    switch (token()) {
      case TSK_AtToken:
        jp_removeTrailingWhitespace(&c->comments);
        if (!c->hasCommentsPos || c->commentsPos == 0) {
          c->commentsPos = getNodePos();
          c->hasCommentsPos = true;
        }
        addTag(parseTag(indent3));
        state = 0;
        hasMargin = false;
        break;
      case TSK_NewLineTrivia:
        jp_push(&c->comments, TS.tokenStart, TS.pos);
        state = 0;
        indent3 = 0;
        break;
      case TSK_AsteriskToken: {
        int aS = TS.tokenStart, aE = TS.pos;
        if (state == 1) {
          state = 2;
          JD_PUSH(aS, aE);
        } else {
          state = 1;
          indent3 += aE - aS;
        }
        break;
      }
      case TSK_WhitespaceTrivia: {
        int wl = TS.pos - TS.tokenStart;
        if (hasMargin && indent3 + wl > margin) {
          jp_push(&c->comments, TS.tokenStart + (margin - indent3), TS.pos);
        }
        indent3 += wl;
        break;
      }
      case TSK_EndOfFileToken:
        brk = true;
        break;
      case TSK_JSDocCommentTextToken:
        state = 2;
        JD_PUSH(TS.tokenStart, TS.pos);
        break;
      case TSK_OpenBraceToken: {
        state = 2;
        int commentEnd = TS.fullStart;
        int linkStart = TS.pos - 1;
        TsNode *link = parseJSDocLink(linkStart);
        if (link) {
          if (!c->hasLinkEnd) jp_removeLeadingNewlines(&c->comments);
          TsNode *t = newNode(TSK_JSDocText);
          jn_push(&c->parts, finishNode(t, c->hasLinkEnd ? c->linkEnd : c->start, commentEnd));
          jn_push(&c->parts, link);
          c->comments.n = 0;
          c->linkEnd = TS.pos;
          c->hasLinkEnd = true;
          break;
        }
        /* no link: the brace is ordinary comment text (JS falls through to default) */
        JD_PUSH(TS.tokenStart, TS.pos);
        break;
      }
      default:
        state = 2;
        JD_PUSH(TS.tokenStart, TS.pos);
        break;
    }
    if (brk) break;
    if (state == 2) nextJSDocCommentTextToken(false);
    else nextTokenJSDoc();
  }
#undef JD_PUSH
  int trimmed = jp_trimmedLen(&c->comments);
  if (c->parts.n && trimmed) {
    TsNode *t = newNode(TSK_JSDocText);
    jn_push(&c->parts, finishNode(t, c->hasLinkEnd ? c->linkEnd : c->start, c->hasCommentsPos ? c->commentsPos : -1));
  }
  TsNode *cm = newNode(TSK_JSDocComment);
  if (c->hasTags) {
    TsList *tl = ts_list_empty(c->tagsPos, c->tagsEnd);
    tl->n = c->tags.n;
    tl->v = c->tags.v;
    cm->list1 = tl;
  }
  if (c->parts.n) {
    TsList *pl = ts_list_empty(c->start, c->hasCommentsPos ? c->commentsPos : -1);
    pl->n = c->parts.n;
    pl->v = c->parts.v;
    cm->list2 = pl;
  }
  return finishNode(cm, c->start, -1);
}

static void jsDocCtxInit(JsDocCtx *c, int start) {
  memset(c, 0, sizeof *c);
  c->start = start;
}

/* parseJSDocCommentWorker(start, length) */
static TsNode *parseJSDocCommentWorker(int start, int length) {
  int end = start + length;
  length = end - start;
  if (!isJSDocLikeTextAt(start)) return NULL;
  int saveParsingContext = P.parsingContext;
  P.parsingContext |= 1 << PC_JSDocComment;
  JsDocCtx ctx;
  jsDocCtxInit(&ctx, start);
  JsDocCtx *saveJD = JD;
  JD = &ctx;
  JsRangeSave sv;
  jsScanRangeBegin(&sv, start + 3, length - 5);
  TsNode *result = doJSDocScan();
  jsScanRangeEnd(&sv);
  JD = saveJD;
  P.parsingContext = saveParsingContext;
  free(ctx.comments.v);
  free(ctx.parts.v);
  free(ctx.tags.v);
  return result;
}

/* parseJSDocComment(parent, start, length): parses one comment, keeps the parser's token and error state */
static intptr_t cb_jsWorker(void);
static int jsWorkerStart, jsWorkerLen;
static intptr_t cb_jsWorker(void) { return (intptr_t)parseJSDocCommentWorker(jsWorkerStart, jsWorkerLen); }

static TsNode *parseJSDocComment(TsNode *parent, int start, int length) {
  (void)parent;
  int saveToken = P.currentToken;
  int saveErr = P.nerr;
  bool saveErrBefore = P.errBeforeNextFinishedNode;
  jsWorkerStart = start;
  jsWorkerLen = length;
  TsNode *comment = (TsNode *)doInsideOfContext(TNF_JSDoc, cb_jsWorker);
  P.currentToken = saveToken;
  P.nerr = saveErr;
  P.errBeforeNextFinishedNode = saveErrBefore;
  return comment;
}

/* addJSDocComment(node): every JSDoc comment in front of the node is parsed */
static TsNode *addJSDocComment(TsNode *node) {
  CRanges rs = {0};
  getJSDocCommentRanges(node, &rs);
  for (int i = 0; i < rs.n; i++) {
    parseJSDocComment(node, rs.v[i].pos, rs.v[i].end - rs.v[i].pos);
  }
  free(rs.v);
  if (hasDeprecatedTag) {
    hasDeprecatedTag = false;
    node->flags |= 536870912u;
  }
  return node;
}

#endif
