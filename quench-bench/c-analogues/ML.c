// jetstream-row: ML
// jetstream-files: ARES-6/ml/index.js ARES-6/ml/benchmark.js
// iterations: 60
/* C analogue of JetStream's ARES-6 ml benchmark: a feedforward neural network (the
 * ml/index.js FeedforwardNeuralNetwork over a row-major Matrix) trained by full-batch gradient
 * descent with softmax outputs, the five cases of ml/benchmark.js, and the export/import round trip
 * through JSON-like text. Like the JS, each runIteration trains 14 networks per case.
 * Note on activations: the JS constructor tests `this.activation in Object.keys(...)`, which is
 * always false, so every network uses tanh whatever activation name the benchmark passes. This
 * port keeps that behavior (and the 14-name loop) so the work matches.
 * Deviations: Math.random is replaced by a fixed-seed xorshift64*; libm tanh/exp can differ in the
 * last ulp from V8's, so the checksum is not bit-identical to JS; the Big case still asserts. */
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define NUM_ACTIVATIONS 14 /* keys of FeedforwardNeuralNetworksActivationFunctions */

typedef struct { int rows, cols; double *d; } Mat;
#define AT(m, i, j) ((m).d[(size_t)(i) * (size_t)(m).cols + (size_t)(j)])

static uint64_t rng_state = 0x9E3779B97F4A7C15ull;
static double rng_next(void) {
  rng_state ^= rng_state >> 12;
  rng_state ^= rng_state << 25;
  rng_state ^= rng_state >> 27;
  uint64_t r = rng_state * 2685821657736338717ull;
  return (double)(r >> 11) * (1.0 / 9007199254740992.0);
}

static Mat mat_new(int rows, int cols) {
  Mat m;
  m.rows = rows;
  m.cols = cols;
  size_t n = (size_t)rows * (size_t)cols;
  m.d = calloc(n ? n : 1, sizeof(double));
  if (!m.d) {
    fprintf(stderr, "out of memory\n");
    exit(2);
  }
  return m;
}

static void mat_free(Mat *m) {
  free(m->d);
  m->d = NULL;
  m->rows = m->cols = 0;
}

static Mat mat_clone(Mat a) {
  Mat m = mat_new(a.rows, a.cols);
  memcpy(m.d, a.d, sizeof(double) * (size_t)a.rows * (size_t)a.cols);
  return m;
}

static Mat mat_from(int rows, int cols, const double *src) {
  Mat m = mat_new(rows, cols);
  memcpy(m.d, src, sizeof(double) * (size_t)rows * (size_t)cols);
  return m;
}

/* C = A * B, summing over k in ascending order as Matrix.mmul does. */
static Mat mat_mmul(Mat a, Mat b) {
  Mat c = mat_new(a.rows, b.cols);
  for (int i = 0; i < a.rows; i++)
    for (int j = 0; j < b.cols; j++) {
      double s = 0;
      for (int k = 0; k < a.cols; k++) s += AT(a, i, k) * AT(b, k, j);
      AT(c, i, j) = s;
    }
  return c;
}

/* C = A^T * B (the JS transposeView().mmul). */
static Mat mat_mmul_at_b(Mat a, Mat b) {
  Mat c = mat_new(a.cols, b.cols);
  for (int i = 0; i < a.cols; i++)
    for (int j = 0; j < b.cols; j++) {
      double s = 0;
      for (int k = 0; k < a.rows; k++) s += AT(a, k, i) * AT(b, k, j);
      AT(c, i, j) = s;
    }
  return c;
}

/* C = A * B^T (the JS mmul(W.transposeView())). */
static Mat mat_mmul_a_bt(Mat a, Mat b) {
  Mat c = mat_new(a.rows, b.rows);
  for (int i = 0; i < a.rows; i++)
    for (int j = 0; j < b.rows; j++) {
      double s = 0;
      for (int k = 0; k < a.cols; k++) s += AT(a, i, k) * AT(b, j, k);
      AT(c, i, j) = s;
    }
  return c;
}

/* ---------- layers (Layer.js / OutputLayer.js) ---------- */

typedef struct {
  int in, out;
  double eps, reg;
  int output; /* 1 for OutputLayer: exp activation */
  Mat W, b, a, dW, db;
} Layer;

static Mat layer_forward(Layer *L, Mat X) {
  Mat z = mat_mmul(X, L->W);
  for (int i = 0; i < z.rows; i++)
    for (int j = 0; j < z.cols; j++) AT(z, i, j) += AT(L->b, 0, j);
  for (int i = 0; i < z.rows; i++)
    for (int j = 0; j < z.cols; j++) {
      double v = AT(z, i, j);
      AT(z, i, j) = L->output ? exp(v) : tanh(v);
    }
  mat_free(&L->a);
  L->a = mat_clone(z);
  return z;
}

/* Returns the delta for the previous layer. The derivative is 1 - a*a (tanh) on the input activations. */
static Mat layer_backprop(Layer *L, Mat delta, Mat a) {
  mat_free(&L->dW);
  L->dW = mat_mmul_at_b(a, delta);
  mat_free(&L->db);
  L->db = mat_new(1, delta.cols);
  for (int i = 0; i < delta.rows; i++)
    for (int j = 0; j < delta.cols; j++) AT(L->db, 0, j) += AT(delta, i, j);
  Mat aCopy = mat_clone(a);
  for (int i = 0; i < aCopy.rows; i++)
    for (int j = 0; j < aCopy.cols; j++) {
      double v = AT(aCopy, i, j);
      AT(aCopy, i, j) = 1 - v * v;
    }
  Mat t = mat_mmul_a_bt(delta, L->W);
  for (int i = 0; i < t.rows; i++)
    for (int j = 0; j < t.cols; j++) AT(t, i, j) = AT(t, i, j) * AT(aCopy, i, j);
  mat_free(&aCopy);
  return t;
}

static void layer_update(Layer *L) {
  for (int i = 0; i < L->dW.rows; i++)
    for (int j = 0; j < L->dW.cols; j++) AT(L->dW, i, j) = AT(L->dW, i, j) + AT(L->W, i, j) * L->reg;
  for (int i = 0; i < L->dW.rows; i++)
    for (int j = 0; j < L->dW.cols; j++) AT(L->dW, i, j) = AT(L->dW, i, j) * -L->eps;
  for (int i = 0; i < L->W.rows; i++)
    for (int j = 0; j < L->W.cols; j++) AT(L->W, i, j) = AT(L->W, i, j) + AT(L->dW, i, j);
  for (int j = 0; j < L->db.cols; j++) {
    AT(L->db, 0, j) = AT(L->db, 0, j) * -L->eps;
    AT(L->b, 0, j) = AT(L->b, 0, j) + AT(L->db, 0, j);
  }
}

/* Allocates zeroed parameters (the model-load path of Layer, which draws no random numbers). */
static void layer_alloc(Layer *L, int in, int out, int output, double eps, double reg) {
  L->in = in;
  L->out = out;
  L->eps = eps;
  L->reg = reg;
  L->output = output;
  L->W = mat_new(in, out);
  L->b = mat_new(1, out);
  L->a = (Mat){0, 0, NULL};
  L->dW = (Mat){0, 0, NULL};
  L->db = (Mat){0, 0, NULL};
}

/* Default constructor: random weights scaled by 1/sqrt(inputSize). */
static void layer_init(Layer *L, int in, int out, int output, double eps, double reg) {
  layer_alloc(L, in, out, output, eps, reg);
  for (int i = 0; i < in; i++)
    for (int j = 0; j < out; j++) AT(L->W, i, j) = rng_next();
  for (int i = 0; i < in; i++)
    for (int j = 0; j < out; j++) AT(L->W, i, j) /= sqrt((double)in);
}

static void layer_release(Layer *L) {
  mat_free(&L->W);
  mat_free(&L->b);
  mat_free(&L->a);
  mat_free(&L->dW);
  mat_free(&L->db);
}

/* ---------- feedforward network (FeedForwardNeuralNetwork.js) ---------- */

#define MAX_LABELS 8
#define MAX_HIDDEN 4

typedef struct {
  int nhidden, hidden[MAX_HIDDEN];
  int nlayers;
  Layer *layers;
  double learning_rate, regularization;
  int nclasses;
  int class_of_label[MAX_LABELS]; /* dictOutputs().inputs */
  int label_of_class[MAX_LABELS]; /* dictOutputs().outputs */
  int iterations;
} FNN;

static void fnn_build_dict(FNN *net, const int *labels, int n) {
  for (int i = 0; i < MAX_LABELS; i++) net->class_of_label[i] = -1;
  net->nclasses = 0;
  for (int i = 0; i < n; i++)
    if (net->class_of_label[labels[i]] < 0) {
      net->class_of_label[labels[i]] = net->nclasses;
      net->label_of_class[net->nclasses++] = labels[i];
    }
}

/* buildNetwork (random = 1) or the load path (random = 0) for the hidden and output layers. */
static void fnn_build_network(FNN *net, int input_size, int random) {
  net->nlayers = net->nhidden + 1;
  net->layers = calloc((size_t)net->nlayers, sizeof *net->layers);
  if (!net->layers) {
    fprintf(stderr, "out of memory\n");
    exit(2);
  }
  int in = input_size;
  for (int i = 0; i < net->nlayers; i++) {
    int out = i < net->nhidden ? net->hidden[i] : net->nclasses;
    int output = i == net->nlayers - 1;
    if (random)
      layer_init(&net->layers[i], in, out, output, net->learning_rate, net->regularization);
    else
      layer_alloc(&net->layers[i], in, out, output, net->learning_rate, net->regularization);
    in = out;
  }
}

static void fnn_free_network(FNN *net) {
  for (int i = 0; i < net->nlayers; i++) layer_release(&net->layers[i]);
  free(net->layers);
  net->layers = NULL;
  net->nlayers = 0;
}

/* Forward pass and row normalization: probabilities = output / rowSum(output). */
static Mat fnn_propagate(FNN *net, Mat X) {
  Mat input = X;
  int owned = 0;
  for (int i = 0; i < net->nlayers; i++) {
    Mat next = layer_forward(&net->layers[i], input);
    if (owned) mat_free(&input);
    input = next;
    owned = 1;
  }
  for (int i = 0; i < input.rows; i++) {
    double sum = 0;
    for (int j = 0; j < input.cols; j++) sum += AT(input, i, j);
    for (int j = 0; j < input.cols; j++) AT(input, i, j) = AT(input, i, j) / sum;
  }
  return input;
}

static void fnn_backprop(FNN *net, Mat X, const int *labels, Mat probs) {
  for (int i = 0; i < probs.rows; i++) AT(probs, i, net->class_of_label[labels[i]]) -= 1;
  Mat delta = probs;
  for (int i = net->nlayers - 1; i >= 0; i--) {
    Mat a = i > 0 ? net->layers[i - 1].a : X;
    Mat next = layer_backprop(&net->layers[i], delta, a);
    mat_free(&delta);
    delta = next;
  }
  mat_free(&delta);
  for (int i = 0; i < net->nlayers; i++) layer_update(&net->layers[i]);
}

static void fnn_train(FNN *net, Mat X, const int *labels, int n) {
  fnn_build_dict(net, labels, n);
  fnn_build_network(net, X.cols, 1);
  for (int it = 0; it < net->iterations; it++) {
    Mat probs = fnn_propagate(net, X);
    /* fnn_backprop takes ownership of probs (it becomes the first delta and is freed there). */
    fnn_backprop(net, X, labels, probs);
  }
}

/* predict(): label id of the first maximal probability in each row. */
static void fnn_predict(FNN *net, Mat X, int *out) {
  Mat probs = fnn_propagate(net, X);
  for (int i = 0; i < probs.rows; i++) {
    int idx = 0;
    double v = AT(probs, i, 0);
    for (int j = 1; j < probs.cols; j++)
      if (AT(probs, i, j) > v) {
        v = AT(probs, i, j);
        idx = j;
      }
    out[i] = net->label_of_class[idx];
  }
  mat_free(&probs);
}

/* toJSON: weights go out as text, one value per "%.17g", then JSON.parse(JSON.stringify()) is the
 * text round trip. */
static void fnn_export(const FNN *net, char **text) {
  size_t cap = 4096, n = 0;
  char *buf = malloc(cap);
  if (!buf) {
    fprintf(stderr, "out of memory\n");
    exit(2);
  }
  for (int l = 0; l < net->nlayers; l++) {
    const Mat *mats[2] = {&net->layers[l].W, &net->layers[l].b};
    for (int m = 0; m < 2; m++)
      for (int k = 0; k < mats[m]->rows * mats[m]->cols; k++) {
        if (cap - n < 32) {
          cap *= 2;
          char *p = realloc(buf, cap);
          if (!p) {
            fprintf(stderr, "out of memory\n");
            exit(2);
          }
          buf = p;
        }
        n += (size_t)sprintf(buf + n, "%.17g,", mats[m]->d[k]);
      }
  }
  *text = buf;
}

/* FeedForwardNeuralNetwork.load: same shapes and hyperparameters, weights read back from text. */
static void fnn_import(FNN *dst, const FNN *src, const char *text) {
  *dst = *src;
  fnn_build_network(dst, src->layers[0].in, 0);
  const char *p = text;
  for (int l = 0; l < dst->nlayers; l++) {
    Layer *L = &dst->layers[l];
    for (int k = 0; k < L->W.rows * L->W.cols; k++) {
      char *end;
      L->W.d[k] = strtod(p, &end);
      p = end + 1;
    }
    for (int k = 0; k < L->b.rows * L->b.cols; k++) {
      char *end;
      L->b.d[k] = strtod(p, &end);
      p = end + 1;
    }
  }
}

/* ---------- benchmark cases (ml/benchmark.js) ---------- */

static uint64_t checksum = 1469598103934665603ull;
static void mix(int label) {
  checksum ^= (uint64_t)(label + 1);
  checksum *= 1099511628211ull;
}

static FNN make_net(const int *hidden, int nhidden, int iterations, double lr) {
  FNN net;
  memset(&net, 0, sizeof net);
  net.nhidden = nhidden;
  memcpy(net.hidden, hidden, sizeof(int) * (size_t)nhidden);
  net.iterations = iterations;
  net.learning_rate = lr;
  net.regularization = 0.01;
  return net;
}

static const double XOR_X[8] = {0, 0, 0, 1, 1, 0, 1, 1};
static const double BIG_X[32] = {1, 1, 1, 2, 2, 1, 2, 2, 3, 1, 1, 3, 1, 4, 4, 1,
                                 6, 1, 6, 2, 6, 3, 6, 4, 6, 5, 5, 5, 4, 5, 3, 5};

static void case_xor(void) {
  static const int labels[4] = {0, 1, 1, 0}; /* false, true, true, false */
  static const int hidden[1] = {4};
  Mat X = mat_from(4, 2, XOR_X);
  int pred[4];
  for (int f = 0; f < NUM_ACTIVATIONS; f++) {
    FNN net = make_net(hidden, 1, 40, 0.3);
    fnn_train(&net, X, labels, 4);
    fnn_predict(&net, X, pred);
    for (int i = 0; i < 4; i++) mix(pred[i]);
    fnn_free_network(&net);
  }
  mat_free(&X);
}

static void case_and(void) {
  static const int labels[4] = {0, 0, 0, 1}; /* [1,0] three times, then [0,1] */
  static const int hidden[1] = {3};
  Mat X = mat_from(4, 2, XOR_X);
  int pred[4];
  for (int f = 0; f < NUM_ACTIVATIONS; f++) {
    FNN net = make_net(hidden, 1, 75, 0.3);
    fnn_train(&net, X, labels, 4);
    fnn_predict(&net, X, pred);
    for (int i = 0; i < 4; i++) mix(pred[i]);
    fnn_free_network(&net);
  }
  mat_free(&X);
}

static void case_export_import(void) {
  static const int labels[4] = {0, 1, 1, 1};
  static const int hidden[1] = {4};
  Mat X = mat_from(4, 2, XOR_X);
  int pred[4];
  for (int f = 0; f < NUM_ACTIVATIONS; f++) {
    FNN net = make_net(hidden, 1, 40, 0.3);
    fnn_train(&net, X, labels, 4);
    char *text;
    fnn_export(&net, &text);
    FNN loaded;
    fnn_import(&loaded, &net, text);
    fnn_predict(&loaded, X, pred);
    for (int i = 0; i < 4; i++) mix(pred[i]);
    free(text);
    fnn_free_network(&loaded);
    fnn_free_network(&net);
  }
  mat_free(&X);
}

static void case_multiclass(void) {
  static const int labels[4] = {2, 0, 1, 0};
  static const int hidden[1] = {4};
  Mat X = mat_from(4, 2, XOR_X);
  int pred[4];
  for (int f = 0; f < NUM_ACTIVATIONS; f++) {
    FNN net = make_net(hidden, 1, 40, 0.5);
    fnn_train(&net, X, labels, 4);
    fnn_predict(&net, X, pred);
    for (int i = 0; i < 4; i++) mix(pred[i]);
    fnn_free_network(&net);
  }
  mat_free(&X);
}

static void case_big(void) {
  static int labels[16];
  static const int hidden[1] = {20};
  static const double query[2] = {5, 4};
  Mat X = mat_from(16, 2, BIG_X);
  for (int i = 0; i < 16; i++) labels[i] = i < 8 ? 0 : 1; /* [1,0] for the first eight rows */
  for (int f = 0; f < NUM_ACTIVATIONS; f++) {
    FNN net = make_net(hidden, 1, 60, 0.01);
    fnn_train(&net, X, labels, 16);
    Mat q = mat_from(1, 2, query);
    int pred[1];
    fnn_predict(&net, q, pred);
    /* assert(result[0][0] < result[0][1]): the predicted label must be [0,1], id 1 */
    if (pred[0] != 1) {
      printf("validation FAILED: big case predicted label %d, expected 1\n", pred[0]);
      exit(1);
    }
    mix(pred[0]);
    mat_free(&q);
    fnn_free_network(&net);
  }
  mat_free(&X);
}

static void run_iteration(void) {
  case_xor();
  case_and();
  case_export_import();
  case_multiclass();
  case_big();
}

static double now_ms(void) {
  struct timespec ts;
  timespec_get(&ts, TIME_UTC);
  return ts.tv_sec * 1e3 + ts.tv_nsec / 1e6;
}

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : 60;
  double t0 = now_ms();
  for (int i = 0; i < iterations; i++) run_iteration();
  double t1 = now_ms();
  printf("ML %d iterations %.3f ms\n", iterations, t1 - t0);
  printf("checksum %016llx\n", (unsigned long long)checksum);
  printf("validation OK: big case predicts [0,1] on every iteration\n");
  return 0;
}
