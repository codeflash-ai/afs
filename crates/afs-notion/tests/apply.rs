use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;

use afs_connector::{ApplyPlanRequest, Connector};
use afs_core::journal::{JournalApplyEffect, PushId, PushOperationId};
use afs_core::model::{MountId, RemoteId};
use afs_core::planner::{PushOperation, PushPlan};
use afs_core::push::RemotePrecondition;
use afs_core::{AfsError, AfsResult};
use afs_notion::client::NotionApi;
use afs_notion::dto::{
    BlockDto, BlockListDto, DateMentionDto, LinkDto, MentionRichTextDto, PageDto, PageListDto,
    PaginatedListDto, RichTextAnnotationsDto, RichTextBlockDto, RichTextDto, TextRichTextDto,
};
use afs_notion::{NotionConfig, NotionConnector};
use serde_json::{Value, json};

#[test]
fn apply_updates_appends_and_archives_supported_blocks() {
    let api = Arc::new(RecordingNotionApi::new("2026-06-10T00:00:00.000Z", false));
    let connector = NotionConnector::with_api(NotionConfig::default(), api.clone());
    let plan = PushPlan::new(
        vec![RemoteId::new("page-1")],
        vec![
            PushOperation::UpdateBlock {
                block_id: RemoteId::new("paragraph-1"),
                content: "Changed paragraph.".to_string(),
            },
            PushOperation::AppendBlock {
                parent_id: RemoteId::new("page-1"),
                after: Some(RemoteId::new("paragraph-1")),
                content: "- New bullet".to_string(),
            },
            PushOperation::ArchiveBlock {
                block_id: RemoteId::new("old-block"),
            },
        ],
    );
    let push_id = PushId("push-1".to_string());
    let operation_ids = operation_ids(&push_id, &plan);
    let mount_id = MountId::new("notion-main");
    let preconditions = vec![RemotePrecondition {
        remote_id: RemoteId::new("page-1"),
        remote_edited_at: Some("2026-06-10T00:00:00.000Z".to_string()),
    }];

    connector
        .check_concurrency(ApplyPlanRequest {
            push_id: &push_id,
            mount_id: &mount_id,
            plan: &plan,
            operation_ids: &operation_ids,
            remote_preconditions: &preconditions,
        })
        .expect("concurrency");
    let result = connector
        .apply(ApplyPlanRequest {
            push_id: &push_id,
            mount_id: &mount_id,
            plan: &plan,
            operation_ids: &operation_ids,
            remote_preconditions: &preconditions,
        })
        .expect("apply");

    assert_eq!(result.changed_remote_ids, vec![RemoteId::new("page-1")]);
    assert_eq!(
        result.effects,
        vec![
            JournalApplyEffect::UpdatedBlock {
                operation_id: operation_ids[0].clone(),
                operation_index: 0,
                block_id: RemoteId::new("paragraph-1"),
            },
            JournalApplyEffect::CreatedBlock {
                operation_id: operation_ids[1].clone(),
                operation_index: 1,
                parent_id: RemoteId::new("page-1"),
                block_id: RemoteId::new("created-1"),
            },
            JournalApplyEffect::ArchivedBlock {
                operation_id: operation_ids[2].clone(),
                operation_index: 2,
                block_id: RemoteId::new("old-block"),
            },
        ]
    );

    let writes = api.writes.lock().expect("writes");
    assert_eq!(
        writes.as_slice(),
        [
            WriteCall::Update {
                block_id: "paragraph-1".to_string(),
                body: json!({
                    "paragraph": {
                        "rich_text": rich_text_json("Changed paragraph."),
                    },
                }),
            },
            WriteCall::Append {
                block_id: "page-1".to_string(),
                body: json!({
                    "children": [{
                        "object": "block",
                        "type": "bulleted_list_item",
                        "bulleted_list_item": {
                            "rich_text": rich_text_json("New bullet"),
                        },
                    }],
                    "position": {
                        "type": "after_block",
                        "after_block": "paragraph-1",
                    },
                }),
            },
            WriteCall::Delete {
                block_id: "old-block".to_string(),
            },
        ]
    );
}

#[test]
fn apply_uses_start_position_and_chains_adjacent_new_blocks() {
    let api = Arc::new(RecordingNotionApi::new("2026-06-10T00:00:00.000Z", false));
    let connector = NotionConnector::with_api(NotionConfig::default(), api.clone());
    let plan = PushPlan::new(
        vec![RemoteId::new("page-1")],
        vec![
            PushOperation::AppendBlock {
                parent_id: RemoteId::new("page-1"),
                after: None,
                content: "First new paragraph.".to_string(),
            },
            PushOperation::AppendBlock {
                parent_id: RemoteId::new("page-1"),
                after: None,
                content: "Second new paragraph.".to_string(),
            },
        ],
    );
    let push_id = PushId("push-1".to_string());
    let operation_ids = operation_ids(&push_id, &plan);
    let mount_id = MountId::new("notion-main");

    connector
        .apply(ApplyPlanRequest {
            push_id: &push_id,
            mount_id: &mount_id,
            plan: &plan,
            operation_ids: &operation_ids,
            remote_preconditions: &[],
        })
        .expect("apply");

    let writes = api.writes.lock().expect("writes");
    assert_eq!(
        writes[0],
        WriteCall::Append {
            block_id: "page-1".to_string(),
            body: json!({
                "children": [{
                    "object": "block",
                    "type": "paragraph",
                    "paragraph": {
                        "rich_text": rich_text_json("First new paragraph."),
                    },
                }],
                "position": {
                    "type": "start",
                },
            }),
        }
    );
    assert_eq!(
        writes[1],
        WriteCall::Append {
            block_id: "page-1".to_string(),
            body: json!({
                "children": [{
                    "object": "block",
                    "type": "paragraph",
                    "paragraph": {
                        "rich_text": rich_text_json("Second new paragraph."),
                    },
                }],
                "position": {
                    "type": "after_block",
                    "after_block": "created-1",
                },
            }),
        }
    );
}

#[test]
fn check_concurrency_rejects_remote_timestamp_mismatch() {
    let api = Arc::new(RecordingNotionApi::new("2026-06-10T01:00:00.000Z", false));
    let connector = NotionConnector::with_api(NotionConfig::default(), api.clone());
    let plan = PushPlan::new(vec![RemoteId::new("page-1")], Vec::new());
    let push_id = PushId("push-1".to_string());
    let mount_id = MountId::new("notion-main");
    let preconditions = vec![RemotePrecondition {
        remote_id: RemoteId::new("page-1"),
        remote_edited_at: Some("2026-06-10T00:00:00.000Z".to_string()),
    }];

    let error = connector
        .check_concurrency(ApplyPlanRequest {
            push_id: &push_id,
            mount_id: &mount_id,
            plan: &plan,
            operation_ids: &[],
            remote_preconditions: &preconditions,
        })
        .expect_err("stale remote");

    assert!(matches!(error, AfsError::Guardrail(_)));
}

#[test]
fn apply_preserves_unchanged_mentions_and_parses_edited_rich_spans() {
    let api = Arc::new(RecordingNotionApi::with_paragraph_rich_text(
        "2026-06-10T00:00:00.000Z",
        vec![
            annotated_text("Bold", |annotations| annotations.bold = true),
            rich_text_part(" and "),
            date_mention("2026-06-10", "2026-06-10"),
            rich_text_part(" plus "),
            linked_text("Docs", "https://example.com/"),
            rich_text_part("."),
        ],
    ));
    let connector = NotionConnector::with_api(NotionConfig::default(), api.clone());
    let plan = PushPlan::new(
        vec![RemoteId::new("page-1")],
        vec![PushOperation::UpdateBlock {
            block_id: RemoteId::new("paragraph-1"),
            content: "**Boldly** and 2026-06-10 plus [Docs](https://example.com/) and $E=mc^2$ [Roadmap](afs://page-2)".to_string(),
        }],
    );
    let push_id = PushId("push-1".to_string());
    let operation_ids = operation_ids(&push_id, &plan);
    let mount_id = MountId::new("notion-main");

    let result = connector
        .apply(ApplyPlanRequest {
            push_id: &push_id,
            mount_id: &mount_id,
            plan: &plan,
            operation_ids: &operation_ids,
            remote_preconditions: &[],
        })
        .expect("apply");

    assert_eq!(result.changed_remote_ids, vec![RemoteId::new("page-1")]);
    let writes = api.writes.lock().expect("writes");
    assert_eq!(
        writes.as_slice(),
        [WriteCall::Update {
            block_id: "paragraph-1".to_string(),
            body: json!({
                "paragraph": {
                    "rich_text": [
                        {
                            "type": "text",
                            "text": {
                                "content": "Boldly",
                            },
                            "annotations": {
                                "bold": true,
                                "italic": false,
                                "strikethrough": false,
                                "underline": false,
                                "code": false,
                                "color": "default",
                            },
                        },
                        {
                            "type": "text",
                            "text": {
                                "content": " and ",
                            },
                        },
                        {
                            "type": "mention",
                            "mention": {
                                "type": "date",
                                "date": {
                                    "start": "2026-06-10",
                                },
                            },
                        },
                        {
                            "type": "text",
                            "text": {
                                "content": " plus ",
                            },
                        },
                        {
                            "type": "text",
                            "text": {
                                "content": "Docs",
                                "link": {
                                    "url": "https://example.com/",
                                },
                            },
                        },
                        {
                            "type": "text",
                            "text": {
                                "content": " and ",
                            },
                        },
                        {
                            "type": "equation",
                            "equation": {
                                "expression": "E=mc^2",
                            },
                        },
                        {
                            "type": "text",
                            "text": {
                                "content": " ",
                            },
                        },
                        {
                            "type": "mention",
                            "mention": {
                                "type": "page",
                                "page": {
                                    "id": "page-2",
                                },
                            },
                        },
                    ],
                },
            }),
        }]
    );
}

fn operation_ids(push_id: &PushId, plan: &PushPlan) -> Vec<PushOperationId> {
    plan.operations
        .iter()
        .enumerate()
        .map(|(index, operation)| PushOperationId::for_operation(push_id, index, operation))
        .collect()
}

#[derive(Debug)]
struct RecordingNotionApi {
    page: PageDto,
    children: BTreeMap<(String, Option<String>), BlockListDto>,
    writes: Mutex<Vec<WriteCall>>,
    append_count: Mutex<usize>,
}

impl RecordingNotionApi {
    fn new(last_edited_time: &str, rich_paragraph: bool) -> Self {
        let rich_text = if rich_paragraph {
            vec![annotated_text("Old paragraph.", |annotations| {
                annotations.bold = true;
            })]
        } else {
            rich_text("Old paragraph.")
        };
        Self::with_paragraph_rich_text(last_edited_time, rich_text)
    }

    fn with_paragraph_rich_text(last_edited_time: &str, rich_text: Vec<RichTextDto>) -> Self {
        let page = PageDto {
            id: "page-1".to_string(),
            created_time: Some("2026-06-10T00:00:00.000Z".to_string()),
            last_edited_time: Some(last_edited_time.to_string()),
            archived: false,
            in_trash: false,
            properties: BTreeMap::new(),
        };
        let children = BTreeMap::from([(
            ("page-1".to_string(), None),
            PaginatedListDto {
                results: vec![
                    paragraph_block_with_rich_text("paragraph-1", rich_text),
                    paragraph_block("old-block", "Old block.", false),
                ],
                next_cursor: None,
                has_more: false,
            },
        )]);

        Self {
            page,
            children,
            writes: Mutex::new(Vec::new()),
            append_count: Mutex::new(0),
        }
    }
}

impl NotionApi for RecordingNotionApi {
    fn retrieve_page(&self, page_id: &str) -> AfsResult<PageDto> {
        if page_id == self.page.id {
            Ok(self.page.clone())
        } else {
            Err(AfsError::InvalidState(format!("missing page {page_id}")))
        }
    }

    fn retrieve_block_children(
        &self,
        block_id: &str,
        start_cursor: Option<&str>,
    ) -> AfsResult<BlockListDto> {
        Ok(self
            .children
            .get(&(block_id.to_string(), start_cursor.map(str::to_string)))
            .cloned()
            .unwrap_or_default())
    }

    fn search_pages(&self, _start_cursor: Option<&str>) -> AfsResult<PageListDto> {
        Ok(PaginatedListDto {
            results: vec![self.page.clone()],
            next_cursor: None,
            has_more: false,
        })
    }

    fn update_block(&self, block_id: &str, body: Value) -> AfsResult<BlockDto> {
        self.writes.lock().expect("writes").push(WriteCall::Update {
            block_id: block_id.to_string(),
            body,
        });
        Ok(block(block_id, "paragraph"))
    }

    fn append_block_children(&self, block_id: &str, body: Value) -> AfsResult<BlockListDto> {
        self.writes.lock().expect("writes").push(WriteCall::Append {
            block_id: block_id.to_string(),
            body,
        });
        let mut append_count = self.append_count.lock().expect("append count");
        *append_count += 1;
        Ok(PaginatedListDto {
            results: vec![paragraph_block(
                &format!("created-{}", *append_count),
                "Created.",
                false,
            )],
            next_cursor: None,
            has_more: false,
        })
    }

    fn delete_block(&self, block_id: &str) -> AfsResult<BlockDto> {
        self.writes.lock().expect("writes").push(WriteCall::Delete {
            block_id: block_id.to_string(),
        });
        Ok(block(block_id, "paragraph"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum WriteCall {
    Update { block_id: String, body: Value },
    Append { block_id: String, body: Value },
    Delete { block_id: String },
}

fn block(id: &str, kind: &str) -> BlockDto {
    BlockDto {
        id: id.to_string(),
        kind: kind.to_string(),
        ..Default::default()
    }
}

fn paragraph_block(id: &str, text: &str, rich: bool) -> BlockDto {
    let mut block = block(id, "paragraph");
    let mut rich_text = rich_text(text);
    if rich {
        rich_text[0].annotations = RichTextAnnotationsDto {
            bold: true,
            ..Default::default()
        };
    }
    block.paragraph = Some(RichTextBlockDto {
        rich_text,
        color: None,
    });
    block
}

fn paragraph_block_with_rich_text(id: &str, rich_text: Vec<RichTextDto>) -> BlockDto {
    let mut block = block(id, "paragraph");
    block.paragraph = Some(RichTextBlockDto {
        rich_text,
        color: None,
    });
    block
}

fn rich_text(text: &str) -> Vec<RichTextDto> {
    vec![rich_text_part(text)]
}

fn rich_text_part(text: &str) -> RichTextDto {
    RichTextDto {
        kind: "text".to_string(),
        text: Some(TextRichTextDto {
            content: text.to_string(),
            link: None,
        }),
        plain_text: text.to_string(),
        ..Default::default()
    }
}

fn annotated_text(text: &str, apply: impl FnOnce(&mut RichTextAnnotationsDto)) -> RichTextDto {
    let mut part = rich_text_part(text);
    apply(&mut part.annotations);
    part
}

fn linked_text(text: &str, href: &str) -> RichTextDto {
    RichTextDto {
        href: Some(href.to_string()),
        text: Some(TextRichTextDto {
            content: text.to_string(),
            link: Some(LinkDto {
                url: href.to_string(),
            }),
        }),
        ..rich_text_part(text)
    }
}

fn date_mention(text: &str, start: &str) -> RichTextDto {
    RichTextDto {
        kind: "mention".to_string(),
        mention: Some(MentionRichTextDto {
            kind: "date".to_string(),
            date: Some(DateMentionDto {
                start: start.to_string(),
                end: None,
                time_zone: None,
            }),
            ..Default::default()
        }),
        plain_text: text.to_string(),
        ..Default::default()
    }
}

fn rich_text_json(text: &str) -> Value {
    json!([
        {
            "type": "text",
            "text": {
                "content": text,
            },
        }
    ])
}
