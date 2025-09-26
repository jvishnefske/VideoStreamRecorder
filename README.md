# 🎥 Video Stream Recorder

**Turn any HTTPS webcam stream into a reliable, searchable video archive in minutes.**

Never lose important footage again. Whether you're monitoring security cameras, recording live streams, or archiving webcam feeds, Video Stream Recorder gives you enterprise-grade reliability with minimal configuration setup.

## ⚡ 30-Second Demo

Record multiple streams simultaneously with automatic storage management:

```bash
# 1. Build the image
docker build -t video-stream-recorder .

# 2. Set your stream URLs and run
docker run -d \
  -e STREAM_IDS="front_door,back_yard" \
  -e STREAM_URL_FRONT_DOOR="rtsp://192.168.1.100/stream" \
  -e STREAM_URL_BACK_YARD="https://your-camera-url/stream" \
  -v ./recordings:/recordings \
  -p 8080:8080 \
  video-stream-recorder

# 3. Control via web UI or API
curl -X POST http://localhost:8080/streams/front_door/start
curl -X POST http://localhost:8080/streams/back_yard/start
```

**That's it!** Your streams are now being archived as H.264 segments with automatic cleanup when disk space runs low.

## 🚀 Why Choose Video Stream Recorder?

- **🔥 Zero-Downtime Reliability** - Exponential backoff retry with infinite attempts
- **💾 Smart Storage Management** - Never run out of disk space with intelligent cleanup
- **📈 Multiple Streams** - Record unlimited simultaneous streams with individual control
- **🛠️ 12-Factor Compliant** - Configure everything via environment variables
- **🐳 Production features** - Docker support, health checks, metrics, graceful shutdown
- **⚡ High Performance** - Rust-powered with direct FFmpeg integration, no re-encoding

## 📊 Perfect For

- **Security Camera Systems** - Reliable 24/7 recording with automatic failover
- **Live Stream Archival** - Never miss a moment of important broadcasts
- **IoT Camera Networks** - Centralized recording from multiple edge devices
- **Rolling Recording storage** - Automated retention with storage management
- **Development Testing** - Record and replay video streams for debugging

## 🎛️ Web Dashboard

Access the intuitive web interface at `http://localhost:8080` to:

- View real-time recording status
- Start/stop individual streams
- Monitor storage usage and cleanup activity
- View recording statistics and logs

## 📡 API Endpoints

- `GET /health` - System health and storage info
- `GET /streams` - List all configured streams
- `POST /streams/{id}/start` - Start specific stream
- `POST /streams/{id}/stop` - Stop specific stream
- `GET /streams/{id}/stats` - Get stream statistics
- `GET /streams/metrics` - All streams metrics

## 🔧 Configuration

Configure via environment variables:

### Stream Setup
```bash
# Define stream IDs and URLs
STREAM_IDS="cam1,cam2,cam3"
STREAM_URL_CAM1="rtsp://192.168.1.100/stream"
STREAM_URL_CAM2="https://your-camera/stream"
STREAM_URL_CAM3="rtsp://192.168.1.102/stream"

# Optional: Control individual streams
STREAM_ENABLED_CAM1="true"
STREAM_ENABLED_CAM2="false"
STREAM_SUBDIR_CAM1="front_camera"
```

### System Settings
```bash
OUTPUT_DIR="/recordings"              # Recording directory
SEGMENT_DURATION="30"                # Seconds per file
MAX_DISK_USAGE_PERCENT="85"          # Auto-cleanup threshold
AUTO_START="true"                    # Start recording immediately
PORT="8080"                          # Web interface port
```

Full configuration reference available in [`INSTALL.md`](INSTALL.md).

## 🚀 Get Started

Ready to never lose video footage again?

**[📚 View Installation Guide →](INSTALL.md)**

## 📄 License

MIT License - use it anywhere, modify as needed.

---

*Built with ❤️ in Rust for reliability you can count on.*
