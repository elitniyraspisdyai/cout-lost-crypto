# 1) Stage: build Rust binary
FROM rust:1.87 AS builder
WORKDIR /usr/src/seed-brute

COPY seed-brute/Cargo.toml seed-brute/Cargo.lock ./
COPY seed-brute/src ./src
RUN cargo build --release

# 2) Final image
FROM python:3.12-slim
WORKDIR /app

# Установим системные пакеты для сборки cryptography и других зависимостей
RUN apt-get update && apt-get install -y \
    build-essential \
    libssl-dev \
    pkg-config \
    libffi-dev \
    cargo \
  && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/src/seed-brute/target/release/seed-brute ./seed-brute
COPY rpc_checker.py ./

# Обновим pip и установим bip-utils
RUN pip install --upgrade pip setuptools wheel \
 && pip install bip-utils

EXPOSE 9184

CMD ["./seed-brute", "--count", "100000", "--threads", "1", "--destination", "2N3oDZkxtZ3Hn2X7gU6n1aYxQn6fWjL4vYs", "--timeout", "30"]
