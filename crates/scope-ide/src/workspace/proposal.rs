//! Exact-path, bounded text proposals. No baseline authority is given to tools.
#[cfg(test)]
#[path = "proposal_tests.rs"]
mod tests;
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use super::Workspace;
use crate::path::ScopePath;

#[derive(Debug, Clone, Copy)]
pub struct ProposalLimits {
    pub max_files: usize,
    pub max_file_bytes: usize,
    pub max_total_bytes: usize,
}

impl Default for ProposalLimits {
    fn default() -> Self {
        Self {
            max_files: 64,
            max_file_bytes: 256 * 1024,
            max_total_bytes: 2 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalChange {
    pub path: ScopePath,
    pub before: Option<String>,
    pub after: String,
}

#[derive(Debug)]
pub struct ProposalError(pub String);
impl std::fmt::Display for ProposalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ProposalError {}
impl From<std::io::Error> for ProposalError {
    fn from(error: std::io::Error) -> Self {
        Self(error.to_string())
    }
}

/// Sequential apply result. `applied` remains meaningful on failure; no rollback
/// or atomic multi-file transaction is promised.
#[derive(Debug, Default)]
pub struct Acceptance {
    pub applied: Vec<ScopePath>,
    pub error: Option<String>,
}

#[derive(Debug)]
struct File {
    before: Option<String>,
    after: Option<String>,
    version: u64,
}

#[derive(Debug)]
pub struct Proposal {
    root: PathBuf,
    writable: bool,
    files: BTreeMap<ScopePath, File>,
    limits: ProposalLimits,
    closed: bool,
}

fn checked_path(root: &Path, path: &ScopePath) -> Result<PathBuf, ProposalError> {
    let root_metadata = fs::symlink_metadata(root)?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(ProposalError(
            "snapshot root is no longer a regular directory".into(),
        ));
    }
    if path.as_str() == "."
        || path
            .as_str()
            .split('/')
            .any(|part| matches!(part, ".git" | "target" | "build" | "node_modules"))
    {
        return Err(ProposalError("excluded proposal path".into()));
    }
    let mut full = root.to_path_buf();
    let parts: Vec<_> = path.as_str().split('/').collect();
    for (index, part) in parts.iter().enumerate() {
        full.push(part);
        match fs::symlink_metadata(&full) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(ProposalError("symlinks are not allowed".into()));
            }
            Ok(meta) if index + 1 < parts.len() && !meta.is_dir() => {
                return Err(ProposalError("parent is not a directory".into()));
            }
            Ok(meta) if index + 1 == parts.len() && !meta.is_file() => {
                return Err(ProposalError("not a regular file".into()));
            }
            Ok(_) => {}
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound && index + 1 == parts.len() => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(full)
}

fn read_text(path: &Path, limit: usize) -> Result<Option<String>, ProposalError> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(ProposalError("file byte budget exceeded".into()));
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| ProposalError("binary/non-UTF-8 file rejected".into()))?;
    if text.contains('\0') {
        return Err(ProposalError("binary/NUL text rejected".into()));
    }
    Ok(Some(text))
}

impl Proposal {
    /// Snapshot only exact host-approved paths, including absent creation targets.
    /// Parents must already exist. Does not recurse or copy build directories.
    pub fn new(
        workspace: &Workspace,
        paths: &[ScopePath],
        limits: ProposalLimits,
    ) -> Result<Self, ProposalError> {
        if paths.len() > limits.max_files {
            return Err(ProposalError("file budget exceeded".into()));
        }
        let root = fs::canonicalize(workspace.root())?;
        let mut files = BTreeMap::new();
        let mut total = 0usize;
        for path in paths {
            let before = read_text(&checked_path(&root, path)?, limits.max_file_bytes)?;
            total = total.saturating_add(before.as_ref().map_or(0, String::len));
            if total > limits.max_total_bytes {
                return Err(ProposalError("snapshot byte budget exceeded".into()));
            }
            if files
                .insert(
                    path.clone(),
                    File {
                        after: before.clone(),
                        before,
                        version: 0,
                    },
                )
                .is_some()
            {
                return Err(ProposalError("duplicate approved path".into()));
            }
        }
        Ok(Self {
            root,
            writable: workspace.writable,
            files,
            limits,
            closed: false,
        })
    }

    fn active(&self) -> Result<(), ProposalError> {
        if self.closed {
            Err(ProposalError("proposal is closed".into()))
        } else {
            Ok(())
        }
    }

    pub fn paths(&self) -> Vec<ScopePath> {
        self.files.keys().cloned().collect()
    }

    pub fn read(&self, path: &ScopePath) -> Result<(&str, u64), ProposalError> {
        self.active()?;
        let file = self
            .files
            .get(path)
            .ok_or_else(|| ProposalError("path not approved".into()))?;
        Ok((
            file.after
                .as_deref()
                .ok_or_else(|| ProposalError("file does not exist in proposal".into()))?,
            file.version,
        ))
    }

    fn set(
        &mut self,
        path: &ScopePath,
        text: String,
        expected: Option<u64>,
    ) -> Result<u64, ProposalError> {
        self.active()?;
        let file = self
            .files
            .get(path)
            .ok_or_else(|| ProposalError("path not approved".into()))?;
        if expected.map_or(file.after.is_some(), |version| {
            file.after.is_none() || file.version != version
        }) {
            return Err(ProposalError("creation/revision conflict".into()));
        }
        let total = self
            .files
            .iter()
            .filter(|(key, _)| *key != path)
            .map(|(_, file)| file.after.as_ref().map_or(0, String::len))
            .sum::<usize>();
        if text.len() > self.limits.max_file_bytes
            || text.len() > self.limits.max_total_bytes.saturating_sub(total)
            || text.contains('\0')
        {
            return Err(ProposalError(
                "text byte budget exceeded or binary text".into(),
            ));
        }
        let file = self.files.get_mut(path).unwrap();
        file.after = Some(text);
        file.version += 1;
        Ok(file.version)
    }

    pub fn create(&mut self, path: &ScopePath, text: String) -> Result<u64, ProposalError> {
        self.set(path, text, None)
    }
    pub fn replace(
        &mut self,
        path: &ScopePath,
        text: String,
        version: u64,
    ) -> Result<u64, ProposalError> {
        self.set(path, text, Some(version))
    }

    pub fn changes(&self) -> Result<Vec<ProposalChange>, ProposalError> {
        self.active()?;
        Ok(self
            .files
            .iter()
            .filter(|(_, file)| file.before != file.after)
            .map(|(path, file)| ProposalChange {
                path: path.clone(),
                before: file.before.clone(),
                after: file.after.clone().unwrap(),
            })
            .collect())
    }

    pub fn discard(&mut self) {
        self.files.clear();
        self.closed = true;
    }

    /// Human host operation only. Preflight all changes, recheck each immediately
    /// before writing. This is not an atomic transaction or compare-and-swap.
    ///
    /// Path checks and opens are separate, pathname-based host operations, not
    /// race-resistant directory-handle traversal. Concurrent replacement of a
    /// leaf, parent, or root ancestor by a symlink can redirect reads/writes
    /// outside the approved root after a successful check. Exclusive creation
    /// prevents overwriting an existing final entry, but not parent-directory
    /// redirection. Hard-linked files are not detected; replacement truncates
    /// the existing inode and may affect other links. Use only with a trusted
    /// filesystem and without concurrent hostile mutation. External content
    /// changes between the final check and write can also be overwritten.
    /// Inspect both `Acceptance::applied` and `Acceptance::error`: truncation or
    /// a failed write can leave a partially applied baseline, with no rollback.
    pub fn accept(&mut self) -> Acceptance {
        let mut result = Acceptance::default();
        let apply = (|| -> Result<(), ProposalError> {
            if !self.writable {
                return Err(ProposalError("baseline workspace is read-only".into()));
            }
            let changes = self.changes()?;
            for change in &changes {
                let path = checked_path(&self.root, &change.path)?;
                if read_text(&path, self.limits.max_file_bytes)? != change.before {
                    return Err(ProposalError(format!("baseline conflict: {}", change.path)));
                }
            }
            for change in changes {
                let path = checked_path(&self.root, &change.path)?;
                if read_text(&path, self.limits.max_file_bytes)? != change.before {
                    return Err(ProposalError(format!("baseline conflict: {}", change.path)));
                }
                let mut options = OpenOptions::new();
                options.write(true);
                if change.before.is_none() {
                    options.create_new(true);
                } else {
                    options.truncate(true);
                }
                let mut file = options.open(path)?;
                // Opening/truncating already mutates the baseline; report this path
                // even when writing fails part-way through.
                result.applied.push(change.path.clone());
                file.write_all(change.after.as_bytes())?;
                self.files.get_mut(&change.path).unwrap().before = Some(change.after);
            }
            self.discard();
            Ok(())
        })();
        if let Err(error) = apply {
            result.error = Some(error.to_string());
        }
        result
    }
}
