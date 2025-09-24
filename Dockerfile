
# Dockerfile
FROM rust:1-bookworm AS builder

# Install FFmpeg development libraries
RUN apt-get update && apt-get install -y \
    pkg-config \
    libavcodec-dev \
    libavformat-dev \
    libavutil-dev \
    libavdevice-dev \
    libavfilter-dev \
    libswscale-dev \
    libswresample-dev \
    clang \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    libavcodec59 \
    libavformat59 \
    libavutil57 \
    libavdevice59 \
    libavfilter8 \
    libswscale6 \
    libswresample4 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/video-stream-recorder .

# Create volume for recordings
VOLUME ["/app/recordings"]

# Expose metrics port
EXPOSE 8080

CMD ["./video-stream-recorder"]

