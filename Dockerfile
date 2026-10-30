FROM rust:1-alpine AS builder

WORKDIR /app

RUN apk add --no-cache musl-dev

COPY Cargo.toml Cargo.lock ./
# Cache dependencies by building both entry points with dummy mains first.
RUN mkdir -p src/bin \
    && echo 'fn main(){}' > src/main.rs \
    && echo 'fn main(){}' > src/bin/disco.rs \
    && cargo build --release --bins \
    && rm -rf src

COPY src ./src
RUN touch src/main.rs src/bin/disco.rs src/lib.rs && cargo build --release --bins

FROM alpine:3 AS runner

RUN apk add --no-cache ca-certificates

WORKDIR /config

COPY --from=builder /app/target/release/disco /usr/local/bin/disco
COPY --from=builder /app/target/release/disco-snails /usr/local/bin/disco-snails

ENV PORT=80

EXPOSE 80

CMD ["/usr/local/bin/disco", "serve"]
