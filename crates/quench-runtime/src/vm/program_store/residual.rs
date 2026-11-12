//! A program's residual: its decoded form, or a restored snapshot's encoding.
use crate::bytecode::ResidualProgram;
use std::rc::Rc;

/// A program's residual: decoded, or a restored snapshot's encoding that its first use
/// decodes. A snapshot holds every builtin a preload loaded, and a process runs few of
/// them, so a restore decodes none: their checksums held when the snapshot was read.
pub(super) struct Residual {
    /// The program's cache site counts, known before it is decoded.
    pub(super) sites: crate::bytecode::CacheSiteCounts,
    decoded: std::cell::OnceCell<Rc<ResidualProgram>>,
    encoded: std::cell::Cell<Option<Box<EncodedResidual>>>,
}

/// What decoding a restored program needs (`ResidualProgram::decode_snapshot`).
pub(super) struct EncodedResidual {
    pub(super) bytes: crate::KeptBytes,
    pub(super) text: Option<crate::bytecode::SourceText>,
    pub(super) atoms: Rc<crate::bytecode::AtomSequence>,
}

impl Residual {
    pub(super) fn decoded(program: Rc<ResidualProgram>) -> Self {
        Self {
            sites: program.cache_site_counts(),
            decoded: std::cell::OnceCell::from(program),
            encoded: std::cell::Cell::new(None),
        }
    }

    pub(super) fn encoded(sites: crate::bytecode::CacheSiteCounts, encoded: EncodedResidual) -> Self {
        Self { sites, decoded: std::cell::OnceCell::new(), encoded: std::cell::Cell::new(Some(Box::new(encoded))) }
    }
}

impl std::ops::Deref for Residual {
    type Target = Rc<ResidualProgram>;

    #[inline(always)]
    fn deref(&self) -> &Rc<ResidualProgram> {
        match self.decoded.get() {
            Some(program) => program,
            None => self.decode(),
        }
    }
}

impl Residual {
    /// The first use of a restored snapshot's program: decodes it.
    #[cold]
    #[inline(never)]
    fn decode(&self) -> &Rc<ResidualProgram> {
        self.decoded.get_or_init(|| {
            let EncodedResidual { bytes, text, atoms } =
                *self.encoded.take().expect("a residual is decoded or holds its encoding");
            let program = ResidualProgram::decode_snapshot(bytes, text, &atoms)
                .expect("a snapshot program whose checksum held decodes");
            debug_assert_eq!(program.cache_site_counts(), self.sites);
            program.prime_loop_decoding();
            Rc::new(program)
        })
    }
}
