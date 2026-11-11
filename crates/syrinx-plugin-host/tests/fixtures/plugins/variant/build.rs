//! Writes this variant's bindings from the real `wit/v0_1/plugin.wit`, changed
//! as the selected feature says, so no copy of the contract can drift.

use std::{env, fs, path::PathBuf};

const WIT: &str = "../../../../../../wit/v0_1/plugin.wit";

fn main() {
    println!("cargo::rerun-if-changed={WIT}");
    let original = fs::read_to_string(WIT).expect("the contract's WIT is readable");
    let wit = change(&original);
    let features = if enabled("widgets") {
        "features: [\"home-widgets\"],"
    } else {
        ""
    };
    let bindings = format!(
        "wit_bindgen::generate!({{ inline: r##\"{wit}\"##, world: \"plugin\", {features} }});"
    );
    let out = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    fs::write(out.join("bindings.rs"), bindings).expect("OUT_DIR is writable");
}

fn enabled(feature: &str) -> bool {
    let name = feature.to_uppercase().replace('-', "_");
    env::var_os(format!("CARGO_FEATURE_{name}")).is_some()
}

/// Apply the one change the selected feature names.
fn change(wit: &str) -> String {
    if enabled("version-0-2-0") {
        return replace(
            wit,
            "package syrinx:plugin@0.1.0;",
            "package syrinx:plugin@0.2.0;",
        );
    }
    if enabled("since-0-1-1") {
        let wit = replace(
            wit,
            "package syrinx:plugin@0.1.0;",
            "package syrinx:plugin@0.1.1;",
        );
        let wit = replace(
            &wit,
            "interface lifecycle {",
            "@since(version = 0.1.1)\ninterface extra { ping: func(); }\n\ninterface lifecycle {",
        );
        return replace(
            &wit,
            "    import host;",
            "    import host;\n    @since(version = 0.1.1)\n    import extra;",
        );
    }
    if enabled("extra-case") {
        return replace(
            wit,
            "uncertain(string),",
            "uncertain(string),\n        retried(string),",
        );
    }
    if enabled("imports-documents") {
        // A capability interface that the test's manifest does not request. It is named
        // `documents@0.1.0` only to exercise the refusal rule: the real contract never carries it.
        let wit = replace(
            wit,
            "interface lifecycle {",
            "interface documents { read: func() -> string; }\n\ninterface lifecycle {",
        );
        return replace(&wit, "    import host;", "    import host;\n    import documents;");
    }
    if enabled("renamed-export") {
        return replace(
            wit,
            "run: func(id: command-id)",
            "start: func(id: command-id)",
        );
    }
    wit.to_owned()
}

fn replace(wit: &str, from: &str, to: &str) -> String {
    assert!(wit.contains(from), "the WIT no longer contains {from:?}");
    wit.replacen(from, to, 1)
}
