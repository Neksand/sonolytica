//! Install the agent skill into the harnesses a user actually runs.
//!
//! The skill files are compiled into the binary, so `brew install` /
//! `cargo install` is enough: no repository checkout is needed at install time.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::ValueEnum;

/// Directory name and skill id.
const NAME: &str = "sonolytica";

/// Every file that makes up the skill, relative to the skill root.
const FILES: &[(&str, &str)] = &[
    (
        "SKILL.md",
        include_str!("../../../skills/sonolytica/SKILL.md"),
    ),
    (
        "README.md",
        include_str!("../../../skills/sonolytica/README.md"),
    ),
    (
        "references/citation-rules.md",
        include_str!("../../../skills/sonolytica/references/citation-rules.md"),
    ),
    (
        "references/modalities.md",
        include_str!("../../../skills/sonolytica/references/modalities.md"),
    ),
    (
        "references/profiles.md",
        include_str!("../../../skills/sonolytica/references/profiles.md"),
    ),
    (
        "references/query-cheatsheet.md",
        include_str!("../../../skills/sonolytica/references/query-cheatsheet.md"),
    ),
    (
        "references/troubleshooting.md",
        include_str!("../../../skills/sonolytica/references/troubleshooting.md"),
    ),
    (
        "references/workflows/interview.md",
        include_str!("../../../skills/sonolytica/references/workflows/interview.md"),
    ),
    (
        "references/workflows/lecture.md",
        include_str!("../../../skills/sonolytica/references/workflows/lecture.md"),
    ),
    (
        "references/workflows/meeting.md",
        include_str!("../../../skills/sonolytica/references/workflows/meeting.md"),
    ),
    (
        "references/workflows/music.md",
        include_str!("../../../skills/sonolytica/references/workflows/music.md"),
    ),
    (
        "references/workflows/video.md",
        include_str!("../../../skills/sonolytica/references/workflows/video.md"),
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Target {
    /// Every supported harness
    All,
    /// Claude Code (~/.claude/skills)
    Claude,
    /// Codex (~/.codex/skills)
    Codex,
    /// OpenCode (~/.config/opencode/skills)
    Opencode,
    /// Cursor (~/.cursor/rules, one .mdc file)
    Cursor,
}

#[derive(Debug)]
pub struct Options {
    pub target: Target,
    /// Write into the current directory instead of the home directory.
    pub project: bool,
    pub dry_run: bool,
    pub uninstall: bool,
}

pub fn run(options: &Options) -> Result<()> {
    let home = home_dir()?;
    let targets = match options.target {
        Target::All => vec![
            Target::Claude,
            Target::Codex,
            Target::Opencode,
            Target::Cursor,
        ],
        other => vec![other],
    };
    let mut written = 0_usize;
    for target in targets {
        let base = base_dir(target, &home, options.project);
        if options.uninstall {
            if options.dry_run {
                println!("{:<8} would remove {}", label(target), display(&base));
                continue;
            }
            let removed = if target == Target::Cursor {
                remove_file(&base.join(format!("{NAME}.mdc")))?
            } else {
                remove_dir(&base.join(NAME))?
            };
            println!(
                "{:<8} {} {}",
                label(target),
                if removed { "removed" } else { "not found" },
                display(&base)
            );
            continue;
        }
        if target == Target::Cursor {
            let path = base.join(format!("{NAME}.mdc"));
            let body = cursor_rule();
            write_file(&path, &body, options.dry_run)?;
        } else {
            for (relative, contents) in FILES {
                let path = base.join(NAME).join(relative);
                write_file(&path, contents, options.dry_run)?;
            }
        }
        written += 1;
        println!(
            "{:<8} {} {}",
            label(target),
            if options.dry_run {
                "would install"
            } else {
                "installed"
            },
            display(&base)
        );
    }
    if written == 0 && !options.uninstall {
        bail!("nothing to install");
    }
    if !options.uninstall && !options.dry_run {
        println!(
            "\nRestart the harness, then ask it to analyse a media file. \
             The skill id is `{NAME}`."
        );
    }
    Ok(())
}

fn label(target: Target) -> &'static str {
    match target {
        Target::All => "all",
        Target::Claude => "claude",
        Target::Codex => "codex",
        Target::Opencode => "opencode",
        Target::Cursor => "cursor",
    }
}

fn base_dir(target: Target, home: &Path, project: bool) -> PathBuf {
    let root = if project {
        PathBuf::from(".")
    } else {
        home.to_path_buf()
    };
    match target {
        Target::Claude => root.join(".claude/skills"),
        Target::Codex => root.join(".codex/skills"),
        Target::Opencode => {
            if project {
                root.join(".opencode/skills")
            } else {
                root.join(".config/opencode/skills")
            }
        }
        Target::Cursor => root.join(".cursor/rules"),
        Target::All => root,
    }
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set; pass --project to install into the current directory")
}

fn write_file(path: &Path, contents: &str, dry_run: bool) -> Result<()> {
    if dry_run {
        println!("  would write {}", display(path));
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    std::fs::write(path, contents).with_context(|| format!("could not write {}", path.display()))
}

fn remove_dir(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    std::fs::remove_dir_all(path)
        .with_context(|| format!("could not remove {}", path.display()))?;
    Ok(true)
}

fn remove_file(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    std::fs::remove_file(path).with_context(|| format!("could not remove {}", path.display()))?;
    Ok(true)
}

fn display(path: &Path) -> String {
    path.display().to_string()
}

/// Cursor reads `.mdc` rules, not skill folders: embed the core without its
/// YAML frontmatter and point at the CLI for the rest.
fn cursor_rule() -> String {
    let core = FILES
        .iter()
        .find(|(name, _)| *name == "SKILL.md")
        .map(|(_, contents)| *contents)
        .unwrap_or_default();
    let body = core
        .split_once("---")
        .and_then(|(_, rest)| rest.split_once("---").map(|(_, body)| body))
        .unwrap_or(core)
        .trim_start();
    format!(
        "---\ndescription: SONOLYTICA media analysis with citations (transcript, speakers, shots, OCR, music)\nglobs: [\"*.mp4\", \"*.mkv\", \"*.mov\", \"*.webm\", \"*.wav\", \"*.mp3\", \"*.m4a\", \"*.flac\", \"*.json\"]\nalwaysApply: false\n---\n\n{body}\n"
    )
}
