# Copyright © 2026 Akira Miyakoda
#
# This software is released under the MIT License.
# https://opensource.org/licenses/MIT

# ==============================================================================

FROM rust:1-trixie AS build

ENV CARGO_TARGET_DIR=/tmp/target
ENV RUSTFLAGS="-C target-cpu=x86-64-v4"

WORKDIR /tmp/project

RUN mkdir -p ./src

COPY ./Cargo.* .
RUN echo "fn main() {}" > ./src/main.rs
RUN cargo build --release

COPY . .
RUN touch ./src/main.rs
RUN cargo build --release

# ==============================================================================

FROM gcr.io/distroless/cc-debian13

COPY --from=build --chmod=755 ./tmp/target/release/wallclock-core /usr/bin

ENTRYPOINT [ "wallclock-core" ]
