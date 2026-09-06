struct OwnedDirectory {
    path: PathBuf,
}

struct ExtractedObject {
    bytes: Vec<u8>,
    fallthrough: Option<Vec<u8>>,
    relocations: Vec<ExtractedRelocation>,
    holes: Vec<ExtractedHole>,
}

#[derive(Clone, Copy)]
struct ExtractedHole {
    offset: u16,
    kind: &'static str,
}

#[derive(Clone, Copy)]
struct ExtractedRelocation {
    offset: u16,
    kind: &'static str,
    target: &'static str,
    addend: i64,
}

#[derive(Clone, Copy)]
struct ExpectedRelocation {
    section: SectionKind,
    offset: u16,
    width: usize,
    kind: &'static str,
    target: &'static str,
    addend: i64,
}

fn expected_relocation_index(
    expected: &[ExpectedRelocation],
    consumed: &[bool],
    section: SectionKind,
    offset: u64,
    kind: &str,
    target: &str,
) -> Option<usize> {
    expected.iter().enumerate().find_map(|(index, item)| {
        (!consumed[index]
            && item.section == section
            && u64::from(item.offset) == offset
            && item.kind == kind
            && item.target == target)
            .then_some(index)
    })
}

struct ParsedFragment {
    bytes: Vec<u8>,
    relocations: Vec<ExtractedRelocation>,
    holes: Vec<ExtractedHole>,
}

impl Drop for OwnedDirectory {
    fn drop(&mut self) {
        let Ok(entries) = fs::read_dir(&self.path) else {
            return;
        };
        for entry in entries.flatten() {
            let _ = fs::remove_file(entry.path());
        }
        let _ = fs::remove_dir(&self.path);
    }
}
