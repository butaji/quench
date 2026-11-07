// C analogue of JetStream 3's deltablue.js (DeltaBlue incremental constraint solver): the
// same object model, quirks included (BinaryConstraint.chooseMethod's non-exclusive ifs,
// Strength.nextWeaker's mapping, removeFirst popping the last element), the same chain and
// projection tests with their checks, and 20 deltaBlue() runs per iteration. Constraints are
// tagged structs; each deltaBlue() allocates from an arena it resets, as a C program frees a
// test's objects at once where the JS collector reclaims them.
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

enum { REQUIRED = 0, STRONG_PREFERRED = 1, PREFERRED = 2, STRONG_DEFAULT = 3, NORMAL = 4, WEAK_DEFAULT = 5, WEAKEST = 6 };
enum { NONE = 0, FORWARD = 1, BACKWARD = -1 };
typedef enum { STAY, EDIT, EQUALITY, SCALE } Kind;

static void fail(const char *message) { fprintf(stderr, "%s\n", message); exit(1); }

// Arena: one block per deltaBlue() run, reset after it.
static char *arena; static size_t arena_used, arena_size;
static void *alloc(size_t bytes) {
  bytes = (bytes + 15) & ~(size_t)15;
  if (arena_used + bytes > arena_size) fail("arena exhausted");
  void *p = arena + arena_used; arena_used += bytes; return p;
}

typedef struct { void **elms; int size, capacity; } Collection;
static Collection *collection(void) { Collection *c = alloc(sizeof *c); c->elms = NULL; c->size = c->capacity = 0; return c; }
static void add(Collection *c, void *elm) {
  if (c->size == c->capacity) {
    int capacity = c->capacity ? c->capacity * 2 : 4;
    void **elms = alloc(sizeof(void *) * capacity);
    if (c->size) memcpy(elms, c->elms, sizeof(void *) * c->size);
    c->elms = elms; c->capacity = capacity;
  }
  c->elms[c->size++] = elm;
}
static void *removeFirst(Collection *c) { return c->elms[--c->size]; }
static void removeElm(Collection *c, void *elm) {
  int index = 0;
  for (int i = 0; i < c->size; i++) if (c->elms[i] != elm) c->elms[index++] = c->elms[i];
  c->size = index;
}

typedef struct Constraint Constraint;
typedef struct Variable {
  double value; Collection *constraints; Constraint *determinedBy;
  int mark, walkStrength, stay; char name[16];
} Variable;
struct Constraint {
  Kind kind; int strength;
  Variable *myOutput; int satisfied;        // unary
  Variable *v1, *v2; int direction;         // binary
  Variable *scale, *offset;                 // scale
};

static int currentMark;
static int stronger(int s1, int s2) { return s1 < s2; }
static int weaker(int s1, int s2) { return s1 > s2; }
static int weakestOf(int s1, int s2) { return weaker(s1, s2) ? s1 : s2; }
static int nextWeaker(int s) {
  switch (s) {
    case 0: return WEAKEST; case 1: return WEAK_DEFAULT; case 2: return NORMAL;
    case 3: return STRONG_DEFAULT; case 4: return PREFERRED; case 5: return REQUIRED;
  }
  return -1;
}

static Variable *variable(const char *prefix, int index, double initial) {
  Variable *v = alloc(sizeof *v);
  v->value = initial; v->constraints = collection(); v->determinedBy = NULL;
  v->mark = 0; v->walkStrength = WEAKEST; v->stay = 1;
  snprintf(v->name, sizeof v->name, "%s%d", prefix, index);
  return v;
}
static void removeConstraint(Variable *v, Constraint *c) {
  removeElm(v->constraints, c);
  if (v->determinedBy == c) v->determinedBy = NULL;
}

static int isUnary(Constraint *c) { return c->kind == STAY || c->kind == EDIT; }
static int isInput(Constraint *c) { return c->kind == EDIT; }
static int isSatisfied(Constraint *c) { return isUnary(c) ? c->satisfied : c->direction != NONE; }
static Variable *input(Constraint *c) { return c->direction == FORWARD ? c->v1 : c->v2; }
static Variable *output(Constraint *c) { return isUnary(c) ? c->myOutput : (c->direction == FORWARD ? c->v2 : c->v1); }
static void markUnsatisfied(Constraint *c) { if (isUnary(c)) c->satisfied = 0; else c->direction = NONE; }

static void execute(Constraint *c) {
  if (c->kind == EQUALITY) output(c)->value = input(c)->value;
  else if (c->kind == SCALE) {
    if (c->direction == FORWARD) c->v2->value = c->v1->value * c->scale->value + c->offset->value;
    else c->v1->value = (c->v2->value - c->offset->value) / c->scale->value;
  }
}
static void addToGraph(Constraint *c) {
  if (isUnary(c)) { add(c->myOutput->constraints, c); c->satisfied = 0; return; }
  add(c->v1->constraints, c); add(c->v2->constraints, c); c->direction = NONE;
  if (c->kind == SCALE) { add(c->scale->constraints, c); add(c->offset->constraints, c); }
}
static void removeFromGraph(Constraint *c) {
  if (isUnary(c)) { if (c->myOutput) removeConstraint(c->myOutput, c); c->satisfied = 0; return; }
  if (c->v1) removeConstraint(c->v1, c);
  if (c->v2) removeConstraint(c->v2, c);
  c->direction = NONE;
  if (c->kind == SCALE) {
    if (c->scale) removeConstraint(c->scale, c);
    if (c->offset) removeConstraint(c->offset, c);
  }
}
static void chooseMethod(Constraint *c, int mark) {
  if (isUnary(c)) {
    c->satisfied = (c->myOutput->mark != mark) && stronger(c->strength, c->myOutput->walkStrength);
    return;
  }
  if (c->v1->mark == mark)
    c->direction = (c->v2->mark != mark && stronger(c->strength, c->v2->walkStrength)) ? FORWARD : NONE;
  if (c->v2->mark == mark)
    c->direction = (c->v1->mark != mark && stronger(c->strength, c->v1->walkStrength)) ? BACKWARD : NONE;
  if (weaker(c->v1->walkStrength, c->v2->walkStrength))
    c->direction = stronger(c->strength, c->v1->walkStrength) ? BACKWARD : NONE;
  else
    c->direction = stronger(c->strength, c->v2->walkStrength) ? FORWARD : BACKWARD;
}
static void markInputs(Constraint *c, int mark) {
  if (isUnary(c)) return;
  input(c)->mark = mark;
  if (c->kind == SCALE) c->scale->mark = c->offset->mark = mark;
}
static void recalculate(Constraint *c) {
  if (isUnary(c)) {
    c->myOutput->walkStrength = c->strength;
    c->myOutput->stay = !isInput(c);
    if (c->myOutput->stay) execute(c);
    return;
  }
  Variable *ihn = input(c), *out = output(c);
  out->walkStrength = weakestOf(c->strength, ihn->walkStrength);
  out->stay = c->kind == SCALE ? (ihn->stay && c->scale->stay && c->offset->stay) : ihn->stay;
  if (out->stay) execute(c);
}
static int inputsKnown(Constraint *c, int mark) {
  if (isUnary(c)) return 1;
  Variable *i = input(c);
  return i->mark == mark || i->stay || i->determinedBy == NULL;
}

static void incrementalRemove(Constraint *c);
static void addConstraintsConsumingTo(Variable *v, Collection *coll) {
  Constraint *determining = v->determinedBy;
  Collection *cc = v->constraints;
  for (int i = 0; i < cc->size; i++) {
    Constraint *c = cc->elms[i];
    if (c != determining && isSatisfied(c)) add(coll, c);
  }
}
static int addPropagate(Constraint *c, int mark) {
  Collection *todo = collection();
  add(todo, c);
  while (todo->size > 0) {
    Constraint *d = removeFirst(todo);
    if (output(d)->mark == mark) { incrementalRemove(c); return 0; }
    recalculate(d);
    addConstraintsConsumingTo(output(d), todo);
  }
  return 1;
}
static Constraint *satisfy(Constraint *c, int mark) {
  chooseMethod(c, mark);
  if (!isSatisfied(c)) {
    if (c->strength == REQUIRED) fail("Could not satisfy a required constraint!");
    return NULL;
  }
  markInputs(c, mark);
  Variable *out = output(c);
  Constraint *overridden = out->determinedBy;
  if (overridden != NULL) markUnsatisfied(overridden);
  out->determinedBy = c;
  if (!addPropagate(c, mark)) fail("Cycle encountered");
  out->mark = mark;
  return overridden;
}
static void incrementalAdd(Constraint *c) {
  int mark = ++currentMark;
  Constraint *overridden = satisfy(c, mark);
  while (overridden != NULL) overridden = satisfy(overridden, mark);
}
static Collection *removePropagateFrom(Variable *out) {
  out->determinedBy = NULL; out->walkStrength = WEAKEST; out->stay = 1;
  Collection *unsatisfied = collection(), *todo = collection();
  add(todo, out);
  while (todo->size > 0) {
    Variable *v = removeFirst(todo);
    for (int i = 0; i < v->constraints->size; i++) {
      Constraint *c = v->constraints->elms[i];
      if (!isSatisfied(c)) add(unsatisfied, c);
    }
    Constraint *determining = v->determinedBy;
    for (int i = 0; i < v->constraints->size; i++) {
      Constraint *next = v->constraints->elms[i];
      if (next != determining && isSatisfied(next)) { recalculate(next); add(todo, output(next)); }
    }
  }
  return unsatisfied;
}
static void incrementalRemove(Constraint *c) {
  Variable *out = output(c);
  markUnsatisfied(c);
  removeFromGraph(c);
  Collection *unsatisfied = removePropagateFrom(out);
  int strength = REQUIRED;
  do {
    for (int i = 0; i < unsatisfied->size; i++) {
      Constraint *u = unsatisfied->elms[i];
      if (u->strength == strength) incrementalAdd(u);
    }
    strength = nextWeaker(strength);
  } while (strength != WEAKEST);
}
static void addConstraint(Constraint *c) { addToGraph(c); incrementalAdd(c); }
static void destroyConstraint(Constraint *c) { if (isSatisfied(c)) incrementalRemove(c); else removeFromGraph(c); }

static Constraint *unary(Kind kind, Variable *v, int strength) {
  Constraint *c = alloc(sizeof *c); memset(c, 0, sizeof *c);
  c->kind = kind; c->strength = strength; c->myOutput = v; c->satisfied = 0;
  addConstraint(c);
  return c;
}
static Constraint *binary(Kind kind, Variable *v1, Variable *v2, int strength, Variable *scale, Variable *offset) {
  Constraint *c = alloc(sizeof *c); memset(c, 0, sizeof *c);
  c->kind = kind; c->strength = strength; c->scale = scale; c->offset = offset;
  c->v1 = v1; c->v2 = v2; c->direction = NONE;
  addConstraint(c);
  return c;
}

static Collection *makePlan(Collection *sources) {
  int mark = ++currentMark;
  Collection *plan = collection(), *todo = sources;
  while (todo->size > 0) {
    Constraint *c = removeFirst(todo);
    if (output(c)->mark != mark && inputsKnown(c, mark)) {
      add(plan, c);
      output(c)->mark = mark;
      addConstraintsConsumingTo(output(c), todo);
    }
  }
  return plan;
}
static Collection *extractPlanFromConstraints(Collection *constraints) {
  Collection *sources = collection();
  for (int i = 0; i < constraints->size; i++) {
    Constraint *c = constraints->elms[i];
    if (isInput(c) && isSatisfied(c)) add(sources, c);
  }
  return makePlan(sources);
}
static void executePlan(Collection *plan) { for (int i = 0; i < plan->size; i++) execute(plan->elms[i]); }

static void chainTest(int n) {
  currentMark = 0;
  Variable *prev = NULL, *first = NULL, *last = NULL;
  for (int i = 0; i <= n; i++) {
    Variable *v = variable("v", i, 0);
    if (prev != NULL) binary(EQUALITY, prev, v, REQUIRED, NULL, NULL);
    if (i == 0) first = v;
    if (i == n) last = v;
    prev = v;
  }
  unary(STAY, last, STRONG_DEFAULT);
  Constraint *edit = unary(EDIT, first, PREFERRED);
  Collection *edits = collection();
  add(edits, edit);
  Collection *plan = extractPlanFromConstraints(edits);
  for (int i = 0; i < 100; i++) {
    first->value = i;
    executePlan(plan);
    if (last->value != i) fail("Chain test failed.");
  }
}
static void change(Variable *v, double newValue) {
  Constraint *edit = unary(EDIT, v, PREFERRED);
  Collection *edits = collection();
  add(edits, edit);
  Collection *plan = extractPlanFromConstraints(edits);
  for (int i = 0; i < 10; i++) { v->value = newValue; executePlan(plan); }
  destroyConstraint(edit);
}
static void projectionTest(int n) {
  currentMark = 0;
  Variable *scale = variable("scale", 0, 10), *offset = variable("offset", 0, 1000);
  Variable *src = NULL, *dst = NULL;
  Collection *dests = collection();
  for (int i = 0; i < n; i++) {
    src = variable("src", i, i);
    dst = variable("dst", i, i);
    add(dests, dst);
    unary(STAY, src, NORMAL);
    binary(SCALE, src, dst, REQUIRED, scale, offset);
  }
  change(src, 17);
  if (dst->value != 1170) fail("Projection 1 failed");
  change(dst, 1050);
  if (src->value != 5) fail("Projection 2 failed");
  change(scale, 5);
  for (int i = 0; i < n - 1; i++) if (((Variable *)dests->elms[i])->value != i * 5 + 1000) fail("Projection 3 failed");
  change(offset, 2000);
  for (int i = 0; i < n - 1; i++) if (((Variable *)dests->elms[i])->value != i * 5 + 2000) fail("Projection 4 failed");
}
static void deltaBlue(void) {
  arena_used = 0;
  chainTest(100);
  projectionTest(100);
}

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : 120;
  arena_size = 64 << 20;
  arena = malloc(arena_size);
  struct timespec a, b;
  clock_gettime(CLOCK_MONOTONIC, &a);
  for (int i = 0; i < iterations; i++) for (int j = 0; j < 20; j++) deltaBlue();
  clock_gettime(CLOCK_MONOTONIC, &b);
  printf("delta-blue %d iterations %.3f ms\n", iterations, (b.tv_sec - a.tv_sec) * 1e3 + (b.tv_nsec - a.tv_nsec) / 1e6);
  return 0;
}
