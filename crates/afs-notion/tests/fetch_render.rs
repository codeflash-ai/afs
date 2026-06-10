use std::collections::BTreeMap;
use std::sync::Arc;

use afs_connector::{Connector, FetchRequest};
use afs_core::model::RemoteId;
use afs_core::shadow::MarkdownBlockKind;
use afs_notion::client::NotionApi;
use afs_notion::dto::{
    BlockDto, BlockListDto, BlockTreeDto, PageDto, PagePropertyDto, PaginatedListDto,
    RichTextBlockDto, RichTextDto,
};
use afs_notion::{NotionConfig, NotionConnector};

#[test]
fn fetch_recurses_paginated_block_children_and_render_preserves_shadow_ids() {
    let api = FixtureNotionApi::new();
    let connector = NotionConnector::with_api(NotionConfig::default(), Arc::new(api));

    let native = connector
        .fetch(FetchRequest {
            remote_id: RemoteId::new("page-1"),
        })
        .expect("fetch");
    let bundle: afs_notion::dto::NotionPageBundle =
        serde_json::from_slice(&native.raw).expect("native bundle");

    assert_eq!(bundle.blocks.len(), 3);
    assert_eq!(bundle.blocks[1].children.len(), 1);

    let rendered = connector
        .render_native_entity(&native)
        .expect("render native entity");

    assert!(rendered.document.frontmatter.contains("id: page-1"));
    assert!(rendered.document.frontmatter.contains("title: \"Roadmap\""));
    assert_eq!(
        rendered.document.body,
        "# Roadmap\n\nPlan paragraph.\n\nNested detail.\n\n---\n"
    );
    assert_eq!(
        rendered
            .shadow
            .blocks
            .iter()
            .map(|block| block.remote_id.as_str())
            .collect::<Vec<_>>(),
        vec![
            "heading-1",
            "paragraph-1",
            "nested-paragraph-1",
            "divider-1"
        ]
    );
}

#[test]
fn render_unsupported_block_as_directive_without_consuming_native_shadow_id() {
    let page = page();
    let block = BlockTreeDto {
        block: unsupported_block("bookmark-1", "bookmark"),
        children: Vec::new(),
    };
    let bundle = afs_notion::dto::NotionPageBundle {
        page,
        blocks: vec![block],
    };
    let raw = serde_json::to_vec(&bundle).expect("raw");
    let native = afs_connector::NativeEntity {
        remote_id: RemoteId::new("page-1"),
        kind: "notion_page".to_string(),
        raw,
    };
    let connector =
        NotionConnector::with_api(NotionConfig::default(), Arc::new(FixtureNotionApi::new()));

    let rendered = connector
        .render_native_entity(&native)
        .expect("render native entity");

    assert_eq!(
        rendered.document.body,
        "::afs{id=bookmark-1 type=unsupported_bookmark}\n"
    );
    assert_eq!(rendered.shadow.blocks.len(), 1);
    assert_eq!(
        rendered.shadow.blocks[0].remote_id,
        RemoteId::new("bookmark-1")
    );
    assert!(matches!(
        rendered.shadow.blocks[0].kind,
        MarkdownBlockKind::Directive { .. }
    ));
}

#[test]
#[ignore = "requires NOTION_TOKEN and AFS_NOTION_PAGE_ID"]
fn live_fetch_and_render_page_from_environment() {
    let page_id = std::env::var("AFS_NOTION_PAGE_ID").expect("AFS_NOTION_PAGE_ID");
    let connector = NotionConnector::new(NotionConfig::default());

    let native = connector
        .fetch(FetchRequest {
            remote_id: RemoteId::new(page_id),
        })
        .expect("live fetch");
    let rendered = connector
        .render_native_entity(&native)
        .expect("live render");

    assert!(!rendered.document.frontmatter.is_empty());
    assert_eq!(rendered.shadow.entity_id, native.remote_id);
}

#[derive(Debug)]
struct FixtureNotionApi {
    children: BTreeMap<(String, Option<String>), BlockListDto>,
}

impl FixtureNotionApi {
    fn new() -> Self {
        let mut children = BTreeMap::new();
        children.insert(
            ("page-1".to_string(), None),
            PaginatedListDto {
                results: vec![
                    rich_text_block("heading-1", "heading_1", "Roadmap"),
                    rich_text_block("paragraph-1", "paragraph", "Plan paragraph.").with_children(),
                ],
                next_cursor: Some("page-1-cursor-2".to_string()),
                has_more: true,
            },
        );
        children.insert(
            ("page-1".to_string(), Some("page-1-cursor-2".to_string())),
            PaginatedListDto {
                results: vec![block("divider-1", "divider")],
                next_cursor: None,
                has_more: false,
            },
        );
        children.insert(
            ("paragraph-1".to_string(), None),
            PaginatedListDto {
                results: vec![rich_text_block(
                    "nested-paragraph-1",
                    "paragraph",
                    "Nested detail.",
                )],
                next_cursor: None,
                has_more: false,
            },
        );

        Self { children }
    }
}

impl NotionApi for FixtureNotionApi {
    fn retrieve_page(&self, page_id: &str) -> afs_core::AfsResult<PageDto> {
        assert_eq!(page_id, "page-1");
        Ok(page())
    }

    fn retrieve_block_children(
        &self,
        block_id: &str,
        start_cursor: Option<&str>,
    ) -> afs_core::AfsResult<BlockListDto> {
        Ok(self
            .children
            .get(&(block_id.to_string(), start_cursor.map(str::to_string)))
            .cloned()
            .unwrap_or_default())
    }
}

trait WithChildren {
    fn with_children(self) -> Self;
}

impl WithChildren for BlockDto {
    fn with_children(mut self) -> Self {
        self.has_children = true;
        self
    }
}

fn page() -> PageDto {
    PageDto {
        id: "page-1".to_string(),
        created_time: Some("2026-06-10T00:00:00.000Z".to_string()),
        last_edited_time: Some("2026-06-10T00:00:00.000Z".to_string()),
        archived: false,
        in_trash: false,
        properties: BTreeMap::from([(
            "title".to_string(),
            PagePropertyDto {
                kind: "title".to_string(),
                title: vec![rich_text("Roadmap")],
                rich_text: Vec::new(),
            },
        )]),
    }
}

fn rich_text_block(id: &str, kind: &str, text: &str) -> BlockDto {
    let mut block = block(id, kind);
    let value = Some(RichTextBlockDto {
        rich_text: vec![rich_text(text)],
        color: None,
    });

    match kind {
        "paragraph" => block.paragraph = value,
        "heading_1" => block.heading_1 = value,
        "heading_2" => block.heading_2 = value,
        "heading_3" => block.heading_3 = value,
        _ => panic!("unsupported fixture rich text kind: {kind}"),
    }

    block
}

fn unsupported_block(id: &str, kind: &str) -> BlockDto {
    block(id, kind)
}

fn block(id: &str, kind: &str) -> BlockDto {
    BlockDto {
        id: id.to_string(),
        kind: kind.to_string(),
        has_children: false,
        archived: false,
        in_trash: false,
        paragraph: None,
        heading_1: None,
        heading_2: None,
        heading_3: None,
        bulleted_list_item: None,
        numbered_list_item: None,
        to_do: None,
        quote: None,
        callout: None,
        code: None,
        child_page: None,
        child_database: None,
    }
}

fn rich_text(text: &str) -> RichTextDto {
    RichTextDto {
        plain_text: text.to_string(),
        href: None,
        annotations: Default::default(),
    }
}
