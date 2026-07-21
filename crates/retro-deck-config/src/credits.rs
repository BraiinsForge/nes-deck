//! Typed, bounded parsing for dashboard software credits.

use std::fmt;

/// Maximum accepted size of one credits document.
pub const MAXIMUM_CREDITS_BYTES: usize = 32 * 1_024;
/// Maximum number of attributed projects shown by the dashboard.
pub const MAXIMUM_CREDITS: usize = 64;
const MAXIMUM_PROJECT_BYTES: usize = 48;
const MAXIMUM_ROLE_BYTES: usize = 64;
const MAXIMUM_LICENSE_BYTES: usize = 64;

/// One validated project, role, and license attribution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectCredit {
    project: String,
    role: String,
    license: String,
}

impl ProjectCredit {
    /// Project name as displayed in the credits.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "String slicing is not const on the pinned Rust toolchain"
    )]
    pub fn project(&self) -> &str {
        &self.project
    }

    /// Project role as displayed in the credits.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "String slicing is not const on the pinned Rust toolchain"
    )]
    pub fn role(&self) -> &str {
        &self.role
    }

    /// SPDX expression or project license label.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "String slicing is not const on the pinned Rust toolchain"
    )]
    pub fn license(&self) -> &str {
        &self.license
    }
}

/// A nonempty, duplicate-free credits manifest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Credits {
    entries: Vec<ProjectCredit>,
}

impl Credits {
    /// Parse bounded printable-ASCII TSV bytes.
    ///
    /// Empty lines, CRLF, and lines beginning with `#` are accepted. Every
    /// data row must contain exactly project, role, and license fields.
    ///
    /// # Errors
    ///
    /// Returns [`CreditsError`] for excessive input, malformed rows,
    /// duplicate projects, or an empty manifest.
    pub fn parse(contents: &[u8]) -> Result<Self, CreditsError> {
        if contents.len() > MAXIMUM_CREDITS_BYTES {
            return Err(CreditsError::InputTooLarge);
        }
        let text = std::str::from_utf8(contents).map_err(|_| CreditsError::NotUtf8)?;
        let mut entries = Vec::new();
        for (line_index, raw_line) in text.split('\n').enumerate() {
            let line_number = line_index.saturating_add(1);
            let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: [&str; 3] =
                line.split('\t')
                    .collect::<Vec<_>>()
                    .try_into()
                    .map_err(|_| CreditsError::Line {
                        number: line_number,
                        reason: "row must have exactly three TSV fields",
                    })?;
            if !valid_field(fields[0], MAXIMUM_PROJECT_BYTES) {
                return Err(CreditsError::Line {
                    number: line_number,
                    reason: "invalid project",
                });
            }
            if !valid_field(fields[1], MAXIMUM_ROLE_BYTES) {
                return Err(CreditsError::Line {
                    number: line_number,
                    reason: "invalid role",
                });
            }
            if !valid_field(fields[2], MAXIMUM_LICENSE_BYTES) {
                return Err(CreditsError::Line {
                    number: line_number,
                    reason: "invalid license",
                });
            }
            if entries
                .iter()
                .any(|entry: &ProjectCredit| entry.project == fields[0])
            {
                return Err(CreditsError::Line {
                    number: line_number,
                    reason: "duplicate project",
                });
            }
            entries.push(ProjectCredit {
                project: fields[0].to_owned(),
                role: fields[1].to_owned(),
                license: fields[2].to_owned(),
            });
            if entries.len() > MAXIMUM_CREDITS {
                return Err(CreditsError::TooManyEntries);
            }
        }
        if entries.is_empty() {
            return Err(CreditsError::Empty);
        }
        Ok(Self { entries })
    }

    /// Borrow entries in manifest order.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Vec slicing is not const on the pinned Rust toolchain"
    )]
    pub fn entries(&self) -> &[ProjectCredit] {
        &self.entries
    }

    /// Number of attributed projects.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the manifest contains no entries.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Credits syntax or invariant failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CreditsError {
    /// Input exceeds the explicit document-size bound.
    InputTooLarge,
    /// Input is not valid UTF-8.
    NotUtf8,
    /// A specific row is malformed.
    Line {
        /// One-based source line number.
        number: usize,
        /// Stable diagnostic for the violated rule.
        reason: &'static str,
    },
    /// Manifest has no data rows.
    Empty,
    /// Manifest exceeds the fixed dashboard capacity.
    TooManyEntries,
}

impl fmt::Display for CreditsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputTooLarge => formatter.write_str("credits exceed their size limit"),
            Self::NotUtf8 => formatter.write_str("credits are not UTF-8"),
            Self::Line { number, reason } => write!(formatter, "credits line {number}: {reason}"),
            Self::Empty => formatter.write_str("credits contain no projects"),
            Self::TooManyEntries => {
                write!(
                    formatter,
                    "credits have more than {MAXIMUM_CREDITS} projects"
                )
            }
        }
    }
}

impl std::error::Error for CreditsError {}

fn valid_field(field: &str, maximum_bytes: usize) -> bool {
    !field.is_empty()
        && field.len() <= maximum_bytes
        && field
            .bytes()
            .all(|byte| (0x20..=0x7e).contains(&byte) && byte != b'\t')
}

#[cfg(test)]
mod tests {
    use super::{Credits, CreditsError, MAXIMUM_CREDITS};

    const DEPLOYED_CREDITS: &[u8] = include_bytes!("../../../deploy/menu/credits.tsv");

    #[test]
    fn parses_the_deployed_manifest_in_order() {
        let parsed = Credits::parse(DEPLOYED_CREDITS);
        assert!(matches!(parsed, Ok(ref credits) if credits.len() >= 25));
        let Some(credits) = parsed.ok() else {
            return;
        };
        let Some(first) = credits.entries().first() else {
            return;
        };
        assert_eq!(first.project(), "FCEUmm");
        assert_eq!(first.role(), "NES emulation");
        assert_eq!(first.license(), "GPL-2.0-only");
        assert!(!credits.is_empty());
    }

    #[test]
    fn accepts_comments_blank_lines_and_crlf() {
        let parsed = Credits::parse(b"# project\trole\tlicense\r\n\r\nOne\tRole\tMIT\r\n");
        assert!(matches!(parsed, Ok(ref credits) if credits.len() == 1));
    }

    #[test]
    fn rejects_empty_malformed_duplicate_and_excessive_manifests() {
        assert_eq!(
            Credits::parse(b"# only a comment\n"),
            Err(CreditsError::Empty)
        );
        for contents in [
            b"One\tRole\n".as_slice(),
            b"One\tRole\tMIT\textra\n".as_slice(),
            b"One\t\tMIT\n".as_slice(),
            b"One\tRole\tM\x01T\n".as_slice(),
            b"One\tRole\tMIT\nOne\tOther\tBSD\n".as_slice(),
        ] {
            assert!(Credits::parse(contents).is_err());
        }

        let mut excessive = String::new();
        for index in 0..=MAXIMUM_CREDITS {
            use std::fmt::Write as _;
            assert!(writeln!(excessive, "P{index}\tROLE\tMIT").is_ok());
        }
        assert_eq!(
            Credits::parse(excessive.as_bytes()),
            Err(CreditsError::TooManyEntries)
        );
    }
}
