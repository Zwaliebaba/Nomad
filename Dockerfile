# Linux container image of nomad-server (D-029, doc 20 §2). The MVP server runs in Docker
# Desktop (D-236); the compose file and process model come with N5 (D-245), and the client's
# files and the WebSocket on TCP 31790 with N6 (D-241, D-243). Until then it listens on UDP
# 31790 on every address inside the container; publish it on the host's loopback only
# (`-p 127.0.0.1:31790:31790/udp`), which keeps the MVP localhost-only (D-150, D-236).
# Usage: README.md, "Run the server in Docker".
#
# Keep the Rust version in step with rust-toolchain.toml.
FROM rust:1.97.0-bookworm AS build

# .git is not in the build context, so build.rs cannot find the commit by itself. Without an
# identity every build would be "unknown", and the check that a journal is replayed only by
# the build that wrote it (D-048) would pass for builds that differ.
ARG NOMAD_BUILD_ID=""
RUN test -n "$NOMAD_BUILD_ID" \
 || { echo 'pass --build-arg NOMAD_BUILD_ID=$(git rev-parse HEAD)' >&2; exit 1; }

WORKDIR /src
COPY . .
# The Debug profile, the only one CI tests (D-010).
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build -p nomad-server --locked \
 && cp target/debug/nomad-server /nomad-server

FROM debian:bookworm-slim
RUN useradd --system --uid 10001 --no-create-home nomad \
 && mkdir /data \
 && chown nomad:nomad /data
COPY --from=build /nomad-server /usr/local/bin/nomad-server
USER nomad
ENV NOMAD_DATA=/data
# All addresses inside the container; the host decides who can reach it (D-150).
ENV NOMAD_BIND=0.0.0.0
EXPOSE 31790/udp
VOLUME /data
# The exec form makes the server PID 1, so `docker stop` reaches it as SIGTERM and it writes
# its final snapshot (doc 20 §7).
ENTRYPOINT ["nomad-server"]
CMD ["run"]
