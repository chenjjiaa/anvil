# Copyright 2025 chenjjiaa
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

# ────────────────────────────────
# Basic image
# ────────────────────────────────
FROM lukemathwalker/cargo-chef:0.1.73-rust-1.92.0-alpine AS chef
WORKDIR /app

# ────────────────────────────────
# Planner stage
# ────────────────────────────────
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo chef prepare --recipe-path recipe.json

# ────────────────────────────────
# Builder stage
# ────────────────────────────────
FROM chef AS builder 
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json --locked
COPY . .
RUN cargo build --release --package cmd

# ────────────────────────────────
# Runtime stage
# ────────────────────────────────
FROM alpine:3.23 AS runtime

# Install CA certificates for HTTPS
RUN apk add --no-cache ca-certificates

# Create non-root user
RUN addgroup -g 1000 appuser && \
    adduser -D -u 1000 -G appuser appuser

WORKDIR /app
COPY --from=builder /app/target/release/cmd /app/cmd

# Set ownership
RUN chown -R appuser:appuser /app
# Switch to non-root user
USER appuser

# TODO:
# Expose port
# Other configurations

ENTRYPOINT ["/app/cmd"]
