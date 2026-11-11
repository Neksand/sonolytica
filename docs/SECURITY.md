# Security

Media files are untrusted input. The threat model below is deliberately narrow
and explicit: parsing hostile containers, exhausting resources, and corrupting
provenance.

## Trust boundaries

1. **File path → FFmpeg.** The only place a user-controlled string reaches an
   external process.
2. **Container bytes → decoder.** FFmpeg's own CVE surface; we contain the
   process, not the parser.
3. **Model file → runtime.** ONNX/GGML weights execute native code paths.
4. **Document → consumer.** JSON is plain data; consumers must still treat text
   fields as untrusted.

## Controls

### Path and process handling

- Input paths are canonicalized and must resolve to a regular file
  (`validate_input`), checked for size before any process is spawned.
- FFmpeg is invoked with an **argv array**, never a shell string; filenames are
  not interpolated into a command line.
- `-protocol_whitelist file,pipe` prevents FFmpeg from following `http://`,
  `concat:`, `subfile:` and similar protocols a malicious playlist could use.
- `-nostdin`, `-hide_banner`, `-loglevel error`; stdout is the only data
  channel, stderr is captured (capped at 64 KiB) for diagnostics.
- Every process has a timeout (`limits.timeout_secs`) and `kill_on_drop(true)`,
  so a wedged decoder cannot outlive the request.
- Duration (`-t`) and decoded-sample budgets are enforced twice: by FFmpeg and
  by arithmetic on the returned buffer.
- Non-finite samples are rejected (`MediaError::BadAudio`) instead of being fed
  to DSP.

### Resource limits

Defaults in `AnalysisConfig.limits`: 2 h / 8 GiB / 10 min per external process.
These are configuration, not constants in the code, so a service can tighten
them per tenant. Peak memory is dominated by PCM (4 bytes/sample) and model
weights; there is no unbounded frame buffer.

### Model integrity

- Every model has a pinned SHA-256. Downloads go to `<file>.partial`, are
  hashed while streaming, and are renamed only on a digest match.
- An unpinned entry (empty digest) is refused rather than trusted.
- A model whose digest mismatches is reported as missing with the remediation
  command; it is never loaded.

### Output integrity

- Documents are validated before emission: ids, references, hierarchy,
  confidence ranges and media bounds. A violating document is an error, not a
  warning.
- Missing measurements are `null`; no sentinel values.
- Text from ASR is exactly what the model produced (trimmed), never summarised
  or paraphrased by the engine.

## Non-goals / known exposure

- **FFmpeg parsing bugs.** A malicious file may crash or exploit FFmpeg. For
  multi-tenant deployments, run the worker in a sandboxed process/container with
  no network and a read-only filesystem. SONOLYTICA does not claim to sandbox
  FFmpeg itself.
- **Model supply chain beyond the digest.** We pin bytes, not training data
  provenance. Model licenses and sources are documented in `docs/MODELS.md`.
- **Adversarial audio.** Speaker identification, emotion and intent are not
  authenticated; they must not be used as identity or safety decisions.
- **Prompt injection through transcripts.** Transcribed content is data. An
  agent consuming a document must not treat quoted speech as instructions.
- **Denial of service via output size.** A dense recording can produce large
  JSON. Consumers should apply their own size limits; the engine's limits bound
  input, not output.

## Reporting

Security issues should be reported privately to the maintainers before public
disclosure. Include the media file or a minimal reproducer, the exact command,
and the toolchain versions from `sonolytica doctor`.
