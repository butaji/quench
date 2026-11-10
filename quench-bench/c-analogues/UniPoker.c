/* C analogue of JetStream's RexBench/UniPoker: four players are dealt 2000 five-card hands
 * per iteration from a deck shuffled with JetStream's deterministic Math.random (reset
 * before each iteration). Cards are Unicode playing-card code points; a hand is scored by
 * sorting it, joining it into a UTF-16 string and searching that string with the
 * benchmark's three Unicode regular expressions. The expressions are kept as data (per
 * alternative, a sequence of card classes with repeat bounds) and searched with leftmost,
 * first-alternative, greedy-with-backtracking semantics over surrogate pairs, as a regex
 * engine compiles them, so a JS engine's native code is compared with what a C programmer
 * writes for the same program. */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

enum { DECK_SIZE = 52, HAND_SIZE = 5, PLAYERS = 4, HANDS_PER_ITERATION = 2000, HAND_TYPES = 10 };
enum { DEFAULT_ITERATIONS = 120 };
enum { HIGH_CARD, PAIR, TWO_PAIR, THREE_OF_A_KIND, STRAIGHT, FLUSH, FULL_HOUSE, FOUR_OF_A_KIND,
       STRAIGHT_FLUSH, ROYAL_FLUSH };
enum { HAND_TYPE_SHIFT = 20 };
#define HAND(type) ((type) << HAND_TYPE_SHIFT)

/* Playing-card code points are 0x1F0SR: suit nibble S (A spades, B hearts, C diamonds,
 * D clubs) and rank nibble R (1 ace .. 14 king, 12 the knight, absent from the deck). */
enum { CARD_BLOCK = 0x1F0, SUIT_FIRST = 0xA, RANK_MASK = 0xF, ACE = 0x1, ACE_HIGH = 0xF };
enum { SPADES = 1, HEARTS = 2, DIAMONDS = 4, CLUBS = 8, ANY_SUIT = SPADES | HEARTS | DIAMONDS | CLUBS };
#define CARD(suit, rank) (0x1F000 | (suit) << 4 | (rank))

/* UTF-16 encoding of astral code points. */
enum { SUPPLEMENTARY_BASE = 0x10000, LEAD_SURROGATE = 0xD800, TRAIL_SURROGATE = 0xDC00,
       SURROGATE_RANGE = 0x400, SURROGATE_BITS = 10 };
enum { CARD_UNITS = 2, HAND_UNITS = HAND_SIZE * CARD_UNITS };

static const uint32_t newDeck[DECK_SIZE] = {
#define SUIT(s) CARD(s, 1), CARD(s, 2), CARD(s, 3), CARD(s, 4), CARD(s, 5), CARD(s, 6), CARD(s, 7), \
                CARD(s, 8), CARD(s, 9), CARD(s, 10), CARD(s, 11), CARD(s, 13), CARD(s, 14)
  SUIT(0xA), SUIT(0xB), SUIT(0xD), SUIT(0xC)
#undef SUIT
};

/* JetStream's deterministic Math.random: Robert Jenkins' 32-bit integer hash. */
enum { INITIAL_SEED = 49734321 };
static uint32_t seed;
static double random_double(void) {
  seed = (seed + 0x7ed55d16) + (seed << 12);
  seed = (seed ^ 0xc761c23c) ^ (seed >> 19);
  seed = (seed + 0x165667b1) + (seed << 5);
  seed = (seed + 0xd3a2646c) ^ (seed << 9);
  seed = (seed + 0xfd7046c5) + (seed << 3);
  seed = (seed ^ 0xb55a4f09) ^ (seed >> 16);
  return seed / 4294967296.0;
}

typedef struct { uint32_t cards[DECK_SIZE]; int head; } CardDeck;

static void deck_new(CardDeck *d) {
  for (int i = 0; i < DECK_SIZE; i++) d->cards[i] = newDeck[i];
  d->head = 0;
}

static void deck_shuffle(CardDeck *d) {
  deck_new(d);
  for (int index = DECK_SIZE; index != 0;) {
    int randomIndex = (int)(random_double() * index);
    index--;
    uint32_t temp = d->cards[index];
    d->cards[index] = d->cards[randomIndex];
    d->cards[randomIndex] = temp;
  }
}

static uint32_t deck_deal(CardDeck *d) { return d->cards[d->head++]; }

static int card_rank(uint32_t card) {
  int rank = card & RANK_MASK;
  return rank == ACE ? ACE_HIGH : rank;
}

/* Patterns: alternatives of terms, each a card class repeated min..max times. */
typedef struct { uint8_t suits; uint16_t ranks; uint8_t min, max; } Term;
enum { MAX_TERMS = 5, MAX_REPEAT = 5 };
typedef struct { int count; Term terms[MAX_TERMS]; } Alternative;
typedef struct { int count; const Alternative *alts; } Pattern;
#define R(rank) (1u << (rank))
#define ONE(rank) {ANY_SUIT, R(rank), 1, 1}
enum { ALL_RANKS = 0x7FFE };

static const Alternative flushAlts[] = {
  {1, {{SPADES, ALL_RANKS, 5, 5}}}, {1, {{HEARTS, ALL_RANKS, 5, 5}}},
  {1, {{DIAMONDS, ALL_RANKS, 5, 5}}}, {1, {{CLUBS, ALL_RANKS, 5, 5}}},
};
/* The first alternative (ace high) is the capture group that marks a royal flush. */
static const Alternative straightAlts[] = {
  {5, {ONE(1), ONE(14), ONE(13), ONE(11), ONE(10)}}, {5, {ONE(14), ONE(13), ONE(11), ONE(10), ONE(9)}},
  {5, {ONE(13), ONE(11), ONE(10), ONE(9), ONE(8)}}, {5, {ONE(11), ONE(10), ONE(9), ONE(8), ONE(7)}},
  {5, {ONE(10), ONE(9), ONE(8), ONE(7), ONE(6)}}, {5, {ONE(9), ONE(8), ONE(7), ONE(6), ONE(5)}},
  {5, {ONE(8), ONE(7), ONE(6), ONE(5), ONE(4)}}, {5, {ONE(7), ONE(6), ONE(5), ONE(4), ONE(3)}},
  {5, {ONE(6), ONE(5), ONE(4), ONE(3), ONE(2)}}, {5, {ONE(1), ONE(5), ONE(4), ONE(3), ONE(2)}},
};
#define KIND(rank) {1, {{ANY_SUIT, R(rank), 2, 4}}}
static const Alternative ofAKindAlts[] = {
  KIND(1), KIND(14), KIND(13), KIND(11), KIND(10), KIND(9), KIND(8), KIND(7), KIND(6), KIND(5),
  KIND(4), KIND(3), KIND(2),
};
#define PATTERN(alts) {sizeof alts / sizeof *alts, alts}
static const Pattern flushPattern = PATTERN(flushAlts), straightPattern = PATTERN(straightAlts),
                     ofAKindPattern = PATTERN(ofAKindAlts);

/* String.prototype.codePointAt; *width receives the code units it spans. */
static uint32_t code_point_at(const uint16_t *s, int len, int i, int *width) {
  uint32_t c = s[i];
  if (c - LEAD_SURROGATE < SURROGATE_RANGE && i + 1 < len && s[i + 1] - TRAIL_SURROGATE < SURROGATE_RANGE) {
    *width = 2;
    return SUPPLEMENTARY_BASE + ((c - LEAD_SURROGATE) << SURROGATE_BITS) + (s[i + 1] - TRAIL_SURROGATE);
  }
  *width = 1;
  return c;
}

static int in_class(const Term *t, uint32_t cp) {
  uint32_t suit = (cp >> 4 & RANK_MASK) - SUIT_FIRST;
  return cp >> 8 == CARD_BLOCK && suit < 4 && (t->suits >> suit & 1) && (t->ranks >> (cp & RANK_MASK) & 1);
}

/* Greedy repeat of the first term, backtracking into fewer repeats if the rest fails. */
static int match_terms(const Term *t, int n, const uint16_t *s, int len, int pos) {
  if (n == 0) return pos;
  int ends[MAX_REPEAT + 1], count = 0, width;
  ends[0] = pos;
  while (count < t->max && pos < len && in_class(t, code_point_at(s, len, pos, &width))) ends[++count] = pos += width;
  for (; count >= t->min; count--) {
    int end = match_terms(t + 1, n - 1, s, len, ends[count]);
    if (end >= 0) return end;
  }
  return -1;
}

typedef struct { int index, length, alt; } Match;

/* String.prototype.match with a non-global /u pattern: the leftmost match, first alternative. */
static int search(const Pattern *p, const uint16_t *s, int len, Match *m) {
  int width;
  for (int start = 0; start < len; start += width) {
    for (int a = 0; a < p->count; a++) {
      int end = match_terms(p->alts[a].terms, p->alts[a].count, s, len, start);
      if (end >= 0) { *m = (Match){start, end - start, a}; return 1; }
    }
    code_point_at(s, len, start, &width);
  }
  return 0;
}

static int index_of(const uint16_t *s, int len, const uint16_t *needle, int n) {
  for (int i = 0; i + n <= len; i++) {
    int j = 0;
    while (j < n && s[i + j] == needle[j]) j++;
    if (j == n) return i;
  }
  return -1;
}

static int rank_at(const uint16_t *s, int len, int i) { int w; return card_rank(code_point_at(s, len, i, &w)); }
static uint32_t cp_at(const uint16_t *s, int len, int i) { int w; return code_point_at(s, len, i, &w); }

typedef struct { const char *name; uint32_t cards[HAND_SIZE]; int count, rank, wins; int handTypeCounts[HAND_TYPES]; } Player;

static void clear(Player *p) { p->count = 0; p->rank = 0; }
static void take_card(Player *p, uint32_t card) { p->cards[p->count++] = card; }

static int by_rank_descending(const void *a, const void *b) {
  return card_rank(*(const uint32_t *)b) - card_rank(*(const uint32_t *)a);
}

static int join(const uint32_t *cards, int count, uint16_t *s) {
  int len = 0;
  for (int i = 0; i < count; i++) {
    uint32_t v = cards[i] - SUPPLEMENTARY_BASE;
    s[len++] = LEAD_SURROGATE + (v >> SURROGATE_BITS);
    s[len++] = TRAIL_SURROGATE + (v & (SURROGATE_RANGE - 1));
  }
  return len;
}

/* Pair, two pair, three of a kind or full house from the first of-a-kind run. */
static int score_of_a_kind(const Player *p, const uint16_t *s, int len, const Match *first) {
  int remaining = index_of(s, len, s + first->index, first->length) + first->length;
  Match second;
  if (remaining <= 6 && search(&ofAKindPattern, s + remaining, len - remaining, &second)) {
    second.index += remaining;
    if ((first->length == 6 && second.length == 4) || (first->length == 4 && second.length == 6)) {
      int three = rank_at(s, len, first->length == 6 ? first->index : second.index);
      int two = rank_at(s, len, first->length == 6 ? second.index : first->index);
      /* The JS writes `three < 12` where `three << 12` was meant; the comparison is kept. */
      return HAND(FULL_HOUSE) | three << 16 | (three < 12) | three << 8 | two << 4 | two;
    }
    if (first->length == 4 && second.length == 4) {
      int firstPair = rank_at(s, len, first->index), secondPair = rank_at(s, len, second.index), other;
      if (cp_at(s, len, first->index) == cp_at(s, len, 0))
        other = rank_at(s, len, cp_at(s, len, second.index) == cp_at(s, len, 4) ? 8 : 4);
      else
        other = rank_at(s, len, 0);
      return HAND(TWO_PAIR) | firstPair << 16 | firstPair << 12 | secondPair << 8 | secondPair << 4 | other;
    }
    return p->rank;
  }
  int kind = rank_at(s, len, first->index), others = 0;
  for (int i = 0; i < p->count; i++) {
    int rank = card_rank(p->cards[i]);
    if (rank != kind) others = others << 4 | rank;
  }
  if (first->length == 6) return HAND(THREE_OF_A_KIND) | kind << 16 | kind << 12 | kind << 8 | others;
  return HAND(PAIR) | kind << 16 | kind << 12 | others;
}

static void score(Player *p) {
  uint16_t s[HAND_UNITS];
  qsort(p->cards, p->count, sizeof *p->cards, by_rank_descending);
  int len = join(p->cards, p->count, s);
  Match flush, straight, ofAKind;
  int isFlush = search(&flushPattern, s, len, &flush);
  int isStraight = search(&straightPattern, s, len, &straight);
  int isOfAKind = search(&ofAKindPattern, s, len, &ofAKind);
  int top = card_rank(p->cards[0]) << 16 | card_rank(p->cards[1]) << 12;
  if (isFlush)
    p->rank = HAND(isStraight ? (straight.alt == 0 ? ROYAL_FLUSH : STRAIGHT_FLUSH) : FLUSH) | top;
  else if (isStraight)
    p->rank = HAND(STRAIGHT) | top;
  else if (isOfAKind)
    p->rank = ofAKind.length == 8 ? HAND(FOUR_OF_A_KIND) | card_rank(p->cards[0]) : score_of_a_kind(p, s, len, &ofAKind);
  else {
    p->rank = 0;
    for (int i = 0; i < p->count; i++) p->rank = p->rank << 4 | card_rank(p->cards[i]);
  }
}

static void score_hand(Player *p) {
  score(p);
  p->handTypeCounts[p->rank >> HAND_TYPE_SHIFT]++;
}

static void play_hands(Player *players) {
  CardDeck deck;
  deck_new(&deck);
  for (int handsPlayed = 0; handsPlayed < HANDS_PER_ITERATION;) {
    deck_shuffle(&deck);
    for (int p = 0; p < PLAYERS; p++) clear(&players[p]);
    for (int i = 0; i < HAND_SIZE; i++)
      for (int p = 0; p < PLAYERS; p++) take_card(&players[p], deck_deal(&deck));
    for (int p = 0; p < PLAYERS; p++) score_hand(&players[p]);
    handsPlayed++;
    int highestRank = 0;
    for (int p = 0; p < PLAYERS; p++) if (players[p].rank > highestRank) highestRank = players[p].rank;
    /* Ties count as wins for each player. */
    for (int p = 0; p < PLAYERS; p++) if (players[p].rank == highestRank) players[p].wins++;
  }
}

static const struct { int wins; int handTypeCounts[HAND_TYPES]; } expectations[PLAYERS] = {
  {60120, {120600, 102000, 10080, 5040, 1440, 360, 480, 0, 0, 0}},
  {60480, {120360, 99600, 11760, 6120, 1200, 360, 480, 120, 0, 0}},
  {61440, {121200, 99720, 11040, 6120, 1080, 480, 360, 0, 0, 0}},
  {57960, {121320, 100440, 11040, 5760, 840, 480, 120, 0, 0, 0}},
};

static void validate(const Player *players, int iterations) {
  double factor = (double)iterations / DEFAULT_ITERATIONS;
  for (int p = 0; p < PLAYERS; p++) {
    if (players[p].wins != expectations[p].wins * factor) {
      fprintf(stderr, "Expected %s to have %g, but they have %d\n", players[p].name, expectations[p].wins * factor, players[p].wins);
      exit(1);
    }
    for (int t = 0; t < HAND_TYPES; t++) {
      if (players[p].handTypeCounts[t] != expectations[p].handTypeCounts[t] * factor) {
        fprintf(stderr, "Expected %s to have %g hands of type %d, but they have %d\n", players[p].name,
                expectations[p].handTypeCounts[t] * factor, t, players[p].handTypeCounts[t]);
        exit(1);
      }
    }
  }
}

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : DEFAULT_ITERATIONS;
  Player players[PLAYERS] = {{"Player 1"}, {"Player 2"}, {"Player 3"}, {"Player 4"}};
  struct timespec a, b;
  clock_gettime(CLOCK_MONOTONIC, &a);
  for (int i = 0; i < iterations; i++) {
    seed = INITIAL_SEED;
    play_hands(players);
  }
  clock_gettime(CLOCK_MONOTONIC, &b);
  validate(players, iterations);
  printf("UniPoker %d iterations %.3f ms\n", iterations, (b.tv_sec - a.tv_sec) * 1e3 + (b.tv_nsec - a.tv_nsec) / 1e6);
  return 0;
}
