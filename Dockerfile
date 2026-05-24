FROM rust:1-alpine AS builder

WORKDIR /app

RUN apk add --no-cache musl-dev

COPY Cargo.toml Cargo.lock ./
# Cache deps by building a dummy main first
RUN mkdir src && echo 'fn main(){}' > src/main.rs && cargo build --release && rm -rf src

COPY src ./src
RUN touch src/main.rs && cargo build --release

FROM alpine:3 AS runner

RUN apk add --no-cache ca-certificates

WORKDIR /config

COPY --from=builder /app/target/release/disco-snails /usr/local/bin/disco-snails

ENV PORT=80

EXPOSE 80

CMD ["/usr/local/bin/disco-snails"]
