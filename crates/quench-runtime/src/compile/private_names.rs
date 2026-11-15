//! The private names of a program's source: the ids its class private names resolve to, and
//! the labels they are written with.

use super::*;

pub(super) fn private_name_ids(semantic: &oxc_semantic::Semantic<'_>) -> FxHashMap<(u32, u32), u32> {
    let classes = semantic.classes();
    let mut definitions = FxHashMap::default();
    let mut next_id = 0;
    for (class_id, _) in classes.iter_enumerated() {
        for element in &classes.elements[class_id] {
            if element.is_private {
                definitions
                    .entry((class_id, element.name.to_string()))
                    .or_insert_with(|| {
                        let id = next_id;
                        next_id += 1;
                        id
                    });
            }
        }
    }
    let mut names = FxHashMap::default();
    for (class_id, _) in classes.iter_enumerated() {
        for element in &classes.elements[class_id] {
            if element.is_private {
                let id = definitions[&(class_id, element.name.to_string())];
                names.insert((element.span.start, element.span.end), id);
            }
        }
        for reference in classes.iter_private_identifiers(class_id) {
            let resolved = classes.ancestors(class_id).find_map(|ancestor| {
                definitions
                    .get(&(ancestor, reference.name.to_string()))
                    .copied()
            });
            if let Some(id) = resolved {
                names.insert((reference.span.start, reference.span.end), id);
            }
        }
    }
    names
}

pub(super) fn private_name_labels(semantic: &oxc_semantic::Semantic<'_>) -> FxHashMap<(u32, u32), String> {
    let classes = semantic.classes();
    let mut labels = FxHashMap::default();
    for (class_id, _) in classes.iter_enumerated() {
        for element in &classes.elements[class_id] {
            if element.is_private {
                labels.insert(
                    (element.span.start, element.span.end),
                    element.name.to_string(),
                );
            }
        }
        for reference in classes.iter_private_identifiers(class_id) {
            labels.insert(
                (reference.span.start, reference.span.end),
                reference.name.to_string(),
            );
        }
    }
    labels
}
