// jetstream-row: WSL
// jetstream-files: WSL/OriginKind.js WSL/WSyntaxError.js WSL/LexerToken.js WSL/Lexer.js WSL/StandardLibrary.js WSL/AddressSpace.js WSL/Test.js
// iterations: 120
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
  const char *root = argc > 1 ? argv[1] : "../jetstream";
  int iterations = argc > 2 ? atoi(argv[2]) : 120;
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
