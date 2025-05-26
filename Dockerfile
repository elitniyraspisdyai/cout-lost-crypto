# 1) Stage: build Rust binary
FROM rust:1.87 as builder
WORKDIR /usr/src/seed-brute
# Копируем только манифесты для кэширования
COPY seed-brute/Cargo.toml seed-brute/Cargo.lock ./
COPY seed-brute/src ./src
RUN cargo build --release

# 2) Final image
FROM python:3.12-slim
WORKDIR /app
COPY --from=builder /usr/src/seed-brute/target/release/seed-brute ./seed-brute
COPY rpc_checker.py ./
RUN pip install --no-cache-dir bip-utils

EXPOSE 9184

CMD ["./seed-brute", "--count", "100000", "--threads", "1", "--destination", "2N3oDZkxtZ3Hn2X7gU6n1aYxQn6fWjL4vYs", "--timeout", "30"]
