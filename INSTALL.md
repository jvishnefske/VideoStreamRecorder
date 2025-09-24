# 📚 Installation Guide

Complete installation and configuration guide for Video Stream Recorder.

## 📋 Prerequisites

### System Requirements

- **Linux/macOS/Windows** (Linux recommended for production)
- **4GB RAM minimum** (8GB+ recommended for multiple streams)
- **Storage space** for recordings (depends on stream bitrate and retention)
- **Network connectivity** to stream sources

### Dependencies

- **Rust 1.70+** (for building from source)
- **FFmpeg 4.4+** (automatically handled in Docker)
- **OpenSSL** development libraries

## 🔧 Installation Methods

### Method 1: Docker (Recommended)

```bash
#  build locally
git clone https://github.com/user/video-stream-recorder.git
cd video-stream-recorder
docker build -t video-stream-recorder .
```


### Method 2: Build from Source
Building requires ffmpeg development library
```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Install system dependencies
# Ubuntu/Debian:
sudo apt update
sudo apt install build-essential pkg-config libssl-dev libavformat-dev libavcodec-dev libavutil-dev

# CentOS/RHEL:
sudo yum groupinstall "Development Tools"
sudo yum install openssl-devel ffmpeg-devel

# macOS:
brew install ffmpeg pkg-config openssl

# Clone and build
git clone https://github.com/user/video-stream-recorder.git
cd video-stream-recorder
cargo build --release

# Binary will be at target/release/video-stream-recorder
sudo cp target/release/video-stream-recorder /usr/local/bin/
```

### Method 3: Docker (Production Ready)

## ⚙️ Configuration

### Environment Variables

#### Stream Configuration

| Variable | Description | Default | Example |
|----------|-------------|---------|---------|
| `STREAM_URL` | Single stream URL (legacy) | - | `https://camera.com/stream` |
| `STREAM_IDS` | Comma-separated stream IDs | - | `cam1,cam2,cam3` |
| `STREAM_URL_{ID}` | URL for specific stream | - | `STREAM_URL_CAM1=https://...` |
| `STREAM_ENABLED_{ID}` | Enable specific stream | `true` | `STREAM_ENABLED_CAM1=false` |
| `STREAM_SUBDIR_{ID}` | Custom subdirectory | `{stream_id}` | `STREAM_SUBDIR_CAM1=front_door` |

#### Recording Settings

| Variable | Description | Default | Example |
|----------|-------------|---------|---------|
| `OUTPUT_DIR` | Recording directory | `./recordings` | `/data/recordings` |
| `SEGMENT_DURATION` | Segment length (seconds) | `10` | `30` |
| `MAX_DISK_USAGE_PERCENT` | Cleanup threshold | `90.0` | `85.0` |
| `CLEANUP_CHECK_INTERVAL` | Check interval (seconds) | `60` | `300` |
| `AUTO_START` | Auto-start recording | `false` | `true` |

#### Network & Server

| Variable | Description | Default | Example |
|----------|-------------|---------|---------|
| `HOST` | Server bind address | `0.0.0.0` | `127.0.0.1` |
| `PORT` | Server port | `8080` | `3000` |

#### Retry Configuration

| Variable | Description | Default | Example |
|----------|-------------|---------|---------|
| `MAX_RETRIES` | Max retry attempts (0=infinite) | `0` | `10` |
| `RETRY_DELAY` | Max retry delay (seconds) | `5` | `30` |

### Configuration File

Create `config.json` for complex setups:

```json
{
  "streams": [
    {
      "id": "front_door",
      "url": "https://camera1.example.com/stream",
      "enabled": true,
      "output_subdir": "front_camera"
    },
    {
      "id": "back_yard",
      "url": "https://camera2.example.com/stream",
      "enabled": true,
      "output_subdir": "back_camera"
    },
    {
      "id": "garage",
      "url": "https://camera3.example.com/stream",
      "enabled": false
    }
  ],
  "output_dir": "/data/recordings",
  "segment_duration": 30,
  "max_disk_usage_percent": 85.0,
  "cleanup_check_interval": 300,
  "server_port": 8080,
  "server_host": "0.0.0.0",
  "auto_start": true,
  "max_retries": 0,
  "retry_delay": 10
}
```

Run with config file:
```bash
video-stream-recorder -c config.json
```

## 🐳 Docker Deployment

### Single Stream
```bash
docker run -d \
  --name video-recorder \
  --restart unless-stopped \
  -e STREAM_URL="https://your-camera.com/stream" \
  -e AUTO_START="true" \
  -v /host/recordings:/recordings \
  -p 8080:8080 \
  video-stream-recorder
```

### Multiple Streams
```bash
docker run -d \
  --name video-recorder \
  --restart unless-stopped \
  -e STREAM_IDS="front,back,side" \
  -e STREAM_URL_FRONT="https://cam1.example.com/stream" \
  -e STREAM_URL_BACK="https://cam2.example.com/stream" \
  -e STREAM_URL_SIDE="https://cam3.example.com/stream" \
  -e AUTO_START="true" \
  -e OUTPUT_DIR="/recordings" \
  -e MAX_DISK_USAGE_PERCENT="80" \
  -v /host/recordings:/recordings \
  -p 8080:8080 \
  video-stream-recorder
```

### Docker Compose

```yaml
version: '3.8'

services:
  video-recorder:
    image: video-stream-recorder:latest
    container_name: video-recorder
    restart: unless-stopped
    environment:
      - STREAM_IDS=cam1,cam2
      - STREAM_URL_CAM1=https://camera1.example.com/stream
      - STREAM_URL_CAM2=https://camera2.example.com/stream
      - AUTO_START=true
      - OUTPUT_DIR=/recordings
      - MAX_DISK_USAGE_PERCENT=85
      - SEGMENT_DURATION=30
    volumes:
      - ./recordings:/recordings
      - ./config.json:/config.json:ro  # Optional config file
    ports:
      - "8080:8080"
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8080/health"]
      interval: 30s
      timeout: 10s
      retries: 3
```

## 🔄 Systemd Service (Linux)

Create `/etc/systemd/system/video-recorder.service`:

```ini
[Unit]
Description=Video Stream Recorder
After=network.target
Wants=network.target

[Service]
Type=simple
User=recorder
Group=recorder
WorkingDirectory=/opt/video-recorder
ExecStart=/usr/local/bin/video-stream-recorder -c /opt/video-recorder/config.json
Restart=always
RestartSec=10

# Environment variables
Environment=STREAM_IDS=cam1,cam2
Environment=STREAM_URL_CAM1=https://camera1.example.com/stream
Environment=STREAM_URL_CAM2=https://camera2.example.com/stream
Environment=AUTO_START=true
Environment=OUTPUT_DIR=/data/recordings

# Security
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/data/recordings
PrivateTmp=true

[Install]
WantedBy=multi-user.target
```

Enable and start:
```bash
sudo systemctl daemon-reload
sudo systemctl enable video-recorder
sudo systemctl start video-recorder
sudo systemctl status video-recorder
```

## 🔍 Monitoring & Troubleshooting

### Health Checks

```bash
# Basic health check
curl http://localhost:8080/health

# Detailed metrics
curl http://localhost:8080/metrics

# Multi-stream metrics
curl http://localhost:8080/streams/metrics

# Specific stream status
curl http://localhost:8080/streams/cam1/status
```

### Log Levels

Set logging verbosity:
```bash
# Info level (default)
RUST_LOG=info video-stream-recorder

# Debug level (verbose)
RUST_LOG=debug video-stream-recorder

# Trace level (very verbose)
RUST_LOG=trace video-stream-recorder

# Module-specific logging
RUST_LOG=video_stream_recorder::recorder=trace,video_stream_recorder::disk_manager=debug video-stream-recorder
```

### Common Issues

#### FFmpeg Not Found
```bash
# Ubuntu/Debian
sudo apt install ffmpeg libavformat-dev libavcodec-dev

# CentOS/RHEL
sudo yum install ffmpeg-devel

# macOS
brew install ffmpeg
```

#### Permission Issues
```bash
# Create dedicated user
sudo useradd -r -s /bin/false recorder
sudo mkdir -p /data/recordings
sudo chown recorder:recorder /data/recordings
sudo chmod 755 /data/recordings
```

#### Network Connectivity
```bash
# Test stream URL manually
ffmpeg -i "https://your-stream-url.com/stream" -t 10 -c copy test.mp4

# Check network connectivity
curl -I "https://your-stream-url.com/stream"
```

#### Storage Issues
```bash
# Check disk usage
df -h /data/recordings

# Check file permissions
ls -la /data/recordings

# Monitor cleanup activity
tail -f /var/log/video-recorder.log | grep cleanup
```

## 🚀 Production Deployment

### Performance Tuning

For high-throughput deployments:

```bash
# Increase file descriptor limits
echo "recorder soft nofile 65536" >> /etc/security/limits.conf
echo "recorder hard nofile 65536" >> /etc/security/limits.conf

# Optimize network buffers
echo 'net.core.rmem_max = 16777216' >> /etc/sysctl.conf
echo 'net.core.wmem_max = 16777216' >> /etc/sysctl.conf
sysctl -p
```

### Scaling Recommendations

| Streams | RAM | CPU Cores | Storage |
|---------|-----|-----------|---------|
| 1-5 | 4GB | 2 | 100GB/day |
| 6-20 | 8GB | 4 | 500GB/day |
| 21-50 | 16GB | 8 | 1TB/day |
| 50+ | 32GB+ | 16+ | Custom |

### Security Considerations

- Run as dedicated non-root user
- Use firewall to restrict access
- Enable HTTPS if exposing externally
- Regularly update dependencies
- Monitor disk space and logs
- Implement backup strategy

## 📞 Support

- **Documentation**: [GitHub Wiki](https://github.com/user/video-stream-recorder/wiki)
- **Issues**: [GitHub Issues](https://github.com/user/video-stream-recorder/issues)
- **Discussions**: [GitHub Discussions](https://github.com/user/video-stream-recorder/discussions)

---

*Need help? [Open an issue](https://github.com/user/video-stream-recorder/issues/new) and we'll get you recording in no time!*
