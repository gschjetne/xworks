//! What an item covers, and what changed between two display lists: the
//! places a window draws again, rather than all of it.

use std::hash::{Hash, Hasher};

use crate::{Glyphs, Item, Page, Path, Rect, Segment, Transform};

/// The most changes [`changed`] looks for before it gives up and says the
/// whole page changed, as it does when a scroll moves everything.
const MOST_CHANGES: usize = 256;

impl Item {
    /// A rectangle holding everything the item draws, anti-aliasing
    /// included, in its page's units; `None` if it draws nothing. It's
    /// generous: glyphs are taken to fill their font's bounding box, and
    /// strokes to reach out twice their width.
    pub fn bounds(&self) -> Option<Rect> {
        match self {
            Item::Fill { path, .. } => path_bounds(path),
            Item::Stroke { path, width, .. } => {
                // Miter joins reach out at most twice the width, at
                // vello's limit of four.
                path_bounds(path).map(|r| r.outset(2 * width.abs() + 1))
            }
            Item::Glyphs(run) => glyph_bounds(run),
            Item::Image { rect, .. } => (!rect.is_empty()).then_some(*rect),
            Item::Group(group) => {
                let inner = group
                    .items
                    .iter()
                    .filter_map(Item::bounds)
                    .reduce(Rect::union)?;
                let outer = group.transform.bounds(inner);
                match &group.clip {
                    Some(clip) => outer.intersection(path_bounds(clip)?),
                    None => Some(outer),
                }
            }
        }
    }
}

/// The places where `new` draws differently from `old`, as rectangles in
/// their units: none if they draw the same, the whole page if their sizes
/// differ or too much changed to say where. Items that `new` draws in the
/// same order as `old`, unchanged, are left out; drawing every item over
/// the rectangles again, in order, gives what drawing all of `new` would.
pub fn changed(old: &Page, new: &Page) -> Vec<Rect> {
    let whole = vec![Rect::new(0, 0, new.width, new.height)];
    if (old.width, old.height) != (new.width, new.height) {
        return whole;
    }
    let a: Vec<u64> = old.items.iter().map(fingerprint).collect();
    let b: Vec<u64> = new.items.iter().map(fingerprint).collect();
    let Some((gone, came)) = diff(&a, &b, MOST_CHANGES) else {
        return whole;
    };
    let mut rects: Vec<Rect> = Vec::new();
    let changes = gone
        .iter()
        .map(|&i| &old.items[i])
        .chain(came.iter().map(|&i| &new.items[i]));
    for item in changes {
        let Some(r) = item.bounds() else { continue };
        let Some(r) = r.intersection(whole[0]) else {
            continue;
        };
        // Rectangles that touch are kept apart; the renderers merge them
        // as pixels.
        if !rects.contains(&r) {
            rects.push(r);
        }
    }
    rects
}

/// The most rectangles [`merge`] leaves: drawing a few larger ones costs
/// less than many small ones.
const MOST_RECTS: usize = 8;

/// Rectangles closer than this, in pixels, are drawn as one.
const NEAR: u64 = 64 * 64;

/// A rectangle of whole pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PixelRect {
    /// Its left edge.
    pub x: u32,
    /// Its top edge.
    pub y: u32,
    /// Its width.
    pub width: u32,
    /// Its height.
    pub height: u32,
}

impl PixelRect {
    fn area(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    fn union(&self, other: &PixelRect) -> PixelRect {
        let (x, y) = (self.x.min(other.x), self.y.min(other.y));
        let r = (self.x + self.width).max(other.x + other.width);
        let b = (self.y + self.height).max(other.y + other.height);
        PixelRect {
            x,
            y,
            width: r - x,
            height: b - y,
        }
    }
}

impl Rect {
    /// The pixels the rectangle touches at `ppi`, and one more on each
    /// side for anti-aliasing, within a canvas `width` by `height`; `None`
    /// if it falls outside.
    pub fn pixels(&self, ppi: f64, width: u32, height: u32) -> Option<PixelRect> {
        let scale = ppi / 914_400.0;
        let px = |v: i64| v as f64 * scale;
        let clamp = |v: f64, most: u32| v.clamp(0.0, f64::from(most)) as u32;
        let x0 = clamp(px(self.x).floor() - 1.0, width);
        let y0 = clamp(px(self.y).floor() - 1.0, height);
        let x1 = clamp(px(self.right()).ceil() + 1.0, width);
        let y1 = clamp(px(self.bottom()).ceil() + 1.0, height);
        (x1 > x0 && y1 > y0).then_some(PixelRect {
            x: x0,
            y: y0,
            width: x1 - x0,
            height: y1 - y0,
        })
    }
}

/// `rects` with those that overlap or lie near each other drawn as one,
/// and then the pairs that waste least merged until there are at most a
/// few.
pub fn merge(mut rects: Vec<PixelRect>) -> Vec<PixelRect> {
    loop {
        let mut best: Option<(u64, usize, usize)> = None;
        for i in 0..rects.len() {
            for j in i + 1..rects.len() {
                let union = rects[i].union(&rects[j]);
                let waste = union
                    .area()
                    .saturating_sub(rects[i].area() + rects[j].area());
                if best.is_none_or(|(w, _, _)| waste < w) {
                    best = Some((waste, i, j));
                }
            }
        }
        match best {
            Some((waste, i, j)) if waste <= NEAR || rects.len() > MOST_RECTS => {
                rects[i] = rects[i].union(&rects[j]);
                rects.remove(j);
            }
            _ => return rects,
        }
    }
}

/// The pixels to draw again to go from showing `old` to showing `new`, at
/// `ppi` on a canvas `width` by `height`: [`changed`], in pixels, merged.
pub fn redraw(old: &Page, new: &Page, ppi: f64, width: u32, height: u32) -> Vec<PixelRect> {
    merge(
        changed(old, new)
            .iter()
            .filter_map(|r| r.pixels(ppi, width, height))
            .collect(),
    )
}

/// A hash of what `item` draws, for telling items apart.
fn fingerprint(item: &Item) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    hash_item(item, &mut h);
    h.finish()
}

fn hash_item(item: &Item, h: &mut impl Hasher) {
    match item {
        Item::Fill { path, color } => {
            0u8.hash(h);
            path.hash(h);
            color.hash(h);
        }
        Item::Stroke { path, width, color } => {
            1u8.hash(h);
            path.hash(h);
            width.hash(h);
            color.hash(h);
        }
        Item::Glyphs(run) => {
            2u8.hash(h);
            run.font.key().hash(h);
            run.size.hash(h);
            run.color.hash(h);
            run.oblique.hash(h);
            run.baseline.hash(h);
            for g in &run.glyphs {
                g.id.hash(h);
                g.at.hash(h);
            }
        }
        Item::Image { image, rect, .. } => {
            3u8.hash(h);
            image.key().hash(h);
            rect.hash(h);
        }
        Item::Group(group) => {
            4u8.hash(h);
            group.clip.hash(h);
            for c in group.transform.0 {
                c.to_bits().hash(h);
            }
            group.items.len().hash(h);
            for item in &group.items {
                hash_item(item, h);
            }
        }
    }
}

/// The items of `a` and of `b` outside a longest sequence common to both,
/// found by Myers' algorithm: `a`'s that went, and `b`'s that came. `None`
/// if there are more than `limit` of them.
fn diff(a: &[u64], b: &[u64], limit: usize) -> Option<(Vec<usize>, Vec<usize>)> {
    // What's the same at either end needs no search.
    let prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (a_mid, b_mid) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);
    let (n, m) = (a_mid.len() as isize, b_mid.len() as isize);
    if n == 0 || m == 0 {
        return Some((
            (prefix..prefix + a_mid.len()).collect(),
            (prefix..prefix + b_mid.len()).collect(),
        ));
    }
    let most = limit.min((n + m) as usize) as isize;
    let offset = most + 1;
    let mut v = vec![0isize; (2 * most + 3) as usize];
    let at = |k: isize| (k + offset) as usize;
    let mut trace: Vec<Vec<isize>> = Vec::new();
    let mut found = None;
    'search: for d in 0..=most {
        trace.push(v.clone());
        let mut k = -d;
        while k <= d {
            let mut x = if k == -d || (k != d && v[at(k - 1)] < v[at(k + 1)]) {
                v[at(k + 1)]
            } else {
                v[at(k - 1)] + 1
            };
            let mut y = x - k;
            while x < n && y < m && a_mid[x as usize] == b_mid[y as usize] {
                x += 1;
                y += 1;
            }
            v[at(k)] = x;
            if x >= n && y >= m {
                found = Some(d);
                break 'search;
            }
            k += 2;
        }
    }
    let d_end = found?;
    // Back from the end, marking what each step skipped.
    let mut gone = Vec::new();
    let mut came = Vec::new();
    let (mut x, mut y) = (n, m);
    for d in (1..=d_end).rev() {
        let v = &trace[d as usize];
        let k = x - y;
        let prev_k = if k == -d || (k != d && v[at(k - 1)] < v[at(k + 1)]) {
            k + 1
        } else {
            k - 1
        };
        let prev_x = v[at(prev_k)];
        let prev_y = prev_x - prev_k;
        while x > prev_x && y > prev_y {
            x -= 1;
            y -= 1;
        }
        if x == prev_x {
            came.push(prefix + (prev_y as usize));
        } else {
            gone.push(prefix + (prev_x as usize));
        }
        x = prev_x;
        y = prev_y;
    }
    gone.reverse();
    came.reverse();
    Some((gone, came))
}

/// The bounds of `path`'s points, control points included, which hold the
/// curves between them; `None` for an empty path.
fn path_bounds(path: &Path) -> Option<Rect> {
    let mut points = path.0.iter().flat_map(|s| match *s {
        Segment::MoveTo(a) | Segment::LineTo(a) => vec![a],
        Segment::QuadTo(a, b) => vec![a, b],
        Segment::CubicTo(a, b, c) => vec![a, b, c],
        Segment::Close => vec![],
    });
    let first = points.next()?;
    let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.x, first.y);
    for p in points {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    Some(Rect::new(x0, y0, x1 - x0, y1 - y0))
}

/// A glyph run's bounds: each glyph taken to fill its font's bounding box,
/// slanted if the run is; everywhere if the font can't be read.
fn glyph_bounds(run: &Glyphs) -> Option<Rect> {
    let first = run.glyphs.first()?;
    let (mut x0, mut y0, mut x1, mut y1) = (first.at.x, first.at.y, first.at.x, first.at.y);
    for g in &run.glyphs {
        x0 = x0.min(g.at.x);
        y0 = y0.min(g.at.y);
        x1 = x1.max(g.at.x);
        y1 = y1.max(g.at.y);
    }
    let Some(bbox) = font_box(run.font.data.as_ref().as_ref(), run.font.index) else {
        return Some(Rect::EVERYWHERE);
    };
    let units = |v: i16| i64::from(v) * run.size / i64::from(bbox.units_per_em);
    let (left, right) = (x0 + units(bbox.x_min), x1 + units(bbox.x_max));
    // Font units grow up the page, layout units down it.
    let (top, bottom) = (y0 - units(bbox.y_max), y1 - units(bbox.y_min));
    let (mut left, mut right) = (left, right);
    if run.oblique {
        // Tops lean right of the baseline, and what hangs below it left.
        let lean = |height: i64| (height.abs() as f64 * crate::OBLIQUE_SLANT).ceil() as i64;
        right += lean(run.baseline - top);
        left -= lean(bottom - run.baseline);
    }
    Some(Rect::new(left, top, right - left, bottom - top))
}

/// A face's bounding box from its `head` table, in font units.
struct FontBox {
    units_per_em: u16,
    x_min: i16,
    y_min: i16,
    x_max: i16,
    y_max: i16,
}

/// The bounding box of face `index` of `data`, a font file or collection.
fn font_box(data: &[u8], index: u32) -> Option<FontBox> {
    let u16_at = |at: usize| Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?));
    let u32_at = |at: usize| Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?));
    let face = if data.get(0..4)? == b"ttcf" {
        u32_at(12 + 4 * index as usize)? as usize
    } else {
        0
    };
    let tables = u16_at(face + 4)? as usize;
    let head = (0..tables).find_map(|i| {
        let record = face + 12 + 16 * i;
        (data.get(record..record + 4)? == b"head").then(|| u32_at(record + 8))?
    })? as usize;
    let i16_at = |at: usize| u16_at(at).map(|v| v as i16);
    let units_per_em = u16_at(head + 18)?;
    if units_per_em == 0 {
        return None;
    }
    Some(FontBox {
        units_per_em,
        x_min: i16_at(head + 36)?,
        y_min: i16_at(head + 38)?,
        x_max: i16_at(head + 40)?,
        y_max: i16_at(head + 42)?,
    })
}

impl Transform {
    /// The bounds of `rect` with the transform applied, rounded out.
    pub fn bounds(&self, rect: Rect) -> Rect {
        let corners = [
            (rect.x, rect.y),
            (rect.right(), rect.y),
            (rect.x, rect.bottom()),
            (rect.right(), rect.bottom()),
        ]
        .map(|(x, y)| self.apply(x as f64, y as f64));
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for (x, y) in corners {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        let (x0, y0) = (x0.floor() as i64, y0.floor() as i64);
        let (x1, y1) = (x1.ceil() as i64, y1.ceil() as i64);
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, Group, Point};

    fn fill(x: i64, y: i64) -> Item {
        Item::Fill {
            path: Path::rect(x, y, 10, 10),
            color: Color::BLACK,
        }
    }

    fn page(items: Vec<Item>) -> Page {
        let mut page = Page::new(1000, 1000);
        page.items = items;
        page
    }

    #[test]
    fn nothing_changed_is_no_damage() {
        let a = page(vec![fill(0, 0), fill(100, 100)]);
        let b = page(vec![fill(0, 0), fill(100, 100)]);
        assert_eq!(changed(&a, &b), []);
    }

    #[test]
    fn a_moved_item_damages_where_it_was_and_where_it_is() {
        let a = page(vec![fill(0, 0), fill(100, 100), fill(500, 500)]);
        let b = page(vec![fill(0, 0), fill(200, 100), fill(500, 500)]);
        assert_eq!(
            changed(&a, &b),
            [Rect::new(100, 100, 10, 10), Rect::new(200, 100, 10, 10)]
        );
    }

    #[test]
    fn added_and_removed_items_are_damage() {
        let a = page(vec![fill(0, 0), fill(100, 100)]);
        let b = page(vec![fill(0, 0), fill(100, 100), fill(300, 300)]);
        assert_eq!(changed(&a, &b), [Rect::new(300, 300, 10, 10)]);
        assert_eq!(changed(&b, &a), [Rect::new(300, 300, 10, 10)]);
        let c = page(vec![fill(100, 100)]);
        assert_eq!(changed(&a, &c), [Rect::new(0, 0, 10, 10)]);
    }

    #[test]
    fn items_drawn_in_another_order_are_damage() {
        let a = page(vec![fill(0, 0), fill(5, 5)]);
        let b = page(vec![fill(5, 5), fill(0, 0)]);
        assert_eq!(changed(&a, &b).len(), 1);
    }

    #[test]
    fn a_new_size_or_too_many_changes_damage_everything() {
        let a = page(vec![fill(0, 0)]);
        let mut b = page(vec![fill(0, 0)]);
        b.width = 999;
        assert_eq!(changed(&a, &b), [Rect::new(0, 0, 999, 1000)]);
        let many: Vec<Item> = (0..400).map(|i| fill(i, 0)).collect();
        let moved: Vec<Item> = (0..400).map(|i| fill(i, 1)).collect();
        assert_eq!(
            changed(&page(many), &page(moved)),
            [Rect::new(0, 0, 1000, 1000)]
        );
    }

    #[test]
    fn the_diff_keeps_the_longest_common_order() {
        let (gone, came) = diff(&[1, 2, 3, 4, 5], &[1, 3, 4, 9, 5], 10).unwrap();
        assert_eq!((gone, came), (vec![1], vec![3]));
        let (gone, came) = diff(&[1, 2], &[3, 4], 10).unwrap();
        assert_eq!((gone, came), (vec![0, 1], vec![0, 1]));
        assert_eq!(diff(&[1, 2], &[3, 4], 3), None);
    }

    #[test]
    fn rects_become_the_pixels_they_touch_and_one_more() {
        // 9 525 units to a pixel at 96 to the inch.
        let px = 9_525;
        let r = Rect::new(10 * px, 10 * px, 5 * px, 5 * px);
        let p = |x, y, width, height| PixelRect {
            x,
            y,
            width,
            height,
        };
        assert_eq!(r.pixels(96.0, 100, 100), Some(p(9, 9, 7, 7)));
        let partly = Rect::new(10 * px + 1, 10 * px, 5 * px, 5 * px);
        assert_eq!(partly.pixels(96.0, 100, 100), Some(p(9, 9, 8, 7)));
        assert_eq!(r.pixels(96.0, 12, 100), Some(p(9, 9, 3, 7)));
        assert_eq!(r.pixels(96.0, 8, 8), None);
    }

    #[test]
    fn near_rects_merge_and_far_ones_stay_apart() {
        let p = |x, y| PixelRect {
            x,
            y,
            width: 10,
            height: 10,
        };
        assert_eq!(
            merge(vec![p(0, 0), p(5, 5)]),
            [PixelRect {
                x: 0,
                y: 0,
                width: 15,
                height: 15
            }]
        );
        assert_eq!(merge(vec![p(0, 0), p(500, 500)]).len(), 2);
        let many: Vec<PixelRect> = (0..20).map(|i| p(i * 300, i * 300)).collect();
        let merged = merge(many.clone());
        assert_eq!(merged.len(), MOST_RECTS);
        for r in many {
            assert!(
                merged.iter().any(|m| m.x <= r.x
                    && m.y <= r.y
                    && m.x + m.width >= r.x + r.width
                    && m.y + m.height >= r.y + r.height),
                "{r:?} is still covered"
            );
        }
    }

    #[test]
    fn a_group_is_bounded_by_its_transformed_items_and_its_clip() {
        let group = Item::Group(Group {
            clip: Some(Path::rect(0, 0, 50, 1000)),
            transform: Transform::translate(40, 0),
            items: vec![fill(0, 0)],
        });
        assert_eq!(group.bounds(), Some(Rect::new(40, 0, 10, 10)));
        let clipped = Item::Group(Group {
            clip: Some(Path::rect(0, 0, 45, 1000)),
            transform: Transform::translate(40, 0),
            items: vec![fill(0, 0)],
        });
        assert_eq!(clipped.bounds(), Some(Rect::new(40, 0, 5, 10)));
        let turned = Item::Group(Group {
            clip: None,
            transform: Transform::rotate(90.0),
            items: vec![fill(0, 0)],
        });
        assert_eq!(turned.bounds(), Some(Rect::new(-10, 0, 10, 10)));
        let _ = Point::new(0, 0);
    }
}
