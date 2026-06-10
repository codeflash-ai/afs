use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationIssue {
    pub code: String,
    pub file: PathBuf,
    pub line: Option<usize>,
    pub message: String,
    pub suggested_fix: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ValidationReport {
    pub issues: Vec<ValidationIssue>,
}

impl ValidationReport {
    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DirectiveIntegrity {
    Intact,
    Moved,
    Deleted,
    Mangled,
}
