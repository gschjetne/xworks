//! Round trips, by property tests (docs/ROADMAP.md, M2 stage 6): a story
//! of every kind of block Markdown holds, with every mark, written as a
//! new Markdown file, reads back as the same story; and that file, read,
//! with one of its paragraphs edited, written back and read again, is the
//! story edited.

use doc::{Block, Document, Kind, Object, ObjectId, Op, Paragraph, Props, Run, Value};
use markdown::{File, Settings, What, shape};
use proptest::prelude::*;
use story::{StyleSheet, keys};

/// Neutral, the program's stylesheet, which Markdown files take.
fn neutral() -> story::SheetSpec {
    let source = testkit::specs("styles")
        .expect("spec/styles")
        .into_iter()
        .find(|(name, _)| name == "neutral")
        .expect("spec/styles/neutral.ron")
        .1;
    story::SheetSpec::parse(&source).expect("neutral.ron is a stylesheet")
}

/// Neutral resolved, as a document read from Markdown has it.
fn sheet() -> StyleSheet {
    StyleSheet::from_spec(&neutral())
}

/// Neutral's objects, as reading Markdown puts them in a document.
fn sheet_objects() -> &'static [(doc::ObjectId, doc::Object)] {
    static OBJECTS: std::sync::OnceLock<Vec<(doc::ObjectId, doc::Object)>> =
        std::sync::OnceLock::new();
    OBJECTS.get_or_init(|| neutral().objects(story::SHEET_AT))
}

/// A run of text and its marks.
#[derive(Clone, Debug)]
struct R {
    text: String,
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
    link: Option<(String, String)>,
}

/// A block to build.
#[derive(Clone, Debug)]
enum B {
    /// A paragraph, or a heading of a level.
    P(u8, Vec<R>),
    Code(String, Vec<String>),
    /// A list, numbered from a start, and its items: each one's task, if
    /// it's one, and its blocks.
    List(Option<i64>, Vec<(Option<bool>, Vec<B>)>),
    Quote(Vec<B>),
    /// A table: its columns' alignments, and its rows of cells.
    Table(Vec<String>, Vec<Vec<Vec<R>>>),
    /// A rule across the text: a thematic break.
    Rule,
}

fn run() -> impl Strategy<Value = R> {
    let url = prop::sample::select(vec![
        "https://example.com/a",
        "a b.md",
        "x(1).png",
        "../up.md",
        "mailto:me@example.com",
        "",
    ]);
    let title = prop::sample::select(vec!["", "", "Title", "say \"hi\""]);
    // Mostly text whose marks start and end on letters, as Markdown can
    // always write them; now and then any text at all.
    // And now and then a web or e-mail address, or what nearly is one,
    // which GitHub's autolinks read (review jy4a2tx5).
    let address = prop::sample::select(vec![
        "www.example.net",
        "www.example.net.",
        "https://example.net/a_(b)",
        "me@example.net",
        "x.y-z@a.b_",
        "(www.a.b)",
        "www.a",
    ])
    .prop_map(str::to_string);
    let text = prop_oneof![
        9 => "[a-zA-Z0-9é日]([a-zA-Z0-9 .,;:!?*_\\[\\]()<>#&`~|+=\\-\\\\'\"é日]{0,8}[a-zA-Z0-9é日])?",
        1 => "[a-zA-Z0-9 .,;:!?*_\\[\\]()<>#&`~|+=\\-\\\\'\"é日]{1,10}",
        1 => address,
    ];
    (
        text,
        any::<bool>(),
        any::<bool>(),
        prop::bool::weighted(0.2),
        prop::bool::weighted(0.2),
        prop::option::weighted(0.2, (url, title)),
    )
        .prop_map(|(text, bold, italic, strike, code, link)| {
            // An address linked, mostly, to itself.
            let link = link.map(|(u, t)| match markdown::address(&text) {
                Some(own) if t.is_empty() => (own, String::new()),
                _ => (u.to_string(), t.to_string()),
            });
            R {
                text,
                bold,
                italic,
                strike,
                code,
                link,
            }
        })
}

fn runs() -> impl Strategy<Value = Vec<R>> {
    prop::collection::vec(run(), 1..4)
}

fn leaf() -> impl Strategy<Value = B> {
    prop_oneof![
        1 => Just(B::Rule),
        4 => (prop::sample::select(vec![0u8, 0, 0, 1, 2, 3, 6]), runs()).prop_map(|(l, r)| B::P(l, r)),
        1 => ("[a-z]{0,5}", prop::collection::vec("[a-z `~*#>\\-]{0,8}", 1..4)).prop_map(|(i, l)| B::Code(i, l)),
        1 => (
            prop::sample::select(vec![1usize, 2, 3]),
            prop::collection::vec(prop::sample::select(vec!["", "start", "center", "end"]), 3),
            prop::collection::vec(prop::collection::vec(runs(), 3), 1..4),
        )
            .prop_map(|(columns, aligns, rows)| B::Table(
                aligns[..columns].iter().map(|a| a.to_string()).collect(),
                rows.into_iter().map(|r| r[..columns].to_vec()).collect(),
            )),
    ]
}

fn block() -> impl Strategy<Value = B> {
    leaf().prop_recursive(3, 24, 3, |inner| {
        prop_oneof![
            (
                prop::option::of(prop::sample::select(vec![1i64, 3, 10])),
                prop::collection::vec(
                    (
                        prop::option::weighted(0.3, any::<bool>()),
                        prop::collection::vec(inner.clone(), 1..3)
                    ),
                    1..4
                )
            )
                .prop_map(|(start, items)| B::List(start, items)),
            prop::collection::vec(inner, 1..3).prop_map(B::Quote),
        ]
    })
}

fn story() -> impl Strategy<Value = Vec<B>> {
    prop::collection::vec(block(), 1..5)
}

/// A paragraph's props: its style, if it names one.
fn para(parent: ObjectId, style: Option<&str>, runs: &[R], extra: Props) -> Object {
    let mut text = String::new();
    let mut out = Vec::new();
    for (i, r) in runs.iter().enumerate() {
        // Runs apart, as marks mostly are, at words' edges.
        if i > 0 {
            text.push(' ');
            out.push(Run::new(1, Props::new()));
        }
        text.push_str(&r.text);
        let mut props = Props::new();
        for (on, key) in [
            (r.bold, keys::BOLD),
            (r.italic, keys::ITALIC),
            (r.strike, keys::STRIKETHROUGH),
            (r.code, keys::CODE),
        ] {
            if on {
                props.insert(key.to_string(), Value::Bool(true));
            }
        }
        if let Some((url, title)) = &r.link {
            props.insert(keys::LINK.to_string(), Value::Text(url.clone()));
            if !title.is_empty() {
                props.insert(keys::LINK_TITLE.to_string(), Value::Text(title.clone()));
            }
        }
        out.push(Run::new(r.text.chars().count() as u32, props));
    }
    let mut props = extra;
    if let Some(style) = style {
        props.insert(keys::STYLE.to_string(), Value::Ref(style_id(style)));
    }
    let mut runs: Vec<Run> = Vec::new();
    for run in out {
        match runs.last_mut() {
            Some(last) if last.props == run.props => last.len += run.len,
            _ => runs.push(run),
        }
    }
    Object::Paragraph(Paragraph {
        parent,
        text,
        runs,
        props,
    })
}

/// `b`'s objects, in `parent`, into `out`, it first; its ID.
fn objects(next: &mut u64, b: &B, parent: ObjectId, out: &mut Vec<(ObjectId, Object)>) -> ObjectId {
    let id = ObjectId(*next);
    *next += 1;
    let at = out.len();
    let container = |kind, props| {
        Object::Block(Block {
            kind,
            parent,
            children: Vec::new(),
            props,
        })
    };
    let push = |out: &mut Vec<(ObjectId, Object)>, at: usize, child: ObjectId| {
        if let Object::Block(b) = &mut out[at].1 {
            b.children.push(child);
        }
    };
    match b {
        B::P(level, runs) => {
            let style = if *level == 0 {
                "Body".to_string()
            } else {
                format!("Heading {level}")
            };
            out.push((id, para(parent, Some(&style), runs, Props::new())));
        }
        B::Code(info, lines) => {
            let mut props = Props::new();
            if !info.is_empty() {
                props.insert(keys::INFO.to_string(), Value::Text(info.clone()));
            }
            let text = lines.join("\u{2028}");
            let runs = [R {
                text,
                bold: false,
                italic: false,
                strike: false,
                code: false,
                link: None,
            }];
            let runs: Vec<R> = runs.into_iter().filter(|r| !r.text.is_empty()).collect();
            out.push((id, para(parent, Some("Code"), &runs, props)));
        }
        B::List(start, items) => {
            let mut props = Props::new();
            if let Some(n) = start {
                props.insert(keys::ORDERED.to_string(), Value::Bool(true));
                if *n != 1 {
                    props.insert(keys::START.to_string(), Value::Int(*n));
                }
            }
            out.push((id, container(Kind::List, props)));
            for (task, blocks) in items {
                let item = ObjectId(*next);
                *next += 1;
                let item_at = out.len();
                let mut props = Props::new();
                // A task's box goes before text.
                if let (Some(done), Some(B::P(0, _))) = (task, blocks.first()) {
                    props.insert(keys::TASK.to_string(), Value::Bool(*done));
                }
                out.push((
                    item,
                    Object::Block(Block {
                        kind: Kind::Item,
                        parent: id,
                        children: Vec::new(),
                        props,
                    }),
                ));
                for b in blocks {
                    let child = objects(next, b, item, out);
                    push(out, item_at, child);
                }
                push(out, at, item);
            }
        }
        B::Quote(blocks) => {
            out.push((id, container(Kind::Quote, Props::new())));
            for b in blocks {
                let child = objects(next, b, id, out);
                push(out, at, child);
            }
        }
        B::Rule => out.push((id, container(Kind::Rule, Props::new()))),
        B::Table(aligns, rows) => {
            out.push((id, container(Kind::Table, Props::new())));
            for cells in rows {
                let row = ObjectId(*next);
                *next += 1;
                let row_at = out.len();
                out.push((
                    row,
                    Object::Block(Block {
                        kind: Kind::Row,
                        parent: id,
                        children: Vec::new(),
                        props: Props::new(),
                    }),
                ));
                for (column, runs) in cells.iter().enumerate() {
                    let cell = ObjectId(*next);
                    *next += 1;
                    let mut props = Props::new();
                    if !aligns[column].is_empty() {
                        props.insert(keys::ALIGN.to_string(), Value::Text(aligns[column].clone()));
                    }
                    out.push((cell, para(row, None, runs, props)));
                    push(out, row_at, cell);
                }
                push(out, at, row);
            }
        }
    }
    id
}

/// The ID of Neutral's paragraph style `name`, in a document made in it.
fn style_id(name: &str) -> ObjectId {
    static NEUTRAL: std::sync::OnceLock<Document> = std::sync::OnceLock::new();
    let document = NEUTRAL.get_or_init(|| story::new_document(&neutral()));
    story::style_named(document, name).expect("a style of Neutral's")
}

/// A document of `blocks`, in Neutral.
fn build(blocks: &[B]) -> Document {
    let mut document = story::new_document(&neutral());
    let root = document.root();
    let first = document.paragraphs()[0];
    let mut next = 100;
    let mut after = Some(first);
    for b in blocks {
        let mut out = Vec::new();
        let id = objects(&mut next, b, root, &mut out);
        document
            .apply(&Op::InsertBlock {
                parent: root,
                after,
                objects: out,
            })
            .expect("building applies");
        after = Some(id);
    }
    // The paragraph it starts with goes, unless the blocks have none, as
    // a story of rules alone has.
    if document.paragraphs().len() > 1 {
        document
            .apply(&Op::DeleteBlock { block: first })
            .expect("the first paragraph goes");
    }
    document
}

fn settings(_sheet: &StyleSheet) -> Settings<'static> {
    Settings {
        sheet: sheet_objects(),
        language: None,
        sizes: &|_| None,
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 400, max_global_rejects: 200, ..ProptestConfig::default() })]

    /// What's written as a new file reads back as the same story.
    #[test]
    fn a_story_written_reads_back_the_same(blocks in story()) {
        let sheet = sheet();
        let document = build(&blocks);
        let written = File::empty(None).write(&document, &sheet);
        // Marks Markdown has no way to write there, as emphasis that
        // starts with punctuation inside a word, are refused.
        prop_assume!(!matches!(&written, Err(e) if e.what == What::Marks));
        let written = written.map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
        let read = markdown::read(&written, &settings(&sheet)).expect("it reads");
        prop_assert_eq!(
            shape(&document, &sheet),
            shape(&read.document, &sheet),
            "written as {:?}",
            String::from_utf8_lossy(&written)
        );
    }

    /// That file, read, and a paragraph of it given new text, written back
    /// as the file it was, reads back as the story edited; and the
    /// blocks the edit didn't touch are written as they were.
    #[test]
    fn a_file_edited_reads_back_as_edited(blocks in story(), which in any::<prop::sample::Index>(), new in runs()) {
        let sheet = sheet();
        let document = build(&blocks);
        let written = File::empty(None).write(&document, &sheet);
        prop_assume!(!matches!(&written, Err(e) if e.what == What::Marks));
        let written = written.map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
        let read = markdown::read(&written, &settings(&sheet)).expect("it reads");
        let paragraphs: Vec<ObjectId> = read
            .document
            .paragraphs()
            .into_iter()
            .filter(|&id| !sheet.style_of(read.document.paragraph(id).expect("a paragraph")).code)
            .collect();
        prop_assume!(!paragraphs.is_empty());
        let id = paragraphs[which.index(paragraphs.len())];
        let p = read.document.paragraph(id).expect("a paragraph");
        let mut edited = read.document.clone();
        let replacement = para(p.parent, None, &new, Props::new());
        let Object::Paragraph(r) = replacement else { unreachable!() };
        edited
            .apply_all(&[
                Op::DeleteText { paragraph: id, offset: 0, len: p.len() },
                Op::InsertText { paragraph: id, offset: 0, text: r.text, runs: r.runs },
            ])
            .expect("it applies");
        let mut file = read.file.clone();
        let again = file.write(&edited, &sheet);
        prop_assume!(!matches!(&again, Err(e) if e.what == What::Marks));
        let again = again.map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
        let reread = markdown::read(&again, &settings(&sheet)).expect("it reads");
        prop_assert_eq!(
            shape(&edited, &sheet),
            shape(&reread.document, &sheet),
            "{:?} edited, written as {:?}",
            String::from_utf8_lossy(&written),
            String::from_utf8_lossy(&again)
        );
    }
}
