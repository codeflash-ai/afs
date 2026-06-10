use std::collections::{BTreeMap, VecDeque};

use afs_core::AfsResult;
use afs_core::hydration::{HydrationReason, HydrationRequest};
use afs_core::model::{HydrationState, MountId, RemoteId};

pub trait HydrationEngine {
    fn queue(&mut self, request: HydrationRequest) -> AfsResult<()>;
    fn drain_ready(&mut self) -> AfsResult<usize>;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HydrationQueue {
    order: VecDeque<HydrationKey>,
    pending: BTreeMap<HydrationKey, HydrationRequest>,
}

impl HydrationQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn queue_request(&mut self, request: HydrationRequest) -> bool {
        let key = HydrationKey::from_request(&request);
        let inserted = !self.pending.contains_key(&key);

        if inserted {
            self.order.push_back(key.clone());
            self.pending.insert(key, request);
            return true;
        }

        if let Some(existing) = self.pending.get_mut(&key) {
            merge_request(existing, request);
        }

        false
    }

    pub fn peek_ready(&self) -> Option<&HydrationRequest> {
        let key = self.next_ready_key()?;
        self.pending.get(key)
    }

    pub fn pop_ready(&mut self) -> Option<HydrationRequest> {
        let index = self.next_ready_index()?;
        let key = self.order.remove(index)?;
        self.pending.remove(&key)
    }

    pub fn drain_ready_with(
        &mut self,
        mut hydrate: impl FnMut(HydrationRequest) -> AfsResult<()>,
    ) -> AfsResult<usize> {
        let mut drained = 0;

        while let Some(request) = self.pop_ready() {
            if let Err(error) = hydrate(request.clone()) {
                self.queue_request(request);
                return Err(error);
            }

            drained += 1;
        }

        Ok(drained)
    }

    fn next_ready_key(&self) -> Option<&HydrationKey> {
        self.next_ready_index()
            .and_then(|index| self.order.get(index))
    }

    fn next_ready_index(&self) -> Option<usize> {
        let mut best: Option<(usize, HydrationPriority)> = None;

        for (index, key) in self.order.iter().enumerate() {
            let Some(request) = self.pending.get(key) else {
                continue;
            };
            let priority = hydration_priority(&request.reason);

            if best
                .as_ref()
                .is_none_or(|(_, best_priority)| priority > *best_priority)
            {
                best = Some((index, priority));
            }
        }

        best.map(|(index, _)| index)
    }
}

impl HydrationEngine for HydrationQueue {
    fn queue(&mut self, request: HydrationRequest) -> AfsResult<()> {
        self.queue_request(request);
        Ok(())
    }

    fn drain_ready(&mut self) -> AfsResult<usize> {
        let count = self.pending.len();
        self.pending.clear();
        self.order.clear();
        Ok(count)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum HydrationPriority {
    Low,
    Normal,
    High,
}

pub fn hydration_priority(reason: &HydrationReason) -> HydrationPriority {
    match reason {
        HydrationReason::ExplicitPull | HydrationReason::StubRead => HydrationPriority::High,
        HydrationReason::Policy => HydrationPriority::Normal,
        HydrationReason::Prefetch => HydrationPriority::Low,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct HydrationKey {
    mount_id: MountId,
    remote_id: RemoteId,
}

impl HydrationKey {
    fn from_request(request: &HydrationRequest) -> Self {
        Self {
            mount_id: request.mount_id.clone(),
            remote_id: request.remote_id.clone(),
        }
    }
}

fn merge_request(existing: &mut HydrationRequest, incoming: HydrationRequest) {
    let existing_priority = hydration_priority(&existing.reason);
    let incoming_priority = hydration_priority(&incoming.reason);
    let target_state = strongest_target_state(&existing.target_state, &incoming.target_state);

    if incoming_priority > existing_priority {
        existing.path = incoming.path;
        existing.reason = incoming.reason;
    }

    existing.target_state = target_state;
}

fn strongest_target_state(current: &HydrationState, incoming: &HydrationState) -> HydrationState {
    if hydration_target_rank(incoming) > hydration_target_rank(current) {
        incoming.clone()
    } else {
        current.clone()
    }
}

fn hydration_target_rank(state: &HydrationState) -> u8 {
    match state {
        HydrationState::Virtual => 0,
        HydrationState::Stub => 1,
        HydrationState::Hydrated => 2,
        HydrationState::Dirty => 3,
        HydrationState::Conflicted => 4,
    }
}
