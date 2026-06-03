# syntax=docker/dockerfile:1

FROM node:24-alpine AS builder

WORKDIR /app

RUN corepack enable

COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile

COPY . .
RUN pnpm exec vite build

FROM node:24-alpine

RUN apk add --no-cache caddy docker-cli

WORKDIR /app

COPY deploy/Caddyfile /etc/caddy/Caddyfile
COPY deploy/entrypoint.sh /entrypoint.sh
COPY deploy/supervisor.mjs /app/deploy/supervisor.mjs
COPY server/memory-profile.mjs /app/server/memory-profile.mjs
COPY --from=builder /app/dist /srv

RUN chmod +x /entrypoint.sh

ENV ADMIN_LISTEN=:8080 \
    BEAVA_ADMIN_UPSTREAM=http://beava:8090 \
    BEAVA_DATA_UPSTREAM=http://beava:8080 \
    BEAVA_MEMORY_PROFILE_UPSTREAM=http://127.0.0.1:8091 \
    BEAVA_MEMORY_PROFILE_PORT=8091 \
    BEAVA_CONTAINER_NAME=beava-admin-beava

EXPOSE 8080

ENTRYPOINT ["/entrypoint.sh"]
