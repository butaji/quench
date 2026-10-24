/* Quench's small ownership-safe wrapper around the pinned nghttp2 C API. */
#include <nghttp2/nghttp2.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

typedef struct qh2_header {
  uint8_t *name;
  size_t name_len;
  uint8_t *value;
  size_t value_len;
  struct qh2_header *next;
} qh2_header;

typedef struct qh2_event {
  int kind; /* 1=headers, 2=data, 3=close, 4=push promise */
  int32_t stream_id;
  int32_t parent_stream_id;
  uint32_t code;
  int end_stream;
  uint8_t *data;
  size_t data_len;
  qh2_header *headers;
  qh2_header *headers_tail;
  size_t header_count;
  struct qh2_event *next;
} qh2_event;

typedef struct qh2_data {
  struct qh2 *owner;
  int32_t stream_id;
  uint8_t *bytes;
  size_t len;
  size_t offset;
  int pending;
  int eof;
  int deferred;
  struct qh2_data *next;
} qh2_data;

typedef struct qh2 {
  nghttp2_session *session;
  qh2_event *events_head;
  qh2_event *events_tail;
  qh2_event *current_headers;
  qh2_event *last_data;
  qh2_data *data_sources;
} qh2;

static void qh2_free_headers(qh2_header *header) {
  while (header) {
    qh2_header *next = header->next;
    free(header->name);
    free(header->value);
    free(header);
    header = next;
  }
}

static void qh2_free_event(qh2_event *event) {
  if (!event) return;
  free(event->data);
  qh2_free_headers(event->headers);
  free(event);
}

static void qh2_enqueue(qh2 *state, qh2_event *event) {
  if (!event) return;
  event->next = NULL;
  if (state->events_tail) state->events_tail->next = event;
  else state->events_head = event;
  state->events_tail = event;
}

static void qh2_remove_data(qh2 *state, qh2_data *data) {
  qh2_data **cursor = &state->data_sources;
  while (*cursor && *cursor != data) cursor = &(*cursor)->next;
  if (*cursor) *cursor = data->next;
  free(data->bytes);
  free(data);
}

static qh2_data *qh2_find_data(qh2 *state, int32_t stream_id) {
  for (qh2_data *data = state->data_sources; data; data = data->next) {
    if (data->stream_id == stream_id) return data;
  }
  return NULL;
}

static int qh2_begin_headers(nghttp2_session *session,
                             const nghttp2_frame *frame, void *user_data) {
  (void)session;
  qh2 *state = (qh2 *)user_data;
  qh2_free_event(state->current_headers);
  state->current_headers = (qh2_event *)calloc(1, sizeof(qh2_event));
  if (!state->current_headers) return NGHTTP2_ERR_CALLBACK_FAILURE;
  state->current_headers->kind = 1;
  state->current_headers->stream_id = frame->hd.stream_id;
  if (frame->hd.type == NGHTTP2_PUSH_PROMISE) {
    state->current_headers->kind = 4;
    state->current_headers->stream_id = frame->push_promise.promised_stream_id;
    state->current_headers->parent_stream_id = frame->hd.stream_id;
  }
  return 0;
}

static int qh2_on_header(nghttp2_session *session, const nghttp2_frame *frame,
                         const uint8_t *name, size_t namelen,
                         const uint8_t *value, size_t valuelen,
                         uint8_t flags, void *user_data) {
  (void)session;
  (void)frame;
  (void)flags;
  qh2 *state = (qh2 *)user_data;
  qh2_event *event = state->current_headers;
  if (!event) return NGHTTP2_ERR_CALLBACK_FAILURE;
  qh2_header *header = (qh2_header *)calloc(1, sizeof(qh2_header));
  if (!header) return NGHTTP2_ERR_CALLBACK_FAILURE;
  header->name = (uint8_t *)malloc(namelen ? namelen : 1);
  header->value = (uint8_t *)malloc(valuelen ? valuelen : 1);
  if (!header->name || !header->value) {
    free(header->name);
    free(header->value);
    free(header);
    return NGHTTP2_ERR_CALLBACK_FAILURE;
  }
  if (namelen) memcpy(header->name, name, namelen);
  if (valuelen) memcpy(header->value, value, valuelen);
  header->name_len = namelen;
  header->value_len = valuelen;
  if (event->headers_tail) event->headers_tail->next = header;
  else event->headers = header;
  event->headers_tail = header;
  event->header_count++;
  return 0;
}

static int qh2_on_data(nghttp2_session *session, uint8_t flags,
                       int32_t stream_id, const uint8_t *data, size_t len,
                       void *user_data) {
  (void)session;
  (void)flags;
  qh2 *state = (qh2 *)user_data;
  qh2_event *event = (qh2_event *)calloc(1, sizeof(qh2_event));
  if (!event) return NGHTTP2_ERR_CALLBACK_FAILURE;
  event->kind = 2;
  event->stream_id = stream_id;
  event->data = (uint8_t *)malloc(len ? len : 1);
  if (!event->data) {
    free(event);
    return NGHTTP2_ERR_CALLBACK_FAILURE;
  }
  if (len) memcpy(event->data, data, len);
  event->data_len = len;
  qh2_enqueue(state, event);
  state->last_data = event;
  return 0;
}

static int qh2_on_frame(nghttp2_session *session, const nghttp2_frame *frame,
                        void *user_data) {
  (void)session;
  qh2 *state = (qh2 *)user_data;
  if (frame->hd.type == NGHTTP2_HEADERS ||
      frame->hd.type == NGHTTP2_PUSH_PROMISE) {
    qh2_event *event = state->current_headers;
    state->current_headers = NULL;
    if (event) {
      event->end_stream = (frame->hd.flags & NGHTTP2_FLAG_END_STREAM) != 0;
      qh2_enqueue(state, event);
    }
  } else if (frame->hd.type == NGHTTP2_DATA) {
    if (state->last_data && state->last_data->stream_id == frame->hd.stream_id) {
      state->last_data->end_stream = (frame->hd.flags & NGHTTP2_FLAG_END_STREAM) != 0;
    } else if (frame->hd.flags & NGHTTP2_FLAG_END_STREAM) {
      qh2_event *event = (qh2_event *)calloc(1, sizeof(qh2_event));
      if (!event) return NGHTTP2_ERR_CALLBACK_FAILURE;
      event->kind = 2;
      event->stream_id = frame->hd.stream_id;
      event->end_stream = 1;
      event->data = (uint8_t *)malloc(1);
      if (!event->data) {
        free(event);
        return NGHTTP2_ERR_CALLBACK_FAILURE;
      }
      qh2_enqueue(state, event);
      state->last_data = event;
    }
    state->last_data = NULL;
  }
  return 0;
}

static int qh2_on_close(nghttp2_session *session, int32_t stream_id,
                        uint32_t error_code, void *user_data) {
  (void)session;
  qh2 *state = (qh2 *)user_data;
  qh2_event *event = (qh2_event *)calloc(1, sizeof(qh2_event));
  if (!event) return NGHTTP2_ERR_CALLBACK_FAILURE;
  event->kind = 3;
  event->stream_id = stream_id;
  event->code = error_code;
  qh2_enqueue(state, event);
  qh2_data *data = qh2_find_data(state, stream_id);
  if (data) qh2_remove_data(state, data);
  return 0;
}

static nghttp2_ssize qh2_read_data(nghttp2_session *session,
                                   int32_t stream_id, uint8_t *buffer,
                                   size_t capacity, uint32_t *flags,
                                   nghttp2_data_source *source,
                                   void *user_data) {
  (void)session;
  (void)stream_id;
  (void)user_data;
  qh2_data *data = (qh2_data *)source->ptr;
  if (!data->pending) {
    data->deferred = 1;
    return NGHTTP2_ERR_DEFERRED;
  }
  size_t remaining = data->len - data->offset;
  size_t amount = remaining < capacity ? remaining : capacity;
  if (amount) memcpy(buffer, data->bytes + data->offset, amount);
  data->offset += amount;
  if (data->offset == data->len) {
    free(data->bytes);
    data->bytes = NULL;
    data->len = 0;
    data->offset = 0;
    data->pending = 0;
    if (data->eof) {
      *flags |= NGHTTP2_DATA_FLAG_EOF;
      qh2_remove_data(data->owner, data);
    }
  }
  return (nghttp2_ssize)amount;
}

static qh2_data *qh2_new_data(qh2 *state, int32_t stream_id) {
  qh2_data *data = (qh2_data *)calloc(1, sizeof(qh2_data));
  if (!data) return NULL;
  data->owner = state;
  data->stream_id = stream_id;
  data->next = state->data_sources;
  state->data_sources = data;
  return data;
}

static nghttp2_data_provider2 qh2_provider(qh2_data *data) {
  nghttp2_data_provider2 provider;
  memset(&provider, 0, sizeof(provider));
  provider.source.ptr = data;
  provider.read_callback = qh2_read_data;
  return provider;
}

static nghttp2_nv *qh2_make_nv(const uint8_t *const *names,
                               const size_t *name_lens,
                               const uint8_t *const *values,
                               const size_t *value_lens, size_t count) {
  nghttp2_nv *nva = (nghttp2_nv *)calloc(count ? count : 1, sizeof(nghttp2_nv));
  if (!nva) return NULL;
  for (size_t i = 0; i < count; i++) {
    nva[i].name = (uint8_t *)names[i];
    nva[i].namelen = name_lens[i];
    nva[i].value = (uint8_t *)values[i];
    nva[i].valuelen = value_lens[i];
    nva[i].flags = NGHTTP2_NV_FLAG_NONE;
  }
  return nva;
}

void *qh2_new(int server) {
  qh2 *state = (qh2 *)calloc(1, sizeof(qh2));
  nghttp2_session_callbacks *callbacks = NULL;
  if (!state) return NULL;
  if (nghttp2_session_callbacks_new(&callbacks) != 0) {
    free(state);
    return NULL;
  }
  nghttp2_session_callbacks_set_on_begin_headers_callback(callbacks, qh2_begin_headers);
  nghttp2_session_callbacks_set_on_header_callback(callbacks, qh2_on_header);
  nghttp2_session_callbacks_set_on_data_chunk_recv_callback(callbacks, qh2_on_data);
  nghttp2_session_callbacks_set_on_frame_recv_callback(callbacks, qh2_on_frame);
  nghttp2_session_callbacks_set_on_stream_close_callback(callbacks, qh2_on_close);
  int result = server
      ? nghttp2_session_server_new(&state->session, callbacks, state)
      : nghttp2_session_client_new(&state->session, callbacks, state);
  nghttp2_session_callbacks_del(callbacks);
  if (result != 0) {
    free(state);
    return NULL;
  }
  /* RFC 9113 requires each endpoint to send an initial SETTINGS frame. The
   * client preface alone is not sufficient: without this, a first request's
   * HEADERS can reach the peer before any SETTINGS and nghttp2 rejects it as
   * "SETTINGS expected". Queue an empty SETTINGS frame for both roles so the
   * JS socket adapter can continue using mem_send as its sole output path. */
  result = nghttp2_submit_settings(state->session, NGHTTP2_FLAG_NONE, NULL, 0);
  if (result != 0) {
    nghttp2_session_del(state->session);
    free(state);
    return NULL;
  }
  return state;
}

int qh2_set_local_window_size(void *opaque, int32_t size) {
  qh2 *state = (qh2 *)opaque;
  if (!state || size < 1) return NGHTTP2_ERR_INVALID_ARGUMENT;
  return nghttp2_session_set_local_window_size(
      state->session, NGHTTP2_FLAG_NONE, 0, size);
}

void qh2_free(void *opaque) {
  qh2 *state = (qh2 *)opaque;
  if (!state) return;
  if (state->session) nghttp2_session_del(state->session);
  qh2_free_event(state->current_headers);
  while (state->events_head) {
    qh2_event *next = state->events_head->next;
    qh2_free_event(state->events_head);
    state->events_head = next;
  }
  while (state->data_sources) qh2_remove_data(state, state->data_sources);
  free(state);
}

int64_t qh2_mem_recv(void *opaque, const uint8_t *bytes, size_t len) {
  return (int64_t)nghttp2_session_mem_recv2(((qh2 *)opaque)->session, bytes, len);
}

int64_t qh2_mem_send(void *opaque, const uint8_t **bytes) {
  return (int64_t)nghttp2_session_mem_send2(((qh2 *)opaque)->session, bytes);
}

int qh2_submit_headers(void *opaque, int32_t stream_id,
                       const uint8_t *const *names, const size_t *name_lens,
                       const uint8_t *const *values, const size_t *value_lens,
                       size_t count, int end_stream) {
  qh2 *state = (qh2 *)opaque;
  nghttp2_nv *nva = qh2_make_nv(names, name_lens, values, value_lens, count);
  if (!nva) return NGHTTP2_ERR_NOMEM;
  int32_t result = nghttp2_submit_headers(
      state->session, end_stream ? NGHTTP2_FLAG_END_STREAM : NGHTTP2_FLAG_NONE,
      stream_id, NULL, nva, count, NULL);
  free(nva);
  return result < 0 ? (int)result : 0;
}

int qh2_submit_settings(void *opaque, const uint32_t *ids,
                        const uint32_t *values, size_t count) {
  qh2 *state = (qh2 *)opaque;
  nghttp2_settings_entry *entries = (nghttp2_settings_entry *)calloc(
      count ? count : 1, sizeof(nghttp2_settings_entry));
  if (!entries) return NGHTTP2_ERR_NOMEM;
  for (size_t i = 0; i < count; i++) {
    entries[i].settings_id = (nghttp2_settings_id)ids[i];
    entries[i].value = values[i];
  }
  int result = nghttp2_submit_settings(state->session, NGHTTP2_FLAG_NONE,
                                       entries, count);
  free(entries);
  return result;
}

int qh2_submit_rst_stream(void *opaque, int32_t stream_id, uint32_t error_code) {
  qh2 *state = (qh2 *)opaque;
  if (!state || stream_id <= 0) return NGHTTP2_ERR_INVALID_ARGUMENT;
  return nghttp2_submit_rst_stream(state->session, NGHTTP2_FLAG_NONE,
                                   stream_id, error_code);
}

int qh2_submit_data(void *opaque, int32_t stream_id, const uint8_t *bytes,
                    size_t len, int end_stream) {
  qh2 *state = (qh2 *)opaque;
  qh2_data *data = qh2_find_data(state, stream_id);
  if (data) {
    if (data->eof) return NGHTTP2_ERR_INVALID_ARGUMENT;
    if (data->pending) {
      /* Writable callbacks may enqueue more than one chunk before nghttp2's
       * peer flow-control window opens. Keep the bytes queued on the existing
       * provider instead of rejecting the next write as a second pending
       * provider submission. Preserve offset because a prefix may already
       * have been sent when the stream ran out of window. */
      if (len > SIZE_MAX - data->len) return NGHTTP2_ERR_NOMEM;
      uint8_t *queued = (uint8_t *)realloc(data->bytes, data->len + len);
      if (!queued && data->len + len != 0) return NGHTTP2_ERR_NOMEM;
      data->bytes = queued;
      if (len) memcpy(data->bytes + data->len, bytes, len);
      data->len += len;
      data->eof = end_stream;
      return 0;
    }
    data->bytes = (uint8_t *)malloc(len ? len : 1);
    if (!data->bytes) return NGHTTP2_ERR_NOMEM;
    if (len) memcpy(data->bytes, bytes, len);
    data->len = len;
    data->pending = 1;
    data->eof = end_stream;
    if (data->deferred) {
      data->deferred = 0;
      return nghttp2_session_resume_data(state->session, stream_id);
    }
    return 0;
  }
  data = qh2_new_data(state, stream_id);
  if (!data) return NGHTTP2_ERR_NOMEM;
  data->bytes = (uint8_t *)malloc(len ? len : 1);
  if (!data->bytes) {
    qh2_remove_data(state, data);
    return NGHTTP2_ERR_NOMEM;
  }
  if (len) memcpy(data->bytes, bytes, len);
  data->len = len;
  data->pending = 1;
  data->eof = end_stream;
  nghttp2_data_provider2 provider = qh2_provider(data);
  int result = nghttp2_submit_data2(
      /* Set END_STREAM on the provider even for an intermediate chunk. The
       * provider only marks EOF once the caller submits its final chunk; the
       * flag is required for nghttp2 to put END_STREAM on that eventual DATA
       * frame. */
      state->session, NGHTTP2_FLAG_END_STREAM,
      stream_id, &provider);
  if (result != 0) qh2_remove_data(state, data);
  return result;
}

int32_t qh2_submit_request(void *opaque, const uint8_t *const *names,
                           const size_t *name_lens,
                           const uint8_t *const *values,
                           const size_t *value_lens, size_t count,
                           int end_stream) {
  qh2 *state = (qh2 *)opaque;
  nghttp2_nv *nva = qh2_make_nv(names, name_lens, values, value_lens, count);
  if (!nva) return NGHTTP2_ERR_NOMEM;
  qh2_data *data = NULL;
  nghttp2_data_provider2 provider;
  nghttp2_data_provider2 *provider_ptr = NULL;
  if (!end_stream) {
    data = qh2_new_data(state, -1);
    if (!data) {
      free(nva);
      return NGHTTP2_ERR_NOMEM;
    }
    provider = qh2_provider(data);
    provider_ptr = &provider;
  }
  int32_t stream_id = nghttp2_submit_request2(
      state->session, NULL, nva, count, provider_ptr, NULL);
  free(nva);
  if (stream_id < 0) {
    if (data) qh2_remove_data(state, data);
    return stream_id;
  }
  if (data) data->stream_id = stream_id;
  return stream_id;
}

int32_t qh2_submit_push_promise(void *opaque, int32_t stream_id,
                                const uint8_t *const *names,
                                const size_t *name_lens,
                                const uint8_t *const *values,
                                const size_t *value_lens, size_t count) {
  qh2 *state = (qh2 *)opaque;
  nghttp2_nv *nva = qh2_make_nv(names, name_lens, values, value_lens, count);
  if (!nva) return NGHTTP2_ERR_NOMEM;
  int32_t promised_stream_id = nghttp2_submit_push_promise(
      state->session, NGHTTP2_FLAG_NONE, stream_id, nva, count, NULL);
  free(nva);
  return promised_stream_id;
}

void *qh2_next_event(void *opaque) {
  qh2 *state = (qh2 *)opaque;
  qh2_event *event = state->events_head;
  if (!event) return NULL;
  state->events_head = event->next;
  if (!state->events_head) state->events_tail = NULL;
  event->next = NULL;
  return event;
}

int qh2_event_kind(void *opaque) { return ((qh2_event *)opaque)->kind; }
int32_t qh2_event_stream_id(void *opaque) { return ((qh2_event *)opaque)->stream_id; }
int32_t qh2_event_parent_stream_id(void *opaque) {
  return ((qh2_event *)opaque)->parent_stream_id;
}
uint32_t qh2_event_code(void *opaque) { return ((qh2_event *)opaque)->code; }
int qh2_event_end_stream(void *opaque) { return ((qh2_event *)opaque)->end_stream; }
const uint8_t *qh2_event_data(void *opaque) { return ((qh2_event *)opaque)->data; }
size_t qh2_event_data_len(void *opaque) { return ((qh2_event *)opaque)->data_len; }
size_t qh2_event_header_count(void *opaque) { return ((qh2_event *)opaque)->header_count; }

int qh2_event_header(void *opaque, size_t index, const uint8_t **name,
                     size_t *name_len, const uint8_t **value,
                     size_t *value_len) {
  qh2_header *header = ((qh2_event *)opaque)->headers;
  while (header && index--) header = header->next;
  if (!header) return -1;
  *name = header->name;
  *name_len = header->name_len;
  *value = header->value;
  *value_len = header->value_len;
  return 0;
}

void qh2_event_free(void *opaque) { qh2_free_event((qh2_event *)opaque); }
const char *qh2_error_string(int code) { return nghttp2_strerror(code); }
