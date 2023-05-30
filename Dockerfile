FROM rust:1.69-slim-bullseye AS builder
RUN mkdir -p /usr/src/cozo
COPY . /usr/src/cozo
WORKDIR /usr/src/cozo
RUN cargo build --release -p cozo-bin -F compact -F storage-sqlite

FROM debian:bullseye-slim AS cozo
COPY --from=builder /usr/src/cozo/target/release/cozo-bin /usr/local/bin/cozo-bin
ENTRYPOINT /usr/local/bin/cozo-bin server -e sqlite -p /usr/share/cozo.db
