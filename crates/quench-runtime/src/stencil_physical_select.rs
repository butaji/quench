/// Select an admitted region with one canonical table lookup.  The complete
/// physical view is returned so callers cannot detach bytes from ABI/layout
/// and continuation metadata.
pub fn select_stencil(key: RegionKey) -> Option<PhysicalStencilView> {
    select_physical(key)
}

/// Select one complete physical view for an ABI-specific entry.  Callers
/// should retain this value through rendering and publication so bytes and
/// boundary metadata cannot be selected independently.
pub fn select_physical_for_abi(key: RegionKey, abi: RegionAbi) -> Option<PhysicalStencilView> {
    select_physical(key)
        .filter(|view| view.executable && view.abi == abi && view.contract().abi_is_well_formed())
}

pub fn select_physical(key: RegionKey) -> Option<PhysicalStencilView> {
    let record = canonical_region_lookup(key)?;
    let artifact = match unique_artifact(BUILD_STENCIL_ARTIFACTS, key, record.name) {
        Ok(Some(artifact)) => artifact,
        Ok(None) => return Some(legacy_physical_view(key, record)),
        Err(()) => return None,
    };
    // A matching identity reserves the generated representation.  If any
    // ABI, target, layout, or effect contract differs, fail closed instead of
    // silently substituting legacy bytes with generated metadata.
    generated_physical_view(key, record, artifact)
}

fn unique_artifact<'a>(
    artifacts: &'a [BuildStencilArtifact],
    key: RegionKey,
    name: &str,
) -> Result<Option<&'a BuildStencilArtifact>, ()> {
    let mut matches = artifacts
        .iter()
        .filter(|artifact| artifact.key == key && artifact.name == name);
    let Some(first) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(());
    }
    Ok(Some(first))
}

fn legacy_physical_view(key: RegionKey, record: &'static RegionRecord) -> PhysicalStencilView {
    PhysicalStencilView {
        key,
        record,
        stencil: &record.stencil,
        generated: false,
        artifact_id: record.name,
        data: &[],
        compiler: None,
        relocations: &[],
        abi: record.abi,
        entry: record.entry,
        external_entries: record.external_entries,
        fallthrough: record.fallthrough,
        executable: record.executable,
        template_calls_helper: record.template_calls_helper,
        target: option_env!("QUENCH_BUILD_TARGET"),
        fingerprint: None,
    }
}

fn generated_physical_view(
    key: RegionKey,
    record: &'static RegionRecord,
    artifact: &'static BuildStencilArtifact,
) -> Option<PhysicalStencilView> {
    let fallthrough = artifact
        .fallthrough
        .as_ref()
        .map(|stencil| PhysicalFallthrough {
            stencil,
            target: record.fallthrough.map_or("", |item| item.target),
        });
    let metadata_matches = artifact.name == record.name
        && artifact_identity_matches(artifact, record)
        && artifact.key == key
        && artifact_target_matches_host(artifact.target)
        && !artifact.compiler.is_empty()
        && !artifact.fingerprint.is_empty()
        && artifact.abi == record.abi
        && artifact.entry == record.entry
        && artifact.external_entries == record.external_entries
        && artifact.has_fallthrough == record.fallthrough.is_some()
        && (artifact.has_fallthrough == fallthrough.is_some())
        && artifact
            .fallthrough
            .is_none_or(|_| record.fallthrough.is_some())
        && record
            .fallthrough
            .is_none_or(|item| artifact_fallthrough_matches(artifact, item));
    let effects_match = artifact.executable == record.executable
        && artifact.template_calls_helper == record.template_calls_helper;
    if !metadata_matches
        || !effects_match
        || !artifact.stencil.validate()
        || !relocations_match(artifact.stencil, artifact.relocations)
        || !artifact
            .fallthrough
            .is_none_or(|stencil| stencil.validate())
    {
        return None;
    }
    Some(PhysicalStencilView {
        key,
        record,
        stencil: &artifact.stencil,
        generated: true,
        artifact_id: artifact.artifact_id,
        data: artifact.data,
        compiler: Some(artifact.compiler),
        relocations: artifact.relocations,
        abi: artifact.abi,
        entry: artifact.entry,
        external_entries: artifact.external_entries,
        fallthrough,
        executable: artifact.executable,
        template_calls_helper: artifact.template_calls_helper,
        target: Some(artifact.target),
        fingerprint: Some(artifact.fingerprint),
    })
}

fn artifact_fallthrough_matches(
    artifact: &BuildStencilArtifact,
    fallthrough: PhysicalFallthrough,
) -> bool {
    let is_relative = |relocation: &&PhysicalRelocation| {
        matches!(
            relocation.kind,
            crate::stencil_fact::HoleKind::Branch26
                | crate::stencil_fact::HoleKind::CondBranch19
                | crate::stencil_fact::HoleKind::Rel32
        )
    };
    let relative_count = artifact.relocations.iter().filter(is_relative).count();
    !fallthrough.target.is_empty()
        && relative_count > 0
        && artifact
            .relocations
            .iter()
            .filter(is_relative)
            .all(|relocation| relocation.target == fallthrough.target)
}

fn relocations_match(stencil: Stencil, relocations: &[PhysicalRelocation]) -> bool {
    relocations.iter().all(|relocation| {
        !relocation.target.is_empty()
            && relocation.addend == 0
            && stencil
                .holes
                .iter()
                .any(|hole| hole.offset == relocation.offset && hole.kind == relocation.kind)
    })
}

fn artifact_identity_matches(artifact: &BuildStencilArtifact, record: &RegionRecord) -> bool {
    let Some(suffix) = artifact.artifact_id.strip_prefix(record.name) else {
        return false;
    };
    suffix.strip_prefix('@') == Some(artifact.fingerprint)
}

fn artifact_target_matches_host(target: &str) -> bool {
    let exact_target =
        option_env!("QUENCH_BUILD_TARGET").is_some_and(|expected| expected == target);
    if !exact_target {
        return false;
    }
    #[cfg(target_arch = "aarch64")]
    return target.starts_with("aarch64");
    #[cfg(target_arch = "x86_64")]
    return target.starts_with("x86_64");
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        let _ = target;
        false
    }
}

pub fn select_region(key: RegionKey) -> Option<&'static RegionRecord> {
    canonical_region_lookup(key)
}

/// Iterate the build-time declaration table for plan construction.  Keeping
/// this accessor next to selection means callers cannot drift a second,
/// hand-maintained list of region keys from the generated catalog.
pub(crate) fn region_records() -> &'static [RegionRecord] {
    CANONICAL_REGION_TABLE
}

/// Execute the selected region through a caller-owned semantic entry point.
/// A miss has exactly one outcome: the complete ordinary interpreter path.
/// Keeping this boundary as a table lookup prevents runtime fact-dependent
/// dispatch chains from growing around individual operations.
pub fn dispatch_region<T, E>(
    key: RegionKey,
    selected: impl FnOnce(&'static RegionRecord) -> Result<T, E>,
    fallback: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    match select_region(key) {
        Some(record) => selected(record),
        None => fallback(),
    }
}

pub fn admitted_region_key(region: RegionId, facts: &[FactState]) -> RegionKey {
    RegionKey::from_facts(region, facts)
}

pub const fn region_table_len() -> usize {
    CANONICAL_REGION_TABLE.len()
}
