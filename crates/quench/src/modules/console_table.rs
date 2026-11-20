//! `console.table` (lib/internal/console/constructor.js `table`, lib/internal/cli_table.js):
//! the rows and columns of its data, drawn as a box.

use super::console_format_shared_vm::{self as format, Options};
use crate::host::NodeHost;
use quench_runtime::{BuiltinBrand, NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

const TABLE_INDEX: &str = "(index)";
const TABLE_ITERATION_INDEX: &str = "(iteration index)";
const TABLE_KEY: &str = "Key";
const TABLE_VALUES: &str = "Values";
/// `console.table` inspects cell values with this array limit.
const TABLE_MAX_ARRAY_LENGTH: usize = 3;
/// A cell object with more keys than this is written at depth -1.
const TABLE_SHALLOW_KEYS: usize = 2;
/// A key and its value: the entries a Map's preview holds per row.
const KEY_AND_VALUE: usize = 2;
/// The space each side of a cell's text.
const CELL_PADDING: usize = 2;
/// The UTF-16 units of one code point, at most.
const MAX_UTF16_UNITS: usize = 2;
/// The brands whose entries `table` lists by iteration index.
const COLLECTIONS: [BuiltinBrand; 4] = [BuiltinBrand::Map, BuiltinBrand::Set, BuiltinBrand::MapIterator, BuiltinBrand::SetIterator];

/// `console.table(data, properties)`'s text for an object.
pub(super) fn text(c: &mut Context<'_>, data: RootId, properties: RootId) -> Result<String, RootedError> {
    match collection_text(c, data)? {
        Some(text) => Ok(text),
        None => object_text(c, data, properties),
    }
}

/// A Map's, a Set's or one of their iterators' entries by iteration index, a key beside its
/// value where the entries are pairs; `None` for any other object.
fn collection_text(c: &mut Context<'_>, data: RootId) -> Result<Option<String>, RootedError> {
    let mut collection = false;
    for brand in COLLECTIONS {
        collection = collection || c.has_builtin_brand_rooted(data, brand)?;
    }
    let Some(preview) = c.collection_preview_rooted(data)?.filter(|_| collection) else { return Ok(None) };
    let width = if preview.is_key_value { KEY_AND_VALUE } else { 1 };
    let rows = preview.entries.len() / width;
    let mut body = vec![(0..rows).map(|row| Some(row.to_string())).collect::<Vec<_>>()];
    for offset in 0..width {
        let mut column = Vec::with_capacity(rows);
        for row in 0..rows {
            column.push(Some(cell(c, preview.entries[row * width + offset])?));
        }
        body.push(column);
    }
    let head: &[&str] = if preview.is_key_value { &[TABLE_ITERATION_INDEX, TABLE_KEY, TABLE_VALUES] } else { &[TABLE_ITERATION_INDEX, TABLE_VALUES] };
    Ok(Some(render_table(&head.iter().map(|name| (*name).to_owned()).collect::<Vec<_>>(), &body)))
}

/// `console.table`'s cell: `inspect` on one line, three array entries, shallow for an object
/// of more than two keys.
fn cell(c: &mut Context<'_>, value: RootId) -> Result<String, RootedError> {
    let shallow = c.is_object_rooted(value)?
        && !c.is_array_rooted(value)?
        && c.enumerable_own_keys_rooted(value, false, true)?.len() > TABLE_SHALLOW_KEYS;
    let options = Options {
        depth: Some(if shallow { -1.0 } else { 0.0 }),
        max_array_length: TABLE_MAX_ARRAY_LENGTH,
        layout: format::Layout::SingleLine,
        ..Options::default()
    };
    format::inspect(c, value, options)
}

/// The cells of an object's or an array's table: a column per property name its rows hold,
/// in the order first seen, and the `Values` column of its primitive rows.
struct ObjectTable {
    index: Vec<Option<String>>,
    columns: Vec<(String, Vec<Option<String>>)>,
    values: Vec<Option<String>>,
    has_primitives: bool,
}

impl ObjectTable {
    /// Adds row `row`, the property `key` of the data whose value is `item`: a primitive is
    /// its own cell unless `filter` names the columns; an object gives a cell per key.
    fn add_row(&mut self, c: &mut Context<'_>, row: usize, key: RootId, item: RootId, filter: Option<&[RootId]>) -> Result<(), RootedError> {
        self.index.push(Some(c.to_string(key)?));
        let primitive = !c.is_object_rooted(item)? && !c.is_callable_rooted(item)?;
        if filter.is_none() && primitive {
            self.has_primitives = true;
            self.values[row] = Some(cell(c, item)?);
            return Ok(());
        }
        let keys = match filter {
            Some(keys) => keys.to_vec(),
            None => c.enumerable_own_keys_rooted(item, false, true)?,
        };
        for key in keys {
            let name = c.to_string(key)?;
            let own = !primitive && c.own_property_fields_rooted(item, key)?.is_some();
            let text = if own {
                let value = c.get_property_rooted(item, key)?;
                cell(c, value)?
            } else {
                String::new()
            };
            let rows = self.values.len();
            let position = self.columns.iter().position(|(column, _)| *column == name).unwrap_or_else(|| {
                self.columns.push((name, vec![None; rows]));
                self.columns.len() - 1
            });
            self.columns[position].1[row] = Some(text);
        }
        Ok(())
    }

    /// The box: the index, each column, then `Values` where a row was a primitive.
    fn render(self) -> String {
        let mut head = vec![TABLE_INDEX.to_owned()];
        let mut body = vec![self.index];
        for (name, column) in self.columns {
            head.push(name);
            body.push(column);
        }
        if self.has_primitives {
            head.push(TABLE_VALUES.to_owned());
            body.push(self.values);
        }
        render_table(&head, &body)
    }
}

/// The rows and columns of `console.table(data, properties)` for an array or object.
fn object_text(c: &mut Context<'_>, data: RootId, properties: RootId) -> Result<String, RootedError> {
    let filter = if c.rooted_value(properties).is_some_and(|value| value.is_undefined()) {
        None
    } else {
        Some(c.array_like_values_rooted(properties)?)
    };
    let index_keys = c.enumerable_own_keys_rooted(data, false, true)?;
    let mut table = ObjectTable { index: Vec::with_capacity(index_keys.len()), columns: Vec::new(), values: vec![None; index_keys.len()], has_primitives: false };
    for (row, &key) in index_keys.iter().enumerate() {
        let item = c.get_property_rooted(data, key)?;
        table.add_row(c, row, key, item, filter.as_deref())?;
    }
    Ok(table.render())
}

/// `getStringWidth` of a cell: the columns its characters take on a terminal.
fn width(text: &str) -> usize {
    let mut units = [0; MAX_UTF16_UNITS];
    text.chars().map(|character| super::readline_shared_vm::character_width(character.encode_utf16(&mut units)) as usize).sum()
}

/// Node's `cli_table` box drawing.
fn render_table(head: &[String], columns: &[Vec<Option<String>>]) -> String {
    let rows = columns.iter().map(Vec::len).max().unwrap_or(0);
    let mut widths = head.iter().map(|name| width(name)).collect::<Vec<_>>();
    for (index, column) in columns.iter().enumerate() {
        for value in column.iter().flatten() {
            widths[index] = widths[index].max(width(value));
        }
    }
    let render_row = |cells: &[&str]| {
        let mut out = String::from("│ ");
        for (index, cell) in cells.iter().enumerate() {
            out += cell;
            out += &" ".repeat(widths[index] - width(cell));
            if index != cells.len() - 1 {
                out += " │ ";
            }
        }
        out + " │"
    };
    let divider = widths.iter().map(|width| "─".repeat(width + CELL_PADDING)).collect::<Vec<_>>();
    let head_cells = head.iter().map(String::as_str).collect::<Vec<_>>();
    let mut result = format!("┌{}┐\n{}\n├{}┤\n", divider.join("┬"), render_row(&head_cells), divider.join("┼"));
    for row in 0..rows {
        let cells = columns.iter().map(|column| column.get(row).cloned().flatten().unwrap_or_default()).collect::<Vec<_>>();
        let cells = cells.iter().map(String::as_str).collect::<Vec<_>>();
        result += &render_row(&cells);
        result.push('\n');
    }
    result + &format!("└{}┘", divider.join("┴"))
}
