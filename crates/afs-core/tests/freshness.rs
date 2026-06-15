use afs_core::freshness::{
    ChangeHintKind, FreshnessTier, RemoteVersion, SyncJob, SyncJobCost, SyncJobKind,
    WorkingCopyState, classify_working_copy,
};
use afs_core::model::{MountId, RemoteId};

#[test]
fn remote_versions_are_opaque_stable_values() {
    let version = RemoteVersion::new("2026-06-15T00:00:00.000Z");

    assert_eq!(version.as_str(), "2026-06-15T00:00:00.000Z");
    assert_eq!(
        serde_json::to_string(&version).expect("serialize version"),
        "\"2026-06-15T00:00:00.000Z\""
    );
}

#[test]
fn freshness_tiers_order_by_scheduling_urgency() {
    assert!(FreshnessTier::Immediate.is_more_urgent_than(&FreshnessTier::Hot));
    assert!(FreshnessTier::Hot.is_more_urgent_than(&FreshnessTier::Warm));
    assert!(FreshnessTier::Warm.is_more_urgent_than(&FreshnessTier::Cold));
    assert!(FreshnessTier::Cold.is_more_urgent_than(&FreshnessTier::Dormant));
    assert!(!FreshnessTier::Dormant.is_more_urgent_than(&FreshnessTier::Immediate));
}

#[test]
fn working_copy_state_tracks_local_and_remote_drift() {
    assert_eq!(classify_working_copy(false, false), WorkingCopyState::Clean);
    assert_eq!(
        classify_working_copy(false, true),
        WorkingCopyState::RemoteChanged
    );
    assert_eq!(
        classify_working_copy(true, false),
        WorkingCopyState::LocalPending
    );
    assert_eq!(
        classify_working_copy(true, true),
        WorkingCopyState::Diverged
    );
}

#[test]
fn change_hints_map_to_default_freshness_tiers() {
    assert_eq!(
        ChangeHintKind::PushRequested.recommended_tier(),
        FreshnessTier::Immediate
    );
    assert_eq!(
        ChangeHintKind::LocalEdited.recommended_tier(),
        FreshnessTier::Hot
    );
    assert_eq!(
        ChangeHintKind::DirectoryListed.recommended_tier(),
        FreshnessTier::Warm
    );
    assert_eq!(
        ChangeHintKind::BackgroundPoll.recommended_tier(),
        FreshnessTier::Cold
    );
}

#[test]
fn sync_jobs_carry_cost_and_stable_dedupe_key() {
    let job = SyncJob::new(
        MountId::new("notion-main"),
        Some(RemoteId::new("page-1")),
        SyncJobKind::ObserveEntity,
        ChangeHintKind::LocalEdited,
    );

    assert_eq!(job.tier, FreshnessTier::Hot);
    assert_eq!(job.estimated_cost, SyncJobCost::Cheap);
    assert_eq!(
        job.dedupe_key(),
        "notion-main:page-1:ObserveEntity".to_string()
    );
    assert_eq!(
        SyncJobKind::HydrateEntity.estimated_cost().budget_units(),
        20
    );
}
