// jetstream-row: WSL
// jetstream-files: WSL/Node.js WSL/Type.js WSL/ReferenceType.js WSL/Value.js WSL/Expression.js WSL/Rewriter.js WSL/Visitor.js WSL/CreateLiteral.js WSL/CreateLiteralType.js WSL/PropertyAccessExpression.js WSL/AddressSpace.js WSL/AnonymousVariable.js WSL/ArrayRefType.js WSL/ArrayType.js WSL/Assignment.js WSL/AutoWrapper.js WSL/Block.js WSL/BoolLiteral.js WSL/Break.js WSL/CallExpression.js WSL/CallFunction.js WSL/Check.js WSL/CheckLiteralTypes.js WSL/CheckLoops.js WSL/CheckRecursiveTypes.js WSL/CheckRecursion.js WSL/CheckReturns.js WSL/CheckUnreachableCode.js WSL/CheckWrapped.js WSL/Checker.js WSL/CloneProgram.js WSL/CommaExpression.js WSL/ConstexprFolder.js WSL/ConstexprTypeParameter.js WSL/Continue.js WSL/ConvertPtrToArrayRefExpression.js WSL/DereferenceExpression.js WSL/DoWhileLoop.js WSL/DotExpression.js WSL/DoubleLiteral.js WSL/DoubleLiteralType.js WSL/EArrayRef.js WSL/EBuffer.js WSL/EBufferBuilder.js WSL/EPtr.js WSL/EnumLiteral.js WSL/EnumMember.js WSL/EnumType.js WSL/EvaluationCommon.js WSL/Evaluator.js WSL/ExpressionFinder.js WSL/ExternalOrigin.js WSL/Field.js WSL/FindHighZombies.js WSL/FlattenProtocolExtends.js WSL/FlattenedStructOffsetGatherer.js WSL/FloatLiteral.js WSL/FloatLiteralType.js WSL/FoldConstexprs.js WSL/ForLoop.js WSL/Func.js WSL/FuncDef.js WSL/FuncInstantiator.js WSL/FuncParameter.js WSL/FunctionLikeBlock.js WSL/HighZombieFinder.js WSL/IdentityExpression.js WSL/IfStatement.js WSL/IndexExpression.js WSL/InferTypesForCall.js WSL/Inline.js WSL/Inliner.js WSL/InstantiateImmediates.js WSL/IntLiteral.js WSL/IntLiteralType.js WSL/Intrinsics.js WSL/LateChecker.js WSL/Lexer.js WSL/LexerToken.js WSL/LiteralTypeChecker.js WSL/LogicalExpression.js WSL/LogicalNot.js WSL/LoopChecker.js WSL/MakeArrayRefExpression.js WSL/MakePtrExpression.js WSL/NameContext.js WSL/NameFinder.js WSL/NameResolver.js WSL/NativeFunc.js WSL/NativeFuncInstance.js WSL/NativeType.js WSL/NativeTypeInstance.js WSL/NormalUsePropertyResolver.js WSL/NullLiteral.js WSL/NullType.js WSL/OriginKind.js WSL/OverloadResolutionFailure.js WSL/Parse.js WSL/Prepare.js WSL/Program.js WSL/ProgramWithUnnecessaryThingsRemoved.js WSL/PropertyResolver.js WSL/Protocol.js WSL/ProtocolDecl.js WSL/ProtocolFuncDecl.js WSL/ProtocolRef.js WSL/PtrType.js WSL/ReadModifyWriteExpression.js WSL/RecursionChecker.js WSL/RecursiveTypeChecker.js WSL/ResolveNames.js WSL/ResolveOverloadImpl.js WSL/ResolveProperties.js WSL/ResolveTypeDefs.js WSL/Return.js WSL/ReturnChecker.js WSL/ReturnException.js WSL/StandardLibrary.js WSL/StatementCloner.js WSL/StructLayoutBuilder.js WSL/StructType.js WSL/Substitution.js WSL/SwitchCase.js WSL/SwitchStatement.js WSL/SynthesizeEnumFunctions.js WSL/SynthesizeStructAccessors.js WSL/TrapStatement.js WSL/TypeDef.js WSL/TypeDefResolver.js WSL/TypeOrVariableRef.js WSL/TypeParameterRewriter.js WSL/TypeRef.js WSL/TypeVariable.js WSL/TypeVariableTracker.js WSL/TypedValue.js WSL/UintLiteral.js WSL/UintLiteralType.js WSL/UnificationContext.js WSL/UnreachableCodeChecker.js WSL/VariableDecl.js WSL/VariableRef.js WSL/VisitingSet.js WSL/WSyntaxError.js WSL/WTrapError.js WSL/WTypeError.js WSL/WhileLoop.js WSL/WrapChecker.js WSL/Test.js
// iterations: 120
// jetstream-driver: runner=wsl
/* C analogue of the lexing stage of JetStream's WSL row (WSL/Lexer.js, LexerToken.js).
 *
 * Scope: this is the lexer only. The JS row compiles every test program through the full
 * pipeline (Parse.js, the name resolver, Checker.js and the rest of the WSL passes, about
 * 20k lines in 160 files). This analogue lexes the same program texts: the standard library
 * (native origin) and every literal argument of doPrep(...) (user origin) or doLex(...)
 * (native origin) in WSL/Test.js; the one template with ${addressSpace} expands over
 * addressSpaces from WSL/AddressSpace.js, as the JS does. Each iteration lexes all inputs;
 * the lexer follows Lexer.next() step by step, including its leftmost-search punctuation
 * rule (only the first alternative of that regex is anchored).
 * Validation: input count, character count, token count, error count and the sum of a
 * per-input FNV-1a over (kind, text) must match the JS lexer's results on the same inputs.
 * Usage: wsl [jetstream-root] [iterations]   root defaults to ../jetstream; iterations to 120. */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "wsl-lex.h"
#include "wsl-inputs.h"

static double now_ms(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return t.tv_sec * 1e3 + t.tv_nsec / 1e6;
}

/* Folds one token into an FNV-1a state: kind, 0x01, text, 0x02. */
static uint32_t fnv_token(uint32_t h, const Tok *t) {
  const char *k = t->kind;
  for (; *k; k++) { h ^= (unsigned char)*k; h *= 16777619u; }
  h ^= 1; h *= 16777619u;
  for (int i = 0; i < t->len; i++) { h ^= (unsigned char)t->text[i]; h *= 16777619u; }
  h ^= 2; h *= 16777619u;
  return h;
}

int main(int argc, char **argv) {
  const char *root = argc > 2 ? argv[2] : "../jetstream";
  int iterations = argc > 1 ? atoi(argv[1]) : 120;
  collect_inputs(root);

  long chars = 0;
  for (int i = 0; i < NINPUTS; i++) chars += INPUTS[i].len;
  long tokens = 0, errors = 0;
  uint32_t hashsum = 0;
  double elapsed = 0;
  for (int it = 0; it < iterations; it++) {
    double t0 = now_ms();
    long tok = 0, err = 0;
    uint32_t hs = 0;
    for (int i = 0; i < NINPUTS; i++) {
      Lexer L = {INPUTS[i].text, INPUTS[i].len, 0, INPUTS[i].native, "/internal/test", 0, NULL};
      Tok t;
      uint32_t h = 0x811c9dc5u;
      int r;
      long count = 0;
      while ((r = lex_next(&L, &t)) == 1) {
        count++;
        h = fnv_token(h, &t);
      }
      if (r < 0) err++;
      tok += count;
      hs += h;
    }
    elapsed += now_ms() - t0;
    tokens = tok; errors = err; hashsum = hs;
  }
  printf("wsl lexer %d iterations %.3f ms\n", iterations, elapsed);
  printf("inputs %d chars %ld tokens %ld errors %ld hashsum %u\n", NINPUTS, chars, tokens, errors, (unsigned)hashsum);
  return 0;
}
