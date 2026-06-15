//! Bounded daemon freshness queue.
//!
//! This queue is intentionally connector-neutral. Runtime integration can feed
//! it from file events, directory listings, remote hints, and push requests,
//! while workers drain a small budget into observation/enumeration/hydration
//! jobs.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use afs_core::freshness::SyncJob;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FreshnessQueue {
    jobs: BTreeMap<String, SyncJob>,
}

impl FreshnessQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.jobs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }

    pub fn upsert(&mut self, job: SyncJob) {
        let key = job.dedupe_key();
        let Some(existing) = self.jobs.get_mut(&key) else {
            self.jobs.insert(key, job);
            return;
        };

        if job.tier.is_more_urgent_than(&existing.tier) {
            existing.tier = job.tier.clone();
            existing.reason = job.reason.clone();
        }
        if earlier_next_eligible(
            job.next_eligible_at.as_ref(),
            existing.next_eligible_at.as_ref(),
        ) {
            existing.next_eligible_at = job.next_eligible_at;
        }
    }

    pub fn drain_budget(&mut self, budget_units: u16) -> Vec<SyncJob> {
        self.drain_ready_budget(None, budget_units)
    }

    pub fn drain_ready_budget(&mut self, now: Option<&str>, budget_units: u16) -> Vec<SyncJob> {
        let mut keys = self
            .jobs
            .iter()
            .filter(|(_, job)| is_ready(job, now))
            .map(|(key, job)| (key.clone(), job.clone()))
            .collect::<Vec<_>>();
        keys.sort_by(|(_, left), (_, right)| compare_jobs(left, right));

        let mut remaining = budget_units;
        let mut drained = Vec::new();
        for (key, job) in keys {
            let cost = job.estimated_cost.budget_units();
            if cost > remaining {
                continue;
            }
            remaining -= cost;
            self.jobs.remove(&key);
            drained.push(job);
        }

        drained
    }
}

fn compare_jobs(left: &SyncJob, right: &SyncJob) -> Ordering {
    left.tier
        .cmp(&right.tier)
        .then_with(|| {
            left.estimated_cost
                .budget_units()
                .cmp(&right.estimated_cost.budget_units())
        })
        .then_with(|| left.dedupe_key().cmp(&right.dedupe_key()))
}

fn is_ready(job: &SyncJob, now: Option<&str>) -> bool {
    match (job.next_eligible_at.as_deref(), now) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(next), Some(now)) => next <= now,
    }
}

fn earlier_next_eligible(candidate: Option<&String>, current: Option<&String>) -> bool {
    match (candidate, current) {
        (None, Some(_)) => true,
        (Some(candidate), Some(current)) => candidate < current,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use afs_core::freshness::{ChangeHintKind, FreshnessTier, SyncJob, SyncJobKind};
    use afs_core::model::{MountId, RemoteId};

    use super::FreshnessQueue;

    #[test]
    fn queue_drains_urgent_and_cheap_jobs_within_budget() {
        let mut queue = FreshnessQueue::new();
        queue.upsert(job(
            "page-cold",
            SyncJobKind::HydrateEntity,
            ChangeHintKind::BackgroundPoll,
        ));
        queue.upsert(job(
            "page-hot",
            SyncJobKind::ObserveEntity,
            ChangeHintKind::LocalEdited,
        ));
        queue.upsert(job(
            "page-warm",
            SyncJobKind::EnumerateChildren,
            ChangeHintKind::DirectoryListed,
        ));

        let drained = queue.drain_budget(6);

        assert_eq!(
            drained
                .iter()
                .map(|job| job.remote_id.as_ref().expect("remote id").as_str())
                .collect::<Vec<_>>(),
            vec!["page-hot", "page-warm"]
        );
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn duplicate_jobs_are_promoted_instead_of_repeated() {
        let mut queue = FreshnessQueue::new();
        queue.upsert(job(
            "page-1",
            SyncJobKind::ObserveEntity,
            ChangeHintKind::BackgroundPoll,
        ));
        queue.upsert(job(
            "page-1",
            SyncJobKind::ObserveEntity,
            ChangeHintKind::PushRequested,
        ));

        let drained = queue.drain_budget(1);

        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].tier, FreshnessTier::Immediate);
        assert!(queue.is_empty());
    }

    #[test]
    fn future_jobs_wait_until_eligible() {
        let mut queue = FreshnessQueue::new();
        queue.upsert(
            job(
                "page-1",
                SyncJobKind::ObserveEntity,
                ChangeHintKind::LocalEdited,
            )
            .next_eligible_at("2026-06-15T00:10:00Z"),
        );

        assert!(
            queue
                .drain_ready_budget(Some("2026-06-15T00:09:59Z"), 10)
                .is_empty()
        );
        assert_eq!(
            queue
                .drain_ready_budget(Some("2026-06-15T00:10:00Z"), 10)
                .len(),
            1
        );
    }

    fn job(remote_id: &str, kind: SyncJobKind, reason: ChangeHintKind) -> SyncJob {
        SyncJob::new(
            MountId::new("notion-main"),
            Some(RemoteId::new(remote_id)),
            kind,
            reason,
        )
    }
}
