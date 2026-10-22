// Node-API contract addon: each export exercises one family of the C ABI and reports
// statuses and values to the JavaScript driver, which compares engines line by line.
#include <node_api.h>
#include <pthread.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#define CHECK(call)                                                            \
  do {                                                                         \
    if ((call) != napi_ok) {                                                   \
      const napi_extended_error_info* info;                                    \
      napi_get_last_error_info(env, &info);                                    \
      napi_throw_error(env, "CONTRACT", info->error_message                    \
                                             ? info->error_message             \
                                             : "unknown failure");             \
      return NULL;                                                             \
    }                                                                          \
  } while (0)

static napi_value args_of(napi_env env, napi_callback_info info, size_t* argc,
                          napi_value* argv, napi_value* self) {
  napi_get_cb_info(env, info, argc, argv, self, NULL);
  return NULL;
}

static void set_int(napi_env env, napi_value object, const char* name,
                    int64_t value) {
  napi_value number;
  napi_create_int64(env, value, &number);
  napi_set_named_property(env, object, name, number);
}

static void set_text(napi_env env, napi_value object, const char* name,
                     const char* text) {
  napi_value string;
  napi_create_string_utf8(env, text, NAPI_AUTO_LENGTH, &string);
  napi_set_named_property(env, object, name, string);
}

static const char* status_message(napi_env env) {
  const napi_extended_error_info* info;
  napi_get_last_error_info(env, &info);
  return info->error_message ? info->error_message : "ok";
}

static napi_value Types(napi_env env, napi_callback_info info) {
  size_t argc = 8;
  napi_value argv[8], result;
  args_of(env, info, &argc, argv, NULL);
  CHECK(napi_create_array_with_length(env, argc, &result));
  for (size_t i = 0; i < argc; i++) {
    napi_valuetype type;
    napi_value code;
    CHECK(napi_typeof(env, argv[i], &type));
    napi_create_int32(env, type, &code);
    napi_set_element(env, result, (uint32_t)i, code);
  }
  return result;
}

static napi_value Strings(napi_env env, napi_callback_info info) {
  size_t argc = 1, length;
  napi_value argv[1], result, value;
  char small[6];
  char16_t units[4];
  args_of(env, info, &argc, argv, NULL);
  napi_create_object(env, &result);
  CHECK(napi_get_value_string_utf8(env, argv[0], NULL, 0, &length));
  set_int(env, result, "utf8Length", (int64_t)length);
  CHECK(napi_get_value_string_utf8(env, argv[0], small, sizeof small, &length));
  set_text(env, result, "utf8Prefix", small);
  set_int(env, result, "utf8PrefixLength", (int64_t)length);
  CHECK(napi_get_value_string_utf16(env, argv[0], units, 4, &length));
  set_int(env, result, "utf16PrefixLength", (int64_t)length);
  set_int(env, result, "utf16First", units[0]);
  CHECK(napi_get_value_string_latin1(env, argv[0], small, sizeof small, &length));
  set_int(env, result, "latin1First", (unsigned char)small[0]);
  CHECK(napi_create_string_utf8(env, "a\xff" "b", 3, &value));
  napi_set_named_property(env, result, "invalidUtf8", value);
  CHECK(napi_create_string_latin1(env, "caf\xe9", NAPI_AUTO_LENGTH, &value));
  napi_set_named_property(env, result, "latin1", value);
  const char16_t text16[] = {0x68, 0xd83d, 0xde00, 0};
  CHECK(napi_create_string_utf16(env, text16, NAPI_AUTO_LENGTH, &value));
  napi_set_named_property(env, result, "utf16", value);
  set_text(env, result, "notString",
           napi_get_value_string_utf8(env, result, small, 2, &length) ==
                   napi_string_expected
               ? status_message(env)
               : "accepted");
  return result;
}

static napi_value Numbers(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1], result;
  int32_t i32;
  uint32_t u32;
  int64_t i64;
  double d;
  args_of(env, info, &argc, argv, NULL);
  napi_create_object(env, &result);
  if (napi_get_value_double(env, argv[0], &d) != napi_ok) {
    set_text(env, result, "error", status_message(env));
    return result;
  }
  napi_get_value_int32(env, argv[0], &i32);
  napi_get_value_uint32(env, argv[0], &u32);
  napi_get_value_int64(env, argv[0], &i64);
  set_int(env, result, "int32", i32);
  set_int(env, result, "uint32", u32);
  set_int(env, result, "int64", i64 == INT64_MAX ? -1 : i64);
  return result;
}

static napi_value Answer(napi_env env, napi_callback_info info) {
  napi_value value;
  void* data;
  napi_get_cb_info(env, info, NULL, NULL, NULL, &data);
  napi_create_int32(env, (int32_t)(intptr_t)data, &value);
  return value;
}

static napi_value Store(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1], self;
  napi_get_cb_info(env, info, &argc, argv, &self, NULL);
  napi_set_named_property(env, self, "_stored", argv[0]);
  return NULL;
}

static napi_value Properties(napi_env env, napi_callback_info info) {
  napi_value object, seven, names, all;
  napi_create_object(env, &object);
  napi_create_int32(env, 7, &seven);
  napi_property_descriptor descriptors[] = {
      {"value", NULL, NULL, NULL, NULL, seven, napi_enumerable, NULL},
      {"method", NULL, Answer, NULL, NULL, NULL, napi_default_method,
       (void*)42},
      {"accessor", NULL, NULL, Answer, Store, NULL, napi_enumerable,
       (void*)9},
      {"hidden", NULL, NULL, NULL, NULL, seven, napi_writable, NULL},
  };
  CHECK(napi_define_properties(env, object, 4, descriptors));
  CHECK(napi_get_property_names(env, object, &names));
  napi_set_named_property(env, object, "_names", names);
  CHECK(napi_get_all_property_names(env, object, napi_key_own_only,
                                    napi_key_writable, napi_key_keep_numbers,
                                    &all));
  napi_set_named_property(env, object, "_writable", all);
  bool has;
  napi_value key;
  napi_create_string_utf8(env, "hidden", NAPI_AUTO_LENGTH, &key);
  CHECK(napi_has_own_property(env, object, key, &has));
  set_int(env, object, "_hasHidden", has);
  set_text(env, object, "_badName",
           napi_has_own_property(env, object, seven, &has) ==
                   napi_name_expected
               ? status_message(env)
               : "accepted");
  CHECK(napi_delete_property(env, object, key, &has));
  set_int(env, object, "_deleted", has);
  return object;
}

typedef struct {
  int32_t count;
} Counter;

static int32_t finalized_counters;

static void CounterFinalize(napi_env env, void* data, void* hint) {
  finalized_counters++;
  free(data);
}

static napi_value CounterNew(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1], self, target;
  napi_get_cb_info(env, info, &argc, argv, &self, NULL);
  CHECK(napi_get_new_target(env, info, &target));
  if (target == NULL) {
    napi_throw_type_error(env, "ERR_CALL", "Counter requires new");
    return NULL;
  }
  Counter* counter = malloc(sizeof *counter);
  counter->count = 0;
  if (argc > 0) napi_get_value_int32(env, argv[0], &counter->count);
  CHECK(napi_wrap(env, self, counter, CounterFinalize, NULL, NULL));
  set_text(env, self, "rewrap",
           napi_wrap(env, self, counter, NULL, NULL, NULL) == napi_invalid_arg
               ? status_message(env)
               : "accepted");
  return self;
}

static napi_value CounterIncrement(napi_env env, napi_callback_info info) {
  napi_value self, value;
  Counter* counter;
  napi_get_cb_info(env, info, NULL, NULL, &self, NULL);
  CHECK(napi_unwrap(env, self, (void**)&counter));
  napi_create_int32(env, ++counter->count, &value);
  return value;
}

static napi_value CounterValue(napi_env env, napi_callback_info info) {
  napi_value self, value;
  Counter* counter;
  napi_get_cb_info(env, info, NULL, NULL, &self, NULL);
  CHECK(napi_unwrap(env, self, (void**)&counter));
  napi_create_int32(env, counter->count, &value);
  return value;
}

static napi_value CounterFinalized(napi_env env, napi_callback_info info) {
  napi_value value;
  napi_create_int32(env, finalized_counters, &value);
  return value;
}

static napi_value DefineCounter(napi_env env) {
  napi_value constructor;
  napi_property_descriptor members[] = {
      {"increment", NULL, CounterIncrement, NULL, NULL, NULL,
       napi_default_method, NULL},
      {"value", NULL, NULL, CounterValue, NULL, NULL, napi_default, NULL},
      {"finalized", NULL, CounterFinalized, NULL, NULL, NULL,
       napi_static | napi_default_method, NULL},
  };
  napi_define_class(env, "Counter", NAPI_AUTO_LENGTH, CounterNew, NULL, 3,
                    members, &constructor);
  return constructor;
}

static napi_value Errors(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1], result, error, code, message, global;
  bool pending;
  args_of(env, info, &argc, argv, NULL);
  napi_create_object(env, &result);
  napi_get_global(env, &global);
  napi_status status = napi_call_function(env, global, argv[0], 0, NULL, NULL);
  set_int(env, result, "callStatus", status);
  napi_is_exception_pending(env, &pending);
  set_int(env, result, "pending", pending);
  set_int(env, result, "blockedStatus",
          napi_call_function(env, global, argv[0], 0, NULL, NULL));
  napi_get_and_clear_last_exception(env, &error);
  napi_set_named_property(env, result, "caught", error);
  napi_is_exception_pending(env, &pending);
  set_int(env, result, "pendingAfter", pending);
  napi_create_string_utf8(env, "E_RANGE", NAPI_AUTO_LENGTH, &code);
  napi_create_string_utf8(env, "out of range", NAPI_AUTO_LENGTH, &message);
  CHECK(napi_create_range_error(env, code, message, &error));
  napi_set_named_property(env, result, "created", error);
  bool is_error;
  napi_is_error(env, error, &is_error);
  set_int(env, result, "isError", is_error);
  set_text(env, result, "badMessage",
           napi_create_error(env, NULL, result, &error) ==
                   napi_string_expected
               ? status_message(env)
               : "accepted");
  return result;
}

static napi_value ThrowCoded(napi_env env, napi_callback_info info) {
  napi_throw_type_error(env, "ERR_CONTRACT", "typed failure");
  return NULL;
}

static napi_value References(napi_env env, napi_callback_info info) {
  napi_value object, result, value;
  napi_ref reference;
  uint32_t count;
  napi_create_object(env, &object);
  napi_create_object(env, &result);
  CHECK(napi_create_reference(env, object, 1, &reference));
  CHECK(napi_reference_ref(env, reference, &count));
  set_int(env, result, "afterRef", count);
  CHECK(napi_reference_unref(env, reference, &count));
  CHECK(napi_reference_unref(env, reference, &count));
  set_int(env, result, "afterUnref", count);
  set_int(env, result, "underflow",
          napi_reference_unref(env, reference, &count));
  CHECK(napi_get_reference_value(env, reference, &value));
  bool same;
  napi_strict_equals(env, value, object, &same);
  set_int(env, result, "same", same);
  CHECK(napi_delete_reference(env, reference));
  return result;
}

static napi_value Scopes(napi_env env, napi_callback_info info) {
  napi_value result, kept = NULL, escaped;
  napi_escapable_handle_scope outer;
  for (int i = 0; i < 2000; i++) {
    napi_handle_scope scope;
    CHECK(napi_open_handle_scope(env, &scope));
    napi_value text;
    napi_create_string_utf8(env, "transient", NAPI_AUTO_LENGTH, &text);
    CHECK(napi_close_handle_scope(env, scope));
  }
  CHECK(napi_open_escapable_handle_scope(env, &outer));
  napi_create_string_utf8(env, "escaped", NAPI_AUTO_LENGTH, &kept);
  CHECK(napi_escape_handle(env, outer, kept, &escaped));
  napi_status twice = napi_escape_handle(env, outer, kept, &kept);
  CHECK(napi_close_escapable_handle_scope(env, outer));
  napi_create_object(env, &result);
  napi_set_named_property(env, result, "value", escaped);
  set_int(env, result, "twice", twice);
  return result;
}

static void FreeBytes(napi_env env, void* data, void* hint) { free(data); }

static napi_value Buffers(napi_env env, napi_callback_info info) {
  size_t argc = 1, length;
  napi_value argv[1], result, buffer, array_buffer, view, data_view;
  void* data;
  args_of(env, info, &argc, argv, NULL);
  napi_create_object(env, &result);
  CHECK(napi_get_buffer_info(env, argv[0], &data, &length));
  ((uint8_t*)data)[0] = 0x7a;
  set_int(env, result, "inputLength", (int64_t)length);
  CHECK(napi_create_buffer_copy(env, 3, "xyz", &data, &buffer));
  napi_set_named_property(env, result, "copy", buffer);
  char* external = malloc(4);
  memcpy(external, "ext!", 4);
  CHECK(napi_create_external_buffer(env, 4, external, FreeBytes, NULL, &buffer));
  napi_set_named_property(env, result, "external", buffer);
  CHECK(napi_create_arraybuffer(env, 8, &data, &array_buffer));
  ((uint8_t*)data)[1] = 0xff;
  napi_set_named_property(env, result, "arrayBuffer", array_buffer);
  CHECK(napi_create_typedarray(env, napi_uint16_array, 2, array_buffer, 2, &view));
  napi_set_named_property(env, result, "uint16", view);
  napi_typedarray_type type;
  size_t offset;
  CHECK(napi_get_typedarray_info(env, view, &type, &length, &data, NULL, &offset));
  set_int(env, result, "viewType", type);
  set_int(env, result, "viewLength", (int64_t)length);
  set_int(env, result, "viewOffset", (int64_t)offset);
  set_int(env, result, "misaligned",
          napi_create_typedarray(env, napi_int32_array, 1, array_buffer, 1, &view));
  napi_value misaligned;
  napi_get_and_clear_last_exception(env, &misaligned);
  napi_set_named_property(env, result, "misalignedError", misaligned);
  CHECK(napi_create_dataview(env, 4, array_buffer, 4, &data_view));
  napi_set_named_property(env, result, "dataView", data_view);
  set_int(env, result, "dataViewTooLong",
          napi_create_dataview(env, 8, array_buffer, 4, &data_view));
  napi_value too_long;
  napi_get_and_clear_last_exception(env, &too_long);
  napi_set_named_property(env, result, "dataViewError", too_long);
  bool flag;
  napi_is_arraybuffer(env, array_buffer, &flag);
  set_int(env, result, "isArrayBuffer", flag);
  napi_is_typedarray(env, array_buffer, &flag);
  set_int(env, result, "arrayBufferIsTyped", flag);
  napi_is_buffer(env, argv[0], &flag);
  set_int(env, result, "isBuffer", flag);
  return result;
}

static napi_value Externals(napi_env env, napi_callback_info info) {
  static int target = 5;
  static const napi_type_tag tag = {0x1234, 0x5678};
  static const napi_type_tag other = {0x1234, 0x5679};
  napi_value external, result, object;
  void* data;
  napi_valuetype type;
  bool matches;
  napi_create_object(env, &result);
  CHECK(napi_create_external(env, &target, NULL, NULL, &external));
  napi_typeof(env, external, &type);
  set_int(env, result, "type", type);
  CHECK(napi_get_value_external(env, external, &data));
  set_int(env, result, "value", *(int*)data);
  napi_create_object(env, &object);
  CHECK(napi_type_tag_object(env, object, &tag));
  set_int(env, result, "retag", napi_type_tag_object(env, object, &tag));
  napi_check_object_type_tag(env, object, &tag, &matches);
  set_int(env, result, "tagMatches", matches);
  napi_check_object_type_tag(env, object, &other, &matches);
  set_int(env, result, "otherMatches", matches);
  return result;
}

typedef struct {
  napi_async_work work;
  napi_deferred deferred;
  int64_t input;
  int64_t output;
} SumWork;

static void SumExecute(napi_env env, void* data) {
  SumWork* work = data;
  for (int64_t i = 1; i <= work->input; i++) work->output += i;
}

static void SumComplete(napi_env env, napi_status status, void* data) {
  SumWork* work = data;
  napi_value value;
  napi_create_int64(env, work->output, &value);
  if (status == napi_ok) napi_resolve_deferred(env, work->deferred, value);
  napi_delete_async_work(env, work->work);
  free(work);
}

static napi_value Sum(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1], promise, name;
  args_of(env, info, &argc, argv, NULL);
  SumWork* work = calloc(1, sizeof *work);
  napi_get_value_int64(env, argv[0], &work->input);
  CHECK(napi_create_promise(env, &work->deferred, &promise));
  napi_create_string_utf8(env, "sum", NAPI_AUTO_LENGTH, &name);
  CHECK(napi_create_async_work(env, NULL, name, SumExecute, SumComplete, work,
                               &work->work));
  CHECK(napi_queue_async_work(env, work->work));
  return promise;
}

static napi_threadsafe_function stream;

static void StreamCall(napi_env env, napi_value callback, void* context,
                       void* data) {
  if (env == NULL) return;
  napi_value value, global;
  napi_create_int32(env, (int32_t)(intptr_t)data, &value);
  napi_get_global(env, &global);
  napi_call_function(env, global, callback, 1, &value, NULL);
}

static void StreamFinalize(napi_env env, void* data, void* context) {
  napi_value callback, global, value;
  napi_get_reference_value(env, (napi_ref)data, &callback);
  napi_get_global(env, &global);
  napi_create_string_utf8(env, "finalized", NAPI_AUTO_LENGTH, &value);
  napi_call_function(env, global, callback, 1, &value, NULL);
  napi_delete_reference(env, (napi_ref)data);
}

static void* StreamProducer(void* argument) {
  for (intptr_t i = 1; i <= 3; i++)
    napi_call_threadsafe_function(stream, (void*)i, napi_tsfn_blocking);
  napi_release_threadsafe_function(stream, napi_tsfn_release);
  return NULL;
}

static napi_value Stream(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1], name;
  napi_ref callback;
  pthread_t thread;
  args_of(env, info, &argc, argv, NULL);
  napi_create_reference(env, argv[0], 1, &callback);
  napi_create_string_utf8(env, "stream", NAPI_AUTO_LENGTH, &name);
  CHECK(napi_create_threadsafe_function(env, argv[0], NULL, name, 1, 1,
                                        callback, StreamFinalize, NULL,
                                        StreamCall, &stream));
  pthread_create(&thread, NULL, StreamProducer, NULL);
  pthread_detach(thread);
  return NULL;
}

static napi_value Misc(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2], result, value, script;
  args_of(env, info, &argc, argv, NULL);
  napi_create_object(env, &result);
  bool flag;
  CHECK(napi_instanceof(env, argv[0], argv[1], &flag));
  set_int(env, result, "instanceOf", flag);
  CHECK(napi_coerce_to_string(env, argv[0], &value));
  napi_set_named_property(env, result, "string", value);
  CHECK(napi_coerce_to_number(env, argv[0], &value));
  napi_set_named_property(env, result, "number", value);
  napi_create_string_utf8(env, "[1, 2].map((x) => x * 3).join()",
                          NAPI_AUTO_LENGTH, &script);
  CHECK(napi_run_script(env, script, &value));
  napi_set_named_property(env, result, "script", value);
  uint32_t version;
  CHECK(napi_get_version(env, &version));
  set_int(env, result, "napiVersion", version);
  const napi_node_version* node;
  CHECK(napi_get_node_version(env, &node));
  set_int(env, result, "nodeMajor", node->major);
  set_text(env, result, "release", node->release);
  CHECK(napi_create_symbol(env, value, &value));
  napi_set_named_property(env, result, "symbol", value);
  set_int(env, result, "notConstructor",
          napi_instanceof(env, argv[0], result, &flag));
  napi_get_and_clear_last_exception(env, &value);
  napi_set_named_property(env, result, "notConstructorError", value);
  return result;
}

NAPI_MODULE_INIT() {
  napi_property_descriptor exports_list[] = {
      {"types", NULL, Types, NULL, NULL, NULL, napi_enumerable, NULL},
      {"strings", NULL, Strings, NULL, NULL, NULL, napi_enumerable, NULL},
      {"numbers", NULL, Numbers, NULL, NULL, NULL, napi_enumerable, NULL},
      {"properties", NULL, Properties, NULL, NULL, NULL, napi_enumerable, NULL},
      {"errors", NULL, Errors, NULL, NULL, NULL, napi_enumerable, NULL},
      {"throwCoded", NULL, ThrowCoded, NULL, NULL, NULL, napi_enumerable, NULL},
      {"references", NULL, References, NULL, NULL, NULL, napi_enumerable, NULL},
      {"scopes", NULL, Scopes, NULL, NULL, NULL, napi_enumerable, NULL},
      {"buffers", NULL, Buffers, NULL, NULL, NULL, napi_enumerable, NULL},
      {"externals", NULL, Externals, NULL, NULL, NULL, napi_enumerable, NULL},
      {"sum", NULL, Sum, NULL, NULL, NULL, napi_enumerable, NULL},
      {"stream", NULL, Stream, NULL, NULL, NULL, napi_enumerable, NULL},
      {"misc", NULL, Misc, NULL, NULL, NULL, napi_enumerable, NULL},
      {"Counter", NULL, NULL, NULL, NULL, DefineCounter(env), napi_enumerable,
       NULL},
  };
  napi_define_properties(env, exports, 14, exports_list);
  return exports;
}
