use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use afs_connector::{Connector, EnumerateRequest, FetchRequest};
use afs_core::model::{EntityKind, MountId, RemoteId};
use afs_core::shadow::MarkdownBlockKind;
use afs_notion::client::NotionApi;
use afs_notion::dto::{
    BlockDto, BlockListDto, BlockTreeDto, ColorOnlyBlockDto, DataSourceDto, DataSourcePropertyDto,
    DataSourceSummaryDto, DatabaseDto, DateMentionDto, EquationBlockDto, EquationRichTextDto,
    ExternalFileDto, FileBlockDto, IdRefDto, LinkDto, LinkToPageBlockDto, MeetingNotesBlockDto,
    MentionRichTextDto, PageDto, PageListDto, PagePropertyDto, PaginatedListDto,
    RichTextAnnotationsDto, RichTextBlockDto, RichTextDto, SelectOptionDto,
    SelectPropertySchemaDto, SyncedBlockDto, SyncedFromDto, TableBlockDto, TableRowBlockDto,
    TextRichTextDto, TitleBlockDto, UrlBlockDto,
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
fn fetch_does_not_inline_child_page_or_database_content_into_parent_body() {
    let api = FixtureNotionApi::parent_with_child_boundaries();
    let connector = NotionConnector::with_api(NotionConfig::default(), Arc::new(api));

    let native = connector
        .fetch(FetchRequest {
            remote_id: RemoteId::new("parent-page"),
        })
        .expect("fetch parent");
    let bundle: afs_notion::dto::NotionPageBundle =
        serde_json::from_slice(&native.raw).expect("native bundle");

    assert_eq!(bundle.blocks.len(), 3);
    assert!(bundle.blocks.iter().all(|tree| tree.children.is_empty()));

    let rendered = connector
        .render_native_entity(&native)
        .expect("render parent");

    assert_eq!(
        rendered.document.body,
        "Parent body.\n\n::afs{id=child-page type=child_page title=\"Child Page\"}\n\n::afs{id=child-db type=child_database title=\"Tasks\"}\n"
    );
    assert!(!rendered.document.body.contains("Child body."));
    assert!(!rendered.document.body.contains("Database body."));
}

#[test]
fn render_unsupported_block_as_directive_without_consuming_native_shadow_id() {
    let page = page("page-1", "Roadmap");
    let block = BlockTreeDto {
        block: unsupported_block("future-1", "future_block"),
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
        "::afs{id=future-1 type=unsupported_future_block}\n"
    );
    assert_eq!(rendered.shadow.blocks.len(), 1);
    assert_eq!(
        rendered.shadow.blocks[0].remote_id,
        RemoteId::new("future-1")
    );
    assert!(matches!(
        rendered.shadow.blocks[0].kind,
        MarkdownBlockKind::Directive { .. }
    ));
}

#[test]
fn render_richer_notion_block_coverage() {
    let bundle = afs_notion::dto::NotionPageBundle {
        page: page("page-1", "Coverage"),
        blocks: vec![
            BlockTreeDto {
                block: rich_text_block("heading-4", "heading_4", "Heading four"),
                children: Vec::new(),
            },
            BlockTreeDto {
                block: toggle_block("toggle-1", "Toggle summary"),
                children: Vec::new(),
            },
            BlockTreeDto {
                block: equation_block("equation-1", "E=mc^2"),
                children: Vec::new(),
            },
            BlockTreeDto {
                block: url_block(
                    "embed-1",
                    "embed",
                    "https://example.com/embed",
                    "Embed caption",
                ),
                children: Vec::new(),
            },
            BlockTreeDto {
                block: url_block(
                    "bookmark-1",
                    "bookmark",
                    "https://example.com/bookmark",
                    "Bookmark caption",
                ),
                children: Vec::new(),
            },
            BlockTreeDto {
                block: file_block(
                    "image-1",
                    "image",
                    "https://example.com/image.png",
                    "Image caption",
                ),
                children: Vec::new(),
            },
            BlockTreeDto {
                block: synced_block("synced-1", "source-block-1"),
                children: Vec::new(),
            },
            BlockTreeDto {
                block: link_to_page_block("link-to-page-1", "target-page-1"),
                children: Vec::new(),
            },
            BlockTreeDto {
                block: table_of_contents_block("toc-1"),
                children: Vec::new(),
            },
            BlockTreeDto {
                block: block("breadcrumb-1", "breadcrumb"),
                children: Vec::new(),
            },
            BlockTreeDto {
                block: block("column-list-1", "column_list"),
                children: Vec::new(),
            },
            BlockTreeDto {
                block: block("column-1", "column"),
                children: Vec::new(),
            },
            BlockTreeDto {
                block: meeting_notes_block("meeting-1", "Weekly sync"),
                children: Vec::new(),
            },
        ],
    };

    let rendered = afs_notion::render::render_page_bundle(&bundle).expect("render");

    assert_eq!(
        rendered.document.body,
        concat!(
            "#### Heading four\n\n",
            "::afs{id=toggle-1 type=toggle title=\"Toggle summary\"}\n\n",
            "$$\nE=mc^2\n$$\n\n",
            "::afs{id=embed-1 type=embed title=\"Embed caption\" url=\"https://example.com/embed\"}\n\n",
            "::afs{id=bookmark-1 type=bookmark title=\"Bookmark caption\" url=\"https://example.com/bookmark\"}\n\n",
            "::afs{id=image-1 type=image title=\"Image caption\" url=\"https://example.com/image.png\"}\n\n",
            "::afs{id=synced-1 type=synced_block source_block_id=\"source-block-1\"}\n\n",
            "::afs{id=link-to-page-1 type=link_to_page page_id=\"target-page-1\"}\n\n",
            "::afs{id=toc-1 type=table_of_contents color=\"default\"}\n\n",
            "::afs{id=breadcrumb-1 type=breadcrumb}\n\n",
            "::afs{id=column-list-1 type=column_list}\n\n",
            "::afs{id=column-1 type=column}\n\n",
            "::afs{id=meeting-1 type=meeting_notes title=\"Weekly sync\"}\n"
        )
    );
    assert_eq!(
        rendered
            .shadow
            .blocks
            .iter()
            .map(|block| block.remote_id.as_str())
            .collect::<Vec<_>>(),
        vec![
            "heading-4",
            "toggle-1",
            "equation-1",
            "embed-1",
            "bookmark-1",
            "image-1",
            "synced-1",
            "link-to-page-1",
            "toc-1",
            "breadcrumb-1",
            "column-list-1",
            "column-1",
            "meeting-1",
        ]
    );
}

#[test]
fn render_table_as_markdown_table_with_row_shadow_metadata() {
    let bundle = afs_notion::dto::NotionPageBundle {
        page: page("page-1", "Roadmap"),
        blocks: vec![BlockTreeDto {
            block: table_block("table-1", 2, true),
            children: vec![
                BlockTreeDto {
                    block: table_row_block("row-1", ["Decision", "Choice"]),
                    children: Vec::new(),
                },
                BlockTreeDto {
                    block: table_row_block("row-2", ["First connector", "Notion"]),
                    children: Vec::new(),
                },
            ],
        }],
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
        "| Decision | Choice |\n| --- | --- |\n| First connector | Notion |\n"
    );
    assert_eq!(rendered.shadow.blocks.len(), 1);
    assert_eq!(
        rendered.shadow.blocks[0].remote_id,
        RemoteId::new("table-1")
    );
    assert_eq!(
        rendered.shadow.blocks[0].kind,
        MarkdownBlockKind::TableWithRows {
            row_ids: vec![RemoteId::new("row-1"), RemoteId::new("row-2")],
            has_column_header: true,
            has_row_header: false,
        }
    );
}

#[test]
fn render_malformed_table_as_directives() {
    let bundle = afs_notion::dto::NotionPageBundle {
        page: page("page-1", "Roadmap"),
        blocks: vec![BlockTreeDto {
            block: table_block("table-1", 3, true),
            children: vec![BlockTreeDto {
                block: table_row_block("row-1", ["Decision", "Choice"]),
                children: Vec::new(),
            }],
        }],
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
        "::afs{id=table-1 type=unsupported_table}\n\n::afs{id=row-1 type=unsupported_table_row}\n"
    );
    assert!(
        rendered
            .shadow
            .blocks
            .iter()
            .all(|block| matches!(block.kind, MarkdownBlockKind::Directive { .. }))
    );
}

#[test]
fn render_media_blocks_with_local_paths_when_page_path_is_available() {
    let bundle = afs_notion::dto::NotionPageBundle {
        page: page("page-1", "Coverage"),
        blocks: vec![BlockTreeDto {
            block: file_block(
                "0123456789abcdef",
                "image",
                "https://example.com/image.PNG?download=1",
                "Image caption",
            ),
            children: Vec::new(),
        }],
    };

    let rendered = afs_notion::render::render_page_bundle_with_options(
        &bundle,
        &afs_notion::render::RenderOptions::with_page_path("Docs/Coverage ~page1.md"),
    )
    .expect("render");

    assert_eq!(rendered.media_assets.len(), 1);
    assert_eq!(
        rendered.media_assets[0].local_path,
        Path::new("media/Docs/Coverage ~page1/image-0123456789ab.png")
    );
    assert!(
        rendered
            .document
            .body
            .contains("local=\"media/Docs/Coverage ~page1/image-0123456789ab.png\"")
    );
    assert!(
        rendered
            .document
            .body
            .contains("url=\"https://example.com/image.PNG?download=1\"")
    );
}

#[test]
fn render_rich_text_annotations_links_mentions_and_equations() {
    let mut bold = rich_text("Bold");
    bold.annotations.bold = true;

    let mut italic = rich_text(" italic");
    italic.annotations.italic = true;

    let mut strikethrough = rich_text(" strike");
    strikethrough.annotations.strikethrough = true;

    let mut underline = rich_text(" underline");
    underline.annotations.underline = true;

    let mut code = rich_text(" code");
    code.annotations.code = true;

    let bundle = afs_notion::dto::NotionPageBundle {
        page: page("page-1", "Roadmap"),
        blocks: vec![BlockTreeDto {
            block: paragraph_block(
                "paragraph-1",
                vec![
                    bold,
                    italic,
                    strikethrough,
                    underline,
                    code,
                    linked_text(" external link", "https://example.com/"),
                    rich_text(" after link."),
                    rich_text(" "),
                    date_mention("2026-06-10", "2026-06-10"),
                    rich_text(" and inline equation "),
                    equation("E=mc^2"),
                    rich_text(" plus page mention "),
                    page_mention("Roadmap", "page-1"),
                ],
            ),
            children: Vec::new(),
        }],
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
        "**Bold** _italic_ ~~strike~~ <u>underline</u> `code` [external link](https://example.com/) after link. 2026-06-10 and inline equation $E=mc^2$ plus page mention [Roadmap](afs://page-1)\n"
    );
}

#[test]
fn render_database_row_properties_as_frontmatter() {
    let mut row = page("row-1", "Fix login bug");
    row.properties.insert(
        "Status".to_string(),
        PagePropertyDto {
            kind: "select".to_string(),
            select: Some(select_option("status-id", "In progress")),
            ..Default::default()
        },
    );
    row.properties.insert(
        "Points".to_string(),
        PagePropertyDto {
            kind: "number".to_string(),
            number: Some(serde_json::Number::from(3)),
            ..Default::default()
        },
    );
    row.properties.insert(
        "Tags".to_string(),
        PagePropertyDto {
            kind: "multi_select".to_string(),
            multi_select: vec![select_option("backend-id", "Backend")],
            ..Default::default()
        },
    );
    row.properties.insert(
        "Done".to_string(),
        PagePropertyDto {
            kind: "checkbox".to_string(),
            checkbox: Some(false),
            ..Default::default()
        },
    );
    row.properties.insert(
        "Due".to_string(),
        PagePropertyDto {
            kind: "date".to_string(),
            date: Some(DateMentionDto {
                start: "2026-06-10".to_string(),
                end: None,
                time_zone: None,
            }),
            ..Default::default()
        },
    );
    let bundle = afs_notion::dto::NotionPageBundle {
        page: row,
        blocks: Vec::new(),
    };

    let rendered = afs_notion::render::render_page_bundle(&bundle).expect("render");

    assert!(
        rendered
            .document
            .frontmatter
            .contains("title: \"Fix login bug\"")
    );
    assert!(
        rendered
            .document
            .frontmatter
            .contains("\"Status\": \"In progress\"")
    );
    assert!(rendered.document.frontmatter.contains("\"Points\": 3"));
    assert!(rendered.document.frontmatter.contains("\"Done\": false"));
    assert!(
        rendered
            .document
            .frontmatter
            .contains("\"Due\": \"2026-06-10\"")
    );
    assert!(
        rendered
            .document
            .frontmatter
            .contains("\"Tags\":\n  - \"Backend\"")
    );
}

#[test]
fn enumerate_projects_root_page_tree_to_stable_paths() {
    let root_page_id = RemoteId::new("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    let api = FixtureNotionApi::tree(root_page_id.as_str());
    let connector = NotionConnector::with_api(
        NotionConfig::default().with_root_page_id(root_page_id),
        Arc::new(api),
    );

    let entries = connector
        .enumerate(EnumerateRequest {
            mount_id: MountId::new("notion-main"),
            cursor: None,
        })
        .expect("enumerate");

    assert_eq!(entries.len(), 4);
    assert_eq!(entries[0].path, Path::new("roadmap ~aaaaaa.md"));
    assert_eq!(entries[0].kind, EntityKind::Page);
    assert_eq!(
        entries[1].path,
        Path::new("roadmap ~aaaaaa/design-notes ~bbbbbb.md")
    );
    assert_eq!(entries[1].kind, EntityKind::Page);
    assert_eq!(entries[2].path, Path::new("roadmap ~aaaaaa/tasks ~cccccc"));
    assert_eq!(entries[2].kind, EntityKind::Database);
    assert_eq!(
        entries[3].path,
        Path::new("roadmap ~aaaaaa/tasks ~cccccc/fix-login-bug ~eeeeee.md")
    );
    assert_eq!(entries[3].kind, EntityKind::Page);
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
    pages: BTreeMap<String, PageDto>,
    children: BTreeMap<(String, Option<String>), BlockListDto>,
    databases: BTreeMap<String, DatabaseDto>,
    data_sources: BTreeMap<String, DataSourceDto>,
    data_source_pages: BTreeMap<(String, Option<String>), PageListDto>,
}

impl FixtureNotionApi {
    fn new() -> Self {
        let pages = BTreeMap::from([("page-1".to_string(), page("page-1", "Roadmap"))]);
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

        Self {
            pages,
            children,
            databases: BTreeMap::new(),
            data_sources: BTreeMap::new(),
            data_source_pages: BTreeMap::new(),
        }
    }

    fn tree(root_page_id: &str) -> Self {
        let child_page_id = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let database_id = "cccccccccccccccccccccccccccccccc";
        let data_source_id = "dddddddddddddddddddddddddddddddd";
        let row_page_id = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
        let pages = BTreeMap::from([
            (root_page_id.to_string(), page(root_page_id, "Roadmap")),
            (
                child_page_id.to_string(),
                page(child_page_id, "Design Notes"),
            ),
            (row_page_id.to_string(), page(row_page_id, "Fix login bug")),
        ]);
        let children = BTreeMap::from([
            (
                (root_page_id.to_string(), None),
                PaginatedListDto {
                    results: vec![
                        child_page_block(child_page_id, "Design Notes"),
                        child_database_block(database_id, "Tasks"),
                    ],
                    next_cursor: None,
                    has_more: false,
                },
            ),
            (
                (child_page_id.to_string(), None),
                PaginatedListDto::default(),
            ),
            ((row_page_id.to_string(), None), PaginatedListDto::default()),
        ]);
        let databases = BTreeMap::from([(
            database_id.to_string(),
            DatabaseDto {
                id: database_id.to_string(),
                title: vec![rich_text("Tasks")],
                data_sources: vec![DataSourceSummaryDto {
                    id: data_source_id.to_string(),
                    name: Some("Tasks".to_string()),
                }],
                ..Default::default()
            },
        )]);
        let data_sources = BTreeMap::from([(
            data_source_id.to_string(),
            DataSourceDto {
                id: data_source_id.to_string(),
                name: Some("Tasks".to_string()),
                properties: BTreeMap::from([
                    (
                        "Name".to_string(),
                        DataSourcePropertyDto {
                            id: "title".to_string(),
                            kind: "title".to_string(),
                            ..Default::default()
                        },
                    ),
                    (
                        "Status".to_string(),
                        DataSourcePropertyDto {
                            id: "status-id".to_string(),
                            kind: "select".to_string(),
                            select: Some(SelectPropertySchemaDto {
                                options: vec![select_option("todo-id", "Todo")],
                            }),
                            ..Default::default()
                        },
                    ),
                ]),
                ..Default::default()
            },
        )]);
        let data_source_pages = BTreeMap::from([(
            (data_source_id.to_string(), None),
            PaginatedListDto {
                results: vec![page(row_page_id, "Fix login bug")],
                next_cursor: None,
                has_more: false,
            },
        )]);

        Self {
            pages,
            children,
            databases,
            data_sources,
            data_source_pages,
        }
    }

    fn parent_with_child_boundaries() -> Self {
        let pages = BTreeMap::from([
            ("parent-page".to_string(), page("parent-page", "Parent")),
            ("child-page".to_string(), page("child-page", "Child Page")),
        ]);
        let databases = BTreeMap::from([(
            "child-db".to_string(),
            DatabaseDto {
                id: "child-db".to_string(),
                title: vec![rich_text("Tasks")],
                data_sources: vec![DataSourceSummaryDto {
                    id: "data-source-1".to_string(),
                    name: Some("Tasks".to_string()),
                }],
                ..Default::default()
            },
        )]);
        let children = BTreeMap::from([
            (
                ("parent-page".to_string(), None),
                PaginatedListDto {
                    results: vec![
                        paragraph_block("parent-paragraph", vec![rich_text("Parent body.")]),
                        child_page_block("child-page", "Child Page").with_children(),
                        child_database_block("child-db", "Tasks").with_children(),
                    ],
                    next_cursor: None,
                    has_more: false,
                },
            ),
            (
                ("child-page".to_string(), None),
                PaginatedListDto {
                    results: vec![paragraph_block(
                        "child-paragraph",
                        vec![rich_text("Child body.")],
                    )],
                    next_cursor: None,
                    has_more: false,
                },
            ),
            (
                ("child-db".to_string(), None),
                PaginatedListDto {
                    results: vec![paragraph_block(
                        "database-paragraph",
                        vec![rich_text("Database body.")],
                    )],
                    next_cursor: None,
                    has_more: false,
                },
            ),
        ]);

        Self {
            pages,
            children,
            databases,
            data_sources: BTreeMap::new(),
            data_source_pages: BTreeMap::new(),
        }
    }
}

impl NotionApi for FixtureNotionApi {
    fn retrieve_page(&self, page_id: &str) -> afs_core::AfsResult<PageDto> {
        self.pages.get(page_id).cloned().ok_or_else(|| {
            afs_core::AfsError::InvalidState(format!("missing fixture page {page_id}"))
        })
    }

    fn retrieve_database(&self, database_id: &str) -> afs_core::AfsResult<DatabaseDto> {
        self.databases.get(database_id).cloned().ok_or_else(|| {
            afs_core::AfsError::InvalidState(format!("missing fixture database {database_id}"))
        })
    }

    fn retrieve_data_source(&self, data_source_id: &str) -> afs_core::AfsResult<DataSourceDto> {
        self.data_sources
            .get(data_source_id)
            .cloned()
            .ok_or_else(|| {
                afs_core::AfsError::InvalidState(format!(
                    "missing fixture data source {data_source_id}"
                ))
            })
    }

    fn query_data_source(
        &self,
        data_source_id: &str,
        start_cursor: Option<&str>,
    ) -> afs_core::AfsResult<PageListDto> {
        Ok(self
            .data_source_pages
            .get(&(data_source_id.to_string(), start_cursor.map(str::to_string)))
            .cloned()
            .unwrap_or_default())
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

    fn search_pages(&self, _start_cursor: Option<&str>) -> afs_core::AfsResult<PageListDto> {
        Ok(PaginatedListDto {
            results: self.pages.values().cloned().collect(),
            next_cursor: None,
            has_more: false,
        })
    }

    fn update_block(
        &self,
        _block_id: &str,
        _body: serde_json::Value,
    ) -> afs_core::AfsResult<BlockDto> {
        Err(afs_core::AfsError::NotImplemented("fixture update block"))
    }

    fn append_block_children(
        &self,
        _block_id: &str,
        _body: serde_json::Value,
    ) -> afs_core::AfsResult<BlockListDto> {
        Err(afs_core::AfsError::NotImplemented(
            "fixture append block children",
        ))
    }

    fn delete_block(&self, _block_id: &str) -> afs_core::AfsResult<BlockDto> {
        Err(afs_core::AfsError::NotImplemented("fixture delete block"))
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

fn page(id: &str, title: &str) -> PageDto {
    PageDto {
        id: id.to_string(),
        created_time: Some("2026-06-10T00:00:00.000Z".to_string()),
        last_edited_time: Some("2026-06-10T00:00:00.000Z".to_string()),
        archived: false,
        in_trash: false,
        properties: BTreeMap::from([(
            "title".to_string(),
            PagePropertyDto {
                kind: "title".to_string(),
                title: vec![rich_text(title)],
                rich_text: Vec::new(),
                ..Default::default()
            },
        )]),
    }
}

fn select_option(id: &str, name: &str) -> SelectOptionDto {
    SelectOptionDto {
        id: id.to_string(),
        name: name.to_string(),
        color: None,
    }
}

fn rich_text_block(id: &str, kind: &str, text: &str) -> BlockDto {
    let mut block = block(id, kind);
    let value = Some(rich_text_block_content(vec![rich_text(text)]));

    match kind {
        "paragraph" => block.paragraph = value,
        "heading_1" => block.heading_1 = value,
        "heading_2" => block.heading_2 = value,
        "heading_3" => block.heading_3 = value,
        "heading_4" => block.heading_4 = value,
        _ => panic!("unsupported fixture rich text kind: {kind}"),
    }

    block
}

fn paragraph_block(id: &str, rich_text: Vec<RichTextDto>) -> BlockDto {
    let mut block = block(id, "paragraph");
    block.paragraph = Some(rich_text_block_content(rich_text));
    block
}

fn rich_text_block_content(rich_text: Vec<RichTextDto>) -> RichTextBlockDto {
    RichTextBlockDto {
        rich_text,
        color: None,
    }
}

fn unsupported_block(id: &str, kind: &str) -> BlockDto {
    block(id, kind)
}

fn child_page_block(id: &str, title: &str) -> BlockDto {
    let mut block = block(id, "child_page");
    block.child_page = Some(TitleBlockDto {
        title: title.to_string(),
    });
    block
}

fn child_database_block(id: &str, title: &str) -> BlockDto {
    let mut block = block(id, "child_database");
    block.child_database = Some(TitleBlockDto {
        title: title.to_string(),
    });
    block
}

fn block(id: &str, kind: &str) -> BlockDto {
    BlockDto {
        id: id.to_string(),
        kind: kind.to_string(),
        ..Default::default()
    }
}

fn toggle_block(id: &str, text: &str) -> BlockDto {
    let mut block = block(id, "toggle");
    block.toggle = Some(rich_text_block_content(vec![rich_text(text)]));
    block
}

fn equation_block(id: &str, expression: &str) -> BlockDto {
    let mut block = block(id, "equation");
    block.equation = Some(EquationBlockDto {
        expression: expression.to_string(),
    });
    block
}

fn url_block(id: &str, kind: &str, url: &str, caption: &str) -> BlockDto {
    let mut block = block(id, kind);
    let payload = Some(UrlBlockDto {
        url: url.to_string(),
        caption: vec![rich_text(caption)],
    });
    match kind {
        "embed" => block.embed = payload,
        "bookmark" => block.bookmark = payload,
        "link_preview" => block.link_preview = payload,
        _ => panic!("unsupported fixture url block kind: {kind}"),
    }
    block
}

fn file_block(id: &str, kind: &str, url: &str, caption: &str) -> BlockDto {
    let mut block = block(id, kind);
    let payload = Some(FileBlockDto {
        kind: "external".to_string(),
        external: Some(ExternalFileDto {
            url: url.to_string(),
        }),
        file: None,
        caption: vec![rich_text(caption)],
    });
    match kind {
        "image" => block.image = payload,
        "video" => block.video = payload,
        "file" => block.file = payload,
        "pdf" => block.pdf = payload,
        "audio" => block.audio = payload,
        _ => panic!("unsupported fixture file block kind: {kind}"),
    }
    block
}

fn synced_block(id: &str, source_block_id: &str) -> BlockDto {
    let mut block = block(id, "synced_block");
    block.synced_block = Some(SyncedBlockDto {
        synced_from: Some(SyncedFromDto {
            kind: "block_id".to_string(),
            block_id: Some(source_block_id.to_string()),
        }),
    });
    block
}

fn link_to_page_block(id: &str, page_id: &str) -> BlockDto {
    let mut block = block(id, "link_to_page");
    block.link_to_page = Some(LinkToPageBlockDto {
        kind: "page_id".to_string(),
        page_id: Some(page_id.to_string()),
        database_id: None,
    });
    block
}

fn table_of_contents_block(id: &str) -> BlockDto {
    let mut block = block(id, "table_of_contents");
    block.table_of_contents = Some(ColorOnlyBlockDto {
        color: Some("default".to_string()),
    });
    block
}

fn meeting_notes_block(id: &str, title: &str) -> BlockDto {
    let mut block = block(id, "meeting_notes");
    block.meeting_notes = Some(MeetingNotesBlockDto {
        title: Some(title.to_string()),
    });
    block
}

fn table_block(id: &str, width: u16, has_column_header: bool) -> BlockDto {
    let mut block = block(id, "table");
    block.table = Some(TableBlockDto {
        table_width: width,
        has_column_header,
        has_row_header: false,
    });
    block
}

fn table_row_block<const N: usize>(id: &str, cells: [&str; N]) -> BlockDto {
    let mut block = block(id, "table_row");
    block.table_row = Some(TableRowBlockDto {
        cells: cells
            .into_iter()
            .map(|cell| vec![rich_text(cell)])
            .collect::<Vec<_>>(),
    });
    block
}

fn rich_text(text: &str) -> RichTextDto {
    RichTextDto {
        kind: "text".to_string(),
        text: Some(TextRichTextDto {
            content: text.to_string(),
            link: None,
        }),
        mention: None,
        equation: None,
        plain_text: text.to_string(),
        href: None,
        annotations: Default::default(),
    }
}

fn linked_text(text: &str, url: &str) -> RichTextDto {
    RichTextDto {
        href: Some(url.to_string()),
        text: Some(TextRichTextDto {
            content: text.to_string(),
            link: Some(LinkDto {
                url: url.to_string(),
            }),
        }),
        ..rich_text(text)
    }
}

fn date_mention(text: &str, start: &str) -> RichTextDto {
    RichTextDto {
        kind: "mention".to_string(),
        text: None,
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
        annotations: RichTextAnnotationsDto::default(),
        ..Default::default()
    }
}

fn equation(expression: &str) -> RichTextDto {
    RichTextDto {
        kind: "equation".to_string(),
        equation: Some(EquationRichTextDto {
            expression: expression.to_string(),
        }),
        plain_text: expression.to_string(),
        ..Default::default()
    }
}

fn page_mention(text: &str, id: &str) -> RichTextDto {
    RichTextDto {
        kind: "mention".to_string(),
        mention: Some(MentionRichTextDto {
            kind: "page".to_string(),
            page: Some(IdRefDto { id: id.to_string() }),
            ..Default::default()
        }),
        plain_text: text.to_string(),
        ..Default::default()
    }
}
