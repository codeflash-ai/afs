#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotionBlockClass {
    Paragraph,
    Heading,
    Quote,
    Callout,
    List,
    Toggle,
    Code,
    Table,
    Equation,
    Mention,
    Media,
    Embed,
    SyncedBlock,
    ChildDatabase,
    ColumnLayout,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoundTripStrategy {
    CleanDiff,
    AnchoredDirective,
    Structural,
    OpaqueShadow,
}

pub fn strategy_for(block: &NotionBlockClass) -> RoundTripStrategy {
    use NotionBlockClass::*;

    match block {
        Paragraph | Heading | Quote | Callout | List | Toggle | Code | Table | Equation
        | Mention => RoundTripStrategy::CleanDiff,
        Media | Embed | SyncedBlock | ColumnLayout => RoundTripStrategy::AnchoredDirective,
        ChildDatabase => RoundTripStrategy::Structural,
        Unsupported => RoundTripStrategy::OpaqueShadow,
    }
}

pub fn directive(id: &str, directive_type: &str, title: Option<&str>) -> String {
    match title {
        Some(title) => format!("::afs{{id={id} type={directive_type} title=\"{title}\"}}"),
        None => format!("::afs{{id={id} type={directive_type}}}"),
    }
}
