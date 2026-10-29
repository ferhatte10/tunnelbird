# tunnelbird.ferhatsn.fr: the static site, served by an unprivileged nginx.
# Built from the repository root, because the site copies the app's icon,
# screenshots, install.sh and release notes at build time:
#
#   docker build -f website/Dockerfile -t tunnelbird-website .
#
# Base images pinned by digest (update both the tag and the digest together).

FROM oven/bun:1.4.4-debian@sha256:93d5032fe06d163427822dc1598f420363759bb18155c9cc90eb1be05a64a19e AS build
WORKDIR /src
# What scripts/assets.js reads from the app.
COPY data/ data/
COPY build-aux/install.sh build-aux/install.sh
COPY website/package.json website/bun.lock website/
RUN cd website && bun install --frozen-lockfile
COPY website/ website/
RUN cd website && bun run check && bun run build

FROM nginxinc/nginx-unprivileged:1.30-alpine@sha256:f5116ed4606f3eb015dd4a8e28cb9b351476dc0ad4bc6e9f93b19cec130cdf03
# Security fixes released since the base image, and no curl: nothing here
# needs it (Kubernetes checks the site over HTTP itself).
USER root
RUN apk upgrade --no-cache && apk del --no-cache curl
USER 101
COPY website/nginx/default.conf /etc/nginx/conf.d/default.conf
COPY website/nginx/security-headers.conf /etc/nginx/security-headers.conf
COPY --from=build /src/website/build /usr/share/nginx/html
# nginx-unprivileged runs as uid 101 and listens on 8080; only /tmp is written.
EXPOSE 8080
