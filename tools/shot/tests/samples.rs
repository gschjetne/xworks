//! The sample documents written as `.xw` files (`shot --xw`): each opens
//! as a document, the stylesheets' sample in its own sheet, the pictures'
//! with the pictures it names kept in its file, and a second run leaves
//! the files there as they are.

use doc::Value;
use store::Store;
use story::keys;

#[test]
fn the_samples_open_as_documents() {
    let dir = testkit::scratch_dir("shot-samples").expect("a folder");
    let (written, left) = shot::write_samples(&dir).expect("written");
    assert!(left.is_empty());
    let names: Vec<String> = written
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        [
            "A walking skeleton.xw",
            "Blocks of a story.xw",
            "Pictures in a story.xw",
            "The Shape of a Page (Neutral).xw",
            "The Shape of a Page (Book).xw",
            "The Shape of a Page (Literary).xw",
            "The Shape of a Page (Modern).xw",
            "The Shape of a Page (Technical).xw",
            "The Shape of a Page (Manuscript).xw",
        ]
    );
    for path in &written {
        let opened = Store::open(path).expect("it opens");
        assert!(opened.document.paragraphs().len() > 1, "{}", path.display());
    }

    let book = Store::open(&dir.join("The Shape of a Page (Book).xw")).expect("it opens");
    let sheet = book.document.stylesheet().expect("a stylesheet");
    assert_eq!(
        sheet.props.get(keys::ORIGIN),
        Some(&Value::Text("Book".to_string()))
    );

    let pictures = Store::open(&dir.join("Pictures in a story.xw")).expect("it opens");
    let kept = pictures.store.image_ids().expect("its pictures");
    assert_eq!(kept.len(), 2, "the landscape and the heart");
    let named: Vec<String> = pictures
        .document
        .paragraphs()
        .iter()
        .filter_map(|&id| pictures.document.paragraph(id))
        .flat_map(|p| p.runs.iter())
        .filter_map(|r| match r.props.get(keys::PICTURE) {
            Some(Value::Text(id)) => Some(id.clone()),
            _ => None,
        })
        .collect();
    assert!(kept.iter().all(|id| named.contains(id)));

    let (again, left) = shot::write_samples(&dir).expect("written again");
    assert!(again.is_empty());
    assert_eq!(left, written);
}
