import { fromEvent, interval, merge } from 'rxjs';
import { startWith, switchMap, catchError, tap, distinctUntilChanged, map } from 'rxjs/operators';
import { of } from 'rxjs';

class VideoStreamRecorderApp {
    constructor() {
        this.apiBase = '';
        this.initializeElements();
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
            switchMap(() => this.fetchHealth()),
            distinctUntilChanged((prev, curr) => JSON.stringify(prev) === JSON.stringify(curr)),
            tap(health => this.updateHealthUI(health)),
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
                this.addLog('Starting recording...', 'info');
            }),
            switchMap(() => this.startRecording()),
            tap(result => {
                this.addLog(`Recording started: ${result}`, 'success');
                this.startBtn.disabled = false;
            }),
            catchError(error => {
                this.addLog(`Failed to start recording: ${error.message}`, 'error');
                this.startBtn.disabled = false;
                return of(null);
            })
        );

        // Stop recording observable
        const stopRecording$ = stopClick$.pipe(
            tap(() => {
                this.stopBtn.disabled = true;
                this.addLog('Stopping recording...', 'info');
            }),
            switchMap(() => this.stopRecording()),
            tap(result => {
                this.addLog(`Recording stopped: ${result}`, 'success');
                this.stopBtn.disabled = false;
            }),
            catchError(error => {
                this.addLog(`Failed to stop recording: ${error.message}`, 'error');
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

    async fetchMetrics() {
        const response = await fetch(`${this.apiBase}/metrics`);
        if (!response.ok) {
            throw new Error(`HTTP ${response.status}: ${response.statusText}`);
        }
        return response.json();
    }

    async startRecording() {
        const response = await fetch(`${this.apiBase}/start`, { method: 'POST' });
        if (!response.ok) {
            throw new Error(`HTTP ${response.status}: ${response.statusText}`);
        }
        return response.text();
    }

    async stopRecording() {
        const response = await fetch(`${this.apiBase}/stop`, { method: 'POST' });
        if (!response.ok) {
            throw new Error(`HTTP ${response.status}: ${response.statusText}`);
        }
        return response.text();
    }

    updateHealthUI(health) {
        if (!health) return;

        this.updateConnectionStatus(true);

        // Update recording status
        const isRecording = health.recording;
        this.recordingStatus.textContent = isRecording ? 'Recording' : 'Idle';

        // Update status indicator
        if (isRecording) {
            this.statusDot.className = 'w-3 h-3 rounded-full bg-secondary-500 animate-pulse-dot transition-colors duration-300';
        } else {
            this.statusDot.className = 'w-3 h-3 rounded-full bg-primary-500 transition-colors duration-300';
        }
        this.statusText.textContent = isRecording ? 'Recording' : 'Ready';

        // Update buttons
        this.startBtn.disabled = isRecording;
        this.stopBtn.disabled = !isRecording;

        // Update storage info
        this.updateStorageUI(health.storage);
    }

    updateMetricsUI(metrics) {
        if (!metrics) return;

        // Update recording metrics
        this.filesRecorded.textContent = metrics.recording_stats.files_recorded;
        this.totalDuration.textContent = this.formatDuration(metrics.recording_stats.total_duration);
        this.filesDeleted.textContent = metrics.files_deleted;

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
}

// Initialize the app when the DOM is loaded
document.addEventListener('DOMContentLoaded', () => {
    new VideoStreamRecorderApp();
});