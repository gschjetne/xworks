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
        assert!(!played.editor.view().items.is_empty(), "{name}");
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
    let path = played.editor.path().to_path_buf();
    let dump = played.editor.document().dump();
    let drawn = renders(&played.editor.pages());
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

/// Each page's pixels, drawn portably at 48 to the inch.
fn renders(pages: &canvas::Pages) -> Vec<Vec<u8>> {
    pages
        .pages
        .iter()
        .map(|page| render_cpu::render(page, 48.0, render_cpu::Level::Portable).pixels)
        .collect()
}
