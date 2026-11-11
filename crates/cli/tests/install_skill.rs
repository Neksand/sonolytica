//! `sonolytica install-skill` copies the embedded skill into each harness.

use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_sonolytica")
}

fn home(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sonolytica-home-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn install(home: &Path, args: &[&str]) -> std::process::Output {
    Command::new(binary())
        .args(["install-skill"])
        .args(args)
        .env("HOME", home)
        .output()
        .expect("run install-skill")
}

#[test]
fn installs_into_every_harness_and_uninstalls() {
    let home = home("all");
    let output = install(&home, &[]);
    assert!(
        output.status.success(),
        "install failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let opencode = home.join(".config/opencode/skills/sonolytica");
    let skill = std::fs::read_to_string(opencode.join("SKILL.md")).expect("opencode skill");
    assert!(skill.contains("name: sonolytica"));
    assert!(opencode.join("references/query-cheatsheet.md").exists());

    assert!(home.join(".claude/skills/sonolytica/SKILL.md").exists());
    assert!(home.join(".codex/skills/sonolytica/SKILL.md").exists());

    let cursor = std::fs::read_to_string(home.join(".cursor/rules/sonolytica.mdc")).unwrap();
    assert!(cursor.starts_with("---\n"));
    assert!(cursor.contains("description: SONOLYTICA"));
    assert!(
        !cursor.contains("name: sonolytica"),
        "frontmatter must be stripped"
    );
    assert!(cursor.contains("media analysis with citations"));

    // Idempotent: a second run overwrites and still succeeds.
    assert!(install(&home, &[]).status.success());
    assert!(opencode.join("SKILL.md").exists());

    assert!(
        install(&home, &["--target", "opencode", "--uninstall"])
            .status
            .success()
    );
    assert!(!opencode.exists());
    assert!(home.join(".claude/skills/sonolytica").exists());
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn dry_run_writes_nothing() {
    let home = home("dry");
    let output = install(&home, &["--dry-run", "--target", "claude"]);
    assert!(output.status.success());
    assert!(!home.join(".claude").exists());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("would write"), "{stdout}");

    assert!(install(&home, &["--target", "claude"]).status.success());
    assert!(
        install(&home, &["--target", "claude", "--uninstall", "--dry-run"])
            .status
            .success()
    );
    assert!(
        home.join(".claude/skills/sonolytica").exists(),
        "a dry-run uninstall must not delete anything"
    );
    let _ = std::fs::remove_dir_all(&home);
}
