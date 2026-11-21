//! Owned Node 26.12.1 config schema, reconciled with that release's option
//! declarations. Quench's existing TLS default uses its bundled CA roots.
//!
//! Source: https://github.com/nodejs/node/tree/v26.12.1/doc/node-config-schema.json
//! and src/node_options.cc. Data is independent of the local Node oracle.
//!
//! Copyright Node.js contributors. All rights reserved.
//! Permission is hereby granted, free of charge, to any person obtaining a copy
//! of this software and associated documentation files (the "Software"), to
//! deal in the Software without restriction, including without limitation the
//! rights to use, copy, modify, merge, publish, distribute, sublicense, and/or
//! sell copies of the Software, and to permit persons to whom the Software is
//! furnished to do so, subject to the following conditions:
//! The above copyright notice and this permission notice shall be included in
//! all copies or substantial portions of the Software.
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//! IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//! FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
//! AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//! LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
//! FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS
//! IN THE SOFTWARE.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

// Only ObjectFromEntries products have ordinary prototypes in the upstream
// generator. The input is private, owned JSON: walking it invokes no guest code.
#[derive(Clone, Copy)]
enum Position {
    Root,
    Defs,
    Config,
    Namespaces,
    Namespace,
    Ordinary,
    Other,
}

pub(crate) fn generate(c: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let root = c.parse_json_rooted(include_str!("schema.json"))?;
    normalize(c, root, Position::Root)?;
    Ok(root)
}

fn normalize(
    c: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    position: Position,
) -> Result<(), RootedError> {
    if !c.is_object_rooted(value)? {
        return Ok(());
    }
    if !c.is_array_rooted(value)? && !matches!(position, Position::Namespaces | Position::Ordinary)
    {
        let null = c.null();
        c.set_prototype_rooted(value, null)?;
    }
    for key in c.own_keys_rooted(value)? {
        let next = match (position, c.string_text(key)?.as_deref()) {
            (Position::Root, Some("$defs")) => Position::Defs,
            (Position::Defs, Some("config")) => Position::Config,
            (Position::Config, Some("properties")) => Position::Namespaces,
            (
                Position::Namespaces,
                Some("nodeOptions" | "bench" | "permission" | "test" | "watch"),
            ) => Position::Namespace,
            (Position::Namespace, Some("properties")) => Position::Ordinary,
            _ => Position::Other,
        };
        let child = c.get_property_rooted(value, key)?;
        normalize(c, child, next)?;
    }
    Ok(())
}
