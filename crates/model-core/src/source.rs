use serde::{Deserialize, Serialize};

/// 0-based, end exclusive (same convention as SCIP and tree-sitter points).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Range {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

impl Range {
    pub fn contains(&self, line: u32, col: u32) -> bool {
        (self.start_line, self.start_col) <= (line, col)
            && (line, col) < (self.end_line, self.end_col)
    }
}

/// Where an entity or a relation is written.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceAnchor {
    /// Path relative to the analysed root, '/' separators.
    pub file: String,
    /// Full extent.
    pub range: Range,
    /// Identifier only (definitions); used to match SCIP occurrences.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_range: Option<Range>,
}
