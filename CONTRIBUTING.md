# Contributing

## Ground rules

1. **No fabricated data.** Missing values are `null`. Do not fill gaps with
   zeros, empty strings, or plausible text.
2. **No accuracy claims without a run.** If you add a metric, add the harness
   and report the dataset.
3. **The model-free build must keep working.** Core crates cannot depend on a
   model runtime; providers stay behind features.
4. **The document is the API.** Schema changes need a version bump and an entry
   in `docs/SCHEMA.md`.
5. **No new dependency without a reason and a license check** (`deny.toml`).

## Local checks

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --features full -- -D warnings
cargo test --workspace

# Full pipeline, real models (needs cmake + models + macOS `say` for fixtures)
SONOLYTICA_E2E=1 cargo test -p sonolytica-engine --features "full,metal" \
  --test e2e_speech -- --nocapture
```

## Adding a provider

- Implement the trait from `crates/providers/src/traits.rs`.
- Put the native runtime behind a feature; do not add it to the default set.
- Register it in `Registry::discover` only when its artifacts verify.
- Add a `ModelSpec` with a real SHA-256 and license.
- Return `ModelMissing` with the exact `models fetch` command in the hint.
- Document its limits in `docs/MODELS.md` and add a functional test.

## Adding an analysis capability

- Add a `Capability` variant and, if needed, a typed `AnalysisBlock`.
- Record a `RunRecord` before adding segments, so provenance resolves.
- Extend `Document::validate` with any new invariant.
- Update `docs/SCHEMA.md` and the skill's rule list.

## Commit style

Imperative subject, one concern per commit. Include in the body what was
verified (`cargo test`, a real file, a metric run) and what was not.
