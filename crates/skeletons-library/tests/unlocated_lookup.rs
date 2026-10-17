//! What `skeletons_named` answers beside a wearing whose package was given no
//! location: the same as beside a wearing that resolved to no locked package,
//! because in both the package asked for may be the one the wearing names.

#[path = "support/lock.rs"]
mod lock;
mod support;

use skeletons_library::{PackageLocations, RefusalKind, SkeletonsNamed, check_without_cargo};
use support::{TestOutcome, undetermined};

#[test]
fn an_unlocated_wearing_leaves_the_lookup_of_its_package_undetermined() -> TestOutcome {
    // Given a git wearing of `house-rules` and no location for it, the one
    // refusal is `Unlocated` and names `house-rules`. Asking for `house-rules`
    // is `Undetermined` with that refusal, as it is for a wearing that
    // resolved to no locked package. Asking for any other package is
    // `NotWorn`, since the refusal's wearing names `house-rules` and cannot be
    // them.
    let fixture = lock::wearing_from_git()?;

    let report = check_without_cargo(fixture.project(), &PackageLocations::new())?;

    let refusals = undetermined(&report, "house-rules")?;
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert_eq!(refusals[0].kind(), RefusalKind::Unlocated);
    assert_eq!(report.refusals(), refusals);
    for other in ["rules", "house_rules", "plain-crate"] {
        assert!(
            matches!(report.skeletons_named(other), SkeletonsNamed::NotWorn),
            "{other}: {:?}",
            report.skeletons_named(other)
        );
    }
    Ok(())
}
