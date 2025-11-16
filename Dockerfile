FROM oven/bun:1 AS base

WORKDIR /config

# Copy dependency manifests separately for better caching
COPY package.json bun.lock ./

RUN bun install --ci --production

# Copy application source
COPY . .

ENV NODE_ENV=production \
    PORT=3000

EXPOSE 3000

CMD ["bun", "run", "index.ts"]
