//! New downstream Host projection, NOT installed/native registry operations.
//! Uses only Workspace::list_files/load_file_revision. Markdown extraction is a
//! deliberately simple downstream line summary, not a Scope Markdown parser.
//! Limits reject, never truncate. Workspace APIs materialize listings/files before
//! these checks: this is not a streaming memory or filesystem traversal budget.
//! Revisions detect changes between reads, not an atomic multi-file snapshot.
use scope_ide::{
    tool::operations::OperationError,
    workspace::{Workspace, WorkspaceError},
};
use scope_script::{ErrorKind, Host, Value};
use std::collections::BTreeMap;

pub const SCRIPT: &str = include_str!("../workspace.scope");
pub const PROVIDER: &str = "downstream_workspace";
const MAX_ENTRIES: usize = 128;
const MAX_DOCUMENTS: usize = 16;
const MAX_BYTES: usize = 32_768;
const MAX_CALLS: usize = 8;

#[derive(Debug)]
pub enum InspectionError {
    Workspace(WorkspaceError),
    Unauthorized,
    Limit,
    DiscoveryRequired,
    Stale { path: String },
}
impl std::fmt::Display for InspectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stale { path } => write!(f, "document changed since discovery: {path}"),
            Self::Workspace(error) => error.fmt(f),
            Self::Unauthorized => f.write_str("read access denied; launch with --allow-read ROOT"),
            Self::Limit => f.write_str("workspace inspection limit exceeded (128 entries, 16 Markdown documents, 32 KiB source per batch, or 8 host calls); select a smaller workspace or reduce calls"),
            Self::DiscoveryRequired => f.write_str("call discover_markdown before inspect_discovered"),
        }
    }
}
impl std::error::Error for InspectionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Workspace(e) => Some(e),
            _ => None,
        }
    }
}
impl From<WorkspaceError> for InspectionError {
    fn from(e: WorkspaceError) -> Self {
        Self::Workspace(e)
    }
}

pub struct WorkspaceHost {
    workspace: Workspace,
    allow_read: bool,
    calls: usize,
    expected: Option<BTreeMap<String, String>>,
    /// ErrorKind has no workspace variant; preserve the typed cause here.
    pub last_error: Option<InspectionError>,
}
impl WorkspaceHost {
    pub fn open(root: &str, allow_read: bool) -> Result<Self, InspectionError> {
        // Deny before even opening the root.
        if !allow_read {
            return Err(InspectionError::Unauthorized);
        }
        Ok(Self {
            workspace: Workspace::read_only(root)?,
            allow_read,
            calls: 0,
            expected: None,
            last_error: None,
        })
    }
    fn charge(&mut self) -> Result<(), InspectionError> {
        if !self.allow_read {
            return Err(InspectionError::Unauthorized);
        }
        if self.calls >= MAX_CALLS {
            return Err(InspectionError::Limit);
        }
        self.calls += 1;
        Ok(())
    }
    fn read_batch(
        &self,
        paths: impl Iterator<Item = String>,
    ) -> Result<Vec<(String, String, String)>, InspectionError> {
        let mut bytes = 0usize;
        let mut files = Vec::new();
        for path in paths {
            if files.len() >= MAX_DOCUMENTS {
                return Err(InspectionError::Limit);
            }
            let file = self.workspace.load_file_revision(path.as_str())?;
            bytes = bytes
                .checked_add(file.contents.len())
                .ok_or(InspectionError::Limit)?;
            if bytes > MAX_BYTES {
                return Err(InspectionError::Limit);
            }
            files.push((path, file.revision, file.contents));
        }
        Ok(files)
    }
    pub fn discover(&mut self) -> Result<Value, InspectionError> {
        self.charge()?;
        let entries = self.workspace.list_files()?;
        if entries.len() > MAX_ENTRIES {
            return Err(InspectionError::Limit);
        }
        let mut paths: Vec<_> = entries
            .into_iter()
            .map(|e| e.path.to_string())
            .filter(|p| p.to_ascii_lowercase().ends_with(".md"))
            .collect();
        paths.sort();
        let files = self.read_batch(paths.into_iter())?;
        let documents = Value::Array(
            files
                .iter()
                .map(|(path, revision, _)| {
                    Value::object([
                        ("path", path.clone().into()),
                        ("revision", revision.clone().into()),
                    ])
                })
                .collect(),
        );
        self.expected = Some(files.into_iter().map(|(p, r, _)| (p, r)).collect());
        Ok(documents)
    }
    pub fn inspect(&mut self) -> Result<Value, InspectionError> {
        self.charge()?;
        let expected = self
            .expected
            .as_ref()
            .ok_or(InspectionError::DiscoveryRequired)?;
        let files = self.read_batch(expected.keys().cloned())?;
        let mut documents = Vec::new();
        let mut todos = 0;
        for (path, revision, text) in files {
            if expected[&path] != revision {
                return Err(InspectionError::Stale { path });
            }
            let headings = text
                .lines()
                .filter(|line| line.starts_with('#'))
                .map(|s| Value::String(s.into()))
                .collect();
            let pending: Vec<_> = text
                .lines()
                .enumerate()
                .filter(|(_, s)| s.contains("TODO") || s.contains("- [ ]"))
                .map(|(i, s)| {
                    Value::object([("line", Value::Integer((i + 1) as i64)), ("text", s.into())])
                })
                .collect();
            todos += pending.len();
            documents.push(Value::object([
                ("path", path.into()),
                ("revision", revision.into()),
                ("text", text.into()),
                ("headings", Value::Array(headings)),
                ("pending", Value::Array(pending)),
            ]));
        }
        Ok(Value::object([
            ("document_count", Value::Integer(documents.len() as i64)),
            ("todo_count", Value::Integer(todos as i64)),
            ("documents", Value::Array(documents)),
        ]))
    }
    fn project(&mut self, result: Result<Value, InspectionError>) -> Result<Value, ErrorKind> {
        result.map_err(|error| {
            let kind = match &error {
                InspectionError::Unauthorized => ErrorKind::Operation(OperationError::Unauthorized),
                InspectionError::Stale { .. } => {
                    ErrorKind::Operation(OperationError::StaleRevision)
                }
                InspectionError::Limit => ErrorKind::LimitExceeded,
                _ => ErrorKind::Runtime(format!("workspace adapter: {error}")),
            };
            self.last_error = Some(error);
            kind
        })
    }
}
impl Host for WorkspaceHost {
    fn operations(&mut self) -> Result<Value, ErrorKind> {
        let result = self.charge().map(|()| {
            Value::Array(
                ["discover_markdown", "inspect_discovered"]
                    .into_iter()
                    .map(|action| {
                        Value::object([
                            ("provider", PROVIDER.into()),
                            ("action", action.into()),
                            ("effect", "Read".into()),
                            (
                                "adapter",
                                "downstream batch projection; not registry operations".into(),
                            ),
                        ])
                    })
                    .collect(),
            )
        });
        self.project(result)
    }
    fn invoke(
        &mut self,
        provider: &str,
        action: &str,
        arguments: Value,
    ) -> Result<Value, ErrorKind> {
        if provider != PROVIDER || !matches!(action, "discover_markdown" | "inspect_discovered") {
            return Err(OperationError::UnknownOperation.into());
        }
        if arguments != Value::Object(BTreeMap::new()) {
            return Err(ErrorKind::Runtime(
                "workspace adapter expects empty arguments; paths and revisions are host-owned"
                    .into(),
            ));
        }
        let result = match action {
            "discover_markdown" => self.discover(),
            _ => self.inspect(),
        };
        self.project(result)
    }
}

/// Direct Rust equivalent of workspace.scope, sharing the same bounded read path.
pub fn direct(host: &mut WorkspaceHost) -> Result<Value, InspectionError> {
    let discovered = host.discover()?;
    let batch = host.inspect()?;
    let Value::Object(fields) = batch else {
        unreachable!()
    };
    Ok(Value::object([
        ("purpose", "Markdown agent handoff".into()),
        ("discovered", discovered),
        ("documents", fields["documents"].clone()),
        (
            "needs_follow_up",
            Value::Boolean(fields["todo_count"] != Value::Integer(0)),
        ),
        ("document_count", fields["document_count"].clone()),
        ("todo_count", fields["todo_count"].clone()),
    ]))
}
