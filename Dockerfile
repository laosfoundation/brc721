FROM rust:latest as builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release

FROM debian:bookworm-slim

RUN useradd -m appuser
WORKDIR /app
RUN chown appuser:appuser /app

COPY --from=builder /app/target/release/brc721 /usr/local/bin/brc721

USER appuser

ENV RUST_LOG=info

ENTRYPOINT ["brc721"]
CMD []
