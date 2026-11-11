## What changed

<!-- One paragraph. Link the issue if there is one. -->

## Evidence

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `cargo test --workspace` (no models)
- [ ] `cargo test --workspace --all-features`
- [ ] docs-as-tests: `cargo test -p sonolytica-cli --test skill_docs`

If this changes perception (ASR, diarization, OCR, music, video), paste the
measured before/after on a licensed or reproducible fixture. If it changes the
skill, paste the paired benchmark from `scripts/bench/`.

## Honest limits

<!-- What does this not cover? Where would it regress? -->
