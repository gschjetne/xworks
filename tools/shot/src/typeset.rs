//! The `typeset` scene: text set well, as M2's stage 3 sets it. One text
//! ragged and justified side by side, justified and hyphenated paragraphs
//! in several languages, and tab stops.

use std::sync::{Arc, OnceLock};

use scene::{Color, Page, Path, Point};
use text::units::{INCH, POINT};
use text::{
    Align, Hyphenation, Hyphenator, Language, Paragraph, Setting, Span, Style, TabAlign, TabStop,
    Tabs,
};

use crate::pages::fonts;

/// Every language's hyphenation patterns, loaded once.
fn hyphenation() -> &'static Hyphenation {
    static HYPHENATION: OnceLock<Hyphenation> = OnceLock::new();
    HYPHENATION.get_or_init(|| {
        let mut all = Hyphenation::new();
        let loaded = testkit::hyphenation::load()
            .expect("the hyphenation patterns; fetch them with `cargo run -p fetch-fonts`");
        for loaded in loaded {
            let l = &loaded.language;
            let hyphenator = Arc::new(Hyphenator::parse(
                &loaded.patterns,
                &loaded.exceptions,
                l.left,
                l.right,
            ));
            for tag in &l.tags {
                all.insert(
                    &Language::parse(tag).expect("a language tag"),
                    hyphenator.clone(),
                );
            }
        }
        all
    })
}

const GREY: Color = Color::rgb(110, 110, 110);
const RULE: Color = Color::rgb(200, 200, 200);

/// What a column of text says, and how it's set.
struct Column<'a> {
    text: &'a str,
    language: &'a str,
    family: &'a str,
    size: f64,
    weight: u16,
    align: Align,
    hyphenate: bool,
    tabs: Option<&'a Tabs>,
}

impl Column<'_> {
    const fn body<'a>(text: &'a str, language: &'a str, align: Align) -> Column<'a> {
        Column {
            text,
            language,
            family: "Source Serif 4",
            size: 9.5,
            weight: 400,
            align,
            hyphenate: matches!(align, Align::Justify),
            tabs: None,
        }
    }

    /// Draws it on `page` at (`x`, `y`), `width` wide; returns its height.
    fn draw(&self, page: &mut Page, x: i64, y: i64, width: i64) -> i64 {
        let spans = [Span {
            len: self.text.len(),
            style: Style {
                family: self.family.to_string(),
                size: (self.size * POINT as f64).round() as i64,
                weight: self.weight,
                italic: false,
                language: Language::parse(self.language).expect("a language tag"),
            },
        }];
        let layout = text::layout_with(
            fonts(),
            &Paragraph {
                text: self.text,
                spans: &spans,
                direction: None,
                align: self.align,
                width,
            },
            &Setting {
                hyphenate: self.hyphenate,
                hyphenation: Some(hyphenation()),
                tabs: self.tabs,
            },
        );
        page.items
            .extend(layout.paint(fonts(), Point::new(x, y), None, |_| {
                if self.weight > 500 {
                    Color::BLACK
                } else {
                    Color::rgb(20, 20, 20)
                }
            }));
        layout.height
    }
}

/// A label above a column, small and grey.
fn label(page: &mut Page, text: &str, x: i64, y: i64, width: i64) -> i64 {
    let spans = [Span {
        len: text.len(),
        style: Style {
            family: "Inter".to_string(),
            size: 7 * POINT,
            weight: 500,
            italic: false,
            language: Language::parse("en").expect("a tag"),
        },
    }];
    let layout = text::layout(
        fonts(),
        &Paragraph {
            text,
            spans: &spans,
            direction: None,
            align: Align::Start,
            width,
        },
    );
    page.items
        .extend(layout.paint(fonts(), Point::new(x, y), None, |_| GREY));
    layout.height + 3 * POINT
}

/// A thin rule marking a column's edges, to show where justified lines
/// meet them.
fn edges(page: &mut Page, x: i64, y: i64, width: i64, height: i64) {
    for at in [x - 2 * POINT, x + width + 2 * POINT] {
        page.items.push(scene::Item::Fill {
            path: Path::rect(at, y, POINT / 4, height),
            color: RULE,
        });
    }
}

const GRIMM: &str = "In olden times when wishing still helped one, there lived a king whose daughters were all beautiful; and the youngest was so beautiful that the sun itself, which has seen so much, was astonished whenever it shone in her face. Close by the king's castle lay a great dark forest, and under an old lime-tree in the forest was a well.";

/// Justified, hyphenated paragraphs in each language, with its tag.
const LANGUAGES: &[(&str, &str, &str)] = &[
    (
        "German",
        "de",
        "Die Silbentrennung folgt den Mustern, die Frank Liang für TeX entwickelt hat: Ein Wort wird nur dort getrennt, wo die Muster seiner Sprache es erlauben, und am Zeilenende erscheint ein Trennstrich, der nicht zum Text gehört.",
    ),
    (
        "French",
        "fr",
        "La typographie extraordinaire de ce paragraphe justifié coupe les mots selon les motifs de sa langue, et l'espace entre les mots s'étire jusqu'à remplir exactement chaque ligne, sauf la dernière.",
    ),
    (
        "Russian",
        "ru",
        "Переносы слов помогают выравнивать строки по ширине: слово разбивается только там, где это позволяют шаблоны языка, а последняя строка абзаца остаётся неровной.",
    ),
    (
        "Greek",
        "el",
        "Ο συλλαβισμός των λέξεων ακολουθεί τους κανόνες της ελληνικής γλώσσας, και τα κενά ανάμεσα στις λέξεις τεντώνονται ώστε κάθε γραμμή να γεμίζει ακριβώς το πλάτος της στήλης.",
    ),
    (
        "Swedish",
        "sv",
        "Avstavningen följer mönster för svenska ord, så att sjuksköterskor och andra långa sammansättningar delas där språket tillåter det, och raderna fyller spalten jämnt.",
    ),
    (
        "Norwegian (Bokmål)",
        "nb",
        "Orddeling etter norske mønstre gjør at lange sammensatte ord som arbeidsledighetstrygd kan deles ved linjeslutt, slik at avsnittet blir jevnt og pent.",
    ),
];

/// The scene's page, 6 by 9 inches.
pub fn typeset_page() -> Page {
    let mut page = Page::new(6 * INCH, 9 * INCH);
    page.fill_background(Color::WHITE);
    let margin = INCH / 2;
    let gap = INCH / 4;
    let column = (page.width - 2 * margin - gap) / 2;
    let (left, right) = (margin, margin + column + gap);
    let mut y = margin;

    // The same text, first fit and ragged, and optimal and justified.
    let a = label(&mut page, "RAGGED, FIRST FIT", left, y, column);
    label(
        &mut page,
        "JUSTIFIED: KNUTH AND PLASS, HYPHENATED",
        right,
        y,
        column,
    );
    y += a;
    let ragged = Column::body(GRIMM, "en-US", Align::Start).draw(&mut page, left, y, column);
    let justified = Column::body(GRIMM, "en-US", Align::Justify).draw(&mut page, right, y, column);
    edges(&mut page, right, y, column, justified);
    y += ragged.max(justified) + 12 * POINT;

    // Languages, two by two.
    for pair in LANGUAGES.chunks(2) {
        let mut tallest = 0;
        for (k, (name, tag, text)) in pair.iter().enumerate() {
            let x = if k == 0 { left } else { right };
            let h = label(&mut page, &name.to_uppercase(), x, y, column);
            let h = h + Column::body(text, tag, Align::Justify).draw(&mut page, x, y + h, column);
            edges(&mut page, x, y + label_height(), column, h - label_height());
            tallest = tallest.max(h);
        }
        y += tallest + 12 * POINT;
    }

    // Japanese stretches between its characters; Hebrew at its spaces.
    let h = label(&mut page, "JAPANESE", left, y, column);
    label(&mut page, "HEBREW", right, y, column);
    let ja = Column::body(
        "日本語の文章は文字の間を少しずつ広げて行末をそろえます。小さな「ょ」や「っ」が行頭に来ることもあります。",
        "ja",
        Align::Justify,
    )
    .draw(&mut page, left, y + h, column);
    let he = Column::body(
        "טקסט עברי מיושר לשני הצדדים: הרווחים בין המילים נמתחים עד שכל שורה ממלאת את רוחב הטור, מלבד השורה האחרונה.",
        "he",
        Align::Justify,
    )
    .draw(&mut page, right, y + h, column);
    edges(&mut page, left, y + h, column, ja);
    edges(&mut page, right, y + h, column, he);
    y += h + ja.max(he) + 12 * POINT;

    // Tab stops: a right one for the quantity, a decimal one for the
    // price.
    let tabs = Tabs {
        stops: vec![
            TabStop {
                position: 150 * POINT,
                align: TabAlign::End,
            },
            TabStop {
                position: 230 * POINT,
                align: TabAlign::Decimal('.'),
            },
        ],
        interval: 36 * POINT,
    };
    let h = label(
        &mut page,
        "TAB STOPS: START, END AND DECIMAL",
        left,
        y,
        4 * INCH,
    );
    Column {
        tabs: Some(&tabs),
        ..Column::body(
            "Item\tQuantity\tPrice\u{2028}Lime-tree\t1\t1250.00\u{2028}Golden ball\t12\t3.5\u{2028}Well\t2\t\u{2014}",
            "en-US",
            Align::Start,
        )
    }
    .draw(&mut page, left, y + h, 5 * INCH);
    page
}

/// The height [`label`] takes.
fn label_height() -> i64 {
    let mut scratch = Page::new(INCH, INCH);
    label(&mut scratch, "X", 0, 0, INCH)
}
