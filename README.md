# Video Stream Recorder

A Rust application that saves streaming video from security cameras to disk with automatic storage management.

## Quick Start

```bash
# Build
cargo build --release

# Configure and run
export STREAM_URL="rtsp://192.168.1.100/stream"
export OUTPUT_DIR="./recordings"
./target/release/video-stream-recorder

# Or use Docker
docker build -t video-stream-recorder .
docker run -d -e STREAM_URL="rtsp://your-camera/stream" -v ./recordings:/recordings -p 8080:8080 video-stream-recorder
```

## Features

- Multi-stream recording with individual stream control
- Automatic disk space management with oldest-file cleanup
- Web dashboard and REST API for monitoring and control
- Configurable segment duration and retention policies
- Exponential backoff retry on stream failures
- 12-factor app configuration via environment variables

## API Endpoints

- `GET /health` - System health and storage info
- `GET /streams` - List all configured streams
- `POST /streams/{id}/start` - Start recording a stream
- `POST /streams/{id}/stop` - Stop recording a stream
- `GET /streams/{id}/stats` - Get stream statistics
- `GET /streams/metrics` - All streams metrics

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `STREAM_URL` | - | Single stream URL (legacy) |
| `STREAM_IDS` | - | Comma-separated stream IDs for multi-stream |
| `STREAM_URL_{ID}` | - | URL for stream ID |
| `OUTPUT_DIR` | `./recordings` | Recording output directory |
| `SEGMENT_DURATION` | `10` | Seconds per segment file |
| `MAX_DISK_USAGE_PERCENT` | `90` | Cleanup threshold |
| `AUTO_START` | `true` | Start recording on launch |
| `PORT` | `8080` | Web server port |

## License

MIT License
