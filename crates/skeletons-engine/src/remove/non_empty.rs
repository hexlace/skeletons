//! A list that has a first element by construction.
//!
//! Some of `remove`'s refusals carry a list that is never empty: the reasons
//! files cannot be known, the keys one crate is worn under, and the manifests
//! a name is worn in elsewhere. What each message says is wrong for an empty
//! list. Holding the first element apart from the rest makes the empty list
//! impossible to build, so no message asserts that it is not.

/// A first element, and the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NonEmpty<T> {
    first: T,
    rest: Vec<T>,
}

impl<T> NonEmpty<T> {
    /// `first` followed by `rest`.
    pub(crate) const fn new(first: T, rest: Vec<T>) -> Self {
        Self { first, rest }
    }

    /// `items` as a list that has a first element, or `None` when there are
    /// no items.
    pub(crate) fn from_vec(items: Vec<T>) -> Option<Self> {
        let mut items = items.into_iter();
        let first = items.next()?;
        Some(Self::new(first, items.collect()))
    }

    /// The first element.
    pub(crate) const fn first(&self) -> &T {
        &self.first
    }

    /// The only element, or `None` when there are more.
    pub(crate) fn only(&self) -> Option<&T> {
        if self.rest.is_empty() {
            Some(&self.first)
        } else {
            None
        }
    }

    /// Every element, the first first.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &T> {
        std::iter::once(&self.first).chain(&self.rest)
    }
}

#[cfg(test)]
mod tests {
    use super::NonEmpty;

    #[test]
    fn there_is_no_list_without_a_first_element() {
        assert_eq!(NonEmpty::<u8>::from_vec(Vec::new()), None);
    }

    #[test]
    fn a_list_keeps_its_elements_in_the_order_they_came_in() {
        let list = NonEmpty::from_vec(vec![3, 1, 2]).expect("three items have a first");

        assert_eq!(list.first(), &3);
        assert_eq!(list.iter().copied().collect::<Vec<_>>(), [3, 1, 2]);
    }

    #[test]
    fn a_list_of_one_has_only_its_first() {
        let list = NonEmpty::new('a', Vec::new());

        assert_eq!(list.iter().count(), 1);
        assert_eq!(list, NonEmpty::from_vec(vec!['a']).expect("one item"));
    }
}
