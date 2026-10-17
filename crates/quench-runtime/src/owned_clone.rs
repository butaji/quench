//! Owned cross-runtime clone draft; no VM handles or reference-counted payloads.
use crate::{BuiltinBrand as B, BuiltinData as D, Host, NativeContext, RootId, RootedError};
#[derive(Clone, Debug)]
pub enum Value {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(Vec<u16>),
    BigInt(String),
    Ref(usize),
}
#[derive(Clone, Debug)]
pub enum Node {
    SharedBuffer(std::sync::Arc<crate::shared_backing::SharedBacking>),
    Object(Vec<(Vec<u16>, Value)>),
    Array(u32, Vec<(Vec<u16>, Value)>),
    Map(Vec<(Value, Value)>),
    Set(Vec<Value>),
    Date(f64),
    Boxed(Value),
    RegExp(Vec<u16>, String),
    Buffer {
        bytes: Vec<u8>,
        maximum: usize,
        resizable: bool,
    },
    View {
        kind: String,
        buffer: Value,
        offset: usize,
        length: usize,
        tracking: bool,
    },
    Error {
        kind: String,
        message: Option<Value>,
        stack: Option<Value>,
        cause: Option<Value>,
    },
}
#[derive(Clone, Debug)]
pub enum Metadata<T = RootId> {
    SharedBuffer(std::sync::Arc<crate::shared_backing::SharedBacking>),
    RegExp(Vec<u16>, String),
    Buffer {
        bytes: Vec<u8>,
        shared: bool,
        detached: bool,
        maximum: usize,
        resizable: bool,
    },
    View {
        kind: String,
        buffer: T,
        offset: usize,
        length: usize,
        tracking: bool,
    },
    Error(String),
}
#[derive(Clone, Debug)]
pub struct Graph {
    pub root: Value,
    pub nodes: Vec<Node>,
}
fn get<H: Host>(c: &mut NativeContext<'_, H>, o: RootId, k: &str) -> Result<RootId, RootedError> {
    let k = c.string_rooted(k);
    c.get_property_rooted(o, k)
}
fn num<H: Host>(c: &mut NativeContext<'_, H>, o: RootId, k: &str) -> Result<usize, RootedError> {
    let v = get(c, o, k)?;
    Ok(c.to_number(v)? as usize)
}
struct Capture {
    roots: Vec<RootId>,
    nodes: Vec<Option<Node>>,
}
impl Capture {
    fn value<H: Host>(
        &mut self,
        c: &mut NativeContext<'_, H>,
        r: RootId,
    ) -> Result<Value, RootedError> {
        let v = c
            .rooted_value(r)
            .ok_or_else(|| RootedError::host("invalid clone root"))?;
        if v.is_undefined() {
            return Ok(Value::Undefined);
        }
        if v.is_null() {
            return Ok(Value::Null);
        }
        if let Some(b) = v.as_bool() {
            return Ok(Value::Bool(b));
        }
        if let Some(n) = v.as_number() {
            return Ok(Value::Number(n));
        }
        if let Some(s) = c.string_code_units(r)? {
            return Ok(Value::String(s));
        }
        if !c.is_object_rooted(r)? {
            return Ok(Value::BigInt(c.to_string(r)?));
        }
        for (i, old) in self.roots.iter().enumerate() {
            if c.strict_equal_rooted(*old, r)? {
                return Ok(Value::Ref(i));
            }
        }
        let id = self.nodes.len();
        self.roots.push(r);
        self.nodes.push(None);
        if let Some(metadata) = c.clone_metadata_rooted(r)? {
            let node = match metadata {
                Metadata::SharedBuffer(backing) => Node::SharedBuffer(backing),
                Metadata::RegExp(s, f) => Node::RegExp(s, f),
                Metadata::Error(kind) => {
                    let mut fields = Vec::new();
                    for k in ["message", "stack", "cause"] {
                        let key = c.string_rooted(k);
                        let d = c.own_property_descriptor_rooted(r, key)?;
                        fields.push(if c.rooted_value(d).unwrap().is_undefined() {
                            None
                        } else {
                            let v = get(c, d, "value")?;
                            Some(self.value(c, v)?)
                        });
                    }
                    Node::Error {
                        kind,
                        message: fields[0].clone(),
                        stack: fields[1].clone(),
                        cause: fields[2].clone(),
                    }
                }
                Metadata::Buffer {
                    bytes,
                    shared,
                    detached,
                    maximum,
                    resizable,
                } => {
                    if shared || detached {
                        return Err(RootedError::host(
                            "Unsupported shared/detached owned clone backing",
                        ));
                    }
                    Node::Buffer {
                        bytes,
                        maximum,
                        resizable,
                    }
                }
                Metadata::View {
                    kind,
                    buffer,
                    offset,
                    length,
                    tracking,
                } => Node::View {
                    kind,
                    buffer: self.value(c, buffer)?,
                    offset,
                    length,
                    tracking,
                },
            };
            self.nodes[id] = Some(node);
            return Ok(Value::Ref(id));
        }
        let node = match c.builtin_data_rooted(r)? {
            Some(D::Map(entries)) => {
                let mut out = Vec::new();
                for (k, v) in entries {
                    out.push((self.value(c, k)?, self.value(c, v)?))
                }
                Node::Map(out)
            }
            Some(D::Set(entries)) => {
                let mut out = Vec::new();
                for v in entries {
                    out.push(self.value(c, v)?)
                }
                Node::Set(out)
            }
            Some(D::Primitive {
                brand: B::Date,
                value,
            }) => Node::Date(c.to_number(value)?),
            Some(D::Primitive { value, .. }) => Node::Boxed(self.value(c, value)?),
            Some(_) => return Err(RootedError::host("unsupported owned clone node")),
            None => {
                let array = c.is_array_rooted(r)?;
                let length = if array {
                    num(c, r, "length")? as u32
                } else {
                    0
                };
                let mut props = Vec::new();
                for k in c.enumerable_own_keys_rooted(r, false, true)? {
                    let key = c.to_string_code_units(k)?;
                    let v = c.get_property_rooted(r, k)?;
                    props.push((key, self.value(c, v)?))
                }
                if array {
                    Node::Array(length, props)
                } else {
                    Node::Object(props)
                }
            }
        };
        self.nodes[id] = Some(node);
        Ok(Value::Ref(id))
    }
}
pub fn capture<H: Host>(
    c: &mut NativeContext<'_, H>,
    source: RootId,
    transfers: &[RootId],
) -> Result<Graph, RootedError> {
    // Core clone owns original getter order, source rooting, transfer validation and
    // detach commit. Subsequent export walks the resulting getter-free clone.
    let cloned = c.structured_clone_with_transfers_rooted(source, transfers)?;
    {
        let mut cap = Capture {
            roots: Vec::new(),
            nodes: Vec::new(),
        };
        let root = cap.value(c, cloned)?;
        Ok(Graph {
            root,
            nodes: cap.nodes.into_iter().map(Option::unwrap).collect(),
        })
    }
}
fn primitive<H: Host>(
    c: &mut NativeContext<'_, H>,
    v: &Value,
    roots: &[Option<RootId>],
) -> Result<RootId, RootedError> {
    Ok(match v {
        Value::Undefined => c.undefined(),
        Value::Null => c.null(),
        Value::Bool(b) => c.boolean(*b),
        Value::Number(n) => c.number(*n),
        Value::String(s) => c.string_units_rooted(s),
        Value::BigInt(s) => c.clone_bigint_rooted(s)?,
        Value::Ref(i) => roots
            .get(*i)
            .copied()
            .flatten()
            .ok_or_else(|| RootedError::host("unallocated owned clone reference"))?,
    })
}
fn define<H: Host>(
    c: &mut NativeContext<'_, H>,
    o: RootId,
    k: &[u16],
    v: RootId,
    enumerable: bool,
) -> Result<(), RootedError> {
    let k = c.string_units_rooted(k);
    if !c.define_data_property_rooted(o, k, v, true, enumerable, true)? {
        return Err(RootedError::host("cannot define clone property"));
    }
    Ok(())
}
pub fn import<H: Host>(c: &mut NativeContext<'_, H>, g: &Graph) -> Result<RootId, RootedError> {
    let mut roots = vec![None; g.nodes.len()];
    for (i, node) in g.nodes.iter().enumerate() {
        if matches!(node, Node::View { .. }) {
            continue;
        }
        let operands = if let Node::Boxed(v) = node {
            vec![primitive(c, v, &roots)?]
        } else {
            vec![]
        };
        roots[i] = Some(c.clone_allocate_rooted(node, &operands)?)
    }
    for (i, node) in g.nodes.iter().enumerate() {
        if let Node::View { buffer, .. } = node {
            let b = primitive(c, buffer, &roots)?;
            roots[i] = Some(c.clone_allocate_rooted(node, &[b])?)
        }
    }
    for (i, node) in g.nodes.iter().enumerate() {
        let r = roots[i].unwrap();
        match node {
            Node::Object(p) | Node::Array(_, p) => {
                for (k, v) in p {
                    let v = primitive(c, v, &roots)?;
                    define(c, r, k, v, true)?
                }
            }
            Node::Map(entries) => {
                for (k, v) in entries {
                    let k = primitive(c, k, &roots)?;
                    let v = primitive(c, v, &roots)?;
                    c.clone_collection_insert_rooted(r, &[k, v], true)?;
                }
            }
            Node::Set(entries) => {
                for v in entries {
                    let v = primitive(c, v, &roots)?;
                    c.clone_collection_insert_rooted(r, &[v], false)?;
                }
            }
            Node::Error {
                message,
                stack,
                cause,
                ..
            } => {
                for (k, v) in [("message", message), ("stack", stack), ("cause", cause)] {
                    if let Some(v) = v {
                        let v = primitive(c, v, &roots)?;
                        define(c, r, &k.encode_utf16().collect::<Vec<_>>(), v, false)?
                    }
                }
            }
            _ => {}
        }
    }
    primitive(c, &g.root, &roots)
}
