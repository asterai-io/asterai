use strum_macros::{Display, EnumString};

/// Sync status tag for artifacts (components and environments).
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, Display)]
#[strum(serialize_all = "lowercase")]
pub enum ArtifactSyncTag {
    /// Exists locally but not pushed to remote.
    Unpushed,
    /// Exists both locally and on remote at the same version.
    Synced,
    /// Exists locally but remote has a newer version.
    Behind,
    /// Exists only on remote, not cached locally.
    Remote,
}
