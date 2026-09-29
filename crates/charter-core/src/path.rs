use crate::ident::string_newtype;

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
}
