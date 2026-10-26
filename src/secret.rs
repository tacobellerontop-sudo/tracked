//! A newtype for bearer credentials that refuses to print itself.

use std::fmt;

/// Wraps a credential (lockfile password, RSO token, entitlements JWT).
///
/// `Debug` and `Display` both print `***`, so a secret can be carried inside
/// structs that derive `Debug` and logged structs never leak it. There is
/// deliberately no `Serialize` impl, so it cannot be written to disk by
/// accident either.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret<T>(T);

impl<T> Secret<T> {
    pub fn new(value: T) -> Self {
        Self(value)
    }

    /// Access the raw value. Call sites should be easy to grep for.
    pub fn expose(&self) -> &T {
        &self.0
    }
}

impl<T> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

impl<T> fmt::Display for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

impl From<String> for Secret<String> {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl<'de> serde::Deserialize<'de> for Secret<String> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d).map(Secret::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_prints_value() {
        let s = Secret::new("hunter2".to_string());
        assert_eq!(format!("{s}"), "***");
        assert_eq!(format!("{s:?}"), "***");
        assert_eq!(s.expose(), "hunter2");
    }
}
