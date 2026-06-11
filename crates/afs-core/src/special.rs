//! Structured write targets for Markdown blocks that map to nested remote data.
//!
//! Most rendered Markdown blocks map one-to-one to a remote block and can use
//! `UpdateBlock`. Some clean Markdown surfaces are structurally richer than one
//! remote block. A Notion simple table is the first example: AFS renders one
//! Markdown table, but Notion stores the editable cells on child `table_row`
//! blocks. This module keeps those special targets typed and connector-neutral
//! so future cases, such as column wrappers or writable media metadata, have a
//! single planning surface.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::model::RemoteId;
use crate::shadow::{MarkdownBlockKind, ShadowBlock};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StructuredWriteTarget {
    TableRows { rows: Vec<TableRowUpdate> },
}

impl StructuredWriteTarget {
    pub fn updated_block_count(&self) -> usize {
        match self {
            Self::TableRows { rows } => rows.len(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableRowUpdate {
    pub row_id: RemoteId,
    pub cells: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructuredWriteError {
    pub code: &'static str,
    pub message: String,
    pub suggestion: &'static str,
}

impl StructuredWriteError {
    fn new(code: &'static str, message: impl Into<String>, suggestion: &'static str) -> Self {
        Self {
            code,
            message: message.into(),
            suggestion,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ParsedMarkdownTable {
    rows: Vec<Vec<String>>,
    width: usize,
}

pub fn plan_table_row_updates(
    row_ids: &[RemoteId],
    has_column_header: bool,
    shadow_markdown: &str,
    edited_markdown: &str,
) -> Result<Vec<TableRowUpdate>, StructuredWriteError> {
    let shadow = parse_markdown_table(shadow_markdown)?;
    let edited = parse_markdown_table(edited_markdown)?;
    let shadow_rows = projected_remote_rows(&shadow, row_ids, has_column_header)?;
    let edited_rows = projected_remote_rows(&edited, row_ids, has_column_header)?;

    if shadow.width != edited.width {
        return Err(StructuredWriteError::new(
            "table_width_changed",
            "table edits cannot add or remove columns yet",
            "keep the same number of Markdown table columns and edit only cell contents",
        ));
    }

    if shadow_rows.len() != edited_rows.len() {
        return Err(StructuredWriteError::new(
            "table_row_count_changed",
            "table edits cannot add or remove rows yet",
            "keep the same number of Markdown table rows and edit only existing cells",
        ));
    }

    Ok(row_ids
        .iter()
        .zip(shadow_rows.iter().zip(edited_rows.iter()))
        .filter(|(_, (shadow_cells, edited_cells))| shadow_cells != edited_cells)
        .map(|(row_id, (_, edited_cells))| TableRowUpdate {
            row_id: row_id.clone(),
            cells: edited_cells.clone(),
        })
        .collect())
}

pub fn restore_structured_target(
    shadow_block: &ShadowBlock,
    target: &StructuredWriteTarget,
) -> Result<StructuredWriteTarget, StructuredWriteError> {
    match target {
        StructuredWriteTarget::TableRows { rows } => {
            let MarkdownBlockKind::TableWithRows {
                row_ids,
                has_column_header,
                ..
            } = &shadow_block.kind
            else {
                return Err(StructuredWriteError::new(
                    "structured_preimage_kind_mismatch",
                    "journaled preimage is not a table block",
                    "pull the page and retry the undo from a complete journal entry",
                ));
            };

            let row_filter = rows
                .iter()
                .map(|row| row.row_id.clone())
                .collect::<BTreeSet<_>>();
            let table = parse_markdown_table(&shadow_block.text)?;
            let projected_rows = projected_remote_rows(&table, row_ids, *has_column_header)?;
            let rows: Vec<TableRowUpdate> = row_ids
                .iter()
                .zip(projected_rows)
                .filter_map(|(row_id, cells)| {
                    row_filter.contains(row_id).then(|| TableRowUpdate {
                        row_id: row_id.clone(),
                        cells,
                    })
                })
                .collect();
            if rows.len() != row_filter.len() {
                return Err(StructuredWriteError::new(
                    "structured_preimage_missing_row",
                    "journaled table preimage is missing at least one changed row",
                    "pull the page and retry the undo from a complete journal entry",
                ));
            }

            Ok(StructuredWriteTarget::TableRows { rows })
        }
    }
}

fn projected_remote_rows(
    table: &ParsedMarkdownTable,
    row_ids: &[RemoteId],
    has_column_header: bool,
) -> Result<Vec<Vec<String>>, StructuredWriteError> {
    let rows = if has_column_header {
        table.rows.as_slice()
    } else {
        let Some((synthetic_header, rows)) = table.rows.split_first() else {
            return Err(invalid_shadow_table("table is missing rows"));
        };
        if synthetic_header.iter().any(|cell| !cell.trim().is_empty()) {
            return Err(StructuredWriteError::new(
                "table_synthetic_header_edited",
                "tables without a Notion column header must keep the synthetic Markdown header empty",
                "leave the first Markdown table row blank and edit the data rows below it",
            ));
        }
        rows
    };

    if rows.len() != row_ids.len() {
        return Err(StructuredWriteError::new(
            "table_row_count_changed",
            format!(
                "table has {} editable rows but the shadow tracks {} remote row IDs",
                rows.len(),
                row_ids.len()
            ),
            "keep the same number of Markdown table rows and pull again if the remote table changed",
        ));
    }

    Ok(rows.to_vec())
}

fn parse_markdown_table(markdown: &str) -> Result<ParsedMarkdownTable, StructuredWriteError> {
    let lines = markdown
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();

    if lines.len() < 2 || !is_table_separator(lines[1]) {
        return Err(StructuredWriteError::new(
            "table_unparseable",
            "edited table is not a valid Markdown table",
            "restore the Markdown table header, separator, and row pipe structure",
        ));
    }

    let rows = lines
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != 1)
        .map(|(_, line)| parse_table_row(line))
        .collect::<Result<Vec<_>, _>>()?;
    let width = rows.first().map(Vec::len).unwrap_or_default();

    if width == 0 || rows.iter().any(|row| row.len() != width) {
        return Err(StructuredWriteError::new(
            "table_width_changed",
            "table rows must all have the same number of cells",
            "keep every Markdown table row at the same column width",
        ));
    }

    let separator_width = parse_table_separator_width(lines[1]);
    if separator_width != width {
        return Err(StructuredWriteError::new(
            "table_width_changed",
            "table separator width does not match the table rows",
            "keep the Markdown table separator aligned with the table columns",
        ));
    }

    Ok(ParsedMarkdownTable { rows, width })
}

fn parse_table_row(line: &str) -> Result<Vec<String>, StructuredWriteError> {
    let trimmed = line.trim();
    if !trimmed.contains('|') {
        return Err(invalid_shadow_table("table row has no cell delimiters"));
    }

    let inner = trimmed
        .strip_prefix('|')
        .unwrap_or(trimmed)
        .strip_suffix('|')
        .unwrap_or_else(|| trimmed.strip_prefix('|').unwrap_or(trimmed));
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut escaped = false;

    for ch in inner.chars() {
        if escaped {
            if ch == '|' {
                current.push('|');
            } else {
                current.push('\\');
                current.push(ch);
            }
            escaped = false;
            continue;
        }

        match ch {
            '\\' => escaped = true,
            '|' => {
                cells.push(normalize_cell(&current));
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    if escaped {
        current.push('\\');
    }
    cells.push(normalize_cell(&current));

    Ok(cells)
}

fn normalize_cell(cell: &str) -> String {
    cell.trim().replace("<br>", "\n")
}

fn parse_table_separator_width(line: &str) -> usize {
    line.trim()
        .trim_matches('|')
        .split('|')
        .filter(|part| !part.trim().is_empty())
        .count()
}

fn is_table_separator(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.contains('|')
        && trimmed
            .chars()
            .all(|ch| matches!(ch, '|' | '-' | ':' | ' '))
        && trimmed.contains('-')
}

fn invalid_shadow_table(message: impl Into<String>) -> StructuredWriteError {
    StructuredWriteError::new(
        "table_unparseable",
        message,
        "pull the page again so AFS has a fresh table shadow",
    )
}
