# CI image: Rust runs here, on the in-cluster BuildKit, because the ARC runner has no toolchain.
# The crate has no dependencies and gif-rs has no private git dependency, so no step needs a token
# and no secret reaches the build. Two other crates have dependencies from crates.io: the fuzz
# crate (libfuzzer-sys) and tests/parity/imagers (the image-rs gif crate). Each is fetched in its
# own layer before `COPY . .`, then every later step runs offline.
# Every tool is installed in a stage before `COPY . .`, so a new commit does not install it again,
# and the fetch runs from the manifests and lockfiles alone.
FROM rust:1.99.1-bookworm AS toolchain
# gcc-aarch64-linux-gnu: the linker of `cargo build --target aarch64-unknown-linux-gnu`.
RUN rustup component add rustfmt clippy && rustup target add aarch64-unknown-linux-gnu \
    && apt-get update && apt-get install -y --no-install-recommends gcc-aarch64-linux-gnu libc6-dev-arm64-cross \
    && rm -rf /var/lib/apt/lists/*
ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
WORKDIR /src
# The manifests and lockfiles of the crate and of the fuzz crate, with stub targets, so the fetch
# is a layer that only a change to them rebuilds. The crate has no dependencies: only the fuzz
# crate has something to fetch.
COPY Cargo.toml Cargo.lock ./
COPY tests/fuzz/Cargo.* tests/fuzz/
RUN mkdir -p src tests/fuzz/fuzz_targets && touch src/lib.rs \
    && for t in decode decoder_reuse read_info; do touch tests/fuzz/fuzz_targets/$t.rs; done \
    && L=; [ -f tests/fuzz/Cargo.lock ] && L=--locked; cargo fetch $L --manifest-path tests/fuzz/Cargo.toml \
    && rm -rf src tests/fuzz/fuzz_targets

FROM toolchain AS src
ENV CARGO_NET_OFFLINE=true
COPY . .

# The checks run in the build, so a red check fails the build; a rerun of the same source is a
# BuildKit cache hit. The target dir is a cache mount, so it persists across builds. The target
# cache is shared by every branch and cargo compares mtimes: another branch's newer build would be
# reused as this one's, so every RUN touches the .rs files itself, just before cargo starts.
FROM src AS ci
RUN --mount=type=cache,id=gif-rs-target-ci,sharing=locked,target=/src/target \
    find src examples tests -name '*.rs' -exec touch {} + && cargo fmt --check
RUN --mount=type=cache,id=gif-rs-target-ci,sharing=locked,target=/src/target \
    find src examples tests -name '*.rs' -exec touch {} + && cargo clippy --locked --all-targets -- -D warnings
# Lint the aarch64 target too: the same code, but a cfg(target_arch) would show here.
RUN --mount=type=cache,id=gif-rs-target-ci,sharing=locked,target=/src/target \
    find src examples tests -name '*.rs' -exec touch {} + && cargo clippy --locked --all-targets --target aarch64-unknown-linux-gnu -- -D warnings
RUN --mount=type=cache,id=gif-rs-target-ci,sharing=locked,target=/src/target \
    find src examples tests -name '*.rs' -exec touch {} + && cargo test --locked --no-fail-fast
# The examples are built too, so the cross linker runs (a lib alone is not linked).
RUN --mount=type=cache,id=gif-rs-target-ci,sharing=locked,target=/src/target \
    find src examples tests -name '*.rs' -exec touch {} + && cargo build --locked --release --target aarch64-unknown-linux-gnu --examples

# qemu-user, for the arm64 stages (the cross gcc is in `toolchain`).
FROM toolchain AS qemu-tools
RUN apt-get update && apt-get install -y --no-install-recommends qemu-user \
    && rm -rf /var/lib/apt/lists/*
ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUNNER="qemu-aarch64 -L /usr/aarch64-linux-gnu" \
    CARGO_NET_OFFLINE=true

FROM qemu-tools AS qemu-arm64
COPY . .

# aarch64 tests under qemu-user. gif-rs has no SIMD: the code is the same on both architectures.
# The suite is small, so this stage runs the whole of it that needs no big input: the unit tests
# (--lib) and the decode tests (--test decode). It checks the integer width and byte order
# assumptions of the LZW port on a second architecture.
FROM qemu-arm64 AS arm64
RUN --mount=type=cache,id=gif-rs-target-arm64,sharing=locked,target=/src/target \
    find src examples tests -name '*.rs' -exec touch {} + \
    && cargo test --locked --target aarch64-unknown-linux-gnu --lib \
    && cargo test --locked --target aarch64-unknown-linux-gnu --test decode

# giflib 5.2.1 oracle (CI only, never linked into the crate), static so no giflib runtime is needed
# in the stages that run it. Copies tests/parity/c only, so a crate change does not rebuild it.
FROM debian:bookworm AS c-oracle
RUN apt-get update && apt-get install -y --no-install-recommends gcc libc6-dev libgif-dev \
    && rm -rf /var/lib/apt/lists/*
COPY tests/parity/c /c
RUN gcc -O2 -Wall -static -o /usr/local/bin/giforacle /c/giforacle.c -lgif

# The image-rs gif crate (MIT OR Apache-2.0, from crates.io) as a second reference: information
# only, never a gate. Its own fetch layer, then an offline build. Copies tests/parity/imagers
# only. The lockfile is optional (`Cargo.*` matches the manifest alone): with a Cargo.lock in the
# directory the fetch and the build use --locked.
FROM rust:1.99.1-bookworm AS imagers-oracle
WORKDIR /imagers
COPY tests/parity/imagers/Cargo.* ./
RUN L=; [ -f Cargo.lock ] && L=--locked; mkdir src && echo 'fn main() {}' > src/main.rs \
    && cargo fetch $L && rm -rf src
COPY tests/parity/imagers/src src
RUN --mount=type=cache,id=gif-rs-target-imagers,sharing=locked,target=/imagers/target \
    L=; [ -f Cargo.lock ] && L=--locked; find src -name '*.rs' -exec touch {} + \
    && CARGO_NET_OFFLINE=true cargo build $L --release \
    && cp target/release/imagers-oracle /usr/local/bin/imagers-oracle

# The public corpus (tests/parity/corpus.toml: sizes and SHA-256 are checked) in a cache mount, so
# a build downloads it once. sharing=locked: two builds never download at once. Only a list of the
# .gif files reaches the layer; the stages that read the corpus mount the same cache and depend on
# this stage through that list. An entry with an empty sha256 fails this stage (exit 3) and the
# log shows the PIN line to write into corpus.toml.
FROM debian:bookworm-slim AS data
RUN apt-get update && apt-get install -y --no-install-recommends curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY tests/parity/fetch.sh tests/parity/corpus.toml /parity/
RUN --mount=type=cache,id=gif-rs-data,target=/data,sharing=locked \
    sh /parity/fetch.sh /parity/corpus.toml /data \
    && (cd /data/x && find . -type f -name '*.gif' | sort) > /data.list && wc -l /data.list

# The mutations of the corpus (giforacle -gen: truncations, bit flips, length-field edits,
# sub-block length edits, LZW code size edits; 40 per seed file, deterministic). Made once here
# and copied into the parity stage, so the same files feed every oracle.
FROM debian:bookworm-slim AS gen
WORKDIR /src
COPY tests/parity/run.sh tests/parity/
COPY --from=c-oracle /usr/local/bin/giforacle /usr/local/bin/giforacle
COPY --from=data /data.list /data.list
RUN --mount=type=cache,id=gif-rs-data,target=/data,sharing=locked,readonly \
    mkdir -p /gen && sh tests/parity/run.sh gen /gen && echo "$(ls /gen | wc -l) mutations"

# The reference tree of gif-rs on a subset of the corpus (one thread), x86_64: what the aarch64
# oracle must reproduce byte for byte.
FROM src AS x86-ref
COPY --from=data /data.list /data.list
RUN --mount=type=cache,id=gif-rs-target-parity,sharing=locked,target=/src/target \
    find src examples tests -name '*.rs' -exec touch {} + \
    && cargo build --locked --release --example oracle && cp target/release/examples/oracle /usr/local/bin/gif-oracle
RUN --mount=type=cache,id=gif-rs-data,target=/data,sharing=locked,readonly \
    sh tests/parity/run.sh ref /ref

# Parity gates (tests/parity/run.sh full): giflib vs gif-rs on the corpus and the mutations, one
# thread vs default threads, and the image-rs report (information only). The stage does not fail
# on a diff: the report (summary.md, *.diff, imagers.txt, status) is the build output (target
# parity-report), and the workflow fails on `status`, as the report is wanted most when the gate
# is red.
FROM src AS parity
COPY --from=c-oracle /usr/local/bin/giforacle /usr/local/bin/giforacle
COPY --from=imagers-oracle /usr/local/bin/imagers-oracle /usr/local/bin/imagers-oracle
COPY --from=gen /gen /gen
COPY --from=data /data.list /data.list
RUN --mount=type=cache,id=gif-rs-target-parity,sharing=locked,target=/src/target \
    find src examples tests -name '*.rs' -exec touch {} + \
    && cargo build --locked --release --example oracle && cp target/release/examples/oracle /usr/local/bin/gif-oracle
RUN --mount=type=cache,id=gif-rs-data,target=/data,sharing=locked,readonly \
    mkdir -p /report && { sh tests/parity/run.sh full /report; echo $? > /report/status; }

FROM scratch AS parity-report
COPY --from=parity /report /

# The oracle built for aarch64, run under qemu-user on the same subset, compared with the x86_64
# tree above.
FROM qemu-arm64 AS parity-arm64
COPY --from=data /data.list /data.list
RUN --mount=type=cache,id=gif-rs-target-arm64,sharing=locked,target=/src/target \
    find src examples tests -name '*.rs' -exec touch {} + \
    && cargo build --locked --release --target aarch64-unknown-linux-gnu --example oracle \
    && cp target/aarch64-unknown-linux-gnu/release/examples/oracle /usr/local/bin/gif-oracle-arm64
RUN --mount=type=bind,from=x86-ref,source=/ref,target=/ref,rw \
    --mount=type=cache,id=gif-rs-data,target=/data,sharing=locked,readonly \
    sh tests/parity/run.sh arm /ref

# Fuzzing (fuzz.yml): nightly and cargo-fuzz, with the targets built into the image. The workflow
# runs it with `docker create` + `docker start`, so a BuildKit cache hit cannot skip the fuzzing.
FROM toolchain AS fuzz-tools
RUN rustup toolchain install nightly --profile minimal \
    && cargo install cargo-fuzz --locked --version 0.13.1
ENV CARGO_NET_OFFLINE=true

# Seeds are the corpus (tests/data and the public sets), copied here, not kept twice in git. The
# targets read one option byte at the end, so each seed gets a 0x00 byte.
FROM fuzz-tools AS fuzz
COPY . .
COPY --from=data /data.list /data.list
RUN --mount=type=cache,id=gif-rs-data,target=/data,sharing=locked,readonly \
    sh tests/parity/run.sh gather tests/fuzz/seeds tests/data . \
    && sh tests/parity/run.sh gather tests/fuzz/seeds /data/x . \
    && for f in tests/fuzz/seeds/*; do printf '\0' >> "$f"; done \
    && ls tests/fuzz/seeds | wc -l \
    && cargo +nightly fuzz build --fuzz-dir tests/fuzz

# Benchmark (bench.yml): giflib (C oracle, bench mode) and examples/bench on the same corpus, in
# one container, so the ratio compares the same machine. BENCH_RUN changes per run, so a cache hit
# of the same source does not reuse an old timing.
FROM src AS bench
COPY --from=c-oracle /usr/local/bin/giforacle /usr/local/bin/giforacle
COPY --from=data /data.list /data.list
RUN --mount=type=cache,id=gif-rs-target-bench,sharing=locked,target=/src/target \
    find src examples tests -name '*.rs' -exec touch {} + \
    && cargo build --locked --release --example bench && cp target/release/examples/bench /usr/local/bin/gif-bench
ARG BENCH_RUN=0
RUN --mount=type=cache,id=gif-rs-data,target=/data,sharing=locked,readonly \
    echo "run $BENCH_RUN" && mkdir -p /report && { sh tests/parity/bench.sh /report; echo $? > /report/status; }

FROM scratch AS bench-report
COPY --from=bench /report /
