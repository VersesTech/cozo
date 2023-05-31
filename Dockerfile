FROM rust:1.69-slim-bullseye AS builder
RUN mkdir -p /usr/src/cozo
COPY . /usr/src/cozo
WORKDIR /usr/src/cozo
RUN cargo build --release -p cozo-bin -F compact -F storage-sqlite

FROM debian:bullseye-slim AS cozo
RUN apt-get update && apt-get -y install ca-certificates sqlite3
COPY --from=builder /usr/src/cozo/target/release/cozo-bin /usr/local/bin/cozo-bin
RUN chmod +x /usr/local/bin/cozo-bin
