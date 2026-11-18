use serde::{Deserialize, Serialize};

/// 0-based, end exclusive (same convention as SCIP and tree-sitter points).
/// Serialised as `[start_line, start_col, end_line, end_col]`.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(from = "[u32; 4]", into = "[u32; 4]")]
pub struct Range {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

impl Range {
    pub fn new(start_line: u32, start_col: u32, end_line: u32, end_col: u32) -> Self {
        Range {
            start_line,
            start_col,
            end_line,
            end_col,
        }
    }

    pub fn contains(&self, line: u32, col: u32) -> bool {
        (self.start_line, self.start_col) <= (line, col)
            && (line, col) < (self.end_line, self.end_col)
    }
}

impl From<[u32; 4]> for Range {
    fn from([a, b, c, d]: [u32; 4]) -> Self {
        Range::new(a, b, c, d)
    }
}

impl From<Range> for [u32; 4] {
    fn from(r: Range) -> Self {
        [r.start_line, r.start_col, r.end_line, r.end_col]
    }
}
