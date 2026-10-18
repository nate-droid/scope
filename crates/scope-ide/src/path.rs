use std::borrow::Cow;

use cell_guest::{CellPathBuf, InvalidPath};

/// Scope-owned workspace-relative path value used by public APIs.
///
/// `ScopePath` is backed by Cell's `CellPathBuf`, so Scope uses the same
/// UTF-8, relative, `/`-separated capability path rules as Cell guest
/// components. It does not grant filesystem access by itself; filesystem
/// operations go through capability-bearing types such as `Workspace`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ScopePath {
    inner: CellPathBuf,
}

impl ScopePath {
    pub fn new(path: impl Into<String>) -> Result<Self, InvalidPath> {
        Ok(Self {
            inner: CellPathBuf::new(path)?,
        })
    }

    pub fn root() -> Self {
        Self {
            inner: CellPathBuf::root(),
        }
    }

    pub fn join(&self, path: impl ToScopePath) -> Result<Self, InvalidPath> {
        let path = path.to_scope_path()?;
        if self.as_str() == "." {
            return Ok(path);
        }
        if path.as_str() == "." {
            return Ok(self.clone());
        }

        Self::new(format!("{}/{}", self.as_str(), path.as_str()))
    }

    pub fn parent(&self) -> Option<Self> {
        let (parent, _) = self.as_str().rsplit_once('/')?;
        Some(Self::new(parent).expect("parent of a valid ScopePath is valid"))
    }

    pub fn file_name(&self) -> Option<&str> {
        if self.as_str() == "." {
            return None;
        }

        self.as_str().rsplit('/').next()
    }

    pub fn extension(&self) -> Option<&str> {
        self.file_name()?
            .rsplit_once('.')
            .and_then(|(stem, extension)| {
                (!stem.is_empty() && !extension.is_empty()).then_some(extension)
            })
    }

    pub fn is_absolute(&self) -> bool {
        false
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn as_str(&self) -> &str {
        self.inner.as_str()
    }

    pub fn to_string_lossy(&self) -> Cow<'_, str> {
        Cow::Borrowed(self.as_str())
    }

    pub fn display(&self) -> impl std::fmt::Display + '_ {
        self
    }

    pub fn as_cell_path_buf(&self) -> &CellPathBuf {
        &self.inner
    }
}

impl std::fmt::Display for ScopePath {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inner.fmt(formatter)
    }
}

pub trait ToScopePath {
    fn to_scope_path(self) -> Result<ScopePath, InvalidPath>;
}

impl ToScopePath for ScopePath {
    fn to_scope_path(self) -> Result<ScopePath, InvalidPath> {
        Ok(self)
    }
}

impl ToScopePath for &ScopePath {
    fn to_scope_path(self) -> Result<ScopePath, InvalidPath> {
        Ok(self.clone())
    }
}

impl ToScopePath for &str {
    fn to_scope_path(self) -> Result<ScopePath, InvalidPath> {
        ScopePath::new(self)
    }
}

impl ToScopePath for String {
    fn to_scope_path(self) -> Result<ScopePath, InvalidPath> {
        ScopePath::new(self)
    }
}

impl ToScopePath for &String {
    fn to_scope_path(self) -> Result<ScopePath, InvalidPath> {
        ScopePath::new(self.as_str())
    }
}

impl From<CellPathBuf> for ScopePath {
    fn from(path: CellPathBuf) -> Self {
        Self { inner: path }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offers_common_path_queries_without_std_path_at_call_sites() {
        let path = ScopePath::new("src/main.scope").unwrap();

        assert_eq!(path.file_name(), Some("main.scope"));
        assert_eq!(path.extension(), Some("scope"));
        assert_eq!(path.to_string(), "src/main.scope");
    }

    #[test]
    fn joins_scope_paths() {
        let path = ScopePath::new("src").unwrap().join("main.scope").unwrap();

        assert_eq!(path.to_string_lossy(), "src/main.scope");
    }

    #[test]
    fn rejects_host_escape_paths_using_cell_rules() {
        assert!(ScopePath::new("../outside.txt").is_err());
        assert!(ScopePath::new("/tmp/input.txt").is_err());
        assert!(ScopePath::new("dir\\input.txt").is_err());
    }
}
