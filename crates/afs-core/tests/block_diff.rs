use afs_core::AfsError;
use afs_core::diff::{BlockDiffEngine, DiffEngine};
use afs_core::model::{CanonicalDocument, RemoteId};
use afs_core::planner::{PlanDegradationKind, PushOperation};
use afs_core::shadow::{MarkdownBlockKind, ShadowDocument, segment_markdown_body};
use afs_core::special::{StructuredWriteTarget, TableRowUpdate};

#[test]
fn segments_common_markdown_blocks_with_source_spans() {
    let body = "# Title\n\nParagraph text.\n\n- one\n- two\n\n```rust\nfn main() {}\n```\n\n| A | B |\n| - | - |\n| 1 | 2 |\n\n::afs{id=media-1 type=image title=\"Diagram\"}\n";
    let blocks = segment_markdown_body(body, 10);

    assert_eq!(blocks.len(), 6);
    assert_eq!(blocks[0].kind, MarkdownBlockKind::Heading);
    assert_eq!(blocks[0].source_span.start_line, 10);
    assert_eq!(blocks[1].kind, MarkdownBlockKind::Paragraph);
    assert_eq!(blocks[2].kind, MarkdownBlockKind::List);
    assert_eq!(blocks[3].kind, MarkdownBlockKind::CodeFence);
    assert_eq!(blocks[4].kind, MarkdownBlockKind::Table);
    assert!(matches!(
        blocks[5].kind,
        MarkdownBlockKind::Directive {
            directive_type: Some(_),
            malformed: false,
            ..
        }
    ));
    assert_eq!(blocks[5].remote_id, Some(RemoteId::new("media-1")));
}

#[test]
fn editing_one_paragraph_produces_one_block_update() {
    let shadow = shadow("# Roadmap\n\nOld paragraph.", ["heading-1", "paragraph-1"]);
    let edited = CanonicalDocument::new("", "# Roadmap\n\nNew paragraph.");

    let plan = BlockDiffEngine::new()
        .plan_push(&shadow, &edited)
        .expect("plan");

    assert_eq!(
        plan.operations,
        vec![PushOperation::UpdateBlock {
            block_id: RemoteId::new("paragraph-1"),
            content: "New paragraph.".to_string(),
        }]
    );
    assert_eq!(plan.summary.blocks_updated, 1);
    assert!(plan.degradations.is_empty());
}

#[test]
fn appending_a_paragraph_produces_append_after_last_existing_block() {
    let shadow = shadow(
        "# Roadmap\n\nExisting paragraph.",
        ["heading-1", "paragraph-1"],
    );
    let edited = CanonicalDocument::new("", "# Roadmap\n\nExisting paragraph.\n\nAdded paragraph.");

    let plan = BlockDiffEngine::new()
        .plan_push(&shadow, &edited)
        .expect("plan");

    assert_eq!(
        plan.operations,
        vec![PushOperation::AppendBlock {
            parent_id: RemoteId::new("page-1"),
            after: Some(RemoteId::new("paragraph-1")),
            content: "Added paragraph.".to_string(),
        }]
    );
    assert_eq!(plan.summary.blocks_created, 1);
}

#[test]
fn deleting_a_normal_paragraph_produces_archive() {
    let shadow = shadow(
        "# Roadmap\n\nParagraph to delete.",
        ["heading-1", "paragraph-1"],
    );
    let edited = CanonicalDocument::new("", "# Roadmap");

    let plan = BlockDiffEngine::new()
        .plan_push(&shadow, &edited)
        .expect("plan");

    assert_eq!(
        plan.operations,
        vec![PushOperation::ArchiveBlock {
            block_id: RemoteId::new("paragraph-1"),
        }]
    );
    assert_eq!(plan.summary.blocks_archived, 1);
}

#[test]
fn moving_an_unchanged_directive_produces_move_not_validation_error() {
    let shadow = shadow(
        "Intro paragraph.\n\n::afs{id=media-1 type=image title=\"Diagram\"}",
        ["paragraph-1"],
    );
    let edited = CanonicalDocument::new(
        "",
        "::afs{id=media-1 type=image title=\"Diagram\"}\n\nIntro paragraph.",
    );

    let plan = BlockDiffEngine::new()
        .plan_push(&shadow, &edited)
        .expect("plan");

    assert_eq!(
        plan.operations,
        vec![PushOperation::MoveBlock {
            block_id: RemoteId::new("media-1"),
            after: None,
        }]
    );
}

#[test]
fn editing_a_directive_fails_validation_instead_of_planning() {
    let shadow = shadow(
        "Intro paragraph.\n\n::afs{id=media-1 type=image title=\"Diagram\"}",
        ["paragraph-1"],
    );
    let edited = CanonicalDocument::new(
        "",
        "Intro paragraph.\n\n::afs{id=media-1 type=image title=\"Edited\"}",
    );

    let error = BlockDiffEngine::new()
        .plan_push(&shadow, &edited)
        .expect_err("directive edit should fail");

    let AfsError::Validation(issues) = error else {
        panic!("expected validation error");
    };
    assert_eq!(issues[0].code, "directive_mangled");
}

#[test]
fn ambiguous_residual_alignment_is_explicitly_degraded() {
    let shadow = shadow(
        "First paragraph.\n\nSecond paragraph.",
        ["block-1", "block-2"],
    );
    let edited = CanonicalDocument::new("", "First rewrite.\n\nSecond rewrite.");

    let plan = BlockDiffEngine::new()
        .plan_push(&shadow, &edited)
        .expect("plan");

    assert_eq!(plan.summary.blocks_updated, 0);
    assert_eq!(plan.summary.blocks_created, 2);
    assert_eq!(plan.summary.blocks_archived, 2);
    assert_eq!(plan.degradations.len(), 1);
    assert_eq!(
        plan.degradations[0].kind,
        PlanDegradationKind::AmbiguousBlockAlignment
    );
}

#[test]
fn editing_table_cells_produces_structured_row_updates() {
    let shadow = table_shadow(
        "| Decision | Choice |\n| --- | --- |\n| First connector | Notion |",
        ["row-1", "row-2"],
        true,
    );
    let edited = CanonicalDocument::new(
        "",
        "| Decision | Choice |\n| --- | --- |\n| First connector | AFS |",
    );

    let plan = BlockDiffEngine::new()
        .plan_push(&shadow, &edited)
        .expect("plan");

    assert_eq!(
        plan.operations,
        vec![PushOperation::UpdateStructuredBlock {
            block_id: RemoteId::new("table-1"),
            target: StructuredWriteTarget::TableRows {
                rows: vec![TableRowUpdate {
                    row_id: RemoteId::new("row-2"),
                    cells: vec!["First connector".to_string(), "AFS".to_string()],
                }],
            },
        }]
    );
    assert_eq!(plan.summary.blocks_updated, 1);
}

#[test]
fn table_without_column_header_skips_synthetic_header_row() {
    let shadow = table_shadow(
        "|  |  |\n| --- | --- |\n| First connector | Notion |",
        ["row-1"],
        false,
    );
    let edited = CanonicalDocument::new("", "|  |  |\n| --- | --- |\n| First connector | AFS |");

    let plan = BlockDiffEngine::new()
        .plan_push(&shadow, &edited)
        .expect("plan");

    assert_eq!(
        plan.operations,
        vec![PushOperation::UpdateStructuredBlock {
            block_id: RemoteId::new("table-1"),
            target: StructuredWriteTarget::TableRows {
                rows: vec![TableRowUpdate {
                    row_id: RemoteId::new("row-1"),
                    cells: vec!["First connector".to_string(), "AFS".to_string()],
                }],
            },
        }]
    );
}

#[test]
fn table_row_count_changes_fail_before_apply() {
    let shadow = table_shadow(
        "| Decision | Choice |\n| --- | --- |\n| First connector | Notion |",
        ["row-1", "row-2"],
        true,
    );
    let edited = CanonicalDocument::new(
        "",
        "| Decision | Choice |\n| --- | --- |\n| First connector | Notion |\n| Second connector | Linear |",
    );

    let error = BlockDiffEngine::new()
        .plan_push(&shadow, &edited)
        .expect_err("row add");

    let AfsError::Validation(issues) = error else {
        panic!("expected validation error");
    };
    assert_eq!(issues[0].code, "table_row_count_changed");
}

#[test]
fn editing_synthetic_table_header_fails_before_apply() {
    let shadow = table_shadow(
        "|  |  |\n| --- | --- |\n| First connector | Notion |",
        ["row-1"],
        false,
    );
    let edited = CanonicalDocument::new(
        "",
        "| Label | Value |\n| --- | --- |\n| First connector | Notion |",
    );

    let error = BlockDiffEngine::new()
        .plan_push(&shadow, &edited)
        .expect_err("synthetic header edit");

    let AfsError::Validation(issues) = error else {
        panic!("expected validation error");
    };
    assert_eq!(issues[0].code, "table_synthetic_header_edited");
}

fn shadow<const N: usize>(body: &str, ids: [&str; N]) -> ShadowDocument {
    ShadowDocument::from_synced_body(
        RemoteId::new("page-1"),
        body,
        1,
        ids.into_iter().map(RemoteId::new),
    )
    .expect("shadow")
}

fn table_shadow<const N: usize>(
    body: &str,
    row_ids: [&str; N],
    has_column_header: bool,
) -> ShadowDocument {
    let mut shadow = ShadowDocument::from_synced_body(
        RemoteId::new("page-1"),
        body,
        1,
        [RemoteId::new("table-1")],
    )
    .expect("shadow");
    shadow.blocks[0].kind = MarkdownBlockKind::TableWithRows {
        row_ids: row_ids.into_iter().map(RemoteId::new).collect(),
        has_column_header,
        has_row_header: false,
    };
    shadow
}
