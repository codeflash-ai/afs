use afs_core::freshness::{FreshnessTier, RemoteVersion, WorkingCopyState, classify_working_copy};

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
