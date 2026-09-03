//! QuickJS `JSRuntime` / `JSContext`: heap, atoms, shapes, GC list, realm.

use super::atom::AtomTable;
use super::jsvalue::JsValue;
use super::shape::{Shape, ShapeId, ShapeTable};

/// Heap object header. RC + cycle list, like `JSGCObjectHeader`.
#[derive(Clone, Debug, PartialEq)]
pub struct GcHeader {
    pub ref_count: i32,
    pub in_cycle_list: bool,
}

/// Ordinary object: shape + dense slots. Fast arrays keep a dense tail.
#[derive(Clone, Debug, PartialEq)]
pub struct Object {
    pub header: GcHeader,
    pub shape: ShapeId,
    pub slots: Vec<JsValue>,
    pub fast_array: Option<Vec<JsValue>>,
}

/// QuickJS string: 8-bit if Latin-1, else 16-bit. Random char access is O(1).
#[derive(Clone, Debug, PartialEq)]
pub enum JsString {
    Bytes(Box<[u8]>),
    Units(Box<[u16]>),
}

/// One object heap. Contexts share it; they cannot exchange values across runtimes.
#[derive(Clone, Debug, Default)]
pub struct Runtime {
    pub atoms: AtomTable,
    pub shapes: ShapeTable,
    pub objects: Vec<Object>,
    pub strings: Vec<JsString>,
    pub malloc_size: usize,
    pub gc_threshold: usize,
}

impl Runtime {
    pub fn new() -> Self {
        Self {
            gc_threshold: 256 * 1024,
            ..Self::default()
        }
    }

    pub fn new_object(&mut self, proto: JsValue) -> JsValue {
        let shape = self.shapes.intern(Shape::empty(proto));
        let id = self.objects.len() as u32;
        self.objects.push(Object {
            header: GcHeader {
                ref_count: 1,
                in_cycle_list: false,
            },
            shape,
            slots: Vec::new(),
            fast_array: None,
        });
        JsValue::ptr(super::jsvalue::Tag::Object, id)
    }

    pub fn new_string(&mut self, s: &str) -> JsValue {
        // QuickJS: 8-bit if every code point fits in Latin-1, else UTF-16.
        let latin1 = s.chars().all(|c| (c as u32) <= 0xff);
        let body = if latin1 {
            JsString::Bytes(
                s.chars()
                    .map(|c| c as u8)
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            )
        } else {
            JsString::Units(s.encode_utf16().collect::<Vec<_>>().into_boxed_slice())
        };
        let id = self.strings.len() as u32;
        self.strings.push(body);
        JsValue::ptr(super::jsvalue::Tag::String, id)
    }

    pub fn dup(&mut self, v: &JsValue) {
        if let Some((_, id)) = v.pointer() {
            if let Some(obj) = self.objects.get_mut(id as usize) {
                obj.header.ref_count += 1;
            }
        }
    }

    pub fn free(&mut self, v: &JsValue) {
        if let Some((_, id)) = v.pointer() {
            if let Some(obj) = self.objects.get_mut(id as usize) {
                obj.header.ref_count -= 1;
            }
        }
    }

    /// QuickJS cycle pass: objects with RC 0 after a decref walk are garbage.
    pub fn run_gc(&mut self) {
        for obj in &mut self.objects {
            if obj.header.ref_count <= 0 {
                obj.header.in_cycle_list = true;
                obj.slots.clear();
                obj.fast_array = None;
            }
        }
    }
}

/// Realm: own global, shares the runtime heap (QuickJS `JSContext`).
#[derive(Clone, Debug)]
pub struct Context {
    pub global: JsValue,
}

impl Context {
    pub fn new(rt: &mut Runtime) -> Self {
        Self {
            global: rt.new_object(JsValue::Null),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Context, Runtime};
    use crate::dynamic::jsvalue::{JsValue, Tag};

    #[test]
    fn runtime_owns_heap_context_has_global() {
        let mut rt = Runtime::new();
        let ctx = Context::new(&mut rt);
        assert_eq!(ctx.global.tag(), Tag::Object);
        assert_eq!(rt.objects.len(), 1);
        assert_eq!(rt.objects[0].header.ref_count, 1);
        assert_eq!(std::mem::size_of::<JsValue>(), 16);
        rt.objects[0].slots.push(JsValue::Int(7));
        rt.objects[0].fast_array = Some(vec![JsValue::Bool(true)]);
        assert_eq!(rt.objects[0].slots[0], JsValue::Int(7));
        assert_eq!(
            rt.objects[0].fast_array.as_ref().unwrap()[0],
            JsValue::Bool(true)
        );
    }

    #[test]
    fn rc_and_cycle_pass() {
        let mut rt = Runtime::new();
        let obj = rt.new_object(JsValue::Null);
        rt.dup(&obj);
        assert_eq!(rt.objects[0].header.ref_count, 2);
        rt.free(&obj);
        rt.free(&obj);
        rt.run_gc();
        assert!(rt.objects[0].header.in_cycle_list);
    }

    #[test]
    fn ascii_string_is_8bit() {
        let mut rt = Runtime::new();
        let s = rt.new_string("hi");
        assert_eq!(s.tag(), Tag::String);
        assert!(matches!(rt.strings[0], super::JsString::Bytes(_)));
        let _w = rt.new_string("é");
        assert!(matches!(rt.strings[1], super::JsString::Bytes(_)));
        let _wide = rt.new_string("🙂");
        assert!(matches!(rt.strings[2], super::JsString::Units(_)));
    }
}
