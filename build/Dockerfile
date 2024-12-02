FROM rust:1.76-slim as builder

WORKDIR /usr/src/app
COPY . .

RUN cargo build --release

FROM debian:bookworm-slim

WORKDIR /usr/local/bin

COPY --from=builder /usr/src/app/target/release/url-shortener .
COPY --from=builder /usr/src/app/.env .
COPY --from=builder /usr/src/app/src/templates ./templates

RUN apt-get update && apt-get install -y libsqlite3-0 && rm -rf /var/lib/apt/lists/*

ENV RUST_LOG=info

EXPOSE 3000

CMD ["./url-shortener"]
