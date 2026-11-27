//! No guest property access during owned clone metadata/materialization.
use super::*;
use crate::owned_clone::{Metadata, Node};
/// What an owned clone's view covers of its backing buffer, whose byte length is `bytes_len`: from
/// `offset` bytes, `length` units, or the buffer's length when `tracking` it.
#[derive(Clone, Copy)]
struct ViewSpan {
    buffer: Value,
    bytes_len: usize,
    offset: usize,
    length: usize,
    tracking: bool,
}

impl Vm {
    pub(crate) fn embedding_clone_metadata(
        &mut self,
        root: RootId,
    ) -> Result<Option<Metadata<Value>>, JsError> {
        let v = self.embedding_value(root)?;
        if self.error_is_error(v) {
            let proto = self
                .object_data(v)
                .map(|o| o.proto())
                .unwrap_or(Value::NULL);
            let mut kind = "Error";
            for (native, name) in [
                (Native::EvalError, "EvalError"),
                (Native::RangeError, "RangeError"),
                (Native::ReferenceError, "ReferenceError"),
                (Native::SyntaxError, "SyntaxError"),
                (Native::TypeError, "TypeError"),
                (Native::URIError, "URIError"),
            ] {
                if proto == self.intrinsic_builtin_prototype(native) {
                    kind = name;
                    break;
                }
            }
            return Ok(Some(Metadata::Error(kind.into())));
        }
        if let Some(Cell::ArrayBuffer {
            bytes,
            shared: true,
            ..
        }) = self.heap.get(v)
            && let Some(backing) = bytes.shared_backing() {
                return Ok(Some(Metadata::SharedBuffer(backing)));
            }
        Ok(match self.heap.get(v) {
            Some(Cell::RegExp { meta, .. }) => {
                let units = meta.source.units();
                Some(Metadata::RegExp(
                    (0..units.len()).map(|i| units.at(i)).collect(),
                    meta.flags.text(),
                ))
            }
            Some(Cell::ArrayBuffer {
                bytes,
                shared,
                detached,
                max_byte_length,
                resizable,
                ..
            }) => Some(Metadata::Buffer {
                bytes: bytes.to_vec(),
                shared: *shared,
                detached: *detached,
                maximum: *max_byte_length,
                resizable: *resizable,
            }),
            Some(Cell::TypedArray {
                object,
                buffer,
                length,
            }) => Some(Metadata::View {
                kind: format!("{:?}", object.native.kind()),
                buffer: *buffer,
                offset: object.offset,
                length: length.get().unwrap_or(0),
                tracking: length.is_tracking(),
            }),
            Some(Cell::DataView {
                object,
                buffer,
                length,
            }) => Some(Metadata::View {
                kind: "DataView".into(),
                buffer: *buffer,
                offset: object.offset,
                length: length.get().unwrap_or(0),
                tracking: length.is_tracking(),
            }),
            _ => None,
        })
    }
    pub(crate) fn embedding_clone_bigint(&mut self, text: &str) -> Result<Value, JsError> {
        let program = self.embedding_program()?;
        let digits = text.strip_prefix('-').unwrap_or(text);
        if digits.is_empty()
            || !digits.bytes().all(|x| x.is_ascii_digit())
            || (digits.len() > 1 && digits.starts_with('0'))
            || text == "-0"
        {
            return Err(self.type_error(&program, "Invalid owned clone BigInt".into()));
        }
        let Ok(value) = text.parse() else {
            return Err(self.type_error(&program, "Invalid owned clone BigInt".into()));
        };
        Ok(self.heap.alloc(Cell::bigint(value)))
    }
    pub(crate) fn embedding_clone_allocate(
        &mut self,
        node: &Node,
        operands: &[RootId],
    ) -> Result<Value, JsError> {
        let p = self.embedding_program()?;
        let operand = |vm: &Self, index: usize| -> Result<Value, JsError> {
            let root = operands
                .get(index)
                .ok_or_else(|| JsError::validation("Missing owned clone operand".into()))?;
            vm.embedding_value(*root)
        };
        let target = match node {
            Node::SharedBuffer(backing) => {
                let backing = backing.clone_for_agent();
                let proto = self.intrinsic_builtin_prototype(Native::SharedArrayBuffer);
                self.heap.alloc(Cell::ArrayBuffer {
                    object: Box::new(Self::empty_object(proto)),
                    bytes: crate::heap::BufferBytes::from_shared(backing.clone()),
                    shared: true,
                    detached: false,
                    max_byte_length: backing.maximum(),
                    resizable: backing.growable(),
                    immutable: false,
                })
            }
            Node::Object(_) => self
                .heap
                .alloc_object(Self::empty_object(self.object_proto)),
            Node::Array(length, _) => {
                let prototype = self.array_prototype_for_realm(self.realm.globals);
                let v = self.heap.alloc_array(crate::heap::ArrayObject::new(
                        Self::empty_object(prototype),
                        Vec::new(),
                    ));
                self.set_array_length(&p, v, Value::number(*length as f64))?;
                v
            }
            Node::Map(_) => {
                let proto = self.intrinsic_builtin_prototype(Native::Map);
                self.heap.alloc_map(Self::empty_object(proto), Default::default())
            }
            Node::Set(_) => {
                let proto = self.intrinsic_builtin_prototype(Native::Set);
                self.heap.alloc_set(Self::empty_object(proto), Default::default())
            }
            Node::Date(n) => {
                let proto = self.intrinsic_builtin_prototype(Native::Date);
                self.heap.alloc(Cell::Date {
                    milliseconds: *n,
                    object: Box::new(Self::empty_object(proto)),
                })
            }
            Node::Boxed(_) => {
                let value = operand(self, 0)?;
                self.box_primitive_object(value)?
            }
            Node::RegExp(source, flags) => {
                let source = self.heap.alloc_string(JsString::from_units(source));
                let source = self.heap.root(source);
                let flags = self.heap.alloc_string(flags.clone().into());
                let source_value = self.heap.root_value(source).unwrap();
                let result = self.regexp_create(&p, source_value, flags);
                self.heap.release_root(source);
                result?
            }
            Node::Buffer {
                bytes,
                maximum,
                resizable,
            } => {
                if bytes.len() > *maximum || (!*resizable && bytes.len() != *maximum) {
                    return Err(self.range_error(&p, "Invalid owned ArrayBuffer bounds".into()));
                }
                let proto = self.intrinsic_builtin_prototype(Native::ArrayBuffer);
                self.heap.alloc(Cell::ArrayBuffer {
                    object: Box::new(Self::empty_object(proto)),
                    bytes: bytes.clone().into(),
                    shared: false,
                    detached: false,
                    max_byte_length: *maximum,
                    resizable: *resizable,
                    immutable: false,
                })
            }
            Node::View {
                kind,
                offset,
                length,
                tracking,
                ..
            } => {
                let buffer = operand(self, 0)?;
                self.embedding_clone_view(&p, buffer, (kind, *offset, *length, *tracking))?
            }
            Node::Error { kind, .. } => self.embedding_clone_error(&p, kind)?,
        };
        Ok(target)
    }

    /// The view of kind `kind` (`DataView` or a typed array's name) over `buffer`, at `offset` bytes
    /// and `length` units, or tracking the buffer's length, as an owned clone describes it.
    fn embedding_clone_view(
        &mut self,
        p: &ResidualProgram,
        buffer: Value,
        (kind, offset, length, tracking): (&str, usize, usize, bool),
    ) -> Result<Value, JsError> {
        let Some(Cell::ArrayBuffer { bytes, detached, resizable, .. }) = self.heap.get(buffer) else {
            return Err(self.type_error(p, "Owned view needs ArrayBuffer".into()));
        };
        let bytes_len = bytes.len();
        if *detached || (tracking && !*resizable) {
            return Err(self.type_error(p, "Invalid owned view backing".into()));
        }
        let span = ViewSpan { buffer, bytes_len, offset, length, tracking };
        if kind == "DataView" {
            self.embedding_clone_data_view(p, &span)
        } else {
            self.embedding_clone_typed_view(p, kind, &span)
        }
    }

    fn embedding_clone_data_view(&mut self, p: &ResidualProgram, span: &ViewSpan) -> Result<Value, JsError> {
        let ViewSpan { buffer, bytes_len, offset, length, tracking } = *span;
        if offset > bytes_len || (!tracking && length > bytes_len - offset) {
            return Err(self.range_error(p, "Owned DataView out of bounds".into()));
        }
        self.materialize_global_intrinsic(p, "DataView")?;
        let proto = self.intrinsic_builtin_prototype(Native::DataView);
        Ok(self.heap.alloc(Cell::DataView {
            object: Box::new(crate::heap::BufferViewObject {
                native: Default::default(),
                object: Self::empty_object(proto),
                offset,
            }),
            buffer,
            length: crate::heap::ViewLength::new(length, tracking),
        }))
    }

    fn embedding_clone_typed_view(&mut self, p: &ResidualProgram, kind: &str, span: &ViewSpan) -> Result<Value, JsError> {
        let ViewSpan { buffer, bytes_len, offset, length, tracking } = *span;
        let stem = kind.strip_suffix("Array").unwrap_or(kind);
        // The owned clone names a kind by its constructor's name without the `Array` suffix.
        let Some(kind) = crate::heap::TYPED_ARRAYS
            .iter()
            .find(|row| row.name.strip_suffix("Array") == Some(stem))
            .map(|row| row.kind)
        else {
            return Err(self.type_error(p, "Unknown owned view kind".into()));
        };
        let width = kind.width();
        let required = if tracking {
            0
        } else {
            length.checked_mul(width).ok_or_else(|| self.range_error(p, "Owned view size overflow".into()))?
        };
        if offset % width != 0 || offset > bytes_len || required > bytes_len - offset {
            return Err(self.range_error(p, "Owned TypedArray out of bounds".into()));
        }
        let proto = self.typed_array_proto(kind);
        Ok(self.heap.alloc(Cell::TypedArray {
            object: Box::new(crate::heap::BufferViewObject {
                native: crate::heap::NativeView::typed(kind),
                object: Self::empty_object(proto),
                offset,
            }),
            buffer,
            length: crate::heap::ViewLength::new(length, tracking),
        }))
    }

    /// The error object of the class named `kind`.
    fn embedding_clone_error(&mut self, p: &ResidualProgram, kind: &str) -> Result<Value, JsError> {
        let native = match kind {
            "Error" => Native::Error,
            "EvalError" => Native::EvalError,
            "RangeError" => Native::RangeError,
            "ReferenceError" => Native::ReferenceError,
            "SyntaxError" => Native::SyntaxError,
            "TypeError" => Native::TypeError,
            "URIError" => Native::URIError,
            _ => return Err(self.type_error(p, "Unknown owned Error kind".into())),
        };
        let proto = self.intrinsic_builtin_prototype(native);
        Ok(self.heap.alloc_object(Object::error(proto)))
    }

    pub(crate) fn embedding_clone_collection_insert(
        &mut self,
        target: RootId,
        args: &[RootId],
        map: bool,
    ) -> Result<Value, JsError> {
        let p = self.embedding_program()?;
        let target = self.embedding_value(target)?;
        let values = args
            .iter()
            .map(|r| self.embedding_value(*r))
            .collect::<Result<Vec<_>, _>>()?;
        self.call_native_dispatch(
            &p,
            if map { Native::MapSet } else { Native::SetAdd },
            target,
            &values,
        )
    }
}
