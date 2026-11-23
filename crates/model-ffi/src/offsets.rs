//! `(line, byte column)` → character offsets, for sources read from disk.
//!
//! Same convention as the Moose exporter (`model-moose`): `start` is the
//! 1-based offset of the first character, `end` the 1-based offset of the last
//! one (inclusive); a character is a Unicode scalar value, as Pharo counts.
//! Kept here, not shared, so this crate depends on `model-core` only.

use std::path::Path;

/// A source file indexed for `(line, byte column)` → character offset.
pub(crate) struct FileIndex {
    line_bytes: Vec<usize>,
    /// Characters before each line; only for non-ASCII text.
    line_chars: Vec<usize>,
    /// The text, kept only when it is not ASCII (offsets = bytes otherwise).
    text: Option<String>,
    len: usize,
}

impl FileIndex {
    pub(crate) fn load(path: &Path) -> std::io::Result<FileIndex> {
        Ok(FileIndex::new(std::fs::read_to_string(path)?))
    }

    pub(crate) fn new(s: String) -> FileIndex {
        let mut line_bytes = vec![0];
        line_bytes.extend(s.match_indices('\n').map(|(i, _)| i + 1));
        let len = s.len();
        if s.is_ascii() {
            return FileIndex {
                line_bytes,
                line_chars: Vec::new(),
                text: None,
                len,
            };
        }
        let line_chars = line_bytes.iter().map(|&b| s[..b].chars().count()).collect();
        FileIndex {
            line_bytes,
            line_chars,
            text: Some(s),
            len,
        }
    }

    /// 0-based character offset of `(line, column in bytes)`; positions past
    /// the end are clamped to it.
    fn offset(&self, line: u32, col: u32) -> usize {
        let line = (line as usize).min(self.line_bytes.len() - 1);
        let start = self.line_bytes[line];
        let byte = (start + col as usize).min(self.len);
        match &self.text {
            None => byte,
            Some(t) => {
                let mut b = byte;
                while !t.is_char_boundary(b) {
                    b -= 1;
                }
                self.line_chars[line] + t[start..b].chars().count()
            }
        }
    }

    /// `[start_line, start_col, end_line, end_col]` → `(start, end)` as
    /// described in the module doc.
    pub(crate) fn span(&self, range: [u32; 4]) -> (u32, u32) {
        let start = self.offset(range[0], range[1]) + 1;
        let end = self.offset(range[2], range[3]).max(start);
        (clamp(start), clamp(end))
    }
}

fn clamp(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_offsets_are_bytes() {
        let f = FileIndex::new("ab\ncd".to_string());
        // "ab" on line 0: first char 1, last char 2 (end exclusive col 2).
        assert_eq!(f.span([0, 0, 0, 2]), (1, 2));
        assert_eq!(f.span([1, 0, 1, 2]), (4, 5));
    }

    #[test]
    fn non_ascii_counts_characters() {
        // "é" is two bytes, one character.
        let f = FileIndex::new("éa\nb".to_string());
        assert_eq!(f.span([0, 0, 0, 2]), (1, 1));
        assert_eq!(f.span([0, 2, 0, 3]), (2, 2));
        assert_eq!(f.span([1, 0, 1, 1]), (4, 4));
    }

    #[test]
    fn positions_past_the_end_are_clamped() {
        let f = FileIndex::new("ab".to_string());
        assert_eq!(f.span([9, 9, 9, 9]), (3, 3));
    }
}
