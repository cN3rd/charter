//! Record the source revision at build time for the About and welcome screens.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() -> io::Result<()> {
    emit_commit(
        std::env::var_os("CHARTER_COMMIT").is_some(),
        || read_revision(git, Path::try_exists),
        &mut io::stdout().lock(),
    )
}

/// Emit Cargo's build metadata without replacing an explicit revision.
fn emit_commit(
    overridden: bool,
    read: impl FnOnce() -> io::Result<(String, Vec<PathBuf>)>,
    output: &mut impl Write,
) -> io::Result<()> {
    writeln!(output, "cargo::rerun-if-changed=build.rs")?;
    writeln!(output, "cargo::rerun-if-env-changed=CHARTER_COMMIT")?;
    if overridden {
        return Ok(());
    }
    let (commit, paths) = match read() {
        Ok(revision) => revision,
        Err(error) => {
            writeln!(
                output,
                "cargo::warning=could not determine the Git commit: {error}"
            )?;
            ("unknown".to_owned(), Vec::new())
        }
    };
    for path in paths {
        writeln!(output, "cargo::rerun-if-changed={}", path.display())?;
    }
    writeln!(output, "cargo::rustc-env=CHARTER_COMMIT={commit}")
}

/// Read the source revision and the existing Git files that can change it.
fn read_revision(
    mut query: impl FnMut(&[&str]) -> io::Result<String>,
    exists: impl Fn(&Path) -> io::Result<bool>,
) -> io::Result<(String, Vec<PathBuf>)> {
    let mut paths = Vec::new();
    // ponytail: watch refs together; track only the current ref if unrelated ref updates
    // start causing costly rebuilds.
    let directories = query(&["rev-parse", "--git-dir", "--git-common-dir"])?;
    for directory in directories.lines() {
        for file in ["HEAD", "refs", "packed-refs", "reftable"] {
            let path = Path::new(directory).join(file);
            if exists(&path)? && !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    let commit = query(&["rev-parse", "--short=12", "HEAD"])?;
    Ok((commit, paths))
}

/// Run a read-only Git query, retaining diagnostics when it fails.
fn git(args: &[&str]) -> io::Result<String> {
    let output = Command::new("git").args(args).output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let output = String::from_utf8(output.stdout)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(output.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_checkout_embeds_its_commit_and_tracks_revision_changes() {
        let mut output = Vec::new();
        emit_commit(
            false,
            || {
                Ok((
                    "1a2b3c4d5e6f".to_owned(),
                    vec![
                        PathBuf::from("/checkout/.git/HEAD"),
                        PathBuf::from("/checkout/.git/refs"),
                        PathBuf::from("/checkout/.git/packed-refs"),
                    ],
                ))
            },
            &mut output,
        )
        .unwrap();
        let output = String::from_utf8(output).unwrap();
        for directive in [
            "cargo::rustc-env=CHARTER_COMMIT=1a2b3c4d5e6f",
            "cargo::rerun-if-changed=/checkout/.git/HEAD",
            "cargo::rerun-if-changed=/checkout/.git/refs",
            "cargo::rerun-if-changed=/checkout/.git/packed-refs",
        ] {
            assert!(output.contains(directive), "{directive}");
        }
    }

    #[test]
    fn an_explicit_commit_never_consults_git_or_gets_overwritten() {
        let mut output = Vec::new();
        emit_commit(
            true,
            || panic!("an explicit revision must not consult Git"),
            &mut output,
        )
        .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("cargo::rerun-if-env-changed=CHARTER_COMMIT"));
        assert!(!output.contains("cargo::rustc-env=CHARTER_COMMIT="));
    }

    #[test]
    fn unavailable_git_warns_and_marks_the_commit_unknown() {
        let mut output = Vec::new();
        emit_commit(false, || Err(io::ErrorKind::NotFound.into()), &mut output).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("cargo::warning="));
        assert!(output.contains("cargo::rustc-env=CHARTER_COMMIT=unknown"));
    }

    #[test]
    fn linked_reftable_worktrees_track_both_reference_stores() {
        let existing = [
            PathBuf::from("/checkout/.git/worktrees/editor/HEAD"),
            PathBuf::from("/checkout/.git/worktrees/editor/reftable"),
            PathBuf::from("/checkout/.git/HEAD"),
            PathBuf::from("/checkout/.git/refs"),
            PathBuf::from("/checkout/.git/reftable"),
        ];
        let (commit, paths) = read_revision(
            |args| {
                let output = match args {
                    ["rev-parse", "--git-dir", "--git-common-dir"] => {
                        "/checkout/.git/worktrees/editor\n/checkout/.git"
                    }
                    ["rev-parse", "--short=12", "HEAD"] => "1a2b3c4d5e6f",
                    _ => panic!("unexpected Git query: {args:?}"),
                };
                Ok(output.to_owned())
            },
            |path| Ok(existing.iter().any(|entry| entry.as_path() == path)),
        )
        .unwrap();
        assert_eq!(commit, "1a2b3c4d5e6f");
        for path in [
            PathBuf::from("/checkout/.git/worktrees/editor/reftable"),
            PathBuf::from("/checkout/.git/reftable"),
        ] {
            assert!(paths.contains(&path), "{} is not tracked", path.display());
        }
    }
}
