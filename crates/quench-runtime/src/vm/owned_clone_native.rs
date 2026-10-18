//! No guest property access during owned clone metadata/materialization.
use super::*;
use crate::owned_clone::{Metadata, Node};
impl<H: Host> Vm<H> {
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
        {
            if let Some(backing) = bytes.shared_backing() {
                return Ok(Some(Metadata::SharedBuffer(backing)));
            }
        }
        Ok(match self.heap.get(v) {
            Some(Cell::RegExp { meta, .. }) => {
                let units = meta.source.units();
                Some(Metadata::RegExp(
                    (0..units.len()).map(|i| units.at(i)).collect(),
                    meta.flags.clone(),
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
                kind,
                object,
                buffer,
                length,
                length_tracking,
            }) => Some(Metadata::View {
                kind: format!("{:?}", kind),
                buffer: *buffer,
                offset: object.offset,
                length: *length,
                tracking: *length_tracking,
            }),
            Some(Cell::DataView {
                object,
                buffer,
                length,
                length_tracking,
            }) => Some(Metadata::View {
                kind: "DataView".into(),
                buffer: *buffer,
                offset: object.offset,
                length: *length,
                tracking: *length_tracking,
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
        Ok(self.heap.alloc(Cell::BigInt(text.to_owned())))
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
                .alloc(Cell::Object(Self::empty_object(self.object_proto))),
            Node::Array(length, _) => {
                let prototype = self.array_prototype_for_realm(self.realm.globals);
                let v = self.heap.alloc(Cell::Array {
                    object: crate::heap::ArrayObject::new(
                        Self::empty_object(prototype),
                        Rc::new(Vec::new()),
                    ),
                });
                self.set_array_length(&p, v, Value::number(*length as f64))?;
                v
            }
            Node::Map(_) => {
                let proto = self.intrinsic_builtin_prototype(Native::Map);
                self.heap.alloc(Cell::Map {
                    object: Box::new(Self::empty_object(proto)),
                    entries: Box::default(),
                })
            }
            Node::Set(_) => {
                let proto = self.intrinsic_builtin_prototype(Native::Set);
                self.heap.alloc(Cell::Set {
                    object: Box::new(Self::empty_object(proto)),
                    entries: Box::default(),
                })
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
                let source = self.heap.alloc(Cell::String(JsString::from_units(source)));
                let source = self.heap.root(source);
                let flags = self.heap.alloc(Cell::String(flags.clone().into()));
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
                let Some(Cell::ArrayBuffer {
                    bytes,
                    detached,
                    resizable,
                    ..
                }) = self.heap.get(buffer)
                else {
                    return Err(self.type_error(&p, "Owned view needs ArrayBuffer".into()));
                };
                let bytes_len = bytes.len();
                if *detached || (*tracking && !*resizable) {
                    return Err(self.type_error(&p, "Invalid owned view backing".into()));
                }
                if kind == "DataView" {
                    if *offset > bytes_len || (!*tracking && *length > bytes_len - *offset) {
                        return Err(self.range_error(&p, "Owned DataView out of bounds".into()));
                    }
                    let proto = self.intrinsic_builtin_prototype(Native::DataView);
                    self.heap.alloc(Cell::DataView {
                        object: Box::new(crate::heap::BufferViewObject {
                            object: Self::empty_object(proto),
                            offset: *offset,
                        }),
                        buffer,
                        length: *length,
                        length_tracking: *tracking,
                    })
                } else {
                    let kind = match kind.as_str() {
                        "Uint8" | "Uint8Array" => TypedArrayKind::Uint8,
                        "Uint8Clamped" | "Uint8ClampedArray" => TypedArrayKind::Uint8Clamped,
                        "Uint16" | "Uint16Array" => TypedArrayKind::Uint16,
                        "Uint32" | "Uint32Array" => TypedArrayKind::Uint32,
                        "Int8" | "Int8Array" => TypedArrayKind::Int8,
                        "Int16" | "Int16Array" => TypedArrayKind::Int16,
                        "Int32" | "Int32Array" => TypedArrayKind::Int32,
                        "BigInt64" | "BigInt64Array" => TypedArrayKind::BigInt64,
                        "BigUint64" | "BigUint64Array" => TypedArrayKind::BigUint64,
                        "Float16" | "Float16Array" => TypedArrayKind::Float16,
                        "Float32" | "Float32Array" => TypedArrayKind::Float32,
                        "Float64" | "Float64Array" => TypedArrayKind::Float64,
                        _ => return Err(self.type_error(&p, "Unknown owned view kind".into())),
                    };
                    let width = kind.width();
                    let required = if *tracking {
                        0
                    } else {
                        length.checked_mul(width).ok_or_else(|| {
                            self.range_error(&p, "Owned view size overflow".into())
                        })?
                    };
                    if offset % width != 0 || *offset > bytes_len || required > bytes_len - *offset
                    {
                        return Err(self.range_error(&p, "Owned TypedArray out of bounds".into()));
                    }
                    let proto = self.typed_array_proto(kind);
                    self.heap.alloc(Cell::TypedArray {
                        kind,
                        object: Box::new(crate::heap::BufferViewObject {
                            object: Self::empty_object(proto),
                            offset: *offset,
                        }),
                        buffer,
                        length: *length,
                        length_tracking: *tracking,
                    })
                }
            }
            Node::Error { kind, .. } => {
                let native = match kind.as_str() {
                    "Error" => Native::Error,
                    "EvalError" => Native::EvalError,
                    "RangeError" => Native::RangeError,
                    "ReferenceError" => Native::ReferenceError,
                    "SyntaxError" => Native::SyntaxError,
                    "TypeError" => Native::TypeError,
                    "URIError" => Native::URIError,
                    _ => return Err(self.type_error(&p, "Unknown owned Error kind".into())),
                };
                let proto = self.intrinsic_builtin_prototype(native);
                self.heap.alloc(Cell::Object(Object::error(proto)))
            }
        };
        Ok(target)
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
        self.call_collection_native(
            &p,
            if map { Native::MapSet } else { Native::SetAdd },
            target,
            &values,
        )
    }
}
