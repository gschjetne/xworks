//! Pagination over random documents (docs/ROADMAP.md, M1 stage 7): every
//! line placed once, in order, within its page; and page breaks that stay
//! put when nothing before them changes.

use canvas::{Block, Piece, paginate};
use proptest::prelude::*;

fn block() -> impl Strategy<Value = Block> {
    (
        prop::collection::vec(5..40i64, 1..12),
        0..20i64,
        0..20i64,
        prop::bool::weighted(0.25),
    )
        .prop_map(|(lines, space_before, space_after, keep_with_next)| Block {
            lines,
            space_before,
            space_after,
            keep_with_next,
        })
}

fn document() -> impl Strategy<Value = (i64, Vec<Block>)> {
    (60..300i64, prop::collection::vec(block(), 0..30))
}

/// The page holding the first line of block `b`.
fn page_of(pages: &[Vec<Piece>], b: usize) -> usize {
    pages
        .iter()
        .position(|p| p.iter().any(|q| q.block == b && q.lines.start == 0))
        .expect("every block starts somewhere")
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn every_line_is_placed_once_in_order_and_fits((height, blocks) in document()) {
        let pages = paginate(height, &blocks);
        prop_assert!(!pages.is_empty());
        let mut next = (0, 0);
        for page in &pages {
            let mut bottom = 0;
            for (i, piece) in page.iter().enumerate() {
                prop_assert_eq!((piece.block, piece.lines.start), next);
                prop_assert!(piece.lines.start < piece.lines.end);
                prop_assert!(piece.y >= bottom, "pieces overlap");
                let block = &blocks[piece.block];
                let end = piece.y + block.lines[piece.lines.clone()].iter().sum::<i64>();
                // Only a page that can't hold even its first line overflows.
                let alone = i == 0 && piece.lines.len() == 1 && block.lines[piece.lines.start] > height;
                prop_assert!(end <= height || alone, "a piece ends at {} of {}", end, height);
                bottom = end;
                next = if piece.lines.end == block.lines.len() {
                    (piece.block + 1, 0)
                } else {
                    (piece.block, piece.lines.end)
                };
            }
        }
        prop_assert_eq!(next, (blocks.len(), 0));
    }

    #[test]
    fn breaks_before_an_edit_stay_put(
        (height, blocks) in document(),
        at in any::<prop::sample::Index>(),
        edit in block(),
    ) {
        prop_assume!(!blocks.is_empty());
        let k = at.index(blocks.len());
        let mut edited = blocks.clone();
        edited[k] = edit;
        // A run of blocks that keep with the next moves with the block
        // after it, so the edit can reach back to the run's start.
        let mut start = k;
        while start > 0 && blocks[start - 1].keep_with_next {
            start -= 1;
        }
        let before = paginate(height, &blocks);
        let after = paginate(height, &edited);
        let p = page_of(&before, start).min(page_of(&after, start));
        prop_assert_eq!(&before[..p], &after[..p]);
    }

    #[test]
    fn the_same_blocks_give_the_same_pages((height, blocks) in document()) {
        prop_assert_eq!(paginate(height, &blocks), paginate(height, &blocks));
    }
}
