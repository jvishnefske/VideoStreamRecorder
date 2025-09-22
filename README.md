
# README.md
# Video Stream Recorder

A 12-factor Rust application for recording HTTPS webcam streams into segmented H.264 files.

## Features

- 📹 Records from HTTPS webcam streams
- 🎬 Saves as 10-second H.264 segments without re-encoding
- 💾 Automatic cleanup when disk space is low
- 🔄 Configurable via environment variables
- 📊 Health checks and metrics endpoints
- 🐳 Docker support
- ♻️ Graceful shutdown handling

## Usage

### Environment Variables

- `STREAM_URL`: HTTPS webcam stream URL
- `OUTPUT_DIR`: Directory for recordings
- `SEGMENT_DURATION`: Segment length in seconds (default: 10)
- `MAX_DISK_USAGE_PERCENT`: Cleanup threshold (default: 90)
- `PORT`: Server port (default: 8080)
- `AUTO_START`: Auto-start recording (default: false)

### Running

```bash
# Build and run
cargo run

# With Docker
docker-compose up

# Health check
curl http://localhost:8080/health

# Start/stop recording
curl -X POST http://localhost:8080/start
curl -X POST http://localhost:8080/stop
```

## API Endpoints

- `GET /health` - Health status and storage info
- `GET /metrics` - Recording statistics
- `POST /start` - Start recording
- `POST /stop` - Stop recording

