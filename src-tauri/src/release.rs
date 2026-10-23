//! Which Writer this is, baked in when it's built: the release workflow sets
//! `WRITER_BUILD` to its tag ("v0.0.0", "nightly-1") and `WRITER_RELEASE=1`;
//! a build without them is a dev build, named by its version, which never
//! updates itself.

use serde::Serialize;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The tag it was built from, if the release workflow built it.
pub fn tag() -> Option<&'static str> {
    option_env!("WRITER_BUILD").filter(|tag| !tag.trim().is_empty())
}

/// Built by the release workflow: only these update themselves.
pub fn is_release() -> bool {
    option_env!("WRITER_RELEASE") == Some("1")
}

/// "Writer nightly-1", "Writer v0.0.0", or "Writer 0.0.0 (dev)".
pub fn name() -> String {
    match tag() {
        Some(tag) => format!("Writer {tag}"),
        None => format!("Writer {VERSION} (dev)"),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct About {
    pub name: String,
    pub version: &'static str,
    pub tag: Option<&'static str>,
    pub release: bool,
}

pub fn about() -> About {
    About { name: name(), version: VERSION, tag: tag(), release: is_release() }
}
