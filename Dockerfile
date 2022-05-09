FROM docker.io/debian:stable-slim AS builder

RUN apt-get update && apt-get install curl openssl libssl-dev make gcc pkg-config -y
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --default-host x86_64-unknown-linux-gnu
ENV PATH="/root/.cargo/bin:${PATH}"
RUN cargo search libc

COPY . /build

WORKDIR /build
RUN cargo build --release --bin server

FROM docker.io/debian:stable-slim
RUN apt-get -y update && apt-get -y install ca-certificates openssl

WORKDIR /app/
COPY --from=builder /build/target/release/server /app/

EXPOSE 4050
VOLUME /app/data

ENTRYPOINT ["/app/server", "--config", "/app/data/config.toml"]
