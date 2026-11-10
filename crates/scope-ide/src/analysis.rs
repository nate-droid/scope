//! Invocation-scoped analysis reads. No filesystem authority escapes this module.
use std::sync::Arc;

use scope_language::{RevisionId, SourceId, SourceSnapshot};

use crate::{
    path::ScopePath,
    scratch::{ScratchBuffers, SourceOrigin},
    tui::EditorDraftSnapshot,
};

/// Bounds count every capture attempt, including failures and pinned reads.
/// Text limits are UTF-8 bytes, not characters. Cell buffers at most the read
/// allowance and may read one extra byte to detect overflow. Before every I/O,
/// the full allowance plus that probe is charged, even on failure or short reads.
/// Pinned reads cost no byte-work. Revalidation gets a fresh max_read_bytes
/// ceiling per call and charges captured text length plus one for each edge.
#[derive(Clone, Copy, Debug)]
pub struct AnalysisLimits {
    pub max_attempts: usize,
    pub max_path_bytes: usize,
    pub max_snapshots: usize,
    pub max_file_bytes: usize,
    pub max_total_bytes: usize,
    pub max_read_bytes: usize,
}

impl Default for AnalysisLimits {
    fn default() -> Self {
        Self {
            max_attempts: 128,
            max_path_bytes: 4096,
            max_snapshots: 64,
            max_file_bytes: 1024 * 1024,
            max_total_bytes: 4 * 1024 * 1024,
            max_read_bytes: 8 * 1024 * 1024,
        }
    }
}

/// An opaque check invocation identity, not a source reference or a grant.
#[derive(Clone, Debug)]
pub struct CheckInvocationId(Arc<()>);

impl PartialEq for CheckInvocationId {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for CheckInvocationId {}

/// An opaque invocation-local source reference, not a path or a grant.
#[derive(Clone, Debug)]
pub struct SourceHandle(Arc<HandleIdentity>);

#[derive(Debug)]
struct HandleIdentity {
    invocation: Arc<()>,
    path: ScopePath,
}

#[derive(Clone, Debug)]
pub struct DependencySnapshot {
    handle: SourceHandle,
    source: SourceSnapshot,
    path: ScopePath,
    disk_revision: String,
}

impl DependencySnapshot {
    pub fn disk_revision(&self) -> &str {
        &self.disk_revision
    }
    pub fn handle(&self) -> SourceHandle {
        self.handle.clone()
    }
    pub fn text(&self) -> &str {
        &self.source.text
    }
    pub fn source_id(&self) -> &SourceId {
        &self.source.source_id
    }
    pub fn revision_id(&self) -> RevisionId {
        self.source.revision_id
    }
    pub fn path(&self) -> &ScopePath {
        &self.path
    }
    pub fn source(&self) -> &SourceSnapshot {
        &self.source
    }
}

/// Successful acquisitions, in order, including repeated reads of pinned text.
#[derive(Clone, Debug)]
pub struct DependencyEdge {
    from: ScopePath,
    request: String,
    resolved: ScopePath,
    snapshot_index: usize,
    attempt: usize,
}
impl DependencyEdge {
    pub fn from(&self) -> &ScopePath {
        &self.from
    }
    pub fn request(&self) -> &str {
        &self.request
    }
    pub fn resolved(&self) -> &ScopePath {
        &self.resolved
    }
    pub fn snapshot_index(&self) -> usize {
        self.snapshot_index
    }
    /// One-based capture attempt, preserving gaps from rejected acquisitions.
    pub fn attempt(&self) -> usize {
        self.attempt
    }
}

/// Receipt for a successful acquisition, including a pinned repeat.
#[derive(Clone, Debug)]
pub struct AcquisitionReceipt {
    snapshot_index: usize,
    disk_revision: String,
}
impl AcquisitionReceipt {
    pub fn snapshot_index(&self) -> usize {
        self.snapshot_index
    }
    pub fn disk_revision(&self) -> &str {
        &self.disk_revision
    }
}

/// One admitted capture attempt. Oversized requests retain only a bounded UTF-8
/// prefix; request_bytes records the original length, never a silently exact name.
#[derive(Clone, Debug)]
pub struct AcquisitionRecord {
    attempt: usize,
    base: ScopePath,
    request: String,
    request_bytes: usize,
    resolved: Option<ScopePath>,
    outcome: Result<AcquisitionReceipt, String>,
    read_limit: Option<usize>,
    read_bytes_charged: usize,
}
impl AcquisitionRecord {
    /// Content allowance passed to Cell, or None when no filesystem read occurred.
    pub fn read_limit(&self) -> Option<usize> {
        self.read_limit
    }
    /// Worst-case bytes charged, including the one-byte overflow probe.
    pub fn read_bytes_charged(&self) -> usize {
        self.read_bytes_charged
    }

    pub fn attempt(&self) -> usize {
        self.attempt
    }
    pub fn base(&self) -> &ScopePath {
        &self.base
    }
    pub fn request(&self) -> &str {
        &self.request
    }
    pub fn request_bytes(&self) -> usize {
        self.request_bytes
    }
    pub fn request_truncated(&self) -> bool {
        self.request.len() != self.request_bytes
    }
    pub fn resolved(&self) -> Option<&ScopePath> {
        self.resolved.as_ref()
    }
    pub fn outcome(&self) -> Result<&AcquisitionReceipt, &str> {
        self.outcome.as_ref().map_err(String::as_str)
    }
}

pub struct AnalysisContext<'a> {
    sources: &'a ScratchBuffers,
    editor: EditorDraftSnapshot,
    root: SourceSnapshot,
    origin: Option<Arc<SourceOrigin>>,
    root_handle: Option<SourceHandle>,
    invocation: Arc<()>,
    limits: AnalysisLimits,
    attempts: usize,
    bytes: usize,
    read_bytes_charged: usize,
    snapshots: Vec<DependencySnapshot>,
    edges: Vec<DependencyEdge>,
    acquisitions: Vec<AcquisitionRecord>,
    navigation: Option<std::ops::Range<usize>>,
    exhausted: bool,
}

impl<'a> AnalysisContext<'a> {
    pub fn invocation_id(&self) -> CheckInvocationId {
        CheckInvocationId(self.invocation.clone())
    }

    pub(crate) fn new(
        sources: &'a ScratchBuffers,
        editor: EditorDraftSnapshot,
        limits: AnalysisLimits,
    ) -> Result<Self, String> {
        let owner_id = &editor.baseline.source.source_id;
        let owner = sources
            .owner(owner_id)
            .ok_or("analysis owner is no longer live")?;
        if owner.id() != editor.baseline.scratch_id
            || owner.version() != editor.baseline.version
            || editor.baseline.source.revision_id != RevisionId::from_version(owner.version())
            || editor.baseline.source.text.as_ref() != owner.text()
        {
            return Err("analysis editor baseline is stale".into());
        }
        if editor.text.len() > limits.max_file_bytes || editor.text.len() > limits.max_total_bytes {
            return Err("analysis root byte limit exceeded".into());
        }
        let origin = sources.owner_origin(owner_id).cloned();
        if origin
            .as_ref()
            .is_some_and(|origin| sources.validate_origin(owner_id, origin).is_none())
        {
            return Err("analysis origin is stale".into());
        }
        let invocation = Arc::new(());
        let root_handle = origin.as_ref().map(|origin| {
            SourceHandle(Arc::new(HandleIdentity {
                invocation: invocation.clone(),
                path: origin.path().clone(),
            }))
        });
        let root = SourceSnapshot {
            source_id: SourceId::fresh(),
            revision_id: RevisionId::from_version(0),
            text: editor.text.clone().into(),
        };
        Ok(Self {
            sources,
            bytes: editor.text.len(),
            read_bytes_charged: 0,
            editor,
            root,
            origin,
            root_handle,
            invocation,
            limits,
            attempts: 0,
            snapshots: Vec::new(),
            edges: Vec::new(),
            acquisitions: Vec::new(),
            navigation: None,
            exhausted: false,
        })
    }

    /// Retain a canonical root-draft location, never a location parsed from output.
    pub fn set_navigation_range(&mut self, range: scope_language::TextRange) -> Result<(), String> {
        self.navigation = Some(
            scope_language::LineIndex::new(&self.editor.text)
                .span_for_range(range)
                .ok_or("invalid root navigation range")?,
        );
        Ok(())
    }

    pub fn draft(&self) -> &EditorDraftSnapshot {
        &self.editor
    }
    pub fn root(&self) -> &SourceSnapshot {
        &self.root
    }
    pub fn origin(&self) -> Option<SourceHandle> {
        self.root_handle.clone()
    }
    pub fn attempts(&self) -> usize {
        self.attempts
    }
    pub fn read_bytes_charged(&self) -> usize {
        self.read_bytes_charged
    }

    pub fn capture(
        &mut self,
        from: &SourceHandle,
        request: &str,
    ) -> Result<DependencySnapshot, String> {
        if self.attempts >= self.limits.max_attempts {
            self.exhausted = true;
            return Err("analysis capture attempt limit exceeded".into());
        }
        self.attempts += 1;
        let mut resolved = None;
        let mut read_limit = None;
        let before = self.read_bytes_charged;
        let result = self.capture_admitted(from, request, &mut resolved, &mut read_limit);
        let outcome = result
            .as_ref()
            .map(|snapshot| AcquisitionReceipt {
                snapshot_index: self
                    .edges
                    .last()
                    .expect("successful capture records an edge")
                    .snapshot_index,
                disk_revision: snapshot.disk_revision.clone(),
            })
            .map_err(Clone::clone);
        let mut end = request.len().min(self.limits.max_path_bytes);
        while !request.is_char_boundary(end) {
            end -= 1;
        }
        self.acquisitions.push(AcquisitionRecord {
            attempt: self.attempts,
            base: from.0.path.clone(),
            request: request[..end].into(),
            request_bytes: request.len(),
            resolved,
            outcome,
            read_limit,
            read_bytes_charged: self.read_bytes_charged - before,
        });
        result
    }

    fn capture_admitted(
        &mut self,
        from: &SourceHandle,
        request: &str,
        resolved: &mut Option<ScopePath>,
        read_limit: &mut Option<usize>,
    ) -> Result<DependencySnapshot, String> {
        if !Arc::ptr_eq(&self.invocation, &from.0.invocation) {
            return Err("foreign analysis source handle".into());
        }
        if request.len() > self.limits.max_path_bytes {
            return Err("analysis path byte limit exceeded".into());
        }
        let path = resolve(&from.0.path, request)?;
        *resolved = Some(path.clone());
        if path.as_str().len() > self.limits.max_path_bytes {
            return Err("analysis resolved path byte limit exceeded".into());
        }
        let index = match self
            .snapshots
            .iter()
            .position(|snapshot| snapshot.path == path)
        {
            Some(index) => index,
            None => {
                if self.limits.max_total_bytes.saturating_sub(self.bytes) == 0
                    || self.limits.max_file_bytes == 0
                {
                    return Err("analysis dependency byte budget exhausted before read".into());
                }
                if self.snapshots.len() >= self.limits.max_snapshots {
                    return Err("analysis snapshot limit exceeded".into());
                }
                let origin = self
                    .origin
                    .as_ref()
                    .ok_or("analysis has no source origin")?;
                let work_remaining = self
                    .limits
                    .max_read_bytes
                    .saturating_sub(self.read_bytes_charged);
                if work_remaining <= 1 {
                    return Err("analysis read byte-work budget exhausted before read".into());
                }
                let allowance = self
                    .limits
                    .max_file_bytes
                    .min(self.limits.max_total_bytes - self.bytes)
                    .min(work_remaining - 1);
                // Reserve the worst case before entering Cell: failures never refund work.
                self.read_bytes_charged += allowance + 1;
                *read_limit = Some(allowance);
                let file = origin
                    .read_bounded(&path, allowance)
                    .map_err(|error| error.to_string())?;
                if file.path != path {
                    return Err("analysis read path mismatch".into());
                }
                let bytes = file.contents.len();
                if bytes > self.limits.max_file_bytes
                    || bytes > self.limits.max_total_bytes.saturating_sub(self.bytes)
                {
                    return Err("analysis dependency byte limit exceeded".into());
                }
                self.bytes += bytes;
                let index = self.snapshots.len();
                self.snapshots.push(DependencySnapshot {
                    handle: SourceHandle(Arc::new(HandleIdentity {
                        invocation: self.invocation.clone(),
                        path: path.clone(),
                    })),
                    source: SourceSnapshot {
                        source_id: SourceId::fresh(),
                        revision_id: RevisionId::from_version(0),
                        text: file.contents.into(),
                    },
                    path: path.clone(),
                    disk_revision: file.revision,
                });
                index
            }
        };
        self.edges.push(DependencyEdge {
            from: from.0.path.clone(),
            request: request.into(),
            resolved: path,
            snapshot_index: index,
            attempt: self.attempts,
        });
        Ok(self.snapshots[index].clone())
    }

    pub(crate) fn seal(self) -> CheckEvidence {
        // Keep the owner borrow until all acquisition is complete.
        let _ = self.sources;
        CheckEvidence {
            invocation: self.invocation,
            editor: self.editor,
            root: self.root,
            origin: self.origin,
            snapshots: self.snapshots,
            edges: self.edges,
            acquisitions: self.acquisitions,
            navigation: self.navigation,
            exhausted: self.exhausted,
            attempts: self.attempts,
            read_bytes_charged: self.read_bytes_charged,
            max_read_bytes: self.limits.max_read_bytes,
        }
    }
}

fn resolve(from: &ScopePath, request: &str) -> Result<ScopePath, String> {
    if request.is_empty() {
        return Err("empty analysis dependency path".into());
    }
    let relative = ScopePath::new(request.to_owned()).map_err(|error| error.to_string())?;
    from.parent()
        .unwrap_or_else(ScopePath::root)
        .join(relative)
        .map_err(|error| error.to_string())
}

/// Historical immutable diagnostic mappings and the evidence needed to revalidate
/// them. The host must separately compare the full editor snapshot before use.
#[derive(Clone, Debug)]
pub struct CheckEvidence {
    invocation: Arc<()>,
    editor: EditorDraftSnapshot,
    root: SourceSnapshot,
    origin: Option<Arc<SourceOrigin>>,
    snapshots: Vec<DependencySnapshot>,
    edges: Vec<DependencyEdge>,
    acquisitions: Vec<AcquisitionRecord>,
    navigation: Option<std::ops::Range<usize>>,
    exhausted: bool,
    attempts: usize,
    read_bytes_charged: usize,
    max_read_bytes: usize,
}
impl scope_language::SourceResolver for CheckEvidence {
    fn resolve(
        &self,
        source: &SourceId,
        revision: RevisionId,
    ) -> Result<SourceSnapshot, scope_language::SourceUnavailable> {
        CheckEvidence::resolve(self, source, revision)
            .cloned()
            .ok_or(scope_language::SourceUnavailable::UnknownOrExpired)
    }
}

impl CheckEvidence {
    pub(crate) fn navigation(&self) -> Option<usize> {
        self.navigation.as_ref().map(|range| range.start)
    }

    pub(crate) fn navigated(&mut self, cursor: usize) {
        self.editor.cursor = cursor;
        self.editor.anchor = None;
    }

    pub fn invocation_id(&self) -> CheckInvocationId {
        CheckInvocationId(self.invocation.clone())
    }

    pub fn draft(&self) -> &EditorDraftSnapshot {
        &self.editor
    }
    pub fn root(&self) -> &SourceSnapshot {
        &self.root
    }
    pub fn root_path(&self) -> Option<&ScopePath> {
        self.origin.as_ref().map(|origin| origin.path())
    }
    pub fn snapshots(&self) -> &[DependencySnapshot] {
        &self.snapshots
    }
    pub fn edges(&self) -> &[DependencyEdge] {
        &self.edges
    }
    pub fn attempts(&self) -> usize {
        self.attempts
    }
    pub fn read_bytes_charged(&self) -> usize {
        self.read_bytes_charged
    }
    pub fn resolve(&self, source: &SourceId, revision: RevisionId) -> Option<&SourceSnapshot> {
        std::iter::once(&self.root)
            .chain(self.snapshots.iter().map(|snapshot| &snapshot.source))
            .find(|snapshot| &snapshot.source_id == source && snapshot.revision_id == revision)
    }
    pub fn acquisitions(&self) -> &[AcquisitionRecord] {
        &self.acquisitions
    }
    pub fn exhausted(&self) -> bool {
        self.exhausted
    }
    pub fn validate(&self, sources: &ScratchBuffers) -> Result<(), String> {
        if self.exhausted
            || self
                .acquisitions
                .iter()
                .any(|record| record.outcome.is_err())
        {
            return Err("analysis contains rejected acquisitions; historical evidence only".into());
        }
        let owner = &self.editor.baseline.source.source_id;
        match &self.origin {
            Some(origin) => {
                sources
                    .validate_origin(owner, origin)
                    .ok_or("analysis origin is no longer current")?;
                let mut work_remaining = self.max_read_bytes;
                for edge in &self.edges {
                    let path = resolve(&edge.from, &edge.request)?;
                    if path != edge.resolved {
                        return Err("analysis dependency resolution changed".into());
                    }
                    let snapshot = &self.snapshots[edge.snapshot_index];
                    let charge = snapshot
                        .text()
                        .len()
                        .checked_add(1)
                        .ok_or("analysis revalidation byte-work overflow")?;
                    work_remaining = work_remaining
                        .checked_sub(charge)
                        .ok_or("analysis revalidation byte-work budget exhausted")?;
                    let file = origin
                        .read_bounded(&path, snapshot.text().len())
                        .map_err(|error| error.to_string())?;
                    if file.path != snapshot.path
                        || file.revision != snapshot.disk_revision
                        || file.contents != snapshot.text()
                    {
                        return Err("analysis dependency changed".into());
                    }
                }
            }
            None => {
                sources
                    .owner(owner)
                    .ok_or("analysis owner is no longer live")?;
                if sources.owner_origin(owner).is_some() {
                    return Err("analysis owner acquired an origin".into());
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        language::LanguageRegistry, test_support::TempWorkspace, tui::ScratchEditSnapshot,
    };

    fn draft(sources: &ScratchBuffers, text: &str) -> EditorDraftSnapshot {
        let owner = sources.selected().unwrap();
        EditorDraftSnapshot {
            baseline: ScratchEditSnapshot {
                scratch_id: owner.id(),
                source: SourceSnapshot {
                    source_id: owner.evaluation_source_id().clone(),
                    revision_id: RevisionId::from_version(owner.version()),
                    text: owner.text().into(),
                },
                version: owner.version(),
            },
            text: text.into(),
            edit_identity: 12,
            cursor: 0,
            anchor: None,
            editor: "test".into(),
            editor_generation: 3,
            selection_identity: 4,
            source_selection_identity: 5,
            owner_binding_generation: 6,
        }
    }

    fn fixture() -> (TempWorkspace, ScratchBuffers) {
        let root = TempWorkspace::new("analysis");
        root.write_file("dir/root.txt", "baseline");
        root.write_file("dir/dep.txt", "dependency");
        root.write_file("dir/sub/child.txt", "child");
        let mut sources = ScratchBuffers::default();
        sources
            .open(
                &root.workspace(),
                &LanguageRegistry::default(),
                "dir/root.txt",
            )
            .unwrap();
        (root, sources)
    }

    #[test]
    fn invocation_identity_is_shared_with_handles_and_sealed_evidence() {
        let (_files, sources) = fixture();
        let editor = draft(&sources, "draft");
        let mut context =
            AnalysisContext::new(&sources, editor.clone(), AnalysisLimits::default()).unwrap();
        let id = context.invocation_id();
        assert_eq!(id, id.clone());
        assert_eq!(id, context.invocation_id());
        let other = AnalysisContext::new(&sources, editor, AnalysisLimits::default()).unwrap();
        assert_ne!(id, other.invocation_id());
        let root = context.origin().unwrap();
        assert!(Arc::ptr_eq(&id.0, &root.0.invocation));
        let dependency = context.capture(&root, "dep.txt").unwrap();
        assert!(Arc::ptr_eq(&id.0, &dependency.handle.0.invocation));
        let evidence = context.seal();
        assert_eq!(id, evidence.invocation_id());
        assert_eq!(id, evidence.clone().invocation_id());
        assert_ne!(evidence.invocation_id(), other.seal().invocation_id());
    }

    #[test]
    fn invocation_identity_survives_sealing_without_origin_or_dependencies() {
        let mut sources = ScratchBuffers::default();
        sources.create("text", "local");
        let context = AnalysisContext::new(
            &sources,
            draft(&sources, "local draft"),
            AnalysisLimits::default(),
        )
        .unwrap();
        assert!(context.origin().is_none());
        let id = context.invocation_id();
        let evidence = context.seal();
        assert!(evidence.snapshots().is_empty());
        assert_eq!(id, evidence.invocation_id());
        assert_eq!(id, evidence.clone().invocation_id());
    }

    #[test]
    fn captures_real_cell_reads_pins_paths_and_retains_diagnostic_mappings() {
        let (files, sources) = fixture();
        let editor = draft(&sources, "unsaved editor text");
        let mut context =
            AnalysisContext::new(&sources, editor.clone(), AnalysisLimits::default()).unwrap();
        assert_eq!(context.draft(), &editor);
        assert_eq!(&*context.root().text, editor.text);
        assert_ne!(context.root().source_id, editor.baseline.source.source_id);
        let root = context.origin().unwrap();
        let first = context.capture(&root, "dep.txt").unwrap();
        assert_eq!(first.text(), "dependency");
        assert_eq!(first.path().as_str(), "dir/dep.txt");
        files.write_file("dir/dep.txt", "changed");
        let again = context.capture(&root, "dep.txt").unwrap();
        assert_eq!(again.source(), first.source());
        assert!(Arc::ptr_eq(&again.handle.0, &first.handle.0));
        let child = context.capture(&root, "sub/child.txt").unwrap();
        let nested = context.capture(&child.handle(), "child.txt").unwrap();
        assert_eq!(child.source_id(), nested.source_id());
        let evidence = context.seal();
        assert_eq!(evidence.snapshots().len(), 2);
        assert_eq!(evidence.edges().len(), 4);
        assert_eq!(evidence.edges()[1].snapshot_index(), 0);
        assert_eq!(evidence.edges()[1].attempt(), 2);
        assert_eq!(evidence.edges()[0].from().as_str(), "dir/root.txt");
        assert_eq!(evidence.edges()[0].request(), "dep.txt");
        assert_eq!(evidence.edges()[0].resolved(), first.path());
        assert_eq!(evidence.attempts(), 4);
        assert_eq!(
            evidence.resolve(first.source_id(), first.revision_id()),
            Some(first.source())
        );
        assert_eq!(
            evidence.resolve(&evidence.root().source_id, evidence.root().revision_id),
            Some(evidence.root())
        );
        assert!(evidence.validate(&sources).is_err());
        assert_eq!(evidence.draft(), &editor);
    }

    #[test]
    fn original_grant_survives_workspace_drop_and_rejects_dependency_changes() {
        let (files, sources) = fixture();
        let mut context = AnalysisContext::new(
            &sources,
            draft(&sources, "draft"),
            AnalysisLimits::default(),
        )
        .unwrap();
        let handle = context.origin().unwrap();
        context.capture(&handle, "dep.txt").unwrap();
        let evidence = context.seal();
        assert!(evidence.validate(&sources).is_ok());
        files.write_file("dir/dep.txt", "other bytes");
        assert!(evidence.validate(&sources).is_err());
    }

    #[test]
    fn origin_generations_closure_and_foreign_owners_invalidate() {
        let (files, mut sources) = fixture();
        let evidence = AnalysisContext::new(
            &sources,
            draft(&sources, "draft"),
            AnalysisLimits::default(),
        )
        .unwrap()
        .seal();
        let (_other_files, other) = fixture();
        assert!(evidence.validate(&other).is_err());
        sources
            .selected_mut()
            .unwrap()
            .bind_destination(&files.workspace(), "dir/root.txt")
            .unwrap();
        assert!(evidence.validate(&sources).is_err());
        let evidence = AnalysisContext::new(
            &sources,
            draft(&sources, "draft"),
            AnalysisLimits::default(),
        )
        .unwrap()
        .seal();
        sources.discard_selected().unwrap();
        assert!(evidence.validate(&sources).is_err());
        sources
            .open(
                &files.workspace(),
                &LanguageRegistry::default(),
                "dir/root.txt",
            )
            .unwrap();
        assert!(evidence.validate(&sources).is_err());
    }

    #[test]
    fn local_draft_never_infers_authority_and_later_binding_invalidates() {
        let (files, _) = fixture();
        let mut sources = ScratchBuffers::default();
        sources.create("text", "local");
        let context = AnalysisContext::new(
            &sources,
            draft(&sources, "local draft"),
            AnalysisLimits::default(),
        )
        .unwrap();
        assert!(context.origin().is_none());
        let evidence = context.seal();
        assert!(evidence.validate(&sources).is_ok());
        sources
            .selected_mut()
            .unwrap()
            .bind_destination(&files.workspace(), "dir/root.txt")
            .unwrap();
        assert!(evidence.validate(&sources).is_err());
    }

    #[test]
    fn foreign_handles_and_all_failed_requests_consume_attempts() {
        let (_files, sources) = fixture();
        let mut context = AnalysisContext::new(
            &sources,
            draft(&sources, "draft"),
            AnalysisLimits {
                max_attempts: 8,
                ..AnalysisLimits::default()
            },
        )
        .unwrap();
        let other = AnalysisContext::new(
            &sources,
            draft(&sources, "draft"),
            AnalysisLimits::default(),
        )
        .unwrap();
        assert!(
            context
                .capture(&other.origin().unwrap(), "dep.txt")
                .unwrap_err()
                .contains("foreign")
        );
        let handle = context.origin().unwrap();
        for request in [
            "",
            "/tmp/escape",
            "../escape",
            "sub/../../escape",
            "bad\\path",
            "missing",
            "missing-again",
        ] {
            assert!(context.capture(&handle, request).is_err(), "{request}");
        }
        assert_eq!(context.attempts(), 8);
        assert!(
            context
                .capture(&handle, "dep.txt")
                .unwrap_err()
                .contains("attempt limit")
        );
        let evidence = context.seal();
        assert!(evidence.snapshots().is_empty());
        assert!(evidence.edges().is_empty());
    }

    #[test]
    fn retention_limits_count_utf8_bytes_and_root_and_pinned_reads() {
        let (files, sources) = fixture();
        files.write_file("dir/dep.txt", "éé");
        let editor = draft(&sources, "é");
        assert!(
            AnalysisContext::new(
                &sources,
                editor.clone(),
                AnalysisLimits {
                    max_file_bytes: 1,
                    ..AnalysisLimits::default()
                }
            )
            .is_err()
        );
        let mut context = AnalysisContext::new(
            &sources,
            editor.clone(),
            AnalysisLimits {
                max_total_bytes: 5,
                ..AnalysisLimits::default()
            },
        )
        .unwrap();
        assert!(
            context
                .capture(&context.origin().unwrap(), "dep.txt")
                .is_err()
        );
        let mut context = AnalysisContext::new(
            &sources,
            editor,
            AnalysisLimits {
                max_total_bytes: 6,
                max_snapshots: 1,
                max_attempts: 3,
                ..AnalysisLimits::default()
            },
        )
        .unwrap();
        let handle = context.origin().unwrap();
        let first = context.capture(&handle, "dep.txt").unwrap();
        assert_eq!(first.text().len(), 4);
        assert!(context.capture(&handle, "sub/child.txt").is_err());
        assert!(context.capture(&handle, "dep.txt").is_ok());
        assert!(context.capture(&handle, "dep.txt").is_err());
        assert_eq!(context.seal().snapshots().len(), 1);
    }

    #[test]
    fn path_and_per_file_limits_fail_without_recording_snapshots() {
        let (_files, sources) = fixture();
        for limits in [
            AnalysisLimits {
                max_path_bytes: 3,
                ..AnalysisLimits::default()
            },
            AnalysisLimits {
                max_path_bytes: 7,
                ..AnalysisLimits::default()
            },
            AnalysisLimits {
                max_file_bytes: 5,
                ..AnalysisLimits::default()
            },
            AnalysisLimits {
                max_snapshots: 0,
                ..AnalysisLimits::default()
            },
        ] {
            let mut context = AnalysisContext::new(&sources, draft(&sources, "x"), limits).unwrap();
            assert!(
                context
                    .capture(&context.origin().unwrap(), "dep.txt")
                    .is_err()
            );
            assert!(context.seal().snapshots().is_empty());
        }
    }

    #[test]
    fn stale_or_forged_baselines_are_rejected() {
        let (_files, sources) = fixture();
        let mut editor = draft(&sources, "draft");
        editor.baseline.version += 1;
        assert!(AnalysisContext::new(&sources, editor, AnalysisLimits::default()).is_err());
        let mut editor = draft(&sources, "draft");
        editor.baseline.source.text = "forged".into();
        assert!(AnalysisContext::new(&sources, editor, AnalysisLimits::default()).is_err());
        let mut editor = draft(&sources, "draft");
        editor.baseline.source.source_id = SourceId::fresh();
        assert!(AnalysisContext::new(&sources, editor, AnalysisLimits::default()).is_err());
    }

    #[test]
    fn failed_reads_are_not_pinned_and_success_sequence_keeps_failure_gaps() {
        let (files, sources) = fixture();
        let mut context = AnalysisContext::new(
            &sources,
            draft(&sources, "draft"),
            AnalysisLimits::default(),
        )
        .unwrap();
        let handle = context.origin().unwrap();
        assert!(context.capture(&handle, "new.txt").is_err());
        files.write_bytes("dir/new.txt", &[0xff]);
        assert!(context.capture(&handle, "new.txt").is_err());
        files.write_file("dir/new.txt", "now present");
        assert_eq!(
            context.capture(&handle, "new.txt").unwrap().text(),
            "now present"
        );
        assert!(context.capture(&handle, "./new.txt").is_err());
        let evidence = context.seal();
        assert_eq!(evidence.edges()[0].attempt(), 3);
        assert_eq!(evidence.root_path().unwrap().as_str(), "dir/root.txt");
        assert_eq!(evidence.acquisitions().len(), 4);
        assert!(evidence.acquisitions()[0].outcome().is_err());
        assert!(evidence.acquisitions()[1].outcome().is_err());
        assert!(evidence.acquisitions()[2].outcome().is_ok());
        assert!(evidence.validate(&sources).is_err());
        std::fs::remove_file(files.root().join("dir/new.txt")).unwrap();
        assert!(evidence.validate(&sources).is_err());
    }

    #[test]
    fn missing_optional_observation_is_historical_even_after_file_appears() {
        let (files, sources) = fixture();
        let mut context = AnalysisContext::new(
            &sources,
            draft(&sources, "draft"),
            AnalysisLimits::default(),
        )
        .unwrap();
        assert!(
            context
                .capture(&context.origin().unwrap(), "optional.txt")
                .is_err()
        );
        let evidence = context.seal();
        let record = &evidence.acquisitions()[0];
        assert_eq!(record.attempt(), 1);
        assert_eq!(record.base().as_str(), "dir/root.txt");
        assert_eq!(record.request(), "optional.txt");
        assert_eq!(record.resolved().unwrap().as_str(), "dir/optional.txt");
        assert!(record.outcome().is_err());
        assert!(!evidence.exhausted());
        assert!(evidence.validate(&sources).is_err());
        files.write_file("dir/optional.txt", "appeared");
        assert!(evidence.validate(&sources).is_err());
        assert_eq!(evidence.root().text.as_ref(), "draft");
    }

    #[test]
    fn exhaustion_poison_is_bounded_even_after_only_successful_admitted_reads() {
        let (_files, sources) = fixture();
        for limit in [0, 1] {
            let mut context = AnalysisContext::new(
                &sources,
                draft(&sources, "draft"),
                AnalysisLimits {
                    max_attempts: limit,
                    ..AnalysisLimits::default()
                },
            )
            .unwrap();
            let handle = context.origin().unwrap();
            if limit == 1 {
                context.capture(&handle, "dep.txt").unwrap();
            }
            for _ in 0..100 {
                assert!(context.capture(&handle, "dep.txt").is_err());
            }
            let evidence = context.seal();
            assert!(evidence.exhausted());
            assert_eq!(evidence.attempts(), limit);
            assert_eq!(evidence.acquisitions().len(), limit);
            assert!(
                evidence
                    .acquisitions()
                    .iter()
                    .all(|record| record.outcome().is_ok())
            );
            assert!(evidence.validate(&sources).is_err());
        }
    }

    #[test]
    fn successful_receipts_retain_revision_and_pinned_identity_and_validate() {
        let (_files, sources) = fixture();
        let mut context = AnalysisContext::new(
            &sources,
            draft(&sources, "draft"),
            AnalysisLimits::default(),
        )
        .unwrap();
        let handle = context.origin().unwrap();
        let snapshot = context.capture(&handle, "dep.txt").unwrap();
        context.capture(&handle, "dep.txt").unwrap();
        let evidence = context.seal();
        for (index, record) in evidence.acquisitions().iter().enumerate() {
            assert_eq!(record.attempt(), index + 1);
            assert_eq!(record.request_bytes(), 7);
            assert!(!record.request_truncated());
            let receipt = record.outcome().unwrap();
            assert_eq!(receipt.snapshot_index(), 0);
            assert_eq!(receipt.disk_revision(), snapshot.disk_revision());
            assert!(!receipt.disk_revision().is_empty());
        }
        assert!(evidence.validate(&sources).is_ok());
    }

    #[test]
    fn zero_remaining_budget_rejects_before_read_but_allows_pinned_text() {
        let (_files, sources) = fixture();
        let mut context = AnalysisContext::new(
            &sources,
            draft(&sources, "draft"),
            AnalysisLimits {
                max_total_bytes: 15,
                ..AnalysisLimits::default()
            },
        )
        .unwrap();
        let handle = context.origin().unwrap();
        context.capture(&handle, "dep.txt").unwrap();
        context.capture(&handle, "dep.txt").unwrap();
        // A missing file would produce a Cell read error if I/O were attempted.
        assert!(
            context
                .capture(&handle, "missing.txt")
                .unwrap_err()
                .contains("before read")
        );
        let evidence = context.seal();
        assert_eq!(evidence.snapshots().len(), 1);
        assert_eq!(evidence.acquisitions().len(), 3);
        assert!(evidence.validate(&sources).is_err());
    }

    #[test]
    fn cell_reads_use_minimum_file_retention_and_work_allowance() {
        let (_files, sources) = fixture();
        for (limits, allowance) in [
            (
                AnalysisLimits {
                    max_file_bytes: 8,
                    ..AnalysisLimits::default()
                },
                8,
            ),
            (
                AnalysisLimits {
                    max_total_bytes: 12,
                    ..AnalysisLimits::default()
                },
                7,
            ),
            (
                AnalysisLimits {
                    max_read_bytes: 6,
                    ..AnalysisLimits::default()
                },
                5,
            ),
        ] {
            let mut context =
                AnalysisContext::new(&sources, draft(&sources, "draft"), limits).unwrap();
            // The real Cell file contains ten bytes, exceeding every allowance.
            assert!(
                context
                    .capture(&context.origin().unwrap(), "dep.txt")
                    .is_err()
            );
            let evidence = context.seal();
            assert!(evidence.snapshots().is_empty());
            let record = &evidence.acquisitions()[0];
            assert_eq!(record.read_limit(), Some(allowance));
            assert_eq!(record.read_bytes_charged(), allowance + 1);
            assert_eq!(evidence.read_bytes_charged(), allowance + 1);
            assert!(record.outcome().is_err());
            assert!(evidence.validate(&sources).is_err());
        }
    }

    #[test]
    fn oversized_and_missing_read_failures_spend_worst_case_work_before_io() {
        let (_files, sources) = fixture();
        for request in ["dep.txt", "missing.txt"] {
            let mut context = AnalysisContext::new(
                &sources,
                draft(&sources, "draft"),
                AnalysisLimits {
                    max_file_bytes: 8,
                    max_read_bytes: 18,
                    ..AnalysisLimits::default()
                },
            )
            .unwrap();
            let handle = context.origin().unwrap();
            for _ in 0..2 {
                assert!(context.capture(&handle, request).is_err());
            }
            assert_eq!(context.read_bytes_charged(), 18);
            assert!(
                context
                    .capture(&handle, request)
                    .unwrap_err()
                    .contains("byte-work budget exhausted before read")
            );
            let evidence = context.seal();
            assert_eq!(evidence.acquisitions()[0].read_bytes_charged(), 9);
            assert_eq!(evidence.acquisitions()[1].read_bytes_charged(), 9);
            assert_eq!(evidence.acquisitions()[2].read_limit(), None);
            assert_eq!(evidence.acquisitions()[2].read_bytes_charged(), 0);
            assert_eq!(evidence.read_bytes_charged(), 18);
            assert!(evidence.validate(&sources).is_err());
        }
    }

    #[test]
    fn pinned_reads_do_not_reread_or_spend_work_after_budget_exhaustion() {
        let (files, sources) = fixture();
        files.write_file("dir/dep.txt", "tiny");
        let mut context = AnalysisContext::new(
            &sources,
            draft(&sources, "draft"),
            AnalysisLimits {
                max_file_bytes: 5,
                max_read_bytes: 6,
                ..AnalysisLimits::default()
            },
        )
        .unwrap();
        let handle = context.origin().unwrap();
        let first = context.capture(&handle, "dep.txt").unwrap();
        assert_eq!(context.read_bytes_charged(), 6);
        files.write_file("dir/dep.txt", "this replacement exceeds the read allowance");
        let repeated = context.capture(&handle, "dep.txt").unwrap();
        assert_eq!(repeated.source(), first.source());
        assert_eq!(context.read_bytes_charged(), 6);
        let evidence = context.seal();
        assert_eq!(evidence.acquisitions()[0].read_limit(), Some(5));
        assert_eq!(evidence.acquisitions()[1].read_limit(), None);
        assert_eq!(evidence.acquisitions()[1].read_bytes_charged(), 0);
        assert!(evidence.validate(&sources).is_err());
    }

    #[test]
    fn zero_or_probe_only_work_budget_rejects_without_cell_read() {
        let (_files, sources) = fixture();
        for budget in [0, 1] {
            let mut context = AnalysisContext::new(
                &sources,
                draft(&sources, "draft"),
                AnalysisLimits {
                    max_read_bytes: budget,
                    ..AnalysisLimits::default()
                },
            )
            .unwrap();
            assert!(
                context
                    .capture(&context.origin().unwrap(), "missing.txt")
                    .unwrap_err()
                    .contains("byte-work budget exhausted before read")
            );
            let evidence = context.seal();
            assert_eq!(evidence.acquisitions()[0].read_limit(), None);
            assert_eq!(evidence.read_bytes_charged(), 0);
            assert!(evidence.validate(&sources).is_err());
        }
    }

    #[test]
    fn revalidation_bounds_growth_and_aggregate_edge_work() {
        let (files, sources) = fixture();
        files.write_file("dir/dep.txt", "x");
        let mut context = AnalysisContext::new(
            &sources,
            draft(&sources, "x"),
            AnalysisLimits {
                max_file_bytes: 1,
                max_read_bytes: 4,
                ..AnalysisLimits::default()
            },
        )
        .unwrap();
        let handle = context.origin().unwrap();
        context.capture(&handle, "dep.txt").unwrap();
        let evidence = context.seal();
        assert!(evidence.validate(&sources).is_ok());
        files.write_file("dir/dep.txt", "x".repeat(65536));
        assert!(evidence.validate(&sources).is_err());
        files.write_file("dir/dep.txt", "x");
        assert!(evidence.validate(&sources).is_ok());
        let mut context = AnalysisContext::new(
            &sources,
            draft(&sources, "x"),
            AnalysisLimits {
                max_file_bytes: 1,
                max_read_bytes: 4,
                ..AnalysisLimits::default()
            },
        )
        .unwrap();
        let handle = context.origin().unwrap();
        for _ in 0..3 {
            context.capture(&handle, "dep.txt").unwrap();
        }
        let evidence = context.seal();
        assert_eq!(evidence.read_bytes_charged(), 2);
        assert!(
            evidence
                .validate(&sources)
                .unwrap_err()
                .contains("revalidation byte-work budget exhausted")
        );
    }

    #[test]
    fn oversized_request_evidence_is_bounded_and_utf8_safe() {
        let (_files, sources) = fixture();
        let mut context = AnalysisContext::new(
            &sources,
            draft(&sources, "draft"),
            AnalysisLimits {
                max_path_bytes: 3,
                ..AnalysisLimits::default()
            },
        )
        .unwrap();
        assert!(
            context
                .capture(&context.origin().unwrap(), "ééééé")
                .is_err()
        );
        let evidence = context.seal();
        let record = &evidence.acquisitions()[0];
        assert_eq!(record.request(), "é");
        assert_eq!(record.request_bytes(), 10);
        assert!(record.request_truncated());
        assert!(record.resolved().is_none());
        assert!(record.outcome().is_err());
        assert!(evidence.validate(&sources).is_err());
    }

    #[test]
    fn local_evidence_owner_closure_and_foreign_handle_are_rejected() {
        let (_files, authorized) = fixture();
        let foreign = AnalysisContext::new(
            &authorized,
            draft(&authorized, "draft"),
            AnalysisLimits::default(),
        )
        .unwrap()
        .origin()
        .unwrap();
        let mut sources = ScratchBuffers::default();
        sources.create("text", "local");
        let mut context = AnalysisContext::new(
            &sources,
            draft(&sources, "local"),
            AnalysisLimits::default(),
        )
        .unwrap();
        assert!(context.capture(&foreign, "dep.txt").is_err());
        let evidence = context.seal();
        assert!(evidence.root_path().is_none());
        sources.discard_selected().unwrap();
        assert!(evidence.validate(&sources).is_err());
    }

    #[test]
    #[cfg(unix)]
    fn captured_file_symlink_retarget_is_revalidated_through_original_grant() {
        for outside_mount in [false, true] {
            let (files, sources) = fixture();
            let outside = TempWorkspace::new("analysis_file_retarget");
            outside.write_file("dep.txt", "dependency");
            files.write_file("replacement.txt", "different!");
            let mut context = AnalysisContext::new(
                &sources,
                draft(&sources, "draft"),
                AnalysisLimits::default(),
            )
            .unwrap();
            let handle = context.origin().unwrap();
            let captured = context.capture(&handle, "dep.txt").unwrap();
            assert_eq!(captured.text(), "dependency");
            let evidence = context.seal();
            assert!(evidence.validate(&sources).is_ok());
            std::fs::remove_file(files.root().join("dir/dep.txt")).unwrap();
            let target = if outside_mount {
                outside.root().join("dep.txt")
            } else {
                files.root().join("replacement.txt")
            };
            files.symlink(target, "dir/dep.txt");
            assert!(evidence.validate(&sources).is_err());
            assert_eq!(evidence.snapshots()[0].text(), "dependency");
            assert_eq!(
                evidence.acquisitions()[0]
                    .outcome()
                    .unwrap()
                    .disk_revision(),
                captured.disk_revision()
            );
        }
    }

    #[test]
    #[cfg(unix)]
    fn captured_parent_directory_symlink_retarget_is_revalidated_through_original_grant() {
        for outside_mount in [false, true] {
            let (files, sources) = fixture();
            let outside = TempWorkspace::new("analysis_parent_retarget");
            outside.write_file("child.txt", "child");
            files.write_file("replacement/child.txt", "other");
            let mut context = AnalysisContext::new(
                &sources,
                draft(&sources, "draft"),
                AnalysisLimits::default(),
            )
            .unwrap();
            context
                .capture(&context.origin().unwrap(), "sub/child.txt")
                .unwrap();
            let evidence = context.seal();
            assert!(evidence.validate(&sources).is_ok());
            std::fs::rename(files.root().join("dir/sub"), files.root().join("old-sub")).unwrap();
            let target = if outside_mount {
                outside.root().to_owned()
            } else {
                files.root().join("replacement")
            };
            files.symlink(target, "dir/sub");
            assert!(evidence.validate(&sources).is_err());
            assert_eq!(evidence.snapshots()[0].text(), "child");
        }
    }

    #[test]
    fn same_byte_replacement_preserves_content_revision_but_not_analysis_identity() {
        let (files, sources) = fixture();
        let mut context = AnalysisContext::new(
            &sources,
            draft(&sources, "draft"),
            AnalysisLimits::default(),
        )
        .unwrap();
        let captured = context
            .capture(&context.origin().unwrap(), "dep.txt")
            .unwrap();
        let evidence = context.seal();
        files.write_file("new-dep.txt", "dependency");
        std::fs::rename(
            files.root().join("new-dep.txt"),
            files.root().join("dir/dep.txt"),
        )
        .unwrap();
        // Cell's revision is content-based, not an inode or replacement-generation token.
        assert!(evidence.validate(&sources).is_ok());
        let mut next = AnalysisContext::new(
            &sources,
            draft(&sources, "draft"),
            AnalysisLimits::default(),
        )
        .unwrap();
        let replaced = next.capture(&next.origin().unwrap(), "dep.txt").unwrap();
        let next = next.seal();
        assert_eq!(captured.disk_revision(), replaced.disk_revision());
        assert_eq!(
            evidence.acquisitions()[0]
                .outcome()
                .unwrap()
                .disk_revision(),
            next.acquisitions()[0].outcome().unwrap().disk_revision()
        );
        assert_ne!(captured.source_id(), replaced.source_id());
        assert!(next.validate(&sources).is_ok());
    }

    #[test]
    fn no_origin_has_no_read_grant_even_with_internal_forged_handle() {
        let mut sources = ScratchBuffers::default();
        sources.create("text", "local");
        let mut context = AnalysisContext::new(
            &sources,
            draft(&sources, "local"),
            AnalysisLimits::default(),
        )
        .unwrap();
        assert!(context.origin().is_none());
        // Only this module can forge fields. Even possession of invocation
        // identity cannot reconstruct an absent filesystem grant.
        let forged = SourceHandle(Arc::new(HandleIdentity {
            invocation: context.invocation.clone(),
            path: ScopePath::new("root.txt").unwrap(),
        }));
        assert!(
            context
                .capture(&forged, "dep.txt")
                .unwrap_err()
                .contains("no source origin")
        );
        let evidence = context.seal();
        assert_eq!(evidence.read_bytes_charged(), 0);
        assert_eq!(evidence.acquisitions()[0].read_limit(), None);
        assert!(evidence.acquisitions()[0].outcome().is_err());
        assert!(evidence.validate(&sources).is_err());
    }

    #[test]
    fn invalid_and_foreign_requests_keep_bounded_records_without_io_work() {
        let (_files, sources) = fixture();
        let mut context = AnalysisContext::new(
            &sources,
            draft(&sources, "draft"),
            AnalysisLimits {
                max_path_bytes: 16,
                max_attempts: 4,
                ..AnalysisLimits::default()
            },
        )
        .unwrap();
        let forged = SourceHandle(Arc::new(HandleIdentity {
            invocation: Arc::new(()),
            path: ScopePath::new("forged.txt").unwrap(),
        }));
        let huge = "é".repeat(65536);
        assert!(
            context
                .capture(&forged, &huge)
                .unwrap_err()
                .contains("foreign")
        );
        let handle = context.origin().unwrap();
        for request in [huge.as_str(), "../invalid", "missing.txt"] {
            assert!(context.capture(&handle, request).is_err());
        }
        for _ in 0..32 {
            assert!(context.capture(&handle, &huge).is_err());
        }
        let evidence = context.seal();
        assert_eq!(evidence.acquisitions().len(), 4);
        for record in evidence.acquisitions() {
            assert!(record.request().len() <= 16);
            assert!(record.outcome().unwrap_err().len() < 1024);
        }
        for record in &evidence.acquisitions()[..3] {
            assert_eq!(record.read_limit(), None);
            assert_eq!(record.read_bytes_charged(), 0);
        }
        assert!(evidence.acquisitions()[3].read_bytes_charged() > 0);
        assert!(evidence.exhausted());
        assert!(evidence.validate(&sources).is_err());
    }

    #[test]
    #[cfg(unix)]
    fn cell_rejects_symlink_dependency_escape() {
        let (files, sources) = fixture();
        let outside = TempWorkspace::new("analysis_outside");
        outside.write_file("secret", "secret");
        files.symlink(outside.root().join("secret"), "dir/link");
        let mut context = AnalysisContext::new(
            &sources,
            draft(&sources, "draft"),
            AnalysisLimits::default(),
        )
        .unwrap();
        assert!(context.capture(&context.origin().unwrap(), "link").is_err());
        assert!(context.seal().snapshots().is_empty());
    }
}
