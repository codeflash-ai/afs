//! Human-review diff artifacts for explicit push previews.
//!
//! Review diffs are an inspectable text surface only. `PushPlan` remains the
//! source of truth for remote mutations.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::canonical::render_canonical_markdown;
use crate::diff::align_blocks;
use crate::model::CanonicalDocument;
use crate::planner::{PushOperation, PushPlan};
use crate::shadow::{ShadowDocument, segment_markdown_body};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewDiff {
    pub old_label: String,
    pub new_label: String,
    pub hunks: Vec<ReviewHunk>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewHunk {
    pub old_start: usize,
    pub new_start: usize,
    pub old_lines: usize,
    pub new_lines: usize,
    pub operations: Vec<ReviewOperation>,
    pub lines: Vec<ReviewLine>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewOperation {
    pub operation_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

impl ReviewOperation {
    pub fn label(&self) -> String {
        let mut parts = vec![self.operation_type.clone()];
        if let Some(block_id) = &self.block_id {
            parts.push(block_id.clone());
        } else if let Some(entity_id) = &self.entity_id {
            parts.push(entity_id.clone());
        } else if let Some(parent_id) = &self.parent_id {
            parts.push(format!("parent={parent_id}"));
        }
        if let Some(after) = &self.after {
            parts.push(format!("after={after}"));
        }
        parts.join(" ")
    }

    fn key(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}",
            self.operation_type,
            self.block_id.as_deref().unwrap_or_default(),
            self.entity_id.as_deref().unwrap_or_default(),
            self.parent_id.as_deref().unwrap_or_default(),
            self.after.as_deref().unwrap_or_default()
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewLine {
    pub kind: ReviewLineKind,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_type: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewLineKind {
    Context,
    Add,
    Remove,
}

pub fn review_diff_for_existing_document(
    old_label: impl Into<String>,
    new_label: impl Into<String>,
    shadow: &ShadowDocument,
    edited_body: &str,
    edited_body_start_line: usize,
    local_text: &str,
    plan: &PushPlan,
) -> ReviewDiff {
    let synced_text = render_canonical_markdown(&CanonicalDocument::new(
        shadow.frontmatter.clone(),
        shadow.rendered_body.clone(),
    ));
    let operations = review_operations(plan);
    let old_annotations = old_line_annotations(shadow, &synced_text, &operations);
    let new_annotations = new_line_annotations(
        shadow,
        edited_body,
        edited_body_start_line,
        local_text,
        &operations,
    );

    build_review_diff(
        old_label,
        new_label,
        &synced_text,
        local_text,
        old_annotations,
        new_annotations,
        operations,
    )
}

pub fn review_diff_for_create_entity(
    old_label: impl Into<String>,
    new_label: impl Into<String>,
    local_text: &str,
    plan: &PushPlan,
) -> ReviewDiff {
    let operations = review_operations(plan);
    let operation_keys = operations
        .iter()
        .filter(|operation| operation.operation_type == "create_entity")
        .map(ReviewOperation::key)
        .collect::<Vec<_>>();
    let new_annotations = (1..=split_lines(local_text).len())
        .map(|line| {
            (
                line,
                LineAnnotation {
                    block_id: None,
                    operation_keys: operation_keys.clone(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();

    build_review_diff(
        old_label,
        new_label,
        "",
        local_text,
        BTreeMap::new(),
        new_annotations,
        operations,
    )
}

fn build_review_diff(
    old_label: impl Into<String>,
    new_label: impl Into<String>,
    old_text: &str,
    new_text: &str,
    old_annotations: BTreeMap<usize, LineAnnotation>,
    new_annotations: BTreeMap<usize, LineAnnotation>,
    operations: Vec<ReviewOperation>,
) -> ReviewDiff {
    let old_lines = split_lines(old_text);
    let new_lines = split_lines(new_text);
    let operation_by_key = operations
        .iter()
        .map(|operation| (operation.key(), operation))
        .collect::<BTreeMap<_, _>>();
    let script = diff_script(&old_lines, &new_lines)
        .into_iter()
        .map(|line| annotated_line(line, &old_annotations, &new_annotations, &operation_by_key))
        .collect::<Vec<_>>();
    let ranges = hunk_ranges(&script, 3);
    let hunks = ranges
        .into_iter()
        .map(|(start, end)| build_hunk(start, end, &script, &operations))
        .collect();

    ReviewDiff {
        old_label: old_label.into(),
        new_label: new_label.into(),
        hunks,
    }
}

fn review_operations(plan: &PushPlan) -> Vec<ReviewOperation> {
    plan.operations
        .iter()
        .map(|operation| match operation {
            PushOperation::UpdateBlock { block_id, .. } => ReviewOperation {
                operation_type: "update_block".to_string(),
                block_id: Some(block_id.0.clone()),
                entity_id: None,
                parent_id: None,
                after: None,
            },
            PushOperation::AppendBlock {
                parent_id, after, ..
            } => ReviewOperation {
                operation_type: "append_block".to_string(),
                block_id: None,
                entity_id: None,
                parent_id: Some(parent_id.0.clone()),
                after: after.as_ref().map(|remote_id| remote_id.0.clone()),
            },
            PushOperation::MoveBlock { block_id, after } => ReviewOperation {
                operation_type: "move_block".to_string(),
                block_id: Some(block_id.0.clone()),
                entity_id: None,
                parent_id: None,
                after: after.as_ref().map(|remote_id| remote_id.0.clone()),
            },
            PushOperation::ArchiveBlock { block_id } => ReviewOperation {
                operation_type: "archive_block".to_string(),
                block_id: Some(block_id.0.clone()),
                entity_id: None,
                parent_id: None,
                after: None,
            },
            PushOperation::ArchiveEntity { entity_id } => ReviewOperation {
                operation_type: "archive_entity".to_string(),
                block_id: None,
                entity_id: Some(entity_id.0.clone()),
                parent_id: None,
                after: None,
            },
            PushOperation::UpdateProperties { entity_id, .. } => ReviewOperation {
                operation_type: "update_properties".to_string(),
                block_id: None,
                entity_id: Some(entity_id.0.clone()),
                parent_id: None,
                after: None,
            },
            PushOperation::CreateEntity { parent_id, .. } => ReviewOperation {
                operation_type: "create_entity".to_string(),
                block_id: None,
                entity_id: None,
                parent_id: Some(parent_id.0.clone()),
                after: None,
            },
        })
        .collect()
}

fn old_line_annotations(
    shadow: &ShadowDocument,
    synced_text: &str,
    operations: &[ReviewOperation],
) -> BTreeMap<usize, LineAnnotation> {
    let block_operations = block_operation_keys(operations);
    let property_operations = property_operation_keys(operations);
    let body_start_line = body_start_line(synced_text);
    let mut annotations = BTreeMap::new();

    if !property_operations.is_empty() {
        for line in 1..body_start_line {
            annotations.insert(
                line,
                LineAnnotation {
                    block_id: None,
                    operation_keys: property_operations.clone(),
                },
            );
        }
    }

    for block in &shadow.blocks {
        let block_id = block.remote_id.0.clone();
        let operation_keys = block_operations.get(&block_id).cloned().unwrap_or_default();
        for line in block.source_span.start_line..=block.source_span.end_line {
            annotations.insert(
                line,
                LineAnnotation {
                    block_id: Some(block_id.clone()),
                    operation_keys: operation_keys.clone(),
                },
            );
        }
    }

    annotations
}

fn new_line_annotations(
    shadow: &ShadowDocument,
    edited_body: &str,
    edited_body_start_line: usize,
    local_text: &str,
    operations: &[ReviewOperation],
) -> BTreeMap<usize, LineAnnotation> {
    let block_operations = block_operation_keys(operations);
    let property_operations = property_operation_keys(operations);
    let append_operations = append_operation_keys(operations);
    let edited_blocks = segment_markdown_body(edited_body, edited_body_start_line);
    let (matches, _) = align_blocks(shadow, &edited_blocks);
    let body_start_line = body_start_line(local_text);
    let mut append_index = 0usize;
    let mut annotations = BTreeMap::new();

    if !property_operations.is_empty() {
        for line in 1..body_start_line {
            annotations.insert(
                line,
                LineAnnotation {
                    block_id: None,
                    operation_keys: property_operations.clone(),
                },
            );
        }
    }

    for (index, block) in edited_blocks.iter().enumerate() {
        let (block_id, operation_keys) = match matches[index] {
            Some(shadow_index) => {
                let block_id = shadow.blocks[shadow_index].remote_id.0.clone();
                let operation_keys = block_operations.get(&block_id).cloned().unwrap_or_default();
                (Some(block_id), operation_keys)
            }
            None => {
                let operation_keys = append_operations
                    .get(append_index)
                    .cloned()
                    .map(|key| vec![key])
                    .unwrap_or_default();
                append_index += 1;
                (None, operation_keys)
            }
        };

        for line in block.source_span.start_line..=block.source_span.end_line {
            annotations.insert(
                line,
                LineAnnotation {
                    block_id: block_id.clone(),
                    operation_keys: operation_keys.clone(),
                },
            );
        }
    }

    annotations
}

fn block_operation_keys(operations: &[ReviewOperation]) -> BTreeMap<String, Vec<String>> {
    let mut keys = BTreeMap::<String, Vec<String>>::new();
    for operation in operations {
        if let Some(block_id) = &operation.block_id {
            keys.entry(block_id.clone())
                .or_default()
                .push(operation.key());
        }
    }
    keys
}

fn property_operation_keys(operations: &[ReviewOperation]) -> Vec<String> {
    operations
        .iter()
        .filter(|operation| operation.operation_type == "update_properties")
        .map(ReviewOperation::key)
        .collect()
}

fn append_operation_keys(operations: &[ReviewOperation]) -> Vec<String> {
    operations
        .iter()
        .filter(|operation| operation.operation_type == "append_block")
        .map(ReviewOperation::key)
        .collect()
}

fn annotated_line(
    line: ScriptLine,
    old_annotations: &BTreeMap<usize, LineAnnotation>,
    new_annotations: &BTreeMap<usize, LineAnnotation>,
    operation_by_key: &BTreeMap<String, &ReviewOperation>,
) -> AnnotatedReviewLine {
    let annotation = match line.kind {
        ReviewLineKind::Remove => line
            .old_line
            .and_then(|line| old_annotations.get(&line).cloned())
            .unwrap_or_default(),
        ReviewLineKind::Add => line
            .new_line
            .and_then(|line| new_annotations.get(&line).cloned())
            .unwrap_or_default(),
        ReviewLineKind::Context => combine_annotations(
            line.old_line.and_then(|line| old_annotations.get(&line)),
            line.new_line.and_then(|line| new_annotations.get(&line)),
        ),
    };
    let operation_type = annotation
        .operation_keys
        .iter()
        .find_map(|key| operation_by_key.get(key))
        .map(|operation| operation.operation_type.clone());

    AnnotatedReviewLine {
        line: ReviewLine {
            kind: line.kind,
            text: line.text,
            old_line: line.old_line,
            new_line: line.new_line,
            block_id: annotation.block_id,
            operation_type,
        },
        operation_keys: annotation.operation_keys,
    }
}

fn combine_annotations(
    old: Option<&LineAnnotation>,
    new: Option<&LineAnnotation>,
) -> LineAnnotation {
    let mut annotation = LineAnnotation {
        block_id: old
            .and_then(|annotation| annotation.block_id.clone())
            .or_else(|| new.and_then(|annotation| annotation.block_id.clone())),
        operation_keys: Vec::new(),
    };
    let mut seen = BTreeSet::new();
    for key in old
        .into_iter()
        .chain(new)
        .flat_map(|annotation| annotation.operation_keys.iter())
    {
        if seen.insert(key.clone()) {
            annotation.operation_keys.push(key.clone());
        }
    }
    annotation
}

fn build_hunk(
    start: usize,
    end: usize,
    script: &[AnnotatedReviewLine],
    operations: &[ReviewOperation],
) -> ReviewHunk {
    let lines = &script[start..end];
    let old_start = lines
        .iter()
        .find_map(|line| line.line.old_line)
        .unwrap_or_else(|| previous_old_line(script, start));
    let new_start = lines
        .iter()
        .find_map(|line| line.line.new_line)
        .unwrap_or_else(|| previous_new_line(script, start));
    let old_lines = lines
        .iter()
        .filter(|line| line.line.old_line.is_some())
        .count();
    let new_lines = lines
        .iter()
        .filter(|line| line.line.new_line.is_some())
        .count();
    let operation_keys = lines
        .iter()
        .flat_map(|line| line.operation_keys.iter().cloned())
        .collect::<BTreeSet<_>>();
    let hunk_operations = operations
        .iter()
        .filter(|operation| operation_keys.contains(&operation.key()))
        .cloned()
        .collect();

    ReviewHunk {
        old_start,
        new_start,
        old_lines,
        new_lines,
        operations: hunk_operations,
        lines: lines.iter().map(|line| line.line.clone()).collect(),
    }
}

fn previous_old_line(script: &[AnnotatedReviewLine], start: usize) -> usize {
    script[..start]
        .iter()
        .rev()
        .find_map(|line| line.line.old_line)
        .unwrap_or(0)
}

fn previous_new_line(script: &[AnnotatedReviewLine], start: usize) -> usize {
    script[..start]
        .iter()
        .rev()
        .find_map(|line| line.line.new_line)
        .unwrap_or(0)
}

fn hunk_ranges(script: &[AnnotatedReviewLine], context: usize) -> Vec<(usize, usize)> {
    let mut ranges = Vec::<(usize, usize)>::new();

    for (index, line) in script.iter().enumerate() {
        if line.line.kind == ReviewLineKind::Context {
            continue;
        }
        let start = index.saturating_sub(context);
        let end = (index + context + 1).min(script.len());
        if let Some((_, last_end)) = ranges.last_mut()
            && start <= *last_end
        {
            *last_end = (*last_end).max(end);
            continue;
        }
        ranges.push((start, end));
    }

    ranges
}

fn diff_script(old_lines: &[String], new_lines: &[String]) -> Vec<ScriptLine> {
    let mut lengths = vec![vec![0usize; new_lines.len() + 1]; old_lines.len() + 1];
    for old_index in (0..old_lines.len()).rev() {
        for new_index in (0..new_lines.len()).rev() {
            lengths[old_index][new_index] = if old_lines[old_index] == new_lines[new_index] {
                lengths[old_index + 1][new_index + 1] + 1
            } else {
                lengths[old_index + 1][new_index].max(lengths[old_index][new_index + 1])
            };
        }
    }

    let mut old_index = 0usize;
    let mut new_index = 0usize;
    let mut script = Vec::new();
    while old_index < old_lines.len() && new_index < new_lines.len() {
        if old_lines[old_index] == new_lines[new_index] {
            script.push(ScriptLine {
                kind: ReviewLineKind::Context,
                text: old_lines[old_index].clone(),
                old_line: Some(old_index + 1),
                new_line: Some(new_index + 1),
            });
            old_index += 1;
            new_index += 1;
        } else if lengths[old_index + 1][new_index] >= lengths[old_index][new_index + 1] {
            script.push(ScriptLine {
                kind: ReviewLineKind::Remove,
                text: old_lines[old_index].clone(),
                old_line: Some(old_index + 1),
                new_line: None,
            });
            old_index += 1;
        } else {
            script.push(ScriptLine {
                kind: ReviewLineKind::Add,
                text: new_lines[new_index].clone(),
                old_line: None,
                new_line: Some(new_index + 1),
            });
            new_index += 1;
        }
    }

    while old_index < old_lines.len() {
        script.push(ScriptLine {
            kind: ReviewLineKind::Remove,
            text: old_lines[old_index].clone(),
            old_line: Some(old_index + 1),
            new_line: None,
        });
        old_index += 1;
    }
    while new_index < new_lines.len() {
        script.push(ScriptLine {
            kind: ReviewLineKind::Add,
            text: new_lines[new_index].clone(),
            old_line: None,
            new_line: Some(new_index + 1),
        });
        new_index += 1;
    }

    script
}

fn split_lines(text: &str) -> Vec<String> {
    text.lines().map(str::to_string).collect()
}

fn body_start_line(text: &str) -> usize {
    let mut lines = text.lines();
    if lines.next() != Some("---") {
        return 1;
    }

    for (index, line) in lines.enumerate() {
        if line == "---" {
            return index + 3;
        }
    }

    1
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct LineAnnotation {
    block_id: Option<String>,
    operation_keys: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AnnotatedReviewLine {
    line: ReviewLine,
    operation_keys: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ScriptLine {
    kind: ReviewLineKind,
    text: String,
    old_line: Option<usize>,
    new_line: Option<usize>,
}
