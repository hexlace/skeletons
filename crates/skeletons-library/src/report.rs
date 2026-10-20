//! Everything `check`, `check_without_cargo` or `check_without_cargo_configuration`
//! found in a workspace.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use skeletons_engine::{RefusalRecord, Survey, WornDependency};

use crate::claimed_file::ClaimedFile;
use crate::refusal::Refusal;
use crate::skeletons_named::SkeletonsNamed;
use crate::worn_skeleton::WornSkeleton;

/// What `check`, `check_without_cargo` or `check_without_cargo_configuration`
/// found in a workspace: each wearing that resolved to a skeleton, and every
/// refusal.
///
/// There are two complete ways to read a report. For one package, use
/// [`skeletons_named`](Self::skeletons_named). For the whole workspace, use
/// [`skeletons`](Self::skeletons) together with [`refusals`](Self::refusals):
/// every refusal is in `refusals()`, including those of wearings that resolved
/// to no skeleton, which no [`WornSkeleton`] carries. The workspace is as the
/// command line's `check` would pass it only when `refusals()` is empty and
/// every file of every skeleton is [`Matches`](crate::FileState::Matches).
///
/// Where `check_without_cargo` gives a report, it is the report `check` gives
/// on the same files, or, in the one stale shape its documentation describes,
/// the one `check` gives once Cargo brings the lock up to date, and so does
/// `check_without_cargo_configuration` for a workspace with no Cargo
/// configuration. So everything here reads the same whichever produced it,
/// except that only the two reads without cargo can report an
/// [`Unlocated`](crate::RefusalKind::Unlocated) refusal.
///
/// A report never changes after the call that made it returns it.
///
/// # Examples
///
/// ```no_run
/// let report = skeletons_library::check(".")?;
/// let all_clear = report.refusals().is_empty()
///     && report.skeletons().iter().all(|skeleton| match skeleton.files() {
///         skeletons_library::Files::Checked(files) => {
///             files.iter().all(|file| !file.state().is_drifted())
///         }
///         _ => false,
///     });
/// println!("{all_clear}");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Report {
    root: PathBuf,
    skeletons: Vec<WornSkeleton>,
    /// Refusals about a wearing that resolved to no skeleton come first, those
    /// whose wearing names no package before those that name one, so that
    /// [`SkeletonsNamed::Undetermined`] can borrow the first group as one slice.
    refusals: Vec<Refusal>,
    unnamed_count: usize,
    /// For each package an unidentified wearing names, the refusals that could
    /// be that package: the ones naming it, then every one naming no package.
    undetermined: BTreeMap<String, Vec<Refusal>>,
}

/// Where a refusal sits in [`Report::refusals`], in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Group {
    /// About a wearing that resolved to no skeleton, naming no package.
    UnidentifiedNamingNone,
    /// About a wearing that resolved to no skeleton, naming a package.
    UnidentifiedNamingOne,
    /// About worn skeletons.
    Identified,
}

impl Group {
    fn of(refusal: &Refusal) -> Self {
        if refusal.kind().concerns_unidentified_wearing() {
            if refusal.package_named().is_some() {
                Self::UnidentifiedNamingOne
            } else {
                Self::UnidentifiedNamingNone
            }
        } else {
            Self::Identified
        }
    }
}

impl Report {
    /// Joins the survey's flat rows and refusal records to the worn skeletons
    /// they belong to, once, so no caller ever does.
    pub(crate) fn from_survey(survey: &Survey<'_>) -> Self {
        let mut worn: Vec<&WornDependency> = survey.worn.clone();
        worn.sort_by(|left, right| {
            (&left.package, &left.manifest, &left.key).cmp(&(
                &right.package,
                &right.manifest,
                &right.key,
            ))
        });
        let mut skeletons: Vec<WornSkeleton> = worn
            .iter()
            .map(|worn| WornSkeleton::from_worn(worn))
            .collect();
        let index: BTreeMap<(&str, &str), usize> = worn
            .iter()
            .enumerate()
            .map(|(position, worn)| ((worn.manifest.as_str(), worn.key.as_str()), position))
            .collect();
        assert_eq!(
            index.len(),
            skeletons.len(),
            "a manifest and a dependency key name one worn dependency"
        );

        for row in &survey.rows {
            let key = (
                row.claimant.manifest.as_str(),
                row.claimant.dependency.as_str(),
            );
            let Some(&position) = index.get(&key) else {
                unreachable!("a file is claimed by a worn dependency, which the survey lists")
            };
            skeletons[position].push_file(ClaimedFile::from_row(row));
        }

        let records = survey.refusal_records();
        let mut refusals = Vec::with_capacity(records.len());
        for record in &records {
            let refusal = Refusal::from_record(record);
            hand_to_skeletons(&mut skeletons, &index, record, &refusal);
            refusals.push(refusal);
        }
        assert!(
            refusals
                .iter()
                .filter(|refusal| refusal.kind().concerns_unidentified_wearing())
                .all(|refusal| refusal.wearings().len() == 1),
            "a refusal about a wearing that resolved to no skeleton concerns exactly one wearing"
        );
        refusals.sort_by_key(Group::of);
        let unnamed_count = refusals
            .iter()
            .take_while(|refusal| Group::of(refusal) == Group::UnidentifiedNamingNone)
            .count();
        let unidentified_count = refusals
            .iter()
            .take_while(|refusal| refusal.kind().concerns_unidentified_wearing())
            .count();
        assert!(
            refusals[unidentified_count..]
                .iter()
                .all(|refusal| !refusal.kind().concerns_unidentified_wearing()),
            "the sort put every refusal about an unidentified wearing first"
        );
        let undetermined = undetermined_by_package(&refusals, unnamed_count, unidentified_count);

        Self {
            root: survey.root.clone(),
            skeletons,
            refusals,
            unnamed_count,
            undetermined,
        }
    }

    /// The workspace root: an absolute path, as cargo reports it for `check`
    /// and as the canonical directory holding the root manifest for
    /// `check_without_cargo` and `check_without_cargo_configuration`. Every
    /// other path in a report is relative to it.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Every wearing that resolved to a skeleton, refused or not, ordered by
    /// package name, then manifest, then dependency key.
    #[must_use]
    pub fn skeletons(&self) -> &[WornSkeleton] {
        &self.skeletons
    }

    /// The worn skeletons whose package name is `package`, compared exactly.
    ///
    /// The name is the skeleton's package name and never the dependency key,
    /// which belongs to the wearer. A wearing that was refused before its
    /// skeleton was identified makes the answer unsettled only when it names
    /// `package` or names no package at all. See [`SkeletonsNamed`] for what
    /// each answer does and does not say.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use skeletons_library::SkeletonsNamed;
    ///
    /// let report = skeletons_library::check(".")?;
    /// if let SkeletonsNamed::Worn(skeletons) = report.skeletons_named("house-rules") {
    ///     println!("{} wearings", skeletons.len());
    /// }
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn skeletons_named(&self, package: &str) -> SkeletonsNamed<'_> {
        let start = self
            .skeletons
            .partition_point(|skeleton| skeleton.package() < package);
        let end = self
            .skeletons
            .partition_point(|skeleton| skeleton.package() <= package);
        if start < end {
            return SkeletonsNamed::Worn(&self.skeletons[start..end]);
        }
        if let Some(refusals) = self.undetermined.get(package) {
            return SkeletonsNamed::Undetermined(refusals);
        }
        let unnamed = &self.refusals[..self.unnamed_count];
        if unnamed.is_empty() {
            SkeletonsNamed::NotWorn
        } else {
            SkeletonsNamed::Undetermined(unnamed)
        }
    }

    /// Every refusal in the workspace, in the same order on every run.
    ///
    /// Refusals about a wearing that resolved to no skeleton come first: those
    /// whose [`Wearing`](crate::Wearing) names no package, then those whose
    /// wearing names one, then the rest. A refusal that names worn skeletons is
    /// also reached through each one's [`files`](WornSkeleton::files).
    #[must_use]
    pub fn refusals(&self) -> &[Refusal] {
        &self.refusals
    }
}

/// For each package a refused unidentified wearing names, the refusals that
/// could be that package: those naming it, in report order, then clones of the
/// ones naming none.
///
/// `refusals` is sorted by [`Group`]: the first `unnamed_count` name no package
/// and the next ones, up to `unidentified_count`, each name one.
fn undetermined_by_package(
    refusals: &[Refusal],
    unnamed_count: usize,
    unidentified_count: usize,
) -> BTreeMap<String, Vec<Refusal>> {
    let mut undetermined: BTreeMap<String, Vec<Refusal>> = BTreeMap::new();
    for refusal in &refusals[unnamed_count..unidentified_count] {
        let Some(package) = refusal.package_named() else {
            unreachable!("the sort put every refusal naming no package before this range")
        };
        undetermined
            .entry(package.to_owned())
            .or_default()
            .push(refusal.clone());
    }
    for list in undetermined.values_mut() {
        list.extend(refusals[..unnamed_count].iter().cloned());
    }
    undetermined
}

/// Gives `refusal` to each worn skeleton it names, which only a refusal that
/// concerns an identified wearing does.
fn hand_to_skeletons(
    skeletons: &mut [WornSkeleton],
    index: &BTreeMap<(&str, &str), usize>,
    record: &RefusalRecord,
    refusal: &Refusal,
) {
    for wearing in &record.wearings {
        let position = wearing
            .dependency
            .as_deref()
            .and_then(|key| index.get(&(wearing.manifest.as_str(), key)).copied());
        match (refusal.kind().concerns_unidentified_wearing(), position) {
            (true, None) => {}
            (true, Some(_)) => {
                unreachable!("a wearing that resolved to no skeleton is not a worn dependency")
            }
            (false, Some(position)) => skeletons[position].push_refusal(refusal.clone()),
            (false, None) => {
                unreachable!("a refusal about a skeleton names only worn dependencies")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::fs;
    use std::hash::{DefaultHasher, Hash, Hasher};
    use std::path::Path;
    use std::process::Command;

    use tempfile::TempDir;

    use super::Report;
    use crate::files::Files;
    use crate::refusal::{Refusal, RefusalKind};
    use crate::skeletons_named::SkeletonsNamed;

    type TestOutcome = Result<(), Box<dyn Error>>;

    /// A member of the workspace a test writes: its name, the dependencies it
    /// declares as `(key, package)`, and the keys it gives a wearing table.
    struct Member<'a> {
        name: &'a str,
        dependencies: &'a [(&'a str, &'a str)],
        wearing_tables: &'a [&'a str],
    }

    fn hash_of(refusal: &Refusal) -> u64 {
        let mut hasher = DefaultHasher::new();
        refusal.hash(&mut hasher);
        hasher.finish()
    }

    fn write(path: &Path, contents: &str) -> Result<(), Box<dyn Error>> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, contents)?;
        Ok(())
    }

    /// Writes a crate `name` beside the project, with a library and, when
    /// `skeleton` is true, the metadata and one file that make it a skeleton.
    fn write_crate(base: &Path, name: &str, skeleton: bool) -> Result<(), Box<dyn Error>> {
        let directory = base.join("skeletons").join(name);
        let metadata = if skeleton {
            "\n[package.metadata.skeletons]\n"
        } else {
            ""
        };
        write(
            &directory.join("Cargo.toml"),
            &format!(
                "[package]\nname = \"{name}\"\nversion = \"1.0.0\"\nedition = \"2021\"\n\
                 publish = false\n{metadata}"
            ),
        )?;
        write(&directory.join("src/lib.rs"), "")?;
        if skeleton {
            write(&directory.join("files/notes/house.txt"), "house rules\n")?;
        }
        Ok(())
    }

    /// Writes a workspace of `members` beside two crates, `house-rules`, a
    /// skeleton, and `plain-one` and `plain-two`, which are not, makes its
    /// lockfile offline with a `CARGO_HOME` and `HOME` of the child's own, and
    /// reads it with [`crate::check`]. The directory is returned beside the
    /// report so that it outlives the read.
    fn checked(members: &[Member<'_>]) -> Result<(TempDir, Report), Box<dyn Error>> {
        let base = tempfile::Builder::new()
            .prefix("skeletons-library-report-")
            .tempdir()?;
        for (name, skeleton) in [
            ("house-rules", true),
            ("plain-one", false),
            ("plain-two", false),
        ] {
            write_crate(base.path(), name, skeleton)?;
        }
        let project = base.path().join("project");
        let names: Vec<String> = members
            .iter()
            .map(|member| format!("\"{}\"", member.name))
            .collect();
        write(
            &project.join("Cargo.toml"),
            &format!(
                "[workspace]\nmembers = [{}]\nresolver = \"3\"\n",
                names.join(", ")
            ),
        )?;
        for member in members {
            let dependencies: Vec<String> = member
                .dependencies
                .iter()
                .map(|(key, package)| {
                    let directory = base.path().join("skeletons").join(package);
                    format!(
                        "{key} = {{ package = \"{package}\", path = \"{}\" }}\n",
                        directory.display()
                    )
                })
                .collect();
            let tables: Vec<String> = member
                .wearing_tables
                .iter()
                .map(|key| format!("\n[package.metadata.skeletons.{key}]\n"))
                .collect();
            let manifest = format!(
                "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
                 publish = false\n\n[dependencies]\n{}{}",
                member.name,
                dependencies.concat(),
                tables.concat()
            );
            write(&project.join(member.name).join("Cargo.toml"), &manifest)?;
            write(&project.join(member.name).join("src/lib.rs"), "")?;
        }
        let output = Command::new(env!("CARGO"))
            .current_dir(&project)
            .env("CARGO_HOME", base.path().join("cargo-home"))
            .env("HOME", base.path().join("home"))
            .env_remove("CARGO_TARGET_DIR")
            .args(["generate-lockfile", "--offline"])
            .output()?;
        if !output.status.success() {
            return Err(format!(
                "cargo failed ({}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        let report = crate::check(&project)?;
        Ok((base, report))
    }

    #[test]
    fn an_overlap_is_one_value_in_every_skeleton_and_in_the_report() -> TestOutcome {
        // Given `house-rules` worn by two members, the one path it claims
        // collides with itself and is refused as one `Overlap` naming both
        // wearings. That refusal is handed to both worn skeletons and is also
        // in the report's own list. All three must be the same value, so that
        // handing a refusal on copies a pointer and never its wearings and
        // paths. Each skeleton's list is read through `files()`, as a caller
        // does.
        let (_base, report) = checked(&[
            Member {
                name: "tools",
                dependencies: &[("rules", "house-rules")],
                wearing_tables: &["rules"],
            },
            Member {
                name: "more-tools",
                dependencies: &[("rules", "house-rules")],
                wearing_tables: &["rules"],
            },
        ])?;

        let [overlap] = report.refusals() else {
            return Err(format!("expected one refusal, found {:?}", report.refusals()).into());
        };
        assert_eq!(overlap.kind(), RefusalKind::Overlap);
        assert_eq!(overlap.wearings().len(), 2, "{overlap:?}");
        assert_eq!(report.skeletons().len(), 2, "{:?}", report.skeletons());
        for skeleton in report.skeletons() {
            let Files::Refused { refusals, .. } = skeleton.files() else {
                return Err(format!("{} was not refused", skeleton.manifest().display()).into());
            };
            let [held] = refusals else {
                return Err(format!("expected one refusal, found {refusals:?}").into());
            };
            assert!(
                held.shares_data_with(overlap),
                "{} holds a copy of the report's refusal",
                skeleton.manifest().display()
            );
        }
        Ok(())
    }

    #[test]
    fn a_refusal_naming_no_package_is_one_value_in_every_packages_lookup() -> TestOutcome {
        // Given two packages that are not skeletons, each declared and given a
        // wearing table, and a third wearing table under a key no dependency
        // has, the report holds three refusals: two that name a package and
        // one, `NamesNoDependency`, that names none. Asking for either package
        // is `Undetermined` with its own refusal and then the one naming none.
        // That last refusal must be the very value in the report's list in
        // both lookups, and each package's own refusal must be the report's
        // too.
        let (_base, report) = checked(&[Member {
            name: "tools",
            dependencies: &[("one", "plain-one"), ("two", "plain-two")],
            wearing_tables: &["one", "two", "rulez"],
        }])?;

        let unnamed: Vec<&Refusal> = report
            .refusals()
            .iter()
            .filter(|refusal| refusal.kind() == RefusalKind::NamesNoDependency)
            .collect();
        let [unnamed] = unnamed.as_slice() else {
            return Err(format!("expected one unnamed refusal in {:?}", report.refusals()).into());
        };
        for package in ["plain-one", "plain-two"] {
            let SkeletonsNamed::Undetermined(refusals) = report.skeletons_named(package) else {
                return Err(format!("{package} was not Undetermined").into());
            };
            let [own, shared] = refusals else {
                return Err(format!("{package}: expected two refusals, found {refusals:?}").into());
            };
            assert_eq!(own.kind(), RefusalKind::NotASkeleton, "{package}");
            assert!(
                report
                    .refusals()
                    .iter()
                    .any(|refusal| refusal.shares_data_with(own)),
                "{package}: its own refusal is a copy of the report's"
            );
            assert!(
                shared.shares_data_with(unnamed),
                "{package}: the refusal naming no package is a copy of the report's"
            );
        }
        Ok(())
    }

    #[test]
    fn refusals_are_equal_by_value_and_not_by_the_data_they_share() -> TestOutcome {
        // Given the same workspace read twice, each report holds an `Overlap`
        // built from its own survey. A refusal equals its clone, which shares
        // its data, and equals the one in the other report, which does not:
        // equality and hashing follow the kind, wearings and paths.
        let members = [
            Member {
                name: "tools",
                dependencies: &[("rules", "house-rules")],
                wearing_tables: &["rules"],
            },
            Member {
                name: "more-tools",
                dependencies: &[("rules", "house-rules")],
                wearing_tables: &["rules"],
            },
        ];
        let (_first_base, first) = checked(&members)?;
        let (_second_base, second) = checked(&members)?;
        let (Some(original), Some(other)) = (first.refusals().first(), second.refusals().first())
        else {
            return Err("a report held no refusal".into());
        };

        let copy = original.clone();

        assert!(copy.shares_data_with(original));
        assert_eq!(&copy, original);
        assert_eq!(hash_of(&copy), hash_of(original));
        assert!(!other.shares_data_with(original));
        assert_eq!(other, original);
        assert_eq!(hash_of(other), hash_of(original));
        Ok(())
    }
}
