use super::*;

#[test]
fn every_mapping_owns_exactly_one_aggregate_charge() {
    let direct = StencilArena::new(1).expect("direct arena");
    assert_eq!(direct.capacity, PAGE);
    assert_eq!(direct.global_charge.bytes(), PAGE);

    let mut shared = SharedStencilSlab::new(1).expect("shared pool");
    let stencil = Stencil {
        bytes: &[0xc0, 0x03, 0x5f, 0xd6],
        holes: &[],
    };
    let mut cache = RenderedRegionCache::new();
    let site = crate::quickening::QuickeningSite::<2>::new(crate::ir::Opcode::Add);
    shared
        .render_or_get(
            &mut cache,
            crate::stencil_fact::RegionKey(0x7500),
            &stencil,
            &PatchValues::from_site(&site),
        )
        .expect("shared render");
    assert_eq!(shared.total_capacity(), PAGE);
    assert_eq!(shared.slabs[0].global_charge.bytes(), PAGE);
}
