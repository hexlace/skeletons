//! Parsing `git ls-files -v --stage -z`'s own output — one record per index
//! entry, `<tag> SP <mode> SP <oid> SP <stage> TAB <path> NUL` — and reading
//! the one letter that says whether git looks at the file in the work tree.
//! `sync` proves every path it writes against these records, and `wear` the
//! two files it changes.

use crate::git::ObjectId;

/// What `ls-files -v` says about how git treats one index entry, the one
/// letter it puts before the record. `-f` is never passed, so the letters
/// that mean an fsmonitor-valid entry (git lowercases them under `-f`) never
/// appear; a letter outside these five makes the record unreadable rather
/// than guessed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IndexTag {
    /// `H`: tracked, and git reads the file from the work tree.
    Tracked,
    /// `S`: skip-worktree, so git does not read the file from the work tree.
    SkipWorktree,
    /// `h`: assume-unchanged, so git does not look at the file.
    AssumeUnchanged,
    /// `s`: both.
    SkipWorktreeAndAssumeUnchanged,
    /// `M`: unmerged, at every stage.
    Unmerged,
}

/// Which flag hides a present file from git.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HiddenFlag {
    SkipWorktree,
    AssumeUnchanged,
    Both,
}

impl IndexTag {
    /// The flag that makes git not read this entry's file from the work tree,
    /// or `None` for an entry git does read (tracked, or unmerged). Asked of
    /// the tag so that the three hiding letters are read in one place for
    /// every command that refuses a hidden file.
    pub(crate) const fn hiding_flag(self) -> Option<HiddenFlag> {
        match self {
            Self::SkipWorktree => Some(HiddenFlag::SkipWorktree),
            Self::AssumeUnchanged => Some(HiddenFlag::AssumeUnchanged),
            Self::SkipWorktreeAndAssumeUnchanged => Some(HiddenFlag::Both),
            Self::Tracked | Self::Unmerged => None,
        }
    }

    const fn from_byte(letter: u8) -> Option<Self> {
        match letter {
            b'H' => Some(Self::Tracked),
            b'S' => Some(Self::SkipWorktree),
            b'h' => Some(Self::AssumeUnchanged),
            b's' => Some(Self::SkipWorktreeAndAssumeUnchanged),
            b'M' => Some(Self::Unmerged),
            _ => None,
        }
    }
}

/// One index entry `git ls-files -v --stage -z` reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexRecord {
    pub(crate) tag: IndexTag,
    pub(crate) mode: String,
    pub(crate) object: ObjectId,
    pub(crate) stage: u8,
    pub(crate) path: Vec<u8>,
}

/// The one record of `git ls-files -v --stage -z`'s output that does not have
/// the shape `<tag> SP <mode> SP <oid> SP <stage> TAB <path>`, kept whole (the shape
/// is fixed, so the record itself is what identifies what is wrong with
/// it). Displays as the sentence `sync` shows, with the record lossily
/// decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MalformedIndexRecord {
    record: Vec<u8>,
}

impl MalformedIndexRecord {
    fn new(record: &[u8]) -> Self {
        Self {
            record: record.to_vec(),
        }
    }
}

impl std::fmt::Display for MalformedIndexRecord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "git listed an index entry `skeletons` cannot read: {}",
            String::from_utf8_lossy(&self.record)
        )
    }
}

impl std::error::Error for MalformedIndexRecord {}

/// Parses every record in `bytes` (`ls-files -v --stage -z`'s own output).
///
/// # Errors
///
/// Returns the first record that does not parse as `<tag> SP <mode> SP <oid>
/// SP <stage> TAB <path>`, with a tag from [`IndexTag`], as a
/// [`MalformedIndexRecord`] holding the record itself — never partway
/// through: a caller that cannot read one entry cannot trust the shape of
/// any other either.
pub(crate) fn parse_ls_files_tagged(
    bytes: &[u8],
) -> Result<Vec<IndexRecord>, MalformedIndexRecord> {
    bytes
        .split(|&byte| byte == 0)
        .filter(|record| !record.is_empty())
        .map(parse_one_index_record)
        .collect()
}

fn parse_one_index_record(record: &[u8]) -> Result<IndexRecord, MalformedIndexRecord> {
    // The tag is exactly one letter and a space, so a record that opens with
    // anything else (an untagged `ls-files --stage` line, say) is refused
    // here rather than read with its mode taken for a tag.
    let Some((&letter, after_letter)) = record.split_first() else {
        return Err(MalformedIndexRecord::new(record));
    };
    let tag = IndexTag::from_byte(letter).ok_or_else(|| MalformedIndexRecord::new(record))?;
    let Some(record_after_tag) = after_letter.strip_prefix(b" ") else {
        return Err(MalformedIndexRecord::new(record));
    };

    let first_space = record_after_tag
        .iter()
        .position(|&byte| byte == b' ')
        .ok_or_else(|| MalformedIndexRecord::new(record))?;
    let mode = String::from_utf8_lossy(&record_after_tag[..first_space]).into_owned();

    let after_mode = &record_after_tag[first_space + 1..];
    let second_space = after_mode
        .iter()
        .position(|&byte| byte == b' ')
        .ok_or_else(|| MalformedIndexRecord::new(record))?;
    let object_text = String::from_utf8_lossy(&after_mode[..second_space]);
    let object = ObjectId::parse(&object_text).ok_or_else(|| MalformedIndexRecord::new(record))?;

    let after_object = &after_mode[second_space + 1..];
    let tab = after_object
        .iter()
        .position(|&byte| byte == b'\t')
        .ok_or_else(|| MalformedIndexRecord::new(record))?;
    let stage_text = String::from_utf8_lossy(&after_object[..tab]);
    let stage: u8 = stage_text
        .parse()
        .map_err(|_parse_error| MalformedIndexRecord::new(record))?;

    let path = after_object[tab + 1..].to_vec();
    Ok(IndexRecord {
        tag,
        mode,
        object,
        stage,
        path,
    })
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{HiddenFlag, IndexTag, parse_ls_files_tagged};

    #[test]
    fn each_tag_that_hides_a_file_names_its_flag_and_the_others_name_none() {
        // Every tag, so a new one cannot be left out: the three letters that
        // make git look away name the flag that does, and tracked and
        // unmerged entries are ones git reads.
        for (tag, expected) in [
            (IndexTag::Tracked, None),
            (IndexTag::SkipWorktree, Some(HiddenFlag::SkipWorktree)),
            (IndexTag::AssumeUnchanged, Some(HiddenFlag::AssumeUnchanged)),
            (
                IndexTag::SkipWorktreeAndAssumeUnchanged,
                Some(HiddenFlag::Both),
            ),
            (IndexTag::Unmerged, None),
        ] {
            assert_eq!(tag.hiding_flag(), expected, "{tag:?}");
        }
    }

    #[test]
    fn empty_input_parses_to_no_entries() {
        assert!(
            parse_ls_files_tagged(b"")
                .expect("empty input parses")
                .is_empty()
        );
    }

    #[test]
    fn a_regular_stage_zero_entry_is_read() {
        let bytes = b"H 100644 7898192e4d1a1e6c0e7c8e6a1a1e6c0e7c8e6a1a 0\tf.txt\0";
        let records = parse_ls_files_tagged(bytes).expect("must parse");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].mode, "100644");
        assert_eq!(records[0].stage, 0);
        assert_eq!(records[0].path, b"f.txt");
    }

    #[test]
    fn an_executable_entry_is_read() {
        let bytes = b"H 100755 7898192e4d1a1e6c0e7c8e6a1a1e6c0e7c8e6a1a 0\tf.txt\0";
        let records = parse_ls_files_tagged(bytes).expect("must parse");
        assert_eq!(records[0].mode, "100755");
    }

    #[test]
    fn a_symlink_entry_is_read() {
        let bytes = b"H 120000 7f66e4f7c8e6a1a1e6c0e7c8e6a1a1e6c0e7c8e6 0\tl\0";
        let records = parse_ls_files_tagged(bytes).expect("must parse");
        assert_eq!(records[0].mode, "120000");
    }

    #[test]
    fn a_gitlink_entry_is_read() {
        let bytes = b"H 160000 b10e1234e4d1a1e6c0e7c8e6a1a1e6c0e7c8e6a1 0\t.github\0";
        let records = parse_ls_files_tagged(bytes).expect("must parse");
        assert_eq!(records[0].mode, "160000");
    }

    #[test]
    fn three_conflict_stages_are_all_read() {
        let bytes = b"H 100644 df961234e4d1a1e6c0e7c8e6a1a1e6c0e7c8e6a1 1\tf.txt\0\
                       H 100644 28ce1234e4d1a1e6c0e7c8e6a1a1e6c0e7c8e6a1 2\tf.txt\0\
                       H 100644 13e71234e4d1a1e6c0e7c8e6a1a1e6c0e7c8e6a1 3\tf.txt\0";
        let records = parse_ls_files_tagged(bytes).expect("must parse");
        assert_eq!(records.len(), 3);
        assert_eq!(
            records
                .iter()
                .map(|record| record.stage)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn a_path_that_holds_a_literal_tab_is_read_in_full() {
        let bytes = b"H 100644 7898192e4d1a1e6c0e7c8e6a1a1e6c0e7c8e6a1a 0\ta\tb.txt\0";
        let records = parse_ls_files_tagged(bytes).expect("must parse");
        assert_eq!(records[0].path, b"a\tb.txt");
    }

    #[test]
    fn nfc_path_bytes_are_read_exactly() {
        let bytes =
            "H 100644 7898192e4d1a1e6c0e7c8e6a1a1e6c0e7c8e6a1a 0\tcaf\u{e9}.txt\0".as_bytes();
        let records = parse_ls_files_tagged(bytes).expect("must parse");
        assert_eq!(records[0].path, "café.txt".as_bytes());
    }

    /// One well-formed record's tail, for a test that varies only the tag.
    const RECORD_TAIL: &str = " 100644 7898192e4d1a1e6c0e7c8e6a1a1e6c0e7c8e6a1a 0\tf.txt\0";

    #[test]
    fn every_tag_git_prints_is_read_as_its_own_meaning() {
        // `H`, `S`, `h`, `s` and `M` are what `ls-files -v` prints without
        // `-f` (captured: git 2.53.0): tracked, skip-worktree,
        // assume-unchanged, both, and unmerged.
        for (letter, expected) in [
            ("H", IndexTag::Tracked),
            ("S", IndexTag::SkipWorktree),
            ("h", IndexTag::AssumeUnchanged),
            ("s", IndexTag::SkipWorktreeAndAssumeUnchanged),
            ("M", IndexTag::Unmerged),
        ] {
            let bytes = format!("{letter}{RECORD_TAIL}");
            let records = parse_ls_files_tagged(bytes.as_bytes()).expect("must parse");
            assert_eq!(records[0].tag, expected, "{letter}");
            assert_eq!(
                records[0].path, b"f.txt",
                "{letter}: the rest is read as before"
            );
        }
    }

    #[test]
    fn a_tag_git_does_not_print_is_rejected() {
        // `-f` would print `F`-family letters for fsmonitor-valid entries,
        // and `-t` prints `R`, `C`, `K` and `?`; none is asked for, so none
        // is guessed at.
        for letter in ["F", "f", "R", "C", "K", "?", "x", "1", "0"] {
            let bytes = format!("{letter}{RECORD_TAIL}");
            assert!(
                parse_ls_files_tagged(bytes.as_bytes()).is_err(),
                "{letter} is not a tag `skeletons` asked git for"
            );
        }
    }

    #[test]
    fn a_record_with_no_tag_is_rejected_not_read_with_its_mode_as_the_tag() {
        // What `ls-files --stage -z` without `-v` prints. Read leniently, the
        // mode `100644` would be taken for the tag `1`; refusing it keeps a
        // command built without `-v` from passing every tag check silently.
        let untagged = b"100644 7898192e4d1a1e6c0e7c8e6a1a1e6c0e7c8e6a1a 0\tf.txt\0";
        assert!(parse_ls_files_tagged(untagged).is_err());
    }

    #[test]
    fn a_tag_with_no_space_after_it_is_rejected() {
        let bytes = b"H100644 7898192e4d1a1e6c0e7c8e6a1a1e6c0e7c8e6a1a 0\tf.txt\0";
        assert!(parse_ls_files_tagged(bytes).is_err());
        assert!(parse_ls_files_tagged(b"H\0").is_err());
    }

    #[test]
    fn a_malformed_mode_is_rejected() {
        assert!(parse_ls_files_tagged(b"not-a-mode\0").is_err());
    }

    #[test]
    fn a_malformed_stage_is_rejected() {
        let bytes = b"H 100644 7898192e4d1a1e6c0e7c8e6a1a1e6c0e7c8e6a1a not-a-stage\tf.txt\0";
        assert!(parse_ls_files_tagged(bytes).is_err());
    }

    #[test]
    fn a_malformed_object_id_is_rejected() {
        let bytes = b"H 100644 not-an-oid 0\tf.txt\0";
        assert!(parse_ls_files_tagged(bytes).is_err());
    }

    /// The error's `Display` is the sentence `sync` reports, naming the
    /// malformed record itself, lossily decoded.
    #[test]
    fn a_malformed_record_displays_as_the_sentence_sync_shows() {
        let error = parse_ls_files_tagged(b"H 100644 not-an-oid 0\tf.txt\0")
            .expect_err("a malformed object id must be rejected");
        assert_eq!(
            error.to_string(),
            "git listed an index entry `skeletons` cannot read: H 100644 not-an-oid 0\tf.txt"
        );
    }

    proptest! {
        /// However `ls-files --stage -z` output is shaped, this parser
        /// never panics.
        #[test]
        fn never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..200)) {
            let _result = parse_ls_files_tagged(&bytes);
        }
    }
}
