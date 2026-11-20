// jetstream-row: earley-boyer
// jetstream-files: Octane/earley-boyer.js
// iterations: 120
/* C analogue of JetStream 3's Octane/earley-boyer.js, which is Scheme code compiled to JS.
 * Two workloads run on tagged cons cells, as the Scheme does:
 *  - earley: Feeley's Earley parser (make-parser builds the nonterminal, starter, ender and
 *    predictor tables; parse fills the forward and backward conf-set states; parse-trees builds
 *    the derivation trees), k = 7 tokens over the grammar s -> a | s s, counting 132 trees.
 *  - nboyer: the Boyer rewrite benchmark (n = 0): add-lemma setup over the lemma table with
 *    symbol records, translation, the rewrite loop with one-way unification, and the
 *    tautology check; the rewrite count must be 95024.
 * Each iteration runs earley then nboyer and exits non-zero when a count is wrong, as the JS
 * RunBenchmark warn does. The Scheme data (lemmas, test term, substitution alist, grammar) are
 * the S-expressions of the JS constants, read once into cons cells. Work objects live in an
 * arena reset per iteration. */
#define _POSIX_C_SOURCE 199309L
#include <ctype.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

/* ---- arena ---- */
#define ARENA_CHUNK (1 << 20)
typedef struct Chunk { struct Chunk *next; size_t used, cap; char data[]; } Chunk;
typedef struct { Chunk *head, *cur; } Arena;

static void *arena_alloc(Arena *a, size_t n) {
  n = (n + 15) & ~(size_t)15;
  Chunk *c = a->cur;
  if (c == NULL || c->used + n > c->cap) {
    Chunk *nx = (c == NULL) ? a->head : c->next;
    if (nx == NULL || nx->cap < n) {
      size_t cap = n > ARENA_CHUNK ? n : ARENA_CHUNK;
      Chunk *nc = malloc(sizeof(Chunk) + cap);
      if (nc == NULL) { fprintf(stderr, "out of memory\n"); exit(1); }
      nc->cap = cap; nc->used = 0; nc->next = nx;
      if (c) c->next = nc; else a->head = nc;
      nx = nc;
    }
    a->cur = nx;
    c = nx;
  }
  void *p = c->data + c->used;
  c->used += n;
  return p;
}

static void arena_reset(Arena *a) {
  for (Chunk *c = a->head; c; c = c->next) c->used = 0;
  a->cur = a->head;
}

static Arena perm_arena, work_arena;
static Arena *cur;   /* arena for new objects: perm while reading data, work during runs */

/* ---- tagged objects: pairs, interned symbols, numbers, boyer symbol records ---- */
enum { T_PAIR = 1, T_ATOM, T_NUM, T_REC };

typedef struct Obj Obj;
struct Obj {
  int tag;
  Obj *car, *cdr;     /* T_PAIR: car/cdr; T_REC: car = symbol atom, cdr = property list;
                         T_ATOM: cdr = chain in the symbol table */
  const char *name;   /* T_ATOM */
  long num;           /* T_NUM */
};

static Obj *new_obj(Arena *a, int tag) {
  Obj *o = arena_alloc(a, sizeof(Obj));
  memset(o, 0, sizeof *o);
  o->tag = tag;
  return o;
}
static Obj *cons(Obj *a, Obj *d) { Obj *o = new_obj(cur, T_PAIR); o->car = a; o->cdr = d; return o; }
static Obj *mknum(long v) { Obj *o = new_obj(cur, T_NUM); o->num = v; return o; }
static int is_pair(const Obj *o) { return o != NULL && o->tag == T_PAIR; }

#define SYMTAB_SIZE 256
static Obj *symtab[SYMTAB_SIZE];

static Obj *intern(const char *s) {
  unsigned h = 0;
  for (const char *p = s; *p; ++p) h = h * 31u + (unsigned char)*p;
  h %= SYMTAB_SIZE;
  for (Obj *o = symtab[h]; o; o = o->cdr)
    if (strcmp(o->name, s) == 0) return o;
  Obj *o = new_obj(&perm_arena, T_ATOM);
  char *n = arena_alloc(&perm_arena, strlen(s) + 1);
  strcpy(n, s);
  o->name = n;
  o->cdr = symtab[h];
  symtab[h] = o;
  return o;
}

/* ---- reader for the Scheme text of the JS constants ---- */
static const char *rd;

static void skip_ws(void) {
  while (*rd == ' ' || *rd == '\n' || *rd == '\t' || *rd == '\r') rd++;
}

static Obj *read_expr(void) {
  skip_ws();
  if (*rd == '(') {
    rd++;
    Obj *head = NULL, *tail = NULL;
    for (;;) {
      skip_ws();
      if (*rd == ')') { rd++; break; }
      if (*rd == '\0') { fprintf(stderr, "unterminated list\n"); exit(1); }
      Obj *cell = cons(read_expr(), NULL);
      if (tail) tail->cdr = cell; else head = cell;
      tail = cell;
    }
    return head;
  }
  const char *s = rd;
  while (*rd && *rd != ' ' && *rd != '\n' && *rd != '\t' && *rd != '\r' && *rd != '(' && *rd != ')') rd++;
  size_t n = (size_t)(rd - s);
  char buf[64];
  if (n == 0 || n >= sizeof buf) { fprintf(stderr, "bad token\n"); exit(1); }
  memcpy(buf, s, n);
  buf[n] = '\0';
  if (isdigit((unsigned char)buf[0]) || (buf[0] == '-' && n > 1 && isdigit((unsigned char)buf[1])))
    return mknum(atol(buf));
  return intern(buf);
}

/* ---- list helpers ---- */
static long length_of(Obj *l) { long n = 0; for (; l; l = l->cdr) n++; return n; }

static Obj *append2(Obj *a, Obj *b) {   /* sc_append of two lists: copies a */
  if (a == NULL) return b;
  Obj *head = NULL, *tail = NULL;
  for (; a; a = a->cdr) {
    Obj *c = cons(a->car, NULL);
    if (tail) tail->cdr = c; else head = c;
    tail = c;
  }
  tail->cdr = b;
  return head;
}

static int memq_obj(Obj *x, Obj *l) { for (; l; l = l->cdr) if (l->car == x) return 1; return 0; }

/* =====================================================================
 * Boyer
 * ===================================================================== */
static Obj *A_equal, *A_if, *A_f, *A_t, *A_or;
static Obj *alist_sym;          /* symbol -> record alist, reset by setup */
static Obj *if_constructor, *false_term, *true_term, *unify_subst;
static long rewrite_count;
static Obj *lemma_list, *test_term, *const_alist;

static Obj *assq(Obj *key, Obj *alist) {
  for (; alist; alist = alist->cdr) if (alist->car->car == key) return alist->car;
  return NULL;
}

static Obj *symrec(Obj *sym) {
  Obj *b = assq(sym, alist_sym);
  if (b) return b->cdr;
  Obj *r = new_obj(cur, T_REC);
  r->car = sym;
  r->cdr = NULL;
  alist_sym = cons(cons(sym, r), alist_sym);
  return r;
}

static Obj *translate_term(Obj *t);
static Obj *translate_list(Obj *l) {
  if (l == NULL) return NULL;
  Obj *a = translate_term(l->car);
  return cons(a, translate_list(l->cdr));
}
static Obj *translate_term(Obj *t) {
  if (!is_pair(t)) return t;
  Obj *h = symrec(t->car);
  return cons(h, translate_list(t->cdr));
}
static Obj *translate_alist(Obj *al) {
  if (al == NULL) return NULL;
  Obj *entry = cons(al->car->car, translate_term(al->car->cdr));
  return cons(entry, translate_alist(al->cdr));
}

static Obj *apply_subst(Obj *alist, Obj *term);
static Obj *apply_subst_list(Obj *alist, Obj *l) {
  if (l == NULL) return NULL;
  Obj *a = apply_subst(alist, l->car);
  return cons(a, apply_subst_list(alist, l->cdr));
}
static Obj *apply_subst(Obj *alist, Obj *term) {
  if (!is_pair(term)) {
    Obj *b = assq(term, alist);
    return b ? b->cdr : term;
  }
  return cons(term->car, apply_subst_list(alist, term->cdr));
}

static int obj_equal(Obj *x, Obj *y) {   /* sc_isEqual on atoms, numbers and lists */
  if (x == y) return 1;
  if (x == NULL || y == NULL) return 0;
  if (x->tag == T_NUM && y->tag == T_NUM) return x->num == y->num;
  if (is_pair(x) && is_pair(y)) return obj_equal(x->car, y->car) && obj_equal(x->cdr, y->cdr);
  return 0;
}

static int is_term_equal(Obj *x, Obj *y) {
  if (is_pair(x)) {
    if (!is_pair(y)) return 0;
    if (x->car != y->car) return 0;
    Obj *l1 = x->cdr, *l2 = y->cdr;
    for (;;) {
      if (l1 == NULL) return l2 == NULL;
      if (l2 == NULL) return 0;
      if (!is_term_equal(l1->car, l2->car)) return 0;
      l1 = l1->cdr;
      l2 = l2->cdr;
    }
  }
  return obj_equal(x, y);
}

static int is_term_member(Obj *x, Obj *l) {
  for (; l; l = l->cdr) if (is_term_equal(x, l->car)) return 1;
  return 0;
}

static int one_way_unify1(Obj *t1, Obj *t2) {
  if (!is_pair(t2)) {
    Obj *b = assq(t2, unify_subst);
    if (b) return is_term_equal(t1, b->cdr);
    if (t2->tag == T_NUM) return obj_equal(t1, t2);
    unify_subst = cons(cons(t2, t1), unify_subst);
    return 1;
  }
  if (!is_pair(t1)) return 0;
  if (t1->car != t2->car) return 0;
  Obj *l1 = t1->cdr, *l2 = t2->cdr;
  for (;;) {
    if (l1 == NULL) return l2 == NULL;
    if (l2 == NULL) return 0;
    if (!one_way_unify1(l1->car, l2->car)) return 0;
    l1 = l1->cdr;
    l2 = l2->cdr;
  }
}

static int tautologyp(Obj *x, Obj *true_lst, Obj *false_lst) {
  for (;;) {
    if (is_term_equal(x, true_term) || is_term_member(x, true_lst)) return 1;
    if (is_term_equal(x, false_term) || is_term_member(x, false_lst)) return 0;
    if (!is_pair(x)) return 0;
    if (x->car != if_constructor) return 0;
    Obj *test = x->cdr->car;
    Obj *then_t = x->cdr->cdr->car;
    Obj *else_t = x->cdr->cdr->cdr->car;
    if (is_term_equal(test, true_term) || is_term_member(test, true_lst)) {
      x = then_t;
    } else if (is_term_equal(test, false_term) || is_term_member(test, false_lst)) {
      x = else_t;
    } else if (tautologyp(then_t, cons(test, true_lst), false_lst)) {
      false_lst = cons(test, false_lst);
      x = else_t;
    } else {
      return 0;
    }
  }
}

static Obj *rewrite(Obj *term);
static Obj *rewrite_list(Obj *l) {
  if (l == NULL) return NULL;
  Obj *a = rewrite(l->car);
  return cons(a, rewrite_list(l->cdr));
}
static Obj *rewrite(Obj *term) {
  rewrite_count++;
  if (!is_pair(term)) return term;
  if (term->car->tag != T_REC) { fprintf(stderr, "rewrite: head is not a symbol record\n"); exit(1); }
  Obj *t12 = cons(term->car, rewrite_list(term->cdr));
  Obj *lst = term->car->cdr;
  for (;;) {
    if (lst == NULL) return t12;
    Obj *entry = lst->car;
    Obj *pattern = entry->cdr->car;
    unify_subst = NULL;
    if (one_way_unify1(t12, pattern)) {
      Obj *rhs = entry->cdr->cdr->car;
      Obj *inst = apply_subst(unify_subst, rhs);
      return rewrite(inst);
    }
    lst = lst->cdr;
  }
}

static void boyer_setup(void) {
  alist_sym = NULL;
  if_constructor = symrec(A_if);
  false_term = translate_term(cons(A_f, NULL));
  true_term = translate_term(cons(A_t, NULL));
  for (Obj *lst = lemma_list; lst; lst = lst->cdr) {
    Obj *term = lst->car;
    if (!(is_pair(term) && term->car == A_equal && is_pair(term->cdr) && is_pair(term->cdr->car))) {
      fprintf(stderr, "ADD-LEMMA did not like term\n");
      exit(1);
    }
    Obj *sym = term->cdr->car->car;
    Obj *x = translate_term(term);
    Obj *rec = symrec(sym);
    rec->cdr = cons(x, rec->cdr);
  }
}

/* testboyer(n): returns the rewrite count, or -1 when the tautology check fails. */
static long boyer_test(int n) {
  rewrite_count = 0;
  Obj *term = test_term;
  for (int k = n; k != 0; --k) term = cons(A_or, cons(term, cons(cons(A_f, NULL), NULL)));
  Obj *opnd = is_pair(term) ? cons(symrec(term->car), translate_list(term->cdr)) : term;
  Obj *alist = NULL;
  if (const_alist != NULL)
    alist = cons(cons(const_alist->car->car, translate_term(const_alist->car->cdr)),
                 translate_alist(const_alist->cdr));
  Obj *t21 = apply_subst(alist, opnd);
  Obj *rewritten = rewrite(t21);
  int answer = tautologyp(rewritten, NULL, NULL);
  return answer ? rewrite_count : -1;
}

/* =====================================================================
 * Earley (Feeley's conf-set formulation)
 * ===================================================================== */
#define NOTSET (-1000000)

typedef struct { int wl; int **cs; } EState;   /* wl: worklist head; cs[conf]: conf set or NULL */

typedef struct {
  int nb_nts, nb_confs, nb_toks;
  Obj **nts;                    /* nonterminal atoms, index = nt number */
  Obj **starters, **enders, **predictors;   /* per nt: list of conf numbers */
  int *steps;                   /* per conf: nt to predict, or negative completion marker */
  Obj **names;                  /* per conf: (head rule) or (nt 0); NULL = #f */
  Obj **toks;                   /* per token: (name . list of nt numbers) */
  EState *states, *statesA;
} Earley;

static int ind(Obj *nt, Obj **nts, int n) {
  for (int i = n - 1; i >= 0; --i) if (nts[i] == nt) return i;
  return -1;
}

static Earley *make_parser(Obj *grammar) {
  Earley *E = arena_alloc(cur, sizeof *E);
  /* nonterminals: each head, then each rule symbol, first seen first */
  Obj *acc = NULL;
  for (Obj *defs = grammar; defs; defs = defs->cdr) {
    Obj *head = defs->car->car;
    if (!memq_obj(head, acc)) acc = cons(head, acc);
    for (Obj *rl = defs->car->cdr; rl; rl = rl->cdr)
      for (Obj *l = rl->car; l; l = l->cdr)
        if (!memq_obj(l->car, acc)) acc = cons(l->car, acc);
  }
  int n = (int)length_of(acc);
  E->nb_nts = n;
  E->nts = arena_alloc(cur, sizeof(Obj *) * (size_t)n);
  {
    int idx = n - 1;
    for (Obj *l = acc; l; l = l->cdr) E->nts[idx--] = l->car;
  }
  /* conf count: each rule of length L takes L + 1 confs, plus one per nonterminal */
  int c = 0;
  for (Obj *defs = grammar; defs; defs = defs->cdr)
    for (Obj *rl = defs->car->cdr; rl; rl = rl->cdr) {
      int c16 = c;
      for (Obj *l = rl->car; l; l = l->cdr) c16++;
      c = c16 + 1;
    }
  int nc = c + n;
  E->nb_confs = nc;
  E->starters = arena_alloc(cur, sizeof(Obj *) * (size_t)n);
  E->enders = arena_alloc(cur, sizeof(Obj *) * (size_t)n);
  E->predictors = arena_alloc(cur, sizeof(Obj *) * (size_t)n);
  E->steps = arena_alloc(cur, sizeof(int) * (size_t)nc);
  E->names = arena_alloc(cur, sizeof(Obj *) * (size_t)nc);
  for (int i = 0; i < n; ++i) { E->starters[i] = NULL; E->enders[i] = NULL; E->predictors[i] = NULL; }
  for (int i = 0; i < nc; ++i) { E->names[i] = NULL; E->steps[i] = 0; }
  for (int i = n - 1; i >= 0; --i) {
    E->steps[i] = i - n;
    E->names[i] = cons(E->nts[i], cons(mknum(0), NULL));
    E->enders[i] = cons(mknum(i), NULL);
  }
  /* rules: starters, predictors, steps and enders */
  int conf = n;
  for (Obj *defs = grammar; defs; defs = defs->cdr) {
    Obj *head = defs->car->car;
    int ih = ind(head, E->nts, n);
    int rule_num = 1;
    int c0 = conf;
    for (Obj *rl = defs->car->cdr; rl; rl = rl->cdr) {
      E->names[c0] = cons(head, cons(mknum(rule_num), NULL));
      E->starters[ih] = cons(mknum(c0), E->starters[ih]);
      int sc = c0;
      for (Obj *l = rl->car; l; l = l->cdr) {
        int in = ind(l->car, E->nts, n);
        E->steps[sc] = in;
        E->predictors[in] = cons(mknum(sc), E->predictors[in]);
        sc++;
      }
      E->steps[sc] = ih - n;
      E->enders[ih] = cons(mknum(sc), E->enders[ih]);
      c0 = sc + 1;
      rule_num++;
    }
    conf = c0;
  }
  return E;
}

static EState *make_states(int ntoks, int nconf) {
  EState *s = arena_alloc(cur, sizeof(EState) * (size_t)(ntoks + 1));
  for (int i = ntoks; i >= 0; --i) {
    s[i].wl = -1;
    s[i].cs = arena_alloc(cur, sizeof(int *) * (size_t)nconf);
    for (int c = 0; c < nconf; ++c) s[i].cs[c] = NULL;
  }
  return s;
}

static int *getget(EState *st, int num, int conf) {
  if (st->cs[conf] != NULL) return st->cs[conf];
  int *cs = arena_alloc(cur, sizeof(int) * (size_t)(num + 6));
  for (int k = 0; k < num + 6; ++k) cs[k] = NOTSET;
  cs[0] = -1;
  cs[1] = -3;   /* merged tail: -3 means the merged head lives in slot 2 */
  cs[2] = -1;   /* merged origins head */
  cs[3] = -1;   /* new origins tail */
  cs[4] = -1;   /* new origins head */
  st->cs[conf] = cs;
  return cs;
}

static void conf_adjoin(EState *st, int *cs, int conf, int i) {
  int tail = cs[3];
  cs[i + 5] = -1;
  cs[tail + 5] = i;        /* tail = -1 writes the new-head slot 4 */
  cs[3] = i;
  if (tail < 0) {          /* conf was idle: put it on the state worklist */
    cs[0] = st->wl;
    st->wl = conf;
  }
}

static void conf_merge_new(int *cs) {
  cs[cs[1] + 5] = cs[4];
  cs[1] = cs[3];
  cs[3] = -1;
  cs[4] = -1;
}

static void conf_union(EState *st, int *cs, int conf, const int *other) {
  int i = other[2];
  while (i >= 0) {
    if (cs[i + 5] == NOTSET) conf_adjoin(st, cs, conf, i);
    i = other[i + 5];
  }
}

static void adjoin45(EState *states, int num, Obj *l, int i) {
  EState *st = &states[num];
  for (; l; l = l->cdr) {
    int conf = (int)l->car->num;
    int *cs = getget(st, num, conf);
    if (cs[i + 5] == NOTSET) conf_adjoin(st, cs, conf, i);
  }
}

static int adjoin46(EState *states, EState *statesA, int num, int conf, int i) {
  int *cs = states[num].cs[conf];
  if (cs != NULL && cs[i + 5] != NOTSET) {
    EState *stA = &statesA[num];
    int *csA = getget(stA, num, conf);
    if (csA[i + 5] == NOTSET) conf_adjoin(stA, csA, conf, i);
    return 1;
  }
  return 0;
}

static void forw(Earley *E, EState *states, int num) {
  EState *st = &states[num];
  int nb_nts = E->nb_nts;
  for (;;) {
    int conf = st->wl;
    if (conf < 0) return;
    int step = E->steps[conf];
    int *cs = st->cs[conf];
    int head = cs[4];
    st->wl = cs[0];
    conf_merge_new(cs);
    if (step >= 0) {
      for (Obj *l = E->starters[step]; l; l = l->cdr) {
        int starter = (int)l->car->num;
        int *ss = getget(st, num, starter);
        if (ss[num + 5] == NOTSET) conf_adjoin(st, ss, starter, num);
      }
      for (Obj *l = E->enders[step]; l; l = l->cdr) {
        int ender = (int)l->car->num;
        int *es = st->cs[ender];
        if (es != NULL && es[num + 5] != NOTSET) {
          int next = conf + 1;
          int *ns = getget(st, num, next);
          conf_union(st, ns, next, cs);
        }
      }
    } else {
      for (Obj *l = E->predictors[step + nb_nts]; l; l = l->cdr) {
        int pred = (int)l->car->num;
        for (int i = head; i >= 0; i = cs[i + 5]) {
          int *ps = states[i].cs[pred];
          if (ps != NULL) {
            int next = pred + 1;
            int *ns = getget(st, num, next);
            conf_union(st, ns, next, ps);
          }
        }
      }
    }
  }
}

/* parse: forward pass over the token states, then the backward pass into statesA. */
static void earley_parse(Earley *E, Obj **input, int k) {
  E->nb_toks = k;
  E->toks = arena_alloc(cur, sizeof(Obj *) * (size_t)k);
  for (int i = 0; i < k; ++i) {
    Obj *x = input[i];
    int idx = ind(x, E->nts, E->nb_nts);
    Obj *nts_of_tok = (idx >= 0) ? cons(mknum(idx), NULL) : NULL;
    E->toks[i] = cons(x, nts_of_tok);
  }
  int nb_nts = E->nb_nts;
  EState *states = make_states(k, E->nb_confs);
  E->states = states;
  adjoin45(states, 0, E->starters[0], 0);
  forw(E, states, 0);
  for (int i = 0; i < k; ++i) {
    adjoin45(states, i + 1, E->toks[i]->cdr, i);
    forw(E, states, i + 1);
  }

  EState *statesA = make_states(k, E->nb_confs);
  E->statesA = statesA;
  for (Obj *l = E->enders[0]; l; l = l->cdr)
    adjoin46(states, statesA, k, (int)l->car->num, 0);
  for (int si = k; si >= 0; --si) {
    EState *sa = &statesA[si];
    for (;;) {
      int conf = sa->wl;
      if (conf < 0) break;
      int *cs = sa->cs[conf];
      int head = cs[4];
      sa->wl = cs[0];
      conf_merge_new(cs);
      for (int ii = head; ii >= 0; ii = cs[ii + 5]) {
        int prev = conf - 1;
        if (conf >= nb_nts && E->steps[prev] >= 0) {
          for (Obj *l = E->enders[E->steps[prev]]; l; l = l->cdr) {
            int ender = (int)l->car->num;
            int *es = states[si].cs[ender];
            if (es == NULL) continue;
            for (int kk = es[2]; kk >= 0; kk = es[kk + 5]) {
              if (kk >= ii && adjoin46(states, statesA, kk, prev, ii))
                adjoin46(states, statesA, si, ender, kk);
            }
          }
        }
      }
    }
  }
}

static Obj *deriv_trees(Earley *E, int conf, int i, int j);

static Obj *dt_loop1(Earley *E, int prev, int i, int j, Obj *l1, Obj *l2);

static Obj *dt_loop2(Earley *E, int prev, int i, int j, Obj *l1, int ender, int *es, int k, Obj *l2) {
  for (;;) {
    if (k < 0) return dt_loop1(E, prev, i, j, l1->cdr, l2);
    int *cp = (k >= i) ? E->statesA[k].cs[prev] : NULL;
    if (cp != NULL && cp[i + 5] != NOTSET) {
      Obj *prev_trees = deriv_trees(E, prev, i, k);
      Obj *ender_trees = deriv_trees(E, ender, k, j);
      Obj *acc = l2;
      for (Obj *l3 = ender_trees; l3; l3 = l3->cdr) {
        Obj *ender_tree = cons(l3->car, NULL);
        for (Obj *l4 = prev_trees; l4; l4 = l4->cdr) acc = cons(append2(l4->car, ender_tree), acc);
      }
      return dt_loop2(E, prev, i, j, l1, ender, es, es[k + 5], acc);
    }
    k = es[k + 5];
  }
}

static Obj *dt_loop1(Earley *E, int prev, int i, int j, Obj *l1, Obj *l2) {
  while (l1 != NULL) {
    int ender = (int)l1->car->num;
    int *es = E->statesA[j].cs[ender];
    if (es != NULL) return dt_loop2(E, prev, i, j, l1, ender, es, es[2], l2);
    l1 = l1->cdr;
  }
  return l2;
}

static Obj *deriv_trees(Earley *E, int conf, int i, int j) {
  Obj *name = E->names[conf];
  if (name != NULL) {
    if (conf < E->nb_nts) return cons(cons(name, cons(E->toks[i]->car, NULL)), NULL);
    return cons(cons(name, NULL), NULL);
  }
  int prev = conf - 1;
  return dt_loop1(E, prev, i, j, E->enders[E->steps[prev]], NULL);
}

static Obj *parse_trees(Earley *E, Obj *nt, int i, int j) {
  int ntidx = ind(nt, E->nts, E->nb_nts);
  Obj *trees = NULL;
  for (Obj *l = E->enders[ntidx]; l; l = l->cdr) {
    int conf = (int)l->car->num;
    int *cs = E->statesA[j].cs[conf];
    if (cs != NULL && cs[i + 5] != NOTSET) {
      Obj *d = deriv_trees(E, conf, i, j);
      trees = append2(d, trees);
    }
  }
  return trees;
}

/* =====================================================================
 * Data and benchmark driver
 * ===================================================================== */
static const char BOYER_LEMMAS[] = "((equal (compile form) (reverse (codegen (optimize form) (nil)))) (equal (eqp x y) (equal (fix "
    "x) (fix y))) (equal (greaterp x y) (lessp y x)) (equal (lesseqp x y) (not (lessp y x))) (equal "
    "(greatereqp x y) (not (lessp x y))) (equal (boolean x) (or (equal x (t)) (equal x (f)))) (equal "
    "(iff x y) (and (implies x y) (implies y x))) (equal (even1 x) (if (zerop x) (t) (odd (sub1 "
    "x)))) (equal (countps- l pred) (countps-loop l pred (zero))) (equal (fact- i) (fact-loop i 1)) "
    "(equal (reverse- x) (reverse-loop x (nil))) (equal (divides x y) (zerop (remainder y x))) "
    "(equal (assume-true var alist) (cons (cons var (t)) alist)) (equal (assume-false var alist) "
    "(cons (cons var (f)) alist)) (equal (tautology-checker x) (tautologyp (normalize x) (nil))) "
    "(equal (falsify x) (falsify1 (normalize x) (nil))) (equal (prime x) (and (not (zerop x)) (not "
    "(equal x (add1 (zero)))) (prime1 x (sub1 x)))) (equal (and p q) (if p (if q (t) (f)) (f))) "
    "(equal (or p q) (if p (t) (if q (t) (f)))) (equal (not p) (if p (f) (t))) (equal (implies p q) "
    "(if p (if q (t) (f)) (t))) (equal (fix x) (if (numberp x) x (zero))) (equal (if (if a b c) d e) "
    "(if a (if b d e) (if c d e))) (equal (zerop x) (or (equal x (zero)) (not (numberp x)))) (equal "
    "(plus (plus x y) z) (plus x (plus y z))) (equal (equal (plus a b) (zero)) (and (zerop a) (zerop "
    "b))) (equal (difference x x) (zero)) (equal (equal (plus a b) (plus a c)) (equal (fix b) (fix "
    "c))) (equal (equal (zero) (difference x y)) (not (lessp y x))) (equal (equal x (difference x "
    "y)) (and (numberp x) (or (equal x (zero)) (zerop y)))) (equal (meaning (plus-tree (append x y)) "
    "a) (plus (meaning (plus-tree x) a) (meaning (plus-tree y) a))) (equal (meaning (plus-tree "
    "(plus-fringe x)) a) (fix (meaning x a))) (equal (append (append x y) z) (append x (append y "
    "z))) (equal (reverse (append a b)) (append (reverse b) (reverse a))) (equal (times x (plus y "
    "z)) (plus (times x y) (times x z))) (equal (times (times x y) z) (times x (times y z))) (equal "
    "(equal (times x y) (zero)) (or (zerop x) (zerop y))) (equal (exec (append x y) pds envrn) (exec "
    "y (exec x pds envrn) envrn)) (equal (mc-flatten x y) (append (flatten x) y)) (equal (member x "
    "(append a b)) (or (member x a) (member x b))) (equal (member x (reverse y)) (member x y)) "
    "(equal (length (reverse x)) (length x)) (equal (member a (intersect b c)) (and (member a b) "
    "(member a c))) (equal (nth (zero) i) (zero)) (equal (exp i (plus j k)) (times (exp i j) (exp i "
    "k))) (equal (exp i (times j k)) (exp (exp i j) k)) (equal (reverse-loop x y) (append (reverse "
    "x) y)) (equal (reverse-loop x (nil)) (reverse x)) (equal (count-list z (sort-lp x y)) (plus "
    "(count-list z x) (count-list z y))) (equal (equal (append a b) (append a c)) (equal b c)) "
    "(equal (plus (remainder x y) (times y (quotient x y))) (fix x)) (equal (power-eval (big-plus1 l "
    "i base) base) (plus (power-eval l base) i)) (equal (power-eval (big-plus x y i base) base) "
    "(plus i (plus (power-eval x base) (power-eval y base)))) (equal (remainder y 1) (zero)) (equal "
    "(lessp (remainder x y) y) (not (zerop y))) (equal (remainder x x) (zero)) (equal (lessp "
    "(quotient i j) i) (and (not (zerop i)) (or (zerop j) (not (equal j 1))))) (equal (lessp "
    "(remainder x y) x) (and (not (zerop y)) (not (zerop x)) (not (lessp x y)))) (equal (power-eval "
    "(power-rep i base) base) (fix i)) (equal (power-eval (big-plus (power-rep i base) (power-rep j "
    "base) (zero) base) base) (plus i j)) (equal (gcd x y) (gcd y x)) (equal (nth (append a b) i) "
    "(append (nth a i) (nth b (difference i (length a))))) (equal (difference (plus x y) x) (fix y)) "
    "(equal (difference (plus y x) x) (fix y)) (equal (difference (plus x y) (plus x z)) (difference "
    "y z)) (equal (times x (difference c w)) (difference (times c x) (times w x))) (equal (remainder "
    "(times x z) z) (zero)) (equal (difference (plus b (plus a c)) a) (plus b c)) (equal (difference "
    "(add1 (plus y z)) z) (add1 y)) (equal (lessp (plus x y) (plus x z)) (lessp y z)) (equal (lessp "
    "(times x z) (times y z)) (and (not (zerop z)) (lessp x y))) (equal (lessp y (plus x y)) (not "
    "(zerop x))) (equal (gcd (times x z) (times y z)) (times z (gcd x y))) (equal (value (normalize "
    "x) a) (value x a)) (equal (equal (flatten x) (cons y (nil))) (and (nlistp x) (equal x y))) "
    "(equal (listp (gopher x)) (listp x)) (equal (samefringe x y) (equal (flatten x) (flatten y))) "
    "(equal (equal (greatest-factor x y) (zero)) (and (or (zerop y) (equal y 1)) (equal x (zero)))) "
    "(equal (equal (greatest-factor x y) 1) (equal x 1)) (equal (numberp (greatest-factor x y)) (not "
    "(and (or (zerop y) (equal y 1)) (not (numberp x))))) (equal (times-list (append x y)) (times "
    "(times-list x) (times-list y))) (equal (prime-list (append x y)) (and (prime-list x) "
    "(prime-list y))) (equal (equal z (times w z)) (and (numberp z) (or (equal z (zero)) (equal w "
    "1)))) (equal (greatereqp x y) (not (lessp x y))) (equal (equal x (times x y)) (or (equal x "
    "(zero)) (and (numberp x) (equal y 1)))) (equal (remainder (times y x) y) (zero)) (equal (equal "
    "(times a b) 1) (and (not (equal a (zero))) (not (equal b (zero))) (numberp a) (numberp b) "
    "(equal (sub1 a) (zero)) (equal (sub1 b) (zero)))) (equal (lessp (length (delete x l)) (length "
    "l)) (member x l)) (equal (sort2 (delete x l)) (delete x (sort2 l))) (equal (dsort x) (sort2 x)) "
    "(equal (length (cons x1 (cons x2 (cons x3 (cons x4 (cons x5 (cons x6 x7))))))) (plus 6 (length "
    "x7))) (equal (difference (add1 (add1 x)) 2) (fix x)) (equal (quotient (plus x (plus x y)) 2) "
    "(plus x (quotient y 2))) (equal (sigma (zero) i) (quotient (times i (add1 i)) 2)) (equal (plus "
    "x (add1 y)) (if (numberp y) (add1 (plus x y)) (add1 x))) (equal (equal (difference x y) "
    "(difference z y)) (if (lessp x y) (not (lessp y z)) (if (lessp z y) (not (lessp y x)) (equal "
    "(fix x) (fix z))))) (equal (meaning (plus-tree (delete x y)) a) (if (member x y) (difference "
    "(meaning (plus-tree y) a) (meaning x a)) (meaning (plus-tree y) a))) (equal (times x (add1 y)) "
    "(if (numberp y) (plus x (times x y)) (fix x))) (equal (nth (nil) i) (if (zerop i) (nil) "
    "(zero))) (equal (last (append a b)) (if (listp b) (last b) (if (listp a) (cons (car (last a)) "
    "b) b))) (equal (equal (lessp x y) z) (if (lessp x y) (equal (t) z) (equal (f) z))) (equal "
    "(assignment x (append a b)) (if (assignedp x a) (assignment x a) (assignment x b))) (equal (car "
    "(gopher x)) (if (listp x) (car (flatten x)) (zero))) (equal (flatten (cdr (gopher x))) (if "
    "(listp x) (cdr (flatten x)) (cons (zero) (nil)))) (equal (quotient (times y x) y) (if (zerop y) "
    "(zero) (fix x))) (equal (get j (set i val mem)) (if (eqp j i) val (get j mem)))) ";
static const char BOYER_TEST[] = "(implies (and (implies x y) (and (implies y z) (and (implies z u) (implies u w)))) (implies x "
    "w)) ";
static const char BOYER_ALIST[] = "((x f (plus (plus a b) (plus c (zero)))) (y f (times (times a b) (plus c d))) (z f (reverse "
    "(append (append a b) (nil)))) (u equal (plus a b) (difference x y)) (w lessp (remainder a b) "
    "(member a (length b)))) ";
static const char EARLEY_GRAMMAR[] = "((s (a) (s s))) ";

static Obj *grammar_obj;
static Obj *A_s, *A_a;

static void init_data(void) {
  cur = &perm_arena;
  A_equal = intern("equal");
  A_if = intern("if");
  A_f = intern("f");
  A_t = intern("t");
  A_or = intern("or");
  A_s = intern("s");
  A_a = intern("a");
  rd = BOYER_LEMMAS;
  lemma_list = read_expr();
  rd = BOYER_TEST;
  test_term = read_expr();
  rd = BOYER_ALIST;
  const_alist = read_expr();
  rd = EARLEY_GRAMMAR;
  grammar_obj = read_expr();
}

/* earley benchmark test(k): parse k tokens "a", count the trees for s. */
static long earley_test(int k) {
  Earley *p = make_parser(grammar_obj);
  Obj **input = arena_alloc(cur, sizeof(Obj *) * (size_t)k);
  for (int i = 0; i < k; ++i) input[i] = A_a;
  earley_parse(p, input, k);
  return length_of(parse_trees(p, A_s, 0, k));
}

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : 120;
  init_data();
  struct timespec a, b;
  clock_gettime(CLOCK_MONOTONIC, &a);
  for (int it = 0; it < iterations; ++it) {
    arena_reset(&work_arena);
    cur = &work_arena;
    long trees = earley_test(7);
    if (trees != 132) {
      fprintf(stderr, "Earley or Boyer did incorrect number of rewrites\n");
      return 1;
    }
    boyer_setup();
    long rewrites = boyer_test(0);
    if (rewrites != 95024) {
      fprintf(stderr, "Earley or Boyer did incorrect number of rewrites\n");
      return 1;
    }
  }
  clock_gettime(CLOCK_MONOTONIC, &b);
  printf("earley-boyer %d iterations %.3f ms\n", iterations,
         (b.tv_sec - a.tv_sec) * 1e3 + (b.tv_nsec - a.tv_nsec) / 1e6);
  printf("validated: earley 132 trees, nboyer 95024 rewrites per iteration\n");
  return 0;
}
