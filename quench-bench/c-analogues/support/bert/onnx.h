/*
 * Minimal ONNX graph interpreter: protobuf reader, tensors, kernels, executor.
 *
 * Supports the operators used by the quantized DistilBERT SST-2 graph
 * (opset 11). Tensors are dense, row-major, up to ONNX_MAX_DIMS dimensions.
 * Initializers are read from the model file once (session load); every run
 * executes the graph nodes in file order and frees intermediates after their
 * last use.
 */
#ifndef ONNX_H
#define ONNX_H

#include <stddef.h>
#include <stdint.h>

#define ONNX_MAX_DIMS 8

/* TensorProto.DataType values. */
typedef enum {
  ONNX_FLOAT = 1,
  ONNX_UINT8 = 2,
  ONNX_INT8 = 3,
  ONNX_INT32 = 6,
  ONNX_INT64 = 7,
  ONNX_BOOL = 9
} onnx_dtype;

typedef struct onnx_tensor {
  onnx_dtype dtype;
  int ndim;
  int64_t dims[ONNX_MAX_DIMS];
  size_t count;   /* number of elements (1 for a scalar) */
  void *data;     /* owned; count * onnx_dtype_size(dtype) bytes */
} onnx_tensor;

typedef struct onnx_session onnx_session;

size_t onnx_dtype_size(onnx_dtype t);

/* Allocates a zeroed tensor. Returns 0 on success, -1 with err set on failure. */
int onnx_tensor_init(onnx_tensor *t, onnx_dtype dt, int ndim, const int64_t *dims,
                     char *err, size_t errlen);
void onnx_tensor_free(onnx_tensor *t);

/* Parses the model file, copies its initializers, and prepares the value table. */
int onnx_session_load(const char *path, onnx_session **out, char *err, size_t errlen);
void onnx_session_free(onnx_session *s);
size_t onnx_session_node_count(const onnx_session *s);

/*
 * Runs the graph. Inputs are copied in (the caller keeps ownership of them).
 * On success *out receives the named graph output (caller frees it).
 */
int onnx_session_run(onnx_session *s,
                     const char *const *in_names, const onnx_tensor *const *in_tensors,
                     size_t n_in, const char *out_name, onnx_tensor *out,
                     char *err, size_t errlen);

#endif /* ONNX_H */
