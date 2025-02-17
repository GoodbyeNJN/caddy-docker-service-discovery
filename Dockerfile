FROM rust:alpine AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock /app/
COPY /src/ /app/src/

RUN apk add musl-dev libressl-dev \
    && rustup target add x86_64-unknown-linux-musl \
    && cargo build --release --target=x86_64-unknown-linux-musl


FROM alpine AS runner

COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/caddy-docker-service-discovery /bin/server

ENTRYPOINT ["/bin/server"]
