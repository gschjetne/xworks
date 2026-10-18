//! Renders every scene that has a golden record again and checks it against
//! the record (spec/golden/README.md). A mismatch writes the new render to
//! `target/golden-reports/`, for a person to look at.

use testkit::golden;

#[test]
fn every_recorded_scene_matches_its_golden() {
    let records = golden::load_all(&golden::dir()).expect("spec/golden/ reads");
    let mut failures = Vec::new();
    for record in &records {
        let Some(scene) = shot::find(&record.name) else {
            failures.push(format!(
                "{}: recorded, but there is no such scene",
                record.name
            ));
            continue;
        };
        let image = (scene.render)();
        if !record.matches(&image) {
            let path = golden::report_dir().join(format!("{}.png", record.name));
            image.write_png(&path).expect("the report writes");
            failures.push(format!(
                "{}: the render differs from the approved one; see {}",
                record.name,
                path.display()
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn spec_golden_holds_only_records_and_its_readme() {
    for entry in std::fs::read_dir(golden::dir()).expect("spec/golden/ reads") {
        let path = entry.expect("spec/golden/ reads").path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            name == "README.md" || name.ends_with(".ron"),
            "spec/golden/ holds records and its README only, not {name}"
        );
    }
}
