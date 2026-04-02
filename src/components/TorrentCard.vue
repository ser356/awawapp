<script setup lang="ts">
import { computed } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { Command } from '@tauri-apps/plugin-shell';
import type { TorrentStats, TorrentInfo, CommandResult } from '../types';
import { formatBytes, formatSpeed, formatEta } from '../types';

const props = defineProps<{
  stats: TorrentStats;
  torrentInfo?: TorrentInfo;
}>();

const emit = defineEmits<{
  (e: 'pause', id: number): void;
  (e: 'resume', id: number): void;
  (e: 'delete', id: number): void;
}>();

const isPaused = computed(() => {
  const state = props.stats.state.toLowerCase();
  return state.includes('paused') || state.includes('stopped');
});

const isCompleted = computed(() => props.stats.progress >= 99.9);

const progressBarColor = computed(() => {
  if (isCompleted.value) return 'var(--success-color, #10b981)';
  if (isPaused.value) return 'var(--warning-color, #f59e0b)';
  return 'var(--accent-color, #6366f1)';
});

// Get streamable files from torrent info
const streamableFiles = computed(() => {
  if (!props.torrentInfo) return [];
  const streamableExtensions = ['.mp4', '.mkv', '.avi', '.mov', '.wmv', '.webm', '.m4v'];
  return props.torrentInfo.files.filter(f => 
    streamableExtensions.some(ext => f.path.toLowerCase().endsWith(ext))
  );
});

async function playInVlc(fileIndex: number) {
  try {
    // Get the streaming URL
    const result = await invoke<CommandResult<string>>('get_stream_url', {
      torrentId: props.stats.id,
      fileIndex
    });
    
    if (!result.success || !result.data) {
      console.error('Failed to get stream URL:', result.error);
      return;
    }
    
    const streamUrl = result.data;
    
    // Launch VLC with the stream URL
    // Using macOS `open` command with VLC
    const command = Command.create('open', ['-a', 'VLC', streamUrl]);
    await command.execute();
    
  } catch (err) {
    console.error('Failed to open VLC:', err);
    // Try alternative method
    try {
      const result = await invoke<CommandResult<string>>('get_stream_url', {
        torrentId: props.stats.id,
        fileIndex
      });
      if (result.success && result.data) {
        // Open in browser as fallback
        window.open(result.data, '_blank');
      }
    } catch (e) {
      console.error('Fallback also failed:', e);
    }
  }
}

function togglePause() {
  if (isPaused.value) {
    emit('resume', props.stats.id);
  } else {
    emit('pause', props.stats.id);
  }
}
</script>

<template>
  <div class="torrent-card" :class="{ completed: isCompleted, paused: isPaused }">
    <div class="card-header">
      <h3 class="torrent-name">{{ stats.name || 'Loading...' }}</h3>
      <div class="state-badge" :class="stats.state.toLowerCase()">
        {{ stats.state }}
      </div>
    </div>
    
    <div class="progress-section">
      <div class="progress-bar">
        <div
          class="progress-fill"
          :style="{ width: `${stats.progress}%`, background: progressBarColor }"
        ></div>
      </div>
      <div class="progress-info">
        <span class="progress-percent">{{ stats.progress.toFixed(1) }}%</span>
        <span class="progress-size">
          {{ formatBytes(stats.downloaded_bytes) }} / {{ formatBytes(stats.total_bytes) }}
        </span>
      </div>
    </div>
    
    <div class="stats-row">
      <div class="stat">
        <span class="stat-icon">⬇️</span>
        <span class="stat-value">{{ formatSpeed(stats.download_speed) }}</span>
      </div>
      <div class="stat">
        <span class="stat-icon">⬆️</span>
        <span class="stat-value">{{ formatSpeed(stats.upload_speed) }}</span>
      </div>
      <div class="stat">
        <span class="stat-icon">👥</span>
        <span class="stat-value">{{ stats.peers_connected }} / {{ stats.peers_total }}</span>
      </div>
      <div class="stat" v-if="stats.eta_seconds">
        <span class="stat-icon">⏱️</span>
        <span class="stat-value">{{ formatEta(stats.eta_seconds) }}</span>
      </div>
    </div>
    
    <!-- Streamable files (if torrent info available) -->
    <div v-if="streamableFiles.length > 0" class="stream-section">
      <p class="stream-label">Stream in VLC:</p>
      <div class="stream-files">
        <button
          v-for="file in streamableFiles.slice(0, 3)"
          :key="file.index"
          @click="playInVlc(file.index)"
          class="stream-btn"
          :title="file.path"
        >
          🎬 {{ file.path.split('/').pop() }}
        </button>
        <span v-if="streamableFiles.length > 3" class="more-files">
          +{{ streamableFiles.length - 3 }} more
        </span>
      </div>
    </div>
    
    <div class="card-actions">
      <button @click="togglePause" class="action-btn">
        {{ isPaused ? '▶️ Resume' : '⏸️ Pause' }}
      </button>
      <button @click="emit('delete', stats.id)" class="action-btn danger">
        🗑️ Delete
      </button>
    </div>
  </div>
</template>

<style scoped>
.torrent-card {
  background: var(--card-bg, #1a1a2e);
  border-radius: 12px;
  padding: 1.25rem;
  margin-bottom: 1rem;
  transition: transform 0.2s, box-shadow 0.2s;
}

.torrent-card:hover {
  transform: translateY(-2px);
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.3);
}

.torrent-card.completed {
  border-left: 4px solid var(--success-color, #10b981);
}

.torrent-card.paused {
  opacity: 0.8;
}

.card-header {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  margin-bottom: 1rem;
}

.torrent-name {
  margin: 0;
  font-size: 1rem;
  font-weight: 600;
  color: var(--text-color, #fff);
  word-break: break-word;
  flex: 1;
  margin-right: 1rem;
}

.state-badge {
  padding: 0.25rem 0.5rem;
  border-radius: 4px;
  font-size: 0.7rem;
  font-weight: 600;
  text-transform: uppercase;
  background: var(--badge-bg, #2a2a40);
  color: var(--text-muted, #888);
}

.state-badge.live,
.state-badge.downloading {
  background: rgba(99, 102, 241, 0.2);
  color: var(--accent-color, #6366f1);
}

.state-badge.completed,
.state-badge.seeding {
  background: rgba(16, 185, 129, 0.2);
  color: var(--success-color, #10b981);
}

.state-badge.paused {
  background: rgba(245, 158, 11, 0.2);
  color: var(--warning-color, #f59e0b);
}

.progress-section {
  margin-bottom: 1rem;
}

.progress-bar {
  height: 8px;
  background: var(--progress-bg, #2a2a40);
  border-radius: 4px;
  overflow: hidden;
  margin-bottom: 0.5rem;
}

.progress-fill {
  height: 100%;
  border-radius: 4px;
  transition: width 0.3s ease;
}

.progress-info {
  display: flex;
  justify-content: space-between;
  font-size: 0.8rem;
}

.progress-percent {
  color: var(--accent-color, #6366f1);
  font-weight: 600;
}

.progress-size {
  color: var(--text-muted, #888);
}

.stats-row {
  display: flex;
  gap: 1.5rem;
  margin-bottom: 1rem;
  flex-wrap: wrap;
}

.stat {
  display: flex;
  align-items: center;
  gap: 0.35rem;
}

.stat-icon {
  font-size: 0.9rem;
}

.stat-value {
  font-size: 0.85rem;
  color: var(--text-color, #fff);
}

.stream-section {
  margin-bottom: 1rem;
  padding-top: 0.75rem;
  border-top: 1px solid var(--border-color, #333);
}

.stream-label {
  font-size: 0.8rem;
  color: var(--text-muted, #888);
  margin: 0 0 0.5rem 0;
}

.stream-files {
  display: flex;
  gap: 0.5rem;
  flex-wrap: wrap;
  align-items: center;
}

.stream-btn {
  padding: 0.4rem 0.75rem;
  background: var(--btn-secondary, #2a2a40);
  color: var(--text-color, #fff);
  border: none;
  border-radius: 6px;
  font-size: 0.75rem;
  cursor: pointer;
  transition: background 0.2s;
  max-width: 200px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.stream-btn:hover {
  background: var(--accent-color, #6366f1);
}

.more-files {
  font-size: 0.75rem;
  color: var(--text-muted, #888);
}

.card-actions {
  display: flex;
  gap: 0.5rem;
  padding-top: 0.75rem;
  border-top: 1px solid var(--border-color, #333);
}

.action-btn {
  padding: 0.5rem 1rem;
  background: var(--btn-secondary, #2a2a40);
  color: var(--text-color, #fff);
  border: none;
  border-radius: 6px;
  font-size: 0.8rem;
  cursor: pointer;
  transition: background 0.2s;
}

.action-btn:hover {
  background: var(--btn-secondary-hover, #3a3a50);
}

.action-btn.danger:hover {
  background: var(--error-color, #ef4444);
}
</style>
