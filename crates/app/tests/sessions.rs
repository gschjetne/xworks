//! Scripted sessions (spec/sessions/, docs/ROADMAP.md, M1 stage 9): input
//! played headless into an editor, and the document compared with what the
//! session expects.

use std::sync::{Arc, OnceLock};

use app::{Resources, session};

fn resources() -> &'static Resources {
    static RES: OnceLock<Resources> = OnceLock::new();
    RES.get_or_init(|| {
        let root = testkit::workspace_root();
        Resources::load(&root, root.join("target/scratch/app-unused"), false).expect("resources")
    })
}

#[test]
fn sessions_end_as_they_expect() {
    let sessions = testkit::specs("sessions").expect("spec/sessions");
    assert!(!sessions.is_empty());
    let mut failures = Vec::new();
    for (name, source) in &sessions {
        let session = session::read(source).unwrap_or_else(|e| panic!("{name}.ron: {e}"));
        let mut res = resources().clone();
        res.documents = testkit::scratch_dir(&format!("app-session-{name}")).expect("a folder");
        let played = match session::play(Arc::new(res), &session) {
            Ok(played) => played,
            Err(e) => {
                failures.push(format!("{name}: {e}"));
                continue;
            }
        };
        for problem in session::check(&session, &played) {
            failures.push(format!("{name}: {problem}"));
        }
        // The window draws.
        assert!(!played.window.view().items.is_empty(), "{name}");
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// Export as PDF writes the PDF beside the document and asks the shell to
/// show it, so the export is seen to happen.
#[test]
fn export_writes_beside_the_document_and_shows_it() {
    let mut res = resources().clone();
    res.documents = testkit::scratch_dir("app-export").expect("a folder");
    let mut editor = app::Editor::create(Arc::new(res)).expect("a document");
    editor.update(app::Event::Text("Exported".to_string()));
    let pdf = editor.path().with_extension("pdf");
    let effects = editor.update(app::Event::Command("export-pdf".to_string()));
    assert_eq!(effects, vec![app::Effect::Reveal(pdf.clone())]);
    let bytes = std::fs::read(&pdf).expect("the PDF");
    assert!(bytes.starts_with(b"%PDF-"));
}

/// The CJK faces load only once text needs them, for every window, and the
/// text is then drawn in them.
#[test]
fn cjk_faces_load_when_text_needs_them() {
    let root = testkit::workspace_root();
    let documents = testkit::scratch_dir("app-on-demand").expect("a folder");
    let res = Arc::new(Resources::load(&root, documents, false).expect("resources"));
    let cjk = |res: &Resources| {
        res.fonts()
            .faces()
            .find(|(_, f)| f.family() == "Noto Serif CJK SC" && f.weight() == 400)
            .map(|(_, f)| f.font())
    };
    // A copy made before, as another window's would be.
    let copy = (*res).clone();
    let mut editor = app::Editor::create(res.clone()).expect("a document");
    editor.update(app::Event::Text("Hello".to_string()));
    assert!(cjk(&res).is_none(), "loaded before any CJK text");
    editor.update(app::Event::Text(", 世界".to_string()));
    let face = cjk(&res).expect("loaded once CJK is typed");
    let drawn = editor.view().items.iter().any(|item| match item {
        scene::Item::Glyphs(g) => g.font.same(&face),
        _ => false,
    });
    assert!(drawn, "the Chinese is drawn in the CJK face");
    assert!(cjk(&copy).is_some(), "every copy shares them");
}

/// A new document is in the system's language. Justified, its words
/// hyphenate: the patterns for its language load the first time they're
/// needed, for every window, and the pages show hyphens, which read as soft
/// hyphens.
#[test]
fn justified_text_loads_its_patterns_and_hyphenates() {
    let root = testkit::workspace_root();
    let documents = testkit::scratch_dir("app-hyphenation").expect("a folder");
    let mut res = Resources::load(&root, documents, false).expect("resources");
    res.language = text::Language::parse("en-US").expect("a tag");
    let res = Arc::new(res);
    let us = text::Language::parse("en-US").expect("a tag");
    let mut editor = app::Editor::create(res.clone()).expect("a document");
    editor.update(app::Event::Text(
        "Supercalifragilisticexpialidocious, said the counterrevolutionaries. ".repeat(6),
    ));
    assert!(!res.hyphenation().has(&us), "loaded for ragged text");
    editor.update(app::Event::Command("align-justify".to_string()));
    assert!(res.hyphenation().has(&us), "loaded once text is justified");
    let hyphens = editor
        .pages()
        .pages
        .iter()
        .flat_map(|page| &page.items)
        .filter(|item| matches!(item, scene::Item::Glyphs(g) if g.text.ends_with('\u{ad}')))
        .count();
    assert!(hyphens > 0, "no word was hyphenated");
}

/// M1's exit (docs/ROADMAP.md): the document the exit session types
/// reopens with the same dump and draws the same pages, and Export as PDF
/// gives those pages, with every paragraph's text extractable.
#[test]
fn the_exit_document_reopens_and_exports_the_same() {
    let source = testkit::specs("sessions")
        .expect("spec/sessions")
        .into_iter()
        .find(|(name, _)| name == "exit")
        .expect("spec/sessions/exit.ron")
        .1;
    let mut exit = session::read(&source).expect("a session");
    // The session ends by reopening the document; here that's done by hand.
    // Until then the caret is at the end, not where the status expects it.
    assert!(matches!(exit.events.pop(), Some(session::Step::Reopen)));
    exit.status = None;
    let mut res = resources().clone();
    res.documents = testkit::scratch_dir("app-exit").expect("a folder");
    let res = Arc::new(res);
    let played = session::play(res.clone(), &exit).expect("the session plays");
    let problems = session::check(&exit, &played);
    assert!(problems.is_empty(), "{problems:?}");
    let path = played.editor().path().to_path_buf();
    let dump = played.editor().document().dump();
    let drawn = renders(&played.editor().pages());
    assert!(drawn.len() >= 3, "several pages, not {}", drawn.len());
    drop(played);

    let mut reopened = app::Editor::open(res, &path).expect("it reopens");
    assert_eq!(reopened.document().dump(), dump, "the same dump");
    let pages = reopened.pages();
    assert!(renders(&pages) == drawn, "the same pages");

    reopened.update(app::Event::Command("export-pdf".to_string()));
    let pdf = std::fs::read(path.with_extension("pdf")).expect("the PDF");
    let read = lopdf::Document::load_mem(&pdf).expect("the PDF reads");
    assert_eq!(read.get_pages().len(), pages.pages.len());
    let squeeze = |text: &str| -> String { text.chars().filter(|c| !c.is_whitespace()).collect() };
    let text = squeeze(&pdf_extract::extract_text_from_mem(&pdf).expect("text extracts"));
    // pdf-extract 0.12 misreads some of this PDF's Arabic letters (ب as ن,
    // ع as غ, ت as ي), which pdftotext reads right, so with it each
    // paragraph is checked but for its Arabic.
    let arabic = |c: char| matches!(c, '\u{0600}'..='\u{06FF}' | '\u{0750}'..='\u{077F}');
    let missing: Vec<&str> = exit
        .expect
        .iter()
        .flat_map(|(_, paragraph)| paragraph.split(arabic))
        .filter(|stretch| !text.contains(&squeeze(stretch)))
        .collect();
    assert!(missing.is_empty(), "not extracted: {missing:?}");
    // The second reader, through the fonts' ToUnicode maps and krilla's
    // ActualText, reads every paragraph whole, its Arabic too.
    let text = squeeze(&testkit::pdf::text(&pdf).expect("text reads"));
    let missing: Vec<&str> = exit
        .expect
        .iter()
        .map(|(_, paragraph)| paragraph.as_str())
        .filter(|paragraph| !text.contains(&squeeze(paragraph)))
        .collect();
    assert!(missing.is_empty(), "not read: {missing:?}\nfrom: {text}");
}

/// M2's exit (docs/ROADMAP.md, review 7fnfdhb5): the documents the exit
/// session writes, a document with pictures and a Markdown file, reopen
/// after xWorks quits as they were left, the Markdown byte for byte, with
/// the same story and the same pages; and each prints to PDF as those
/// pages, its text extractable. The session itself goes on to quit, open
/// both again and print them; here that's done by hand, to compare.
#[test]
fn the_m2_exit_documents_reopen_and_print_the_same() {
    let source = testkit::specs("sessions")
        .expect("spec/sessions")
        .into_iter()
        .find(|(name, _)| name == "exit-m2")
        .expect("spec/sessions/exit-m2.ron")
        .1;
    let mut exit = session::read(&source).expect("a session");
    let quit = exit
        .events
        .iter()
        .position(|s| matches!(s, session::Step::Command(c) if c == "quit"))
        .expect("xWorks quits");
    exit.events.truncate(quit);
    let mut res = resources().clone();
    res.documents = testkit::scratch_dir("app-exit-m2").expect("a folder");
    res.language = text::Language::parse("en-GB").expect("a tag");
    let res = Arc::new(res);
    let played = session::play(res.clone(), &exit).expect("the session plays");
    assert!(played.problems.is_empty(), "{:?}", played.problems);
    // The Markdown file's window in front, the document's behind it.
    let markdown = played.editor();
    assert!(markdown.is_markdown());
    let document = played
        .behind
        .iter()
        .map(app::Window::editor)
        .find(|e| !e.is_markdown())
        .expect("the document with pictures");
    assert!(
        !story::pictures(document.document()).is_empty(),
        "it has pictures"
    );
    let md_path = markdown.path().to_path_buf();
    let md_bytes = std::fs::read(&md_path).expect("the Markdown file");
    let md_outline = story::outline(markdown.document(), markdown.sheet());
    let md_drawn = renders(&markdown.pages());
    let xw_path = document.path().to_path_buf();
    let xw_dump = document.document().dump();
    let xw_paragraphs = paragraph_texts(document.document());
    let xw_drawn = renders(&document.pages());
    let md_paragraphs = paragraph_texts(markdown.document());
    // xWorks quits: every window closes.
    let session::Played {
        mut window,
        mut behind,
        ..
    } = played;
    window.close();
    for other in &mut behind {
        other.close();
    }
    drop((window, behind));
    assert_eq!(
        std::fs::read(&md_path).expect("the Markdown file"),
        md_bytes,
        "quitting wrote it as it was"
    );

    // Opened again, each is as it was left.
    let mut md = app::Editor::open(res.clone(), &md_path).expect("it reopens");
    assert_eq!(story::outline(md.document(), md.sheet()), md_outline);
    let md_pages = md.pages();
    assert!(renders(&md_pages) == md_drawn, "the Markdown's same pages");
    let mut xw = app::Editor::open(res.clone(), &xw_path).expect("it reopens");
    assert_eq!(xw.document().dump(), xw_dump, "the same dump");
    let xw_pages = xw.pages();
    assert!(xw_drawn.len() >= 2, "several pages, not {}", xw_drawn.len());
    assert!(renders(&xw_pages) == xw_drawn, "the document's same pages");
    // Opening wrote nothing.
    assert_eq!(std::fs::read(&md_path).unwrap(), md_bytes);

    // Printed to PDF, beside themselves: their pages, their text read
    // back, each stretch between tabs and pictures.
    for (editor, pages, paragraphs) in [
        (&mut md, &md_pages, &md_paragraphs),
        (&mut xw, &xw_pages, &xw_paragraphs),
    ] {
        editor.update(app::Event::Command("export-pdf".to_string()));
        let pdf = std::fs::read(editor.path().with_extension("pdf")).expect("the PDF");
        let read = lopdf::Document::load_mem(&pdf).expect("the PDF reads");
        assert_eq!(read.get_pages().len(), pages.pages.len());
        // Without spaces, and without the soft hyphens Book's justified
        // lines break words at.
        let squeeze = |text: &str| -> String {
            text.chars()
                .filter(|c| !c.is_whitespace() && *c != '\u{ad}')
                .collect()
        };
        let text = squeeze(&testkit::pdf::text(&pdf).expect("text reads"));
        let missing: Vec<&str> = paragraphs
            .iter()
            .flat_map(|p| p.split(['\t', '\u{fffc}', '\u{2028}']))
            .filter(|stretch| !text.contains(&squeeze(stretch)))
            .collect();
        assert!(missing.is_empty(), "not read: {missing:?}\nfrom: {text}");
    }
}

/// Each paragraph's text, in reading order.
fn paragraph_texts(document: &doc::Document) -> Vec<String> {
    document
        .paragraphs()
        .iter()
        .filter_map(|&id| document.paragraph(id).map(|p| p.text.clone()))
        .collect()
}

/// The editor lays out again only what an edit changed: whatever a session
/// did, its pages draw as those of its document laid out afresh, as
/// opening it does, with the same structure.
#[test]
fn layout_kept_through_edits_is_layout_made_afresh() {
    let mut failures = Vec::new();
    for (name, source) in testkit::specs("sessions").expect("spec/sessions") {
        let session = session::read(&source).unwrap_or_else(|e| panic!("{name}.ron: {e}"));
        let mut res = resources().clone();
        res.documents = testkit::scratch_dir(&format!("app-afresh-{name}")).expect("a folder");
        let res = Arc::new(res);
        let mut played = session::play(res.clone(), &session).expect("the session plays");
        // A composition in progress is shown but not in the document:
        // it's cancelled first.
        played.window.update(app::Event::Preedit(String::new()));
        let pages = played.editor().pages();
        let path = played.editor().path().to_path_buf();
        drop(played);
        let fresh = app::Editor::open(res, &path).expect("it opens").pages();
        if pages.structure != fresh.structure || renders(&pages) != renders(&fresh) {
            failures.push(name);
        }
    }
    assert!(failures.is_empty(), "drawn otherwise afresh: {failures:?}");
}

/// Each page's pixels, drawn portably at 48 to the inch.
fn renders(pages: &canvas::Pages) -> Vec<Vec<u8>> {
    pages
        .pages
        .iter()
        .map(|page| render_cpu::render(page, 48.0, render_cpu::Level::Portable).pixels)
        .collect()
}
