//! Bounds on a render's work: how many entries the directory walk may list,
//! how many bytes may be read across the manifest, every file and every
//! partial, and how many bytes a render may produce.
//!
//! A skeleton is configuration text; all three limits sit far above what one
//! holds. They apply to every render, whatever skeleton it is given, so a
//! mistake — a manifest that describes far more than any real skeleton would, a
//! partial reused past what a render can produce — fails loudly instead of
//! walking forever, exhausting memory while reading, or exhausting memory
//! while assembling. Raising any of them is a compatible change, since
//! nothing that renders under a lower limit stops rendering under a higher
//! one.

mod regular_file;

use std::path::Path;

use super::error::Reason;
use regular_file::RegularFile;

/// The most entries — files, directories, anything else — a listing of
/// `files/` and `partials/` together may hand back across one render.
pub(crate) const ENTRIES_MAX: u32 = 1024;

/// The most bytes a render may read in total, across `Cargo.toml`, every
/// file under `files/` and every file under `partials/`.
pub(crate) const BYTES_MAX: u64 = 1024 * 1024;

/// The most bytes a render may produce, across every file under `files/`,
/// checked against the largest render any choice could produce rather than
/// the one a wearer actually chose — a skeleton's validity must not depend on
/// who wears it. Reuse (one partial behind several directives, one value
/// behind many placeholders) can multiply what a skeleton ships far past what it
/// read, which is why this is its own budget rather than a multiple of
/// [`BYTES_MAX`] left implicit.
pub(crate) const RENDERED_BYTES_MAX: u64 = 8 * BYTES_MAX;

/// How many entries a render has left to list before it refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EntryBudget {
    remaining: u32,
}

impl EntryBudget {
    pub(crate) const fn new() -> Self {
        Self {
            remaining: ENTRIES_MAX,
        }
    }

    /// A budget starting with fewer than [`ENTRIES_MAX`] entries left, so a
    /// test can exhaust it without creating a thousand-entry fixture.
    #[cfg(test)]
    pub(crate) const fn with_remaining(remaining: u32) -> Self {
        Self { remaining }
    }

    /// Consumes one entry from the budget, or refuses when none remain.
    pub(crate) fn consume(&mut self) -> Result<(), Reason> {
        let Some(next) = self.remaining.checked_sub(1) else {
            return Err(Reason::TooManyEntries {
                entries_max: ENTRIES_MAX,
            });
        };
        // Postcondition: consuming can only ever shrink what remains.
        assert!(
            next < self.remaining,
            "the budget never grows back on its own"
        );
        self.remaining = next;
        Ok(())
    }
}

/// How many bytes a render has left to read before it refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ByteBudget {
    remaining: u64,
}

impl ByteBudget {
    pub(crate) const fn new() -> Self {
        Self {
            remaining: BYTES_MAX,
        }
    }

    /// Reserves `size` bytes for a file about to be read, or refuses when
    /// that would exceed what remains.
    fn reserve(&mut self, size: u64) -> Result<(), Reason> {
        if size > self.remaining {
            return Err(Reason::TooManyBytes {
                bytes_max: BYTES_MAX,
            });
        }
        self.remaining -= size;
        // Postcondition: reserving can only ever shrink what remains.
        assert!(
            self.remaining <= BYTES_MAX,
            "the budget never grows back on its own"
        );
        Ok(())
    }
}

/// How many rendered bytes a render has left to produce before it refuses.
/// Mirrors [`ByteBudget`], for the same reason: refuse before the ceiling is
/// crossed rather than after.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RenderedByteBudget {
    remaining: u64,
}

impl RenderedByteBudget {
    pub(crate) const fn new() -> Self {
        Self {
            remaining: RENDERED_BYTES_MAX,
        }
    }

    /// Reserves `bytes` of rendered output, or refuses when that would
    /// exceed what remains.
    pub(crate) fn reserve(&mut self, bytes: u64) -> Result<(), Reason> {
        if bytes > self.remaining {
            return Err(Reason::TooManyRenderedBytes {
                bytes_max: RENDERED_BYTES_MAX,
            });
        }
        self.remaining -= bytes;
        // Postcondition: reserving can only ever shrink what remains.
        assert!(
            self.remaining <= RENDERED_BYTES_MAX,
            "the budget never grows back on its own"
        );
        Ok(())
    }
}

// `error::one_based_line` relies on a byte budget that fits in a `u32`: a
// text of at most `BYTES_MAX` bytes can never hold more lines than a `u32`
// can count, and that reasoning only holds while `BYTES_MAX` itself is below
// `u32::MAX`. Written as a `u64` literal rather than a cast, since a cast
// in a const context would need arguing past clippy's own lossless-cast
// lint for no benefit here.
const _: () = assert!(BYTES_MAX < 4_294_967_295);

/// Reads `path` as bytes, reserving its declared size from `budget` before
/// reading.
///
/// `path` is judged from its own [`std::fs::symlink_metadata`] by
/// [`RegularFile::judge`], which refuses a symbolic link and anything
/// that is not a regular file before the file is ever opened.
pub(crate) fn read_bytes(path: &Path, budget: &mut ByteBudget) -> Result<Vec<u8>, Reason> {
    RegularFile::judge(path)?.read(budget)
}

/// Reads `path` as UTF-8 text under every rule [`read_bytes`] applies, then
/// refuses bytes that do not decode.
pub(crate) fn read_utf8(path: &Path, budget: &mut ByteBudget) -> Result<String, Reason> {
    let bytes = read_bytes(path, budget)?;
    String::from_utf8(bytes).map_err(|_utf8_error| Reason::NotUtf8)
}

#[cfg(test)]
mod tests {
    use super::{
        BYTES_MAX, ByteBudget, ENTRIES_MAX, EntryBudget, RENDERED_BYTES_MAX, Reason,
        RenderedByteBudget,
    };

    #[test]
    fn an_entry_budget_allows_exactly_its_declared_maximum() {
        let mut budget = EntryBudget::new();
        for _ in 0..ENTRIES_MAX {
            budget
                .consume()
                .expect("every entry up to the maximum is allowed");
        }
        assert!(
            matches!(
                budget.consume(),
                Err(Reason::TooManyEntries { entries_max }) if entries_max == ENTRIES_MAX
            ),
            "the entry past the maximum must be refused"
        );
    }

    #[test]
    fn a_byte_budget_allows_exactly_its_declared_maximum() {
        let mut budget = ByteBudget::new();
        budget
            .reserve(BYTES_MAX)
            .expect("reserving exactly the maximum must succeed");
        assert!(
            matches!(
                ByteBudget::new().reserve(BYTES_MAX + 1),
                Err(Reason::TooManyBytes { bytes_max }) if bytes_max == BYTES_MAX
            ),
            "reserving one byte past the maximum must be refused"
        );
    }

    #[test]
    fn a_byte_budget_tracks_what_earlier_reads_already_spent() {
        let mut budget = ByteBudget::new();
        budget.reserve(BYTES_MAX - 1).expect("within budget");
        assert!(
            matches!(budget.reserve(2), Err(Reason::TooManyBytes { .. })),
            "two more bytes than the one left in the budget must be refused"
        );
        budget
            .reserve(1)
            .expect("exactly the one byte left must still fit");
    }

    #[test]
    fn a_rendered_byte_budget_allows_exactly_its_declared_maximum() {
        let mut budget = RenderedByteBudget::new();
        budget
            .reserve(RENDERED_BYTES_MAX)
            .expect("reserving exactly the maximum must succeed");
        assert!(
            matches!(
                RenderedByteBudget::new().reserve(RENDERED_BYTES_MAX + 1),
                Err(Reason::TooManyRenderedBytes { bytes_max }) if bytes_max == RENDERED_BYTES_MAX
            ),
            "reserving one byte past the maximum must be refused"
        );
    }

    #[test]
    fn a_rendered_byte_budget_tracks_what_earlier_renders_already_spent() {
        let mut budget = RenderedByteBudget::new();
        budget
            .reserve(RENDERED_BYTES_MAX - 1)
            .expect("within budget");
        assert!(
            matches!(budget.reserve(2), Err(Reason::TooManyRenderedBytes { .. })),
            "two more bytes than the one left in the budget must be refused"
        );
        budget
            .reserve(1)
            .expect("exactly the one byte left must still fit");
    }

    #[test]
    fn read_utf8_reads_a_files_exact_bytes() {
        let directory = tempfile::tempdir().expect("scratch directory");
        let path = directory.path().join("plain.txt");
        std::fs::write(&path, "hello\n").expect("write fixture");

        let mut budget = ByteBudget::new();
        let text = super::read_utf8(&path, &mut budget).expect("a small UTF-8 file must read");
        assert_eq!(text, "hello\n");
    }

    #[test]
    fn read_utf8_refuses_a_file_that_is_not_valid_utf8() {
        let directory = tempfile::tempdir().expect("scratch directory");
        let path = directory.path().join("binary.bin");
        std::fs::write(&path, [0xFFu8, 0xFE]).expect("write fixture");

        let mut budget = ByteBudget::new();
        assert!(matches!(
            super::read_utf8(&path, &mut budget),
            Err(Reason::NotUtf8)
        ));
    }

    #[test]
    fn read_utf8_refuses_a_file_larger_than_the_remaining_budget() {
        let directory = tempfile::tempdir().expect("scratch directory");
        let path = directory.path().join("small.txt");
        std::fs::write(&path, "0123456789").expect("write fixture");

        let mut budget = ByteBudget { remaining: 4 };
        assert!(matches!(
            super::read_utf8(&path, &mut budget),
            Err(Reason::TooManyBytes { .. })
        ));
    }

    #[test]
    fn read_bytes_returns_bytes_that_are_not_utf8_exactly() {
        // Bytes that no UTF-8 decoder accepts, with a NUL among them, must
        // come back untouched: `read_bytes` decodes nothing.
        let directory = tempfile::tempdir().expect("scratch directory");
        let path = directory.path().join("binary.bin");
        let written = [0xFFu8, 0xFE, 0x00, b'a', 0xC0, 0x80];
        std::fs::write(&path, written).expect("write fixture");

        let mut budget = ByteBudget::new();
        let bytes = super::read_bytes(&path, &mut budget).expect("any regular file must read");
        assert_eq!(bytes, written);
    }

    #[test]
    fn read_bytes_and_read_utf8_charge_the_budget_the_same() {
        // Reads one file through each, from equal budgets, and compares the
        // budgets afterwards: what is left must match, and be exactly the
        // file's size short of the maximum.
        let directory = tempfile::tempdir().expect("scratch directory");
        let path = directory.path().join("ten.txt");
        std::fs::write(&path, "0123456789").expect("write fixture");

        let mut bytes_budget = ByteBudget::new();
        let mut utf8_budget = ByteBudget::new();
        super::read_bytes(&path, &mut bytes_budget).expect("within budget");
        super::read_utf8(&path, &mut utf8_budget).expect("within budget");
        assert_eq!(bytes_budget, utf8_budget);
        assert_eq!(bytes_budget.remaining, BYTES_MAX - 10);
    }

    #[test]
    fn read_bytes_refuses_a_file_larger_than_the_remaining_budget() {
        let directory = tempfile::tempdir().expect("scratch directory");
        let path = directory.path().join("small.txt");
        std::fs::write(&path, "0123456789").expect("write fixture");

        let mut budget = ByteBudget { remaining: 4 };
        assert!(matches!(
            super::read_bytes(&path, &mut budget),
            Err(Reason::TooManyBytes { .. })
        ));
        assert_eq!(budget.remaining, 4, "a refused read spends nothing");
    }

    #[test]
    fn read_utf8_refuses_a_symbolic_link() {
        let directory = tempfile::tempdir().expect("scratch directory");
        std::fs::write(directory.path().join("target.txt"), "hello\n").expect("link target");
        let link = directory.path().join("link.txt");
        std::os::unix::fs::symlink(directory.path().join("target.txt"), &link)
            .expect("create a symlink");

        let mut budget = ByteBudget::new();
        assert!(matches!(
            super::read_utf8(&link, &mut budget),
            Err(Reason::SymbolicLink)
        ));
    }

    #[test]
    fn read_utf8_refuses_a_directory() {
        let directory = tempfile::tempdir().expect("scratch directory");
        let nested = directory.path().join("a-directory");
        std::fs::create_dir(&nested).expect("create a directory to read as though it were a file");

        let mut budget = ByteBudget::new();
        assert!(matches!(
            super::read_utf8(&nested, &mut budget),
            Err(Reason::NotAFile)
        ));
    }
}
