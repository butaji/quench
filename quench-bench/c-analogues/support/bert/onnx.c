/*
 * Minimal ONNX interpreter. See onnx.h.
 *
 * Assumes a little-endian host (raw_data is copied verbatim into typed
 * buffers), which holds for x86-64 and wasm32.
 */
#define _POSIX_C_SOURCE 200809L

#include <math.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "onnx.h"

#define MAX_IO 16

#define FAIL(...)                       \
  do {                                  \
    set_err(err, errlen, __VA_ARGS__);  \
    return -1;                          \
  } while (0)

static void set_err(char *err, size_t errlen, const char *fmt, ...) {
  if (!err || errlen == 0) return;
  va_list ap;
  va_start(ap, fmt);
  vsnprintf(err, errlen, fmt, ap);
  va_end(ap);
}

/* ------------------------------------------------------------------------ */
/* Tensors                                                                  */
/* ------------------------------------------------------------------------ */

size_t onnx_dtype_size(onnx_dtype t) {
  switch (t) {
    case ONNX_FLOAT:
    case ONNX_INT32:
      return 4;
    case ONNX_INT64:
      return 8;
    case ONNX_UINT8:
    case ONNX_INT8:
    case ONNX_BOOL:
      return 1;
  }
  return 0;
}

static size_t numel(int nd, const int64_t *dims) {
  size_t n = 1;
  for (int i = 0; i < nd; i++) n *= (size_t)dims[i];
  return n;
}

int onnx_tensor_init(onnx_tensor *t, onnx_dtype dt, int ndim, const int64_t *dims,
                     char *err, size_t errlen) {
  size_t es = onnx_dtype_size(dt);
  if (!es) FAIL("unsupported dtype %d", (int)dt);
  if (ndim < 0 || ndim > ONNX_MAX_DIMS) FAIL("unsupported rank %d", ndim);
  memset(t, 0, sizeof *t);
  t->dtype = dt;
  t->ndim = ndim;
  for (int i = 0; i < ndim; i++) {
    if (dims[i] < 0) FAIL("negative dimension");
    t->dims[i] = dims[i];
  }
  t->count = numel(ndim, dims);
  t->data = calloc(t->count ? t->count : 1, es);
  if (!t->data) FAIL("out of memory");
  return 0;
}

void onnx_tensor_free(onnx_tensor *t) {
  if (!t) return;
  free(t->data);
  memset(t, 0, sizeof *t);
}

static int tensor_copy(onnx_tensor *dst, const onnx_tensor *src, char *err, size_t errlen) {
  if (onnx_tensor_init(dst, src->dtype, src->ndim, src->dims, err, errlen)) return -1;
  memcpy(dst->data, src->data, src->count * onnx_dtype_size(src->dtype));
  return 0;
}

/* Element read as int64 (integer and bool dtypes; floats are truncated). */
static int64_t elem_i(const onnx_tensor *t, size_t i) {
  switch (t->dtype) {
    case ONNX_FLOAT:
      return (int64_t)((const float *)t->data)[i];
    case ONNX_INT32:
      return ((const int32_t *)t->data)[i];
    case ONNX_INT64:
      return ((const int64_t *)t->data)[i];
    case ONNX_UINT8:
      return ((const uint8_t *)t->data)[i];
    case ONNX_INT8:
      return ((const int8_t *)t->data)[i];
    case ONNX_BOOL:
      return ((const uint8_t *)t->data)[i] != 0;
  }
  return 0;
}

static void contig_strides(int nd, const int64_t *dims, int64_t *st) {
  int64_t s = 1;
  for (int i = nd - 1; i >= 0; i--) {
    st[i] = s;
    s *= dims[i];
  }
}

/* Numpy-style broadcast of two shapes (right-aligned). */
static int bcast_dims(int na, const int64_t *da, int nb, const int64_t *db,
                      int *no, int64_t *dout, char *err, size_t errlen) {
  int nd = na > nb ? na : nb;
  if (nd > ONNX_MAX_DIMS) FAIL("broadcast rank too large");
  *no = nd;
  for (int i = 0; i < nd; i++) {
    int ia = i - (nd - na), ib = i - (nd - nb);
    int64_t a = ia >= 0 ? da[ia] : 1;
    int64_t b = ib >= 0 ? db[ib] : 1;
    if (a == b || b == 1) dout[i] = a;
    else if (a == 1) dout[i] = b;
    else FAIL("shapes cannot be broadcast together");
  }
  return 0;
}

/* Element strides of tensor t viewed as shape odims (broadcast dims get 0). */
static void bcast_strides(const onnx_tensor *t, int nd, const int64_t *odims, int64_t *st) {
  (void)odims;
  int64_t cs[ONNX_MAX_DIMS];
  contig_strides(t->ndim, t->dims, cs);
  for (int i = 0; i < nd; i++) {
    int ti = i - (nd - t->ndim);
    st[i] = (ti < 0 || t->dims[ti] == 1) ? 0 : cs[ti];
  }
}

/* off[o] = base + sum_d idx_d * st[d], with idx in row-major order over dims. */
static void strided_offsets(int nd, const int64_t *dims, const int64_t *st, int64_t base,
                            size_t n, int64_t *off) {
  int64_t idx[ONNX_MAX_DIMS] = {0};
  int64_t cur = base;
  for (size_t o = 0; o < n; o++) {
    off[o] = cur;
    for (int d = nd - 1; d >= 0; d--) {
      idx[d]++;
      cur += st[d];
      if (idx[d] < dims[d]) break;
      cur -= st[d] * dims[d];
      idx[d] = 0;
    }
  }
}

static int64_t *alloc_offsets(size_t n) {
  return malloc(sizeof(int64_t) * (n ? n : 1));
}

/* Copies elements: dst[o] = src[off[o]] with element size es. */
static void gather_elems(void *dst, const void *src, const int64_t *off, size_t n, size_t es) {
  const char *s = src;
  char *d = dst;
  for (size_t o = 0; o < n; o++) memcpy(d + o * es, s + (size_t)off[o] * es, es);
}

/* ------------------------------------------------------------------------ */
/* Protobuf wire format                                                     */
/* ------------------------------------------------------------------------ */

typedef struct {
  const uint8_t *p, *end;
} pbr;

static int pb_varint(pbr *r, uint64_t *v, char *err, size_t errlen) {
  uint64_t x = 0;
  int shift = 0;
  for (;;) {
    if (r->p >= r->end) FAIL("protobuf: truncated varint");
    uint8_t b = *r->p++;
    if (shift < 64) x |= (uint64_t)(b & 0x7f) << shift;
    if (!(b & 0x80)) break;
    shift += 7;
    if (shift > 63 + 7) FAIL("protobuf: varint too long");
  }
  *v = x;
  return 0;
}

static int pb_key(pbr *r, uint32_t *field, uint32_t *wt, char *err, size_t errlen) {
  uint64_t k;
  if (pb_varint(r, &k, err, errlen)) return -1;
  *field = (uint32_t)(k >> 3);
  *wt = (uint32_t)(k & 7);
  return 0;
}

/* Length-delimited payload. */
static int pb_bytes(pbr *r, const uint8_t **p, size_t *n, char *err, size_t errlen) {
  uint64_t len;
  if (pb_varint(r, &len, err, errlen)) return -1;
  if (len > (uint64_t)(r->end - r->p)) FAIL("protobuf: length past end");
  *p = r->p;
  *n = (size_t)len;
  r->p += len;
  return 0;
}

static int pb_fixed32(pbr *r, uint32_t *v, char *err, size_t errlen) {
  if (r->end - r->p < 4) FAIL("protobuf: truncated fixed32");
  const uint8_t *q = r->p;
  *v = (uint32_t)q[0] | ((uint32_t)q[1] << 8) | ((uint32_t)q[2] << 16) | ((uint32_t)q[3] << 24);
  r->p += 4;
  return 0;
}

static int pb_skip(pbr *r, uint32_t wt, char *err, size_t errlen) {
  uint64_t v;
  const uint8_t *p;
  size_t n;
  switch (wt) {
    case 0:
      return pb_varint(r, &v, err, errlen);
    case 1:
      if (r->end - r->p < 8) FAIL("protobuf: truncated fixed64");
      r->p += 8;
      return 0;
    case 2:
      return pb_bytes(r, &p, &n, err, errlen);
    case 5: {
      uint32_t bits;
      return pb_fixed32(r, &bits, err, errlen);
    }
  }
  FAIL("protobuf: unsupported wire type %u", (unsigned)wt);
}

static char *dup_n(const uint8_t *p, size_t n) {
  char *s = malloc(n + 1);
  if (!s) return NULL;
  memcpy(s, p, n);
  s[n] = 0;
  return s;
}

/* Appends to a growable array. Returns pointer to a zeroed new element. */
static void *arr_push(void **arr, size_t *n, size_t *cap, size_t es) {
  if (*n == *cap) {
    size_t nc = *cap ? *cap * 2 : 8;
    void *p = realloc(*arr, nc * es);
    if (!p) return NULL;
    memset((char *)p + *cap * es, 0, (nc - *cap) * es);
    *arr = p;
    *cap = nc;
  }
  void *e = (char *)*arr + *n * es;
  (*n)++;
  return e;
}

/* Repeated int64 field: unpacked varints (wt 0) or one packed run (wt 2). */
static int pb_read_i64s(pbr *r, uint32_t wt, int64_t **arr, size_t *n, size_t *cap,
                        char *err, size_t errlen) {
  if (wt == 0) {
    uint64_t v;
    if (pb_varint(r, &v, err, errlen)) return -1;
    int64_t *e = arr_push((void **)arr, n, cap, sizeof(int64_t));
    if (!e) FAIL("out of memory");
    *e = (int64_t)v;
    return 0;
  }
  if (wt == 2) {
    const uint8_t *p;
    size_t len;
    if (pb_bytes(r, &p, &len, err, errlen)) return -1;
    pbr sub = {p, p + len};
    while (sub.p < sub.end) {
      uint64_t v;
      if (pb_varint(&sub, &v, err, errlen)) return -1;
      int64_t *e = arr_push((void **)arr, n, cap, sizeof(int64_t));
      if (!e) FAIL("out of memory");
      *e = (int64_t)v;
    }
    return 0;
  }
  FAIL("protobuf: bad wire type for int list");
}

/* Repeated float field: unpacked fixed32 (wt 5) or packed run (wt 2). */
static int pb_read_floats(pbr *r, uint32_t wt, float **arr, size_t *n, size_t *cap,
                          char *err, size_t errlen) {
  if (wt == 5) {
    uint32_t bits;
    if (pb_fixed32(r, &bits, err, errlen)) return -1;
    float *e = arr_push((void **)arr, n, cap, sizeof(float));
    if (!e) FAIL("out of memory");
    memcpy(e, &bits, 4);
    return 0;
  }
  if (wt == 2) {
    const uint8_t *p;
    size_t len;
    if (pb_bytes(r, &p, &len, err, errlen)) return -1;
    if (len % 4) FAIL("protobuf: packed float length");
    for (size_t i = 0; i < len; i += 4) {
      uint32_t bits = (uint32_t)p[i] | ((uint32_t)p[i + 1] << 8) | ((uint32_t)p[i + 2] << 16) |
                      ((uint32_t)p[i + 3] << 24);
      float *e = arr_push((void **)arr, n, cap, sizeof(float));
      if (!e) FAIL("out of memory");
      memcpy(e, &bits, 4);
    }
    return 0;
  }
  FAIL("protobuf: bad wire type for float list");
}

/* ------------------------------------------------------------------------ */
/* Model structures produced by the parser                                  */
/* ------------------------------------------------------------------------ */

/* AttributeProto.AttributeType values used here. */
enum { ATTR_FLOAT = 1, ATTR_INT = 2, ATTR_TENSOR = 4, ATTR_INTS = 7 };

typedef struct onnx_attr {
  char *name;
  int type;
  float f;
  int64_t i;
  int64_t *ints;
  size_t n_ints;
  onnx_tensor t;
} onnx_attr;

enum {
  OP_CONSTANT, OP_SHAPE, OP_GATHER, OP_UNSQUEEZE, OP_CONCAT, OP_RESHAPE, OP_TRANSPOSE,
  OP_CAST, OP_ADD, OP_SUB, OP_MUL, OP_DIV, OP_POW, OP_SQRT, OP_ERF, OP_RELU, OP_EQUAL,
  OP_WHERE, OP_EXPAND, OP_REDUCEMEAN, OP_SOFTMAX, OP_MATMUL, OP_MATMULINTEGER,
  OP_DYNAMICQUANTIZELINEAR, OP_DEQUANTIZELINEAR, OP_SLICE
};

static const struct {
  const char *name;
  int code;
} op_table[] = {
  {"Constant", OP_CONSTANT}, {"Shape", OP_SHAPE}, {"Gather", OP_GATHER},
  {"Unsqueeze", OP_UNSQUEEZE}, {"Concat", OP_CONCAT}, {"Reshape", OP_RESHAPE},
  {"Transpose", OP_TRANSPOSE}, {"Cast", OP_CAST}, {"Add", OP_ADD}, {"Sub", OP_SUB},
  {"Mul", OP_MUL}, {"Div", OP_DIV}, {"Pow", OP_POW}, {"Sqrt", OP_SQRT}, {"Erf", OP_ERF},
  {"Relu", OP_RELU}, {"Equal", OP_EQUAL}, {"Where", OP_WHERE}, {"Expand", OP_EXPAND},
  {"ReduceMean", OP_REDUCEMEAN}, {"Softmax", OP_SOFTMAX}, {"MatMul", OP_MATMUL},
  {"MatMulInteger", OP_MATMULINTEGER}, {"DynamicQuantizeLinear", OP_DYNAMICQUANTIZELINEAR},
  {"DequantizeLinear", OP_DEQUANTIZELINEAR}, {"Slice", OP_SLICE},
};

typedef struct onnx_node {
  char *op;
  int code;
  char **in_names;
  size_t n_in;
  char **out_names;
  size_t n_out;
  int *in_ids;
  int *out_ids;
  onnx_attr *attrs;
  size_t n_attrs;
  int *frees;       /* value ids to free after this node */
  size_t n_frees;
} onnx_node;

typedef struct named_tensor {
  char *name;
  onnx_tensor t;
} named_tensor;

typedef struct parsed {
  onnx_node *nodes;
  size_t n_nodes, cap_nodes;
  named_tensor *inits;
  size_t n_inits, cap_inits;
  char **inputs;
  size_t n_inputs, cap_inputs;
  char **outputs;
  size_t n_outputs, cap_outputs;
  int have_graph;
} parsed;

typedef struct namemap {
  char **keys;
  int *vals;
  size_t cap;
} namemap;

typedef struct onnx_slot {
  onnx_tensor t;
  int live;
  int is_init;
  int is_input;
  int is_output;
} onnx_slot;

struct onnx_session {
  onnx_node *nodes;
  size_t n_nodes;
  onnx_slot *slots;
  size_t n_vals;
  namemap map;
};

static void attr_free(onnx_attr *a) {
  free(a->name);
  free(a->ints);
  onnx_tensor_free(&a->t);
}

static void node_free(onnx_node *nd) {
  free(nd->op);
  for (size_t i = 0; i < nd->n_in; i++) free(nd->in_names[i]);
  free(nd->in_names);
  for (size_t i = 0; i < nd->n_out; i++) free(nd->out_names[i]);
  free(nd->out_names);
  free(nd->in_ids);
  free(nd->out_ids);
  for (size_t i = 0; i < nd->n_attrs; i++) attr_free(&nd->attrs[i]);
  free(nd->attrs);
  free(nd->frees);
  memset(nd, 0, sizeof *nd);
}

static void parsed_free(parsed *pm) {
  for (size_t i = 0; i < pm->n_nodes; i++) node_free(&pm->nodes[i]);
  free(pm->nodes);
  for (size_t i = 0; i < pm->n_inits; i++) {
    free(pm->inits[i].name);
    onnx_tensor_free(&pm->inits[i].t);
  }
  free(pm->inits);
  for (size_t i = 0; i < pm->n_inputs; i++) free(pm->inputs[i]);
  free(pm->inputs);
  for (size_t i = 0; i < pm->n_outputs; i++) free(pm->outputs[i]);
  free(pm->outputs);
  memset(pm, 0, sizeof *pm);
}

/* ------------------------------------------------------------------------ */
/* Parser                                                                   */
/* ------------------------------------------------------------------------ */

typedef struct tparse {
  int64_t *dims;
  size_t nd, cap_d;
  int64_t *ivals;
  size_t ni, cap_i;
  float *fvals;
  size_t nf, cap_f;
  const uint8_t *raw;
  size_t raw_len;
  int dtype, loc;
  char *name;
} tparse;

static void tparse_free(tparse *tp) {
  free(tp->dims);
  free(tp->ivals);
  free(tp->fvals);
  free(tp->name);
}

static int parse_tensor(const uint8_t *buf, size_t len, named_tensor *out, char *err, size_t errlen) {
  pbr r = {buf, buf + len};
  tparse tp = {0};
  int rc = -1;
  while (r.p < r.end) {
    uint32_t field, wt;
    if (pb_key(&r, &field, &wt, err, errlen)) goto done;
    switch (field) {
      case 1:  /* dims */
        if (pb_read_i64s(&r, wt, &tp.dims, &tp.nd, &tp.cap_d, err, errlen)) goto done;
        break;
      case 2: {  /* data_type */
        uint64_t v;
        if (wt != 0 || pb_varint(&r, &v, err, errlen)) { set_err(err, errlen, "protobuf: data_type"); goto done; }
        tp.dtype = (int)v;
        break;
      }
      case 4:  /* float_data */
        if (pb_read_floats(&r, wt, &tp.fvals, &tp.nf, &tp.cap_f, err, errlen)) goto done;
        break;
      case 5:  /* int32_data */
      case 7:  /* int64_data */
        if (pb_read_i64s(&r, wt, &tp.ivals, &tp.ni, &tp.cap_i, err, errlen)) goto done;
        break;
      case 8: {  /* name */
        const uint8_t *p;
        size_t n;
        if (wt != 2 || pb_bytes(&r, &p, &n, err, errlen)) { set_err(err, errlen, "protobuf: name"); goto done; }
        free(tp.name);
        tp.name = dup_n(p, n);
        break;
      }
      case 9: {  /* raw_data */
        const uint8_t *p;
        size_t n;
        if (wt != 2 || pb_bytes(&r, &p, &n, err, errlen)) { set_err(err, errlen, "protobuf: raw_data"); goto done; }
        tp.raw = p;
        tp.raw_len = n;
        break;
      }
      case 14: {  /* data_location */
        uint64_t v;
        if (wt != 0 || pb_varint(&r, &v, err, errlen)) { set_err(err, errlen, "protobuf: data_location"); goto done; }
        tp.loc = (int)v;
        break;
      }
      default:
        if (pb_skip(&r, wt, err, errlen)) goto done;
    }
  }

  if (tp.loc == 1) { set_err(err, errlen, "external tensor data is not supported"); goto done; }
  onnx_dtype dt = (onnx_dtype)tp.dtype;
  size_t es = onnx_dtype_size(dt);
  if (!es) { set_err(err, errlen, "unsupported tensor dtype %d", tp.dtype); goto done; }
  if (tp.nd > ONNX_MAX_DIMS) { set_err(err, errlen, "tensor rank too large"); goto done; }
  if (onnx_tensor_init(&out->t, dt, (int)tp.nd, tp.dims, err, errlen)) goto done;
  size_t count = out->t.count;
  if (tp.raw) {
    if (tp.raw_len != count * es) { set_err(err, errlen, "raw_data size mismatch"); goto done; }
    memcpy(out->t.data, tp.raw, tp.raw_len);
  } else if (dt == ONNX_FLOAT && tp.nf == count) {
    memcpy(out->t.data, tp.fvals, count * sizeof(float));
  } else if (count == 0) {
    /* empty tensor: nothing to copy */
  } else if (tp.ni == count) {
    for (size_t i = 0; i < count; i++) {
      int64_t v = tp.ivals[i];
      switch (dt) {
        case ONNX_INT64: ((int64_t *)out->t.data)[i] = v; break;
        case ONNX_INT32: ((int32_t *)out->t.data)[i] = (int32_t)v; break;
        case ONNX_UINT8: ((uint8_t *)out->t.data)[i] = (uint8_t)v; break;
        case ONNX_INT8: ((int8_t *)out->t.data)[i] = (int8_t)v; break;
        case ONNX_BOOL: ((uint8_t *)out->t.data)[i] = v != 0; break;
        default: set_err(err, errlen, "typed data for dtype %d", tp.dtype); goto done;
      }
    }
  } else {
    set_err(err, errlen, "tensor has no usable data");
    goto done;
  }
  out->name = tp.name;
  tp.name = NULL;
  rc = 0;
done:
  if (rc) onnx_tensor_free(&out->t);
  tparse_free(&tp);
  return rc;
}

static int parse_attr(const uint8_t *buf, size_t len, onnx_attr *a, char *err, size_t errlen) {
  pbr r = {buf, buf + len};
  int has_f = 0, has_i = 0, has_t = 0, has_ints = 0;
  int type = 0;
  size_t cap_ints = 0;
  while (r.p < r.end) {
    uint32_t field, wt;
    if (pb_key(&r, &field, &wt, err, errlen)) return -1;
    const uint8_t *p;
    size_t n;
    switch (field) {
      case 1:
        if (pb_bytes(&r, &p, &n, err, errlen)) return -1;
        a->name = dup_n(p, n);
        break;
      case 2: {
        uint32_t bits;
        if (pb_fixed32(&r, &bits, err, errlen)) return -1;
        memcpy(&a->f, &bits, sizeof bits);
        has_f = 1;
        break;
      }
      case 3: {
        uint64_t v;
        if (pb_varint(&r, &v, err, errlen)) return -1;
        a->i = (int64_t)v;
        has_i = 1;
        break;
      }
      case 5: {
        if (pb_bytes(&r, &p, &n, err, errlen)) return -1;
        named_tensor nt = {0};
        if (parse_tensor(p, n, &nt, err, errlen)) return -1;
        free(nt.name);
        a->t = nt.t;
        has_t = 1;
        break;
      }
      case 8:
        if (pb_read_i64s(&r, wt, &a->ints, &a->n_ints, &cap_ints, err, errlen)) return -1;
        has_ints = 1;
        break;
      case 20: {
        uint64_t v;
        if (pb_varint(&r, &v, err, errlen)) return -1;
        type = (int)v;
        break;
      }
      default:
        if (pb_skip(&r, wt, err, errlen)) return -1;
    }
  }
  if (!type) type = has_t ? ATTR_TENSOR : has_ints ? ATTR_INTS : has_i ? ATTR_INT : has_f ? ATTR_FLOAT : 0;
  a->type = type;
  return 0;
}

static int push_name(char ***arr, size_t *n, size_t *cap, const uint8_t *p, size_t len) {
  char **e = arr_push((void **)arr, n, cap, sizeof(char *));
  if (!e) return -1;
  *e = dup_n(p, len);
  return *e ? 0 : -1;
}

static int parse_node(const uint8_t *buf, size_t len, onnx_node *nd, char *err, size_t errlen) {
  pbr r = {buf, buf + len};
  size_t cap_in = 0, cap_out = 0, cap_attr = 0;
  while (r.p < r.end) {
    uint32_t field, wt;
    if (pb_key(&r, &field, &wt, err, errlen)) return -1;
    const uint8_t *p;
    size_t n;
    switch (field) {
      case 1:
        if (wt != 2 || pb_bytes(&r, &p, &n, err, errlen)) FAIL("protobuf: node input");
        if (push_name(&nd->in_names, &nd->n_in, &cap_in, p, n)) FAIL("out of memory");
        break;
      case 2:
        if (wt != 2 || pb_bytes(&r, &p, &n, err, errlen)) FAIL("protobuf: node output");
        if (push_name(&nd->out_names, &nd->n_out, &cap_out, p, n)) FAIL("out of memory");
        break;
      case 4:
        if (wt != 2 || pb_bytes(&r, &p, &n, err, errlen)) FAIL("protobuf: op_type");
        free(nd->op);
        nd->op = dup_n(p, n);
        break;
      case 5: {
        if (wt != 2 || pb_bytes(&r, &p, &n, err, errlen)) FAIL("protobuf: attribute");
        onnx_attr *a = arr_push((void **)&nd->attrs, &nd->n_attrs, &cap_attr, sizeof(onnx_attr));
        if (!a) FAIL("out of memory");
        if (parse_attr(p, n, a, err, errlen)) return -1;
        break;
      }
      default:
        if (pb_skip(&r, wt, err, errlen)) return -1;
    }
  }
  if (!nd->op) FAIL("node without op_type");
  return 0;
}

static int parse_name_only(const uint8_t *buf, size_t len, char **name, char *err, size_t errlen) {
  pbr r = {buf, buf + len};
  while (r.p < r.end) {
    uint32_t field, wt;
    if (pb_key(&r, &field, &wt, err, errlen)) return -1;
    if (field == 1 && wt == 2) {
      const uint8_t *p;
      size_t n;
      if (pb_bytes(&r, &p, &n, err, errlen)) return -1;
      free(*name);
      *name = dup_n(p, n);
      if (!*name) FAIL("out of memory");
    } else if (pb_skip(&r, wt, err, errlen)) {
      return -1;
    }
  }
  return 0;
}

static int parse_graph(const uint8_t *buf, size_t len, parsed *pm, char *err, size_t errlen) {
  pbr r = {buf, buf + len};
  while (r.p < r.end) {
    uint32_t field, wt;
    if (pb_key(&r, &field, &wt, err, errlen)) return -1;
    const uint8_t *p;
    size_t n;
    if (field == 1 && wt == 2) {
      if (pb_bytes(&r, &p, &n, err, errlen)) return -1;
      onnx_node *nd = arr_push((void **)&pm->nodes, &pm->n_nodes, &pm->cap_nodes, sizeof(onnx_node));
      if (!nd || parse_node(p, n, nd, err, errlen)) return -1;
    } else if (field == 5 && wt == 2) {
      if (pb_bytes(&r, &p, &n, err, errlen)) return -1;
      named_tensor *nt = arr_push((void **)&pm->inits, &pm->n_inits, &pm->cap_inits, sizeof(named_tensor));
      if (!nt || parse_tensor(p, n, nt, err, errlen)) return -1;
    } else if ((field == 11 || field == 12) && wt == 2) {
      if (pb_bytes(&r, &p, &n, err, errlen)) return -1;
      char *name = NULL;
      if (parse_name_only(p, n, &name, err, errlen)) { free(name); return -1; }
      char ***arr = field == 11 ? &pm->inputs : &pm->outputs;
      size_t *cnt = field == 11 ? &pm->n_inputs : &pm->n_outputs;
      size_t *cap = field == 11 ? &pm->cap_inputs : &pm->cap_outputs;
      char **slot = arr_push((void **)arr, cnt, cap, sizeof(char *));
      if (!slot) { free(name); FAIL("out of memory"); }
      *slot = name;
    } else if (pb_skip(&r, wt, err, errlen)) {
      return -1;
    }
  }
  pm->have_graph = 1;
  return 0;
}

static int parse_model(const uint8_t *buf, size_t len, parsed *pm, char *err, size_t errlen) {
  pbr r = {buf, buf + len};
  while (r.p < r.end) {
    uint32_t field, wt;
    if (pb_key(&r, &field, &wt, err, errlen)) return -1;
    const uint8_t *p;
    size_t n;
    if (field == 7 && wt == 2) {
      if (pb_bytes(&r, &p, &n, err, errlen)) return -1;
      if (parse_graph(p, n, pm, err, errlen)) return -1;
    } else if (pb_skip(&r, wt, err, errlen)) {
      return -1;
    }
  }
  if (!pm->have_graph) FAIL("model has no graph");
  return 0;
}

/* ------------------------------------------------------------------------ */
/* Name table (name -> value id)                                            */
/* ------------------------------------------------------------------------ */

static uint32_t name_hash(const char *s) {
  uint32_t h = 2166136261u;
  for (; *s; s++) {
    h ^= (uint8_t)*s;
    h *= 16777619u;
  }
  return h;
}

static int namemap_init(namemap *m, size_t max_names) {
  size_t cap = 16;
  while (cap < 2 * max_names) cap <<= 1;
  m->cap = cap;
  m->keys = calloc(cap, sizeof(char *));
  m->vals = malloc(cap * sizeof(int));
  return (m->keys && m->vals) ? 0 : -1;
}

static void namemap_free(namemap *m) {
  for (size_t i = 0; i < m->cap; i++) free(m->keys ? m->keys[i] : NULL);
  free(m->keys);
  free(m->vals);
  memset(m, 0, sizeof *m);
}

static int namemap_find(const namemap *m, const char *name) {
  size_t i = name_hash(name) & (m->cap - 1);
  while (m->keys[i]) {
    if (strcmp(m->keys[i], name) == 0) return m->vals[i];
    i = (i + 1) & (m->cap - 1);
  }
  return -1;
}

/* Inserts a new name; returns 0, or -1 if the name already exists or on OOM. */
static int namemap_add(namemap *m, const char *name, int val) {
  size_t i = name_hash(name) & (m->cap - 1);
  while (m->keys[i]) {
    if (strcmp(m->keys[i], name) == 0) return -1;
    i = (i + 1) & (m->cap - 1);
  }
  m->keys[i] = malloc(strlen(name) + 1);
  if (!m->keys[i]) return -1;
  strcpy(m->keys[i], name);
  m->vals[i] = val;
  return 0;
}

/* ------------------------------------------------------------------------ */
/* Session construction                                                     */
/* ------------------------------------------------------------------------ */

static int find_op(const char *op) {
  for (size_t i = 0; i < sizeof op_table / sizeof op_table[0]; i++)
    if (strcmp(op_table[i].name, op) == 0) return op_table[i].code;
  return -1;
}

static int build_session(onnx_session *s, parsed *pm, char *err, size_t errlen) {
  size_t max_names = pm->n_inits + pm->n_inputs + 1;
  for (size_t i = 0; i < pm->n_nodes; i++) max_names += pm->nodes[i].n_out;
  if (namemap_init(&s->map, max_names)) FAIL("out of memory");
  s->slots = calloc(max_names, sizeof(onnx_slot));
  if (!s->slots) FAIL("out of memory");
  s->n_vals = max_names;
  size_t nv = 0;

  /* Initializers: values that are live from the start. */
  for (size_t k = 0; k < pm->n_inits; k++) {
    named_tensor *nt = &pm->inits[k];
    if (namemap_add(&s->map, nt->name, (int)nv)) FAIL("duplicate initializer %s", nt->name);
    onnx_slot *sl = &s->slots[nv++];
    sl->t = nt->t;
    memset(&nt->t, 0, sizeof nt->t);
    sl->live = 1;
    sl->is_init = 1;
  }
  /* Graph inputs that are not initializers. */
  for (size_t k = 0; k < pm->n_inputs; k++) {
    if (namemap_find(&s->map, pm->inputs[k]) >= 0) continue;
    if (namemap_add(&s->map, pm->inputs[k], (int)nv)) FAIL("duplicate input %s", pm->inputs[k]);
    s->slots[nv++].is_input = 1;
  }
  /* Node outputs (SSA: each defined once). */
  for (size_t i = 0; i < pm->n_nodes; i++) {
    onnx_node *nd = &pm->nodes[i];
    nd->out_ids = malloc(sizeof(int) * (nd->n_out ? nd->n_out : 1));
    if (!nd->out_ids) FAIL("out of memory");
    for (size_t j = 0; j < nd->n_out; j++) {
      if (!nd->out_names[j][0]) { nd->out_ids[j] = -1; continue; }
      if (namemap_add(&s->map, nd->out_names[j], (int)nv)) FAIL("value %s defined twice", nd->out_names[j]);
      nd->out_ids[j] = (int)nv++;
    }
  }
  /* Node inputs and op codes. */
  for (size_t i = 0; i < pm->n_nodes; i++) {
    onnx_node *nd = &pm->nodes[i];
    nd->code = find_op(nd->op);
    if (nd->code < 0) FAIL("unsupported operator %s", nd->op);
    nd->in_ids = malloc(sizeof(int) * (nd->n_in ? nd->n_in : 1));
    if (!nd->in_ids) FAIL("out of memory");
    for (size_t j = 0; j < nd->n_in; j++) {
      if (!nd->in_names[j][0]) { nd->in_ids[j] = -1; continue; }
      int v = namemap_find(&s->map, nd->in_names[j]);
      if (v < 0) FAIL("node %zu (%s) reads undefined value %s", i, nd->op, nd->in_names[j]);
      nd->in_ids[j] = v;
    }
  }

  /* Last use of each value; graph outputs are never freed. */
  int *last = malloc(sizeof(int) * (nv ? nv : 1));
  if (!last) FAIL("out of memory");
  for (size_t v = 0; v < nv; v++) last[v] = -1;
  for (size_t i = 0; i < pm->n_nodes; i++)
    for (size_t j = 0; j < pm->nodes[i].n_in; j++)
      if (pm->nodes[i].in_ids[j] >= 0) last[pm->nodes[i].in_ids[j]] = (int)i;
  for (size_t k = 0; k < pm->n_outputs; k++) {
    int v = namemap_find(&s->map, pm->outputs[k]);
    if (v < 0) FAIL("graph output %s is undefined", pm->outputs[k]);
    s->slots[v].is_output = 1;
  }
  for (size_t v = 0; v < nv; v++)
    if (s->slots[v].is_output) last[v] = (int)pm->n_nodes;

  /* Values to free after each node: last use reached, or never used (dead). */
  for (size_t i = 0; i < pm->n_nodes; i++) {
    onnx_node *nd = &pm->nodes[i];
    size_t cap = nd->n_in + nd->n_out;
    nd->frees = malloc(sizeof(int) * (cap ? cap : 1));
    if (!nd->frees) FAIL("out of memory");
    nd->n_frees = 0;
    for (size_t j = 0; j < nd->n_in; j++) {
      int v = nd->in_ids[j];
      if (v < 0 || s->slots[v].is_init || s->slots[v].is_output || last[v] != (int)i) continue;
      int dup = 0;
      for (size_t jj = 0; jj < j; jj++) dup |= nd->in_ids[jj] == v;
      if (!dup) nd->frees[nd->n_frees++] = v;
    }
    for (size_t j = 0; j < nd->n_out; j++) {
      int v = nd->out_ids[j];
      if (v >= 0 && !s->slots[v].is_output && last[v] == -1) nd->frees[nd->n_frees++] = v;
    }
  }
  free(last);

  /* Hand over the nodes (names are no longer needed after this point). */
  s->n_nodes = pm->n_nodes;
  s->nodes = pm->nodes;
  pm->nodes = NULL;
  pm->n_nodes = 0;
  return 0;
}


int onnx_session_load(const char *path, onnx_session **out, char *err, size_t errlen) {
  *out = NULL;
  FILE *f = fopen(path, "rb");
  if (!f) FAIL("cannot open %s", path);
  if (fseek(f, 0, SEEK_END) != 0) { fclose(f); FAIL("seek failed"); }
  long sz = ftell(f);
  rewind(f);
  if (sz <= 0) { fclose(f); FAIL("empty model file"); }
  uint8_t *buf = malloc((size_t)sz);
  if (!buf) { fclose(f); FAIL("out of memory"); }
  if (fread(buf, 1, (size_t)sz, f) != (size_t)sz) { fclose(f); free(buf); FAIL("short read"); }
  fclose(f);

  parsed pm = {0};
  int rc = parse_model(buf, (size_t)sz, &pm, err, errlen);
  free(buf);
  if (rc) { parsed_free(&pm); return -1; }

  onnx_session *s = calloc(1, sizeof *s);
  if (!s) { parsed_free(&pm); FAIL("out of memory"); }
  if (build_session(s, &pm, err, errlen)) {
    parsed_free(&pm);
    onnx_session_free(s);
    return -1;
  }
  parsed_free(&pm);
  *out = s;
  return 0;
}

void onnx_session_free(onnx_session *s) {
  if (!s) return;
  for (size_t i = 0; i < s->n_nodes; i++) node_free(&s->nodes[i]);
  free(s->nodes);
  for (size_t v = 0; v < s->n_vals; v++) onnx_tensor_free(&s->slots[v].t);
  free(s->slots);
  namemap_free(&s->map);
  free(s);
}

size_t onnx_session_node_count(const onnx_session *s) { return s->n_nodes; }

/* ------------------------------------------------------------------------ */
/* Attribute helpers                                                        */
/* ------------------------------------------------------------------------ */

static const onnx_attr *attr_get(const onnx_node *nd, const char *name) {
  for (size_t i = 0; i < nd->n_attrs; i++)
    if (nd->attrs[i].name && strcmp(nd->attrs[i].name, name) == 0) return &nd->attrs[i];
  return NULL;
}

static int64_t attr_int(const onnx_node *nd, const char *name, int64_t def) {
  const onnx_attr *a = attr_get(nd, name);
  return (a && a->type == ATTR_INT) ? a->i : def;
}

/* ------------------------------------------------------------------------ */
/* Kernels. Each fills out[0..n) (zeroed on entry) and returns 0 or -1.     */
/* ------------------------------------------------------------------------ */

typedef int (*kernel_fn)(const onnx_node *nd, const onnx_tensor *const *in, size_t nin,
                         onnx_tensor *out, char *err, size_t errlen);

static int k_constant(const onnx_node *nd, const onnx_tensor *const *in, size_t nin,
                      onnx_tensor *out, char *err, size_t errlen) {
  (void)in; (void)nin;
  const onnx_attr *a = attr_get(nd, "value");
  if (!a || a->type != ATTR_TENSOR) FAIL("Constant without tensor value");
  return tensor_copy(&out[0], &a->t, err, errlen);
}

static int k_shape(const onnx_node *nd, const onnx_tensor *const *in, size_t nin,
                   onnx_tensor *out, char *err, size_t errlen) {
  (void)nd; (void)nin;
  const onnx_tensor *x = in[0];
  int64_t n = x->ndim;
  if (onnx_tensor_init(&out[0], ONNX_INT64, 1, &n, err, errlen)) return -1;
  for (int i = 0; i < x->ndim; i++) ((int64_t *)out[0].data)[i] = x->dims[i];
  return 0;
}

static int k_gather(const onnx_node *nd, const onnx_tensor *const *in, size_t nin,
                    onnx_tensor *out, char *err, size_t errlen) {
  (void)nin;
  const onnx_tensor *d = in[0], *ix = in[1];
  int64_t axis = attr_int(nd, "axis", 0);
  if (axis < 0) axis += d->ndim;
  if (axis < 0 || axis >= d->ndim) FAIL("Gather: bad axis");
  int ond = d->ndim - 1 + ix->ndim;
  if (ond > ONNX_MAX_DIMS) FAIL("Gather: rank too large");
  int64_t dims[ONNX_MAX_DIMS];
  int k = 0;
  for (int i = 0; i < axis; i++) dims[k++] = d->dims[i];
  for (int j = 0; j < ix->ndim; j++) dims[k++] = ix->dims[j];
  for (int i = (int)axis + 1; i < d->ndim; i++) dims[k++] = d->dims[i];
  size_t outer = 1, inner = 1;
  for (int i = 0; i < axis; i++) outer *= (size_t)d->dims[i];
  for (int i = (int)axis + 1; i < d->ndim; i++) inner *= (size_t)d->dims[i];
  int64_t adim = d->dims[axis];
  size_t nidx = ix->count;
  size_t es = onnx_dtype_size(d->dtype);
  if (onnx_tensor_init(&out[0], d->dtype, ond, dims, err, errlen)) return -1;
  const char *src = d->data;
  char *dst = out[0].data;
  for (size_t o = 0; o < outer; o++) {
    for (size_t j = 0; j < nidx; j++) {
      int64_t v = elem_i(ix, j);
      if (v < 0) v += adim;
      if (v < 0 || v >= adim) FAIL("Gather: index out of range");
      memcpy(dst + (o * nidx + j) * inner * es, src + (o * (size_t)adim + (size_t)v) * inner * es,
             inner * es);
    }
  }
  return 0;
}

static int k_unsqueeze(const onnx_node *nd, const onnx_tensor *const *in, size_t nin,
                       onnx_tensor *out, char *err, size_t errlen) {
  const onnx_tensor *x = in[0];
  int64_t axes_buf[ONNX_MAX_DIMS];
  const int64_t *axes = axes_buf;
  size_t nax = 0;
  if (nin >= 2 && in[1]) {
    nax = in[1]->count;
    if (nax > ONNX_MAX_DIMS) FAIL("Unsqueeze: too many axes");
    for (size_t k = 0; k < nax; k++) axes_buf[k] = elem_i(in[1], k);
  } else {
    const onnx_attr *a = attr_get(nd, "axes");
    if (!a || a->type != ATTR_INTS) FAIL("Unsqueeze without axes");
    nax = a->n_ints;
    if (nax > ONNX_MAX_DIMS) FAIL("Unsqueeze: too many axes");
    axes = a->ints;
  }
  int ond = x->ndim + (int)nax;
  if (ond > ONNX_MAX_DIMS) FAIL("Unsqueeze: rank too large");
  int is_ax[ONNX_MAX_DIMS] = {0};
  for (size_t k = 0; k < nax; k++) {
    int64_t a = axes[k];
    if (a < 0) a += ond;
    if (a < 0 || a >= ond) FAIL("Unsqueeze: bad axis");
    is_ax[a] = 1;
  }
  int64_t dims[ONNX_MAX_DIMS];
  int src = 0;
  for (int d = 0; d < ond; d++) dims[d] = is_ax[d] ? 1 : x->dims[src++];
  if (onnx_tensor_init(&out[0], x->dtype, ond, dims, err, errlen)) return -1;
  memcpy(out[0].data, x->data, x->count * onnx_dtype_size(x->dtype));
  return 0;
}

static int k_concat(const onnx_node *nd, const onnx_tensor *const *in, size_t nin,
                    onnx_tensor *out, char *err, size_t errlen) {
  const onnx_tensor *x0 = in[0];
  int64_t axis = attr_int(nd, "axis", 0);
  if (axis < 0) axis += x0->ndim;
  if (axis < 0 || axis >= x0->ndim) FAIL("Concat: bad axis");
  int64_t dims[ONNX_MAX_DIMS];
  for (int d = 0; d < x0->ndim; d++) dims[d] = x0->dims[d];
  dims[axis] = 0;
  for (size_t k = 0; k < nin; k++) {
    if (in[k]->dtype != x0->dtype || in[k]->ndim != x0->ndim) FAIL("Concat: mismatched inputs");
    dims[axis] += in[k]->dims[axis];
  }
  size_t es = onnx_dtype_size(x0->dtype);
  size_t outer = 1, inner = 1;
  for (int d = 0; d < axis; d++) outer *= (size_t)dims[d];
  for (int d = (int)axis + 1; d < x0->ndim; d++) inner *= (size_t)dims[d];
  if (onnx_tensor_init(&out[0], x0->dtype, x0->ndim, dims, err, errlen)) return -1;
  size_t out_chunk = (size_t)dims[axis] * inner;
  char *dst = out[0].data;
  for (size_t o = 0; o < outer; o++) {
    size_t off = 0;
    for (size_t k = 0; k < nin; k++) {
      size_t chunk = (size_t)in[k]->dims[axis] * inner;
      memcpy(dst + (o * out_chunk + off) * es, (const char *)in[k]->data + o * chunk * es, chunk * es);
      off += chunk;
    }
  }
  return 0;
}

static int k_reshape(const onnx_node *nd, const onnx_tensor *const *in, size_t nin,
                     onnx_tensor *out, char *err, size_t errlen) {
  (void)nin;
  const onnx_tensor *x = in[0], *sh = in[1];
  int allowzero = (int)attr_int(nd, "allowzero", 0);
  size_t ns = sh->count;
  if (ns > ONNX_MAX_DIMS) FAIL("Reshape: rank too large");
  int64_t dims[ONNX_MAX_DIMS];
  int infer = -1;
  size_t known = 1;
  for (size_t k = 0; k < ns; k++) {
    int64_t v = elem_i(sh, k);
    if (v == 0 && !allowzero) {
      if ((int)k >= x->ndim) FAIL("Reshape: zero dim beyond input rank");
      v = x->dims[k];
    }
    if (v == -1) {
      if (infer >= 0) FAIL("Reshape: two -1 dims");
      infer = (int)k;
      v = 1;
    }
    dims[k] = v;
    known *= (size_t)v;
  }
  if (infer >= 0) {
    if (known == 0 || x->count % known) FAIL("Reshape: cannot infer dimension");
    dims[infer] = (int64_t)(x->count / known);
  }
  if (numel((int)ns, dims) != x->count) FAIL("Reshape: element count mismatch");
  if (onnx_tensor_init(&out[0], x->dtype, (int)ns, dims, err, errlen)) return -1;
  memcpy(out[0].data, x->data, x->count * onnx_dtype_size(x->dtype));
  return 0;
}

static int k_transpose(const onnx_node *nd, const onnx_tensor *const *in, size_t nin,
                       onnx_tensor *out, char *err, size_t errlen) {
  (void)nin;
  const onnx_tensor *x = in[0];
  int nd_ = x->ndim;
  int64_t perm[ONNX_MAX_DIMS];
  const onnx_attr *a = attr_get(nd, "perm");
  for (int i = 0; i < nd_; i++) perm[i] = nd_ - 1 - i;
  if (a && a->type == ATTR_INTS) {
    if ((int)a->n_ints != nd_) FAIL("Transpose: perm rank mismatch");
    for (int i = 0; i < nd_; i++) perm[i] = a->ints[i];
  }
  int64_t cs[ONNX_MAX_DIMS], dims[ONNX_MAX_DIMS], st[ONNX_MAX_DIMS];
  contig_strides(nd_, x->dims, cs);
  for (int i = 0; i < nd_; i++) {
    if (perm[i] < 0 || perm[i] >= nd_) FAIL("Transpose: bad perm");
    dims[i] = x->dims[perm[i]];
    st[i] = cs[perm[i]];
  }
  if (onnx_tensor_init(&out[0], x->dtype, nd_, dims, err, errlen)) return -1;
  int64_t *off = alloc_offsets(x->count);
  if (!off) FAIL("out of memory");
  strided_offsets(nd_, dims, st, 0, x->count, off);
  gather_elems(out[0].data, x->data, off, x->count, onnx_dtype_size(x->dtype));
  free(off);
  return 0;
}

static int k_cast(const onnx_node *nd, const onnx_tensor *const *in, size_t nin,
                  onnx_tensor *out, char *err, size_t errlen) {
  (void)nin;
  const onnx_tensor *x = in[0];
  onnx_dtype dt = (onnx_dtype)attr_int(nd, "to", 0);
  if (!onnx_dtype_size(dt)) FAIL("Cast: unsupported target type");
  if (onnx_tensor_init(&out[0], dt, x->ndim, x->dims, err, errlen)) return -1;
  int src_f = x->dtype == ONNX_FLOAT;
  for (size_t i = 0; i < x->count; i++) {
    float fv = src_f ? ((const float *)x->data)[i] : 0.0f;
    int64_t iv = src_f ? 0 : elem_i(x, i);
    switch (dt) {
      case ONNX_FLOAT: ((float *)out[0].data)[i] = src_f ? fv : (float)iv; break;
      case ONNX_INT64: ((int64_t *)out[0].data)[i] = src_f ? (int64_t)fv : iv; break;
      case ONNX_INT32: ((int32_t *)out[0].data)[i] = (int32_t)(src_f ? (int64_t)fv : iv); break;
      case ONNX_UINT8: ((uint8_t *)out[0].data)[i] = (uint8_t)(src_f ? (int64_t)fv : iv); break;
      case ONNX_INT8: ((int8_t *)out[0].data)[i] = (int8_t)(src_f ? (int64_t)fv : iv); break;
      case ONNX_BOOL: ((uint8_t *)out[0].data)[i] = src_f ? (fv != 0.0f) : (iv != 0); break;
    }
  }
  return 0;
}

enum { BIN_ADD, BIN_SUB, BIN_MUL, BIN_DIV, BIN_POW, BIN_EQUAL };

/* Broadcasting binary op over two operands. */
static int binary_op(int op, const onnx_tensor *a, const onnx_tensor *b, onnx_tensor *out,
                     char *err, size_t errlen) {
  if (a->dtype != b->dtype) FAIL("binary op: dtype mismatch");
  int ond;
  int64_t odims[ONNX_MAX_DIMS];
  if (bcast_dims(a->ndim, a->dims, b->ndim, b->dims, &ond, odims, err, errlen)) return -1;
  size_t n = numel(ond, odims);
  onnx_dtype dt = op == BIN_EQUAL ? ONNX_BOOL : a->dtype;
  if (onnx_tensor_init(out, dt, ond, odims, err, errlen)) return -1;
  int64_t sa[ONNX_MAX_DIMS], sb[ONNX_MAX_DIMS];
  bcast_strides(a, ond, odims, sa);
  bcast_strides(b, ond, odims, sb);
  int64_t *oa = alloc_offsets(n), *ob = alloc_offsets(n);
  if (!oa || !ob) { free(oa); free(ob); FAIL("out of memory"); }
  strided_offsets(ond, odims, sa, 0, n, oa);
  strided_offsets(ond, odims, sb, 0, n, ob);
  int rc = 0;
  switch (a->dtype) {
    case ONNX_FLOAT: {
      const float *pa = a->data, *pb = b->data;
      float *po = out->data;
      for (size_t i = 0; i < n; i++) {
        float x = pa[oa[i]], y = pb[ob[i]];
        switch (op) {
          case BIN_ADD: po[i] = x + y; break;
          case BIN_SUB: po[i] = x - y; break;
          case BIN_MUL: po[i] = x * y; break;
          case BIN_DIV: po[i] = x / y; break;
          case BIN_POW: po[i] = powf(x, y); break;
          case BIN_EQUAL: ((uint8_t *)out->data)[i] = x == y; break;
        }
      }
      break;
    }
    case ONNX_INT64: {
      const int64_t *pa = a->data, *pb = b->data;
      int64_t *po = out->data;
      for (size_t i = 0; i < n; i++) {
        int64_t x = pa[oa[i]], y = pb[ob[i]];
        switch (op) {
          case BIN_ADD: po[i] = x + y; break;
          case BIN_SUB: po[i] = x - y; break;
          case BIN_MUL: po[i] = x * y; break;
          case BIN_DIV: po[i] = y ? x / y : 0; break;
          case BIN_EQUAL: ((uint8_t *)out->data)[i] = x == y; break;
          default: rc = -1; break;
        }
      }
      break;
    }
    case ONNX_BOOL:
    case ONNX_UINT8:
    case ONNX_INT8:
    case ONNX_INT32:
      if (op != BIN_EQUAL) { rc = -1; break; }
      for (size_t i = 0; i < n; i++)
        ((uint8_t *)out->data)[i] = elem_i(a, (size_t)oa[i]) == elem_i(b, (size_t)ob[i]);
      break;
  }
  free(oa);
  free(ob);
  if (rc) {
    onnx_tensor_free(out);
    FAIL("binary op: unsupported dtype/op combination");
  }
  return 0;
}

static int k_add(const onnx_node *nd, const onnx_tensor *const *in, size_t nin, onnx_tensor *out,
                 char *err, size_t errlen) {
  (void)nd; (void)nin;
  return binary_op(BIN_ADD, in[0], in[1], out, err, errlen);
}
static int k_sub(const onnx_node *nd, const onnx_tensor *const *in, size_t nin, onnx_tensor *out,
                 char *err, size_t errlen) {
  (void)nd; (void)nin;
  return binary_op(BIN_SUB, in[0], in[1], out, err, errlen);
}
static int k_mul(const onnx_node *nd, const onnx_tensor *const *in, size_t nin, onnx_tensor *out,
                 char *err, size_t errlen) {
  (void)nd; (void)nin;
  return binary_op(BIN_MUL, in[0], in[1], out, err, errlen);
}
static int k_div(const onnx_node *nd, const onnx_tensor *const *in, size_t nin, onnx_tensor *out,
                 char *err, size_t errlen) {
  (void)nd; (void)nin;
  return binary_op(BIN_DIV, in[0], in[1], out, err, errlen);
}
static int k_pow(const onnx_node *nd, const onnx_tensor *const *in, size_t nin, onnx_tensor *out,
                 char *err, size_t errlen) {
  (void)nd; (void)nin;
  return binary_op(BIN_POW, in[0], in[1], out, err, errlen);
}
static int k_equal(const onnx_node *nd, const onnx_tensor *const *in, size_t nin, onnx_tensor *out,
                   char *err, size_t errlen) {
  (void)nd; (void)nin;
  return binary_op(BIN_EQUAL, in[0], in[1], out, err, errlen);
}

static int unary_float(int code, const onnx_tensor *x, onnx_tensor *out, char *err, size_t errlen) {
  if (x->dtype != ONNX_FLOAT) FAIL("unary op expects float");
  if (onnx_tensor_init(out, ONNX_FLOAT, x->ndim, x->dims, err, errlen)) return -1;
  const float *p = x->data;
  float *o = out->data;
  for (size_t i = 0; i < x->count; i++) {
    switch (code) {
      case OP_SQRT: o[i] = sqrtf(p[i]); break;
      case OP_ERF: o[i] = erff(p[i]); break;
      case OP_RELU: o[i] = p[i] > 0.0f ? p[i] : 0.0f; break;
    }
  }
  return 0;
}

static int k_sqrt(const onnx_node *nd, const onnx_tensor *const *in, size_t nin, onnx_tensor *out,
                  char *err, size_t errlen) {
  (void)nd; (void)nin;
  return unary_float(OP_SQRT, in[0], out, err, errlen);
}
static int k_erf(const onnx_node *nd, const onnx_tensor *const *in, size_t nin, onnx_tensor *out,
                 char *err, size_t errlen) {
  (void)nd; (void)nin;
  return unary_float(OP_ERF, in[0], out, err, errlen);
}
static int k_relu(const onnx_node *nd, const onnx_tensor *const *in, size_t nin, onnx_tensor *out,
                  char *err, size_t errlen) {
  (void)nd; (void)nin;
  return unary_float(OP_RELU, in[0], out, err, errlen);
}

static int k_where(const onnx_node *nd, const onnx_tensor *const *in, size_t nin, onnx_tensor *out,
                   char *err, size_t errlen) {
  (void)nd; (void)nin;
  const onnx_tensor *c = in[0], *x = in[1], *y = in[2];
  if (x->dtype != y->dtype || c->dtype != ONNX_BOOL) FAIL("Where: bad operand types");
  int t1, t2, ond;
  int64_t d1[ONNX_MAX_DIMS], odims[ONNX_MAX_DIMS];
  if (bcast_dims(c->ndim, c->dims, x->ndim, x->dims, &t1, d1, err, errlen)) return -1;
  if (bcast_dims(t1, d1, y->ndim, y->dims, &t2, odims, err, errlen)) return -1;
  ond = t2;
  size_t n = numel(ond, odims);
  if (onnx_tensor_init(out, x->dtype, ond, odims, err, errlen)) return -1;
  int64_t sc[ONNX_MAX_DIMS], sx[ONNX_MAX_DIMS], sy[ONNX_MAX_DIMS];
  bcast_strides(c, ond, odims, sc);
  bcast_strides(x, ond, odims, sx);
  bcast_strides(y, ond, odims, sy);
  int64_t *oc = alloc_offsets(n), *ox = alloc_offsets(n), *oy = alloc_offsets(n);
  if (!oc || !ox || !oy) { free(oc); free(ox); free(oy); FAIL("out of memory"); }
  strided_offsets(ond, odims, sc, 0, n, oc);
  strided_offsets(ond, odims, sx, 0, n, ox);
  strided_offsets(ond, odims, sy, 0, n, oy);
  size_t es = onnx_dtype_size(x->dtype);
  char *o = out->data;
  for (size_t i = 0; i < n; i++) {
    const char *src = ((const uint8_t *)c->data)[oc[i]] ? (const char *)x->data + (size_t)ox[i] * es
                                                        : (const char *)y->data + (size_t)oy[i] * es;
    memcpy(o + i * es, src, es);
  }
  free(oc); free(ox); free(oy);
  return 0;
}

static int k_expand(const onnx_node *nd, const onnx_tensor *const *in, size_t nin, onnx_tensor *out,
                    char *err, size_t errlen) {
  (void)nd; (void)nin;
  const onnx_tensor *x = in[0], *sh = in[1];
  size_t ns = sh->count;
  if (ns > ONNX_MAX_DIMS) FAIL("Expand: rank too large");
  int64_t shv[ONNX_MAX_DIMS];
  for (size_t k = 0; k < ns; k++) shv[k] = elem_i(sh, k);
  int ond;
  int64_t odims[ONNX_MAX_DIMS];
  if (bcast_dims(x->ndim, x->dims, (int)ns, shv, &ond, odims, err, errlen)) return -1;
  size_t n = numel(ond, odims);
  if (onnx_tensor_init(out, x->dtype, ond, odims, err, errlen)) return -1;
  int64_t st[ONNX_MAX_DIMS];
  bcast_strides(x, ond, odims, st);
  int64_t *off = alloc_offsets(n);
  if (!off) FAIL("out of memory");
  strided_offsets(ond, odims, st, 0, n, off);
  gather_elems(out->data, x->data, off, n, onnx_dtype_size(x->dtype));
  free(off);
  return 0;
}

static int k_slice(const onnx_node *nd, const onnx_tensor *const *in, size_t nin, onnx_tensor *out,
                   char *err, size_t errlen) {
  (void)nd;
  const onnx_tensor *x = in[0];
  size_t ns = in[1]->count;
  if (ns > ONNX_MAX_DIMS || in[2]->count != ns) FAIL("Slice: bad starts/ends");
  int64_t sv[ONNX_MAX_DIMS], ev[ONNX_MAX_DIMS], av[ONNX_MAX_DIMS], pv[ONNX_MAX_DIMS];
  for (size_t k = 0; k < ns; k++) {
    sv[k] = elem_i(in[1], k);
    ev[k] = elem_i(in[2], k);
    av[k] = (nin > 3 && in[3]) ? elem_i(in[3], k) : (int64_t)k;
    pv[k] = (nin > 4 && in[4]) ? elem_i(in[4], k) : 1;
  }
  int nd_ = x->ndim;
  int64_t start[ONNX_MAX_DIMS], step[ONNX_MAX_DIMS], odims[ONNX_MAX_DIMS];
  for (int d = 0; d < nd_; d++) {
    start[d] = 0;
    step[d] = 1;
    odims[d] = x->dims[d];
  }
  for (size_t k = 0; k < ns; k++) {
    int64_t a = av[k];
    if (a < 0) a += nd_;
    if (a < 0 || a >= nd_) FAIL("Slice: bad axis");
    int64_t dim = x->dims[a], s = pv[k], b = sv[k], e = ev[k];
    if (s == 0) FAIL("Slice: zero step");
    if (s > 0) {
      if (b < 0) b += dim;
      if (e < 0) e += dim;
      b = b < 0 ? 0 : (b > dim ? dim : b);
      e = e < 0 ? 0 : (e > dim ? dim : e);
    } else {
      if (b < 0) b += dim;
      if (e < 0) e += dim;
      b = b < -1 ? -1 : (b > dim - 1 ? dim - 1 : b);
      e = e < -1 ? -1 : (e > dim - 1 ? dim - 1 : e);
    }
    int64_t len = 0;
    if (s > 0 && e > b) len = (e - b + s - 1) / s;
    if (s < 0 && b > e) len = (b - e + (-s) - 1) / (-s);
    start[a] = b;
    step[a] = s;
    odims[a] = len;
  }
  int64_t cs[ONNX_MAX_DIMS], st[ONNX_MAX_DIMS];
  contig_strides(nd_, x->dims, cs);
  int64_t base = 0;
  for (int d = 0; d < nd_; d++) {
    base += start[d] * cs[d];
    st[d] = cs[d] * step[d];
  }
  size_t n = numel(nd_, odims);
  if (onnx_tensor_init(out, x->dtype, nd_, odims, err, errlen)) return -1;
  if (n) {
    int64_t *off = alloc_offsets(n);
    if (!off) FAIL("out of memory");
    strided_offsets(nd_, odims, st, base, n, off);
    gather_elems(out->data, x->data, off, n, onnx_dtype_size(x->dtype));
    free(off);
  }
  return 0;
}

static int k_reducemean(const onnx_node *nd, const onnx_tensor *const *in, size_t nin, onnx_tensor *out,
                        char *err, size_t errlen) {
  (void)nin;
  const onnx_tensor *x = in[0];
  if (x->dtype != ONNX_FLOAT) FAIL("ReduceMean: float only");
  int nd_ = x->ndim;
  int reduced[ONNX_MAX_DIMS] = {0};
  const onnx_attr *a = attr_get(nd, "axes");
  if (a && a->type == ATTR_INTS) {
    for (size_t k = 0; k < a->n_ints; k++) {
      int64_t ax = a->ints[k];
      if (ax < 0) ax += nd_;
      if (ax < 0 || ax >= nd_) FAIL("ReduceMean: bad axis");
      reduced[ax] = 1;
    }
  } else {
    for (int d = 0; d < nd_; d++) reduced[d] = 1;
  }
  int keep = (int)attr_int(nd, "keepdims", 1);
  int64_t kdims[ONNX_MAX_DIMS], odims[ONNX_MAX_DIMS];
  int64_t rcount = 1;
  int on = 0;
  for (int d = 0; d < nd_; d++) {
    if (reduced[d]) {
      kdims[d] = 1;
      rcount *= x->dims[d];
      if (keep) odims[on++] = 1;
    } else {
      kdims[d] = x->dims[d];
      odims[on++] = x->dims[d];
    }
  }
  int64_t kcs[ONNX_MAX_DIMS], ost[ONNX_MAX_DIMS];
  contig_strides(nd_, kdims, kcs);
  for (int d = 0; d < nd_; d++) ost[d] = reduced[d] ? 0 : kcs[d];
  size_t n = x->count;
  size_t on_count = numel(nd_, kdims);
  if (onnx_tensor_init(out, ONNX_FLOAT, on, odims, err, errlen)) return -1;
  int64_t *off = alloc_offsets(n);
  float *acc = calloc(on_count ? on_count : 1, sizeof(float));
  if (!off || !acc) { free(off); free(acc); FAIL("out of memory"); }
  strided_offsets(nd_, x->dims, ost, 0, n, off);
  const float *px = x->data;
  for (size_t i = 0; i < n; i++) acc[off[i]] += px[i];
  float *po = out->data;
  for (size_t i = 0; i < on_count; i++) po[i] = acc[i] / (float)rcount;
  free(off);
  free(acc);
  return 0;
}

static int k_softmax(const onnx_node *nd, const onnx_tensor *const *in, size_t nin, onnx_tensor *out,
                     char *err, size_t errlen) {
  (void)nin;
  const onnx_tensor *x = in[0];
  if (x->dtype != ONNX_FLOAT) FAIL("Softmax: float only");
  int64_t axis = attr_int(nd, "axis", 1);
  if (axis < 0) axis += x->ndim;
  if (axis < 0 || axis > x->ndim) FAIL("Softmax: bad axis");
  /* Opset 11 semantics: coerce to 2-D at axis, softmax over the trailing block. */
  size_t rows = 1, cols = 1;
  for (int d = 0; d < axis; d++) rows *= (size_t)x->dims[d];
  for (int d = (int)axis; d < x->ndim; d++) cols *= (size_t)x->dims[d];
  if (onnx_tensor_init(out, ONNX_FLOAT, x->ndim, x->dims, err, errlen)) return -1;
  const float *px = x->data;
  float *po = out->data;
  for (size_t r = 0; r < rows; r++) {
    const float *row = px + r * cols;
    float *orow = po + r * cols;
    float mx = row[0];
    for (size_t c = 1; c < cols; c++) if (row[c] > mx) mx = row[c];
    float sum = 0.0f;
    for (size_t c = 0; c < cols; c++) {
      orow[c] = expf(row[c] - mx);
      sum += orow[c];
    }
    for (size_t c = 0; c < cols; c++) orow[c] /= sum;
  }
  return 0;
}

/* Batch plan for MatMul-style ops: broadcast the leading (batch) dims. */
typedef struct bplan {
  int nd;
  int64_t dims[ONNX_MAX_DIMS];
  size_t count;
  int64_t *oa; /* per batch: matrix index into a */
  int64_t *ob; /* per batch: matrix index into b */
} bplan;

static void bplan_free(bplan *p) {
  free(p->oa);
  free(p->ob);
  p->oa = p->ob = NULL;
}

static int bplan_make(const onnx_tensor *a, const onnx_tensor *b, bplan *p, char *err, size_t errlen) {
  onnx_tensor ab = *a, bb = *b;
  ab.ndim = a->ndim - 2;
  bb.ndim = b->ndim - 2;
  if (bcast_dims(ab.ndim, ab.dims, bb.ndim, bb.dims, &p->nd, p->dims, err, errlen)) return -1;
  p->count = numel(p->nd, p->dims);
  int64_t sa[ONNX_MAX_DIMS], sb[ONNX_MAX_DIMS];
  bcast_strides(&ab, p->nd, p->dims, sa);
  bcast_strides(&bb, p->nd, p->dims, sb);
  p->oa = alloc_offsets(p->count);
  p->ob = alloc_offsets(p->count);
  if (!p->oa || !p->ob) { bplan_free(p); FAIL("out of memory"); }
  strided_offsets(p->nd, p->dims, sa, 0, p->count, p->oa);
  strided_offsets(p->nd, p->dims, sb, 0, p->count, p->ob);
  return 0;
}

static int matmul_dims(const onnx_tensor *a, const onnx_tensor *b, size_t *M, size_t *K, size_t *N,
                       char *err, size_t errlen) {
  if (a->ndim < 2 || b->ndim < 2) FAIL("MatMul: rank < 2 not supported");
  *M = (size_t)a->dims[a->ndim - 2];
  *K = (size_t)a->dims[a->ndim - 1];
  size_t K2 = (size_t)b->dims[b->ndim - 2];
  *N = (size_t)b->dims[b->ndim - 1];
  if (*K != K2) FAIL("MatMul: inner dimensions differ");
  return 0;
}

static int matmul_out_dims(const bplan *p, size_t M, size_t N, int *ond, int64_t *odims) {
  *ond = p->nd + 2;
  if (*ond > ONNX_MAX_DIMS) return -1;
  for (int d = 0; d < p->nd; d++) odims[d] = p->dims[d];
  odims[p->nd] = (int64_t)M;
  odims[p->nd + 1] = (int64_t)N;
  return 0;
}

static int k_matmul(const onnx_node *nd, const onnx_tensor *const *in, size_t nin, onnx_tensor *out,
                    char *err, size_t errlen) {
  (void)nd; (void)nin;
  const onnx_tensor *a = in[0], *b = in[1];
  if (a->dtype != ONNX_FLOAT || b->dtype != ONNX_FLOAT) FAIL("MatMul: float only");
  size_t M, K, N;
  if (matmul_dims(a, b, &M, &K, &N, err, errlen)) return -1;
  bplan p;
  if (bplan_make(a, b, &p, err, errlen)) return -1;
  int ond;
  int64_t odims[ONNX_MAX_DIMS];
  if (matmul_out_dims(&p, M, N, &ond, odims)) { bplan_free(&p); FAIL("MatMul: rank too large"); }
  if (onnx_tensor_init(out, ONNX_FLOAT, ond, odims, err, errlen)) { bplan_free(&p); return -1; }
  const float *pa = a->data, *pb = b->data;
  float *pc = out->data;
  for (size_t bi = 0; bi < p.count; bi++) {
    const float *A = pa + (size_t)p.oa[bi] * M * K;
    const float *B = pb + (size_t)p.ob[bi] * K * N;
    float *C = pc + bi * M * N;
    for (size_t m = 0; m < M; m++) {
      float *crow = C + m * N;
      memset(crow, 0, N * sizeof(float));
      for (size_t k = 0; k < K; k++) {
        float av = A[m * K + k];
        const float *brow = B + k * N;
        for (size_t n = 0; n < N; n++) crow[n] += av * brow[n];
      }
    }
  }
  bplan_free(&p);
  return 0;
}

/* int32 accumulate kernels: A already has its zero point subtracted. */
static void mm_int_bu8(const int32_t *A, const uint8_t *B, const int32_t *bz, size_t M, size_t K,
                       size_t N, int32_t *C) {
  for (size_t m = 0; m < M; m++) {
    int32_t *crow = C + m * N;
    memset(crow, 0, N * sizeof(int32_t));
    for (size_t k = 0; k < K; k++) {
      int32_t av = A[m * K + k];
      if (!av) continue;
      const uint8_t *brow = B + k * N;
      for (size_t n = 0; n < N; n++) crow[n] += av * ((int32_t)brow[n] - bz[n]);
    }
  }
}

static void mm_int_bi8(const int32_t *A, const int8_t *B, const int32_t *bz, size_t M, size_t K,
                       size_t N, int32_t *C) {
  for (size_t m = 0; m < M; m++) {
    int32_t *crow = C + m * N;
    memset(crow, 0, N * sizeof(int32_t));
    for (size_t k = 0; k < K; k++) {
      int32_t av = A[m * K + k];
      if (!av) continue;
      const int8_t *brow = B + k * N;
      for (size_t n = 0; n < N; n++) crow[n] += av * ((int32_t)brow[n] - bz[n]);
    }
  }
}

static int is_qint(onnx_dtype t) { return t == ONNX_UINT8 || t == ONNX_INT8; }

static int k_matmulinteger(const onnx_node *nd, const onnx_tensor *const *in, size_t nin, onnx_tensor *out,
                           char *err, size_t errlen) {
  (void)nd;
  const onnx_tensor *a = in[0], *b = in[1];
  const onnx_tensor *azp = nin > 2 ? in[2] : NULL, *bzp = nin > 3 ? in[3] : NULL;
  if (!is_qint(a->dtype) || !is_qint(b->dtype)) FAIL("MatMulInteger: uint8/int8 only");
  size_t M, K, N;
  if (matmul_dims(a, b, &M, &K, &N, err, errlen)) return -1;
  int32_t azv = 0;
  if (azp) {
    if (azp->count != 1) FAIL("MatMulInteger: per-row a_zero_point not supported");
    azv = (int32_t)elem_i(azp, 0);
  }
  int32_t *bz = malloc(sizeof(int32_t) * (N != 0 ? N : 1));
  if (!bz) FAIL("out of memory");
  for (size_t n = 0; n < N; n++) {
    if (!bzp) bz[n] = 0;
    else if (bzp->count == 1) bz[n] = (int32_t)elem_i(bzp, 0);
    else if (bzp->count == N) bz[n] = (int32_t)elem_i(bzp, n);
    else { free(bz); FAIL("MatMulInteger: bad b_zero_point shape"); }
  }
  bplan p;
  if (bplan_make(a, b, &p, err, errlen)) { free(bz); return -1; }
  int ond;
  int64_t odims[ONNX_MAX_DIMS];
  if (matmul_out_dims(&p, M, N, &ond, odims) || onnx_tensor_init(out, ONNX_INT32, ond, odims, err, errlen)) {
    bplan_free(&p);
    free(bz);
    FAIL("MatMulInteger: output setup failed");
  }
  size_t aelems = (M * K) != 0 ? M * K : 1;
  int32_t *Aint = malloc(sizeof(int32_t) * aelems);
  if (!Aint) { bplan_free(&p); free(bz); FAIL("out of memory"); }
  int32_t *pc = out->data;
  for (size_t bi = 0; bi < p.count; bi++) {
    size_t a_base = (size_t)p.oa[bi] * M * K;
    for (size_t i = 0; i < M * K; i++) Aint[i] = (int32_t)elem_i(a, a_base + i) - azv;
    size_t b_base = (size_t)p.ob[bi] * K * N;
    int32_t *C = pc + bi * M * N;
    if (b->dtype == ONNX_UINT8) mm_int_bu8(Aint, (const uint8_t *)b->data + b_base, bz, M, K, N, C);
    else mm_int_bi8(Aint, (const int8_t *)b->data + b_base, bz, M, K, N, C);
  }
  free(Aint);
  bplan_free(&p);
  free(bz);
  return 0;
}

static int k_dynamicquantizelinear(const onnx_node *nd, const onnx_tensor *const *in, size_t nin,
                                   onnx_tensor *out, char *err, size_t errlen) {
  (void)nd; (void)nin;
  const onnx_tensor *x = in[0];
  if (x->dtype != ONNX_FLOAT) FAIL("DynamicQuantizeLinear: float only");
  const float *px = x->data;
  float mn = 0.0f, mx = 0.0f;   /* range always includes zero */
  for (size_t i = 0; i < x->count; i++) {
    if (px[i] < mn) mn = px[i];
    if (px[i] > mx) mx = px[i];
  }
  float scale = (mx - mn) / 255.0f;
  float zp_f = 0.0f;
  if (scale == 0.0f) {
    scale = 1.0f;
  } else {
    zp_f = nearbyintf(0.0f - mn / scale);
  }
  if (zp_f < 0.0f) zp_f = 0.0f;
  if (zp_f > 255.0f) zp_f = 255.0f;
  uint8_t zp = (uint8_t)zp_f;
  if (onnx_tensor_init(&out[0], ONNX_UINT8, x->ndim, x->dims, err, errlen)) return -1;
  if (onnx_tensor_init(&out[1], ONNX_FLOAT, 0, NULL, err, errlen)) return -1;
  if (onnx_tensor_init(&out[2], ONNX_UINT8, 0, NULL, err, errlen)) return -1;
  uint8_t *py = out[0].data;
  for (size_t i = 0; i < x->count; i++) {
    float q = nearbyintf(px[i] / scale) + (float)zp;
    if (q < 0.0f) q = 0.0f;
    if (q > 255.0f) q = 255.0f;
    py[i] = (uint8_t)q;
  }
  ((float *)out[1].data)[0] = scale;
  ((uint8_t *)out[2].data)[0] = zp;
  return 0;
}

static int k_dequantizelinear(const onnx_node *nd, const onnx_tensor *const *in, size_t nin,
                              onnx_tensor *out, char *err, size_t errlen) {
  const onnx_tensor *x = in[0], *sc = in[1];
  const onnx_tensor *zp = nin > 2 ? in[2] : NULL;
  if (sc->dtype != ONNX_FLOAT) FAIL("DequantizeLinear: float scale only");
  if (!is_qint(x->dtype) && x->dtype != ONNX_INT32) FAIL("DequantizeLinear: bad input dtype");
  int64_t axis = attr_int(nd, "axis", 1);
  if (x->ndim > 0) {
    if (axis < 0) axis += x->ndim;
    if (axis < 0 || axis >= x->ndim) FAIL("DequantizeLinear: bad axis");
  } else {
    axis = 0;
  }
  size_t inner = 1;
  int64_t adim = 1;
  if (x->ndim > 0) {
    adim = x->dims[axis];
    for (int d = (int)axis + 1; d < x->ndim; d++) inner *= (size_t)x->dims[d];
  }
  if (onnx_tensor_init(out, ONNX_FLOAT, x->ndim, x->dims, err, errlen)) return -1;
  const float *ps = sc->data;
  float *py = out->data;
  for (size_t i = 0; i < x->count; i++) {
    size_t coord = x->ndim > 0 ? (i / (inner ? inner : 1)) % (size_t)adim : 0;
    size_t si = sc->count == 1 ? 0 : coord;
    int64_t z = zp ? elem_i(zp, zp->count == 1 ? 0 : coord) : 0;
    py[i] = (float)(elem_i(x, i) - z) * ps[si];
  }
  return 0;
}

static const kernel_fn kernels[] = {
  [OP_CONSTANT] = k_constant,
  [OP_SHAPE] = k_shape,
  [OP_GATHER] = k_gather,
  [OP_UNSQUEEZE] = k_unsqueeze,
  [OP_CONCAT] = k_concat,
  [OP_RESHAPE] = k_reshape,
  [OP_TRANSPOSE] = k_transpose,
  [OP_CAST] = k_cast,
  [OP_ADD] = k_add,
  [OP_SUB] = k_sub,
  [OP_MUL] = k_mul,
  [OP_DIV] = k_div,
  [OP_POW] = k_pow,
  [OP_SQRT] = k_sqrt,
  [OP_ERF] = k_erf,
  [OP_RELU] = k_relu,
  [OP_EQUAL] = k_equal,
  [OP_WHERE] = k_where,
  [OP_EXPAND] = k_expand,
  [OP_REDUCEMEAN] = k_reducemean,
  [OP_SOFTMAX] = k_softmax,
  [OP_MATMUL] = k_matmul,
  [OP_MATMULINTEGER] = k_matmulinteger,
  [OP_DYNAMICQUANTIZELINEAR] = k_dynamicquantizelinear,
  [OP_DEQUANTIZELINEAR] = k_dequantizelinear,
  [OP_SLICE] = k_slice,
};

/* ------------------------------------------------------------------------ */
/* Executor                                                                 */
/* ------------------------------------------------------------------------ */

int onnx_session_run(onnx_session *s, const char *const *in_names, const onnx_tensor *const *in_tensors,
                     size_t n_in, const char *out_name, onnx_tensor *out, char *err, size_t errlen) {
  int rc = -1;
  for (size_t k = 0; k < n_in; k++) {
    int v = namemap_find(&s->map, in_names[k]);
    if (v < 0 || !s->slots[v].is_input) FAIL("not a graph input: %s", in_names[k]);
    onnx_slot *sl = &s->slots[v];
    onnx_tensor_free(&sl->t);
    sl->live = 0;
    if (tensor_copy(&sl->t, in_tensors[k], err, errlen)) goto cleanup;
    sl->live = 1;
  }

  for (size_t i = 0; i < s->n_nodes; i++) {
    onnx_node *nd = &s->nodes[i];
    if (nd->n_in > MAX_IO || nd->n_out > MAX_IO) {
      set_err(err, errlen, "node %zu has too many operands", i);
      goto cleanup;
    }
    const onnx_tensor *inp[MAX_IO] = {0};
    for (size_t j = 0; j < nd->n_in; j++) {
      int v = nd->in_ids[j];
      if (v < 0) continue;
      if (!s->slots[v].live) {
        set_err(err, errlen, "internal: %s operand %zu not live", nd->op, j);
        goto cleanup;
      }
      inp[j] = &s->slots[v].t;
    }
    onnx_tensor outs[MAX_IO];
    memset(outs, 0, sizeof outs);
    if (kernels[nd->code](nd, inp, nd->n_in, outs, err, errlen)) {
      for (size_t j = 0; j < nd->n_out; j++) onnx_tensor_free(&outs[j]);
      size_t used = err ? strlen(err) : 0;
      if (err && used < errlen) set_err(err + used, errlen - used, " (node %zu %s)", i, nd->op);
      goto cleanup;
    }
    for (size_t j = 0; j < nd->n_out; j++) {
      int v = nd->out_ids[j];
      if (v < 0) {
        onnx_tensor_free(&outs[j]);
        continue;
      }
      onnx_tensor_free(&s->slots[v].t);
      s->slots[v].t = outs[j];
      s->slots[v].live = 1;
    }
    for (size_t f = 0; f < nd->n_frees; f++) {
      onnx_slot *sl = &s->slots[nd->frees[f]];
      onnx_tensor_free(&sl->t);
      sl->live = 0;
    }
  }

  int ov = namemap_find(&s->map, out_name);
  if (ov < 0 || !s->slots[ov].live) FAIL("graph output %s not produced", out_name);
  *out = s->slots[ov].t;
  memset(&s->slots[ov].t, 0, sizeof s->slots[ov].t);
  s->slots[ov].live = 0;
  rc = 0;

cleanup:
  for (size_t v = 0; v < s->n_vals; v++) {
    if (!s->slots[v].is_init && s->slots[v].live) {
      onnx_tensor_free(&s->slots[v].t);
      s->slots[v].live = 0;
    }
  }
  return rc;
}
