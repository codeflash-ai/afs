use afs_core::AfsResult;
use afs_core::hydration::HydrationRequest;

pub trait HydrationEngine {
    fn queue(&mut self, request: HydrationRequest) -> AfsResult<()>;
    fn drain_ready(&mut self) -> AfsResult<usize>;
}
