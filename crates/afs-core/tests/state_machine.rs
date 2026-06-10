use afs_core::model::{HydrationState, RemoteId};
use afs_core::sync::{SyncDecision, classify};

#[test]
fn hydration_ladder_allows_expected_transitions() {
    assert!(HydrationState::Virtual.can_transition_to(&HydrationState::Stub));
    assert!(HydrationState::Stub.can_transition_to(&HydrationState::Hydrated));
    assert!(HydrationState::Hydrated.can_transition_to(&HydrationState::Dirty));
    assert!(HydrationState::Dirty.can_transition_to(&HydrationState::Conflicted));
    assert!(!HydrationState::Conflicted.can_transition_to(&HydrationState::Dirty));
}

#[test]
fn three_tree_change_classification_matches_push_pull_model() {
    let id = RemoteId("page-1".to_string());

    assert_eq!(classify(false, false, id.clone()), SyncDecision::Noop);
    assert_eq!(
        classify(true, false, id.clone()),
        SyncDecision::Pull {
            remote_id: id.clone()
        }
    );
    assert_eq!(
        classify(false, true, id.clone()),
        SyncDecision::Push {
            remote_id: id.clone()
        }
    );
    assert_eq!(
        classify(true, true, id.clone()),
        SyncDecision::Conflict { remote_id: id }
    );
}
