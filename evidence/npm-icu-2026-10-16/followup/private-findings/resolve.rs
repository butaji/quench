use rustc_hash::FxHashMap;
use oxc_ast_visit::Visit;
use crate::private_name::{Name, Receiver};
pub fn private_names<'a>(semantic: &oxc_semantic::Semantic<'a>, program: &oxc_ast::ast::Program<'a>) -> FxHashMap<(u32,u32), Name> {
    struct Classes(FxHashMap<(u32,u32),String>);
    impl<'a> Visit<'a> for Classes {
        fn visit_class(&mut self, class: &oxc_ast::ast::Class<'a>) {
            let name=class.id.as_ref().map_or("anonymous",|id|id.name.as_str());
            for element in &class.body.body {
                use oxc_ast::ast::{ClassElement,PropertyKey};
                let key=match element {
                    ClassElement::PropertyDefinition(field)=>Some(&field.key),
                    ClassElement::MethodDefinition(method)=>Some(&method.key),
                    ClassElement::AccessorProperty(accessor)=>Some(&accessor.key),
                    _=>None,
                };
                if let Some(PropertyKey::PrivateIdentifier(id))=key {
                    self.0.insert((id.span.start,id.span.end),name.to_owned());
                }
            }
            oxc_ast_visit::walk::walk_class(self,class);
        }
    }
    let mut lexical_names=Classes(FxHashMap::default());lexical_names.visit_program(program);
    let classes = semantic.classes();
    let mut definitions = FxHashMap::default();
    let mut next_id = 0;
    for (class_id,_) in classes.iter_enumerated() {
        for element in &classes.elements[class_id] {
            if element.is_private {
                let class_name=lexical_names.0[&(element.span.start,element.span.end)].as_str();
                definitions.entry((class_id,element.name.to_string())).or_insert_with(|| {
                    let id=next_id;next_id+=1;
                    let receiver=if element.kind.is_property(){Receiver::Field}
                        else if element.r#static {Receiver::Static(class_name.to_owned())}
                        else {Receiver::Instance(class_name.to_owned())};
                    Name{id,label:element.name.to_string(),receiver}
                });
            }
        }
    }
    let mut names=FxHashMap::default();
    for (class_id,_) in classes.iter_enumerated(){
        for element in &classes.elements[class_id]{
            if element.is_private{
                names.insert((element.span.start,element.span.end),definitions[&(class_id,element.name.to_string())].clone());
            }
        }
        for reference in classes.iter_private_identifiers(class_id){
            if let Some(name)=classes.ancestors(class_id).find_map(|ancestor|definitions.get(&(ancestor,reference.name.to_string()))){
                names.insert((reference.span.start,reference.span.end),name.clone());
            }
        }
    }
    names
}
