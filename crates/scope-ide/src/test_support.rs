use std::path::{Path, PathBuf};

use crate::workspace::Workspace;

/// Host-backed fixture directory for tests that need to mount real files into
/// Cell's workspace capability.
///
/// Keeping `std::fs` and `std::env::temp_dir` usage here makes ambient host
/// filesystem access in tests explicit and avoids scattering it through runtime
/// module tests.
pub struct TempWorkspace {
    root: PathBuf,
}

impl TempWorkspace {
    pub fn new(name: &str) -> Self {
        let root = unique_temp_dir(name);
        std::fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn workspace(&self) -> Workspace {
        Workspace::new(self.root.to_string_lossy()).unwrap()
    }

    pub fn create_dir(&self, path: impl AsRef<Path>) {
        std::fs::create_dir_all(self.root.join(path)).unwrap();
    }

    pub fn write_file(&self, path: impl AsRef<Path>, contents: impl AsRef<str>) {
        let path = self.root.join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents.as_ref()).unwrap();
    }

    pub fn write_bytes(&self, path: impl AsRef<Path>, contents: &[u8]) {
        std::fs::write(self.root.join(path), contents).unwrap();
    }

    #[cfg(unix)]
    pub fn symlink(&self, target: impl AsRef<Path>, path: impl AsRef<Path>) {
        std::os::unix::fs::symlink(target, self.root.join(path)).unwrap();
    }

    pub fn read_file(&self, path: impl AsRef<Path>) -> String {
        std::fs::read_to_string(self.root.join(path)).unwrap()
    }

    pub fn host_path_exists(&self, path: impl AsRef<Path>) -> bool {
        self.root.join(path).exists()
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[cfg(unix)]
pub fn symlink_workspace(name: &str, target: &Path) -> TempSymlink {
    let path = unique_temp_dir(name);
    std::os::unix::fs::symlink(target, &path).unwrap();
    TempSymlink { path }
}

#[cfg(unix)]
pub struct TempSymlink {
    path: PathBuf,
}

#[cfg(unix)]
impl TempSymlink {
    pub fn workspace(&self) -> Workspace {
        Workspace::new(self.path.to_string_lossy()).unwrap()
    }
}

#[cfg(unix)]
impl Drop for TempSymlink {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn unique_temp_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "scope-ide-{name}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}
