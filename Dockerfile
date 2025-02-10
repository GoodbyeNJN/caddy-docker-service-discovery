FROM rust AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock /app/
COPY /src/ /app/src/

RUN rustup target add x86_64-unknown-linux-musl
RUN cargo build --release --target=x86_64-unknown-linux-musl


FROM alpine AS runner

ENV RUST_LOG=info

COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/docker-dns-server /bin/dns-server

ENTRYPOINT ["/bin/dns-server"]
