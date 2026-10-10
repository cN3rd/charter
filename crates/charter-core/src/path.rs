use std::borrow::Borrow;
use std::fmt;

use crate::error::InvalidValue;
use crate::ident::string_newtype;

/// A single file name to read or write beside another file.
///
/// Empty names, `.` and `..`, separators, NUL and `:` are refused.
///
/// ```
/// # use charter_core::FileName;
/// let file: FileName = FileName::new("info.plist").unwrap();
/// assert_eq!(file.as_str(), "info.plist");
/// ```
///
/// A chart resource path is not a file name:
///
/// ```compile_fail
/// # use charter_core::{FileName, ResourcePath};
/// let file: FileName = ResourcePath::new("info.plist").unwrap();
/// ```
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileName(String);

impl FileName {
    /// Creates a file name from one plain path component.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidValue`] if `value` is not a plain file name.
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidValue> {
        let value = value.into();
        if value.is_empty()
            || matches!(value.as_str(), "." | "..")
            || value.contains(['/', '\\', '\0', ':'])
        {
            Err(InvalidValue::new(
                "a single plain file name",
                format_args!("{value:?}"),
            ))
        } else {
            Ok(Self(value))
        }
    }

    /// The validated file name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FileName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Borrow<str> for FileName {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

string_newtype!(
    /// A file path relative to the chart's directory, safe to join onto it.
    ///
    /// `/` is the only separator, and every component is non-empty, is neither `.` nor `..`,
    /// and contains no `\`, `:` or NUL. That excludes absolute paths, drive letters, UNC
    /// prefixes and traversal. Whoever opens the file must still refuse a link that leaves the
    /// chart's directory, since links can change after parsing.
    ResourcePath,
    "a relative path of plain /-separated components",
    |v| v.split('/').all(|c| {
        !c.is_empty() && c != "." && c != ".." && !c.contains(['\\', ':', '\0'])
    })
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_relative_paths_pass() {
        ResourcePath::new("keysounds/clap.wav").unwrap();
        for bad in [
            "",
            "/etc/passwd",
            "a//b",
            "./a",
            "a/../b",
            "..",
            "C:/x",
            "a\\b",
            "\\\\srv\\x",
            "a\0",
            "a/",
        ] {
            assert!(ResourcePath::new(bad).is_err(), "{bad:?} accepted");
        }
    }

    #[test]
    fn file_names_are_single_plain_components() {
        for good in ["info.plist", ".hidden", "..notes", "a b", "caf\u{e9}.wav"] {
            let name = FileName::new(good).unwrap();
            assert_eq!(name.as_str(), good);
            assert_eq!(name.to_string(), good);
        }
        for bad in [
            "",
            ".",
            "..",
            "../escape.txt",
            "/etc/passwd",
            "a/b",
            "a\\b",
            "\\\\server\\share",
            "C:\\file",
            "a:b",
            "a\0b",
        ] {
            assert!(FileName::new(bad).is_err(), "{bad:?} accepted");
        }
    }
}
