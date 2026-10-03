//! Hand-made [`Workspace`] values for the tests of what `wear` makes of one:
//! a worn dependency with every field a plausible placeholder, and the
//! refusals a wearing table can come back as.

use std::path::PathBuf;

use crate::skeleton::Choices;
use crate::workspace::{Pin, Wearing, WearingRefusal, Workspace, WornDependency};

/// A worn dependency named `package`, declared under `key` in `manifest`.
pub(super) fn worn(manifest: &str, key: &str, package: &str) -> Wearing {
    let directory = PathBuf::from("/skeleton");
    Wearing::Worn(WornDependency {
        manifest: manifest.to_owned(),
        key: key.to_owned(),
        package: package.to_owned(),
        version: semver::Version::new(1, 2, 3),
        pin: Pin::from_source(None, &directory),
        skeleton_directory: directory,
        choices: Ok(Choices::new()),
    })
}

/// A wearing table that named a package that is no skeleton.
pub(super) fn not_a_skeleton(manifest: &str, key: &str, package: &str) -> Wearing {
    Wearing::Refused(WearingRefusal::NotASkeleton {
        manifest: manifest.to_owned(),
        dependency: key.to_owned(),
        package: (package.to_owned(), "0.1.0".to_owned()),
    })
}

/// A workspace rooted at `/workspace` holding `wearing`.
pub(super) fn workspace_of(wearing: Vec<Wearing>) -> Workspace {
    Workspace {
        root: PathBuf::from("/workspace"),
        wearing,
    }
}
