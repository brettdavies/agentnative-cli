//! Pins what `scripts/check-surface-bump.sh`, the required changelog-bump
//! gate, counts as anc's surface: a schema bump that moves only `$id` and
//! `examples` is not a removal, and every requirement and audit ID in
//! `coverage/matrix.json` is surface. Each case builds a base and a head tree
//! from the committed files, edits the head, and runs the gate on both.

#![cfg(unix)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use serde_json::{Value, json};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every file the gate reads from a tree; the changelog parser and the
/// manifest are read from the head tree only, but both trees carry them.
const TREE_FILES: [&str; 6] = [
    "completions/anc.bash",
    "schema/scorecard.schema.json",
    "schema/web-scorecard.schema.json",
    "coverage/matrix.json",
    "scripts/generate-changelog.py",
    "Cargo.toml",
];

struct Trees {
    _tmp: tempfile::TempDir,
    root: PathBuf,
}

impl Trees {
    fn new() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().to_path_buf();
        for side in ["base", "head"] {
            for rel in TREE_FILES {
                let dst = root.join(side).join(rel);
                fs::create_dir_all(dst.parent().unwrap()).unwrap();
                fs::copy(repo_root().join(rel), &dst).unwrap_or_else(|e| panic!("copy {rel}: {e}"));
            }
        }
        Trees { _tmp: tmp, root }
    }

    fn edit_head(&self, rel: &str, edit: impl FnOnce(&mut Value)) {
        let path = self.root.join("head").join(rel);
        let mut doc: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        edit(&mut doc);
        fs::write(&path, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
    }

    /// Runs the gate with a PR body whose changelog block files one bullet
    /// under `heading`, and returns its exit code and stderr.
    fn check(&self, heading: &str) -> (i32, String) {
        let body = self.root.join("body.md");
        fs::write(
            &body,
            format!("## Summary\n\nx\n\n## Changelog\n\n### {heading}\n\n- Change x.\n"),
        )
        .unwrap();
        let out = Command::new("bash")
            .arg(repo_root().join("scripts/check-surface-bump.sh"))
            .arg("check")
            .arg(self.root.join("base"))
            .arg(self.root.join("head"))
            .arg(&body)
            .output()
            .expect("run check-surface-bump.sh");
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
}

fn bump_scorecard_schema(doc: &mut Value) {
    let id = doc["$id"]
        .as_str()
        .unwrap()
        .replace(".schema.json", "-next.schema.json");
    doc["$id"] = json!(id);
    doc["properties"]["schema_version"]["examples"] = json!(["next"]);
    for example in doc["examples"].as_array_mut().unwrap() {
        example["schema_version"] = json!("next");
    }
    doc["properties"]["new_field"] = json!({"type": "string", "description": "added"});
}

#[test]
fn an_additive_schema_bump_is_growth_not_a_removal() {
    let trees = Trees::new();
    trees.edit_head("schema/scorecard.schema.json", bump_scorecard_schema);

    let (code, stderr) = trees.check("Added");
    assert_eq!(
        code, 0,
        "an Added bullet covers an additive schema bump:\n{stderr}"
    );

    let (code, stderr) = trees.check("Changed");
    assert_eq!(
        code, 1,
        "a new schema field needs a minor section:\n{stderr}"
    );
    assert!(
        stderr.contains("new_field"),
        "the new field is listed:\n{stderr}"
    );
    assert!(
        !stderr.contains("lost entries"),
        "moving $id and examples is not a removal:\n{stderr}"
    );
}

#[test]
fn a_new_requirement_or_audit_id_is_surface() {
    let trees = Trees::new();
    trees.edit_head("coverage/matrix.json", |doc| {
        doc["rows"].as_array_mut().unwrap().push(json!({
            "id": "p9-must-new-requirement",
            "verifiers": [{"audit_id": "p9-new-audit"}],
        }));
    });

    let (code, stderr) = trees.check("Changed");
    assert_eq!(code, 1, "a new ID filed under Changed must fail:\n{stderr}");
    assert!(
        stderr.contains("p9-must-new-requirement") && stderr.contains("p9-new-audit"),
        "both new IDs are listed:\n{stderr}"
    );

    let (code, stderr) = trees.check("Added");
    assert_eq!(code, 0, "an Added bullet covers a new ID:\n{stderr}");
}
