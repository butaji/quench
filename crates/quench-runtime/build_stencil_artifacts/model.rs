struct OwnedDirectory {
    path: PathBuf,
}

struct ExtractedObject {
    bytes: Vec<u8>,
    fallthrough: Option<Vec<u8>>,
    relocations: Vec<DeclaredRelocation>,
    holes: Vec<ExtractedHole>,
}

#[derive(Clone, Copy)]
struct ExtractedHole {
    offset: u16,
    kind: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RelocationRecord<T> {
    section: SectionKind,
    offset: u64,
    width: usize,
    kind: &'static str,
    target: T,
    addend: i64,
}

type DeclaredRelocation = RelocationRecord<&'static str>;
type ExpectedRelocation = DeclaredRelocation;
type ObservedRelocation = RelocationRecord<String>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RelocationContractError {
    Unknown {
        offset: u64,
    },
    Duplicate {
        offset: u64,
    },
    Missing {
        offset: u64,
    },
    Width {
        offset: u64,
        expected: usize,
        actual: usize,
    },
    Addend {
        offset: u64,
        expected: i64,
        actual: i64,
    },
    RangeOverflow,
    Overlap,
}

struct ParsedFragment {
    bytes: Vec<u8>,
    relocations: Vec<DeclaredRelocation>,
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
