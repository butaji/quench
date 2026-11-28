//! The shape recipe of a class's instances (`Symbol::ClassLayout`): the closed world's proven field list
//! (`AotFacts::class_layout`) as the construction the class's `new` makes, so the image names the instance
//! shape by the one recipe table every shape symbol uses.
use super::symbols::{ShapeRecipe, SymbolTable};
use crate::compiled::ahead_images::constructions::construction_recipe;
use quench_runtime::compile::aot_facts::AotFacts;

/// The recipe of the instances of class `class`, where the facts prove its instance layout: its fields in slot order, each a
/// plain data property (`construction_recipe`). The recipe's constructor and prototype are its body's closure and prototype,
/// which a synthetic class shares with its siblings, so a synthetic class has no recipe. `None` where the class has no
/// proven layout, or is synthetic.
pub(crate) fn class_recipe(facts: &AotFacts, class: u32, symbols: &mut SymbolTable) -> Option<ShapeRecipe> {
    let body = facts.class_body(class);
    if body != class { return None; }
    let fields = facts.class_layout(class)?;
    construction_recipe(body, fields.iter().copied(), symbols)
}
