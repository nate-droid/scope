//! Exact-path, bounded text proposals. No baseline authority is given to tools.
#[cfg(test)]
#[path = "proposal_tests.rs"]
mod tests;
use cell_capability_fs::{FileType, FsError, FsErrorKind, SaveOutcome};
use std::collections::BTreeMap;

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

/// Cell failures retain their original category, logical path, and operation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProposalErrorKind {
    Cell(FsError),
    Policy,
    NonRegularFile,
    Budget,
    NulText,
    Missing,
    Conflict,
    Closed,
}

/// `.0` and Display retain the human-readable message. Construct Scope errors
/// with `new`; Cell errors convert with `From<FsError>` without losing detail.
#[derive(Debug)]
pub struct ProposalError(pub String, ProposalErrorKind);
impl ProposalError {
    pub fn new(kind: ProposalErrorKind, message: impl Into<String>) -> Self {
        Self(message.into(), kind)
    }

    pub fn kind(&self) -> &ProposalErrorKind {
        &self.1
    }

    pub fn fs_error(&self) -> Option<&FsError> {
        match &self.1 {
            ProposalErrorKind::Cell(error) => Some(error),
            _ => None,
        }
    }
}
impl std::fmt::Display for ProposalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ProposalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.fs_error().map(|error| error as &dyn std::error::Error)
    }
}
impl From<FsError> for ProposalError {
    fn from(error: FsError) -> Self {
        Self(error.to_string(), ProposalErrorKind::Cell(error))
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
    revision: Option<String>,
    after: Option<String>,
    version: u64,
}

#[derive(Debug)]
pub struct Proposal {
    workspace: Workspace,
    writable: bool,
    files: BTreeMap<ScopePath, File>,
    limits: ProposalLimits,
    closed: bool,
}

fn checked_path(workspace: &Workspace, path: &ScopePath) -> Result<String, ProposalError> {
    let root_metadata = workspace.fs.symlink_metadata(super::DEFAULT_MOUNT_NAME)?;
    if !matches!(root_metadata.file_type, FileType::Directory) {
        return Err(ProposalError::new(
            ProposalErrorKind::NonRegularFile,
            "snapshot root is no longer a regular directory",
        ));
    }
    if path.as_str() == "."
        || path
            .as_str()
            .split('/')
            .any(|part| matches!(part, ".git" | "target" | "build" | "node_modules"))
    {
        return Err(ProposalError::new(
            ProposalErrorKind::Policy,
            "excluded proposal path",
        ));
    }
    let mut full = super::DEFAULT_MOUNT_NAME.to_string();
    let parts: Vec<_> = path.as_str().split('/').collect();
    for (index, part) in parts.iter().enumerate() {
        full.push('/');
        full.push_str(part);
        match workspace.fs.symlink_metadata(&full) {
            Ok(meta) if matches!(meta.file_type, FileType::Symlink) => {
                return Err(ProposalError::new(
                    ProposalErrorKind::Policy,
                    "symlinks are not allowed",
                ));
            }
            Ok(meta)
                if index + 1 < parts.len() && !matches!(meta.file_type, FileType::Directory) =>
            {
                return Err(ProposalError::new(
                    ProposalErrorKind::NonRegularFile,
                    "parent is not a directory",
                ));
            }
            Ok(meta) if index + 1 == parts.len() && !matches!(meta.file_type, FileType::File) => {
                return Err(ProposalError::new(
                    ProposalErrorKind::NonRegularFile,
                    "not a regular file",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind == FsErrorKind::NotFound && index + 1 == parts.len() => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(full)
}

fn read_text(
    workspace: &Workspace,
    path: &str,
    limit: usize,
) -> Result<Option<cell_capability_fs::RevisionedText>, ProposalError> {
    let text = match workspace.fs.read_text_revision_bounded(path, limit) {
        Ok(file) => file,
        Err(error) if error.kind == FsErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if text.contents.contains('\0') {
        return Err(ProposalError::new(
            ProposalErrorKind::NulText,
            "binary/NUL text rejected",
        ));
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
            return Err(ProposalError::new(
                ProposalErrorKind::Budget,
                "file budget exceeded",
            ));
        }

        let mut files = BTreeMap::new();
        let mut total = 0usize;
        for path in paths {
            let snapshot = read_text(
                workspace,
                &checked_path(workspace, path)?,
                limits.max_file_bytes,
            )?;
            let (before, revision) = match snapshot {
                Some(file) => (Some(file.contents), Some(file.revision)),
                None => (None, None),
            };
            total = total.saturating_add(before.as_ref().map_or(0, String::len));
            if total > limits.max_total_bytes {
                return Err(ProposalError::new(
                    ProposalErrorKind::Budget,
                    "snapshot byte budget exceeded",
                ));
            }
            if files
                .insert(
                    path.clone(),
                    File {
                        after: before.clone(),
                        before,
                        revision,
                        version: 0,
                    },
                )
                .is_some()
            {
                return Err(ProposalError::new(
                    ProposalErrorKind::Policy,
                    "duplicate approved path",
                ));
            }
        }
        Ok(Self {
            workspace: workspace.clone(),
            writable: workspace.writable,
            files,
            limits,
            closed: false,
        })
    }

    fn active(&self) -> Result<(), ProposalError> {
        if self.closed {
            Err(ProposalError::new(
                ProposalErrorKind::Closed,
                "proposal is closed",
            ))
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
            .ok_or_else(|| ProposalError::new(ProposalErrorKind::Policy, "path not approved"))?;
        Ok((
            file.after.as_deref().ok_or_else(|| {
                ProposalError::new(
                    ProposalErrorKind::Missing,
                    "file does not exist in proposal",
                )
            })?,
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
            .ok_or_else(|| ProposalError::new(ProposalErrorKind::Policy, "path not approved"))?;
        if expected.map_or(file.after.is_some(), |version| {
            file.after.is_none() || file.version != version
        }) {
            return Err(ProposalError::new(
                ProposalErrorKind::Conflict,
                "creation/revision conflict",
            ));
        }
        let total = self
            .files
            .iter()
            .filter(|(key, _)| *key != path)
            .map(|(_, file)| file.after.as_ref().map_or(0, String::len))
            .sum::<usize>();
        if text.len() > self.limits.max_file_bytes
            || text.len() > self.limits.max_total_bytes.saturating_sub(total)
        {
            return Err(ProposalError::new(
                ProposalErrorKind::Budget,
                "text byte budget exceeded",
            ));
        }
        if text.contains('\0') {
            return Err(ProposalError::new(
                ProposalErrorKind::NulText,
                "binary/NUL text rejected",
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

    fn check_baseline(&self, change: &ProposalChange) -> Result<String, ProposalError> {
        let path = checked_path(&self.workspace, &change.path)?;
        if read_text(&self.workspace, &path, self.limits.max_file_bytes)?.map(|file| file.contents)
            != change.before
        {
            return Err(ProposalError::new(
                ProposalErrorKind::Conflict,
                format!("baseline conflict: {}", change.path),
            ));
        }
        Ok(path)
    }

    /// Human host operation only. Preflight all changes, recheck each immediately
    /// before writing. This is not an atomic transaction or compare-and-swap.
    ///
    /// Cell retains the original directory grant and confines each operation.
    /// Component metadata checks preserve the no-symlink policy only at check
    /// time: they are not nofollow traversal or namespace locks. A concurrent
    /// contained symlink substitution can redirect an operation within the grant.
    /// Use only with a trusted filesystem, including Cell's temporary namespace.
    /// Snapshot and preflight/recheck reads are bounded. Cell's additional
    /// expected-revision verification inside save_text currently rereads without
    /// a byte bound, so concurrent growth can exceed the proposal read budget.
    /// Revision checking is best-effort: changes after verification can still
    /// be overwritten.
    /// Save replaces an inode rather than truncating it; hardlink identity and
    /// extended security metadata are not preserved. No crash durability is promised.
    /// Inspect both `Acceptance::applied` and `Acceptance::error`: only successfully
    /// published paths are reported, including when a later publication fails.
    pub fn accept(&mut self) -> Acceptance {
        let mut result = Acceptance::default();
        let apply = (|| -> Result<(), ProposalError> {
            if !self.writable {
                return Err(ProposalError::new(
                    ProposalErrorKind::Policy,
                    "baseline workspace is read-only",
                ));
            }
            let changes = self.changes()?;
            for change in &changes {
                self.check_baseline(change)?;
            }
            for change in changes {
                let path = self.check_baseline(&change)?;
                let revision = if change.before.is_none() {
                    self.workspace.fs.create_text(&path, &change.after)?
                } else {
                    let expected =
                        self.files[&change.path]
                            .revision
                            .as_deref()
                            .ok_or_else(|| {
                                ProposalError::new(
                                    ProposalErrorKind::Policy,
                                    "missing baseline revision; replacement blocked",
                                )
                            })?;
                    match self
                        .workspace
                        .fs
                        .save_text(&path, &change.after, Some(expected))?
                    {
                        SaveOutcome::Saved(revision) => revision,
                        SaveOutcome::Conflict(_) => {
                            return Err(ProposalError::new(
                                ProposalErrorKind::Conflict,
                                format!("baseline conflict: {}", change.path),
                            ));
                        }
                    }
                };
                result.applied.push(change.path.clone());
                let file = self.files.get_mut(&change.path).unwrap();
                file.before = Some(change.after);
                file.revision = Some(revision);
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
