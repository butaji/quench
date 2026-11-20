// jetstream-row: Air
// jetstream-files: ARES-6/Air/symbols.js ARES-6/Air/tmp_base.js ARES-6/Air/arg.js ARES-6/Air/basic_block.js ARES-6/Air/code.js ARES-6/Air/frequented_block.js ARES-6/Air/inst.js ARES-6/Air/opcode.js ARES-6/Air/reg.js ARES-6/Air/stack_slot.js ARES-6/Air/tmp.js ARES-6/Air/util.js ARES-6/Air/custom.js ARES-6/Air/liveness.js ARES-6/Air/insertion_set.js ARES-6/Air/allocate_stack.js ARES-6/Air/payload-gbemu-executeIteration.js ARES-6/Air/payload-imaging-gaussian-blur-gaussianBlur.js ARES-6/Air/payload-airjs-ACLj8C.js ARES-6/Air/payload-typescript-scanIdentifier.js ARES-6/Air/benchmark.js
// iterations: 120
/*
 * C analogue of ARES-6 Air's benchmark.js runIteration. Every iteration builds the IR of
 * the four payloads (gbemu executeIteration, imaging gaussianBlur, typescript
 * scanIdentifier, airjs ACLj8C) afresh, hashes it, runs allocateStack, and hashes it
 * again, checking the same early and late hashes as benchmark.js. Only allocateStack is
 * run by the benchmark (it is not a register allocator), so that is what is mirrored:
 * stack-slot assignment, the liveness fixed point with the same ordered-set semantics as
 * JS Set (insertion order decides the order of interference lists), dead spill-store
 * elimination, frame and call-area sizing, and the rewrite of Stack and CallArg operands
 * to rbp-relative addresses. The opcode operand roles and widths are those of
 * opcode.js; the Arg hash and the Code hash are those of arg.js, inst.js and code.js.
 *
 * Payload data: the JS payloads are straight-line code that calls Code/Inst/Arg
 * constructors. Instead of a generator, this program parses those four .js files at
 * startup (argv[2] names the jetstream directory) into a flat op list, one op per
 * statement, and each iteration replays that list to build the Code, which is the C
 * counterpart of calling createPayloadX(). The parser accepts exactly the statement forms
 * the payloads use and aborts on anything else. Blocks, instructions and operands are
 * heap objects, as in JS. Deliberate differences: no per-object GC headers, and
 * temporaries of the form newTmp are only counted (the payloads never reference them).
 * JS's insertionSet.insert is missing from insertion_set.js, so the one path that would
 * call it (a ZDef stack operand on a spill slot wider than the operand) aborts here, as it
 * would throw in JS; the payloads never reach it. */
#define _POSIX_C_SOURCE 199309L
#include <ctype.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define NUM_PAYLOADS 4
#define MAX_SLOTS 64
#define MAX_REFS 64
#define DEFAULT_ITERATIONS 120

static void fatal(const char *msg) {
  fprintf(stderr, "Error: %s\n", msg);
  exit(1);
}

/* ---- symbols.js, reg.js, tmp_base.js ---- */
enum { TG, TF };                                  /* GP, FP */
enum { RU, RCU, RLU, RLCU, RD, RZD, RUD, RUZD, RED, RSC, RUA }; /* roles, arg.js order */
enum { K_INVALID, K_TMP, K_IMM, K_BIGIMM, K_BITIMM, K_BITIMM64, K_ADDR, K_STACK,
       K_CALLARG, K_INDEX, K_RELCOND, K_RESCOND, K_DOUBLECOND, K_SPECIAL };
enum { REG_RBP = 5 };                              /* callFrameRegister */

/* Register ids: GP index 0..15 in ids 0..15, FP xmm index i in id 16 + i. */
static const char *reg_names[32] = {
  "rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi", "r8", "r9", "r10", "r11",
  "r12", "r13", "r14", "r15", "xmm0", "xmm1", "xmm2", "xmm3", "xmm4", "xmm5", "xmm6",
  "xmm7", "xmm8", "xmm9", "xmm10", "xmm11", "xmm12", "xmm13", "xmm14", "xmm15"
};

static uint32_t reg_hash(int r) {
  /* Reg.hash: GP -> 1 + index, FP -> -1 - index */
  return r < 16 ? (uint32_t)(1 + r) : (uint32_t)(-1 - (r - 16));
}

typedef struct { const char *name; int code; } NameCode;
static const NameCode cond_names[] = {
  {"Equal", 4}, {"NotEqual", 5}, {"Above", 7}, {"AboveOrEqual", 3}, {"Below", 2},
  {"BelowOrEqual", 6}, {"GreaterThan", 15}, {"GreaterThanOrEqual", 13},
  {"LessThan", 12}, {"LessThanOrEqual", 14},
  {"Overflow", 0}, {"Signed", 8}, {"PositiveOrZero", 9}, {"Zero", 4}, {"NonZero", 5},
  {"DoubleEqual", 36}, {"DoubleNotEqual", 5}, {"DoubleGreaterThan", 7},
  {"DoubleGreaterThanOrEqual", 3}, {"DoubleLessThan", 23}, {"DoubleLessThanOrEqual", 19},
  {"DoubleEqualOrUnordered", 4}, {"DoubleNotEqualOrUnordered", 37},
  {"DoubleGreaterThanOrUnordered", 18}, {"DoubleGreaterThanOrEqualOrUnordered", 22},
  {"DoubleLessThanOrUnordered", 2}, {"DoubleLessThanOrEqualOrUnordered", 6}
};

/* ---- opcodes: enum for the opcodes the payloads use, with opcode.js hash numbers ---- */
enum {
  O_NOP, O_MOVE, O_MOVE32, O_MOVEDOUBLE, O_MOVEZEROTODOUBLE, O_MOVE64TODOUBLE,
  O_MOVEDOUBLETO64, O_PATCH, O_JUMP, O_BRANCHTEST64, O_BRANCHTEST32, O_BRANCH32,
  O_BRANCH64, O_BRANCHDOUBLE, O_OOPS, O_RET64, O_XOR64, O_XOR32, O_AND64, O_AND32,
  O_ADD32, O_ADD64, O_ADDDOUBLE, O_MULDOUBLE, O_DIVDOUBLE, O_CONVERTINT32TODOUBLE,
  O_MOVECONDITIONALLYTEST64, O_RSHIFT32, O_OR32, O_LOAD8, O_LOAD16, O_STORE8,
  O_ZEROEXTEND16TO32, O_COMPARE32, O_COUNT
};

static const int opcode_code[O_COUNT] = {
  92 /* Nop */, 58 /* Move */, 59 /* Move32 */, 68 /* MoveDouble */,
  78 /* MoveZeroToDouble */, 61 /* Move64ToDouble */, 75 /* MoveDoubleTo64 */,
  98 /* Patch */, 50 /* Jump */, 26 /* BranchTest64 */, 25 /* BranchTest32 */,
  12 /* Branch32 */, 13 /* Branch64 */, 17 /* BranchDouble */, 95 /* Oops */,
  100 /* Ret64 */, 129 /* Xor64 */, 128 /* Xor32 */, 9 /* And64 */, 8 /* And32 */,
  3 /* Add32 */, 4 /* Add64 */, 6 /* AddDouble */, 81 /* MulDouble */,
  46 /* DivDouble */, 38 /* ConvertInt32ToDouble */, 67 /* MoveConditionallyTest64 */,
  103 /* Rshift32 */, 96 /* Or32 */, 54 /* Load8 */, 52 /* Load16 */, 112 /* Store8 */,
  132 /* ZeroExtend16To32 */, 32 /* Compare32 */
};

static const NameCode opcode_names[] = {
  {"Move", O_MOVE}, {"Move32", O_MOVE32}, {"MoveDouble", O_MOVEDOUBLE},
  {"MoveZeroToDouble", O_MOVEZEROTODOUBLE}, {"Move64ToDouble", O_MOVE64TODOUBLE},
  {"MoveDoubleTo64", O_MOVEDOUBLETO64}, {"Patch", O_PATCH}, {"Jump", O_JUMP},
  {"BranchTest64", O_BRANCHTEST64}, {"BranchTest32", O_BRANCHTEST32},
  {"Branch32", O_BRANCH32}, {"Branch64", O_BRANCH64}, {"BranchDouble", O_BRANCHDOUBLE},
  {"Oops", O_OOPS}, {"Ret64", O_RET64}, {"Xor64", O_XOR64}, {"Xor32", O_XOR32},
  {"And64", O_AND64}, {"And32", O_AND32}, {"Add32", O_ADD32}, {"Add64", O_ADD64},
  {"AddDouble", O_ADDDOUBLE}, {"MulDouble", O_MULDOUBLE}, {"DivDouble", O_DIVDOUBLE},
  {"ConvertInt32ToDouble", O_CONVERTINT32TODOUBLE},
  {"MoveConditionallyTest64", O_MOVECONDITIONALLYTEST64}, {"Rshift32", O_RSHIFT32},
  {"Or32", O_OR32}, {"Load8", O_LOAD8}, {"Load16", O_LOAD16}, {"Store8", O_STORE8},
  {"ZeroExtend16To32", O_ZEROEXTEND16TO32}, {"Compare32", O_COMPARE32}
};

/* ---- forEachArg operand specs from opcode.js (Inst_forEachArg) ---- */
typedef struct { uint8_t role, type, width; } Spec;
typedef struct { int n; const Spec *s; } Overload;
typedef struct { int nov; Overload ov[2]; } OpSpec;

static const Spec s_move[] = {{RU, TG, 64}, {RD, TG, 64}};
static const Spec s_move32[] = {{RU, TG, 32}, {RZD, TG, 32}};
static const Spec s_movedouble[] = {{RU, TF, 64}, {RD, TF, 64}};
static const Spec s_movezerodouble[] = {{RD, TF, 64}};
static const Spec s_move64todouble[] = {{RU, TG, 64}, {RD, TF, 64}};
static const Spec s_movedoubleto64[] = {{RU, TF, 64}, {RD, TG, 64}};
static const Spec s_branch32[] = {{RU, TG, 32}, {RU, TG, 32}, {RU, TG, 32}};
static const Spec s_branch64[] = {{RU, TG, 32}, {RU, TG, 64}, {RU, TG, 64}};
static const Spec s_branchdouble[] = {{RU, TG, 32}, {RU, TF, 64}, {RU, TF, 64}};
static const Spec s_ret64[] = {{RU, TG, 64}};
static const Spec s_add32_3[] = {{RU, TG, 32}, {RU, TG, 32}, {RZD, TG, 32}};
static const Spec s_add32_2[] = {{RU, TG, 32}, {RUZD, TG, 32}};
static const Spec s_add64_3[] = {{RU, TG, 64}, {RU, TG, 64}, {RD, TG, 64}};
static const Spec s_add64_2[] = {{RU, TG, 64}, {RUD, TG, 64}};
static const Spec s_adddouble_3[] = {{RU, TF, 64}, {RU, TF, 64}, {RD, TF, 64}};
static const Spec s_adddouble_2[] = {{RU, TF, 64}, {RUD, TF, 64}};
static const Spec s_divdouble_3[] = {{RU, TF, 64}, {RU, TF, 32}, {RD, TF, 64}};
static const Spec s_convert[] = {{RU, TG, 32}, {RD, TF, 64}};
static const Spec s_mct64_5[] = {{RU, TG, 32}, {RU, TG, 64}, {RU, TG, 64}, {RU, TG, 64}, {RUD, TG, 64}};
static const Spec s_mct64_6[] = {{RU, TG, 32}, {RU, TG, 32}, {RU, TG, 32}, {RU, TG, 64}, {RU, TG, 64}, {RD, TG, 64}};
static const Spec s_load8[] = {{RU, TG, 8}, {RZD, TG, 32}};
static const Spec s_load16[] = {{RU, TG, 16}, {RZD, TG, 32}};
static const Spec s_store8[] = {{RU, TG, 8}, {RD, TG, 8}};
static const Spec s_zext16[] = {{RU, TG, 16}, {RZD, TG, 32}};
static const Spec s_compare32[] = {{RU, TG, 32}, {RU, TG, 32}, {RU, TG, 32}, {RZD, TG, 32}};

static const OpSpec opspecs[O_COUNT] = {
  [O_MOVE] = {1, {{2, s_move}}},
  [O_MOVE32] = {1, {{2, s_move32}}},
  [O_MOVEDOUBLE] = {1, {{2, s_movedouble}}},
  [O_MOVEZEROTODOUBLE] = {1, {{1, s_movezerodouble}}},
  [O_MOVE64TODOUBLE] = {1, {{2, s_move64todouble}}},
  [O_MOVEDOUBLETO64] = {1, {{2, s_movedoubleto64}}},
  [O_JUMP] = {1, {{0, NULL}}},
  [O_BRANCHTEST64] = {1, {{3, s_branch64}}},
  [O_BRANCHTEST32] = {1, {{3, s_branch32}}},
  [O_BRANCH32] = {1, {{3, s_branch32}}},
  [O_BRANCH64] = {1, {{3, s_branch64}}},
  [O_BRANCHDOUBLE] = {1, {{3, s_branchdouble}}},
  [O_OOPS] = {1, {{0, NULL}}},
  [O_RET64] = {1, {{1, s_ret64}}},
  [O_XOR64] = {2, {{3, s_add64_3}, {2, s_add64_2}}},
  [O_AND64] = {2, {{3, s_add64_3}, {2, s_add64_2}}},
  [O_ADD64] = {2, {{3, s_add64_3}, {2, s_add64_2}}},
  [O_XOR32] = {2, {{3, s_add32_3}, {2, s_add32_2}}},
  [O_AND32] = {2, {{3, s_add32_3}, {2, s_add32_2}}},
  [O_ADD32] = {2, {{3, s_add32_3}, {2, s_add32_2}}},
  [O_RSHIFT32] = {2, {{3, s_add32_3}, {2, s_add32_2}}},
  [O_OR32] = {2, {{3, s_add32_3}, {2, s_add32_2}}},
  [O_ADDDOUBLE] = {2, {{3, s_adddouble_3}, {2, s_adddouble_2}}},
  [O_MULDOUBLE] = {2, {{3, s_adddouble_3}, {2, s_adddouble_2}}},
  [O_DIVDOUBLE] = {2, {{3, s_divdouble_3}, {2, s_adddouble_2}}},
  [O_CONVERTINT32TODOUBLE] = {1, {{2, s_convert}}},
  [O_MOVECONDITIONALLYTEST64] = {2, {{5, s_mct64_5}, {6, s_mct64_6}}},
  [O_LOAD8] = {1, {{2, s_load8}}},
  [O_LOAD16] = {1, {{2, s_load16}}},
  [O_STORE8] = {1, {{2, s_store8}}},
  [O_ZEROEXTEND16TO32] = {1, {{2, s_zext16}}},
  [O_COMPARE32] = {1, {{4, s_compare32}}},
};

static int is_early_use(int r) { return r == RU || r == RCU || r == RUD || r == RUZD; }
static int is_late_use(int r) { return r == RLU || r == RLCU || r == RSC; }
static int is_early_def(int r) { return r == RED || r == RSC; }
static int is_late_def(int r) { return r == RD || r == RUD || r == RZD || r == RUZD; }
static int is_zdef(int r) { return r == RZD || r == RUZD; }

static int bytes_of(int width) { return width / 8; }

/* ---- Arg (arg.js) ---- */
typedef struct {
  uint8_t kind;
  uint8_t reg;     /* K_TMP register, K_ADDR/K_INDEX base */
  uint8_t index;   /* K_INDEX index register */
  uint8_t scale;   /* K_INDEX */
  uint8_t cond;    /* condition code for K_RELCOND/K_RESCOND/K_DOUBLECOND */
  int32_t slot;    /* K_STACK slot index */
  int64_t value;   /* K_IMM value, K_BIGIMM low word */
  int64_t high;    /* K_BIGIMM high word */
  int64_t offset;  /* K_ADDR/K_STACK/K_CALLARG/K_INDEX offset */
} Arg;

static uint32_t arg_hash(const Arg *a) {
  uint32_t r = (uint32_t)a->kind;
  switch (a->kind) {
  case K_INVALID:
  case K_SPECIAL:
    break;
  case K_TMP:
    r += reg_hash(a->reg);
    break;
  case K_IMM:
  case K_BITIMM:
    r += (uint32_t)a->value;
    break;
  case K_BIGIMM:
  case K_BITIMM64:
    r += (uint32_t)a->value;
    r += (uint32_t)a->high;
    break;
  case K_CALLARG:
    r += (uint32_t)a->offset;
    break;
  case K_RELCOND:
  case K_RESCOND:
  case K_DOUBLECOND:
    r += a->cond;
    break;
  case K_ADDR:
    r += (uint32_t)a->offset;
    r += reg_hash(a->reg);
    break;
  case K_INDEX:
    r += (uint32_t)a->offset;
    r += a->scale;
    r += reg_hash(a->reg);
    r += reg_hash(a->index);
    break;
  case K_STACK:
    r += (uint32_t)a->offset;
    r += (uint32_t)a->slot;
    break;
  default:
    fatal("Bad kind");
  }
  return r;
}

/* ---- ordered sets of stack slots (JS Set semantics: insertion order, add ignores
 * present members, delete keeps the order of the rest) ---- */
typedef struct {
  uint64_t mask;
  int n;
  uint8_t items[MAX_SLOTS];
} SlotSet;

static int ss_has(const SlotSet *s, int x) { return (int)((s->mask >> x) & 1); }

static void ss_add(SlotSet *s, int x) {
  if (ss_has(s, x)) return;
  s->items[s->n++] = (uint8_t)x;
  s->mask |= (uint64_t)1 << x;
}

static void ss_delete(SlotSet *s, int x) {
  if (!ss_has(s, x)) return;
  int i = 0;
  while (s->items[i] != x) i++;
  memmove(&s->items[i], &s->items[i + 1], (size_t)(s->n - i - 1));
  s->n--;
  s->mask &= ~((uint64_t)1 << x);
}

/* mergeIntoSet(target, source): true if anything was added. */
static int ss_merge(SlotSet *target, const SlotSet *source) {
  int did = 0;
  for (int i = 0; i < source->n; i++) {
    if (ss_has(target, source->items[i])) continue;
    ss_add(target, source->items[i]);
    did = 1;
  }
  return did;
}

/* ---- StackSlot (stack_slot.js) ---- */
typedef struct {
  int index;
  int size;
  int locked;      /* kind == Locked; otherwise Spill */
  int64_t offset;  /* offsetFromFP; 0 means unassigned, as in JS truthiness */
} Slot;

static int slot_alignment(const Slot *s) {
  if (s->size <= 1) return 1;
  if (s->size <= 2) return 2;
  if (s->size <= 4) return 4;
  return 8;
}

static uint32_t slot_hash(const Slot *s) {
  int64_t v = (s->locked ? 0 : 1) + (int64_t)s->size * 3 + (s->offset ? s->offset * 7 : 0);
  return (uint32_t)v;
}

/* ---- Inst, BasicBlock, Code ---- */
typedef struct { uint8_t role, type, width; } PatchArg;

typedef struct Inst {
  int opcode;
  Arg *args;
  size_t nargs, capargs;
  int patch_nonarg;          /* patchHasNonArgEffects */
  PatchArg *patch;           /* patchArgData */
  size_t npatch, cappatch;
  uint32_t clob_late;        /* extraClobberedRegs, bit per register id */
  uint32_t clob_early;       /* extraEarlyClobberedRegs */
} Inst;

typedef struct { struct Block *block; int freq; } Succ;

typedef struct Block {
  int index;
  int frequency;
  Inst **insts;
  size_t ninsts, capinsts;
  Succ *succs;
  size_t nsuccs, capsuccs;
  struct Block **preds;
  size_t npreds, cappreds;
} Block;

typedef struct {
  Block **blocks;
  size_t nblocks, capblocks;
  Slot slots[MAX_SLOTS];
  int nslots;
  int ntmps[2];
  int64_t call_arg_area_size;
  int64_t frame_size;
} Code;

static void *xmalloc(size_t n) {
  void *p = malloc(n ? n : 1);
  if (!p) fatal("out of memory");
  return p;
}

static void *xgrow(void *p, size_t *cap, size_t elt) {
  size_t nc = *cap ? *cap * 2 : 4;
  void *np = realloc(p, nc * elt);
  if (!np) fatal("out of memory");
  *cap = nc;
  return np;
}

static Inst *inst_new(int opcode) {
  Inst *i = xmalloc(sizeof *i);
  memset(i, 0, sizeof *i);
  i->opcode = opcode;
  return i;
}

static void inst_push_arg(Inst *i, const Arg *a) {
  if (i->nargs == i->capargs) i->args = xgrow(i->args, &i->capargs, sizeof *i->args);
  i->args[i->nargs++] = *a;
}

static void inst_push_patch(Inst *i, int role, int type, int width) {
  if (i->npatch == i->cappatch) i->patch = xgrow(i->patch, &i->cappatch, sizeof *i->patch);
  PatchArg pa = {(uint8_t)role, (uint8_t)type, (uint8_t)width};
  i->patch[i->npatch++] = pa;
}

static void inst_free(Inst *i) {
  free(i->args);
  free(i->patch);
  free(i);
}

static Block *block_new(Code *c, int frequency) {
  Block *b = xmalloc(sizeof *b);
  memset(b, 0, sizeof *b);
  b->index = (int)c->nblocks;
  b->frequency = frequency;
  if (c->nblocks == c->capblocks) c->blocks = xgrow(c->blocks, &c->capblocks, sizeof *c->blocks);
  c->blocks[c->nblocks++] = b;
  return b;
}

static void block_append(Block *b, Inst *i) {
  if (b->ninsts == b->capinsts) b->insts = xgrow(b->insts, &b->capinsts, sizeof *b->insts);
  b->insts[b->ninsts++] = i;
}

static void block_add_succ(Block *b, Block *target, int freq) {
  if (b->nsuccs == b->capsuccs) b->succs = xgrow(b->succs, &b->capsuccs, sizeof *b->succs);
  Succ s = {target, freq};
  b->succs[b->nsuccs++] = s;
}

static void block_add_pred(Block *b, Block *p) {
  if (b->npreds == b->cappreds) b->preds = xgrow(b->preds, &b->cappreds, sizeof *b->preds);
  b->preds[b->npreds++] = p;
}

static void code_free(Code *c) {
  for (size_t b = 0; b < c->nblocks; b++) {
    Block *blk = c->blocks[b];
    for (size_t i = 0; i < blk->ninsts; i++) inst_free(blk->insts[i]);
    free(blk->insts);
    free(blk->succs);
    free(blk->preds);
    free(blk);
  }
  free(c->blocks);
  free(c);
}

/* Code.hash() from code.js. */
static uint32_t code_hash(const Code *c) {
  uint32_t r = 0;
  for (size_t b = 0; b < c->nblocks; b++) {
    const Block *blk = c->blocks[b];
    r *= 1000001u;
    for (size_t i = 0; i < blk->ninsts; i++) {
      const Inst *in = blk->insts[i];
      r *= 97u;
      /* Inst.hash(): opcodeCode, then each arg hash */
      uint32_t h = (uint32_t)opcode_code[in->opcode];
      for (size_t a = 0; a < in->nargs; a++) h += arg_hash(&in->args[a]);
      r += h;
    }
    for (size_t s = 0; s < blk->nsuccs; s++) {
      r *= 7u;
      r += (uint32_t)blk->succs[s].block->index;
    }
  }
  for (int s = 0; s < c->nslots; s++) {
    r *= 101u;
    r += slot_hash(&c->slots[s]);
  }
  return r;
}

/* ---- operand enumeration: InstRef list in forEachArg order ---- */
typedef struct { uint8_t idx, role, type, width; } ArgRef;

/* Inst.forEachArg: the (index, role, type, width) of each operand the JS visits. */
static int arg_refs(const Inst *in, ArgRef *out) {
  int n = 0;
  if (in->opcode == O_NOP) return 0;
  if (in->opcode == O_PATCH) {
    if (in->npatch < in->nargs) fatal("Patch without patchArgData");
    if (in->nargs > MAX_REFS) fatal("too many operands");
    for (size_t i = 0; i < in->nargs; i++) {
      out[n].idx = (uint8_t)i;
      out[n].role = in->patch[i].role;
      out[n].type = in->patch[i].type;
      out[n].width = in->patch[i].width;
      n++;
    }
    return n;
  }
  const OpSpec *os = &opspecs[in->opcode];
  const Spec *s = NULL;
  int count = 0;
  if (os->nov == 1) {
    count = os->ov[0].n;
    s = os->ov[0].s;
    if ((int)in->nargs < count) fatal("Bad operand count");
  } else {
    for (int k = 0; k < os->nov; k++) {
      if (os->ov[k].n == (int)in->nargs) { s = os->ov[k].s; count = os->ov[k].n; break; }
    }
    if (!s) fatal("Bad overload");
  }
  for (int k = 0; k < count; k++) {
    out[n].idx = (uint8_t)k;
    out[n].role = s[k].role;
    out[n].type = s[k].type;
    out[n].width = s[k].width;
    n++;
  }
  return n;
}

static int inst_has_nonarg_effects(const Inst *in) {
  switch (in->opcode) {
  case O_BRANCH32: case O_BRANCH64: case O_BRANCHTEST64: case O_BRANCHTEST32:
  case O_BRANCHDOUBLE: case O_JUMP: case O_RET64: case O_OOPS:
    return 1;
  case O_PATCH:
    return in->patch_nonarg;
  default:
    return 0;
  }
}

static int stack_slot_of(const Inst *in, const ArgRef *r) {
  const Arg *a = &in->args[r->idx];
  return a->kind == K_STACK ? a->slot : -1;
}

/* ---- allocateStack (allocate_stack.js) ---- */
static int64_t round_up(int64_t amount, int64_t value) {
  return ((value + amount - 1) / amount) * amount;
}

static int has_off(const Slot *s) { return s->offset != 0; }

static int attempt_assignment(Code *c, int si, int64_t offset, const int *others, int nothers) {
  Slot *slot = &c->slots[si];
  if (offset > 0) fatal("Expect negative offset");
  offset = -round_up(slot_alignment(slot), -offset);
  for (int k = 0; k < nothers; k++) {
    const Slot *o = &c->slots[others[k]];
    if (!has_off(o)) continue;
    int64_t lmin = offset, lmax = offset + slot->size;
    int64_t rmin = o->offset, rmax = o->offset + o->size;
    if (lmin == lmax || rmin == rmax) continue;   /* rangesOverlap */
    if ((lmin <= rmin && lmax > rmin) || (rmin <= lmin && rmax > lmin)) return 0;
  }
  slot->offset = offset;
  return 1;
}

static void assign_slot(Code *c, int si, const int *others, int nothers) {
  Slot *slot = &c->slots[si];
  if (attempt_assignment(c, si, -(int64_t)slot->size, others, nothers)) return;
  for (int k = 0; k < nothers; k++) {
    const Slot *o = &c->slots[others[k]];
    if (!has_off(o)) continue;
    if (attempt_assignment(c, si, o->offset - slot->size, others, nothers)) return;
  }
  fatal("Assignment failed");
}

static void local_execute(const Block *blk, size_t ii, SlotSet *live) {
  ArgRef refs[MAX_REFS];
  int n;
  if (ii + 1 < blk->ninsts) {
    const Inst *nx = blk->insts[ii + 1];
    n = arg_refs(nx, refs);
    for (int k = 0; k < n; k++) {
      int s = stack_slot_of(nx, &refs[k]);
      if (s >= 0 && is_early_def(refs[k].role)) ss_delete(live, s);
    }
  }
  const Inst *in = blk->insts[ii];
  n = arg_refs(in, refs);
  for (int k = 0; k < n; k++) {
    int s = stack_slot_of(in, &refs[k]);
    if (s >= 0 && is_late_def(refs[k].role)) ss_delete(live, s);
  }
  for (int k = 0; k < n; k++) {
    int s = stack_slot_of(in, &refs[k]);
    if (s >= 0 && is_early_use(refs[k].role)) ss_add(live, s);
  }
  if (ii >= 1) {
    const Inst *pv = blk->insts[ii - 1];
    n = arg_refs(pv, refs);
    for (int k = 0; k < n; k++) {
      int s = stack_slot_of(pv, &refs[k]);
      if (s >= 0 && is_late_use(refs[k].role)) ss_add(live, s);
    }
  }
}

/* Inst.forEachDef(StackSlot, prev, next, ...) with the interfere callback. */
static void interfere(const Code *c, const Block *blk, long ii, const SlotSet *live, SlotSet *interf) {
  const Inst *prev = (ii >= 0 && (size_t)ii < blk->ninsts) ? blk->insts[ii] : NULL;
  const Inst *next = (ii + 1 >= 0 && (size_t)(ii + 1) < blk->ninsts) ? blk->insts[ii + 1] : NULL;
  ArgRef refs[MAX_REFS];
  int n;
  for (int pass = 0; pass < 2; pass++) {
    const Inst *in = pass == 0 ? prev : next;
    if (!in) continue;
    n = arg_refs(in, refs);
    for (int k = 0; k < n; k++) {
      int s = stack_slot_of(in, &refs[k]);
      if (s < 0) continue;
      int isdef = pass == 0 ? is_late_def(refs[k].role) : is_early_def(refs[k].role);
      if (!isdef) continue;
      if (c->slots[s].locked) continue;          /* only spill slots interfere */
      for (int j = 0; j < live->n; j++) {
        int other = live->items[j];
        ss_add(&interf[s], other);
        ss_add(&interf[other], s);
      }
    }
  }
}

static void allocate_stack(Code *code) {
  if (code->frame_size) fatal("Frame size already determined");
  int esc[MAX_SLOTS], nesc = 0;
  int work[MAX_SLOTS], nwork = 0;
  for (int i = 0; i < code->nslots; i++) {
    Slot *s = &code->slots[i];
    if (s->locked) {
      if (has_off(s)) esc[nesc++] = i;
      else work[nwork++] = i;
    } else if (has_off(s)) {
      fatal("Offset already assigned");
    }
  }
  while (nwork) {
    int si = work[--nwork];
    assign_slot(code, si, esc, nesc);
    esc[nesc++] = si;
  }

  /* Liveness (liveness.js) over stack slots: fixed point over blocks. */
  size_t nb = code->nblocks;
  SlotSet *head = calloc(nb, sizeof *head);
  SlotSet *tail = calloc(nb, sizeof *tail);
  uint8_t *dirty = xmalloc(nb);
  if (!head || !tail) fatal("out of memory");
  ArgRef refs[MAX_REFS];
  for (size_t b = 0; b < nb; b++) {
    const Block *blk = code->blocks[b];
    if (!blk->ninsts) fatal("empty block");
    const Inst *last = blk->insts[blk->ninsts - 1];
    int n = arg_refs(last, refs);
    for (int k = 0; k < n; k++) {
      int s = stack_slot_of(last, &refs[k]);
      if (s >= 0 && is_late_use(refs[k].role)) ss_add(&tail[b], s);
    }
    dirty[b] = 1;
  }
  int changed;
  do {
    changed = 0;
    for (size_t bi = nb; bi-- > 0;) {
      if (!dirty[bi]) continue;
      dirty[bi] = 0;
      const Block *blk = code->blocks[bi];
      SlotSet live = tail[bi];
      for (size_t ii = blk->ninsts; ii-- > 0;) local_execute(blk, ii, &live);
      /* block.at(0): an early-def stack operand would call liveSet.remove, which JS lacks. */
      int n = arg_refs(blk->insts[0], refs);
      for (int k = 0; k < n; k++) {
        if (stack_slot_of(blk->insts[0], &refs[k]) >= 0 && is_early_def(refs[k].role))
          fatal("unsupported: early-def stack operand at block head");
      }
      if (!ss_merge(&head[bi], &live)) continue;
      for (size_t p = 0; p < blk->npreds; p++) {
        size_t pi = (size_t)blk->preds[p]->index;
        if (ss_merge(&tail[pi], &head[bi])) {
          dirty[pi] = 1;
          changed = 1;
        }
      }
    }
  } while (changed);

  /* Dead spill-store elimination and interference (allocate_stack.js). */
  SlotSet *interf = calloc((size_t)code->nslots + 1, sizeof *interf);
  if (!interf) fatal("out of memory");
  for (size_t bi = 0; bi < nb; bi++) {
    Block *blk = code->blocks[bi];
    SlotSet live = tail[bi];
    for (size_t ii = blk->ninsts; ii-- > 0;) {
      Inst *in = blk->insts[ii];
      if (!inst_has_nonarg_effects(in)) {
        int ok = 1;
        int n = arg_refs(in, refs);
        for (int k = 0; k < n; k++) {
          const Arg *a = &in->args[refs[k].idx];
          if (is_early_def(refs[k].role)) { ok = 0; continue; }
          if (!is_late_def(refs[k].role)) continue;
          if (a->kind != K_STACK) { ok = 0; continue; }
          if (code->slots[a->slot].locked) { ok = 0; continue; }    /* !isSpill */
          if (ss_has(&live, a->slot)) { ok = 0; continue; }
        }
        if (ok) {
          in->opcode = O_NOP;
          in->nargs = 0;
        }
      }
      interfere(code, blk, (long)ii, &live, interf);
      local_execute(blk, ii, &live);
    }
    interfere(code, blk, -1, &live, interf);
    size_t dst = 0;
    for (size_t i = 0; i < blk->ninsts; i++) {
      if (blk->insts[i]->opcode == O_NOP) {
        inst_free(blk->insts[i]);
      } else {
        blk->insts[dst++] = blk->insts[i];
      }
    }
    blk->ninsts = dst;
  }

  /* Assign the remaining slots. */
  for (int i = 0; i < code->nslots; i++) {
    if (has_off(&code->slots[i])) continue;
    int others[2 * MAX_SLOTS], no = 0;
    for (int k = 0; k < nesc; k++) others[no++] = esc[k];
    for (int k = 0; k < interf[i].n; k++) others[no++] = interf[i].items[k];
    assign_slot(code, i, others, no);
  }
  free(interf);
  free(head);
  free(tail);
  free(dirty);

  int64_t frame_for_slots = 0;
  for (int i = 0; i < code->nslots; i++) {
    int64_t v = -code->slots[i].offset;
    if (v > frame_for_slots) frame_for_slots = v;
  }
  frame_for_slots = round_up(16, frame_for_slots);

  for (size_t bi = 0; bi < code->nblocks; bi++) {
    Block *blk = code->blocks[bi];
    for (size_t ii = 0; ii < blk->ninsts; ii++) {
      Inst *in = blk->insts[ii];
      for (size_t a = 0; a < in->nargs; a++) {
        if (in->args[a].kind != K_CALLARG) continue;
        if (in->args[a].offset < 0) fatal("Did not expect negative offset for callArg");
        int64_t req = round_up(16, in->args[a].offset + 8);
        if (req > code->call_arg_area_size) code->call_arg_area_size = req;
      }
    }
  }
  code->frame_size = frame_for_slots + code->call_arg_area_size;

  /* Rewrite Stack and CallArg operands to rbp-relative addresses. */
  for (size_t bi = 0; bi < code->nblocks; bi++) {
    Block *blk = code->blocks[bi];
    for (size_t ii = 0; ii < blk->ninsts; ii++) {
      Inst *in = blk->insts[ii];
      int n = arg_refs(in, refs);
      for (int k = 0; k < n; k++) {
        Arg *a = &in->args[refs[k].idx];
        Arg repl;
        int have = 0;
        memset(&repl, 0, sizeof repl);
        if (a->kind == K_STACK) {
          const Slot *s = &code->slots[a->slot];
          if (is_zdef(refs[k].role) && !s->locked && s->size > bytes_of(refs[k].width))
            fatal("unsupported: ZDef spill operand (insertion path)");
          repl.kind = K_ADDR;
          repl.reg = REG_RBP;
          repl.offset = a->offset + s->offset;
          have = 1;
        } else if (a->kind == K_CALLARG) {
          repl.kind = K_ADDR;
          repl.reg = REG_RBP;
          repl.offset = a->offset - code->frame_size;
          have = 1;
        }
        if (have) *a = repl;
      }
    }
  }
}

/* ---- payload statements (parsed from the JS payload files) ---- */
typedef enum { OP_BLOCK, OP_SLOT, OP_TMP, OP_SLOTOFF, OP_SUCC, OP_PRED, OP_INST, OP_ARG,
               OP_APPEND, OP_PATCHNONARG, OP_PATCHDATA, OP_CLOB, OP_ECLOB } OpKind;

typedef struct {
  uint8_t kind;
  uint8_t flag;
  int32_t a, b, c;
  int64_t v;
  Arg arg;
} Op;

typedef struct {
  Op *ops;
  size_t nops, capops;
} Payload;

typedef struct { char name[32]; int is_slot; int index; } Sym;

typedef struct {
  Payload *p;
  Sym syms[512];
  int nsyms;
  int nblocks, nslots;
  Arg pending;
} Parser;

typedef struct { const char *p; const char *file; int line; } Lex;

static void lex_die(const Lex *l, const char *what) {
  fprintf(stderr, "Error: %s:%d: unexpected text near \"%.40s\" (expected %s)\n", l->file, l->line, l->p, what);
  exit(1);
}

static void lex_ws(Lex *l) {
  while (*l->p == ' ' || *l->p == '\t' || *l->p == '\r') l->p++;
}

static int lex_try(Lex *l, const char *lit) {
  lex_ws(l);
  size_t n = strlen(lit);
  if (strncmp(l->p, lit, n) == 0) {
    l->p += n;
    return 1;
  }
  return 0;
}

static void lex_expect(Lex *l, const char *lit) {
  if (!lex_try(l, lit)) lex_die(l, lit);
}

static void lex_ident(Lex *l, char *buf, size_t cap) {
  lex_ws(l);
  size_t n = 0;
  while (isalnum((unsigned char)*l->p) || *l->p == '_') {
    if (n + 1 >= cap) lex_die(l, "short identifier");
    buf[n++] = *l->p++;
  }
  if (n == 0) lex_die(l, "identifier");
  buf[n] = 0;
}

static int64_t lex_int(Lex *l) {
  lex_ws(l);
  int neg = 0;
  if (*l->p == '-') { neg = 1; l->p++; }
  if (!isdigit((unsigned char)*l->p)) lex_die(l, "integer");
  int64_t v = 0;
  while (isdigit((unsigned char)*l->p)) v = v * 10 + (*l->p++ - '0');
  return neg ? -v : v;
}

static int lookup_name(const NameCode *tab, size_t n, const char *name) {
  for (size_t i = 0; i < n; i++)
    if (strcmp(tab[i].name, name) == 0) return tab[i].code;
  return -1;
}

static int reg_of_name(const char *name) {
  for (int i = 0; i < 32; i++)
    if (strcmp(reg_names[i], name) == 0) return i;
  return -1;
}

static Op *emit(Parser *ps, OpKind kind) {
  Payload *p = ps->p;
  if (p->nops == p->capops) p->ops = xgrow(p->ops, &p->capops, sizeof *p->ops);
  Op *op = &p->ops[p->nops++];
  memset(op, 0, sizeof *op);
  op->kind = (uint8_t)kind;
  return op;
}

static void add_sym(Parser *ps, const char *name, int is_slot, int index) {
  if (ps->nsyms >= 512) fatal("too many names");
  Sym *s = &ps->syms[ps->nsyms++];
  snprintf(s->name, sizeof s->name, "%s", name);
  s->is_slot = is_slot;
  s->index = index;
}

static Sym *find_sym(Parser *ps, const char *name) {
  for (int i = 0; i < ps->nsyms; i++)
    if (strcmp(ps->syms[i].name, name) == 0) return &ps->syms[i];
  fatal("payload refers to an unknown name");
  return NULL;
}

static int ident_block(Parser *ps, Lex *l) {
  char name[32];
  lex_ident(l, name, sizeof name);
  Sym *s = find_sym(ps, name);
  if (s->is_slot) lex_die(l, "block name");
  return s->index;
}

static void parse_arg(Parser *ps, Lex *l) {
  char kind[32], name[32];
  Arg a;
  memset(&a, 0, sizeof a);
  lex_ident(l, kind, sizeof kind);
  lex_expect(l, "(");
  if (strcmp(kind, "createTmp") == 0) {
    lex_expect(l, "Reg.");
    lex_ident(l, name, sizeof name);
    int r = reg_of_name(name);
    if (r < 0) lex_die(l, "register");
    a.kind = K_TMP;
    a.reg = (uint8_t)r;
  } else if (strcmp(kind, "createImm") == 0) {
    a.kind = K_IMM;
    a.value = lex_int(l);
  } else if (strcmp(kind, "createBigImm") == 0) {
    a.kind = K_BIGIMM;
    a.value = lex_int(l);
    lex_expect(l, ",");
    a.high = lex_int(l);
  } else if (strcmp(kind, "createAddr") == 0) {
    lex_expect(l, "Reg.");
    lex_ident(l, name, sizeof name);
    int r = reg_of_name(name);
    if (r < 0) lex_die(l, "register");
    lex_expect(l, ",");
    a.kind = K_ADDR;
    a.reg = (uint8_t)r;
    a.offset = lex_int(l);
  } else if (strcmp(kind, "createStack") == 0) {
    lex_ident(l, name, sizeof name);
    Sym *s = find_sym(ps, name);
    if (!s->is_slot) lex_die(l, "slot name");
    lex_expect(l, ",");
    a.kind = K_STACK;
    a.slot = s->index;
    a.offset = lex_int(l);
  } else if (strcmp(kind, "createCallArg") == 0) {
    a.kind = K_CALLARG;
    a.offset = lex_int(l);
  } else if (strcmp(kind, "createResCond") == 0 || strcmp(kind, "createRelCond") == 0 ||
             strcmp(kind, "createDoubleCond") == 0) {
    lex_ident(l, name, sizeof name);
    int code = lookup_name(cond_names, sizeof cond_names / sizeof cond_names[0], name);
    if (code < 0) lex_die(l, "condition");
    a.kind = strcmp(kind, "createResCond") == 0 ? K_RESCOND
           : strcmp(kind, "createRelCond") == 0 ? K_RELCOND : K_DOUBLECOND;
    a.cond = (uint8_t)code;
  } else if (strcmp(kind, "createIndex") == 0) {
    lex_expect(l, "Reg.");
    lex_ident(l, name, sizeof name);
    int base = reg_of_name(name);
    lex_expect(l, ",");
    lex_expect(l, "Reg.");
    lex_ident(l, name, sizeof name);
    int idx = reg_of_name(name);
    if (base < 0 || idx < 0) lex_die(l, "register");
    lex_expect(l, ",");
    a.kind = K_INDEX;
    a.reg = (uint8_t)base;
    a.index = (uint8_t)idx;
    a.scale = (uint8_t)lex_int(l);
    lex_expect(l, ",");
    a.offset = lex_int(l);
  } else if (strcmp(kind, "createSpecial") == 0) {
    a.kind = K_SPECIAL;
  } else {
    lex_die(l, "Arg constructor");
  }
  lex_expect(l, ")");
  lex_expect(l, ";");
  ps->pending = a;
}

static void parse_statement(Parser *ps, Lex *l) {
  lex_ws(l);
  if (*l->p == 0 || (l->p[0] == '/' && l->p[1] == '/')) return;
  if (lex_try(l, "\"use strict\"") || lex_try(l, "function") || lex_try(l, "return") ||
      lex_try(l, "{") || lex_try(l, "}"))
    return;
  char name[32];
  lex_ident(l, name, sizeof name);
  if (strcmp(name, "inst") == 0) {
    if (lex_try(l, "=")) {
      lex_expect(l, "new Inst(");
      char op[32];
      lex_ident(l, op, sizeof op);
      int code = lookup_name(opcode_names, sizeof opcode_names / sizeof opcode_names[0], op);
      if (code < 0) lex_die(l, "opcode");
      lex_expect(l, ");");
      emit(ps, OP_INST)->a = code;
      return;
    }
    lex_expect(l, ".");
    char member[32];
    lex_ident(l, member, sizeof member);
    if (strcmp(member, "args") == 0) {
      lex_expect(l, ".push(arg);");
      Op *op = emit(ps, OP_ARG);
      op->arg = ps->pending;
      return;
    }
    if (strcmp(member, "patchHasNonArgEffects") == 0) {
      lex_expect(l, "=");
      char v[8];
      lex_ident(l, v, sizeof v);
      lex_expect(l, ";");
      emit(ps, OP_PATCHNONARG)->flag = strcmp(v, "true") == 0;
      return;
    }
    if (strcmp(member, "patchArgData") == 0) {
      if (lex_try(l, "=")) {
        lex_expect(l, "[];");
        return;
      }
      lex_expect(l, ".push({role: Arg.");
      char role[32], type[8];
      lex_ident(l, role, sizeof role);
      lex_expect(l, ", type: ");
      lex_ident(l, type, sizeof type);
      lex_expect(l, ", width: ");
      int64_t width = lex_int(l);
      lex_expect(l, "});");
      static const NameCode role_names[] = {
        {"Use", RU}, {"ColdUse", RCU}, {"LateUse", RLU}, {"LateColdUse", RLCU}, {"Def", RD},
        {"ZDef", RZD}, {"UseDef", RUD}, {"UseZDef", RUZD}, {"EarlyDef", RED},
        {"Scratch", RSC}, {"UseAddr", RUA}
      };
      int r = lookup_name(role_names, sizeof role_names / sizeof role_names[0], role);
      if (r < 0) lex_die(l, "role");
      Op *op = emit(ps, OP_PATCHDATA);
      op->a = r;
      op->b = strcmp(type, "GP") == 0 ? TG : TF;
      op->c = (int32_t)width;
      return;
    }
    if (strcmp(member, "extraClobberedRegs") == 0 || strcmp(member, "extraEarlyClobberedRegs") == 0) {
      int early = strcmp(member, "extraEarlyClobberedRegs") == 0;
      if (lex_try(l, "=")) {
        lex_expect(l, "new Set();");
        return;
      }
      lex_expect(l, ".add(Reg.");
      lex_ident(l, name, sizeof name);
      lex_expect(l, ");");
      int r = reg_of_name(name);
      if (r < 0) lex_die(l, "register");
      emit(ps, early ? OP_ECLOB : OP_CLOB)->a = r;
      return;
    }
    lex_die(l, "inst member");
  }
  if (strcmp(name, "arg") == 0) {
    lex_expect(l, "=");
    lex_expect(l, "Arg.");
    parse_arg(ps, l);
    return;
  }
  Sym *sym = find_sym(ps, name);
  if (sym->is_slot) {
    lex_expect(l, ".setOffsetFromFP(");
    int64_t off = lex_int(l);
    lex_expect(l, ");");
    Op *op = emit(ps, OP_SLOTOFF);
    op->a = sym->index;
    op->v = off;
    return;
  }
  if (lex_try(l, ".successors.push(new FrequentedBlock(")) {
    int target = ident_block(ps, l);
    lex_expect(l, ",");
    char freq[16];
    lex_ident(l, freq, sizeof freq);
    lex_expect(l, "));");
    Op *op = emit(ps, OP_SUCC);
    op->a = sym->index;
    op->b = target;
    op->c = strcmp(freq, "Rare") == 0;
    return;
  }
  if (lex_try(l, ".predecessors.push(")) {
    int target = ident_block(ps, l);
    lex_expect(l, ");");
    Op *op = emit(ps, OP_PRED);
    op->a = sym->index;
    op->b = target;
    return;
  }
  if (lex_try(l, ".append(inst);")) {
    emit(ps, OP_APPEND)->a = sym->index;
    return;
  }
  lex_die(l, "statement");
}

/* Parses a slot declaration after "code.addStackSlot(": size, kind. */
static void parse_slot_decl(Parser *ps, Lex *l, const char *name) {
  int64_t size = lex_int(l);
  lex_expect(l, ",");
  char kind[16];
  lex_ident(l, kind, sizeof kind);
  lex_expect(l, ");");
  if (ps->nslots >= MAX_SLOTS) fatal("too many stack slots");
  Op *op = emit(ps, OP_SLOT);
  op->a = (int32_t)size;
  op->flag = strcmp(kind, "Locked") == 0;
  add_sym(ps, name, 1, ps->nslots++);
}

static void parse_line(Parser *ps, Lex *l) {
  lex_ws(l);
  if (*l->p == 0 || (l->p[0] == '/' && l->p[1] == '/')) return;
  if (lex_try(l, "let ")) {
    char name[32];
    lex_ident(l, name, sizeof name);
    if (strcmp(name, "code") == 0 || strcmp(name, "inst") == 0 || strcmp(name, "arg") == 0) return;
    lex_expect(l, "=");
    if (lex_try(l, "code.addBlock();")) {
      emit(ps, OP_BLOCK)->a = 1;
      add_sym(ps, name, 0, ps->nblocks++);
      return;
    }
    if (lex_try(l, "code.addStackSlot(")) {
      parse_slot_decl(ps, l, name);
      return;
    }
    if (lex_try(l, "code.newTmp(")) {
      char type[8];
      lex_ident(l, type, sizeof type);
      lex_expect(l, ");");
      emit(ps, OP_TMP)->a = strcmp(type, "FP") == 0 ? TF : TG;
      return;
    }
    lex_die(l, "payload declaration");
  }
  parse_statement(ps, l);
}

static void load_payload(const char *path, Payload *p) {
  FILE *f = fopen(path, "rb");
  if (!f) {
    fprintf(stderr, "Error: cannot open %s\n", path);
    exit(1);
  }
  fseek(f, 0, SEEK_END);
  long n = ftell(f);
  fseek(f, 0, SEEK_SET);
  char *text = xmalloc((size_t)n + 1);
  if (fread(text, 1, (size_t)n, f) != (size_t)n) fatal("short read");
  text[n] = 0;
  fclose(f);

  Parser ps;
  memset(&ps, 0, sizeof ps);
  ps.p = p;
  int line = 0;
  char *s = text;
  while (*s) {
    char *e = strchr(s, '\n');
    if (e) *e = 0;
    line++;
    Lex l = {s, path, line};
    parse_line(&ps, &l);
    if (!e) break;
    s = e + 1;
  }
  free(text);
}

static Code *generate(const Payload *p) {
  Code *c = xmalloc(sizeof *c);
  memset(c, 0, sizeof *c);
  Inst *cur = NULL;
  for (size_t i = 0; i < p->nops; i++) {
    const Op *op = &p->ops[i];
    switch (op->kind) {
    case OP_BLOCK:
      block_new(c, op->a);
      break;
    case OP_SLOT: {
      Slot *s = &c->slots[c->nslots];
      s->index = c->nslots;
      s->size = op->a;
      s->locked = op->flag;
      s->offset = 0;
      c->nslots++;
      break;
    }
    case OP_TMP:
      c->ntmps[op->a]++;
      break;
    case OP_SLOTOFF:
      c->slots[op->a].offset = op->v;
      break;
    case OP_SUCC:
      block_add_succ(c->blocks[op->a], c->blocks[op->b], op->c);
      break;
    case OP_PRED:
      block_add_pred(c->blocks[op->a], c->blocks[op->b]);
      break;
    case OP_INST:
      cur = inst_new(op->a);
      break;
    case OP_ARG:
      inst_push_arg(cur, &op->arg);
      break;
    case OP_APPEND:
      block_append(c->blocks[op->a], cur);
      break;
    case OP_PATCHNONARG:
      cur->patch_nonarg = op->flag;
      break;
    case OP_PATCHDATA:
      inst_push_patch(cur, op->a, op->b, op->c);
      break;
    case OP_CLOB:
      cur->clob_late |= (uint32_t)1 << op->a;
      break;
    case OP_ECLOB:
      cur->clob_early |= (uint32_t)1 << op->a;
      break;
    }
  }
  return c;
}

/* ---- benchmark ---- */
typedef struct { const char *file; uint32_t early, late; } PayloadSpec;
static const PayloadSpec specs[NUM_PAYLOADS] = {
  {"ARES-6/Air/payload-gbemu-executeIteration.js", 632653144u, 372715518u},
  {"ARES-6/Air/payload-imaging-gaussian-blur-gaussianBlur.js", 3677819581u, 1252116304u},
  {"ARES-6/Air/payload-typescript-scanIdentifier.js", 1914852601u, 837339551u},
  {"ARES-6/Air/payload-airjs-ACLj8C.js", 1373599940u, 3981283600u},
};

static double now_ms(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return t.tv_sec * 1e3 + t.tv_nsec / 1e6;
}

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : DEFAULT_ITERATIONS;
  const char *root = argc > 2 ? argv[2] : "quench-bench/jetstream";
  static Payload payloads[NUM_PAYLOADS];
  for (int p = 0; p < NUM_PAYLOADS; p++) {
    char path[1024];
    snprintf(path, sizeof path, "%s/%s", root, specs[p].file);
    load_payload(path, &payloads[p]);
  }

  double start = now_ms();
  for (int it = 0; it < iterations; it++) {
    for (int p = 0; p < NUM_PAYLOADS; p++) {
      Code *code = generate(&payloads[p]);
      uint32_t hash = code_hash(code);
      if (hash != specs[p].early) {
        fprintf(stderr, "Error: wrong early hash for payload %d: %u\n", p, (unsigned)hash);
        return 1;
      }
      allocate_stack(code);
      hash = code_hash(code);
      if (hash != specs[p].late) {
        fprintf(stderr, "Error: wrong late hash for payload %d: %u\n", p, (unsigned)hash);
        return 1;
      }
      code_free(code);
    }
  }
  double elapsed = now_ms() - start;
  printf("Air %d iterations %.3f ms\n", iterations, elapsed);
  printf("validated: %d payloads, early and late hashes match benchmark.js\n", NUM_PAYLOADS);
  return 0;
}
