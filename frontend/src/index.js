import { fromEvent, interval, merge } from 'rxjs';
import { startWith, switchMap, catchError, tap, distinctUntilChanged, map } from 'rxjs/operators';
import { of } from 'rxjs';

class VideoStreamRecorderApp {
    constructor() {
        this.apiBase = '';
        this.streams = [];
        this.initializeElements();
        this.loadStreams();
        this.setupObservables();
        this.addLog('Application started');
    }

    initializeElements() {
        // Status elements
        this.statusDot = document.getElementById('statusDot');
        this.statusText = document.getElementById('statusText');

        // Control buttons
        this.startBtn = document.getElementById('startBtn');
        this.stopBtn = document.getElementById('stopBtn');
        this.refreshBtn = document.getElementById('refreshBtn');

        // Metric elements
        this.recordingStatus = document.getElementById('recordingStatus');
        this.filesRecorded = document.getElementById('filesRecorded');
        this.totalDuration = document.getElementById('totalDuration');
        this.storageUsed = document.getElementById('storageUsed');

        // Storage elements
        this.storageFill = document.getElementById('storageFill');
        this.storageText = document.getElementById('storageText');
        this.availableSpace = document.getElementById('availableSpace');
        this.totalSpace = document.getElementById('totalSpace');
        this.filesDeleted = document.getElementById('filesDeleted');

        // Log container
        this.logContainer = document.getElementById('logContainer');

        // Stream management elements
        this.streamManagement = document.getElementById('streamManagement');
        this.streamContainer = document.getElementById('streamContainer');
    }

    async loadStreams() {
        try {
            const streamIds = await this.fetchStreams();
            const metrics = await this.fetchMetrics();
            this.streams = this.buildStreamObjects(streamIds, metrics);
            this.addLog(`Loaded ${this.streams.length} stream(s)`, 'info');
        } catch (error) {
            this.addLog(`Failed to load streams: ${error.message}`, 'error');
            this.streams = [];
        }
    }

    setupObservables() {
        // Create observables for button clicks
        const startClick$ = fromEvent(this.startBtn, 'click');
        const stopClick$ = fromEvent(this.stopBtn, 'click');
        const refreshClick$ = fromEvent(this.refreshBtn, 'click');

        // Auto-refresh every 5 seconds + manual refresh
        const refresh$ = merge(
            interval(5000),
            refreshClick$
        ).pipe(startWith(0));

        // Health check observable
        const health$ = refresh$.pipe(
            switchMap(() => Promise.all([this.fetchHealth(), this.fetchStreams(), this.fetchMetrics()])),
            distinctUntilChanged((prev, curr) => JSON.stringify(prev) === JSON.stringify(curr)),
            tap(([health, streamIds, metrics]) => {
                // Build complete stream objects from metrics data
                this.streams = this.buildStreamObjects(streamIds, metrics);
                this.updateHealthUI(health);
                this.updateStreamUI();
            }),
            catchError(error => {
                this.addLog(`Health check failed: ${error.message}`, 'error');
                this.updateConnectionStatus(false);
                return of(null);
            })
        );

        // Metrics observable (every 10 seconds)
        const metrics$ = interval(10000).pipe(
            startWith(0),
            switchMap(() => this.fetchMetrics()),
            tap(metrics => this.updateMetricsUI(metrics)),
            catchError(error => {
                this.addLog(`Metrics fetch failed: ${error.message}`, 'error');
                return of(null);
            })
        );

        // Start recording observable
        const startRecording$ = startClick$.pipe(
            tap(() => {
                this.startBtn.disabled = true;
                this.addLog('Starting all enabled streams...', 'info');
            }),
            switchMap(() => this.startAllStreams()),
            tap(result => {
                this.addLog(`Streams started: ${result}`, 'success');
                this.startBtn.disabled = false;
            }),
            catchError(error => {
                this.addLog(`Failed to start streams: ${error.message}`, 'error');
                this.startBtn.disabled = false;
                return of(null);
            })
        );

        // Stop recording observable
        const stopRecording$ = stopClick$.pipe(
            tap(() => {
                this.stopBtn.disabled = true;
                this.addLog('Stopping all streams...', 'info');
            }),
            switchMap(() => this.stopAllStreams()),
            tap(result => {
                this.addLog(`Streams stopped: ${result}`, 'success');
                this.stopBtn.disabled = false;
            }),
            catchError(error => {
                this.addLog(`Failed to stop streams: ${error.message}`, 'error');
                this.stopBtn.disabled = false;
                return of(null);
            })
        );

        // Subscribe to all observables
        health$.subscribe();
        metrics$.subscribe();
        startRecording$.subscribe();
        stopRecording$.subscribe();
    }

    async fetchHealth() {
        const response = await fetch(`${this.apiBase}/health`);
        if (!response.ok) {
            throw new Error(`HTTP ${response.status}: ${response.statusText}`);
        }
        return response.json();
    }

    async fetchStreams() {
        const response = await fetch(`${this.apiBase}/streams`);
        if (!response.ok) {
            throw new Error(`HTTP ${response.status}: ${response.statusText}`);
        }
        const data = await response.json();
        // The API returns { "streams": ["stream1", "stream2"] }
        // Convert to array of stream IDs for compatibility
        return data.streams || [];
    }

    async fetchMetrics() {
        const response = await fetch(`${this.apiBase}/streams/metrics`);
        if (!response.ok) {
            throw new Error(`HTTP ${response.status}: ${response.statusText}`);
        }
        return response.json();
    }

    async startStream(streamId) {
        const response = await fetch(`${this.apiBase}/streams/${streamId}/start`, { method: 'POST' });
        if (!response.ok) {
            throw new Error(`HTTP ${response.status}: ${response.statusText}`);
        }
        return response.text();
    }

    async stopStream(streamId) {
        const response = await fetch(`${this.apiBase}/streams/${streamId}/stop`, { method: 'POST' });
        if (!response.ok) {
            throw new Error(`HTTP ${response.status}: ${response.statusText}`);
        }
        return response.text();
    }

    async getStreamStatus(streamId) {
        const response = await fetch(`${this.apiBase}/streams/${streamId}/status`);
        if (!response.ok) {
            throw new Error(`HTTP ${response.status}: ${response.statusText}`);
        }
        return response.json();
    }

    async startAllStreams() {
        const enabledStreams = this.streams.filter(s => s.enabled);
        const results = [];

        for (const stream of enabledStreams) {
            try {
                const response = await fetch(`${this.apiBase}/streams/${stream.id}/start`, { method: 'POST' });
                if (!response.ok) {
                    throw new Error(`HTTP ${response.status}: ${response.statusText}`);
                }
                const result = await response.text();
                results.push(`${stream.id}: ${result}`);
            } catch (error) {
                results.push(`${stream.id}: Error - ${error.message}`);
            }
        }

        return results.join(', ');
    }

    async stopAllStreams() {
        const enabledStreams = this.streams.filter(s => s.enabled);
        const results = [];

        for (const stream of enabledStreams) {
            try {
                const response = await fetch(`${this.apiBase}/streams/${stream.id}/stop`, { method: 'POST' });
                if (!response.ok) {
                    throw new Error(`HTTP ${response.status}: ${response.statusText}`);
                }
                const result = await response.text();
                results.push(`${stream.id}: ${result}`);
            } catch (error) {
                results.push(`${stream.id}: Error - ${error.message}`);
            }
        }

        return results.join(', ');
    }

    updateHealthUI(health) {
        if (!health) return;

        this.updateConnectionStatus(true);

        // Count recording streams
        const recordingStreams = this.streams.filter(s => s.recording).length;
        const totalEnabledStreams = this.streams.filter(s => s.enabled).length;
        const isRecording = recordingStreams > 0;

        // Update recording status
        if (recordingStreams === 0) {
            this.recordingStatus.textContent = 'Idle';
        } else if (recordingStreams === totalEnabledStreams) {
            this.recordingStatus.textContent = 'Recording All';
        } else {
            this.recordingStatus.textContent = `Recording ${recordingStreams}/${totalEnabledStreams}`;
        }

        // Update status indicator
        if (isRecording) {
            this.statusDot.className = 'w-3 h-3 rounded-full bg-secondary-500 animate-pulse-dot transition-colors duration-300';
        } else {
            this.statusDot.className = 'w-3 h-3 rounded-full bg-primary-500 transition-colors duration-300';
        }
        this.statusText.textContent = isRecording ? `Recording (${recordingStreams})` : 'Ready';

        // Update buttons - allow start if any stream is not recording, allow stop if any stream is recording
        this.startBtn.disabled = (recordingStreams === totalEnabledStreams) || (totalEnabledStreams === 0);
        this.stopBtn.disabled = recordingStreams === 0;

        // Update storage info
        this.updateStorageUI(health.storage);
    }

    updateMetricsUI(metrics) {
        if (!metrics) return;

        // Aggregate metrics from all streams
        let totalFiles = 0;
        let totalDuration = 0;

        if (metrics.multi_stream_stats && metrics.multi_stream_stats.streams) {
            for (const streamMetrics of metrics.multi_stream_stats.streams) {
                if (streamMetrics.stats) {
                    totalFiles += streamMetrics.stats.files_recorded || 0;
                    totalDuration += streamMetrics.stats.total_duration || 0;
                }
            }
        }

        // Update recording metrics
        this.filesRecorded.textContent = totalFiles;
        this.totalDuration.textContent = this.formatDuration(totalDuration);
        this.filesDeleted.textContent = metrics.files_deleted || 0;

        // Update storage info
        this.updateStorageUI(metrics.storage_info);

        // Log last cleanup time if available
        if (metrics.last_cleanup) {
            const cleanupTime = new Date(metrics.last_cleanup).toLocaleString();
            this.addLog(`Last cleanup: ${cleanupTime}`, 'info');
        }
    }

    updateStorageUI(storage) {
        if (!storage) return;

        const usagePercent = Math.round(storage.usage_percent * 10) / 10;
        this.storageUsed.textContent = `${usagePercent}%`;
        this.storageText.textContent = `${usagePercent}% used`;
        this.storageFill.style.width = `${Math.min(usagePercent, 100)}%`;

        this.availableSpace.textContent = this.formatBytes(storage.available_space);
        this.totalSpace.textContent = this.formatBytes(storage.total_space);

        // Change storage bar color based on usage
        this.storageFill.className = this.storageFill.className.replace(/storage-fill-\w+/, '');
        if (usagePercent > 90) {
            this.storageFill.classList.add('storage-fill-high');
        } else if (usagePercent > 75) {
            this.storageFill.classList.add('storage-fill-medium');
        } else {
            this.storageFill.classList.add('storage-fill-low');
        }
    }

    updateConnectionStatus(connected) {
        if (connected) {
            this.statusText.textContent = this.statusText.textContent || 'Ready';
            this.refreshBtn.disabled = false;
        } else {
            this.statusDot.className = 'w-3 h-3 rounded-full bg-error transition-colors duration-300';
            this.statusText.textContent = 'Connection Error';
            this.startBtn.disabled = true;
            this.stopBtn.disabled = true;
            this.refreshBtn.disabled = false;
        }
    }

    addLog(message, type = 'info') {
        const timestamp = new Date().toLocaleTimeString();
        const logEntry = document.createElement('div');

        let colorClass = 'text-neutral-800';
        if (type === 'success') colorClass = 'text-primary-500';
        else if (type === 'error') colorClass = 'text-secondary-500';
        else if (type === 'warning') colorClass = 'text-warning';

        logEntry.className = `py-1 border-b border-gray-200 last:border-b-0 ${colorClass}`;
        logEntry.textContent = `[${timestamp}] ${message}`;

        this.logContainer.appendChild(logEntry);
        this.logContainer.scrollTop = this.logContainer.scrollHeight;

        // Keep only last 100 log entries
        while (this.logContainer.children.length > 100) {
            this.logContainer.removeChild(this.logContainer.firstChild);
        }
    }

    formatDuration(seconds) {
        const hours = Math.floor(seconds / 3600);
        const minutes = Math.floor((seconds % 3600) / 60);
        const secs = seconds % 60;

        if (hours > 0) {
            return `${hours}h ${minutes}m ${secs}s`;
        } else if (minutes > 0) {
            return `${minutes}m ${secs}s`;
        } else {
            return `${secs}s`;
        }
    }

    formatBytes(bytes) {
        if (bytes === 0) return '0 B';

        const k = 1024;
        const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
        const i = Math.floor(Math.log(bytes) / Math.log(k));

        return `${(bytes / Math.pow(k, i)).toFixed(1)} ${sizes[i]}`;
    }

    buildStreamObjects(streamIds, metrics) {
        if (!streamIds || !Array.isArray(streamIds)) {
            return [];
        }

        return streamIds.map(streamId => {
            // Find this stream in the metrics data
            let streamData = null;
            if (metrics && metrics.multi_stream_stats && metrics.multi_stream_stats.streams) {
                streamData = metrics.multi_stream_stats.streams.find(s => s.stream_id === streamId);
            }

            return {
                id: streamId,
                url: streamData ? streamData.stream_url : 'Unknown',
                enabled: true, // Assume enabled if it's in the list
                recording: streamData ? streamData.is_recording : false
            };
        });
    }

    updateStreamUI() {
        if (!this.streams || this.streams.length === 0) {
            this.streamManagement.style.display = 'none';
            return;
        }

        this.streamManagement.style.display = 'block';
        this.streamContainer.innerHTML = '';

        this.streams.forEach(stream => {
            const streamCard = document.createElement('div');
            streamCard.className = 'bg-gray-50 border border-gray-200 rounded-lg p-4';
            streamCard.innerHTML = `
                <div class="flex items-center justify-between mb-3">
                    <h4 class="font-medium text-gray-900">${stream.id}</h4>
                    <span class="px-2 py-1 text-xs font-medium rounded-full ${stream.enabled ? 'bg-green-100 text-green-800' : 'bg-gray-100 text-gray-800'}">
                        ${stream.enabled ? 'Enabled' : 'Disabled'}
                    </span>
                </div>
                <div class="text-sm text-gray-600 mb-3 break-all">${stream.url}</div>
                <div class="flex items-center justify-between mb-3">
                    <span class="text-sm font-medium">Status:</span>
                    <span class="stream-status text-sm font-medium ${stream.recording ? 'text-green-600' : 'text-gray-600'}" data-stream-id="${stream.id}">
                        ${stream.recording ? 'Recording' : 'Idle'}
                    </span>
                </div>
                <div class="flex gap-2">
                    <button class="stream-start-btn px-3 py-1 text-sm font-medium bg-green-500 text-white rounded hover:bg-green-600 disabled:opacity-60 disabled:cursor-not-allowed"
                            data-stream-id="${stream.id}" ${!stream.enabled || stream.recording ? 'disabled' : ''}>
                        Start
                    </button>
                    <button class="stream-stop-btn px-3 py-1 text-sm font-medium bg-red-500 text-white rounded hover:bg-red-600 disabled:opacity-60 disabled:cursor-not-allowed"
                            data-stream-id="${stream.id}" ${!stream.enabled || !stream.recording ? 'disabled' : ''}>
                        Stop
                    </button>
                </div>
            `;
            this.streamContainer.appendChild(streamCard);
        });

        // Add event listeners for individual stream buttons
        this.streamContainer.addEventListener('click', (e) => {
            const streamId = e.target.dataset.streamId;
            if (!streamId) return;

            if (e.target.classList.contains('stream-start-btn')) {
                this.handleStreamStart(streamId, e.target);
            } else if (e.target.classList.contains('stream-stop-btn')) {
                this.handleStreamStop(streamId, e.target);
            }
        });
    }

    async handleStreamStart(streamId, button) {
        button.disabled = true;
        this.addLog(`Starting stream ${streamId}...`, 'info');

        try {
            const result = await this.startStream(streamId);
            this.addLog(`Stream ${streamId} started: ${result}`, 'success');
        } catch (error) {
            this.addLog(`Failed to start stream ${streamId}: ${error.message}`, 'error');
        } finally {
            button.disabled = false;
        }
    }

    async handleStreamStop(streamId, button) {
        button.disabled = true;
        this.addLog(`Stopping stream ${streamId}...`, 'info');

        try {
            const result = await this.stopStream(streamId);
            this.addLog(`Stream ${streamId} stopped: ${result}`, 'success');
        } catch (error) {
            this.addLog(`Failed to stop stream ${streamId}: ${error.message}`, 'error');
        } finally {
            button.disabled = false;
        }
    }
}

// Initialize the app when the DOM is loaded
document.addEventListener('DOMContentLoaded', () => {
    new VideoStreamRecorderApp();
});