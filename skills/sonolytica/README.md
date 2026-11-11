# Installing the SONOLYTICA skill

The skill is this directory: `SKILL.md` plus `references/`. It is plain
Markdown; any agent that can read a skill directory and run `sonolytica` can
use it.

## 1. Build the binary

```bash
cargo build --release -p sonolytica-cli --features full-metal   # macOS/Metal
# or: --features full                                          # CPU
```

Put `target/release/sonolytica` on `PATH` (or symlink it). The skill invokes
`sonolytica` by name, so a shell-capable agent must find it there.

## 2. Install the skill where your agent looks

| Runtime | Directory |
|---|---|
| OpenCode, Codex, Gemini CLI | `~/.agents/skills/sonolytica` |
| Claude Code | `~/.claude/skills/sonolytica` |

```bash
mkdir -p ~/.agents/skills
cp -R skills/sonolytica ~/.agents/skills/
```

Symlinks work too and survive `git pull`. The directory name should stay
`sonolytica`: it matches the `name` in the frontmatter.

## 3. Fetch models and check

```bash
sonolytica doctor
sonolytica models fetch silero-vad-v5 whisper-base.en \
  pyannote-segmentation-3.0 wespeaker-campplus
```

Models live in `~/.cache/sonolytica/models`; override with `--models-dir` or
`SONOLYTICA_MODEL_DIR`. FFmpeg 7+ must be on `PATH`.

## 4. Smoke test

```bash
sonolytica analyze any-media-file --out /tmp/doc.json --summary
sonolytica query runs /tmp/doc.json
sonolytica query transcript /tmp/doc.json
```

If `doctor` reports `build: minimal (DSP only)` or lists providers as
unavailable, the binary was built without features — the skill will refuse to
invent a transcript, but it cannot transcribe until the full build is in place.

## Updating

The skill tracks the CLI it ships with. Update both together; `SKILL.md` is
tested against the binary in CI (`crates/cli/tests/skill_docs.rs` checks that
every documented command exists).
