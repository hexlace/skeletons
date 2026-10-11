//! Asking crates.io's sparse index whether a registry-pinned skeleton is
//! behind — or, in a test build only, reading a captured answer instead, so
//! a test build can never reach the real network.

#[cfg(any(test, feature = "test-util"))]
use std::path::PathBuf;
use std::time::Duration;

/// The host every registry-pinned `behind` query is asked of, named in every
/// message this module's failures produce.
pub(crate) const CRATES_IO_HOST: &str = "index.crates.io";

/// The most this crate ever reads from one sparse-index response — a real
/// crate's index file is a handful of kilobytes per version; this is large
/// enough for any real crate's whole history and small enough that a
/// misbehaving or hostile server cannot exhaust memory: [`fetch_network`]'s
/// own `.limit(INDEX_BYTES_MAX)` call wraps the response body in `ureq`'s
/// own limiting reader, which errors once this many bytes have been read
/// rather than allocating past it, whatever the server keeps sending.
pub(crate) const INDEX_BYTES_MAX: u64 = 16 * 1024 * 1024;

/// The most a single sparse-index request is allowed to take end to end, from
/// DNS lookup to the last byte of the body. An index file is small and
/// CDN-served, so a healthy response arrives in well under this; the bound
/// exists so an unreachable or hanging network cannot hold a `check` run — or
/// a CI job running it — open indefinitely. Exceeding it reads
/// `undetermined`, never `current`: [`fetch_network`] maps a `ureq` call that
/// fails before the response arrives — a timeout included — to
/// [`IndexFetchFailure::Unreachable`] and a timeout while the body is read
/// to [`IndexFetchFailure::UnexpectedResponse`], and `finalize_crates_io` in
/// `behind.rs` matches every [`IndexFetchFailure`] variant exhaustively,
/// sending each to `Behind::Undetermined`; there is no arm that reads
/// either as `Behind::Current` (tested end to end by
/// `ritual/tests/check_behind_registry.rs` →
/// `network_unreachable_reads_undetermined_and_does_not_change_the_default_exit_status`,
/// which drives an `Unreachable` answer through this exact path and checks
/// both the reported state and the exit code).
const REMOTE_TIMEOUT: Duration = Duration::from_secs(30);
/// The most that establishing the connection itself, before any response
/// bytes arrive, is allowed to take. Shorter than [`REMOTE_TIMEOUT`] for the
/// same reason a CDN's own TCP handshake is expected to be fast even when
/// the response body is not: a connection that cannot even open within this
/// bound is failing in a way no amount of extra waiting fixes, so this ends
/// it sooner than the whole-request bound would.
const REMOTE_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const CRATES_IO_INDEX_URL: &str = "https://index.crates.io/";

/// Why a sparse-index request did not answer with a body to parse.
#[derive(Debug)]
pub(crate) enum IndexFetchFailure {
    /// The request could not be completed at all: DNS, connect, TLS, or a
    /// timeout.
    Unreachable { detail: String },
    /// The crate does not exist in this index at all (`404`, `410`, `451`).
    NotInIndex,
    /// A response came back, but not one this crate can read: any other
    /// HTTP status, or a body that was too large or unreadable.
    UnexpectedResponse { detail: String },
}

/// Where `behind` reads a registry's sparse-index answers from: the real
/// network, or, in a test build only, a captured file or a deliberately
/// empty seam.
///
/// `#[cfg(any(test, feature = "test-util"))]` on the two test variants keeps
/// them, and the environment variable that selects them, out of every
/// production build: they exist when this crate's own unit tests are built
/// and when a dependent enables `test-util` as a dev-dependency feature, and
/// in no other build.
pub(crate) enum CratesIoIndex {
    /// The real sparse index, over HTTPS.
    Network(ureq::Agent),
    /// A test build with `SKELETONS_TEST_ONLY_CRATES_IO_INDEX` set: read
    /// `<root>/<index_path(name)>` instead of making a request. A missing
    /// file (the variable points somewhere without a capture for this
    /// crate) answers [`IndexFetchFailure::Unreachable`], the same as a real
    /// network failure would.
    #[cfg(any(test, feature = "test-util"))]
    Captured(PathBuf),
    /// A test build with `SKELETONS_TEST_ONLY_CRATES_IO_INDEX` unset: every
    /// query answers [`IndexFetchFailure::Unreachable`] without looking at
    /// anything, so a test that forgets to set the variable fails loudly
    /// rather than silently reaching the real network.
    #[cfg(any(test, feature = "test-util"))]
    Withheld,
}

impl CratesIoIndex {
    /// Builds the seam this process uses for the rest of its run: the real
    /// network in a production build, or, in a test build, whatever
    /// `SKELETONS_TEST_ONLY_CRATES_IO_INDEX` says.
    ///
    /// In a test build, `from_test_environment` (only compiled under
    /// `test`/`test-util`) always returns `Some`, so [`network_agent`] below is
    /// never actually reached there — but it stays unconditional so a test
    /// build's `Network` match arm in [`Self::fetch`] is never dead code to
    /// the compiler, only unreachable at runtime.
    pub(crate) fn from_environment() -> Self {
        #[cfg(any(test, feature = "test-util"))]
        if let Some(seam) = Self::from_test_environment() {
            return seam;
        }
        Self::Network(network_agent())
    }

    // `Option`, though every branch is `Some`: `from_environment` above
    // relies on this being genuinely a `match` on an `Option` (not a plain
    // `Self`-returning call) so the compiler cannot prove `network_agent()`
    // unreachable and warn on it as dead code in a test build, where it
    // never actually runs.
    #[cfg(any(test, feature = "test-util"))]
    #[expect(
        clippy::unnecessary_wraps,
        reason = "the Option is load-bearing for from_environment's own dead-code shape, not \
                  for this function's own logic"
    )]
    fn from_test_environment() -> Option<Self> {
        let seam = std::env::var_os("SKELETONS_TEST_ONLY_CRATES_IO_INDEX").map_or_else(
            || Self::Withheld,
            |root| Self::Captured(PathBuf::from(root)),
        );
        Some(seam)
    }

    /// Asks whether `name`'s sparse-index file exists and, if so, returns
    /// its body verbatim.
    pub(crate) fn fetch(&self, name: &str) -> Result<Vec<u8>, IndexFetchFailure> {
        match self {
            Self::Network(agent) => fetch_network(agent, name),
            #[cfg(any(test, feature = "test-util"))]
            Self::Captured(root) => fetch_captured(root, name),
            #[cfg(any(test, feature = "test-util"))]
            Self::Withheld => Err(IndexFetchFailure::Unreachable {
                detail: "no captured crates.io index was given to this test build".to_owned(),
            }),
        }
    }
}

/// The sparse-index path layout
/// (<https://doc.rust-lang.org/cargo/reference/registry-index.html#index-files>),
/// lowercased: one character gives `1/<n>`, two give `2/<n>`, three give
/// `3/<first>/<n>`, and longer names give `<c1c2>/<c3c4>/<n>`.
pub(crate) fn index_path(name: &str) -> String {
    let lower = name.to_lowercase();
    match lower.chars().count() {
        1 => format!("1/{lower}"),
        2 => format!("2/{lower}"),
        3 => {
            let Some(first) = lower.chars().next() else {
                unreachable!("a length of 3 characters guarantees a first character")
            };
            format!("3/{first}/{lower}")
        }
        _ => {
            let mut characters = lower.chars();
            let first_two: String = characters.by_ref().take(2).collect();
            let next_two: String = characters.take(2).collect();
            format!("{first_two}/{next_two}/{lower}")
        }
    }
}

fn network_agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        // Every status this crate cares about (`200`, `404`, `410`, `451`,
        // anything else) is read from the response directly; `ureq`
        // otherwise turns a 4xx/5xx into `Err` before this code ever sees
        // the status it was given.
        .http_status_as_error(false)
        .timeout_global(Some(REMOTE_TIMEOUT))
        .timeout_connect(Some(REMOTE_CONNECT_TIMEOUT))
        .proxy(ureq::Proxy::try_from_env())
        .user_agent(format!(
            "skeletons/{} (+https://github.com/hexlace/skeletons)",
            env!("CARGO_PKG_VERSION")
        ))
        .build();
    ureq::Agent::new_with_config(config)
}

fn fetch_network(agent: &ureq::Agent, name: &str) -> Result<Vec<u8>, IndexFetchFailure> {
    let url = format!("{CRATES_IO_INDEX_URL}{}", index_path(name));
    let mut response = agent
        .get(&url)
        .call()
        .map_err(|error| IndexFetchFailure::Unreachable {
            detail: error.to_string(),
        })?;

    let status = response.status();
    if status.is_success() {
        response
            .body_mut()
            .with_config()
            .limit(INDEX_BYTES_MAX)
            .read_to_vec()
            .map_err(|error| IndexFetchFailure::UnexpectedResponse {
                detail: format!("reading the response body failed: {error}"),
            })
    } else if matches!(status.as_u16(), 404 | 410 | 451) {
        Err(IndexFetchFailure::NotInIndex)
    } else {
        Err(IndexFetchFailure::UnexpectedResponse {
            detail: format!("HTTP {status}"),
        })
    }
}

#[cfg(any(test, feature = "test-util"))]
fn fetch_captured(root: &std::path::Path, name: &str) -> Result<Vec<u8>, IndexFetchFailure> {
    let path = root.join(index_path(name));
    match std::fs::read(&path) {
        Ok(bytes) => Ok(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(IndexFetchFailure::Unreachable {
                detail: "no captured crates.io index exists for this crate".to_owned(),
            })
        }
        Err(error) => Err(IndexFetchFailure::Unreachable {
            detail: error.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{CratesIoIndex, IndexFetchFailure, index_path};

    #[test]
    fn a_one_character_name_uses_the_1_layout() {
        assert_eq!(index_path("a"), "1/a");
    }

    #[test]
    fn a_two_character_name_uses_the_2_layout() {
        assert_eq!(index_path("ab"), "2/ab");
    }

    #[test]
    fn a_three_character_name_uses_the_3_layout() {
        assert_eq!(index_path("url"), "3/u/url");
    }

    #[test]
    fn a_longer_name_splits_its_first_four_characters_in_half() {
        assert_eq!(index_path("semver"), "se/mv/semver");
        assert_eq!(index_path("rustls"), "ru/st/rustls");
    }

    #[test]
    fn the_name_is_lowercased_throughout() {
        assert_eq!(index_path("SeMVeR"), "se/mv/semver");
    }

    #[test]
    fn withheld_answers_unreachable_without_reading_anything() {
        let index = CratesIoIndex::Withheld;
        let error = index.fetch("semver").expect_err("withheld never answers");
        assert!(matches!(error, IndexFetchFailure::Unreachable { .. }));
    }

    #[test]
    fn captured_reads_the_bytes_at_the_expected_path() {
        let directory = tempfile::tempdir().expect("scratch directory");
        std::fs::create_dir_all(directory.path().join("se/mv")).expect("create index directory");
        std::fs::write(directory.path().join("se/mv/semver"), b"captured body")
            .expect("write capture");

        let index = CratesIoIndex::Captured(directory.path().to_owned());
        let body = index.fetch("semver").expect("a captured file must be read");
        assert_eq!(body, b"captured body");
    }

    #[test]
    fn captured_with_no_file_for_the_crate_is_unreachable() {
        let directory = tempfile::tempdir().expect("scratch directory");
        let index = CratesIoIndex::Captured(directory.path().to_owned());
        let error = index
            .fetch("semver")
            .expect_err("a capture root with nothing for this crate must not fabricate a body");
        assert!(matches!(error, IndexFetchFailure::Unreachable { .. }));
    }
}
