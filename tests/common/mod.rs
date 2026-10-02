//! Helpers shared by the integration tests.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// An empty user-level config the tests own, so no audit reads the
/// developer's own `~/.anc.toml`. The file exists, so `anc` has no missing
/// `AGENTNATIVE_HOME_CONFIG` file to warn about on stderr.
pub fn empty_home_config() -> &'static Path {
    static FILE: OnceLock<PathBuf> = OnceLock::new();
    FILE.get_or_init(|| {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("empty-home");
        std::fs::create_dir_all(&dir).expect("create the empty home directory");
        let file = dir.join(".anc.toml");
        std::fs::write(&file, "").expect("write the empty home config");
        file
    })
}
