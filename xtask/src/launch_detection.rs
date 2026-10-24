use crate::{Result, command, node_executable, run};
use atelier_core::document::Document;

pub fn test() -> Result<()> {
    let fixture = tempfile::tempdir()?;
    let expected = fixture.path().join("expected.json");
    run(command(node_executable())
        .args(["test/fixtures/rust-launch-detection-reference.mjs", "seed"])
        .arg(fixture.path())
        .arg(&expected))?;
    let expected: Vec<Document> = serde_json::from_slice(&std::fs::read(expected)?)?;
    let mut actual = Vec::new();
    for case in expected {
        let name = case.get::<String>("name")?.ok_or("Fixture sans nom.")?;
        let mut record = Document::default();
        record.set("name", &name)?;
        record.set(
            "value",
            &atelier_app::infrastructure::launch_detection::detect(&fixture.path().join(name))?,
        )?;
        actual.push(record);
    }
    let result = fixture.path().join("actual.json");
    std::fs::write(&result, serde_json::to_vec(&actual)?)?;
    run(command(node_executable())
        .args([
            "test/fixtures/rust-launch-detection-reference.mjs",
            "verify",
        ])
        .arg(fixture.path())
        .arg(result))
}
