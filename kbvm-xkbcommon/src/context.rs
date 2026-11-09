use kbvm::xkb::Context;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;

pub(crate) const NO_DEFAULT_INCLUDES: u32 = 1 << 0;
pub(crate) const NO_ENVIRONMENT_NAMES: u32 = 1 << 1;

pub(crate) struct ContextHandle {
    flags: u32,
    include_paths: Mutex<Vec<PathBuf>>,
}

impl ContextHandle {
    pub(crate) fn new(flags: u32) -> Self {
        Self {
            flags,
            include_paths: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn append_include_path(&self, path: &Path) -> bool {
        if !is_usable_directory(path) {
            return false;
        }
        let mut paths = self
            .include_paths
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        paths.push(path.to_path_buf());
        true
    }

    pub(crate) fn kbvm_context(&self) -> Context {
        let mut builder = Context::builder();
        builder.enable_default_includes(self.flags & NO_DEFAULT_INCLUDES == 0);
        builder.enable_environment(self.flags & NO_ENVIRONMENT_NAMES == 0);
        let paths = self
            .include_paths
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        for path in paths.iter() {
            builder.append_path(path);
        }
        builder.build()
    }
}

fn is_usable_directory(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_dir() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return false;
        }
    }
    std::fs::read_dir(path).is_ok()
}
