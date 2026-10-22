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
