//! [`Keyed`] and [`Key`]: how one validated part of a skeleton refers to
//! another — a placeholder to its `enum` or `text` option, a directive to its
//! `set` option, a `set` value to its partial — without a name to look up again
//! and without a way to name something that is not there.
//!
//! Validation proves each of those references once, when it finds the thing
//! referred to; the key it keeps is the proof. Everything downstream — sizing,
//! resolving a wearer's choices, assembly — follows keys, and never asks by
//! name a question validation already answered.

use std::fmt;
use std::marker::PhantomData;
use std::ops::{Index, IndexMut};

/// The position of one `K` in the [`Keyed`] list that handed it out.
///
/// Keys are minted only for positions that exist: by
/// [`KeyedBuilder::push`], for the item just added, and by [`Keyed::iter`],
/// for the items listed. Every other list over the same `K` is made from
/// that one by [`Keyed::map`] or [`Keyed::try_map`], position for position,
/// and a finished list never grows, so a key is in range for every list it
/// can be used with. `K` names what is being counted and is never stored.
pub(crate) struct Key<K> {
    position: usize,
    counts: PhantomData<fn() -> K>,
}

// Written by hand rather than derived: a derive would require `K` itself to
// be `Clone`, `Copy`, `PartialEq` and `Debug`, but a key never holds a `K`,
// only a position among them.
impl<K> Clone for Key<K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K> Copy for Key<K> {}

impl<K> PartialEq for Key<K> {
    fn eq(&self, other: &Self) -> bool {
        self.position == other.position
    }
}

impl<K> Eq for Key<K> {}

impl<K> fmt::Debug for Key<K> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Key({})", self.position)
    }
}

/// A [`Keyed`] list while it is being built: the one thing that can grow
/// one, and so the only place a list's length can change.
pub(crate) struct KeyedBuilder<K, T> {
    items: Vec<T>,
    counts: PhantomData<fn() -> K>,
}

impl<K, T> KeyedBuilder<K, T> {
    pub(crate) const fn new() -> Self {
        Self {
            items: Vec::new(),
            counts: PhantomData,
        }
    }

    /// Adds `item` at the end of the list and returns the key it now has.
    pub(crate) fn push(&mut self, item: T) -> Key<K> {
        let key = Key {
            position: self.items.len(),
            counts: PhantomData,
        };
        self.items.push(item);
        // Postcondition: the new key addresses exactly the item just pushed.
        assert_eq!(
            key.position + 1,
            self.items.len(),
            "a pushed item's key is the last position in the list"
        );
        key
    }

    /// The finished list, which never grows again.
    pub(crate) fn finish(self) -> Keyed<K, T> {
        Keyed {
            items: self.items,
            counts: PhantomData,
        }
    }
}

impl<K, T> Default for KeyedBuilder<K, T> {
    fn default() -> Self {
        Self::new()
    }
}

/// One `T` for each `K`, in a fixed order, addressed by [`Key<K>`].
///
/// Built once, by a [`KeyedBuilder`], and never grown: an item can be
/// replaced ([`IndexMut`]) but a finished list has no way to add or remove
/// one. Two lists over the same `K` from two different skeletons are not told
/// apart by the type; nothing ever holds two, since every list lives inside
/// one call to [`super::render`].
pub(crate) struct Keyed<K, T> {
    items: Vec<T>,
    counts: PhantomData<fn() -> K>,
}

impl<K, T> Keyed<K, T> {
    /// How many items the list holds.
    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }

    /// Every item with its key, in the list's own order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (Key<K>, &T)> {
        self.items.iter().enumerate().map(|(position, item)| {
            (
                Key {
                    position,
                    counts: PhantomData,
                },
                item,
            )
        })
    }

    /// A list of the same length and keys, one `U` for each item, made by
    /// `derive`.
    pub(crate) fn map<U>(&self, mut derive: impl FnMut(Key<K>, &T) -> U) -> Keyed<K, U> {
        let items: Vec<U> = self.iter().map(|(key, item)| derive(key, item)).collect();
        // Postcondition: a mapped list has a key for exactly every key this
        // one has, and no other.
        assert_eq!(
            items.len(),
            self.items.len(),
            "a mapped list keeps every position of the list it was made from"
        );
        Keyed {
            items,
            counts: PhantomData,
        }
    }

    /// As [`Self::map`], stopping at the first item `derive` refuses, in the
    /// list's own order.
    pub(crate) fn try_map<U, E>(
        &self,
        mut derive: impl FnMut(Key<K>, &T) -> Result<U, E>,
    ) -> Result<Keyed<K, U>, E> {
        let items: Vec<U> = self
            .iter()
            .map(|(key, item)| derive(key, item))
            .collect::<Result<_, E>>()?;
        // Postcondition: as for `map`.
        assert_eq!(
            items.len(),
            self.items.len(),
            "a mapped list keeps every position of the list it was made from"
        );
        Ok(Keyed {
            items,
            counts: PhantomData,
        })
    }
}

impl<K, T: fmt::Debug> fmt::Debug for Keyed<K, T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(&self.items).finish()
    }
}

impl<K, T> Index<Key<K>> for Keyed<K, T> {
    type Output = T;

    /// The item at `key`.
    ///
    /// # Panics
    ///
    /// If `key` is out of range, which a key cannot be for any list it can
    /// reach: it was minted by the list that sized every list over `K`
    /// (see [`Key`]). This is the one place a key is turned back into a
    /// position, so it is the one place that invariant is asserted — by the
    /// slice's own bounds check — rather than at every reference.
    fn index(&self, key: Key<K>) -> &T {
        &self.items[key.position]
    }
}

impl<K, T> IndexMut<Key<K>> for Keyed<K, T> {
    /// The item at `key`, to replace it.
    ///
    /// # Panics
    ///
    /// As for [`Index::index`].
    fn index_mut(&mut self, key: Key<K>) -> &mut T {
        &mut self.items[key.position]
    }
}

#[cfg(test)]
mod tests {
    use super::KeyedBuilder;

    /// A marker for what the lists below count.
    struct Letter;

    #[test]
    fn a_pushed_items_key_addresses_that_item() {
        let mut builder: KeyedBuilder<Letter, char> = KeyedBuilder::new();
        let a = builder.push('a');
        let b = builder.push('b');
        let letters = builder.finish();
        assert_eq!(letters[a], 'a');
        assert_eq!(letters[b], 'b');
        assert_ne!(a, b);
    }

    #[test]
    fn a_mapped_list_is_addressed_by_the_originals_keys() {
        let mut builder: KeyedBuilder<Letter, char> = KeyedBuilder::new();
        let keys = ['x', 'y', 'z'].map(|letter| builder.push(letter));
        let letters = builder.finish();
        let upper = letters.map(|_key, letter| letter.to_ascii_uppercase());
        assert_eq!(upper.len(), letters.len());
        for (key, expected) in keys.into_iter().zip(['X', 'Y', 'Z']) {
            assert_eq!(upper[key], expected);
        }
    }

    #[test]
    fn try_map_stops_at_the_first_refusal_in_list_order() {
        let mut builder: KeyedBuilder<Letter, char> = KeyedBuilder::new();
        for letter in ['a', '1', '2'] {
            builder.push(letter);
        }
        let letters = builder.finish();
        let refused = letters.try_map(|_key, letter| {
            if letter.is_ascii_digit() {
                Err(*letter)
            } else {
                Ok(*letter)
            }
        });
        assert_eq!(refused.err(), Some('1'));
    }
}
