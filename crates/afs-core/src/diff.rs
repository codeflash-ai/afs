use crate::model::CanonicalDocument;
use crate::planner::PushPlan;
use crate::{AfsError, AfsResult};

pub trait DiffEngine {
    fn plan_push(
        &self,
        shadow: &CanonicalDocument,
        edited: &CanonicalDocument,
    ) -> AfsResult<PushPlan>;
}

#[derive(Clone, Debug, Default)]
pub struct StubDiffEngine;

impl DiffEngine for StubDiffEngine {
    fn plan_push(
        &self,
        _shadow: &CanonicalDocument,
        _edited: &CanonicalDocument,
    ) -> AfsResult<PushPlan> {
        Err(AfsError::NotImplemented("block-aware diff engine"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AlignmentPass {
    Exact,
    Structural,
    Residual,
}
