# Video Stream Recorder - MVP Design

## Functional Requirements

### Stream Recording
- [ ] FR-1: Connect to RTSP/HTTPS video streams
- [ ] FR-2: Record video to disk as segmented MP4 files
- [ ] FR-3: Support multiple simultaneous streams
- [ ] FR-4: Configurable segment duration
- [ ] FR-5: Automatic retry with exponential backoff on stream failure

### Storage Management
- [ ] FR-6: Monitor disk usage percentage
- [ ] FR-7: Automatic cleanup of oldest files when threshold exceeded
- [ ] FR-8: Configurable disk usage threshold
- [ ] FR-9: Per-stream output directories

### Web Interface
- [ ] FR-10: Health check endpoint returning system status
- [ ] FR-11: List configured streams endpoint
- [ ] FR-12: Start/stop recording per stream via API
- [ ] FR-13: Stream statistics endpoint
- [ ] FR-14: Web dashboard for monitoring

### Configuration
- [ ] FR-15: 12-factor app configuration via environment variables
- [ ] FR-16: JSON configuration file support
- [ ] FR-17: Legacy single-stream backward compatibility

## Non-Functional Requirements

### Reliability
- NFR-1: Graceful shutdown preserving segment integrity
- NFR-2: No data loss on controlled restart
- NFR-3: Infinite retry mode for critical streams

### Performance
- NFR-4: Direct stream copy without re-encoding
- NFR-5: Minimal CPU overhead during recording

### Security
- NFR-6: No unsafe Rust code
- NFR-7: Result-based error handling throughout

## Architecture

### Modules
- `config.rs` - Configuration loading and environment variable parsing
- `recorder.rs` - Stream recording logic with FFmpeg integration
- `disk_manager.rs` - Storage monitoring and cleanup
- `server.rs` - Axum web server and API endpoints
- `error.rs` - Custom error types with thiserror

### Design Patterns
- Builder pattern for complex configuration
- Immutable-by-default data structures
- Functional core with imperative shell for I/O
- Linear ownership without Arc/Mutex where possible
