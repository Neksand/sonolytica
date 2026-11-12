# SONOLYTICA runtime image.
#
#   docker build -t sonolytica .
#   docker run --rm -v sonolytica-data:/data sonolytica \
#     setup --no-skill
#   docker run --rm -v sonolytica-data:/data -v "$PWD:/media" sonolytica \
#     analyze /media/talk.mp4 --out /media/talk.json --ocr --summary
#
# Models and the analysis cache live in the /data volume; nothing is baked
# into the image. FFmpeg is a subprocess, not a linked library.

FROM rust:1.99-bookworm AS build

RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake libasound2-dev pkg-config \
        clang libclang-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY skills ./skills
RUN cargo build --release -p sonolytica-cli --features full \
    && cargo build --release -p sonolytica-eval

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ffmpeg ca-certificates \
        libgomp1 libstdc++6 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /src/target/release/sonolytica /usr/local/bin/sonolytica
COPY --from=build /src/target/release/sonolytica-eval /usr/local/bin/sonolytica-eval

ENV SONOLYTICA_MODEL_DIR=/data/models \
    SONOLYTICA_CACHE_DIR=/data/cache

VOLUME ["/data"]

ENTRYPOINT ["sonolytica"]
CMD ["doctor"]
