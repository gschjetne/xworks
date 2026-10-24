//! The story as screen readers see it, through AccessKit: an editable
//! text holding its paragraphs, headings with their levels, and each
//! paragraph's lines as text runs, with their graphemes, where each sits,
//! where its words start, and the selection in them. So a screen reader
//! reads by line, word and character as the text is laid out, not the
//! document as one string.

use part::Access;
use part::accesskit::{Node, NodeId, Rect, Role, TextDirection, TextPosition, TextSelection};
use text::{Cursor, Direction};

use super::{Editor, Position};

/// How many node IDs each paragraph has: its own and its lines'.
const PER_PARAGRAPH: u64 = 4096;

impl Editor {
    /// The ID of paragraph `k`'s node, from `first`.
    fn paragraph_node(&self, first: u64, k: usize) -> NodeId {
        NodeId(first + (self.laid.paragraphs[k].id.0 + 1) * PER_PARAGRAPH)
    }

    /// The ID of line `line` of paragraph `k`'s text run.
    fn run_node(&self, first: u64, k: usize, line: usize) -> NodeId {
        let line = (line as u64 + 1).min(PER_PARAGRAPH - 1);
        NodeId(self.paragraph_node(first, k).0 + line)
    }

    /// Where `p` is among the text runs: its line's node, and how many
    /// graphemes into the line.
    fn text_position(&self, first: u64, p: Position) -> TextPosition {
        let (k, cursor) = self.cursor(p);
        let layout = &self.laid.paragraphs[k].layout;
        let line = layout.line_of(cursor);
        let range = layout.lines[line].range.clone();
        let start = range.start;
        let text = &self.paragraph(p.paragraph).text[range];
        let index = locale::graphemes(text)
            .iter()
            .filter(|&&g| g > 0 && start + g <= cursor.offset)
            .count();
        TextPosition {
            node: self.run_node(first, k, line),
            character_index: index,
        }
    }

    /// The accessibility tree of the story, its nodes numbered from
    /// `first`, its bounds in layout units of the editor's area.
    pub(super) fn access_tree(&self, first: u64) -> Access {
        let root = NodeId(first);
        let mut nodes = Vec::new();
        let mut text = Node::new(Role::MultilineTextInput);
        text.set_label(self.name());
        let mut paragraphs = Vec::with_capacity(self.laid.paragraphs.len());
        let sheet = &self.sheet;
        let px = |v: i64| v as f64;
        for (k, laid) in self.laid.paragraphs.iter().enumerate() {
            let paragraph = self.paragraph(laid.id);
            let layout = &laid.layout;
            let id = self.paragraph_node(first, k);
            paragraphs.push(id);
            let mut node = match laid.heading {
                Some(level) => {
                    let mut n = Node::new(Role::Heading);
                    // AccessKit counts levels from 0.
                    n.set_level(usize::from(level).saturating_sub(1));
                    n
                }
                None => Node::new(Role::Paragraph),
            };
            // The paragraph's languages, for its words.
            let mut languages = Vec::new();
            let mut at = 0;
            for span in story::spans(paragraph, sheet) {
                languages.push((at..at + span.len, span.style.language.clone()));
                at += span.len;
            }
            let words = locale::words(&paragraph.text, &languages);
            let rtl = layout.direction() == Direction::Rtl;
            let mut runs = Vec::with_capacity(layout.lines.len());
            let last = layout.lines.len() - 1;
            for (l, line) in layout.lines.iter().enumerate() {
                let run_id = self.run_node(first, k, l);
                runs.push(run_id);
                let range = line.range.clone();
                let mut value = paragraph.text[range.clone()].to_string();
                let bounds_of = |l: usize| {
                    let (x, y) = self.line_origin(k, l);
                    Rect {
                        x0: px(x + line.x),
                        y0: px(y),
                        x1: px(x + line.x + line.width.max(1)),
                        y1: px(y + line.height),
                    }
                };
                let bounds = bounds_of(l);
                // Graphemes, in bytes, with the paragraph's end as one more
                // character on its last line.
                let starts: Vec<usize> = locale::graphemes(&value)
                    .into_iter()
                    .filter(|&g| g < value.len())
                    .collect();
                let mut lengths: Vec<u8> = starts
                    .iter()
                    .enumerate()
                    .map(|(i, &g)| {
                        let end = starts.get(i + 1).copied().unwrap_or(value.len());
                        (end - g).min(255) as u8
                    })
                    .collect();
                // Where each grapheme sits and how wide it is, from the
                // caret's places, for lines that read one way.
                let one_way = line.runs.iter().all(|r| (r.level % 2 == 1) == rtl);
                let mut positions = Vec::new();
                let mut widths = Vec::new();
                if one_way {
                    for (i, &g) in starts.iter().enumerate() {
                        let end = starts.get(i + 1).copied().unwrap_or(value.len());
                        let a = layout.caret(Cursor::downstream(range.start + g)).x;
                        let b = layout.caret(Cursor::upstream(range.start + end)).x;
                        let (from, to) = if rtl { (b, a) } else { (a, b) };
                        positions.push(if rtl {
                            (line.x + line.width - to.max(from)) as f32
                        } else {
                            (from - line.x) as f32
                        });
                        widths.push((to - from).unsigned_abs() as f32);
                    }
                }
                if l == last {
                    value.push('\n');
                    lengths.push(1);
                    positions.push(line.width as f32);
                    widths.push(0.0);
                }
                // Where words start on the line, in graphemes.
                let word_starts: Vec<u8> = words
                    .iter()
                    .filter(|w| w.word_like && range.contains(&w.range.start))
                    .filter_map(|w| {
                        starts
                            .iter()
                            .position(|&g| range.start + g == w.range.start)
                            .map(|i| i.min(255) as u8)
                    })
                    .collect();
                let mut run = Node::new(Role::TextRun);
                run.set_value(value);
                run.set_bounds(bounds);
                run.set_character_lengths(lengths);
                run.set_word_starts(word_starts);
                if one_way {
                    run.set_character_positions(positions);
                    run.set_character_widths(widths);
                }
                if rtl {
                    run.set_text_direction(TextDirection::RightToLeft);
                }
                nodes.push((run_id, run));
            }
            node.set_children(runs);
            nodes.push((id, node));
        }
        text.set_children(paragraphs);
        text.set_text_selection(TextSelection {
            anchor: self.text_position(first, self.anchor),
            focus: self.text_position(first, self.focus),
        });
        nodes.push((root, text));
        Access {
            nodes,
            root,
            focus: root,
        }
    }
}
