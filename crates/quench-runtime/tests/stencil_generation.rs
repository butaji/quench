use quench_runtime::stencil_select::{select_physical, BUILD_STENCIL_ARTIFACTS};

#[test]
fn generated_artifact_target_is_selected_as_one_physical_view() {
    if BUILD_STENCIL_ARTIFACTS.is_empty() {
        return;
    }
    for artifact in BUILD_STENCIL_ARTIFACTS {
        let view = select_physical(artifact.key).expect("selected physical view");
        assert!(view.generated);
        assert_eq!(view.key, artifact.key);
        assert_eq!(view.artifact_id, artifact.artifact_id);
        assert_eq!(view.data, artifact.data);
        assert_eq!(view.compiler, Some(artifact.compiler));
        assert_eq!(view.relocations, artifact.relocations);
        assert_eq!(view.abi, artifact.abi);
        assert_eq!(view.entry, artifact.entry);
        assert_eq!(view.external_entries, artifact.external_entries);
        assert_eq!(view.executable, artifact.executable);
        assert_eq!(view.template_calls_helper, artifact.template_calls_helper);
        assert_eq!(view.target, Some(artifact.target));
        assert_eq!(view.stencil.bytes, artifact.stencil.bytes);
        assert_eq!(view.stencil.holes, artifact.stencil.holes);
        assert_eq!(
            view.fallthrough_fixup_offset(),
            artifact
                .fallthrough
                .map(|_| artifact.fallthrough_fixup_offset)
        );
        assert_eq!(view.fingerprint, Some(artifact.fingerprint));
    }
}

#[cfg(quench_generated_stencil_artifacts)]
#[test]
fn generated_artifacts_have_nonzero_identity_and_physical_payload() {
    assert!(!BUILD_STENCIL_ARTIFACTS.is_empty());
    for artifact in BUILD_STENCIL_ARTIFACTS {
        assert!(!artifact.artifact_id.is_empty());
        assert!(!artifact.compiler.is_empty());
        assert!(!artifact.fingerprint.is_empty());
        assert!(!artifact.bytes.is_empty());
        assert!(!artifact.stencil.bytes.is_empty());
        assert_eq!(artifact.stencil.bytes, artifact.bytes);
        assert_eq!(artifact.target, env!("QUENCH_BUILD_TARGET"));
    }
}
