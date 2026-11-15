FROM rust:1-alpine AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src src
RUN cargo build --release --locked

FROM alpine
RUN apk add --no-cache iptables
COPY --from=build /src/target/release/tcp-kernel-slowdown /src/target/release/probe /usr/local/bin/
ENTRYPOINT ["tcp-kernel-slowdown"]
