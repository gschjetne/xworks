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
