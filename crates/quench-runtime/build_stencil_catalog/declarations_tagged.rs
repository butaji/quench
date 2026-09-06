const REGION_DECLARATION_GROUPS: &[&[RegionDeclaration]] = &[
    RUST_LEAF_DECLARATIONS,
    RUST_ASSEMBLY_DECLARATIONS,
    COMPOSED_REGION_DECLARATIONS,
];

fn region_declarations() -> Vec<RegionDeclaration> {
    REGION_DECLARATION_GROUPS
        .iter()
        .flat_map(|group| group.iter().copied())
        .collect()
}
