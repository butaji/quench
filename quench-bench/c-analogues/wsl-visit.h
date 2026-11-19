/* A port of Visitor.js as a traversal: the default visit method of every node class, with the children
 * visited in the order of the JS methods (the order matters for StructLayoutBuilder's offsets, and for the
 * checker's and the inliner's per-node state). A visitor that overrides nothing is the base Visitor: the
 * trace records each Node.visit call as the JS constructor's name (a literal is "GenericLiteral", its
 * type "GenericLiteralType"), in the order the calls are made. Each call that the JS Node.visit would
 * reject (no visit method) is an error, as in Node.visit's "No visit function" message.
 *
 * visit_trace(b) is the --visit mode of wsl-ast-dump: the Program line, then the top-level statements,
 * then every instance of inline()'s first loop in the order of the JS _instances Map (a function's first
 * getUnique call, then its instances in creation order), each visited as a root. The lines are joined by
 * newlines with no trailing newline, as the node driver's dump. */
#ifndef WSL_VISIT_H
#define WSL_VISIT_H

#include "wsl-inline.h"
#include "wsl-late.h"
#include "wsl-layout.h"
#include "wsl-ebuf.h"

/* late: 1 for LateChecker's traversal (wsl-late.h): the same visits, with its checks in the cases below */
typedef struct { Buf *b; int n; int late; int layout; int ebuf; } VTrace;   /* ebuf: EBufferBuilder (wsl-ebuf.h) */

static const char *vis_class(const Ast *n) {
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
  case A_INTLITERAL: case A_UINTLITERAL: case A_FLOATLITERAL: case A_DOUBLELITERAL: return "GenericLiteral";
  case A_INTLITERALTYPE: case A_UINTLITERALTYPE: case A_FLOATLITERALTYPE: case A_DOUBLELITERALTYPE: return "GenericLiteralType";
  case A_NULLTYPE: return "NullType";
  case A_IDENTITY: return "IdentityExpression";
  case A_ENUMLITERAL: return "EnumLiteral";
  case A_ANONVAR: return "AnonymousVariable";
  case A_CONVERTPTR: return "ConvertPtrToArrayRefExpression";
  case A_NATIVEFUNCINST: return "NativeFuncInstance";
  case A_FUNCTIONLIKEBLOCK: return "FunctionLikeBlock";
  }
  return "?";
}

static void vt_node(VTrace *t, Ast *n);

/* Node.visit(child): a null child is not a node and is skipped */
static void vt_opt(VTrace *t, Ast *c) { if (c) vt_node(t, c); }
/* child.visit(visitor) for a child the JS reads without a null check: a null child is an error there, and
 * is never present in a checked program, so it is skipped here too */
static void vt_req(VTrace *t, Ast *c) { if (c) vt_node(t, c); }
static void vt_list(VTrace *t, const AstList *l) { for (int i = 0; i < l->n; i++) vt_req(t, l->v[i]); }

/* Visitor.visitFunc */
static void vt_func(VTrace *t, Ast *n) {
  vt_req(t, n->u.func.returnType);
  vt_list(t, &n->u.func.typeParameters);
  vt_list(t, &n->u.func.parameters);
}

/* the property access defaults (_handlePropertyAccessExpression) */
static void vt_prop(VTrace *t, Ast *pc_base, const PropChk *pc) {
  vt_opt(t, pc->baseType);
  vt_opt(t, pc->callGet);
  vt_opt(t, pc->resGet);
  vt_opt(t, pc->callAnd);
  vt_opt(t, pc->resAnd);
  vt_opt(t, pc->callSet);
  (void)pc_base;
}

static void vt_node(VTrace *t, Ast *n) {
  char line[64];
  const char *cls = vis_class(n);
  if (strcmp(cls, "?") == 0) fatal_unsupported("visit of an unknown node", n);
  if (t->n++) buf_str(t->b, "\n");
  buf_str(t->b, cls);
  switch (n->tag) {
  case A_FUNCDEF:          /* visitFuncDef (LateChecker: the shader check first; EBufferBuilder: the return EPtr first) */
    if (t->late && n->u.func.shaderType) late_shader(n);
    if (t->ebuf) n->ePtrAux = eb_create(n->u.func.returnType);
    vt_func(t, n);
    vt_req(t, n->u.func.body);
    break;
  case A_NATIVEFUNC:       /* visitNativeFunc */
  case A_PROTOCOLFUNCDECL: /* visitProtocolFuncDecl */
    vt_func(t, n);
    break;
  case A_NATIVEFUNCINST:   /* visitNativeFuncInstance: visitFunc, then func.visitImplementationData (the accessor's type) */
    vt_func(t, n);
    vt_opt(t, n->u.func.implType);
    if (t->layout) lsb_native_inst(n);   /* then didLayoutStructsInImplementationData */
    break;
  case A_FUNCPARAMETER:    /* visitFuncParameter (EBufferBuilder: its EPtr, without the type) */
    if (t->ebuf) { n->ePtr = eb_create(n->u.named.type); break; }
    vt_req(t, n->u.named.type);
    break;
  case A_CONSTEXPRTYPEPARAM:   /* visitConstexprTypeParameter */
    vt_req(t, n->u.named.type);
    break;
  case A_FIELD:            /* visitField: super.visitField (the type), then the offset and the advance */
    vt_req(t, n->u.named.type);
    if (t->layout) lsb_field(n);
    break;
  case A_BLOCK:
    vt_list(t, &n->u.block.statements);
    break;
  case A_COMMA:
    vt_list(t, &n->u.comma.list);
    break;
  case A_PROTOCOLREF:
    break;
  case A_PROTOCOLDECL:
    vt_list(t, &n->u.protocolDecl.extends);
    vt_list(t, &n->u.protocolDecl.signatures);
    break;
  case A_TYPEREF:          /* visitTypeRef: the type arguments, then (StructLayoutBuilder) the target */
    vt_list(t, &n->u.typeRef.typeArguments);
    if (t->layout) vt_req(t, n->u.typeRef.type);
    break;
  case A_NATIVETYPE:
    vt_list(t, &n->u.nativeType.typeParameters);
    break;
  case A_TYPEDEF:
    vt_list(t, &n->u.typeDef.typeParameters);
    vt_req(t, n->u.typeDef.type);
    break;
  case A_STRUCTTYPE:       /* visitStructType (StructLayoutBuilder: the layout of the fields, when it has none yet) */
    if (t->layout) {
      if (n->hasSize) break;
      int64_t old = lsb_struct_begin(n);
      vt_list(t, &n->u.structType.typeParameters);
      vt_list(t, &n->u.structType.fields);
      lsb_struct_end(n, old);
      break;
    }
    vt_list(t, &n->u.structType.typeParameters);
    vt_list(t, &n->u.structType.fields);
    break;
  case A_TYPEVARIABLE:     /* Node.visit(node.protocol) */
    vt_opt(t, n->u.typeVariable.protocol);
    break;
  case A_ENUMTYPE:
    vt_req(t, n->u.enumType.baseType);
    vt_list(t, &n->u.enumType.members);
    break;
  case A_ENUMMEMBER:       /* Node.visit(node.value) */
    vt_opt(t, n->u.nameOnly.value);
    break;
  case A_ENUMLITERAL:     /* EBufferBuilder.visitEnumLiteral: a box */
    if (t->ebuf) n->ePtr = eptr_box();
    break;
  case A_PTRTYPE:          /* visitPtrType -> visitReferenceType -> visitElementalType */
  case A_ARRAYREFTYPE:
    if (t->late) {         /* LateChecker.visitReferenceType: checked, and the element type is not visited */
      late_reference(n);
      break;
    }
    if (t->layout) break;  /* StructLayoutBuilder.visitReferenceType: empty */
    vt_req(t, n->u.ref.elementType);
    break;
  case A_ARRAYTYPE:        /* visitArrayType */
    vt_req(t, n->u.arrayType.elementType);
    vt_req(t, n->u.arrayType.numElements);
    break;
  case A_VARIABLEDECL:     /* EBufferBuilder.visitVariableDecl: its EPtr, then the initializer only */
    if (t->ebuf) {
      n->ePtr = eb_create(n->u.varDecl.type);
      vt_opt(t, n->u.varDecl.initializer);
      break;
    }
    vt_req(t, n->u.varDecl.type);
    vt_opt(t, n->u.varDecl.initializer);
    break;
  case A_ASSIGN:
    vt_req(t, n->u.assign.lhs);
    vt_req(t, n->u.assign.rhs);
    vt_opt(t, n->type);
    break;
  case A_RMW:              /* lValue, oldValueVar, newValueVar, newValueExp, resultExp */
    vt_req(t, n->u.rmw.lValue);
    vt_req(t, n->u.rmw.oldVar);
    vt_req(t, n->u.rmw.newVar);
    vt_req(t, n->u.rmw.newValueExp);
    vt_req(t, n->u.rmw.resultExp);
    break;
  case A_DEREF:
    vt_req(t, n->u.deref.ptr);
    break;
  case A_DOT:             /* visitDotExpression: struct, then the property access defaults */
    vt_req(t, n->u.dot.base);
    vt_prop(t, n->u.dot.base, &n->u.dot.pc);
    break;
  case A_INDEX:           /* visitIndexExpression: array, index, then the property access defaults */
    vt_req(t, n->u.index.base);
    vt_req(t, n->u.index.index);
    vt_prop(t, n->u.index.base, &n->u.index.pc);
    break;
  case A_MAKEPTR:
  case A_CONVERTPTR:
    if (t->ebuf) n->ePtr = eptr_box();
    vt_req(t, n->u.lvalueRef.lValue);
    break;
  case A_MAKEARRAYREF:
    if (t->ebuf) n->ePtr = eptr_box();
    vt_req(t, n->u.lvalueRef.lValue);
    vt_opt(t, n->u.lvalueRef.numElements);
    break;
  case A_VARIABLEREF:
    break;
  case A_IF:
    vt_req(t, n->u.ifStmt.conditional);
    vt_req(t, n->u.ifStmt.body);
    vt_opt(t, n->u.ifStmt.elseBody);
    break;
  case A_WHILE:
    vt_req(t, n->u.loop.conditional);
    vt_req(t, n->u.loop.body);
    break;
  case A_DOWHILE:
    vt_req(t, n->u.loop.body);
    vt_req(t, n->u.loop.conditional);
    break;
  case A_FOR:
    vt_opt(t, n->u.forLoop.initialization);
    vt_opt(t, n->u.forLoop.condition);
    vt_opt(t, n->u.forLoop.increment);
    vt_req(t, n->u.forLoop.body);
    break;
  case A_SWITCHSTATEMENT:
    vt_req(t, n->u.switchStmt.value);
    vt_list(t, &n->u.switchStmt.cases);
    break;
  case A_SWITCHCASE:
    vt_opt(t, n->u.switchCase.value);
    vt_req(t, n->u.switchCase.body);
    break;
  case A_RETURN:
    vt_opt(t, n->u.ret.value);
    break;
  case A_CONTINUE:
  case A_BREAK:
  case A_TRAP:
    break;
  case A_INTLITERAL: case A_UINTLITERAL: case A_FLOATLITERAL: case A_DOUBLELITERAL:
    if (t->ebuf) { n->ePtr = eptr_box(); break; }   /* EBufferBuilder.visitGenericLiteral: a box, no children */
    vt_req(t, n->type);                 /* visitGenericLiteral: node.type */
    break;
  case A_INTLITERALTYPE: case A_UINTLITERALTYPE: case A_FLOATLITERALTYPE: case A_DOUBLELITERALTYPE: {
    /* visitGenericLiteralType: Node.visit(node.type) (none), then node.preferredType */
    Ast *o = n->u.litType.owner;
    vt_opt(t, n->type);
    vt_req(t, literal_preferred(o));    /* created on first use (wsl-prep.h), as the JS constructor does */
    break;
  }
  case A_NULLTYPE:         /* visitNullType: Node.visit(node.type) */
    vt_opt(t, n->type);
    break;
  case A_NULLLITERAL:      /* visitNullLiteral: node.type */
    if (t->ebuf) { n->ePtr = eptr_box(); break; }
    vt_req(t, n->type);
    break;
  case A_BOOLLITERAL:
    if (t->ebuf) n->ePtr = eptr_box();
    break;
  case A_CALL:             /* visitCallExpression (StructLayoutBuilder: the arguments, instance type arguments, instance and result) */
    if (t->ebuf) {         /* EBufferBuilder.visitCallExpression: the result EPtr, then the Visitor's traversal */
      if (!n->u.call.resultType) fatal_unsupported("call without a result type", n);
      n->ePtrAux = eb_create(n->u.call.resultType);
    }
    if (t->layout) {
      for (int i = 0; i < n->u.call.argumentList.n; i++) vt_opt(t, n->u.call.argumentList.v[i]);
      vt_list(t, &n->u.call.instTA);
      vt_opt(t, n->u.call.nfi);
      vt_opt(t, n->u.call.resultType);
      break;
    }
    vt_list(t, &n->u.call.typeArguments);
    for (int i = 0; i < n->u.call.argumentList.n; i++) vt_opt(t, n->u.call.argumentList.v[i]);
    vt_list(t, &n->u.call.actualTA);
    vt_list(t, &n->u.call.instTA);
    vt_opt(t, n->u.call.nfi);
    vt_opt(t, n->u.call.returnType);
    vt_opt(t, n->u.call.resultType);
    break;
  case A_LOGICALNOT:
    if (t->ebuf) n->ePtr = eptr_box();
    vt_req(t, n->u.notExpr.operand);
    break;
  case A_LOGICAL:
    if (t->ebuf) n->ePtr = eptr_box();
    vt_req(t, n->u.logical.left);
    vt_req(t, n->u.logical.right);
    break;
  case A_FUNCTIONLIKEBLOCK: /* visitFunctionLikeBlock (EBufferBuilder: its returnEPtr first) */
    if (t->ebuf) n->ePtrAux = eb_create(n->u.fblock.returnType);
    vt_opt(t, n->u.fblock.returnType);
    vt_list(t, &n->u.fblock.argumentList);
    vt_list(t, &n->u.fblock.parameters);
    vt_req(t, n->u.fblock.body);
    break;
  case A_ANONVAR:          /* visitAnonymousVariable: Node.visit(node.type) (EBufferBuilder: its EPtr, no children) */
    if (t->ebuf) { n->ePtr = eb_create(n->type); break; }
    vt_opt(t, n->type);
    break;
  case A_IDENTITY:
    vt_req(t, n->u.identity.target);
    break;
  default:
    snprintf(line, sizeof line, "No visit function for %s in Visitor", cls);
    chk_error(line);
  }
}

/* The --visit trace of a program: the Program line, its top-level statements, then the instances. */
static int visit_trace(Buf *b, const Program *p) {
  VTrace t = {b, 0, 0, 0, 0};
  if (t.n++) buf_str(b, "\n");
  buf_str(b, "Program");
  for (int k = 0; k < p->statements.n; k++) vt_node(&t, p->statements.v[k]);
  for (int f = 0; f < FI_NF; f++)
    for (int i = 0; i < FI_N; i++)
      if (FI_TAB[i].func == FI_FUNCS[f]) vt_node(&t, FI_TAB[i].inst);
  return t.n;
}

/* --late: LateChecker (wsl-late.h) on each function of inline()'s first loop: the instance is made, then, unless it is
 * native, visited by LateChecker's traversal; the instances are visited in order, and the first error stops the input.
 * Returns the trace lines written to b (no trailing newline). */
static int late_trace(Buf *b, const Program *p) {
  VTrace t = {b, 0, 1};
  Ast **fs = arena_alloc(sizeof(Ast *) * (p->statements.n + 1));
  int n = top_funcs(p, fs);
  for (int i = 0; i < n; i++) {
    Ast *inst = fi_get_unique(fs[i], NULL, 0);
    if (inst->tag == A_NATIVEFUNCINST) continue;   /* _inlineFunction returns for native functions */
    vt_node(&t, inst);
  }
  return t.n;
}

/* --layout: for each function of inline()'s first loop, as _inlineFunction does it: LateChecker, then the layout
 * (wsl-layout.h). The log (LSB_LOG) is the output: one entry per struct laid out and per accessor instance. */
static void layout_run(Buf *log, const Program *p) {
  Buf scratch = {0};
  Ast **fs = arena_alloc(sizeof(Ast *) * (p->statements.n + 1));
  int n = top_funcs(p, fs);
  LSB_LOG = log;
  LSB_NE = 0;
  for (int i = 0; i < n; i++) {
    Ast *inst = fi_get_unique(fs[i], NULL, 0);
    if (inst->tag == A_NATIVEFUNCINST) continue;
    VTrace lt = {&scratch, 0, 1, 0};
    vt_node(&lt, inst);
    scratch.n = 0;
    LSB_OFF = 0;
    VTrace st = {&scratch, 0, 0, 1};
    vt_node(&st, inst);
    scratch.n = 0;
  }
  LSB_LOG = NULL;
}

/* --ebuf: inline()'s order over the non-generic functions (LateChecker, the layout, then EBufferBuilder for each
 * instance). The output: the instance trees, each node with its EPtrs (EBUF_DUMP), then "buffers N" and the
 * buffers in creation order (Buffer.toString). A failure in a pass leaves the output empty (the caller prints
 * the status alone). */
static void ebuf_pass(Buf *out, const Program *p) {
  Buf scratch = {0};
  Ast **fs = arena_alloc(sizeof(Ast *) * (p->statements.n + 1));
  int n = top_funcs(p, fs);
  Ast **insts = arena_alloc(sizeof(Ast *) * (n + 1));
  int ni = 0;
  EBUF_COUNT = 0;
  EBUF_NALL = 0;
  for (int i = 0; i < n; i++) {
    Ast *inst = fi_get_unique(fs[i], NULL, 0);
    if (inst->tag == A_NATIVEFUNCINST) continue;
    VTrace lt = {&scratch, 0, 1, 0, 0};
    vt_node(&lt, inst);
    scratch.n = 0;
    LSB_OFF = 0;
    VTrace st = {&scratch, 0, 0, 1, 0};
    vt_node(&st, inst);
    scratch.n = 0;
    VTrace et = {&scratch, 0, 0, 0, 1};
    vt_node(&et, inst);
    scratch.n = 0;
    insts[ni++] = inst;
  }
  Buf body = {0};
  EBUF_DUMP = 1;
  for (int k = 0; k < ni; k++) dump_node(&body, insts[k], 1);   /* each line ends in a newline */
  EBUF_DUMP = 0;
  buf_fmt(&body, "buffers %d\n", EBUF_NALL);
  for (int j = 0; j < EBUF_NALL; j++) {
    buf_str(&body, ebuf_str(EBUF_ALL[j]));
    buf_str(&body, "\n");
  }
  if (body.n && body.s[body.n - 1] == '\n') body.n--;   /* the dump text has no trailing newline */
  buf_put(out, body.s, body.n);
}

#endif
