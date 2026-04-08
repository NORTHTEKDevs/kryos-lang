//! Semantic versioning — version parsing, range specifiers, compatibility checks.

use std::fmt;
use std::str::FromStr;

/// A semantic version: `major.minor.patch[-pre]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Option<String>,
}

impl Version {
    pub fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self { major, minor, patch, pre: None }
    }

    pub fn with_pre(mut self, pre: impl Into<String>) -> Self {
        self.pre = Some(pre.into());
        self
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(ref pre) = self.pre {
            write!(f, "-{pre}")?;
        }
        Ok(())
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.major.cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
            .then(self.patch.cmp(&other.patch))
            .then_with(|| match (&self.pre, &other.pre) {
                // No pre-release is greater than any pre-release
                (None, None) => std::cmp::Ordering::Equal,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (Some(_), None) => std::cmp::Ordering::Less,
                (Some(a), Some(b)) => a.cmp(b),
            })
    }
}

impl FromStr for Version {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (version_part, pre) = if let Some(idx) = s.find('-') {
            (&s[..idx], Some(s[idx + 1..].to_string()))
        } else {
            (s, None)
        };

        let parts: Vec<&str> = version_part.split('.').collect();
        if parts.len() != 3 {
            return Err(format!("invalid version: expected MAJOR.MINOR.PATCH, got '{s}'"));
        }

        let major = parts[0].parse::<u64>()
            .map_err(|_| format!("invalid major version: '{}'", parts[0]))?;
        let minor = parts[1].parse::<u64>()
            .map_err(|_| format!("invalid minor version: '{}'", parts[1]))?;
        let patch = parts[2].parse::<u64>()
            .map_err(|_| format!("invalid patch version: '{}'", parts[2]))?;

        if let Some(ref pre_str) = pre {
            if pre_str.is_empty() {
                return Err("pre-release identifier cannot be empty".to_string());
            }
        }

        Ok(Version { major, minor, patch, pre })
    }
}

/// Comparison operator for version ranges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `^` — compatible (same major, unless major is 0)
    Caret,
    /// `~` — minor-compatible (same major.minor)
    Tilde,
    /// `=` — exact match
    Exact,
    /// `>=`
    GreaterEq,
    /// `>`
    Greater,
    /// `<=`
    LessEq,
    /// `<`
    Less,
}

/// A version requirement that can check if a `Version` satisfies it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionReq {
    pub op: Op,
    pub version: Version,
}

impl VersionReq {
    pub fn new(op: Op, version: Version) -> Self {
        Self { op, version }
    }

    /// Check whether the given version satisfies this requirement.
    pub fn matches(&self, v: &Version) -> bool {
        // Pre-release versions only match if same major.minor.patch
        // (standard semver pre-release semantics).
        match self.op {
            Op::Caret => self.matches_caret(v),
            Op::Tilde => self.matches_tilde(v),
            Op::Exact => self.matches_exact(v),
            Op::GreaterEq => v >= &self.version,
            Op::Greater => v > &self.version,
            Op::LessEq => v <= &self.version,
            Op::Less => v < &self.version,
        }
    }

    /// `^1.2.3` means `>=1.2.3, <2.0.0`
    /// `^0.2.3` means `>=0.2.3, <0.3.0`
    /// `^0.0.3` means `>=0.0.3, <0.0.4`
    fn matches_caret(&self, v: &Version) -> bool {
        if v < &self.version {
            return false;
        }
        if self.version.major != 0 {
            v.major == self.version.major
        } else if self.version.minor != 0 {
            v.major == 0 && v.minor == self.version.minor
        } else {
            v.major == 0 && v.minor == 0 && v.patch == self.version.patch
        }
    }

    /// `~1.2.3` means `>=1.2.3, <1.3.0`
    fn matches_tilde(&self, v: &Version) -> bool {
        if v < &self.version {
            return false;
        }
        v.major == self.version.major && v.minor == self.version.minor
    }

    /// `=1.2.3` means exactly `1.2.3`
    fn matches_exact(&self, v: &Version) -> bool {
        v == &self.version
    }
}

impl fmt::Display for VersionReq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let op_str = match self.op {
            Op::Caret => "^",
            Op::Tilde => "~",
            Op::Exact => "=",
            Op::GreaterEq => ">=",
            Op::Greater => ">",
            Op::LessEq => "<=",
            Op::Less => "<",
        };
        write!(f, "{op_str}{}", self.version)
    }
}

impl FromStr for VersionReq {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.is_empty() {
            return Err("empty version requirement".to_string());
        }

        let (op, rest) = if let Some(r) = s.strip_prefix(">=") {
            (Op::GreaterEq, r)
        } else if let Some(r) = s.strip_prefix("<=") {
            (Op::LessEq, r)
        } else if let Some(r) = s.strip_prefix('>') {
            (Op::Greater, r)
        } else if let Some(r) = s.strip_prefix('<') {
            (Op::Less, r)
        } else if let Some(r) = s.strip_prefix('^') {
            (Op::Caret, r)
        } else if let Some(r) = s.strip_prefix('~') {
            (Op::Tilde, r)
        } else if let Some(r) = s.strip_prefix('=') {
            (Op::Exact, r)
        } else {
            // bare version treated as caret
            (Op::Caret, s)
        };

        let version = rest.trim().parse::<Version>()?;
        Ok(VersionReq { op, version })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_basic() {
        let v: Version = "1.0.0".parse().unwrap();
        assert_eq!(v, Version::new(1, 0, 0));
    }

    #[test]
    fn parse_version_with_pre() {
        let v: Version = "1.2.3-beta.1".parse().unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 2);
        assert_eq!(v.patch, 3);
        assert_eq!(v.pre.as_deref(), Some("beta.1"));
    }

    #[test]
    fn version_ordering() {
        let v1: Version = "1.0.0".parse().unwrap();
        let v2: Version = "1.0.1".parse().unwrap();
        let v3: Version = "2.0.0".parse().unwrap();
        assert!(v1 < v2);
        assert!(v2 < v3);

        // Pre-release is less than release
        let pre: Version = "1.0.0-alpha".parse().unwrap();
        let rel: Version = "1.0.0".parse().unwrap();
        assert!(pre < rel);
    }

    #[test]
    fn caret_range() {
        let req: VersionReq = "^1.0.0".parse().unwrap();
        assert!(req.matches(&"1.5.0".parse().unwrap()));
        assert!(req.matches(&"1.0.0".parse().unwrap()));
        assert!(req.matches(&"1.99.99".parse().unwrap()));
        assert!(!req.matches(&"2.0.0".parse().unwrap()));
        assert!(!req.matches(&"0.9.0".parse().unwrap()));
    }

    #[test]
    fn tilde_range() {
        let req: VersionReq = "~1.2.0".parse().unwrap();
        assert!(req.matches(&"1.2.5".parse().unwrap()));
        assert!(req.matches(&"1.2.0".parse().unwrap()));
        assert!(!req.matches(&"1.3.0".parse().unwrap()));
        assert!(!req.matches(&"1.1.0".parse().unwrap()));
    }

    #[test]
    fn exact_range() {
        let req: VersionReq = "=1.0.0".parse().unwrap();
        assert!(req.matches(&"1.0.0".parse().unwrap()));
        assert!(!req.matches(&"1.0.1".parse().unwrap()));
        assert!(!req.matches(&"0.9.9".parse().unwrap()));
    }

    #[test]
    fn comparison_ranges() {
        let req: VersionReq = ">=1.0.0".parse().unwrap();
        assert!(req.matches(&"1.0.0".parse().unwrap()));
        assert!(req.matches(&"2.0.0".parse().unwrap()));
        assert!(!req.matches(&"0.9.9".parse().unwrap()));

        let req: VersionReq = "<2.0.0".parse().unwrap();
        assert!(req.matches(&"1.9.9".parse().unwrap()));
        assert!(!req.matches(&"2.0.0".parse().unwrap()));
    }

    #[test]
    fn display_roundtrip() {
        let v = Version::new(1, 2, 3).with_pre("beta.1");
        assert_eq!(v.to_string(), "1.2.3-beta.1");

        let req: VersionReq = "^1.0.0".parse().unwrap();
        assert_eq!(req.to_string(), "^1.0.0");
    }
}
