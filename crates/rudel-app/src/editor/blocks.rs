#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BlockRange {
    pub(crate) from: usize,
    pub(crate) to: usize,
}

impl BlockRange {
    pub(crate) fn new(from: usize, to: usize) -> Self {
        Self { from, to }
    }

    pub(crate) fn is_empty_in(self, code: &str) -> bool {
        code.get(self.from..self.to)
            .is_none_or(|block| block.trim().is_empty())
    }
}

/// Blank-line-separated block regions, matching
/// `strudel/packages/codemirror/block_utilities.mjs`.
pub(crate) fn block_regions(code: &str) -> Vec<BlockRange> {
    let chars: Vec<(usize, char)> = code.char_indices().collect();
    let mut i = 0;
    let mut blanks: Vec<usize> = Vec::new();
    let mut block_start = 0;
    let mut regions = Vec::new();
    while i < chars.len() {
        let (byte, ch) = chars[i];
        if ch == '\n' {
            blanks.push(byte);
        } else if !ch.is_whitespace() {
            if blanks.len() > 1 {
                regions.push(BlockRange::new(block_start, blanks[0]));
                block_start = byte;
            }
            blanks.clear();
        }
        i += 1;
    }
    regions.push(BlockRange::new(
        block_start,
        blanks.first().copied().unwrap_or(code.len()),
    ));
    regions
}

pub(crate) fn block_at_byte(code: &str, cursor: usize) -> Option<BlockRange> {
    let cursor = cursor.min(code.len());
    block_regions(code)
        .into_iter()
        .find(|range| cursor >= range.from && cursor <= range.to)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_regions_split_on_blank_lines_like_strudel() {
        let code = "a\nb\n\nc\n\n\n  d";

        assert_eq!(
            block_regions(code),
            vec![
                BlockRange::new(0, 3),
                BlockRange::new(5, 6),
                BlockRange::new(11, code.len())
            ]
        );
    }

    #[test]
    fn block_at_accepts_cursors_on_region_edges() {
        let code = "a\n\nb";

        assert_eq!(block_at_byte(code, 0), Some(BlockRange::new(0, 1)));
        assert_eq!(block_at_byte(code, 1), Some(BlockRange::new(0, 1)));
        assert_eq!(block_at_byte(code, 3), Some(BlockRange::new(3, 4)));
    }

    #[test]
    fn trailing_blank_lines_are_not_part_of_the_block() {
        let code = "a\n\n";

        assert_eq!(block_regions(code), vec![BlockRange::new(0, 1)]);
    }
}
