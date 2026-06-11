//! Conflict data structures and block-overlap helpers.
//!
//! The core preserves local content when conflicts occur. Higher layers can
//! materialize `.remote.md` files and drive `afs resolve`, but the collision
//! decision is deterministic and lives here.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::model::RemoteId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictSummary {
    pub remote_id: RemoteId,
    pub path: PathBuf,
    pub remote_path: PathBuf,
    pub reason: ConflictReason,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConflictReason {
    LocalAndRemoteChanged,
    SameBlockChanged,
    RemoteMovedDuringPush,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConflictResolution {
    Ours,
    Theirs,
    Edited(PathBuf),
}

pub fn remote_variant_path(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("md") => path.with_extension("remote.md"),
        Some(extension) => path.with_extension(format!("remote.{extension}")),
        None => path.with_extension("remote"),
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BlockChangeSet {
    changed_blocks: BTreeSet<RemoteId>,
    has_structural_change: bool,
}

impl BlockChangeSet {
    pub fn from_blocks(blocks: impl IntoIterator<Item = RemoteId>) -> Self {
        Self {
            changed_blocks: blocks.into_iter().collect(),
            has_structural_change: false,
        }
    }

    pub fn structural() -> Self {
        Self {
            changed_blocks: BTreeSet::new(),
            has_structural_change: true,
        }
    }

    pub fn with_structural_change(mut self) -> Self {
        self.has_structural_change = true;
        self
    }

    pub fn is_disjoint(&self, other: &Self) -> bool {
        !self.has_structural_change
            && !other.has_structural_change
            && self.changed_blocks.is_disjoint(&other.changed_blocks)
    }

    pub fn len(&self) -> usize {
        self.changed_blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.changed_blocks.is_empty() && !self.has_structural_change
    }
}
