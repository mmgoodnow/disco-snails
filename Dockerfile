FROM oven/bun:1 AS builder

WORKDIR /app

COPY package.json bun.lock ./
RUN bun install --ci

COPY . .
RUN bun build ./index.ts --compile --outfile /tmp/disco-snails

FROM alpine:3.20 AS runner

RUN apk add --no-cache ca-certificates

WORKDIR /config

COPY --from=builder /tmp/disco-snails /usr/local/bin/disco-snails

ENV NODE_ENV=production \
    PORT=3000

EXPOSE 3000

CMD ["/usr/local/bin/disco-snails"]
