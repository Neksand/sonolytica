//! Docs-as-tests: every command in the agent skill must exist in the binary.
//!
//! The skill is the product's interface, and it is prose. A command renamed in
//! the CLI but not in the skill would make an agent fail mid-task with no test
//! catching it. This walks every fenced code block in `skills/sonolytica/` and
//! checks the command against the real `--help` output.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

fn help(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_sonolytica"))
        .args(args)
        .output()
        .expect("run sonolytica");
    assert!(
        output.status.success(),
        "`sonolytica {}` failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Command names from a clap help section, e.g. the first column of "Commands:".
fn command_names(help: &str) -> BTreeSet<String> {
    help.lines()
        .skip_while(|line| !line.trim_start().starts_with("Commands:"))
        .skip(1)
        .take_while(|line| !line.trim().is_empty() && line.starts_with("  "))
        .filter_map(|line| line.split_whitespace().next())
        .map(|name| name.trim_end_matches(',').to_string())
        .collect()
}

fn skip_token(token: &str) -> bool {
    token.starts_with('<') || token.starts_with('$') || token.starts_with('-')
}

fn markdown_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        panic!("skill directory is missing: {}", dir.display());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            markdown_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "md") {
            files.push(path);
        }
    }
}

#[test]
fn every_documented_command_exists() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/sonolytica");
    let mut files = Vec::new();
    markdown_files(&root, &mut files);
    assert!(!files.is_empty(), "no skill markdown found under {root:?}");

    let top_level = command_names(&help(&["--help"]));
    let query_commands = command_names(&help(&["query", "--help"]));
    let analyze_help = help(&["analyze", "--help"]);

    let mut checked = 0_usize;
    for file in files {
        let text = std::fs::read_to_string(&file).expect("read skill file");
        let mut fenced = false;
        for line in text.lines() {
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
                continue;
            }
            if !fenced || line.trim_start().starts_with('#') {
                continue;
            }
            let tokens: Vec<&str> = line.split_whitespace().collect();
            let Some(position) = tokens.iter().position(|token| *token == "sonolytica") else {
                continue;
            };
            let rest = &tokens[position + 1..];
            let Some(command) = rest.first() else {
                continue;
            };
            if skip_token(command) {
                continue;
            }

            if *command == "query" {
                let Some(subcommand) = rest.get(1) else {
                    continue;
                };
                if skip_token(subcommand) {
                    continue;
                }
                assert!(
                    query_commands.contains(*subcommand),
                    "{}: `query {subcommand}` does not exist; known: {query_commands:?}\n  {line}",
                    file.display()
                );
                checked += 1;
            } else {
                assert!(
                    top_level.contains(*command),
                    "{}: `{command}` does not exist; known: {top_level:?}\n  {line}",
                    file.display()
                );
                checked += 1;
                if *command == "analyze" {
                    for token in rest.iter().filter(|token| token.starts_with("--")) {
                        let flag = token.split('=').next().unwrap_or(token);
                        assert!(
                            analyze_help.contains(flag),
                            "{}: `analyze {flag}` is not a flag of analyze\n  {line}",
                            file.display()
                        );
                    }
                }
            }
        }
    }
    assert!(
        checked >= 10,
        "only {checked} commands checked; parser drift?"
    );
}
