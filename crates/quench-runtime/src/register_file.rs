use std::rc::Rc;

use crate::{
    tagged_value::{DecodedValue, TaggedValue},
    value::Value,
};

#[repr(align(8))]
struct AlignedValue(Value);

fn encode(value: Value) -> TaggedValue {
    match value {
        Value::Object(value) => TaggedValue::object_ptr(Rc::into_raw(value) as usize),
        Value::Array(value) => TaggedValue::array_ptr(Rc::into_raw(value) as usize),
        Value::Function(value) => TaggedValue::function_ptr(Rc::into_raw(value) as usize),
        value => value.to_tagged().or_else(|| {
            let pointer = Rc::into_raw(Rc::new(AlignedValue(value))) as usize;
            TaggedValue::heap_ptr(pointer)
        }),
    }
    .expect("aligned execute payload pointer exceeds tag layout")
}

fn retain(word: TaggedValue) {
    // SAFETY: every pointer originates in `encode`; every copied word retains
    // once, and every discarded word releases once using the same payload type.
    unsafe {
        match word.decode() {
            DecodedValue::ObjectPtr(pointer) => {
                Rc::increment_strong_count(pointer as *const crate::value::ObjectData)
            }
            DecodedValue::ArrayPtr(pointer) => {
                Rc::increment_strong_count(pointer as *const crate::value::ArrayData)
            }
            DecodedValue::FunctionPtr(pointer) => {
                Rc::increment_strong_count(pointer as *const crate::value::FunctionValue)
            }
            DecodedValue::HeapPtr(pointer) => {
                Rc::increment_strong_count(pointer as *const AlignedValue)
            }
            _ => {}
        }
    }
}

fn release(word: TaggedValue) {
    // SAFETY: each arm consumes exactly the typed strong reference owned by
    // `word`; the tag and pointer are created together in `encode`.
    unsafe {
        match word.decode() {
            DecodedValue::ObjectPtr(pointer) => {
                drop(Rc::from_raw(pointer as *const crate::value::ObjectData))
            }
            DecodedValue::ArrayPtr(pointer) => {
                drop(Rc::from_raw(pointer as *const crate::value::ArrayData))
            }
            DecodedValue::FunctionPtr(pointer) => {
                drop(Rc::from_raw(pointer as *const crate::value::FunctionValue))
            }
            DecodedValue::HeapPtr(pointer) => drop(Rc::from_raw(pointer as *const AlignedValue)),
            _ => {}
        }
    }
}

/// Canonical active-frame register storage.
///
/// Registers are one copyable word. Heap pointers fit losslessly in the word's
/// 45-bit payload after removing three known-zero alignment bits. Common JS
/// heap kinds carry their Rust `Rc` directly and need no wrapper allocation.
#[derive(Debug)]
pub struct RegisterFile {
    words: Vec<TaggedValue>,
}

impl RegisterFile {
    pub const fn new() -> Self {
        Self { words: Vec::new() }
    }

    pub fn with_undefined(len: usize) -> Self {
        Self {
            words: vec![TaggedValue::undefined(); len],
        }
    }

    pub fn from_values(values: Vec<Value>) -> Self {
        let mut registers = Self::new();
        registers.reserve(values.len());
        registers.words.extend(values.into_iter().map(encode));
        registers
    }

    pub fn to_values(&self) -> Vec<Value> {
        (0..self.len())
            .filter_map(|index| self.read(index))
            .collect()
    }

    pub fn into_values(self) -> Vec<Value> {
        self.to_values()
    }

    pub fn len(&self) -> usize {
        self.words.len()
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.words.capacity()
    }

    pub fn reserve(&mut self, additional: usize) {
        self.words.reserve(additional);
    }

    pub fn read(&self, index: usize) -> Option<Value> {
        crate::execution_trace::event(crate::execution_trace::Event::ValueDecode);
        let word = *self.words.get(index)?;
        match word.decode() {
            DecodedValue::ObjectPtr(pointer) => {
                retain(word);
                Some(Value::Object(unsafe {
                    Rc::from_raw(pointer as *const crate::value::ObjectData)
                }))
            }
            DecodedValue::ArrayPtr(pointer) => {
                retain(word);
                Some(Value::Array(unsafe {
                    Rc::from_raw(pointer as *const crate::value::ArrayData)
                }))
            }
            DecodedValue::FunctionPtr(pointer) => {
                retain(word);
                Some(Value::Function(unsafe {
                    Rc::from_raw(pointer as *const crate::value::FunctionValue)
                }))
            }
            DecodedValue::HeapPtr(pointer) => {
                // SAFETY: the register word owns a strong reference, and the
                // borrow ends before any caller can mutate the register file.
                Some(unsafe { &*(pointer as *const AlignedValue) }.0.clone())
            }
            DecodedValue::Number(value) => Some(Value::Number(value)),
            DecodedValue::I31(value) => Some(Value::Number(f64::from(value))),
            DecodedValue::Bool(value) => Some(Value::Boolean(value)),
            DecodedValue::Null => Some(Value::Null),
            DecodedValue::Undefined => Some(Value::Undefined),
            DecodedValue::HeapRef(_) => None,
        }
    }

    /// Borrow an array directly from its execute word. The borrow is tied to
    /// the register file, whose owned strong reference keeps the allocation
    /// alive. No `Value` or temporary `Rc` is constructed.
    #[inline(always)]
    pub(crate) fn read_array(&self, index: usize) -> Option<&crate::value::ArrayData> {
        let DecodedValue::ArrayPtr(pointer) = self.words.get(index)?.decode() else {
            return None;
        };
        // SAFETY: `encode` creates this tag only from `Rc<ArrayData>`, and the
        // register word owns that strong reference for the returned lifetime.
        Some(unsafe { &*(pointer as *const crate::value::ArrayData) })
    }

    /// Read the exact non-negative integer domain accepted by packed array
    /// indexing without applying JavaScript property-key coercion.
    #[inline(always)]
    pub(crate) fn read_array_index(&self, index: usize) -> Option<usize> {
        let number = self.read_number(index)?;
        (number >= 0.0 && number <= u32::MAX as f64 && number.fract() == 0.0)
            .then(|| number as usize)
    }

    #[inline(always)]
    pub fn read_number(&self, index: usize) -> Option<f64> {
        match self.words.get(index)?.decode() {
            DecodedValue::Number(value) => Some(value),
            DecodedValue::I31(value) => Some(f64::from(value)),
            _ => None,
        }
    }

    /// Decide ToBoolean directly when the execute word contains the complete
    /// semantic fact. Heap-backed primitives retain the canonical slow path.
    #[inline(always)]
    pub(crate) fn word_truthiness(&self, index: usize) -> Option<bool> {
        let result = match self.words.get(index)?.decode() {
            DecodedValue::Number(value) => Some(value != 0.0 && !value.is_nan()),
            DecodedValue::I31(value) => Some(value != 0),
            DecodedValue::Bool(value) => Some(value),
            DecodedValue::Null | DecodedValue::Undefined => Some(false),
            DecodedValue::ObjectPtr(_)
            | DecodedValue::ArrayPtr(_)
            | DecodedValue::FunctionPtr(_) => Some(true),
            DecodedValue::HeapPtr(_) | DecodedValue::HeapRef(_) => None,
        };
        result
    }

    pub fn get(&self, index: usize) -> Option<Value> {
        self.read(index)
    }

    pub fn resize(&mut self, new_len: usize, value: Value) {
        while self.len() < new_len {
            self.push(value.clone());
        }
        while self.len() > new_len {
            release(self.words.pop().expect("length checked"));
        }
    }

    pub fn write(&mut self, index: usize, value: Value) {
        self.resize_undefined(index + 1);
        release(std::mem::replace(&mut self.words[index], encode(value)));
    }

    #[inline(always)]
    pub fn write_number(&mut self, index: usize, value: f64) {
        self.resize_undefined(index + 1);
        release(std::mem::replace(
            &mut self.words[index],
            TaggedValue::number(value),
        ));
    }

    #[inline(always)]
    pub(crate) fn write_boolean(&mut self, index: usize, value: bool) {
        self.resize_undefined(index + 1);
        release(std::mem::replace(
            &mut self.words[index],
            TaggedValue::bool(value),
        ));
    }

    /// Decide abstract equality directly when both words carry sufficient
    /// semantic facts. Heap-backed coercions remain on the canonical slow path.
    #[inline(always)]
    pub(crate) fn abstract_equal_words(&self, lhs: usize, rhs: usize) -> Option<bool> {
        use DecodedValue::*;
        let left = self.words.get(lhs)?.decode();
        let right = self.words.get(rhs)?.decode();
        match (left, right) {
            (Number(left), Number(right)) => Some(left == right),
            (Number(left), I31(right)) | (I31(right), Number(left)) => {
                Some(left == f64::from(right))
            }
            (I31(left), I31(right)) => Some(left == right),
            (Bool(left), Bool(right)) => Some(left == right),
            (Number(number), Bool(boolean)) | (Bool(boolean), Number(number)) => {
                Some(number == f64::from(boolean))
            }
            (I31(number), Bool(boolean)) | (Bool(boolean), I31(number)) => {
                Some(number == i32::from(boolean))
            }
            (Null, Null | Undefined) | (Undefined, Null | Undefined) => Some(true),
            (Null | Undefined, Number(_) | I31(_) | Bool(_))
            | (Number(_) | I31(_) | Bool(_), Null | Undefined) => Some(false),
            (ObjectPtr(left), ObjectPtr(right))
            | (ArrayPtr(left), ArrayPtr(right))
            | (FunctionPtr(left), FunctionPtr(right)) => Some(left == right),
            (
                ObjectPtr(_) | ArrayPtr(_) | FunctionPtr(_),
                ObjectPtr(_) | ArrayPtr(_) | FunctionPtr(_),
            )
            | (ObjectPtr(_) | ArrayPtr(_) | FunctionPtr(_), Null | Undefined)
            | (Null | Undefined, ObjectPtr(_) | ArrayPtr(_) | FunctionPtr(_)) => Some(false),
            _ => None,
        }
    }

    pub fn copy(&mut self, destination: usize, source: usize) -> bool {
        crate::execution_trace::event(crate::execution_trace::Event::RegisterWordCopy);
        let Some(word) = self.words.get(source).copied() else {
            return false;
        };
        self.resize_undefined(destination + 1);
        retain(word);
        release(std::mem::replace(&mut self.words[destination], word));
        true
    }

    /// Copy one canonical execute word between storage owners. This is the
    /// lexical-slot fast path: heap values retain once, immediates are a u64
    /// copy, and neither side materializes the 32-byte semantic `Value`.
    pub(crate) fn copy_from(
        &mut self,
        destination: usize,
        source: &Self,
        source_index: usize,
    ) -> bool {
        let Some(word) = source.words.get(source_index).copied() else {
            return false;
        };
        self.resize_undefined(destination + 1);
        retain(word);
        release(std::mem::replace(&mut self.words[destination], word));
        true
    }

    pub fn push(&mut self, value: Value) {
        self.words.push(encode(value));
    }

    pub fn resize_undefined(&mut self, len: usize) {
        if self.words.len() < len {
            self.words.resize(len, TaggedValue::undefined());
        }
    }

    pub fn clear(&mut self) {
        for word in std::mem::take(&mut self.words) {
            release(word);
        }
    }

    #[cfg(test)]
    fn word(&self, index: usize) -> Option<TaggedValue> {
        self.words.get(index).copied()
    }
}

#[cfg(test)]
mod truthiness_tests {
    use super::RegisterFile;
    use crate::value::{ObjectData, Value};
    use std::rc::Rc;

    #[test]
    fn execute_words_decide_javascript_truthiness_without_value_decode() {
        let registers = RegisterFile::from_values(vec![
            Value::Number(0.0),
            Value::Number(-0.0),
            Value::Number(f64::NAN),
            Value::Number(1.0),
            Value::Boolean(false),
            Value::Boolean(true),
            Value::Null,
            Value::Undefined,
            Value::Object(Rc::new(ObjectData::new(Vec::new()))),
        ]);
        let expected = [false, false, false, true, false, true, false, false, true];
        for (index, expected) in expected.into_iter().enumerate() {
            assert_eq!(registers.word_truthiness(index), Some(expected));
        }
    }
}

impl Default for RegisterFile {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for RegisterFile {
    fn clone(&self) -> Self {
        for word in &self.words {
            retain(*word);
        }
        Self {
            words: self.words.clone(),
        }
    }
}

impl Drop for RegisterFile {
    fn drop(&mut self) {
        self.clear();
    }
}

impl PartialEq for RegisterFile {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len()
            && (0..self.len()).all(|index| self.read(index) == other.read(index))
    }
}

impl PartialEq<Vec<Value>> for RegisterFile {
    fn eq(&self, other: &Vec<Value>) -> bool {
        self.len() == other.len()
            && other
                .iter()
                .enumerate()
                .all(|(index, value)| self.read(index).as_ref() == Some(value))
    }
}

#[cfg(test)]
mod tests {
    use super::RegisterFile;
    use crate::{tagged_value::DecodedValue, value::Value};

    #[test]
    fn move_copies_one_word_without_copying_string_payload() {
        let mut registers = RegisterFile::from_values(vec![Value::String("payload".into())]);
        assert!(registers.copy(1, 0));
        assert_eq!(registers.word(0), registers.word(1));
        assert!(matches!(
            registers.word(0).unwrap().decode(),
            DecodedValue::HeapPtr(_)
        ));
        assert_eq!(registers.read(1), Some(Value::String("payload".into())));
    }

    #[test]
    fn lexical_storage_copies_one_word_between_owners() {
        let source = RegisterFile::from_values(vec![Value::Number(42.0)]);
        let mut destination = RegisterFile::with_undefined(1);
        assert!(destination.copy_from(0, &source, 0));
        assert_eq!(destination.read_number(0), Some(42.0));
        assert_eq!(source.read_number(0), Some(42.0));
    }

    #[test]
    fn writing_a_lower_register_never_truncates_higher_registers() {
        let mut registers =
            RegisterFile::from_values(vec![Value::Undefined, Value::String("kept".into())]);
        registers.write(0, Value::Number(1.0));
        assert_eq!(registers.len(), 2);
        assert_eq!(registers.read(1), Some(Value::String("kept".into())));
    }

    #[test]
    fn abstract_equality_uses_immediate_word_facts() {
        let registers = RegisterFile::from_values(vec![
            Value::Number(1.0),
            Value::Boolean(true),
            Value::Null,
            Value::Undefined,
        ]);
        assert_eq!(registers.abstract_equal_words(0, 1), Some(true));
        assert_eq!(registers.abstract_equal_words(2, 3), Some(true));
        assert_eq!(registers.abstract_equal_words(0, 2), Some(false));
    }

    #[test]
    fn abstract_equality_defers_heap_coercion() {
        let registers =
            RegisterFile::from_values(vec![Value::String("1".into()), Value::Number(1.0)]);
        assert_eq!(registers.abstract_equal_words(0, 1), None);
    }
}
