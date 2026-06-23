use afs_core::{AfsError, AfsResult};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkdownTableShape {
    pub header: Vec<String>,
    pub width: usize,
    pub row_widths: Vec<usize>,
}

pub fn parse_markdown_table_shape(markdown: &str) -> AfsResult<MarkdownTableShape> {
    let lines = markdown
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    if lines.len() < 2 {
        return Err(malformed_table());
    }

    let header = parse_markdown_table_row(lines[0])?;
    validate_markdown_table_separator(lines[1], header.len())?;
    let row_widths = lines[2..]
        .iter()
        .map(|line| parse_markdown_table_row(line).map(|row| row.len()))
        .collect::<AfsResult<Vec<_>>>()?;
    Ok(MarkdownTableShape {
        width: header.len(),
        header,
        row_widths,
    })
}

pub fn parse_markdown_table_row(line: &str) -> AfsResult<Vec<String>> {
    let trimmed = line.trim();
    if !trimmed.starts_with('|') || !trimmed.ends_with('|') || trimmed.len() < 2 {
        return Err(malformed_table());
    }

    let inner = &trimmed[1..trimmed.len() - 1];
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    for ch in inner.chars() {
        if ch == '|' && !escaped {
            cells.push(unescape_markdown_table_cell(current.trim()));
            current.clear();
        } else {
            current.push(ch);
        }
        escaped = ch == '\\' && !escaped;
        if ch != '\\' {
            escaped = false;
        }
    }
    cells.push(unescape_markdown_table_cell(current.trim()));

    Ok(cells)
}

pub fn validate_markdown_table_separator(line: &str, width: usize) -> AfsResult<()> {
    let cells = parse_markdown_table_row(line)?;
    let valid = cells.len() == width
        && cells.iter().all(|cell| {
            let trimmed = cell.trim();
            trimmed.contains('-') && trimmed.chars().all(|ch| matches!(ch, '-' | ':' | ' '))
        });
    if valid {
        Ok(())
    } else {
        Err(malformed_table())
    }
}

fn unescape_markdown_table_cell(cell: &str) -> String {
    cell.replace("\\|", "|").replace("<br>", "\n")
}

fn malformed_table() -> AfsError {
    AfsError::Unsupported("writing malformed Notion tables")
}

#[cfg(test)]
mod tests {
    use super::{parse_markdown_table_row, parse_markdown_table_shape};

    #[test]
    fn parses_table_shape_and_row_widths() {
        let shape = parse_markdown_table_shape("| Name | Status |\n| --- | --- |\n| Old | Todo |")
            .expect("shape");

        assert_eq!(shape.width, 2);
        assert_eq!(shape.header, vec!["Name", "Status"]);
        assert_eq!(shape.row_widths, vec![2]);
    }

    #[test]
    fn parses_escaped_pipe_and_line_break_cells() {
        let row = parse_markdown_table_row("| A\\|B | hello<br>world |").expect("row");

        assert_eq!(row, vec!["A|B", "hello\nworld"]);
    }
}
