use super::*;
#[test]
fn observed_alignment_accounts_owned_bytes_once_and_preserves_original_store() {
    let mut vm = Vm::new(crate::SystemHost);
    let program = crate::Engine::specialize("0;", "alignment-owner.js").unwrap();
    vm.execute(&program).unwrap();
    let buffer = vm.new_fixed_array_buffer(&program, 4096, false).unwrap();
    let root = vm.heap.root(buffer);
    let accounted = vm.heap.stats().5;
    assert!(accounted >= 4096, "a fresh buffer owns its zeroed bytes");
    let offset = vm.embedding_array_buffer_aligned_offset(root, 64).unwrap();
    assert!(offset < 64);
    assert_eq!(vm.heap.stats().5, accounted);
    let snapshot = vm.heap.get(buffer).unwrap().clone();
    let Some(Cell::ArrayBuffer { bytes, .. }) = vm.heap.get_mut(buffer) else {
        panic!("ArrayBuffer");
    };
    bytes.make_mut()[offset] = 17;
    let Cell::ArrayBuffer { bytes, .. } = snapshot else {
        panic!("ArrayBuffer snapshot");
    };
    assert_eq!(bytes.read()[offset], 0);
    assert_eq!(
        vm.embedding_array_buffer_aligned_offset(root, 64).unwrap(),
        offset
    );
    assert_eq!(vm.heap.stats().5, accounted);
    vm.heap.release_root(root);
}
