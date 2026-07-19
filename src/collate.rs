//! Locale-aware collation for the "Name" sort order (`crate::model::SortOrder::Name`).
//!
//! The bundled SQLite has no ICU collation to `ORDER BY`, so the alphabetical sort is done in Rust
//! with ICU4X — pure Rust with the CLDR tables compiled in, giving one code path on every target
//! (like the bundled SQLite) and ordering that is correct for the device locale rather than raw
//! code-point order. For example Swedish sorts "ä"/"ö" after "z", while English sorts them near
//! "a"/"o". A collator is built once per locale and cached on the calling thread (the list re-sorts
//! on every catalog change and keystroke, so rebuilding it each time would be wasteful).

use std::cell::RefCell;

use icu_collator::options::{CollatorOptions, Strength};
use icu_collator::{CollatorBorrowed, CollatorPreferences};
use icu_locale_core::Locale;

thread_local! {
    /// The collator for the last locale asked for, kept so repeated sorts in one locale reuse it.
    static COLLATOR: RefCell<Option<(String, CollatorBorrowed<'static>)>> =
        const { RefCell::new(None) };
}

/// Build a collator for a BCP-47 tag, falling back to the Unicode root order for an unknown tag or a
/// locale with no tailoring data (rather than failing the sort).
fn build(tag: &str) -> CollatorBorrowed<'static> {
    let locale: Locale = tag.parse().unwrap_or(Locale::UNKNOWN);
    let mut options = CollatorOptions::default();
    // Tertiary strength: case and accents still order otherwise-equal names, matching how a store
    // lists apps rather than collapsing "app"/"App" to one bucket.
    options.strength = Some(Strength::Tertiary);
    CollatorBorrowed::try_new(CollatorPreferences::from(&locale), options).unwrap_or_else(|_| {
        // The root collation data is compiled in (`compiled_data`), so this construction cannot
        // fail — it is the guaranteed fallback for any locale without its own tailoring.
        CollatorBorrowed::try_new(
            CollatorPreferences::from(&Locale::UNKNOWN),
            CollatorOptions::default(),
        )
        .expect("root collation data is compiled in")
    })
}

/// Sort `items` alphabetically by `key`, using `locale`'s collation.
pub fn sort_by_name<T>(locale: &str, items: &mut [T], key: impl Fn(&T) -> &str) {
    COLLATOR.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.as_ref().map(|(tag, _)| tag.as_str()) != Some(locale) {
            *slot = Some((locale.to_string(), build(locale)));
        }
        // Set just above; the collator is present here.
        let collator = &slot.as_ref().expect("collator set above").1;
        items.sort_by(|a, b| collator.compare(key(a), key(b)));
    });
}

/// Compare two names under `locale`'s collation (a one-off, without the thread-local cache).
#[cfg(test)]
pub fn compare(locale: &str, a: &str, b: &str) -> std::cmp::Ordering {
    build(locale).compare(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    #[test]
    fn collation_is_locale_specific() {
        // English orders the accented "ä" next to "a" (before "z"); Swedish orders it after "z".
        assert_eq!(compare("en-US", "äpple", "zoo"), Ordering::Less);
        assert_eq!(compare("sv", "äpple", "zoo"), Ordering::Greater);
    }

    #[test]
    fn unknown_locale_falls_back_to_root() {
        // A malformed tag still collates (via the root order) rather than panicking.
        assert_eq!(compare("not a locale!!", "a", "b"), Ordering::Less);
    }

    #[test]
    fn sort_by_name_orders_in_place() {
        let mut v = vec!["Zebra", "apple", "Apple"];
        sort_by_name("en-US", &mut v, |s| s);
        assert_eq!(v, vec!["apple", "Apple", "Zebra"]);
    }
}
