//! Markdown files of our own, as people write them
//! (`spec/markdown/files/`): each written back untouched, byte for byte;
//! with a word edited, one line changed and the rest as they were; and
//! what's written anew written in each file's habits.

use doc::{Document, ObjectId, Op, Props, Run, Value};
use markdown::{Read, Settings, shape};
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
fn objects() -> &'static [(doc::ObjectId, doc::Object)] {
    static OBJECTS: std::sync::OnceLock<Vec<(doc::ObjectId, doc::Object)>> =
        std::sync::OnceLock::new();
    OBJECTS.get_or_init(|| neutral().objects(story::SHEET_AT))
}

/// Every file, by name, as its bytes.
fn files() -> Vec<(String, Vec<u8>)> {
    let out = testkit::spec_bytes("markdown/files", "md").expect("spec/markdown/files");
    assert!(out.len() >= 6, "the files are there");
    out
}

fn file(name: &str) -> Vec<u8> {
    files()
        .into_iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("{name}"))
        .1
}

fn read(bytes: &[u8], _sheet: &StyleSheet) -> Read {
    let settings = Settings {
        sheet: objects(),
        language: Some("en-US"),
        sizes: &|_| None,
    };
    markdown::read(bytes, &settings).expect("it reads")
}

/// The first paragraph whose text has `needle`, and where it starts in
/// characters.
fn find(document: &Document, needle: &str) -> (ObjectId, u32) {
    document
        .paragraphs()
        .into_iter()
        .find_map(|id| {
            let text = &document.paragraph(id)?.text;
            text.find(needle)
                .map(|b| (id, text[..b].chars().count() as u32))
        })
        .unwrap_or_else(|| panic!("{needle:?} isn't in the document"))
}

/// `document` with the first `old` replaced by `new`, which takes the
/// properties of the text it replaces.
fn replace(document: &Document, old: &str, new: &str) -> Document {
    let (id, at) = find(document, old);
    let p = document.paragraph(id).expect("a paragraph");
    let mut pos = 0;
    let props = p
        .runs
        .iter()
        .find_map(|r| {
            let found = (at < pos + r.len).then(|| (*r.props).clone());
            pos += r.len;
            found
        })
        .unwrap_or_default();
    let mut document = document.clone();
    document
        .apply_all(&[
            Op::DeleteText {
                paragraph: id,
                offset: at,
                len: old.chars().count() as u32,
            },
            Op::InsertText {
                paragraph: id,
                offset: at,
                text: new.to_string(),
                runs: vec![Run::new(new.chars().count() as u32, props)],
            },
        ])
        .expect("it applies");
    document
}

/// The lines of `a` and `b` that differ, where they have as many.
fn changed(a: &[u8], b: &[u8]) -> Vec<(String, String)> {
    let (a, b) = (String::from_utf8_lossy(a), String::from_utf8_lossy(b));
    let (a, b): (Vec<&str>, Vec<&str>) = (a.split('\n').collect(), b.split('\n').collect());
    assert_eq!(a.len(), b.len(), "as many lines");
    a.iter()
        .zip(&b)
        .filter(|(x, y)| x != y)
        .map(|(x, y)| (x.to_string(), y.to_string()))
        .collect()
}

#[test]
fn untouched_files_are_written_back_byte_for_byte() {
    let sheet = sheet();
    for (name, bytes) in files() {
        let mut read = read(&bytes, &sheet);
        let written = read.file.write(&read.document, &sheet).expect("it writes");
        assert!(
            written == bytes,
            "{name} was written back as\n{}",
            String::from_utf8_lossy(&written)
        );
    }
}

/// Every word of four letters or more in every file, but in code, made
/// capitals in turn: the file changes in that word's line alone, and
/// reads back as the story edited.
#[test]
fn a_one_word_edit_changes_one_line() {
    let sheet = sheet();
    let mut edits = 0;
    for (name, bytes) in files() {
        let read = read(&bytes, &sheet);
        for id in read.document.paragraphs() {
            let p = read.document.paragraph(id).expect("a paragraph");
            if p.props.contains_key(keys::SOURCE) || sheet.style_of(p).code {
                continue;
            }
            for word in p.text.split(|c: char| !c.is_ascii_alphabetic()) {
                if word.len() < 4 || word.chars().all(|c| c.is_ascii_uppercase()) {
                    continue;
                }
                let edited = replace(&read.document, word, &word.to_ascii_uppercase());
                let mut file = read.file.clone();
                let written = file
                    .write(&edited, &sheet)
                    .unwrap_or_else(|e| panic!("{name}: {word:?} to capitals: {e:?}"));
                let changes = changed(&bytes, &written);
                assert_eq!(
                    changes.len(),
                    1,
                    "{name}: {word:?} to capitals changed {changes:#?}"
                );
                assert_eq!(
                    changes[0].1,
                    changes[0].0.replacen(word, &word.to_ascii_uppercase(), 1),
                    "{name}: {word:?}"
                );
                let again = self::read(&written, &sheet);
                assert_eq!(
                    shape(&edited, &sheet),
                    shape(&again.document, &sheet),
                    "{name}: {word:?}"
                );
                edits += 1;
                // One word a paragraph is enough.
                break;
            }
        }
    }
    assert!(edits >= 40, "only {edits} edits");
}

/// A paragraph added to a list is written in the file's habits: its
/// bullets and its emphasis.
#[test]
fn new_items_follow_the_files_habits() {
    let sheet = sheet();
    for (name, after, bullet, emphasis) in [
        ("notes.md", "Support stays on the forum for now.", "* ", "_"),
        (
            "changelog.md",
            "Plans can be printed in Norwegian and German.",
            "+ ",
            "*",
        ),
        ("readme.md", "for other tools.", "- ", "*"),
    ] {
        let bytes = file(name);
        let read = read(&bytes, &sheet);
        let (id, _) = find(&read.document, after);
        let item = read.document.parent(id).expect("an item");
        let list = read.document.parent(item).expect("a list");
        let mut document = read.document.clone();
        let (new_item, new_p) = (document.new_id(), document.new_id());
        let italic = Props::from([(keys::ITALIC.to_string(), Value::Bool(true))]);
        let tag = read.file.language().expect("a language").to_string();
        let language = Props::from([(keys::LANGUAGE.to_string(), Value::Text(tag))]);
        let mut paragraph_props = language;
        let body = story::style_named(&read.document, "Body").expect("Body");
        paragraph_props.insert(keys::STYLE.to_string(), Value::Ref(body));
        document
            .apply(&Op::InsertBlock {
                parent: list,
                after: Some(item),
                objects: vec![
                    (
                        new_item,
                        doc::Object::Block(doc::Block {
                            kind: doc::Kind::Item,
                            parent: list,
                            children: vec![new_p],
                            props: Props::new(),
                        }),
                    ),
                    (
                        new_p,
                        doc::Object::Paragraph(doc::Paragraph {
                            parent: new_item,
                            text: "A new one, quite new.".into(),
                            runs: vec![
                                Run::new(11, Props::new()),
                                Run::new(5, italic),
                                Run::new(5, Props::new()),
                            ],
                            props: paragraph_props,
                        }),
                    ),
                ],
            })
            .expect("it applies");
        let mut file = read.file.clone();
        let written =
            String::from_utf8(file.write(&document, &sheet).expect("it writes")).expect("UTF-8");
        let line = format!("{bullet}A new one, {emphasis}quite{emphasis} new.");
        assert!(written.contains(&line), "{name}: no {line:?} in\n{written}");
        // And nothing else changed: it's one line more.
        let before = String::from_utf8_lossy(&bytes).lines().count();
        assert_eq!(written.lines().count(), before + 1, "{name}:\n{written}");
    }
}

/// A paragraph edited in a file wrapped at a width is wrapped there, and
/// the file's line endings and byte order mark stay.
#[test]
fn edits_keep_the_files_width_line_endings_and_mark() {
    let sheet = sheet();
    let bytes = file("notes.md");
    let read = read(&bytes, &sheet);
    let longer = replace(
        &read.document,
        "Ingrid chaired;",
        "Ingrid chaired, as she has since the spring, and kept us to time;",
    );
    let mut file = read.file.clone();
    let written =
        String::from_utf8(file.write(&longer, &sheet).expect("it writes")).expect("UTF-8");
    let width = String::from_utf8_lossy(&bytes)
        .lines()
        .map(|l| l.chars().count())
        .max()
        .unwrap();
    assert!(
        written.lines().all(|l| l.chars().count() <= width),
        "wider than {width}:\n{written}"
    );
    assert_eq!(
        shape(&longer, &sheet),
        shape(&self::read(written.as_bytes(), &sheet).document, &sheet)
    );

    let bytes = self::file("windows.md");
    let read = self::read(&bytes, &sheet);
    assert!(read.file.bom() && read.file.line_ending() == "\r\n");
    let edited = replace(&read.document, "Bread", "Rye bread");
    let mut file = read.file.clone();
    let written = file.write(&edited, &sheet).expect("it writes");
    assert!(written.starts_with(b"\xef\xbb\xbf"));
    let text = String::from_utf8(written[3..].to_vec()).expect("UTF-8");
    assert!(
        !text.replace("\r\n", "").contains('\n'),
        "a bare line feed in\n{text}"
    );
    assert!(text.contains("2. Rye bread\r\n"), "{text}");
}

/// Toggling a task changes its box alone.
#[test]
fn a_task_toggled_changes_its_box_alone() {
    let sheet = sheet();
    let bytes = file("notes.md");
    let read = read(&bytes, &sheet);
    let (id, _) = find(&read.document, "Draft the release notes");
    let item = read.document.parent(id).expect("an item");
    let mut document = read.document.clone();
    document
        .apply(&Op::SetBlockProp {
            block: item,
            key: keys::TASK.to_string(),
            value: Some(Value::Bool(true)),
        })
        .expect("it applies");
    let mut file = read.file.clone();
    let written = file.write(&document, &sheet).expect("it writes");
    assert_eq!(
        changed(&bytes, &written),
        [(
            "- [ ] Draft the release notes (Tomás)".to_string(),
            "- [x] Draft the release notes (Tomás)".to_string()
        )]
    );
}

/// GitHub's alerts are shown as written (review jy4a2tx5): the quote is a
/// quote, its `[!NOTE]` kept as Markdown kept as written at the start of
/// its first paragraph, and written back as it was, also when the text
/// after it is edited, and alone on its line when it's written afresh.
#[test]
fn alerts_are_kept_as_written() {
    let sheet = sheet();
    let bytes = file("notes.md");
    let read = read(&bytes, &sheet);
    let (id, at) = find(&read.document, "[!NOTE]");
    assert_eq!(at, 0);
    let p = read.document.paragraph(id).expect("a paragraph");
    assert!(p.text.starts_with("[!NOTE] The forum's"), "{:?}", p.text);
    assert_eq!(p.runs[0].len, 7);
    assert_eq!(
        p.runs[0].props.get(keys::SOURCE),
        Some(&Value::Text("alert".into()))
    );
    assert!(!p.runs[1].props.contains_key(keys::SOURCE));
    let quote = read.document.block(p.parent).expect("a quote");
    assert_eq!(quote.kind, doc::Kind::Quote);
    assert!(quote.props.is_empty(), "{:?}", quote.props);

    let edited = replace(&read.document, "spam filter", "filter");
    let mut file = read.file.clone();
    let written = file.write(&edited, &sheet).expect("it writes");
    assert_eq!(
        changed(&bytes, &written),
        [(
            "> The forum's spam filter is still too keen. Ade will look at it.".to_string(),
            "> The forum's filter is still too keen. Ade will look at it.".to_string()
        )]
    );

    let fresh = markdown::File::empty(read.file.language())
        .write(&edited, &sheet)
        .expect("it writes");
    let fresh = String::from_utf8(fresh).expect("UTF-8");
    assert!(
        fresh.contains("> [!NOTE]\n> The forum's filter"),
        "written afresh as\n{fresh}"
    );
    assert_eq!(
        shape(&edited, &sheet),
        shape(&self::read(fresh.as_bytes(), &sheet).document, &sheet)
    );
    // Anywhere else, it's text.
    let read = self::read(b"Not an alert:\n\n> Text\n> [!NOTE]\n", &sheet);
    let (id, _) = find(&read.document, "[!NOTE]");
    let p = read.document.paragraph(id).expect("a paragraph");
    assert!(p.runs.iter().all(|r| !r.props.contains_key(keys::SOURCE)));
}

/// Thematic breaks are rules (review jy4a2tx5), however they're written,
/// in the story, in quotes and in list items, and written back byte for
/// byte; a story that ends in one, or is nothing else, too.
#[test]
fn thematic_breaks_are_rules_written_back_as_they_were() {
    let sheet = sheet();
    for bytes in [
        &b"Above\n\n---\n\nBelow\n"[..],
        b"Above\n\n***\nBelow\n",
        b"Above\n\n___\n",
        b" - - -\n",
        b"*  *  *  *\n\n\n",
        b"> Quoted\n>\n> ___\n",
        b"- one\n\n  ***\n- two\n",
        b"Text\n\n---\n\n",
    ] {
        let read = read(bytes, &sheet);
        let rules = read
            .document
            .subtree(read.document.root())
            .into_iter()
            .filter(|&id| {
                read.document
                    .block(id)
                    .is_some_and(|b| b.kind == doc::Kind::Rule)
            })
            .count();
        assert_eq!(rules, 1, "{}", String::from_utf8_lossy(bytes));
        assert!(
            read.document.paragraphs().iter().all(|&id| {
                !read
                    .document
                    .paragraph(id)
                    .is_some_and(|p| p.props.contains_key(keys::SOURCE))
            }),
            "nothing kept as written in {}",
            String::from_utf8_lossy(bytes)
        );
        let written = read
            .file
            .clone()
            .write(&read.document, &sheet)
            .expect("it writes");
        assert_eq!(
            String::from_utf8_lossy(&written),
            String::from_utf8_lossy(bytes)
        );
    }
}

/// A rule written afresh is `---` after a blank line, which no paragraph
/// before it can take for a heading's underline; on a list item's first
/// line, beside a `-` bullet, it's of another character, as the line
/// would otherwise be a rule itself, not an item holding one.
#[test]
fn rules_written_afresh_read_back_as_rules() {
    use story::{BlockSpec, props};
    let sheet = sheet();
    let p = |text: &str| BlockSpec::Paragraph("Body".into(), vec![(text.into(), props(&[]))]);
    for (blocks, want) in [
        (
            vec![p("Above"), BlockSpec::Rule, p("Below")],
            "Above\n\n---\n\nBelow\n",
        ),
        (vec![p("Text"), BlockSpec::Rule], "Text\n\n---\n"),
        (
            vec![BlockSpec::List(false, vec![vec![BlockSpec::Rule, p("x")]])],
            "- ***\n\n  x\n",
        ),
        (
            vec![BlockSpec::Quote(vec![BlockSpec::Rule, p("Quoted")])],
            "> ---\n>\n> Quoted\n",
        ),
    ] {
        let document = story::build(&neutral(), &blocks);
        let written = markdown::File::empty(Some("en-US"))
            .write(&document, &sheet)
            .expect("it writes");
        assert_eq!(String::from_utf8_lossy(&written), want);
        let again = self::read(&written, &sheet);
        assert_eq!(shape(&again.document, &sheet), shape(&document, &sheet));
    }
    // A rule first, and another later, isn't front matter.
    let document = story::build(
        &neutral(),
        &[BlockSpec::Rule, p("Middle"), BlockSpec::Rule, p("End")],
    );
    let written = markdown::File::empty(None)
        .write(&document, &sheet)
        .expect("it writes");
    let again = self::read(&written, &sheet);
    assert_eq!(again.file.front_matter(), None);
    assert_eq!(shape(&again.document, &sheet), shape(&document, &sheet));
}

/// `document` with every paragraph's language `tag`, as Select All and a
/// language command make it.
fn all_in(document: &Document, tag: &str) -> Document {
    let mut out = document.clone();
    for id in document.paragraphs() {
        out.apply(&Op::SetParaProp {
            paragraph: id,
            key: keys::LANGUAGE.to_string(),
            value: Some(Value::Text(tag.to_string())),
        })
        .expect("it applies");
    }
    out
}

/// The language of all the text, changed, is written into the front
/// matter as `lang` (review jy4a2tx5): edited in place where there's one,
/// YAML's or TOML's, added where front matter has none, and front matter
/// added where there's none; the rest of the file as it was, and back as
/// it was when the change is undone. Part of the text in another language
/// is still what Markdown can't hold.
#[test]
fn the_language_of_all_the_text_is_the_front_matters() {
    let sheet = sheet();
    let settings = Settings {
        sheet: objects(),
        language: Some("en-US"),
        sizes: &|_| None,
    };
    for (before, after) in [
        (
            "---\ntitle: Notes\nlang: en-GB  # the team's\n---\n\n# Notes\n\nText.\n",
            "---\ntitle: Notes\nlang: nb  # the team's\n---\n\n# Notes\n\nText.\n",
        ),
        (
            "---\nlang: \"en-GB\"\n---\nText.\n",
            "---\nlang: \"nb\"\n---\nText.\n",
        ),
        (
            "+++\ntitle = \"Notes\"\nlang = \"de\"\n+++\nText.\n",
            "+++\ntitle = \"Notes\"\nlang = \"nb\"\n+++\nText.\n",
        ),
        (
            "---\ntitle: Notes\n---\nText.\n",
            "---\ntitle: Notes\nlang: nb\n---\nText.\n",
        ),
        (
            "+++\ntitle = \"Notes\"\n+++\nText.\n",
            "+++\ntitle = \"Notes\"\nlang = \"nb\"\n+++\nText.\n",
        ),
        (
            "# Notes\r\n\r\n- Text\r\n",
            "---\r\nlang: nb\r\n---\r\n\r\n# Notes\r\n\r\n- Text\r\n",
        ),
        ("", "---\nlang: nb\n---\n\n"),
    ] {
        let read = markdown::read(before.as_bytes(), &settings).expect("it reads");
        let changed = all_in(&read.document, "nb");
        let mut file = read.file.clone();
        let written = file.write(&changed, &sheet).expect("it writes");
        assert_eq!(String::from_utf8_lossy(&written), after);
        let again = markdown::read(&written, &settings).expect("it reads");
        assert_eq!(again.file.language(), Some("nb"));
        // Undone, as it was.
        let undone = file.write(&read.document, &sheet).expect("it writes");
        assert_eq!(String::from_utf8_lossy(&undone), before);
    }
    // Part of the text in another language: Markdown can't hold it.
    let read = markdown::read(b"One.\n\nTwo.\n", &settings).expect("it reads");
    let mut part = read.document.clone();
    part.apply(&Op::SetParaProp {
        paragraph: read.document.paragraphs()[1],
        key: keys::LANGUAGE.to_string(),
        value: Some(Value::Text("nb".into())),
    })
    .expect("it applies");
    let refused = read.file.clone().write(&part, &sheet).unwrap_err();
    assert_eq!(refused.what, markdown::What::Language);
}

/// A task whose item starts with another block than text, as `- [x]` with
/// a list under it, is written with its box alone on its line, and reads
/// back the same, written afresh or edited; one whose block starts on the
/// box's line can't be (found by fuzzing).
#[test]
fn a_task_starting_with_a_block_has_its_box_alone() {
    let sheet = sheet();
    let bytes = b"- [x]\n    - climbing beans on canes\n- Carrots\n";
    let read = read(bytes, &sheet);
    let fresh = markdown::File::empty(read.file.language())
        .write(&read.document, &sheet)
        .expect("it writes afresh");
    assert_eq!(
        String::from_utf8_lossy(&fresh),
        "- [x]\n  - climbing beans on canes\n- Carrots\n"
    );
    assert_eq!(
        shape(&self::read(&fresh, &sheet).document, &sheet),
        shape(&read.document, &sheet)
    );
    let edited = replace(&read.document, "climbing", "tall");
    let written = read.file.clone().write(&edited, &sheet).expect("it writes");
    assert_eq!(
        shape(&self::read(&written, &sheet).document, &sheet),
        shape(&edited, &sheet)
    );
}

/// The file's front matter is kept, and its language is the paragraphs'.
#[test]
fn front_matter_is_kept_and_gives_the_language() {
    let sheet = sheet();
    let read = read(&file("notes.md"), &sheet);
    assert_eq!(read.file.language(), Some("en-GB"));
    assert!(
        read.file
            .front_matter()
            .is_some_and(|f| f.contains("tags: [planning, q4]"))
    );
    for id in read.document.paragraphs() {
        let p = read.document.paragraph(id).expect("a paragraph");
        assert_eq!(
            p.props.get(keys::LANGUAGE),
            Some(&Value::Text("en-GB".into()))
        );
    }
}

/// Lists begun on one line, their bullets alone on it once the text after
/// them is gone, aren't written as a thematic break, which three of one
/// bullet would read as.
#[test]
fn lists_begun_on_one_line_and_emptied_stay_lists() {
    let sheet = sheet();
    for bytes in [&b"- - - Beans\n"[..], b"* * * Beans\n", b"1. - - - Beans\n"] {
        let read = read(bytes, &sheet);
        let (id, at) = find(&read.document, "Beans");
        let mut document = read.document.clone();
        document
            .apply(&Op::DeleteText {
                paragraph: id,
                offset: at,
                len: 5,
            })
            .expect("it applies");
        let edited = read
            .file
            .clone()
            .write(&document, &sheet)
            .expect("it writes");
        let fresh = markdown::File::empty(read.file.language())
            .write(&document, &sheet)
            .expect("it writes");
        for written in [edited, fresh] {
            let again = self::read(&written, &sheet);
            assert_eq!(
                shape(&again.document, &sheet),
                shape(&document, &sheet),
                "{}",
                String::from_utf8_lossy(&written)
            );
        }
    }
}

/// An HTML block that ends its list item, and the blank line after it
/// with it, is written and read back without another blank line in it
/// each time.
#[test]
fn an_html_block_ending_an_item_keeps_its_lines() {
    let sheet = sheet();
    let read = read(b"- <!-- a note\n\nText\n", &sheet);
    let document = replace(&read.document, "note", "memo");
    let edited = read
        .file
        .clone()
        .write(&document, &sheet)
        .expect("it writes");
    let fresh = markdown::File::empty(read.file.language())
        .write(&document, &sheet)
        .expect("it writes");
    for written in [edited, fresh] {
        let again = self::read(&written, &sheet);
        assert_eq!(
            shape(&again.document, &sheet),
            shape(&document, &sheet),
            "{}",
            String::from_utf8_lossy(&written)
        );
    }
}

/// A table's header row's cells are read in Table Header, as headings are
/// read in their styles, and its other cells in body text's (review
/// sjj5ak5a); written, the first row is the header by its place, and the
/// style is never written. A header cell in another style, or Table
/// Header anywhere but a header row, is what Markdown can't hold.
#[test]
fn a_tables_header_row_is_in_table_header() {
    use story::{BlockSpec, props};
    let sheet = sheet();
    let text = b"| Name | Size |\n| --- | ---: |\n| Pear | Small |\n";
    let read = read(text, &sheet);
    let document = &read.document;
    let style = |needle: &str| {
        let (id, _) = find(document, needle);
        sheet
            .style_of(document.paragraph(id).expect("a paragraph"))
            .name
            .clone()
    };
    assert_eq!(style("Name"), "Table Header");
    assert_eq!(style("Size"), "Table Header");
    assert_eq!(style("Pear"), "Body");
    let (pear, _) = find(document, "Pear");
    assert_eq!(
        document.paragraph(pear).unwrap().props.get(keys::STYLE),
        None,
        "a body row's cells name no style"
    );
    // Written afresh, as built, it's a table as any.
    let cell = |text: &str| vec![(text.to_string(), props(&[]))];
    let built = story::build(
        &neutral(),
        &[BlockSpec::Table(
            vec![String::new(), "end".into()],
            vec![
                vec![cell("Name"), cell("Size")],
                vec![cell("Pear"), cell("Small")],
            ],
        )],
    );
    let written = markdown::File::empty(None)
        .write(&built, &sheet)
        .expect("it writes");
    assert_eq!(
        String::from_utf8_lossy(&written),
        "| Name |  Size |\n| ---- | ----: |\n| Pear | Small |\n"
    );
    assert_eq!(
        shape(&self::read(&written, &sheet).document, &sheet),
        shape(&built, &sheet)
    );
    // A header cell in body text, and a paragraph in Table Header outside
    // a header row, aren't held.
    let restyled = |document: &Document, needle: &str, name: &str| {
        let (id, _) = find(document, needle);
        let mut document = document.clone();
        let style = story::style_named(&document, name).map(Value::Ref);
        document
            .apply(&Op::SetParaProp {
                paragraph: id,
                key: keys::STYLE.to_string(),
                value: style,
            })
            .expect("it applies");
        document
    };
    let alone = story::build(
        &neutral(),
        &[BlockSpec::Paragraph("Body".into(), cell("Alone"))],
    );
    for (document, name) in [
        (restyled(&built, "Name", "Body"), "Body"),
        (restyled(&built, "Pear", "Table Header"), "Table Header"),
        (restyled(&alone, "Alone", "Table Header"), "Table Header"),
    ] {
        let unheld = markdown::File::empty(None)
            .write(&document, &sheet)
            .expect_err("Markdown can't hold it");
        assert_eq!(unheld.what, markdown::What::Style(name.into()));
    }
}

/// A list that names a list definition of its own, even the one lists of
/// its kind take, isn't held (M2's exit, review 7fnfdhb5): Markdown has
/// no room to say which, so the app names none for its kind's own, and a
/// Markdown file asks before any other.
#[test]
fn a_lists_own_list_definition_isnt_held() {
    let sheet = sheet();
    let read = read(b"- one\n- two\n", &sheet);
    let list = read
        .document
        .subtree(read.document.root())
        .into_iter()
        .find(|&b| {
            read.document
                .block(b)
                .is_some_and(|b| b.kind == doc::Kind::List)
        })
        .expect("a list");
    let bullets = sheet.list(false, None).id.expect("Neutral's Bullets");
    let mut named = read.document.clone();
    named
        .apply(&Op::SetBlockProp {
            block: list,
            key: keys::LIST.to_string(),
            value: Some(Value::Ref(bullets)),
        })
        .expect("it applies");
    let unheld = read
        .file
        .clone()
        .write(&named, &sheet)
        .expect_err("Markdown can't hold it");
    assert_eq!(unheld.what, markdown::What::ListDefinition);
    assert_eq!(unheld.what.to_string(), "a list definition of a list's own");
    let written = read.file.clone().write(&read.document, &sheet);
    assert_eq!(written.expect("it writes"), b"- one\n- two\n");
}

/// A file's pictures are found as it's read and placed at their sizes
/// once they're measured, the file read once (`read_unsized`): its links
/// are those its story shows, each once, in order, as `links` finds them
/// but for a picture in another's description; each picture whose size is
/// known has it; and the story is otherwise as it is with no sizes known,
/// pictures side by side in one run as they were.
#[test]
fn pictures_are_placed_at_their_sizes_once_theyre_measured() {
    let text = "# ![Logo](logo.png) Larkspur\n\n\
        ![A](a.png)![A](a.png) and ![B](b.png \"Bee\") in *![A](a.png)*.\n\n\
        [![Badge](badge.svg)](https://example.org) ![Gone](gone.png)\n\n\
        - ![Item](b.png)\n\n\
        | Pic |\n|-----|\n| ![Cell](c.png) |\n\n\
        ![Outer ![Inner](inner.png)](outer.png)\n";
    let sizes = |link: &str| match link {
        "logo.png" => Some((100, 50)),
        "a.png" => Some((10, 20)),
        "b.png" => Some((30, 40)),
        "c.png" => Some((5, 5)),
        "outer.png" => Some((7, 8)),
        "badge.svg" => Some((1, 1)),
        _ => None,
    };
    let begun =
        markdown::read_unsized(text.as_bytes(), objects(), Some("en-US"), 1).expect("it reads");
    let links = [
        "logo.png",
        "a.png",
        "b.png",
        "badge.svg",
        "gone.png",
        "c.png",
        "outer.png",
    ];
    assert_eq!(begun.links(), links);
    let mut found = markdown::links(text.as_bytes());
    found.retain(|l| l != "inner.png");
    assert_eq!(found, links);
    let sized = begun.finish(&sizes).expect("it reads").document;
    let plain = markdown::read_unsized(text.as_bytes(), objects(), Some("en-US"), 1)
        .expect("it reads")
        .finish(&|_| None)
        .expect("it reads")
        .document;
    let size_of = |props: &Props| {
        (
            props.get(keys::PICTURE_WIDTH).cloned(),
            props.get(keys::PICTURE_HEIGHT).cloned(),
        )
    };
    let mut pictures = 0;
    assert_eq!(sized.paragraphs(), plain.paragraphs());
    for id in sized.paragraphs() {
        let (a, b) = (sized.paragraph(id).unwrap(), plain.paragraph(id).unwrap());
        assert_eq!(a.text, b.text);
        assert_eq!(a.runs.len(), b.runs.len(), "{:?}", a.text);
        for (ra, rb) in a.runs.iter().zip(&b.runs) {
            assert_eq!(ra.len, rb.len);
            let mut stripped = (*ra.props).clone();
            stripped.remove(keys::PICTURE_WIDTH);
            stripped.remove(keys::PICTURE_HEIGHT);
            assert_eq!(&stripped, &*rb.props);
            assert_eq!(size_of(&rb.props), (None, None));
            let want = match ra.props.get(keys::PICTURE_SRC) {
                Some(Value::Text(src)) => {
                    pictures += 1;
                    sizes(src).map_or((None, None), |(w, h)| {
                        (Some(Value::Int(w)), Some(Value::Int(h)))
                    })
                }
                _ => (None, None),
            };
            assert_eq!(size_of(&ra.props), want, "{:?}", a.text);
        }
    }
    // The two of A side by side are one run.
    assert_eq!(pictures, 9);
    let (id, at) = find(&sized, "\u{fffc}\u{fffc} and");
    assert_eq!((at, sized.paragraph(id).unwrap().runs[0].len), (0, 2));
}
