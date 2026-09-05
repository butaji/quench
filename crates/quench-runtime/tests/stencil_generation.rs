use quench_runtime::stencil_select::{select_physical, BUILD_STENCIL_ARTIFACTS};

#[test]
fn generated_artifact_target_is_selected_as_one_physical_view() {
    if BUILD_STENCIL_ARTIFACTS.is_empty() {
        return;
    }
    let key = quench_runtime::stencil_select::add_const_region_key();
    let artifact = BUILD_STENCIL_ARTIFACTS
        .iter()
        .find(|artifact| artifact.key == key)
        .expect("generated AddConst artifact");
    let view = select_physical(key).expect("selected physical view");
    assert!(view.generated);
    assert_eq!(view.key, artifact.key);
    assert_eq!(view.abi, artifact.abi);
    assert_eq!(view.stencil.bytes, artifact.stencil.bytes);
    assert_eq!(view.fingerprint, Some(artifact.fingerprint));
}

