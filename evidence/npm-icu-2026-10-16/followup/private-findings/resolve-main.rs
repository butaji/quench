mod private_name;
mod resolve;
fn main(){
 let text=std::fs::read_to_string(std::env::args().nth(1).unwrap()).unwrap();
 let allocator=oxc_allocator::Allocator::default();
 let parsed=oxc_parser::Parser::new(&allocator,&text,oxc_span::SourceType::cjs()).parse();assert!(parsed.diagnostics.is_empty(),"{:?}",parsed.diagnostics);
 let semantic=oxc_semantic::SemanticBuilder::new().with_check_syntax_error(true).build(&parsed.program);assert!(semantic.diagnostics.is_empty(),"{:?}",semantic.diagnostics);
 let mut names=resolve::private_names(&semantic.semantic,&parsed.program).into_iter().collect::<Vec<_>>();names.sort_by_key(|(span,_)|*span);
 for (span,name) in names{println!("{:?} {:?}",span,name);}
}
