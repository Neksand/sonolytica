# Docker

The image contains the CLI, `sonolytica-eval` and FFmpeg. Models and the
analysis cache live in a named volume, not in the image; nothing is uploaded.

## Build

```bash
docker build -t sonolytica .
# or pull the published image after a release
docker pull ghcr.io/neksand/sonolytica:latest
```

The published image is `linux/amd64`. On Apple Silicon or an arm64 server,
build locally with `docker build -t sonolytica .`; the Dockerfile compiles for
the host architecture. Verified on an M5 Pro (arm64, Debian trixie): the image
is **790 MB**, and `doctor`, `analyze` with models mounted read-only,
`query brief` and `tools/call` over MCP all pass.

## First run

```bash
docker volume create sonolytica-data

# core models (~1.5 GB); no skill is installed inside the container
docker run --rm -v sonolytica-data:/data sonolytica setup --no-skill

# or every model, including OCR/faces/objects/pose/Florence
docker run --rm -v sonolytica-data:/data sonolytica setup --no-skill --full
```

## Analyze

Mount the media directory and write the document beside it:

```bash
docker run --rm \
  -v sonolytica-data:/data \
  -v "$PWD:/media" \
  sonolytica analyze /media/talk.mp4 \
    --out /media/talk.json --profile standard --ocr --summary

docker run --rm -v sonolytica-data:/data -v "$PWD:/media" \
  sonolytica query brief /media/talk.json
```

To reuse models already downloaded on the host, mount them read-only instead of
running `setup`:

```bash
docker run --rm \
  -v "$HOME/.cache/sonolytica/models:/data/models:ro" \
  -v "$PWD:/media" \
  sonolytica analyze /media/talk.mp4 --out /media/talk.json --ocr --summary
```

The `sonolytica-data` volume holds `models/` and `cache/`. The cache is keyed
by media SHA-256 and config, so repeated analyses of the same bytes are cheap.
`sonolytica cache --clear` removes it.

## MCP from a container

MCP is stdio, so keep stdin open and pass the media directory as a volume:

```bash
docker run -i --rm \
  -v sonolytica-data:/data \
  -v "$PWD:/media" \
  sonolytica mcp
```

The client configuration is the same as in [`MCP.md`](MCP.md), with
`"command": "docker"` and
`"args": ["run","-i","--rm","-v","sonolytica-data:/data","-v","$PWD:/media","sonolytica","mcp"]`.

## Compose

```yaml
services:
  sonolytica:
    image: ghcr.io/neksand/sonolytica:latest
    volumes:
      - sonolytica-data:/data
      - ./media:/media:ro
    command: query brief /media/talk.json
volumes:
  sonolytica-data:
```

## Notes

- **No models in the image.** `setup` downloads and SHA-verifies them into the
  volume. A model digest mismatch is rejected.
- **FFmpeg is a subprocess**, not a linked library. Debian's package license is
  its own; the project's no-GPL rule applies to linked Rust dependencies.
- **Architecture.** The release workflow publishes `linux/amd64`; arm64 builds
  from the same Dockerfile.
- **Permissions.** The container runs as root by default. For a shared host,
  add `--user "$(id -u):$(id -g)"` and make the mounted directories writable.
- **Large media** stays on the host; only the document and keyframes are
  written to `/media` (or another mounted directory).
