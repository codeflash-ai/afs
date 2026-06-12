use std::collections::BTreeMap;

use afs_core::canonical::parse_canonical_markdown;
use afs_core::diff::{BlockDiffEngine, DiffEngine};
use afs_core::model::RemoteId;
use afs_core::planner::{PlanDegradationKind, PropertyValue, PushOperation, PushPlan};
use afs_core::review::{
    ReviewDiff, ReviewLineKind, review_diff_for_create_entity, review_diff_for_existing_document,
};
use afs_core::shadow::ShadowDocument;

#[test]
fn review_diff_annotates_one_paragraph_update() {
    let shadow = shadow("# Roadmap\n\nOld paragraph.", ["heading-1", "paragraph-1"]);
    let local = canonical("", "# Roadmap\n\nNew paragraph.");
    let (plan, parsed) = plan(&shadow, &local);

    let review = review_diff(&shadow, &parsed, &local, &plan);

    assert_eq!(review.hunks.len(), 1);
    assert_operation(&review, "update_block", Some("paragraph-1"));
    assert_line(
        &review,
        ReviewLineKind::Remove,
        "Old paragraph.",
        Some("paragraph-1"),
        Some("update_block"),
    );
    assert_line(
        &review,
        ReviewLineKind::Add,
        "New paragraph.",
        Some("paragraph-1"),
        Some("update_block"),
    );
}

#[test]
fn review_diff_annotates_appended_block() {
    let shadow = shadow(
        "# Roadmap\n\nExisting paragraph.",
        ["heading-1", "paragraph-1"],
    );
    let local = canonical("", "# Roadmap\n\nExisting paragraph.\n\nAdded paragraph.");
    let (plan, parsed) = plan(&shadow, &local);

    let review = review_diff(&shadow, &parsed, &local, &plan);

    assert_operation(&review, "append_block", None);
    assert_line(
        &review,
        ReviewLineKind::Add,
        "Added paragraph.",
        None,
        Some("append_block"),
    );
}

#[test]
fn review_diff_annotates_archived_block() {
    let shadow = shadow(
        "# Roadmap\n\nParagraph to delete.",
        ["heading-1", "paragraph-1"],
    );
    let local = canonical("", "# Roadmap");
    let (plan, parsed) = plan(&shadow, &local);

    let review = review_diff(&shadow, &parsed, &local, &plan);

    assert_operation(&review, "archive_block", Some("paragraph-1"));
    assert_line(
        &review,
        ReviewLineKind::Remove,
        "Paragraph to delete.",
        Some("paragraph-1"),
        Some("archive_block"),
    );
}

#[test]
fn review_diff_annotates_frontmatter_property_update() {
    let old_frontmatter = "afs:\n  id: page-1\n  type: page\n  synced_at: now\n  remote_edited_at: now\ntitle: Roadmap\nStatus: Todo\n";
    let new_frontmatter = "afs:\n  id: page-1\n  type: page\n  synced_at: now\n  remote_edited_at: now\ntitle: Roadmap\nStatus: Done\n";
    let body = "# Roadmap\n";
    let shadow = ShadowDocument::from_synced_body(
        RemoteId::new("page-1"),
        body,
        old_frontmatter.lines().count() + 3,
        [RemoteId::new("heading-1")],
    )
    .expect("shadow")
    .with_frontmatter(old_frontmatter);
    let local = canonical(new_frontmatter, body);
    let (plan, parsed) = plan(&shadow, &local);

    let review = review_diff(&shadow, &parsed, &local, &plan);

    assert_operation(&review, "update_properties", None);
    assert_line(
        &review,
        ReviewLineKind::Remove,
        "Status: Todo",
        None,
        Some("update_properties"),
    );
    assert_line(
        &review,
        ReviewLineKind::Add,
        "Status: Done",
        None,
        Some("update_properties"),
    );
}

#[test]
fn review_diff_for_new_database_row_uses_dev_null_old_label() {
    let local = canonical(
        "title: New task\nStatus: Todo\n",
        "# Notes\n\n- [ ] Wire create\n",
    );
    let plan = PushPlan::new(
        vec![RemoteId::new("database-1")],
        vec![PushOperation::CreateEntity {
            parent_id: RemoteId::new("database-1"),
            title: "New task".to_string(),
            properties: BTreeMap::from([(
                "Status".to_string(),
                PropertyValue::String("Todo".to_string()),
            )]),
            body: "# Notes\n\n- [ ] Wire create\n".to_string(),
            source_path: "Tasks/new-task.md".into(),
        }],
    );

    let review =
        review_diff_for_create_entity("/dev/null", "local:Tasks/new-task.md", &local, &plan);

    assert_eq!(review.old_label, "/dev/null");
    assert_eq!(review.hunks[0].old_start, 0);
    assert_eq!(review.hunks[0].old_lines, 0);
    assert_operation(&review, "create_entity", None);
    assert!(
        review.hunks[0]
            .lines
            .iter()
            .all(|line| line.kind == ReviewLineKind::Add)
    );
}

#[test]
fn ambiguous_alignment_still_produces_review_diff() {
    let shadow = shadow(
        "First paragraph.\n\nSecond paragraph.",
        ["block-1", "block-2"],
    );
    let local = canonical("", "First rewrite.\n\nSecond rewrite.");
    let (plan, parsed) = plan(&shadow, &local);

    let review = review_diff(&shadow, &parsed, &local, &plan);

    assert_eq!(plan.degradations.len(), 1);
    assert_eq!(
        plan.degradations[0].kind,
        PlanDegradationKind::AmbiguousBlockAlignment
    );
    assert!(!review.hunks.is_empty());
    assert_operation(&review, "append_block", None);
    assert_operation(&review, "archive_block", Some("block-1"));
}

fn plan(
    shadow: &ShadowDocument,
    local: &str,
) -> (PushPlan, afs_core::canonical::ParsedCanonicalDocument) {
    let parsed = parse_canonical_markdown(local).expect("parse local");
    let plan = BlockDiffEngine::new()
        .with_edited_body_start_line(parsed.body_start_line)
        .plan_push(shadow, &parsed.document)
        .expect("plan");
    (plan, parsed)
}

fn review_diff(
    shadow: &ShadowDocument,
    parsed: &afs_core::canonical::ParsedCanonicalDocument,
    local: &str,
    plan: &PushPlan,
) -> ReviewDiff {
    review_diff_for_existing_document(
        "synced:Roadmap.md",
        "local:Roadmap.md",
        shadow,
        &parsed.document.body,
        parsed.body_start_line,
        local,
        plan,
    )
}

fn canonical(frontmatter: &str, body: &str) -> String {
    format!("---\n{frontmatter}---\n{body}")
}

fn shadow<const N: usize>(body: &str, ids: [&str; N]) -> ShadowDocument {
    ShadowDocument::from_synced_body(
        RemoteId::new("page-1"),
        body,
        3,
        ids.into_iter().map(RemoteId::new),
    )
    .expect("shadow")
}

fn assert_operation(review: &ReviewDiff, operation_type: &str, block_id: Option<&str>) {
    assert!(
        review.hunks.iter().any(|hunk| hunk
            .operations
            .iter()
            .any(|operation| operation.operation_type == operation_type
                && operation.block_id.as_deref() == block_id)),
        "{review:#?}"
    );
}

fn assert_line(
    review: &ReviewDiff,
    kind: ReviewLineKind,
    text: &str,
    block_id: Option<&str>,
    operation_type: Option<&str>,
) {
    assert!(
        review
            .hunks
            .iter()
            .any(|hunk| hunk.lines.iter().any(|line| {
                line.kind == kind
                    && line.text == text
                    && line.block_id.as_deref() == block_id
                    && line.operation_type.as_deref() == operation_type
            })),
        "{review:#?}"
    );
}
