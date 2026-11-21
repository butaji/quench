// jetstream-row: transformersjs-bert-wasm
// jetstream-files: utils/polyfills/fast-text-encoding/1.0.3/text.js transformersjs/benchmark.js transformersjs/task-bert.js
// iterations: 30
// jetstream-driver: class=Async preload=transformersJsModule=transformersjs/build/transformers.js,onnxJsModule=transformersjs/build/onnxruntime-web/ort-wasm-simd-threaded.mjs,onnxWasmBinary=transformersjs/build/onnxruntime-web/ort-wasm-simd-threaded.wasm,modelWeights=transformersjs/build/models/Xenova/distilbert-base-uncased-finetuned-sst-2-english/onnx/model_uint8.onnx,modelConfig=transformersjs/build/models/Xenova/distilbert-base-uncased-finetuned-sst-2-english/config.json,modelTokenizer=transformersjs/build/models/Xenova/distilbert-base-uncased-finetuned-sst-2-english/tokenizer.json,modelTokenizerConfig=transformersjs/build/models/Xenova/distilbert-base-uncased-finetuned-sst-2-english/tokenizer_config.json
// c-sources: ../c-analogues/support/bert/onnx.c ../c-analogues/support/bert/tokenizer.c -I../c-analogues/support/bert -lm

/*
 * C analogue of the transformersjs-bert-wasm row (JetStream transformersjs/).
 *
 * The row runs Transformers.js sentiment analysis with the quantized DistilBERT
 * SST-2 model (Xenova/distilbert-base-uncased-finetuned-sst-2-english, uint8)
 * on two inputs. This program does the same work in plain C:
 *   - init (first iteration, like the row's pipeline creation): read tokenizer.json
 *     and config.json (id2label), parse the ONNX protobuf of model_uint8.onnx and
 *     build the graph (support/bert/onnx.c, support/bert/tokenizer.c);
 *   - each iteration (doTask): tokenize both inputs as one padded batch with
 *     attention mask, run the graph with the ONNX interpreter, softmax the logits,
 *     pick the label from id2label.
 * Nothing about the inputs or outputs is precomputed: every iteration runs the
 * full tokenizer and the full graph. Expected values are only used by the
 * validation at the end.
 *
 * Usage: transformersjs-bert-wasm [iterations] [jetstream-root]
 */
#define _POSIX_C_SOURCE 200809L

#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "onnx.h"
#include "tokenizer.h"

#define ITERATIONS_DEFAULT 30
#define MODEL_DIR "/transformersjs/build/models/Xenova/distilbert-base-uncased-finetuned-sst-2-english"
#define NUM_INPUTS 2

static const char *const INPUTS[NUM_INPUTS] = {
  "I love transformers!",
  "Benchmarking is hard.",
};

/* Expected values from onnxruntime on the same model (see validation below). */
static const int64_t REF_IDS[NUM_INPUTS][8] = {
  {101, 1045, 2293, 19081, 999, 102, 0, 0},
  {101, 6847, 10665, 2075, 2003, 2524, 1012, 102},
};
static const float REF_LOGITS[NUM_INPUTS][2] = {
  /* onnxruntime 1.x with graph optimizations disabled (ORT_DISABLE_ALL), i.e. the graph as written,
   * op by op. ORT's default fused kernels give -4.1104/4.4143 and 4.1457/-3.4005: summation order
   * flips one uint8 LSB in a layer-3 DynamicQuantizeLinear, which moves logits by ~0.05. */
  {-4.100876808166504f, 4.414535999298096f},
  {4.096129894256592f, -3.37665057182312f},
};
static const size_t REF_SEQ_LEN = 8;
#define LOGIT_TOL 0.001f
#define SCORE_MIN 0.9f

typedef struct pipeline {
  bert_tokenizer *tok;
  onnx_session *sess;
  bert_labels labels;
} pipeline;

typedef struct task_result {
  size_t n;
  int label[NUM_INPUTS];   /* index into labels */
  float score[NUM_INPUTS];
  float logits[NUM_INPUTS][2];
  size_t seq_len;
  int64_t *ids;            /* n * seq_len, last iteration only */
} task_result;

static void pipeline_free(pipeline *p) {
  onnx_session_free(p->sess);
  bert_tokenizer_free(p->tok);
  bert_labels_free(&p->labels);
  memset(p, 0, sizeof *p);
}

static void task_result_free(task_result *r) {
  free(r->ids);
  memset(r, 0, sizeof *r);
}

static int path_join(char *dst, size_t cap, const char *root, const char *rel) {
  int n = snprintf(dst, cap, "%s%s%s", root, MODEL_DIR, rel);
  return (n < 0 || (size_t)n >= cap) ? -1 : 0;
}

/* First-iteration init: the row's initPipeline(). */
static int pipeline_init(pipeline *p, const char *root, char *err, size_t errlen) {
  char path[4096];
  if (path_join(path, sizeof path, root, "/tokenizer.json")) { snprintf(err, errlen, "path too long"); return -1; }
  if (bert_tokenizer_load(path, &p->tok, err, errlen)) return -1;
  if (path_join(path, sizeof path, root, "/config.json")) { snprintf(err, errlen, "path too long"); return -1; }
  if (bert_load_id2label(path, &p->labels, err, errlen)) return -1;
  if (path_join(path, sizeof path, root, "/onnx/model_uint8.onnx")) { snprintf(err, errlen, "path too long"); return -1; }
  if (onnx_session_load(path, &p->sess, err, errlen)) return -1;
  return 0;
}

/* One iteration: the row's doTask() on the two inputs. */
static int do_task(pipeline *p, task_result *r, char *err, size_t errlen) {
  int rc = -1;
  int64_t *ids = NULL, *mask = NULL;
  size_t L = 0;
  onnx_tensor tin = {0}, tmask = {0}, tout = {0};
  memset(r, 0, sizeof *r);

  if (bert_tokenizer_encode_batch(p->tok, INPUTS, NUM_INPUTS, &ids, &mask, &L, err, errlen)) goto done;
  int64_t dims[2] = {NUM_INPUTS, (int64_t)L};
  if (onnx_tensor_init(&tin, ONNX_INT64, 2, dims, err, errlen)) goto done;
  if (onnx_tensor_init(&tmask, ONNX_INT64, 2, dims, err, errlen)) goto done;
  memcpy(tin.data, ids, NUM_INPUTS * L * sizeof(int64_t));
  memcpy(tmask.data, mask, NUM_INPUTS * L * sizeof(int64_t));

  const char *names[2] = {"input_ids", "attention_mask"};
  const onnx_tensor *tensors[2] = {&tin, &tmask};
  if (onnx_session_run(p->sess, names, tensors, 2, "logits", &tout, err, errlen)) goto done;
  if (tout.dtype != ONNX_FLOAT || tout.ndim != 2 || tout.dims[0] != NUM_INPUTS || tout.dims[1] != 2) {
    snprintf(err, errlen, "unexpected logits shape");
    goto done;
  }

  const float *lg = tout.data;
  size_t nl = (size_t)tout.dims[1];
  r->n = NUM_INPUTS;
  for (size_t b = 0; b < NUM_INPUTS; b++) {
    /* Softmax over the label logits, then the top label (text-classification pipeline). */
    float mx = lg[b * nl];
    for (size_t k = 1; k < nl; k++) if (lg[b * nl + k] > mx) mx = lg[b * nl + k];
    float sum = 0.0f, prob[2] = {0};
    for (size_t k = 0; k < nl; k++) {
      prob[k] = expf(lg[b * nl + k] - mx);
      sum += prob[k];
    }
    size_t best = 0;
    for (size_t k = 0; k < nl; k++) {
      prob[k] /= sum;
      if (prob[k] > prob[best]) best = k;
    }
    r->label[b] = (int)best;
    r->score[b] = prob[best];
    r->logits[b][0] = lg[b * nl];
    r->logits[b][1] = lg[b * nl + 1];
  }
  r->seq_len = L;
  r->ids = malloc(NUM_INPUTS * L * sizeof(int64_t));
  if (!r->ids) { snprintf(err, errlen, "out of memory"); goto done; }
  memcpy(r->ids, ids, NUM_INPUTS * L * sizeof(int64_t));
  rc = 0;

done:
  onnx_tensor_free(&tin);
  onnx_tensor_free(&tmask);
  onnx_tensor_free(&tout);
  free(ids);
  free(mask);
  return rc;
}

static double now_ms(void) {
  struct timespec ts;
  clock_gettime(CLOCK_MONOTONIC, &ts);
  return (double)ts.tv_sec * 1e3 + (double)ts.tv_nsec / 1e6;
}

static const char *label_name(const pipeline *p, int id) {
  if ((size_t)id < p->labels.n && p->labels.names[id]) return p->labels.names[id];
  return "?";
}

int main(int argc, char **argv) {
  int iterations = ITERATIONS_DEFAULT;
  if (argc > 1) {
    char *end = NULL;
    long v = strtol(argv[1], &end, 10);
    if (!end || *end || v < 1 || v > 1000000) {
      fprintf(stderr, "usage: %s [iterations] [jetstream-root]\n", argv[0]);
      return 2;
    }
    iterations = (int)v;
  }
  const char *root = argc > 2 ? argv[2] : ".";
  char err[512] = {0};

  pipeline p = {0};
  task_result last = {0};
  int have_last = 0;

  double t0 = now_ms();
  for (int it = 0; it < iterations; it++) {
    if (!p.sess) {
      if (pipeline_init(&p, root, err, sizeof err)) {
        fprintf(stderr, "transformersjs-bert-wasm: init failed: %s\n", err);
        pipeline_free(&p);
        return 1;
      }
    }
    if (have_last) task_result_free(&last);
    if (do_task(&p, &last, err, sizeof err)) {
      fprintf(stderr, "transformersjs-bert-wasm: iteration %d failed: %s\n", it, err);
      task_result_free(&last);
      pipeline_free(&p);
      return 1;
    }
    have_last = 1;
  }
  double t1 = now_ms();

  printf("transformersjs-bert-wasm: %.3f ms for %d iterations\n", t1 - t0, iterations);
  for (size_t b = 0; b < last.n; b++) {
    printf("input %zu \"%s\": label %s score %.6f logits [%.6f, %.6f]\n", b, INPUTS[b],
           label_name(&p, last.label[b]), last.score[b], last.logits[b][0], last.logits[b][1]);
  }

  /* Validation: row's checks (labels and score) plus pinning to the reference computation. */
  char why[256] = "";
  int ok = 1;
  double max_diff = 0.0;
  int ids_ok = last.seq_len == REF_SEQ_LEN;
  if (last.n != NUM_INPUTS) {
    ok = 0;
    snprintf(why, sizeof why, "expected %d outputs, got %zu", NUM_INPUTS, last.n);
  }
  if (ok && (strcmp(label_name(&p, last.label[0]), "POSITIVE") != 0 || last.score[0] < SCORE_MIN)) {
    ok = 0;
    snprintf(why, sizeof why, "input 0 expected POSITIVE >= %.2f, got %s %.4f", SCORE_MIN,
             label_name(&p, last.label[0]), last.score[0]);
  }
  if (ok && (strcmp(label_name(&p, last.label[1]), "NEGATIVE") != 0 || last.score[1] < SCORE_MIN)) {
    ok = 0;
    snprintf(why, sizeof why, "input 1 expected NEGATIVE >= %.2f, got %s %.4f", SCORE_MIN,
             label_name(&p, last.label[1]), last.score[1]);
  }
  for (size_t b = 0; ok && b < NUM_INPUTS; b++) {
    for (size_t k = 0; k < 2; k++) {
      double d = last.logits[b][k] - REF_LOGITS[b][k];
      if (d < 0) d = -d;
      if (d > max_diff) max_diff = d;
      if (d > LOGIT_TOL) {
        ok = 0;
        snprintf(why, sizeof why, "logit[%zu][%zu] = %.6f, reference %.6f", b, k,
                 last.logits[b][k], REF_LOGITS[b][k]);
      }
    }
  }
  if (ids_ok) {
    for (size_t b = 0; b < NUM_INPUTS; b++)
      for (size_t j = 0; j < REF_SEQ_LEN; j++)
        if (last.ids[b * REF_SEQ_LEN + j] != REF_IDS[b][j]) ids_ok = 0;
  }
  if (ok && !ids_ok) {
    ok = 0;
    snprintf(why, sizeof why, "token ids differ from reference");
  }

  if (ok) {
    printf("validation: ok (POSITIVE %.4f, NEGATIVE %.4f; max |logit - reference| = %.6f < %.3f; token ids match reference)\n",
           last.score[0], last.score[1], max_diff, LOGIT_TOL);
  } else {
    printf("validation: FAILED (%s)\n", why);
  }

  task_result_free(&last);
  pipeline_free(&p);
  return ok ? 0 : 1;
}
