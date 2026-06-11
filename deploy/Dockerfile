FROM rust:alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /usr/src/reverse-proxy
COPY . .

RUN cargo build --release --target x86_64-unknown-linux-musl

FROM alpine:latest

RUN apk add --no-cache ca-certificates

COPY --from=builder /usr/src/reverse-proxy/target/x86_64-unknown-linux-musl/release/reverse-proxy /usr/local/bin/reverse-proxy

HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
  CMD wget -q --spider http://127.0.0.1:9900/health || exit 1

EXPOSE 80 443

ENTRYPOINT ["reverse-proxy"]
CMD ["--config", "/etc/reverse-proxy/config.toml"]