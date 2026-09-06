fn match_relocation_observations(
    expected: &[ExpectedRelocation],
    observed: &[ObservedRelocation],
) -> Result<Vec<DeclaredRelocation>, RelocationContractError> {
    validate_relocation_ranges(expected)?;
    let mut consumed = vec![false; expected.len()];
    let mut records = Vec::with_capacity(observed.len());
    for observation in observed {
        records.push(match_relocation(expected, &mut consumed, observation)?);
    }
    let missing = expected
        .iter()
        .zip(&consumed)
        .find_map(|(item, matched)| (!matched).then_some(item));
    if let Some(item) = missing {
        return Err(RelocationContractError::Missing {
            offset: item.offset,
        });
    }
    records.sort_by_key(|relocation| relocation.offset);
    Ok(records)
}

fn match_relocation(
    expected: &[ExpectedRelocation],
    consumed: &mut [bool],
    observed: &ObservedRelocation,
) -> Result<DeclaredRelocation, RelocationContractError> {
    let index = expected
        .iter()
        .position(|item| relocation_identity_matches(item, observed))
        .ok_or(RelocationContractError::Unknown {
            offset: observed.offset,
        })?;
    if consumed[index] {
        return Err(RelocationContractError::Duplicate {
            offset: observed.offset,
        });
    }
    validate_relocation_fields(&expected[index], observed)?;
    consumed[index] = true;
    Ok(expected[index].clone())
}

fn relocation_identity_matches(
    expected: &DeclaredRelocation,
    observed: &ObservedRelocation,
) -> bool {
    expected.section == observed.section
        && expected.offset == observed.offset
        && expected.kind == observed.kind
        && expected.target == observed.target
}

fn validate_relocation_fields(
    expected: &DeclaredRelocation,
    observed: &ObservedRelocation,
) -> Result<(), RelocationContractError> {
    if expected.width != observed.width {
        return Err(RelocationContractError::Width {
            offset: observed.offset,
            expected: expected.width,
            actual: observed.width,
        });
    }
    if expected.addend != observed.addend {
        return Err(RelocationContractError::Addend {
            offset: observed.offset,
            expected: expected.addend,
            actual: observed.addend,
        });
    }
    Ok(())
}

fn validate_relocation_ranges(
    expected: &[ExpectedRelocation],
) -> Result<(), RelocationContractError> {
    for (index, item) in expected.iter().enumerate() {
        let range = relocation_range(item)?;
        for other in &expected[index + 1..] {
            if range.start < relocation_range(other)?.end
                && relocation_range(other)?.start < range.end
            {
                return Err(RelocationContractError::Overlap);
            }
        }
    }
    Ok(())
}

fn relocation_range(
    relocation: &DeclaredRelocation,
) -> Result<std::ops::Range<usize>, RelocationContractError> {
    let start =
        usize::try_from(relocation.offset).map_err(|_| RelocationContractError::RangeOverflow)?;
    let end = start
        .checked_add(relocation.width)
        .ok_or(RelocationContractError::RangeOverflow)?;
    Ok(start..end)
}
