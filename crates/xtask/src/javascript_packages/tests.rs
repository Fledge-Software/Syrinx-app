//! Tests of the top-level JavaScript check, including one end to end on a scratch repository.

use std::path::PathBuf;

use serde_json::json;

use super::*;

#[test]
fn only_the_editor_manifest_and_lockfile_may_be_tracked() {
    let files: Vec<String> = [
        MANIFEST,
        LOCKFILE,
        NPM_SETTINGS,
        "editor/npm-shrinkwrap.json",
        "editor/yarn.lock",
        "editor/bun.lockb",
        "crates/app/package.json",
        ".claude/skills/tool/package.json",
        "pnpm-lock.yaml",
        "crates/app/src/main.rs",
    ]
    .map(str::to_owned)
    .to_vec();
    let found = tracked_file_problems(&files);
    for refused in [
        "editor/npm-shrinkwrap.json",
        "editor/yarn.lock",
        "editor/bun.lockb",
        "crates/app/package.json",
        ".claude/skills/tool/package.json",
        "pnpm-lock.yaml",
    ] {
        assert!(
            found
                .iter()
                .any(|problem| problem.starts_with(&format!("{refused}:"))),
            "{refused}: {found:?}"
        );
    }
    assert_eq!(found.len(), 6, "{found:?}");
}

#[test]
fn a_case_variant_npm_file_is_refused() {
    // Windows and macOS open `NPM-Shrinkwrap.json` when npm asks for `npm-shrinkwrap.json`.
    let files = [
        "editor/NPM-Shrinkwrap.json",
        "editor/Yarn.lock",
        "editor/Dist/extra.js",
        "editor/Package-Lock.json",
    ]
    .map(str::to_owned);
    let found = tracked_file_problems(&files);
    let refused: Vec<&str> = found
        .iter()
        .map(|problem| problem.split(": ").next().unwrap_or_default())
        .collect();
    assert_eq!(refused, files, "{found:?}");
}

#[test]
fn a_non_ascii_path_in_the_editor_folder_is_refused() {
    // U+017F, a long s, folds to `s` under Unicode case folding.
    let files = ["editor/npm-\u{17f}hrinkwrap.json", "docs/caf\u{e9}.md"].map(str::to_owned);
    assert_eq!(
        tracked_file_problems(&files),
        ["editor/npm-\u{17f}hrinkwrap.json: paths in editor/ must be ASCII"]
    );
}

#[test]
fn the_bundle_folder_holds_only_the_bundle_and_its_notices() {
    let files = [BUNDLE, NOTICES, "editor/dist/extra.js"].map(str::to_owned);
    assert_eq!(
        tracked_file_problems(&files),
        [
            "editor/dist/extra.js: editor/dist/ holds only editor/dist/syrinx-editor.js and editor/dist/THIRD-PARTY-NOTICES.txt"
        ]
    );
    assert!(tracked_file_problems(&[NOTICES.to_owned()]).is_empty());
}

#[test]
fn the_npm_settings_must_be_exactly_the_audited_ones() {
    assert_eq!(npm_settings_problem(AUDITED_NPM_SETTINGS), None);
    assert_eq!(
        npm_settings_problem(&AUDITED_NPM_SETTINGS.replace('\n', "\r\n")),
        None
    );
    for refused in [
        String::new(),
        "save-exact=true\n".to_owned(),
        AUDITED_NPM_SETTINGS.replace("ignore-scripts=true", "ignore-scripts=false"),
        format!("ignore-scripts=false\n[x]\n{AUDITED_NPM_SETTINGS}"),
        format!("{AUDITED_NPM_SETTINGS}\"ignore-scripts\"=false\n"),
        format!("{AUDITED_NPM_SETTINGS}# a comment\n"),
    ] {
        assert!(npm_settings_problem(&refused).is_some(), "{refused:?}");
    }
}

#[test]
fn a_range_or_a_version_the_lockfile_does_not_hold_fails() {
    let lock = json!({"packages": {"node_modules/crelt": {"version": "1.0.7"}}});
    let manifest = json!({
        "dependencies": {"crelt": "^1.0.7"},
        "devDependencies": {"esbuild": "0.28.2"},
    });
    let found = dependency_problems(&manifest, &lock);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(
        found[0].contains("`^1.0.7`, not an exact version"),
        "{found:?}"
    );
    assert!(found[1].contains("locks nothing"), "{found:?}");
    assert_eq!(
        dependency_problem("dependencies", "crelt", "1.0.7", &lock),
        None
    );
}

#[test]
fn exact_versions_are_three_numbers() {
    for exact in ["1.0.7", "0.28.2", "10.20.30"] {
        assert!(is_exact_version(exact), "{exact}");
    }
    for loose in [
        "^1.0.7",
        "~1.0",
        "1.0",
        "1.0.x",
        "latest",
        "1.0.7-beta.1",
        ">=1.0.0",
        "",
    ] {
        assert!(!is_exact_version(loose), "{loose}");
    }
}

#[test]
fn the_bundle_hash_activates_once_a_bundle_exists() {
    let hash = "a".repeat(64);
    assert!(judge_bundle(audit::NO_BUNDLE, None).is_ok());
    assert!(judge_bundle(&hash, Some(&hash)).is_ok());
    let unrecorded = judge_bundle(audit::NO_BUNDLE, Some(&hash)).expect_err("refused");
    assert!(
        unrecorded.contains("records no bundle hash"),
        "{unrecorded}"
    );
    let missing = judge_bundle(&hash, None).expect_err("refused");
    assert!(missing.contains("does not exist"), "{missing}");
    let changed = judge_bundle(&hash, Some(&"b".repeat(64))).expect_err("refused");
    assert!(
        changed.contains("but docs/dev/codemirror-audit.md records"),
        "{changed}"
    );
}

#[test]
fn a_committed_bundle_is_hashed_from_its_bytes() {
    let root = scratch("bundle");
    write(&root, BUNDLE, "abc");
    assert_eq!(
        bundle_hash(&root),
        Ok(Some(
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_owned()
        ))
    );
    fs::remove_dir_all(root).expect("scratch removed");
}

#[test]
fn a_folder_git_cannot_read_fails_the_check() {
    // A `.git` file naming a missing directory fails wherever the temporary folder lives.
    let root = scratch("no-git");
    write(
        &root,
        ".git",
        "gitdir: /nonexistent/syrinx-xtask-repository\n",
    );
    let error = tracked_files(&root).expect_err("git refuses the folder");
    assert!(error.contains("git ls-files -z failed"), "{error}");
    fs::remove_dir_all(root).expect("scratch removed");
}

/// Set only in the child run below, naming the scratch repository it reads.
const GIT_VARIABLES_CHILD: &str = "SYRINX_XTASK_GIT_VARIABLES_CHILD";

#[test]
fn bogus_inherited_git_variables_are_ignored() {
    // The child runs this test again with bogus GIT_DIR, GIT_WORK_TREE and GIT_INDEX_FILE set
    // on its process alone, since tests share the parent's environment and run in parallel.
    if let Some(scratch_root) = std::env::var_os(GIT_VARIABLES_CHILD) {
        let tracked = git_output(Path::new(&scratch_root), &["ls-files", "-z"])
            .expect("git reads the scratch repository despite the git variables");
        assert!(tracked.contains(MANIFEST), "{tracked:?}");
        return;
    }
    let root = scratch_repository("git-dir", false);
    let output = std::process::Command::new(std::env::current_exe().expect("the test binary"))
        .args([
            "javascript_packages::tests::bogus_inherited_git_variables_are_ignored",
            "--exact",
        ])
        .env("GIT_DIR", "/nonexistent/syrinx-xtask-git-dir")
        .env("GIT_WORK_TREE", "/nonexistent/syrinx-xtask-work-tree")
        .env("GIT_INDEX_FILE", "/nonexistent/syrinx-xtask-index")
        .env(GIT_VARIABLES_CHILD, &root)
        .output()
        .expect("the child test runs");
    // A filter that matched nothing would also succeed, so require the one test to have run.
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && report.contains("1 passed"),
        "the child run failed ({}):\n{report}",
        output.status
    );
    fs::remove_dir_all(root).expect("scratch removed");
}

#[test]
fn a_clean_scratch_repository_passes_end_to_end() {
    let root = scratch_repository("clean", false);
    assert_eq!(check(&root), Ok(()));
    fs::remove_dir_all(root).expect("scratch removed");
}

#[test]
fn every_kind_of_defect_is_reported_end_to_end() {
    let root = scratch_repository("defects", true);
    let error = check(&root).expect_err("refused");
    for expected in [
        "crates/app/package.json: only editor/package.json",
        "editor/.npmrc must be exactly the audited settings",
        "copyleft 1.0.0 (node_modules/copyleft): licence `GPL-3.0` is not allowed",
        "dependencies.crelt is `^1.0.7`, not an exact version",
        "style-mod 4.1.4 is locked but not in the audit's package table",
        "w3c-keyname 2.2.8 is in the audit but not locked",
        "editor/dist/syrinx-editor.js exists, but docs/dev/codemirror-audit.md records no bundle hash",
        "crelt 1.0.7 is locked at node_modules/a/node_modules/crelt and node_modules/crelt",
    ] {
        assert!(
            error.contains(expected),
            "missing `{expected}` in:\n{error}"
        );
    }
    fs::remove_dir_all(root).expect("scratch removed");
}

#[test]
fn the_repository_packages_match_their_audit() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert_eq!(check(&root), Ok(()));
}

/// A git repository holding a synthetic editor, lockfile, audit and licence policy; when
/// `defective`, it carries one defect of every kind the check reports.
fn scratch_repository(name: &str, defective: bool) -> PathBuf {
    let root = scratch(name);
    let tarball = |package: &str, version: &str| {
        json!({
            "version": version,
            "resolved": format!("https://registry.npmjs.org/{package}/-/{package}-{version}.tgz"),
            "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
            "license": "MIT",
        })
    };
    let mut packages = json!({
        "": {"name": "syrinx-editor"},
        "node_modules/crelt": tarball("crelt", "1.0.7"),
    });
    let mut rows = vec![
        "| crelt | 1.0.7 | MIT | none | sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA== |",
    ];
    let mut requirement = "1.0.7";
    let mut settings = AUDITED_NPM_SETTINGS;
    if defective {
        let mut evil = tarball("crelt", "1.0.7");
        evil["integrity"] = json!(
            "sha512-EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEA=="
        );
        packages["node_modules/a/node_modules/crelt"] = evil;
        packages["node_modules/style-mod"] = tarball("style-mod", "4.1.4");
        let mut copyleft = tarball("copyleft", "1.0.0");
        copyleft["license"] = json!("GPL-3.0");
        packages["node_modules/copyleft"] = copyleft;
        rows.push("| copyleft | 1.0.0 | GPL-3.0 | none | sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA== |");
        rows.push("| w3c-keyname | 2.2.8 | MIT | none | sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA== |");
        requirement = "^1.0.7";
        settings = "save-exact=true\n";
        write(&root, "crates/app/package.json", "{}");
        write(&root, BUNDLE, "bundle");
    }

    write(&root, "licences.toml", "allowed = [\"MIT\"]\n");
    write(&root, NPM_SETTINGS, settings);
    let manifest = json!({"dependencies": {"crelt": requirement}});
    write(&root, MANIFEST, &manifest.to_string());
    let lock = json!({"lockfileVersion": 3, "packages": packages});
    write(&root, LOCKFILE, &lock.to_string());
    let audit_text = format!(
        "# Audit\n\n{} `none`\n\n## Package table\n\n| Package | Version | Licence | Install script | Integrity |\n|---|---|---|---|---|\n{}\n",
        audit::BUNDLE_HASH_LABEL,
        rows.join("\n")
    );
    write(&root, AUDIT, &audit_text);
    git(&root, &["init", "--quiet"]);
    git(&root, &["add", "--all"]);
    root
}

fn git(root: &Path, arguments: &[&str]) {
    if let Err(error) = git_output(root, arguments) {
        panic!("{error}");
    }
}

fn write(root: &Path, relative: &str, text: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("a parent folder")).expect("folder");
    fs::write(path, text).expect("file");
}

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "syrinx-xtask-javascript-{name}-{}",
        std::process::id()
    ));
    if root.exists() {
        fs::remove_dir_all(&root).expect("old scratch removed");
    }
    fs::create_dir_all(&root).expect("scratch folder");
    root
}
