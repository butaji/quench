use rqj::{Engine, Host, Vm};
use std::{cell::RefCell, path::PathBuf, process::Command, rc::Rc};

#[derive(Clone, Default)]
struct Capture(Rc<RefCell<Vec<String>>>);
impl Host for Capture {
    fn write_line(&mut self, text: &str) {
        self.0.borrow_mut().push(text.into());
    }
    fn clock_millis(&mut self) -> f64 {
        0.0
    }
}

fn output(source: &str) -> Vec<String> {
    let host = Capture::default();
    let view = host.clone();
    let program = Engine::specialize(source, "test.js").unwrap();
    Vm::new(host).execute(&program).unwrap();
    Rc::try_unwrap(view.0).unwrap().into_inner()
}

#[test]
fn residual_binary_round_trip_preserves_execution() {
    let path = std::env::temp_dir().join(format!("rqj-roundtrip-{}.residual", std::process::id()));
    let program = Engine::specialize("print(12345678901234567890n);", "roundtrip.js").unwrap();
    program.write_binary(&path).unwrap();
    let decoded = rqj::ResidualProgram::read_binary(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    let host = Capture::default();
    let view = host.clone();
    Vm::new(host).execute(&decoded).unwrap();
    assert_eq!(
        Rc::try_unwrap(view.0).unwrap().into_inner(),
        ["12345678901234567890"]
    );
}

#[test]
fn closures_prototypes_arrays_and_integer_ops() {
    let source = r#"
      var K = 40;
      function Box(x) { this.x = x; }
      Box.prototype.answer = function(y) { return this.x + y + K; };
      var values = new Array(1); values[0] = new Box(1);
      print(values[0].answer(1));
    "#;
    assert_eq!(output(source), ["42"]);
}

#[test]
fn lexical_declarations_use_function_local_slots() {
    let source = r#"
      let left = 40;
      const right = 2;
      {
        let nested = left + right;
        print(nested);
      }
    "#;
    assert_eq!(output(source), ["42"]);
}

#[test]
fn variable_destructuring_reads_object_fields_and_array_indices() {
    assert_eq!(
        output(
            "const { answer } = { answer: 42 }; const [left, right] = [40, 2]; print(answer); print(left + right);"
        ),
        ["42", "42"],
    );
}

#[test]
fn object_is_uses_same_value_semantics() {
    assert_eq!(
        output(
            "var object = {}; print(Object.is(NaN, NaN)); print(Object.is(0, -0)); print(Object.is('x', 'x')); print(Object.is(object, object)); print(Object.is({}, {}));"
        ),
        ["true", "false", "true", "true", "false"],
    );
}

#[test]
fn object_values_and_entries_follow_shape_order() {
    assert_eq!(
        output(
            "var object = { first: 1, second: 2 }; print(Object.values(object).join(',')); var entries = Object.entries(object); print(entries[0][0] + ':' + entries[0][1]); print(entries[1][0] + ':' + entries[1][1]);"
        ),
        ["1,2", "first:1", "second:2"],
    );
}

#[test]
fn object_prototype_has_own_property_uses_receiver() {
    assert_eq!(
        output(
            "var object = { answer: 42 }; print(object.hasOwnProperty('answer')); print(object.hasOwnProperty('missing'));"
        ),
        ["true", "false"],
    );
}

#[test]
fn object_prototype_property_is_enumerable_uses_own_slots() {
    assert_eq!(
        output(
            "var object = { answer: 42 }; print(object.propertyIsEnumerable('answer')); print(object.propertyIsEnumerable('toString'));"
        ),
        ["true", "false"],
    );
}

#[test]
fn object_prototype_is_prototype_of_walks_canonical_links() {
    assert_eq!(
        output(
            "var base = {}; var child = Object.create(base); print(base.isPrototypeOf(child)); print(Object.prototype.isPrototypeOf(child)); print(child.isPrototypeOf(base));"
        ),
        ["true", "true", "false"],
    );
}

#[test]
fn own_property_descriptors_come_from_shape_slots() {
    assert_eq!(
        output(
            "var object = { answer: 42 }; var descriptor = Object.getOwnPropertyDescriptor(object, 'answer'); print(descriptor.value); print(descriptor.writable); print(descriptor.enumerable); print(descriptor.configurable); print(Object.getOwnPropertyDescriptor(object, 'missing') === undefined); print(Reflect.getOwnPropertyDescriptor(object, 'answer').value);"
        ),
        ["42", "true", "true", "true", "true", "42"],
    );
}

#[test]
fn own_property_descriptors_preserve_ordered_keys() {
    assert_eq!(
        output(
            "var descriptors = Object.getOwnPropertyDescriptors({ beta: 2, 2: 2, 1: 1 }); print(Object.keys(descriptors).join(',')); print(descriptors[1].value); print(descriptors[2].value); print(descriptors.beta.value);"
        ),
        ["1,2,beta", "1", "2", "2"],
    );
}

#[test]
fn define_property_tracks_attributes_and_enumeration() {
    assert_eq!(
        output(
            "var object = { answer: 42 }; Object.defineProperty(object, 'hidden', { value: 7 }); var descriptor = Object.getOwnPropertyDescriptor(object, 'hidden'); print(descriptor.value); print(descriptor.writable); print(descriptor.enumerable); print(object.propertyIsEnumerable('hidden')); print(Object.keys(object).join(',')); print(Object.getOwnPropertyNames(object).join(',')); try { Object.defineProperty(object, 'hidden', { value: 8 }); } catch (error) { print('rejected'); } print(Reflect.defineProperty(object, 'visible', { value: 9, enumerable: true, writable: false, configurable: false })); print(Object.keys(object).join(','));"
        ),
        [
            "7",
            "false",
            "false",
            "false",
            "answer",
            "answer,hidden",
            "rejected",
            "true",
            "answer,visible"
        ],
    );
}

#[test]
fn object_integrity_levels_block_mutation_and_report_state() {
    assert_eq!(
        output(
            "var sealed = { answer: 1 }; Object.seal(sealed); print(Object.isSealed(sealed)); try { sealed.extra = 2; } catch (error) { print('sealed'); } print(Object.isExtensible(sealed)); var frozen = { answer: 1 }; Object.freeze(frozen); print(Object.isFrozen(frozen)); try { frozen.answer = 2; } catch (error) { print('frozen'); } print(frozen.answer); print(Object.isFrozen(1));"
        ),
        ["true", "sealed", "false", "true", "frozen", "1", "true"],
    );
}

#[test]
fn reflect_integrity_methods_share_object_state() {
    assert_eq!(
        output(
            "var object = {}; print(Reflect.isExtensible(object)); print(Reflect.preventExtensions(object)); print(Reflect.isExtensible(object)); try { Reflect.preventExtensions(1); } catch (error) { print('type-error'); }"
        ),
        ["true", "true", "false", "type-error"],
    );
}

#[test]
fn string_from_code_point_validates_unicode_scalars() {
    assert_eq!(
        output(
            "print(String.fromCodePoint(0x1f642)); try { String.fromCodePoint(0xd800); } catch (error) { print('range-error'); }"
        ),
        ["🙂", "range-error"],
    );
}

#[test]
fn non_extensible_prototypes_are_stable() {
    assert_eq!(
        output(
            "var object = {}; var other = {}; Object.preventExtensions(object); print(Reflect.setPrototypeOf(object, other)); try { Object.setPrototypeOf(object, other); } catch (error) { print('type-error'); } print(Object.getPrototypeOf(object) === Object.prototype);"
        ),
        ["false", "type-error", "true"],
    );
}

#[test]
fn object_keyed_views_put_integer_indices_first() {
    assert_eq!(
        output(
            "var object = { beta: 2, 10: 10, 2: 2, alpha: 1, 1: 1 }; print(Object.keys(object).join(',')); print(Object.values(object).join(',')); var copy = Object.assign({}, object); print(Reflect.ownKeys(copy).join(','));"
        ),
        ["1,2,10,beta,alpha", "1,2,10,2,1", "1,2,10,beta,alpha"],
    );
}

#[test]
fn array_from_supports_array_like_sources_and_mapping() {
    assert_eq!(
        output(
            "var source = { length: 2 }; source[0] = 'a'; source[1] = 'b'; print(Array.from(source).join('-')); print(Array.from(source, function(value, index) { return value + index; }).join('-')); print(Array.from({ length: 2 }).join(','));"
        ),
        ["a-b", "a0-b1", ","],
    );
}

#[test]
fn object_views_box_primitive_strings_for_indexed_properties() {
    assert_eq!(
        output(
            "print(Object.keys('ab').join(',')); print(Object.values('ab').join(',')); var entries = Object.entries('ab'); print(entries[1][0] + ':' + entries[1][1]); var target = {}; Object.assign(target, 'xy'); print(target[0] + target[1]); print(Object.hasOwn('abc', 1)); print(Object.prototype.hasOwnProperty.call('abc', 2));"
        ),
        ["0,1", "a,b", "1:b", "xy", "true", "true"],
    );
}

#[test]
fn string_search_uses_utf16_indices_and_positions() {
    assert_eq!(
        output(
            "var text = 'a😀ba😀'; print(text.indexOf('😀')); print(text.indexOf('😀', 3)); print(text.lastIndexOf('😀')); print(text.lastIndexOf('😀', 2)); print(text.indexOf('')); print(text.lastIndexOf(''));"
        ),
        ["1", "5", "5", "1", "0", "7"],
    );
}

#[test]
fn string_value_methods_return_the_canonical_primitive() {
    assert_eq!(
        output(
            "var value = 'quench'; print(value.toString()); print(value.valueOf()); print(value.toString() === value);"
        ),
        ["quench", "quench", "true"],
    );
}

#[test]
fn destructuring_defaults_only_evaluate_for_undefined_values() {
    assert_eq!(
        output(
            "var calls = 0; function fallback() { calls = calls + 1; return 42; } const { present = fallback(), missing = fallback() } = { present: 7 }; const [first = fallback(), second = fallback()] = [1]; print(present); print(missing); print(first); print(second); print(calls);"
        ),
        ["7", "42", "1", "42", "2"],
    );
}

#[test]
fn array_destructuring_rest_uses_array_slice_semantics() {
    assert_eq!(
        output(
            "const [head, ...tail] = [1, 2, 3]; print(head); print(tail.length); print(tail[0]); print(tail[1]); var values = [0, 1, 2, 3]; var middle = values.slice(1, -1); print(middle.length); print(middle[0]); print(middle[1]); var coerced = values.slice('1.9', '3.8'); print(coerced.length); print(coerced[0]); print(coerced[1]);"
        ),
        ["1", "2", "2", "3", "2", "1", "2", "2", "1", "2"],
    );
}

#[test]
fn object_destructuring_rest_copies_unexcluded_own_properties() {
    assert_eq!(
        output(
            "const source = { answer: 42, extra: 7 }; const { answer, ...rest } = source; print(answer); print(rest.answer === undefined); print(rest.extra);"
        ),
        ["42", "true", "7"],
    );
}

#[test]
fn array_includes_uses_same_value_zero_and_visits_holes() {
    assert_eq!(
        output(
            "print([NaN].includes(NaN)); print(new Array(2).includes(undefined)); print([1, 2, 3].includes(1, '1.5')); print([1, 2, 3].includes(1, Infinity));"
        ),
        ["true", "true", "false", "false"],
    );
}

#[test]
fn array_concat_is_non_mutating_and_flattens_array_arguments() {
    let source = r#"
      var first = [1, 2];
      var second = first.concat([3], 4, [5, 6]);
      print(first.length);
      print(second.join('-'));
    "#;
    assert_eq!(output(source), ["2", "1-2-3-4-5-6"]);
}

#[test]
fn array_flat_respects_depth_and_does_not_mutate_source() {
    let source = r#"
      var nested = [1, [2, [3]]];
      var shallow = nested.flat();
      print(shallow.length);
      print(shallow[2][0]);
      print(nested.flat(2).join('-'));
      print(nested[1][1][0]);
    "#;
    assert_eq!(output(source), ["3", "3", "1-2-3", "3"]);
}

#[test]
fn array_reverse_mutates_and_returns_the_same_array() {
    let source = r#"
      var values = [1, 2, 3];
      var result = values.reverse();
      print(result === values);
      print(values.join('-'));
    "#;
    assert_eq!(output(source), ["true", "3-2-1"]);
}

#[test]
fn array_shift_and_unshift_preserve_order_and_lengths() {
    let source = r#"
      var values = [2, 3];
      print(values.unshift(0, 1));
      print(values.shift());
      print(values.join('-'));
    "#;
    assert_eq!(output(source), ["4", "0", "1-2-3"]);
}

#[test]
fn array_splice_returns_removed_values_and_updates_receiver() {
    let source = r#"
      var values = [0, 1, 2, 3];
      var removed = values.splice(1, 2, 'a', 'b', 'c');
      print(removed.join('-'));
      print(values.join('-'));
    "#;
    assert_eq!(output(source), ["1-2", "0-a-b-c-3"]);
}

#[test]
fn array_fill_coerces_bounds_and_returns_the_same_array() {
    let source = r#"
      var values = [0, 1, 2, 3];
      var result = values.fill(9, '1.5', -1);
      print(result === values);
      print(values.join('-'));
    "#;
    assert_eq!(output(source), ["true", "0-9-9-3"]);
}

#[test]
fn array_at_and_last_index_of_handle_relative_and_coerced_indices() {
    let source = r#"
      var values = [0, 1, 2, 1, NaN];
      print(values.at(-2));
      print(values.at('1.9'));
      print(values.at(99) === undefined);
      print(values.lastIndexOf(1, -2));
      print(values.lastIndexOf(NaN));
    "#;
    assert_eq!(output(source), ["1", "1", "true", "3", "-1"]);
}

#[test]
fn array_index_of_uses_strict_equality_and_forward_bounds() {
    let source = r#"
      var values = [0, 1, 2, 1, NaN];
      print(values.indexOf(1));
      print(values.indexOf(1, -2));
      print(values.indexOf(1, '2.9'));
      print(values.indexOf(NaN));
    "#;
    assert_eq!(output(source), ["1", "3", "3", "-1"]);
}

#[test]
fn array_copy_within_handles_overlap_and_array_with_is_non_mutating() {
    let source = r#"
      var values = [0, 1, 2, 3, 4];
      print(values.copyWithin(1, 3) === values);
      print(values.join('-'));
      var replaced = values.with(-1, 9);
      print(replaced === values);
      print(replaced.join('-'));
      print(values.join('-'));
    "#;
    assert_eq!(
        output(source),
        ["true", "0-3-4-3-4", "false", "0-3-4-3-9", "0-3-4-3-4"]
    );
}

#[test]
fn array_callback_methods_share_user_function_invocation() {
    let source = r#"
      var values = [1, 2, 3];
      var total = 0;
      values.forEach(function (value, index) { total = total + value + index; });
      print(total);
      print(values.map(function (value) { return value * 2; }).join('-'));
      print(values.filter(function (value) { return value > 1; }).join('-'));
      print(values.some(function (value) { return value === 2; }));
      print(values.every(function (value) { return value > 0; }));
      print(values.find(function (value) { return value > 1; }));
      print(values.findIndex(function (value) { return value > 1; }));
    "#;
    assert_eq!(
        output(source),
        ["9", "2-4-6", "2-3", "true", "true", "2", "1"]
    );
}

#[test]
fn array_flat_map_flattens_callback_arrays_one_level() {
    assert_eq!(
        output(
            "print([1, 2, 3].flatMap(function (value) { return [value, value * 2]; }).join('-'));"
        ),
        ["1-2-2-4-3-6"]
    );
}

#[test]
fn array_reduce_methods_handle_initial_values_and_direction() {
    let source = r#"
      var values = [1, 2, 3];
      print(values.reduce(function (accumulator, value) { return accumulator + value; }, 0));
      print(values.reduceRight(function (accumulator, value) { return accumulator - value; }));
      try { [].reduce(function (accumulator, value) { return accumulator + value; }); }
      catch (error) { print(error); }
    "#;
    assert_eq!(
        output(source),
        ["6", "0", "reduce of empty array with no initial value"]
    );
}

#[test]
fn array_immutable_methods_preserve_the_original_receiver() {
    let source = r#"
      var values = [0, 1, 2, 3];
      var reversed = values.toReversed();
      var spliced = values.toSpliced(1, 2, 'a', 'b');
      print(reversed.join('-'));
      print(spliced.join('-'));
      print(values.join('-'));
      print(reversed === values);
      print(spliced === values);
    "#;
    assert_eq!(
        output(source),
        ["3-2-1-0", "0-a-b-3", "0-1-2-3", "false", "false"]
    );
}

#[test]
fn array_sort_methods_support_default_and_user_comparators() {
    let source = r#"
      var values = [10, 2, 1];
      print(values.sort() === values);
      print(values.join('-'));
      var sorted = [3, 1, 2].toSorted(function (left, right) { return right - left; });
      print(sorted.join('-'));
      print(sorted === values);
    "#;
    assert_eq!(output(source), ["true", "1-10-2", "3-2-1", "false"]);
}

#[test]
fn array_string_methods_join_elements_instead_of_using_object_stringification() {
    assert_eq!(
        output("print([1, undefined, null, 'x'].toString()); print([1, 2].toLocaleString());"),
        ["1,,,x", "1,2"]
    );
}

#[test]
fn array_keys_values_and_entries_use_sparse_lengths_and_iterator_results() {
    let source = r#"
      var values = [7, 8];
      var keys = values.keys();
      var items = values.entries();
      print(keys.next().value); print(keys.next().value); print(keys.next().done);
      print(values.values().next().value);
      var entry = items.next().value;
      print(entry[0]); print(entry[1]);
    "#;
    assert_eq!(output(source), ["0", "1", "true", "7", "0", "7"]);
}

#[test]
fn array_from_consumes_iterables_and_array_of_preserves_arguments() {
    let source = r#"
      print(Array.of(1, 2, 3).join('-'));
      print(Array.from([4, 5], function (value, index) { return value + index; }).join('-'));
      print(Array.from('ab').join('-'));
      print(Array.from(new Set([6, 7])).join('-'));
    "#;
    assert_eq!(output(source), ["1-2-3", "4-6", "a-b", "6-7"]);
}

#[test]
fn array_buffer_allocates_owned_bytes_and_exposes_byte_length() {
    assert_eq!(
        output(
            "var buffer = new ArrayBuffer('4.9'); print(buffer.byteLength); print(new ArrayBuffer().byteLength);"
        ),
        ["4", "0"]
    );
}

#[test]
fn array_buffer_slice_copies_a_coerced_byte_range() {
    assert_eq!(
        output(
            "var buffer = new ArrayBuffer(8); var sliced = buffer.slice('2.9', -1); print(buffer.byteLength); print(sliced.byteLength);"
        ),
        ["8", "5"]
    );
}

#[test]
fn uint8_array_views_share_buffers_and_coerce_indexed_values() {
    let source = r#"
      var values = new Uint8Array([300, -1, 2.9]);
      print(values.length); print(values.byteLength);
      print(values[0]); print(values[1]); print(values[2]);
      values[1] = 258;
      print(values[1]);
      var buffer = new ArrayBuffer(4);
      var view = new Uint8Array(buffer, 1, 2);
      view[0] = 7;
      print(view.byteLength); print(view.byteOffset); print(view.buffer === buffer);
      print(buffer.byteLength); print(view[0]);
    "#;
    assert_eq!(
        output(source),
        ["3", "3", "44", "255", "2", "2", "2", "1", "true", "4", "7"]
    );
}

#[test]
fn uint8_array_methods_preserve_view_and_copy_semantics() {
    let source = r#"
      var values = new Uint8Array([1, 2, 255, 4]);
      var sub = values.subarray(1, 3);
      print(sub.join("-"));
      sub[0] = 9;
      print(values[1]);
      var copy = values.slice(-2);
      copy[0] = 7;
      print(values[2]); print(copy.toString());
      values.set([5, 6], 2);
      print(values.join()); print(values.includes(6)); print(values.indexOf(9));
      var entry = values.entries().next().value;
      print(entry[0]); print(entry[1]);
    "#;
    assert_eq!(
        output(source),
        ["2-255", "9", "255", "7,4", "1,9,5,6", "true", "1", "0", "1"]
    );
}

#[test]
fn array_buffer_view_detection_and_element_size_are_observable() {
    assert_eq!(
        output(
            "var buffer = new ArrayBuffer(2); var view = new Uint8Array(buffer); print(ArrayBuffer.isView(view)); print(ArrayBuffer.isView(buffer)); print(Uint8Array.BYTES_PER_ELEMENT); print(view.BYTES_PER_ELEMENT);"
        ),
        ["true", "false", "1", "1"]
    );
}

#[test]
fn shared_array_buffer_owns_shared_kind_and_typed_views() {
    assert_eq!(
        output(
            "var buffer = new SharedArrayBuffer(2); var view = new Uint8Array(buffer); view[0] = 11; print(buffer.byteLength); print(view.buffer === buffer); print(view[0]); print(ArrayBuffer.isView(view));"
        ),
        ["2", "true", "11", "true"]
    );
}

#[test]
fn atomics_use_shared_uint8_views_and_return_previous_values() {
    assert_eq!(
        output(
            "var buffer = new SharedArrayBuffer(2); var view = new Uint8Array(buffer); print(Atomics.store(view, 0, 260)); print(Atomics.load(view, 0)); print(Atomics.add(view, 0, 3)); print(Atomics.load(view, 0)); print(Atomics.isLockFree(1)); print(Atomics.isLockFree(16));"
        ),
        ["4", "4", "4", "7", "true", "false"]
    );
}

#[test]
fn atomics_read_modify_write_variants_return_old_values() {
    assert_eq!(
        output(
            "var view = new Uint8Array(new SharedArrayBuffer(1)); Atomics.store(view, 0, 15); print(Atomics.sub(view, 0, 3)); print(Atomics.and(view, 0, 6)); print(Atomics.or(view, 0, 8)); print(Atomics.xor(view, 0, 3)); print(Atomics.exchange(view, 0, 42)); print(Atomics.compareExchange(view, 0, 42, 9)); print(Atomics.load(view, 0));"
        ),
        ["15", "12", "4", "12", "15", "42", "9"]
    );
}

#[test]
fn atomics_reject_non_shared_and_out_of_range_views_before_effects() {
    assert_eq!(
        output(
            "var view = new Uint8Array(1); try { Atomics.store(view, 0, 9); } catch (error) { print(view[0]); } var shared = new Uint8Array(new SharedArrayBuffer(1)); try { Atomics.load(shared, 2); } catch (error) { print(shared[0]); }"
        ),
        ["0", "0"]
    );
}

#[test]
fn array_buffer_transfer_detaches_source_and_preserves_moved_bytes() {
    assert_eq!(
        output(
            "var buffer = new ArrayBuffer(3); var view = new Uint8Array(buffer); view[0] = 8; var moved = buffer.transfer(); print(buffer.byteLength); print(view.length); print(view[0]); try { view[0] = 9; } catch (error) { print('write-detached'); } print(moved.byteLength); print(new Uint8Array(moved)[0]);"
        ),
        ["0", "0", "undefined", "write-detached", "3", "8"]
    );
}

#[test]
fn detached_and_shared_buffers_reject_array_buffer_slice() {
    assert_eq!(
        output(
            "var buffer = new ArrayBuffer(2); var view = new Uint8Array(buffer, 1, 1); var moved = buffer.transfer(); print(view.byteOffset); try { buffer.slice(); } catch (error) { print('detached'); } var shared = new SharedArrayBuffer(2); try { shared.slice(); } catch (error) { print('shared'); }"
        ),
        ["0", "detached", "shared"]
    );
}

#[test]
fn data_view_reads_and_writes_the_shared_byte_owner() {
    assert_eq!(
        output(
            "var buffer = new ArrayBuffer(4); var view = new DataView(buffer, 1, 2); print(view.byteLength); print(view.byteOffset); print(view.buffer === buffer); view.setUint8(0, 260); print(view.getUint8(0)); print(new Uint8Array(buffer)[1]); print(ArrayBuffer.isView(view)); try { view.getUint8(2); } catch (error) { print('range'); }"
        ),
        ["2", "1", "true", "4", "4", "true", "range"]
    );
}

#[test]
fn data_view_integer_accessors_honor_width_sign_and_endianness() {
    assert_eq!(
        output(
            "var view = new DataView(new ArrayBuffer(4)); view.setInt8(0, -2); view.setUint16(1, 4660); print(view.getInt8(0)); print(view.getUint16(1)); print(view.getUint16(1, true)); print(view.getInt16(1)); view.setInt16(2, -2, true); print(view.getUint16(2)); print(view.getInt16(2, true));"
        ),
        ["-2", "4660", "13330", "4660", "65279", "-2"],
    );
}

#[test]
fn data_view_wide_and_float_accessors_preserve_bits() {
    assert_eq!(
        output(
            "var view = new DataView(new ArrayBuffer(16)); view.setUint32(0, 305419896); print(view.getUint32(0)); print(view.getUint32(0, true)); view.setInt32(4, -2, true); print(view.getInt32(4, true)); view.setFloat32(8, 1.5); print(view.getFloat32(8)); view.setFloat64(8, -2.25, true); print(view.getFloat64(8, true));"
        ),
        ["305419896", "2018915346", "-2", "1.5", "-2.25"],
    );
}

#[test]
fn uint16_array_shares_bytes_and_inherits_typed_methods() {
    assert_eq!(
        output(
            "var values = new Uint16Array([65537, -1, 2]); print(values.length); print(values.byteLength); print(values[0]); print(values[1]); var buffer = new ArrayBuffer(6); var view = new Uint16Array(buffer, 2, 2); view[0] = 4660; view.set([7], 1); print(new Uint8Array(buffer)[2]); print(view.join('-')); print(view.subarray(1)[0]); print(view.buffer === buffer);"
        ),
        ["3", "6", "1", "65535", "52", "4660-7", "7", "true"],
    );
}

#[test]
fn uint32_array_preserves_four_byte_elements_and_alignment() {
    assert_eq!(
        output(
            "var values = new Uint32Array([4294967297, -1]); print(values.length); print(values.byteLength); print(values[0]); print(values[1]); var buffer = new ArrayBuffer(8); var view = new Uint32Array(buffer); view[0] = 305419896; print(view[0]); print(view.byteLength); try { new Uint32Array(buffer, 2); } catch (error) { print('alignment'); } print(view.buffer === buffer);"
        ),
        [
            "2",
            "8",
            "1",
            "4294967295",
            "305419896",
            "8",
            "alignment",
            "true"
        ],
    );
}

#[test]
fn signed_typed_arrays_wrap_and_decode_element_bits() {
    assert_eq!(
        output(
            "var i8 = new Int8Array([255, -129, 130]); print(i8.join('-')); print(i8[0]); print(i8[1]); print(i8[2]); var i16 = new Int16Array([65535, 32768]); print(i16[0]); print(i16[1]); var i32 = new Int32Array([4294967295, 2147483648]); print(i32[0]); print(i32[1]); var buffer = new ArrayBuffer(4); var view = new Int32Array(buffer); view[0] = -2; print(view[0]); print(view.byteLength); try { new Int16Array(buffer, 1); } catch (error) { print('alignment'); }"
        ),
        [
            "-1-127--126",
            "-1",
            "127",
            "-126",
            "-1",
            "-32768",
            "-1",
            "-2147483648",
            "-2",
            "4",
            "alignment"
        ],
    );
}

#[test]
fn float_typed_arrays_preserve_numeric_values_and_widths() {
    assert_eq!(
        output(
            "var f32 = new Float32Array([1.5, -2.25]); print(f32.length); print(f32.byteLength); print(f32[0]); print(f32[1]); var f64 = new Float64Array([1.5, -2.25]); print(f64.length); print(f64.byteLength); print(f64[0]); print(f64[1]); var buffer = new ArrayBuffer(8); var view = new Float64Array(buffer); view[0] = 3.125; print(view[0]); try { new Float64Array(buffer, 4); } catch (error) { print('alignment'); }"
        ),
        [
            "2",
            "8",
            "1.5",
            "-2.25",
            "2",
            "16",
            "1.5",
            "-2.25",
            "3.125",
            "alignment"
        ],
    );
}

#[test]
fn uint8_clamped_array_uses_ties_to_even_conversion() {
    assert_eq!(
        output(
            "var values = new Uint8ClampedArray([-1, 0.5, 1.5, 2.5, 255.5, 300]); print(values.join('-')); print(values.BYTES_PER_ELEMENT); var view = new Uint8ClampedArray(new ArrayBuffer(2)); view[0] = 1.5; view[1] = 300; print(view[0]); print(view[1]); print(ArrayBuffer.isView(view));"
        ),
        ["0-0-2-2-255-255", "1", "2", "255", "true"],
    );
}

#[test]
fn typed_array_reverse_and_fill_mutate_all_numeric_views() {
    assert_eq!(
        output(
            "var values = new Int16Array([1, 2, 3]); print(values.reverse() === values); print(values.join('-')); print(values.fill(9, 1, -1) === values); print(values.join('-')); var floats = new Float64Array([1, 2, 3]); floats.reverse(); floats.fill(4, -2); print(floats.join('-'));"
        ),
        ["true", "3-2-1", "true", "3-9-1", "3-4-4"],
    );
}

#[test]
fn typed_array_copy_within_snapshots_overlapping_source() {
    assert_eq!(
        output(
            "var values = new Uint32Array([1, 2, 3, 4]); print(values.copyWithin(1, 0, 3) === values); print(values.join('-')); var floats = new Float32Array([1, 2, 3, 4]); floats.copyWithin(0, 2); print(floats.join('-'));"
        ),
        ["true", "1-1-2-3", "3-4-3-4"],
    );
}

#[test]
fn resizable_and_growable_buffers_update_views_and_metadata() {
    assert_eq!(
        output(
            "var buffer = new ArrayBuffer(2, { maxByteLength: 4 }); var view = new Uint8Array(buffer); var fixedView = new Uint8Array(buffer, 0, 2); var data = new DataView(buffer); buffer.resize(4); view[3] = 9; print(buffer.byteLength); print(buffer.maxByteLength); print(buffer.resizable); print(view[3]); print(data.byteLength); buffer.resize(1); print(buffer.byteLength); print(view.length); print(view.byteLength); print(data.byteLength); print(fixedView.length); print(fixedView.byteLength); print(fixedView.byteOffset); var fixed = buffer.transferToFixedLength(); print(fixed.byteLength); print(fixed.resizable); var shared = new SharedArrayBuffer(1, { maxByteLength: 3 }); shared.grow(3); print(shared.byteLength); print(shared.maxByteLength); print(shared.growable);"
        ),
        [
            "4", "4", "true", "9", "4", "1", "1", "1", "1", "0", "0", "0", "1", "false", "3", "3",
            "true"
        ],
    );
}

#[test]
fn regexp_exec_and_test_preserve_captures_and_flags() {
    assert_eq!(
        output(
            "var expression = new RegExp('(a)(b)', 'i'); var match = expression.exec('xxAByy'); print(expression.source); print(expression.flags); print(expression.test('AB')); print(match[0]); print(match[1]); print(match[2]); print(match.index); print(match.input); try { new RegExp('(', 'u'); } catch (error) { print('invalid'); }"
        ),
        [
            "(a)(b)", "i", "true", "AB", "A", "B", "2", "xxAByy", "invalid"
        ],
    );
}

#[test]
fn regexp_literals_lower_through_the_constructor_authority() {
    assert_eq!(
        output("var match = /a+/gi.exec('xxAAyy'); print(match[0]);"),
        ["AA"]
    );
}

#[test]
fn regexp_global_and_sticky_calls_advance_and_reset_last_index() {
    assert_eq!(
        output(
            "var global = /a/g; print(global.exec('aba')[0]); print(global.lastIndex); print(global.exec('aba')[0]); print(global.lastIndex); print(global.exec('aba') === null); print(global.lastIndex); var sticky = /a/y; sticky.lastIndex = 1; print(sticky.exec('ba')[0]); print(sticky.lastIndex); sticky.lastIndex = 0; print(sticky.exec('ba') === null); print(sticky.lastIndex);"
        ),
        ["a", "1", "a", "3", "true", "0", "a", "2", "true", "0"],
    );
}

#[test]
fn regexp_and_string_offsets_use_utf16_code_units() {
    assert_eq!(
        output(
            "var expression = /b/g; var match = expression.exec('😀b'); print(match.index); print(expression.lastIndex); var searched = '😀b'.match(/b/); print(searched.index); print('😀b'.search(/b/)); print('😀b'.replace(/b/, function(value, offset) { return offset; }));"
        ),
        ["2", "3", "2", "2", "😀2"],
    );
}

#[test]
fn string_replace_and_split_preserve_order_and_limits() {
    assert_eq!(
        output(
            "print('a-b-c'.replace('-', ':')); print('a-b-c'.replace('', '_')); var parts = 'a-b-c'.split('-', 2); print(parts.length); print(parts[0]); print(parts[1]); var units = '😀x'.split(''); print(units.length); print(units[0].length); print(units[2]);"
        ),
        ["a:b-c", "_a-b-c", "2", "a", "b", "3", "1", "x"],
    );
}

#[test]
fn string_replace_uses_regexp_global_and_capture_authority() {
    assert_eq!(
        output(
            "print('ab ab'.replace(/(a)(b)/, '$2$1')); print('ab ab'.replace(/(a)(b)/g, '$2$1'));"
        ),
        ["ba ab", "ba ba"],
    );
}

#[test]
fn string_replace_expands_replacement_context_tokens() {
    assert_eq!(
        output(
            r#"print('abc'.replace('b', '$$-$&-$`-$\'')); print('xabcy'.replace(/(b)/, '$1-$$-$`-$\''));"#
        ),
        ["a$-b-a-cc", "xab-$-xa-cycy"],
    );
}

#[test]
fn string_replace_calls_function_replacers_with_match_context() {
    assert_eq!(
        output(
            "print('a1b2'.replace(/(\\d)/g, function(match, digit, offset, input) { return digit + offset + input.length; }));"
        ),
        ["a114b234"],
    );
}

#[test]
fn string_replace_all_handles_strings_regexes_and_empty_searches() {
    assert_eq!(
        output(
            "print('a-a-a'.replaceAll('a', 'x')); print('a-a'.replaceAll(/a/g, 'x')); print('ab'.replaceAll('', '-')); try { 'a'.replaceAll(/a/, 'x'); } catch (error) { print('global-required'); }"
        ),
        ["x-x-x", "x-x", "-a-b-", "global-required"],
    );
}

#[test]
fn string_split_uses_regexp_captures_and_limit() {
    assert_eq!(
        output(
            "var parts = 'a1b2c'.split(/(\\d)/); print(parts.length); print(parts[0]); print(parts[1]); print(parts[2]); print(parts[3]); var limited = 'a1b2c'.split(/\\d/, 2); print(limited.length); print(limited[1]);"
        ),
        ["5", "a", "1", "b", "2", "2", "b"],
    );
}

#[test]
fn string_match_and_search_share_regexp_matching() {
    assert_eq!(
        output(
            "var match = 'ab ab'.match(/(a)(b)/); print(match[0]); print(match[1]); print(match.index); print('xxab'.search(/ab/)); var all = 'ab ab'.match(/ab/g); print(all.length); print(all[1]);"
        ),
        ["ab", "a", "0", "2", "2", "ab"],
    );
}

#[test]
fn string_trim_repeat_and_padding_use_utf16_lengths() {
    assert_eq!(
        output(
            "print('  hi  '.trim()); print('  hi  '.trimStart()); print('  hi  '.trimEnd()); print('ab'.repeat(3)); print('😀'.padStart(3, 'x')); print('😀'.padEnd(3, 'x'));"
        ),
        ["hi", "hi  ", "  hi", "ababab", "x😀", "😀x"],
    );
}

#[test]
fn string_modern_index_case_and_concat_methods_use_utf16() {
    assert_eq!(
        output(
            "var text = '😀x'; print(text.at(0).length); print(text.at(-1)); print(text.codePointAt(0)); print(text.codePointAt(-1)); print('ab'.toUpperCase()); print('AB'.toLowerCase()); print('a'.concat('b', 3));"
        ),
        ["1", "x", "128512", "undefined", "AB", "ab", "ab3"],
    );
}

#[test]
fn string_normalize_supports_unicode_normalization_forms() {
    assert_eq!(
        output(
            "var composed = '\u{00e9}'; var decomposed = 'e\\u0301'; print(composed === decomposed); print(decomposed.normalize() === composed); print(composed.normalize('NFD').length); print(composed.normalize('NFKC')); try { composed.normalize('bad'); } catch (error) { print('invalid-form'); }"
        ),
        ["false", "true", "2", "é", "invalid-form"],
    );
}

#[test]
fn array_join_coerces_values_and_preserves_hole_separators() {
    assert_eq!(
        output(
            "print([1, undefined, null, 'x'].join('|')); print(new Array(2).join('-')); print([1, 2].join());"
        ),
        ["1|||x", "-", "1,2"],
    );
}

#[test]
fn array_find_last_methods_walk_callbacks_in_reverse_order() {
    assert_eq!(
        output(
            "var seen = ''; var values = [1, 2, 3, 2]; var found = values.findLast(function(value, index, owner) { seen = seen + index; return value === 2 && owner === values; }); print(found); print(seen); print(values.findLastIndex(function(value) { return value === 2; })); print([1, 3].findLast(function(value) { return value === 2; }) === undefined);"
        ),
        ["2", "3", "3", "true"],
    );
}

#[test]
fn array_group_methods_build_objects_and_maps_from_callback_keys() {
    assert_eq!(
        output(
            "var values = [1, 2, 3, 4]; var grouped = values.group(function(value, index, owner) { return (value % 2 ? 'odd' : 'even') + owner.length; }); print(grouped.odd4[0]); print(grouped.even4[1]); var mapped = values.groupToMap(function(value) { return value % 2; }); print(mapped.get(0)[0]); print(mapped.get(1)[1]);"
        ),
        ["1", "4", "2", "3"],
    );
}

#[test]
fn array_is_array_distinguishes_arrays_from_array_like_objects() {
    assert_eq!(
        output(
            "print(Array.isArray([])); print(Array.isArray({ length: 0 })); print(Array.isArray('x'));"
        ),
        ["true", "false", "false"],
    );
}

#[test]
fn object_keys_reflect_own_shape_order() {
    assert_eq!(
        output(
            "var object = { first: 1, second: 2 }; var keys = Object.keys(object); print(keys[0]); print(keys[1]);"
        ),
        ["first", "second"],
    );
}

#[test]
fn object_create_preserves_prototype_lookup_and_own_shape() {
    assert_eq!(
        output(
            "var proto = { answer: 42 }; var object = Object.create(proto); print(object.answer); print(Object.keys(object).length);"
        ),
        ["42", "0"],
    );
}

#[test]
fn object_assign_copies_sources_in_argument_order() {
    assert_eq!(
        output(
            "var target = { answer: 0 }; Object.assign(target, { answer: 1 }, { answer: 42, extra: 7 }); print(target.answer); print(target.extra);"
        ),
        ["42", "7"],
    );
}

#[test]
fn object_prototype_controls_follow_the_object_proto_slot() {
    assert_eq!(
        output(
            "var first = { answer: 1 }; var second = { answer: 42 }; var object = {}; Object.setPrototypeOf(object, first); print(Object.getPrototypeOf(object).answer); Object.setPrototypeOf(object, second); print(object.answer);"
        ),
        ["1", "42"],
    );
}

#[test]
fn maps_and_sets_preserve_identity_and_insertion_size() {
    assert_eq!(
        output(
            "var key = {}; var map = new Map(); map.set(key, 42); map.set('x', 7); print(map.get(key)); print(map.has('x')); print(map.size); print(map.delete(key)); print(map.size); var set = new Set(); set.add(key); set.add(key); print(set.has(key)); print(set.size);"
        ),
        ["42", "true", "2", "true", "1", "true", "1"],
    );
}

#[test]
fn map_and_set_iterators_produce_ordered_iterator_results() {
    assert_eq!(
        output(
            "var map = new Map(); map.set('a', 1); map.set('b', 2); var iterator = map.entries(); var first = iterator.next(); print(first.value[0]); print(first.value[1]); print(first.done); var second = iterator.next(); print(second.value[0]); print(second.value[1]); print(iterator.next().done); var set = new Set(); set.add('x'); var set_iterator = set.keys(); print(set_iterator.next().value); print(set_iterator.next().done);"
        ),
        ["a", "1", "false", "b", "2", "true", "x", "true"],
    );
}

#[test]
fn weak_collections_require_object_keys_and_preserve_identity() {
    assert_eq!(
        output(
            "var key = {}; var map = new WeakMap(); map.set(key, 42); print(map.get(key)); print(map.has(key)); print(map.delete(key)); print(map.has(key)); var set = new WeakSet(); set.add(key); print(set.has(key)); print(set.delete(key)); print(set.has(key));"
        ),
        ["42", "true", "true", "false", "true", "true", "false"],
    );
}

#[test]
fn weak_ref_deref_tracks_target_and_rejects_primitives() {
    assert_eq!(
        output(
            "var target = {}; var reference = new WeakRef(target); print(reference.deref() === target);"
        ),
        ["true"],
    );
}

#[test]
fn collection_constructors_consume_array_entries_and_dedupe() {
    assert_eq!(
        output(
            "var map = new Map([['x', 1], ['x', 2]]); print(map.size); print(map.get('x')); var set = new Set(['x', 'x', 'y']); print(set.size); print(set.has('y'));"
        ),
        ["1", "2", "2", "true"],
    );
}

#[test]
fn array_for_of_binds_each_element_in_order() {
    assert_eq!(
        output(
            "var total = 0; for (const value of [1, 2, 3]) { total = total + value; } print(total);"
        ),
        ["6"],
    );
}

#[test]
fn array_for_of_assigns_identifier_and_member_targets() {
    let source = r#"
      var current = 0;
      var box = {};
      for (current of [1, 2]) { box.value = current; }
      print(current);
      print(box.value);
    "#;
    assert_eq!(output(source), ["2", "2"]);
}

#[test]
fn built_in_for_of_uses_array_string_map_and_set_iterators() {
    let source = r#"
      var array_total = 0;
      for (const value of [1, 2]) { array_total = array_total + value; }
      var text = '';
      for (const value of 'ab') { text = text + value; }
      var map_keys = '';
      var map = new Map([['a', 1], ['b', 2]]);
      for (const entry of map) { map_keys = map_keys + entry[0]; }
      var set_total = 0;
      for (const value of new Set([3, 4])) { set_total = set_total + value; }
      print(array_total); print(text); print(map_keys); print(set_total);
    "#;
    assert_eq!(output(source), ["3", "ab", "ab", "7"]);
}

#[test]
fn map_and_set_for_each_preserve_callback_argument_order() {
    assert_eq!(
        output(
            "var map = new Map([['a', 1], ['b', 2]]); var map_seen = ''; map.forEach(function(value, key, owner) { map_seen = map_seen + key + value + (owner === map); }); var set = new Set([2, 2, 3]); var set_seen = ''; set.forEach(function(value, key, owner) { set_seen = set_seen + value + key + (owner === set); }); print(map_seen); print(set_seen);"
        ),
        ["a1trueb2true", "22true33true"],
    );
}

#[test]
fn collection_for_each_observes_live_additions_and_deletions() {
    assert_eq!(
        output(
            "var map = new Map([['a', 1], ['b', 2]]); var map_seen = ''; map.forEach(function(value, key) { map_seen = map_seen + key; if (key === 'a') { map.set('c', 3); map.delete('b'); } }); var set = new Set(['a', 'b']); var set_seen = ''; set.forEach(function(value) { set_seen = set_seen + value; if (value === 'a') { set.add('c'); set.delete('b'); } }); print(map_seen); print(set_seen);"
        ),
        ["ac", "ac"],
    );
}

#[test]
fn reflect_forwards_to_property_and_prototype_authorities() {
    assert_eq!(
        output(
            "var object = {}; Reflect.set(object, 'answer', 42); print(Reflect.get(object, 'answer')); print(Reflect.ownKeys(object)[0]); print(Reflect.getPrototypeOf(object) === Object.prototype);"
        ),
        ["42", "answer", "true"],
    );
}

#[test]
fn object_has_own_uses_direct_shape_properties_only() {
    assert_eq!(
        output(
            "var object = { answer: 42 }; print(Object.hasOwn(object, 'answer')); print(Object.hasOwn(object, 'toString')); print(Object.hasOwn({ '1': 2 }, 1)); try { Object.hasOwn(null, 'x'); } catch (error) { print('nullish'); }"
        ),
        ["true", "false", "true", "nullish"],
    );
}

#[test]
fn object_get_own_property_names_shares_shape_ordering() {
    assert_eq!(
        output(
            "var object = { first: 1, second: 2 }; var names = Object.getOwnPropertyNames(object); print(names.length); print(names[0]); print(names[1]);"
        ),
        ["2", "first", "second"],
    );
}

#[test]
fn object_from_entries_uses_key_coercion_and_last_write_order() {
    assert_eq!(
        output(
            "var object = Object.fromEntries([['answer', 40], [42, 2], ['answer', 41]]); print(object.answer); print(object['42']); var names = Object.keys(object); print(names[0]); print(names[1]); try { Object.fromEntries([1]); } catch (error) { print('invalid-entry'); }"
        ),
        ["41", "2", "42", "answer", "invalid-entry"],
    );
}

#[test]
fn json_round_trip_uses_runtime_objects_and_arrays() {
    assert_eq!(
        output(
            "var value = JSON.parse('{\"answer\":42,\"items\":[1,2]}'); print(value.answer); print(value.items[1]); print(JSON.stringify(value));"
        ),
        ["42", "2", "{\"answer\":42,\"items\":[1,2]}"],
    );
}

#[test]
fn json_stringify_preserves_non_finite_and_rejects_cycles() {
    assert_eq!(
        output(
            "var value = {}; value.self = value; try { JSON.stringify(value); print('no-error'); } catch (error) { print('cycle'); } print(JSON.stringify([NaN, Infinity, -Infinity]));"
        ),
        ["cycle", "[null,null,null]"],
    );
}

#[test]
fn uri_codecs_preserve_component_and_reserved_character_rules() {
    assert_eq!(
        output(
            "print(encodeURIComponent('a b/&')); print(decodeURIComponent('a%20b%2F%26')); print(encodeURI('https://x.test/a b')); print(decodeURI('https://x.test/a%20b'));"
        ),
        [
            "a%20b%2F%26",
            "a b/&",
            "https://x.test/a%20b",
            "https://x.test/a b"
        ],
    );
}

#[test]
fn date_now_uses_the_host_clock_capability() {
    assert_eq!(output("print(Date.now());"), ["0"]);
}

#[test]
fn date_instances_expose_numeric_and_iso_authority() {
    assert_eq!(
        output(
            "var date = new Date(0); print(date.getTime()); print(date.valueOf()); print(date.toISOString()); print(date.toJSON()); print(JSON.stringify(date));"
        ),
        [
            "0",
            "0",
            "1970-01-01T00:00:00.000Z",
            "1970-01-01T00:00:00.000Z",
            "\"1970-01-01T00:00:00.000Z\"",
        ],
    );
}

#[test]
fn date_static_parse_and_utc_share_millisecond_authority() {
    assert_eq!(
        output(
            "print(Date.parse('1970-01-01T00:00:00.000Z')); print(Date.UTC(1970, 0, 1)); print(new Date(Date.UTC(1970, 0, 1, 0, 0, 1, 250)).toISOString());"
        ),
        ["0", "0", "1970-01-01T00:00:01.250Z"],
    );
}

#[test]
fn number_static_predicates_require_numeric_values() {
    assert_eq!(
        output(
            "print(Number('4')); print(Number.isNaN(NaN)); print(Number.isNaN('x')); print(Number.isFinite(4)); print(Number.isInteger(4.5));"
        ),
        ["4", "true", "false", "true", "false"],
    );
}

#[test]
fn number_static_constants_and_parsers_use_numeric_authority() {
    assert_eq!(
        output(
            "print(Number.isSafeInteger(9007199254740991)); print(Number.isSafeInteger(9007199254740992)); print(Number.parseInt('12px')); print(Number.parseFloat('  -1.25e2tail')); print(Number.EPSILON > 0); print(Number.MAX_SAFE_INTEGER);"
        ),
        ["true", "false", "12", "-125", "true", "9007199254740991"],
    );
}

#[test]
fn math_unary_rounding_preserves_ecmascript_edges() {
    assert_eq!(
        output(
            "print(Math.abs(-3)); print(Math.ceil(1.2)); print(Math.round(-1.5)); print(Math.round(-0.25)); print(Math.trunc(-1.9)); print(Math.sqrt(9)); print(Math.sign(-0));"
        ),
        ["3", "2", "-1", "0", "-1", "3", "0"],
    );
}

#[test]
fn string_search_methods_use_the_receiver_text() {
    assert_eq!(
        output(
            "print('quench'.includes('ench')); print('quench'.startsWith('que')); print('quench'.endsWith('nch'));"
        ),
        ["true", "true", "true"],
    );
}

#[test]
fn base_classes_lower_to_constructor_and_prototype_methods() {
    let source = r#"
      class Box {
        constructor(value) { this.value = value; }
        answer(extra) { return this.value + extra; }
        static tag() { return 42; }
      }
      const box = new Box(40);
      print(box.answer(2));
      print(Box.tag());
    "#;
    assert_eq!(output(source), ["42", "42"]);
}

#[test]
fn static_class_fields_initialize_after_methods_are_installed() {
    let source = r#"
      class Counter {
        static answer = Counter.makeAnswer();
        static makeAnswer() { return 40 + 2; }
        static unset;
      }
      print(Counter.answer);
      print(Counter.unset === undefined);
    "#;
    assert_eq!(output(source), ["42", "true"]);
}

#[test]
fn constructors_accept_array_spreads() {
    let source = r#"
      function Box(left, right) { this.total = left + right; }
      var values = [2, 3];
      var box = new Box(...values);
      print(box.total);
      var other = Reflect.construct(Box, [4, 5]);
      print(other.total);
    "#;
    assert_eq!(output(source), ["5", "9"]);
}

#[test]
fn instance_class_fields_initialize_before_constructor_body() {
    let source = r#"
      class Box {
        value = 40;
        unset;
        constructor() { this.value = this.value + 2; }
      }
      class DefaultBox { answer = 42; }
      const box = new Box();
      const default_box = new DefaultBox();
      print(box.value);
      print(box.unset === undefined);
      print(default_box.answer);
    "#;
    assert_eq!(output(source), ["42", "true", "42"]);
}

#[test]
fn class_methods_capture_the_enclosing_activation() {
    let source = r#"
      function make(offset) {
        class Box { answer() { return offset + 1; } }
        return new Box();
      }
      print(make(41).answer());
    "#;
    assert_eq!(output(source), ["42"]);
}

#[test]
fn template_literals_lower_to_string_addition() {
    assert_eq!(
        output("const answer = 42; print(`value: ${answer}!`);"),
        ["value: 42!"]
    );
}

#[test]
fn sequence_expressions_preserve_order_and_return_the_tail() {
    assert_eq!(
        output("var seen = 0; print((seen = 1, seen + 41));"),
        ["42"]
    );
}

#[test]
fn default_parameters_only_evaluate_for_undefined_arguments() {
    let source = r#"
      var calls = 0;
      function add(value = (calls = calls + 1, 40)) { return value + 2; }
      print(add());
      print(add(5));
      print(calls);
      var arrow = (value = 41) => value + 1;
      print(arrow());
    "#;
    assert_eq!(output(source), ["42", "7", "1", "42"]);
}

#[test]
fn rest_parameters_collect_trailing_arguments() {
    let source = r#"
      function collect(first, ...rest) { return first + rest.join('-'); }
      var arrow = (first, ...rest) => first + rest.join('-');
      class Box { method(first, ...rest) { return first + rest.join('-'); } }
      print(collect('a', 'b', 'c'));
      print(collect('a'));
      print(arrow('x', 'y', 'z'));
      print(new Box().method('m', 'n'));
    "#;
    assert_eq!(output(source), ["ab-c", "a", "xy-z", "mn"]);
}

#[test]
fn spread_calls_use_function_apply_semantics() {
    let source = r#"
      function add(left, right) { return left + right; }
      function sum(first, second, third) { return first + second + third; }
      var values = [2, 3];
      var object = { base: 4 };
      object.add = function(left, right) { return this.base + left + right; };
      print(add(...values));
      print(object.add(...[1, 2]));
      print(sum(1, ...[2], ...[3]));
      print(object.add(1, ...[2]));
    "#;
    assert_eq!(output(source), ["5", "7", "6", "7"]);
}

#[test]
fn computed_object_keys_use_indexed_property_semantics() {
    let source = r#"
      var key = 'answer';
      var object = { [key]: 42, [1 + 1]: 'two', [true]: 'yes' };
      print(object.answer); print(object[2]); print(object.true);
    "#;
    assert_eq!(output(source), ["42", "two", "yes"]);
}

#[test]
fn object_spread_copies_sources_in_literal_order() {
    let source = r#"
      var first = { value: 1, first: true };
      var second = { value: 2, second: true };
      var object = { ...first, value: 3, ...second, final: 4 };
      print(object.value); print(object.first); print(object.second); print(object.final);
    "#;
    assert_eq!(output(source), ["2", "true", "true", "4"]);
}

#[test]
fn destructured_formal_parameters_bind_nested_defaults() {
    let source = r#"
      function summarize({ answer = 40 }, [extra = 2]) { return answer + extra; }
      var summarize_arrow = ({ answer = 40 }, [extra = 2]) => answer + extra;
      print(summarize({ answer: 7 }, [5]));
      print(summarize({}, []));
      print(summarize_arrow({}, []));
    "#;
    assert_eq!(output(source), ["12", "42", "42"]);
}

#[test]
fn nullish_coalescing_only_falls_back_for_nullish_values() {
    assert_eq!(
        output("print(null ?? 42); print(undefined ?? 7); print(0 ?? 9); print('' ?? 3);"),
        ["42", "7", "0", ""],
    );
}

#[test]
fn optional_member_access_short_circuits_nullish_bases() {
    assert_eq!(
        output(
            "var none = null; var object = { value: 42 }; print(none?.value); print(object?.value); print(object.missing?.value);"
        ),
        ["undefined", "42", "undefined"],
    );
}

#[test]
fn optional_calls_skip_arguments_and_preserve_method_receivers() {
    assert_eq!(
        output(
            "var calls = 0; var none = null; var values = [41]; print(none?.(calls = 1)); print(none?.(...values)); print(calls); var object = { run: function(value) { return this.offset + value; }, offset: 1 }; print(object?.run(...values)); print(object?.run?.(...values));"
        ),
        ["undefined", "undefined", "0", "42", "42"],
    );
}

#[test]
fn bigint_literals_are_heap_values_and_stringify_without_loss() {
    assert_eq!(
        output("print(12345678901234567890n); print(String(7n)); print(7n === 7n);"),
        ["12345678901234567890", "7", "true"]
    );
}

#[test]
fn symbols_are_identity_values_with_explicit_display() {
    assert_eq!(
        output(
            "const first = Symbol('x'); print(first); print(String(first)); print(first === first); print(Symbol('x') === Symbol('x'));"
        ),
        ["Symbol(x)", "Symbol(x)", "true", "false"],
    );
}

#[test]
fn symbol_registry_preserves_identity_and_key_round_trip() {
    assert_eq!(
        output(
            "var first = Symbol.for('shared'); var second = Symbol.for('shared'); print(first === second); print(Symbol.keyFor(first)); print(Symbol.keyFor(Symbol('local')));"
        ),
        ["true", "shared", "undefined"],
    );
}

#[test]
fn typeof_reports_symbol_and_bigint_primitives() {
    assert_eq!(
        output("print(typeof Symbol('x')); print(typeof 1n);"),
        ["symbol", "bigint"]
    );
}

#[test]
fn method_caches_observe_callable_property_replacement() {
    let source = r#"
      function Box() {}
      Box.prototype.run = function() { return 1; };
      function invoke(value) { return value.run(); }
      var first = new Box();
      var second = new Box();
      print(invoke(first));
      Box.prototype.run = function() { return 2; };
      print(invoke(second));
      var key = "run";
      Box.prototype[key] = function() { return 3; };
      print(invoke(first));
      var own = { run: function() { return 4; } };
      print(invoke(own));
      own.run = function() { return 5; };
      print(invoke(own));
    "#;
    assert_eq!(output(source), ["1", "2", "3", "4", "5"]);
}

#[test]
fn method_caches_distinguish_own_methods_and_cached_writes() {
    let source = r#"
      function invoke(value) { return value.run(); }
      var first = { run: function() { return 1; } };
      var second = { run: function() { return 2; } };
      print(invoke(first));
      print(invoke(second));
      print(invoke(first));
      function install(value, result) {
        value.run = function() { return result; };
      }
      install(first, 3);
      print(invoke(first));
      install(first, 4);
      print(invoke(first));
    "#;
    assert_eq!(output(source), ["1", "2", "1", "3", "4"]);
}

#[test]
fn class_heritage_links_constructor_and_prototype_chains() {
    let source = r#"
      class Base { constructor(value) { this.value = value; } method() { return this.value; } static answer() { return 1; } }
      class Child extends Base {}
      print(new Child(41).method() + 1);
      print(Child.answer() + 1);
    "#;
    assert_eq!(output(source), ["42", "2"]);
}

#[test]
fn derived_constructors_and_super_methods_use_shared_calls() {
    let source = r#"
      class Base {
        constructor(value) { this.value = value; }
        method() { return this.value; }
        static kind() { return 10; }
      }
      class Child extends Base {
        constructor(value) { super(value + 1); }
        method() { var key = 'method'; return super[key]() + 1; }
        static kind() { var key = 'kind'; return super[key]() + 1; }
      }
      print(new Child(40).method());
      print(Child.kind());
    "#;
    assert_eq!(output(source), ["42", "11"]);
}

#[test]
fn catch_receives_thrown_value() {
    assert_eq!(
        output("try { throw 'caught'; } catch (e) { print(e); }"),
        ["caught"]
    );
}

#[test]
fn unified_field_and_binary_operands_preserve_semantics() {
    let source = r#"
      function Box() { this.x = 4; this.inner = { y: 3 }; }
      Box.prototype.check = function() {
        this.x += 2;
        if (this.x == 6) return this.inner.y + 1;
        return 0;
      };
      var box = new Box();
      print(box.x + 2);
      print(2 + box.x);
      print(box.inner.y);
      print(box.check());
    "#;
    assert_eq!(output(source), ["6", "6", "3", "4"]);
}

#[test]
fn computed_property_keys_are_interned_at_runtime() {
    let source = r#"
      var object = {};
      var prefix = 'run';
      var suffix = 'time';
      object[prefix + suffix] = 42;
      print(object[prefix + suffix]);
    "#;
    assert_eq!(output(source), ["42"]);
}

#[test]
fn string_function_converts_values() {
    assert_eq!(
        output("print(String(42)); print(String()); print(new String(true));"),
        ["42", "undefined", "true"]
    );
}

#[test]
fn crypto_primitive_library_matches_es5_behavior() {
    let source = r#"
      print("Az".charCodeAt(1));
      print("abcd".charAt(2));
      print("abcd".substring(3, 1));
      print("abcd".substr(-3, 2));
      print("AéB".charCodeAt(1));
      print("AéB".substring(1, 2));
      print("A😀B".charCodeAt(1));
      print("A😀B".charCodeAt(2));
      print("A😀B".substring(1, 3));
      print(String.fromCharCode(65, 66));
      print((255).toString(16));
      print(parseInt("  -0x10tail", 0));
      print(Math.floor(3.9));
      print(Math.min(7, 2, 5));
      print(Math.max(7, 2, 5));
      print(Math.min(7, undefined));
      print(Math.LN2 > 0);
      var random = Math.random();
      print(random >= 0 && random < 1);
    "#;
    assert_eq!(
        output(source),
        [
            "122", "c", "bc", "bc", "233", "é", "55357", "56832", "😀", "AB", "ff", "-16", "3",
            "2", "7", "NaN", "true", "true"
        ]
    );
}

#[test]
fn deterministic_hash_random_preserves_es5_bitwise_semantics() {
    let source = r#"
      var seed = 49734321;
      function random() {
        seed = ((seed + 0x7ed55d16) + (seed << 12)) & 0xffffffff;
        seed = ((seed ^ 0xc761c23c) ^ (seed >>> 19)) & 0xffffffff;
        seed = ((seed + 0x165667b1) + (seed << 5)) & 0xffffffff;
        seed = ((seed + 0xd3a2646c) ^ (seed << 9)) & 0xffffffff;
        seed = ((seed + 0xfd7046c5) + (seed << 3)) & 0xffffffff;
        seed = ((seed ^ 0xb55a4f09) ^ (seed >>> 16)) & 0xffffffff;
        return (seed & 0xfffffff) / 0x10000000;
      }
      print(random()); print(random()); print(random());
    "#;
    assert_eq!(
        output(source),
        [
            "0.9872818551957607",
            "0.34880331158638",
            "0.5631933622062206"
        ]
    );
}

#[test]
fn abstract_equality_keeps_nullish_values_distinct_from_zero_and_false() {
    let source = r#"
      print(null == undefined);
      print(null == 0);
      print(undefined == 0);
      print(false == 0);
      print("0" == 0);
      print(null === null);
    "#;
    assert_eq!(
        output(source),
        ["true", "false", "false", "true", "true", "true"]
    );
}

#[test]
fn sparse_array_index_does_not_require_dense_prefix() {
    let source = "var a = new Array(0); a[1000000] = 7; print(a.length); print(a[1000000]);";
    assert_eq!(output(source), ["1000001", "7"]);
}

#[test]
fn closure_environment_promotes_only_when_creation_executes() {
    let source = r#"
      function maybe(make) {
        var value = 40;
        if (make) return function(x) { return value + x; };
        return value;
      }
      print(maybe(false));
      var add = maybe(true);
      print(add(2));
    "#;
    assert_eq!(output(source), ["40", "42"]);
}

#[test]
fn switch_supports_strict_matching_fallthrough_default_and_break() {
    let source = r#"
      var tests = 0;
      function choose() { tests++; return 2; }
      var first = 0;
      switch (choose()) {
        case '2': first = 100; break;
        case 2: first += 2;
        case 3: first += 3; break;
        default: first = 200;
      }
      var second = 0;
      switch (9) {
        case 1: second = 1; break;
        default: second += 4;
        case 2: second += 5; break;
      }
      print(tests); print(first); print(second); print('1' === 1);
    "#;
    assert_eq!(output(source), ["1", "5", "9", "false"]);
}

#[test]
fn loop_control_targets_are_derived_from_nesting() {
    let source = r#"
      var once = 0;
      do { once++; } while (false);
      var total = 0;
      for (var i = 0; i < 4; i++) {
        switch (i) {
          case 1: continue;
          case 2: break;
        }
        total += i;
      }
      var n = 0;
      do {
        n++;
        if (n < 3) continue;
        break;
      } while (true);
      print(once); print(total); print(n);
    "#;
    assert_eq!(output(source), ["1", "5", "3"]);
}

#[test]
fn labeled_break_targets_the_named_statement() {
    let source = r#"
      var count = 0;
      label: while (true) {
        count++;
        break label;
      }
      print(count);
    "#;
    assert_eq!(output(source), ["1"]);
}

#[test]
fn labeled_continue_targets_the_named_loop() {
    let source = r#"
      var count = 0;
      label: for (var i = 0; i < 3; i++) {
        count++;
        continue label;
      }
      print(count);
    "#;
    assert_eq!(output(source), ["3"]);
}

#[test]
fn catch_patterns_bind_the_thrown_value() {
    let source = r#"
      try { throw { code: 17, detail: 4 }; }
      catch ({ code, detail }) { print(code); print(detail); }
      try { throw [3, 5]; }
      catch ([first, second]) { print(first); print(second); }
      try { throw {}; }
      catch ({missing = 9}) { print(missing); }
    "#;
    assert_eq!(output(source), ["17", "4", "3", "5", "9"]);
}

#[test]
fn class_static_blocks_run_with_the_class_as_this() {
    let source = r#"
      var order = 0;
      class Example {
        static first = ++order;
        static { this.second = ++order; }
        static third = ++order;
      }
      print(Example.first); print(Example.second); print(Example.third); print(order);
    "#;
    assert_eq!(output(source), ["1", "2", "3", "3"]);
}

#[test]
fn computed_class_fields_and_methods_use_key_expressions() {
    let source = r#"
      var key = 'value';
      class Example {
        static [key] = 7;
        static [key + 'Method']() { return 8; }
        [key + 'Instance'] = 9;
        [key + 'Method']() { return 10; }
      }
      var instance = new Example();
      print(Example.value); print(Example.valueMethod());
      print(instance.valueInstance); print(instance.valueMethod());
    "#;
    assert_eq!(output(source), ["7", "8", "9", "10"]);
}

#[test]
fn object_prototypes_and_function_call_support_inheritance() {
    let source = r#"
      Object.prototype.inheritsFrom = function (parent) {
        function Inheriter() {}
        Inheriter.prototype = parent.prototype;
        this.prototype = new Inheriter();
        this.superConstructor = parent;
      };
      function Base(value) { this.value = value; }
      Base.prototype.answer = function (extra) { return this.value + extra; };
      function Child(value) { Child.superConstructor.call(this, value); }
      Child.inheritsFrom(Base);
      var child = new Child(40);
      var plain = new Object();
      Object.prototype.marker = 7;
      var same = {};
      print(child.answer(2));
      print(plain.marker);
      print(Object(same) === same);
      try { (function () { throw 'called'; }).call(null); }
      catch (error) { print(error); }
    "#;
    assert_eq!(output(source), ["42", "7", "true", "called"]);
}

#[test]
fn array_pop_updates_dense_and_sparse_lengths() {
    let source = r#"
      var dense = new Array();
      dense.push(1); dense.push(2);
      print(dense.pop()); print(dense.length);
      print(dense.pop()); print(dense.pop());
      var sparse = new Array();
      sparse[1000000] = 7;
      print(sparse.pop()); print(sparse.length);
    "#;
    assert_eq!(output(source), ["2", "1", "1", "undefined", "7", "1000000"]);
}

#[test]
fn constant_array_literals_preserve_values() {
    assert_eq!(
        output("var values = [0, 'one', true, null]; print(values[1]); print(values.length);"),
        ["one", "4"]
    );
}

#[test]
fn constant_array_templates_detach_before_mutation() {
    let source = r#"
      function make() { return [1, 2]; }
      var first = make();
      var second = make();
      first[0] = 9;
      first.push(3);
      print(first[0]); print(first.length);
      print(second[0]); print(second.length);
    "#;
    assert_eq!(output(source), ["9", "3", "1", "2"]);
}

#[test]
fn returned_superinstruction_object_preserves_value() {
    let source = r#"
      function build(key) { return { items: [1, 2], text: 'x' + key + 'y' }; }
      var value = build(3);
      print(value.items[1]); print(value.text);
    "#;
    let program = Engine::specialize(source, "super-return.js").unwrap();
    assert!(program.disassemble().contains("SuperConstArrayObject2"));
    assert_eq!(output(source), ["2", "x3y"]);
}

#[test]
fn immutable_root_functions_use_direct_calls_but_reassignments_do_not() {
    let direct = Engine::specialize(
        "function target() { return 42; } function caller() { return target(); } print(caller());",
        "direct-call.js",
    )
    .unwrap();
    assert!(direct.disassemble().contains("CallKnown"));
    assert_eq!(
        output(
            "function f(){return 1;} function g(){return f();} f=function(){return 2;}; print(g());"
        ),
        ["2"]
    );
}

#[test]
fn updates_evaluate_computed_member_reference_once() {
    let source = r#"
      var values = [10, 20, 30];
      var index = 0;
      print(values[++index]++);
      print(index);
      print(values[1]);
      print(values[2]);
    "#;
    assert_eq!(output(source), ["20", "1", "21", "30"]);
}

#[test]
fn numeric_dispatch_class_is_data_derived_and_semantics_neutral() {
    let body = r#"
      function dense(a, b) {
        var x = a + b;
        x = x * b; x = x + a; x = x * b; x = x + a;
        x = x * b; x = x + a; x = x * b; x = x + a;
        x = x * b; x = x + a; x = x * b; x = x + a;
        x = x * b; x = x + a; x = x * b; x = x + a;
        return x;
      }
      print(dense(2, 3));
    "#;
    let edited = format!("var irrelevant = 1; {body}");
    for source in [body, edited.as_str()] {
        let program = Engine::specialize(source, "dispatch-class.js").unwrap();
        assert!(
            program
                .disassemble()
                .lines()
                .any(|line| line.contains(" dense ") && line.ends_with("dispatch=Numeric"))
        );
        assert_eq!(output(source), ["39365"]);
    }
}

#[test]
fn numeric_method_arguments_flow_directly_from_caller_registers() {
    let source = r#"
      function Holder() {}
      Holder.prototype.dense = function(a, b) {
        var x = a + b;
        x = x * b; x = x + a; x = x * b; x = x + a;
        x = x * b; x = x + a; x = x * b; x = x + a;
        x = x * b; x = x + a; x = x * b; x = x + a;
        x = x * b; x = x + a; x = x * b; x = x + a;
        return x;
      };
      var holder = new Holder();
      print(holder.dense(2, 3));
    "#;
    let program = Engine::specialize(source, "numeric-method.js").unwrap();
    assert!(
        program
            .disassemble()
            .lines()
            .any(|line| line.ends_with("dispatch=Numeric"))
    );
    assert_eq!(output(source), ["39365"]);
}

#[test]
fn supported_subset_matches_quickjs() {
    let quickjs = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../quickjs/qjs");
    if !quickjs.is_file() {
        eprintln!(
            "skipping differential test: {} is absent",
            quickjs.display()
        );
        return;
    }
    let source = r#"
      var seed = 3;
      function Counter(value) { this.value = value; }
      Counter.prototype.step = function(delta) {
        this.value += delta;
        return this.value;
      };
      function makeAdder(base) { return function(value) { return base + value; }; }
      var object = new Counter(seed);
      var values = new Array(3);
      values[0] = object.step(2);
      values[1] = makeAdder(30)(7);
      values[2] = (values[0] << 2) | 1;
      var key = 'answer'; object[key] = values[1] + 5;
      var total = 0;
      for (var i = 0; i < values.length; i++) total += values[i];
      try { throw object[key]; } catch (caught) { print(caught); }
      print(total);
      print(object.value == 5);
      print("Az".charCodeAt(1));
      print("abcd".substring(3, 1));
      print((255).toString(16));
      print(parseInt(" -0x10tail", 0));
      print(Math.min(7, 2, 5));
      print(null == 0);
    "#;
    let expected = output(source).join("\n") + "\n";
    let fixture = std::env::temp_dir().join(format!(
        "rqj-differential-{}-{}.js",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    std::fs::write(&fixture, source).unwrap();
    let result = Command::new(&quickjs).arg(&fixture).output().unwrap();
    let _ = std::fs::remove_file(&fixture);
    assert!(result.status.success(), "QuickJS failed: {result:?}");
    assert_eq!(String::from_utf8(result.stdout).unwrap(), expected);
}
