//! Apply connector-neutral push plans to Notion.
//!
//! This module is intentionally narrow. The first write loop supports simple
//! Markdown blocks that map cleanly to one Notion block without preserving rich
//! inline annotations. Unsupported content fails before making a lossy request.

use std::collections::BTreeMap;

use afs_connector::{ApplyPlanRequest, ApplyPlanResult, ApplyUndoRequest, ApplyUndoResult};
use afs_core::journal::JournalApplyEffect;
use afs_core::model::RemoteId;
use afs_core::planner::PushOperation;
use afs_core::undo::{UndoOperation, UndoPlanStatus};
use afs_core::{AfsError, AfsResult};
use serde_json::{Map, Value, json};

use crate::client::NotionApi;
use crate::dto::{BlockDto, BlockTreeDto, NotionPageBundle, RichTextDto};
use crate::fetch::fetch_page_bundle;

pub fn check_concurrency(api: &dyn NotionApi, request: ApplyPlanRequest<'_>) -> AfsResult<()> {
    for precondition in request.remote_preconditions {
        let Some(expected) = &precondition.remote_edited_at else {
            continue;
        };
        let page = api.retrieve_page(precondition.remote_id.as_str())?;
        let actual = page
            .last_edited_time
            .as_deref()
            .or(page.created_time.as_deref())
            .unwrap_or("unknown");

        if actual != expected {
            return Err(AfsError::Guardrail(format!(
                "remote entity `{}` changed since last sync (expected remote_edited_at `{expected}`, found `{actual}`)",
                precondition.remote_id.0
            )));
        }
    }

    Ok(())
}

pub fn apply_plan(
    api: &dyn NotionApi,
    request: ApplyPlanRequest<'_>,
) -> AfsResult<ApplyPlanResult> {
    validate_operation_ids(&request)?;
    let bundles = fetch_affected_bundles(api, &request.plan.affected_entities)?;
    let current_blocks = block_map(&bundles);
    let mut changed_remote_ids = Vec::new();
    let mut effects = Vec::new();
    let mut append_chains: BTreeMap<(RemoteId, Option<RemoteId>), RemoteId> = BTreeMap::new();

    for (operation_index, operation) in request.plan.operations.iter().enumerate() {
        match operation {
            PushOperation::UpdateBlock { block_id, content } => {
                let current = current_block(&current_blocks, block_id)?;
                let patch = parse_supported_block(content)?;
                ensure_update_supported(current, &patch)?;
                api.update_block(block_id.as_str(), patch.update_body())?;
                effects.push(JournalApplyEffect::UpdatedBlock {
                    operation_id: request.operation_ids[operation_index].clone(),
                    operation_index,
                    block_id: block_id.clone(),
                });
            }
            PushOperation::AppendBlock {
                parent_id,
                after,
                content,
            } => {
                let patch = parse_supported_block(content)?;
                let chain_key = (parent_id.clone(), after.clone());
                let effective_after = append_chains
                    .get(&chain_key)
                    .cloned()
                    .or_else(|| after.clone());
                let body = append_body(patch.append_child(), effective_after.as_ref());
                let result = api.append_block_children(parent_id.as_str(), body)?;
                let created = result.results.first().ok_or_else(|| {
                    AfsError::InvalidState(
                        "notion append block children returned no created block".to_string(),
                    )
                })?;
                let created_id = RemoteId::new(created.id.clone());
                append_chains.insert(chain_key, created_id.clone());
                effects.push(JournalApplyEffect::CreatedBlock {
                    operation_id: request.operation_ids[operation_index].clone(),
                    operation_index,
                    parent_id: parent_id.clone(),
                    block_id: created_id,
                });
            }
            PushOperation::ArchiveBlock { block_id } => {
                api.delete_block(block_id.as_str())?;
                effects.push(JournalApplyEffect::ArchivedBlock {
                    operation_id: request.operation_ids[operation_index].clone(),
                    operation_index,
                    block_id: block_id.clone(),
                });
            }
            unsupported => {
                return Err(AfsError::Unsupported(unsupported_operation_name(
                    unsupported,
                )));
            }
        }
    }

    for remote_id in &request.plan.affected_entities {
        if !changed_remote_ids.contains(remote_id) {
            changed_remote_ids.push(remote_id.clone());
        }
    }

    Ok(ApplyPlanResult {
        changed_remote_ids,
        effects,
    })
}

pub fn apply_undo(
    api: &dyn NotionApi,
    request: ApplyUndoRequest<'_>,
) -> AfsResult<ApplyUndoResult> {
    if request.plan.status != UndoPlanStatus::Complete {
        return Err(AfsError::Guardrail(
            "cannot apply an incomplete undo plan".to_string(),
        ));
    }

    for operation in &request.plan.operations {
        match operation {
            UndoOperation::RestoreBlockContent { block_id, content } => {
                let patch = parse_supported_block(content)?;
                api.update_block(block_id.as_str(), patch.update_body())?;
            }
            UndoOperation::ArchiveCreatedBlock { block_id } => {
                api.delete_block(block_id.as_str())?;
            }
            UndoOperation::ArchiveCreatedEntity { entity_id } => {
                api.delete_block(entity_id.as_str())?;
            }
            unsupported => return Err(AfsError::Unsupported(unsupported_undo_name(unsupported))),
        }
    }

    Ok(ApplyUndoResult {
        changed_remote_ids: request.plan.affected_entities.clone(),
    })
}

fn validate_operation_ids(request: &ApplyPlanRequest<'_>) -> AfsResult<()> {
    if request.operation_ids.len() != request.plan.operations.len() {
        return Err(AfsError::InvalidState(format!(
            "push plan has {} operations but {} operation ids",
            request.plan.operations.len(),
            request.operation_ids.len()
        )));
    }

    Ok(())
}

fn fetch_affected_bundles(
    api: &dyn NotionApi,
    affected_entities: &[RemoteId],
) -> AfsResult<Vec<NotionPageBundle>> {
    affected_entities
        .iter()
        .map(|remote_id| fetch_page_bundle(api, remote_id.as_str()))
        .collect()
}

fn block_map(bundles: &[NotionPageBundle]) -> BTreeMap<RemoteId, &BlockDto> {
    let mut blocks = BTreeMap::new();
    for bundle in bundles {
        collect_blocks(&bundle.blocks, &mut blocks);
    }
    blocks
}

fn collect_blocks<'a>(trees: &'a [BlockTreeDto], blocks: &mut BTreeMap<RemoteId, &'a BlockDto>) {
    for tree in trees {
        blocks.insert(RemoteId::new(tree.block.id.clone()), &tree.block);
        collect_blocks(&tree.children, blocks);
    }
}

fn current_block<'a>(
    blocks: &'a BTreeMap<RemoteId, &BlockDto>,
    block_id: &RemoteId,
) -> AfsResult<&'a BlockDto> {
    blocks.get(block_id).copied().ok_or_else(|| {
        AfsError::InvalidState(format!(
            "push referenced block `{}` that is absent from current Notion page content",
            block_id.0
        ))
    })
}

fn ensure_update_supported(current: &BlockDto, patch: &NotionBlockPatch) -> AfsResult<()> {
    if current.kind != patch.kind {
        return Err(AfsError::Unsupported("changing Notion block type"));
    }

    if !current_block_has_plain_rich_text(current)? {
        return Err(AfsError::Unsupported(
            "updating rich Notion blocks with annotations, links, mentions, or equations",
        ));
    }

    Ok(())
}

fn current_block_has_plain_rich_text(block: &BlockDto) -> AfsResult<bool> {
    let rich_text = match block.kind.as_str() {
        "paragraph" => block
            .paragraph
            .as_ref()
            .map(|block| block.rich_text.as_slice()),
        "heading_1" => block
            .heading_1
            .as_ref()
            .map(|block| block.rich_text.as_slice()),
        "heading_2" => block
            .heading_2
            .as_ref()
            .map(|block| block.rich_text.as_slice()),
        "heading_3" => block
            .heading_3
            .as_ref()
            .map(|block| block.rich_text.as_slice()),
        "bulleted_list_item" => block
            .bulleted_list_item
            .as_ref()
            .map(|block| block.rich_text.as_slice()),
        "numbered_list_item" => block
            .numbered_list_item
            .as_ref()
            .map(|block| block.rich_text.as_slice()),
        "quote" => block.quote.as_ref().map(|block| block.rich_text.as_slice()),
        "to_do" => block.to_do.as_ref().map(|block| block.rich_text.as_slice()),
        "code" => block.code.as_ref().map(|block| block.rich_text.as_slice()),
        "divider" => return Ok(true),
        _ => return Ok(false),
    }
    .ok_or_else(|| {
        AfsError::InvalidState(format!(
            "notion block `{}` is missing its `{}` payload",
            block.id, block.kind
        ))
    })?;

    Ok(rich_text.iter().all(is_plain_text_part))
}

fn is_plain_text_part(part: &RichTextDto) -> bool {
    let text_variant = part.kind.is_empty() || part.kind == "text";
    let no_annotations = !part.annotations.bold
        && !part.annotations.italic
        && !part.annotations.strikethrough
        && !part.annotations.underline
        && !part.annotations.code
        && part
            .annotations
            .color
            .as_deref()
            .is_none_or(|color| color == "default");
    let no_link = part.href.is_none()
        && part
            .text
            .as_ref()
            .and_then(|text| text.link.as_ref())
            .is_none();

    text_variant && no_annotations && no_link && part.mention.is_none() && part.equation.is_none()
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NotionBlockPatch {
    kind: &'static str,
    payload: Value,
}

impl NotionBlockPatch {
    fn new(kind: &'static str, payload: Value) -> Self {
        Self { kind, payload }
    }

    fn update_body(&self) -> Value {
        json!({ self.kind: self.payload.clone() })
    }

    fn append_child(&self) -> Value {
        let mut object = Map::new();
        object.insert("object".to_string(), json!("block"));
        object.insert("type".to_string(), json!(self.kind));
        object.insert(self.kind.to_string(), self.payload.clone());
        Value::Object(object)
    }
}

fn parse_supported_block(markdown: &str) -> AfsResult<NotionBlockPatch> {
    let trimmed = markdown.trim_end_matches('\n');

    if trimmed.trim().is_empty() {
        return Err(AfsError::Unsupported("empty Notion block writes"));
    }

    if let Some((language, code)) = parse_code_fence(trimmed) {
        let language = if language.is_empty() {
            "plain text".to_string()
        } else {
            language
        };
        return Ok(NotionBlockPatch::new(
            "code",
            json!({
                "rich_text": rich_text(&code),
                "language": language,
            }),
        ));
    }

    if trimmed == "---" {
        return Ok(NotionBlockPatch::new("divider", json!({})));
    }

    if let Some((level, text)) = parse_heading(trimmed) {
        let kind = match level {
            1 => "heading_1",
            2 => "heading_2",
            3 => "heading_3",
            _ => return Err(AfsError::Unsupported("Notion heading levels above 3")),
        };
        return Ok(NotionBlockPatch::new(
            kind,
            json!({ "rich_text": rich_text(text) }),
        ));
    }

    if let Some((checked, text)) = parse_to_do(trimmed) {
        return Ok(NotionBlockPatch::new(
            "to_do",
            json!({
                "rich_text": rich_text(text),
                "checked": checked,
            }),
        ));
    }

    if let Some(text) = parse_bulleted_list_item(trimmed) {
        return Ok(NotionBlockPatch::new(
            "bulleted_list_item",
            json!({ "rich_text": rich_text(text) }),
        ));
    }

    if let Some(text) = parse_numbered_list_item(trimmed) {
        return Ok(NotionBlockPatch::new(
            "numbered_list_item",
            json!({ "rich_text": rich_text(text) }),
        ));
    }

    if let Some(text) = parse_quote(trimmed) {
        return Ok(NotionBlockPatch::new(
            "quote",
            json!({ "rich_text": rich_text(&text) }),
        ));
    }

    if looks_like_markdown_table(trimmed) {
        return Err(AfsError::Unsupported("writing Notion tables"));
    }

    Ok(NotionBlockPatch::new(
        "paragraph",
        json!({ "rich_text": rich_text(trimmed) }),
    ))
}

fn append_body(child: Value, after: Option<&RemoteId>) -> Value {
    match after {
        Some(after) => json!({
            "children": [child],
            "position": {
                "type": "after_block",
                "after_block": after.0,
            },
        }),
        None => json!({
            "children": [child],
            "position": {
                "type": "start",
            },
        }),
    }
}

fn rich_text(content: &str) -> Value {
    json!([
        {
            "type": "text",
            "text": {
                "content": content,
            },
        }
    ])
}

fn parse_code_fence(markdown: &str) -> Option<(String, String)> {
    let mut lines = markdown.lines();
    let first = lines.next()?.trim_start();
    let fence = if first.starts_with("```") {
        "```"
    } else if first.starts_with("~~~") {
        "~~~"
    } else {
        return None;
    };
    let language = first[fence.len()..].trim();
    let mut body = lines.collect::<Vec<_>>();
    if !body
        .last()
        .is_some_and(|line| line.trim_start().starts_with(fence))
    {
        return None;
    }
    body.pop();
    Some((language.to_string(), body.join("\n")))
}

fn parse_heading(markdown: &str) -> Option<(usize, &str)> {
    let trimmed = markdown.trim_start();
    let level = trimmed.chars().take_while(|ch| *ch == '#').count();
    if !(1..=6).contains(&level) || !trimmed[level..].starts_with(' ') {
        return None;
    }

    Some((level, trimmed[level..].trim_start()))
}

fn parse_to_do(markdown: &str) -> Option<(bool, &str)> {
    let trimmed = markdown.trim_start();
    if let Some(text) = trimmed.strip_prefix("- [ ] ") {
        return Some((false, text));
    }
    if let Some(text) = trimmed
        .strip_prefix("- [x] ")
        .or_else(|| trimmed.strip_prefix("- [X] "))
    {
        return Some((true, text));
    }
    None
}

fn parse_bulleted_list_item(markdown: &str) -> Option<&str> {
    let trimmed = markdown.trim_start();
    trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
}

fn parse_numbered_list_item(markdown: &str) -> Option<&str> {
    let trimmed = markdown.trim_start();
    let digit_count = trimmed.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digit_count == 0 || !trimmed[digit_count..].starts_with(". ") {
        return None;
    }

    Some(&trimmed[digit_count + 2..])
}

fn parse_quote(markdown: &str) -> Option<String> {
    let mut lines = Vec::new();
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        let text = trimmed.strip_prefix("> ")?;
        if text.starts_with("[!") {
            return None;
        }
        lines.push(text);
    }

    Some(lines.join("\n"))
}

fn looks_like_markdown_table(markdown: &str) -> bool {
    let mut lines = markdown.lines();
    let Some(first) = lines.next() else {
        return false;
    };
    let Some(second) = lines.next() else {
        return false;
    };
    first.contains('|')
        && second.contains('|')
        && second
            .trim()
            .chars()
            .all(|ch| matches!(ch, '|' | '-' | ':' | ' '))
}

fn unsupported_operation_name(operation: &PushOperation) -> &'static str {
    match operation {
        PushOperation::MoveBlock { .. } => "moving Notion blocks",
        PushOperation::ArchiveEntity { .. } => "archiving Notion pages",
        PushOperation::UpdateProperties { .. } => "updating Notion properties",
        PushOperation::CreateEntity { .. } => "creating Notion pages",
        PushOperation::UpdateBlock { .. }
        | PushOperation::AppendBlock { .. }
        | PushOperation::ArchiveBlock { .. } => "unsupported Notion push operation",
    }
}

fn unsupported_undo_name(operation: &UndoOperation) -> &'static str {
    match operation {
        UndoOperation::MoveBlock { .. } => "undoing Notion block moves",
        UndoOperation::RestoreArchivedBlock { .. } => "restoring archived Notion blocks",
        UndoOperation::RestoreBlockContent { .. }
        | UndoOperation::ArchiveCreatedBlock { .. }
        | UndoOperation::ArchiveCreatedEntity { .. } => "unsupported Notion undo operation",
    }
}
