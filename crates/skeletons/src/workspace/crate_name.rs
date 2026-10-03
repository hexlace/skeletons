//! How a crate's name is compared with another's.

/// `text` with every `-` turned into `_`: the one rule by which Cargo and
/// `rustc` take two spellings of a name as one.
pub(crate) fn underscored(text: &str) -> String {
    text.replace('-', "_")
}

/// Whether `left` and `right` name the same crate, compared as Cargo compares
/// crate names.
///
/// crates.io holds `foo-bar` and `foo_bar` as one name, and `cargo add`
/// resolves either to the package's own spelling, so a package is the crate
/// a wearer asked for whichever of the two was typed. A dependency's key is
/// not a crate name and is not compared here: a key is matched as written
/// unless it is `rustc` that names it, and that is [`underscored`] alone.
pub(crate) fn same_crate(left: &str, right: &str) -> bool {
    underscored(left) == underscored(right)
}

#[cfg(test)]
mod tests {
    use super::{same_crate, underscored};

    #[test]
    fn a_hyphen_and_an_underscore_are_one_crate() {
        // Both spellings, both ways round, and a name with several of them.
        assert!(same_crate("foo-bar", "foo_bar"));
        assert!(same_crate("foo_bar", "foo-bar"));
        assert!(same_crate("a-b_c-d", "a_b-c_d"));
    }

    #[test]
    fn a_name_is_the_same_crate_as_itself() {
        assert!(same_crate("foo-bar", "foo-bar"));
        assert!(same_crate("foo_bar", "foo_bar"));
        assert!(same_crate("", ""));
    }

    #[test]
    fn names_that_differ_by_anything_else_are_two_crates() {
        assert!(!same_crate("foo-bar", "foobar"));
        assert!(!same_crate("foo-bar", "foo-baz"));
        assert!(!same_crate("foo-bar", "Foo-Bar"));
        assert!(!same_crate("foo-bar", "foo-bar-"));
    }

    #[test]
    fn the_underscored_form_turns_every_hyphen_into_an_underscore_and_nothing_else() {
        assert_eq!(underscored("a-b_c-d"), "a_b_c_d");
        assert_eq!(underscored("A-b"), "A_b");
        assert_eq!(underscored(""), "");
    }
}
