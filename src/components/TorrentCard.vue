<script setup lang="ts">
import { computed } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { TorrentStats, TorrentInfo, CommandResult } from '../types';
import { formatBytes, formatSpeed, formatEta } from '../types';
import Button from 'primevue/button';
import ProgressBar from 'primevue/progressbar';
import Tag from 'primevue/tag';

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
  return state.includes('paused') || state.includes('stopped') || state.includes('ready');
});

const isCompleted = computed(() => props.stats.progress >= 99.9);

const progressBarColor = computed(() => {
  if (isCompleted.value) return 'var(--success-color, #8fb573)';
  if (isPaused.value) return 'var(--warning-color, #d9a85c)';
  return 'var(--accent-color, #9d8a78)';
});

const tagSeverity = computed(() => {
  const state = props.stats.state.toLowerCase();
  if (state.includes('download') || state === 'live') return 'info';
  if (state === 'completed' || state === 'seeding') return 'success';
  if (state === 'paused') return 'warn';
  return 'secondary';
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
    // Start streaming - this sets up the file for download and returns URL
    const result = await invoke<CommandResult<string>>('start_stream', {
      torrentId: props.stats.id,
      fileIndex
    });
    
    if (!result.success || !result.data) {
      console.error('Failed to start stream:', result.error);
      return;
    }
    
    const streamUrl = result.data;
    console.log('Opening stream URL in VLC:', streamUrl);
    
    // Open VLC via backend command
    const vlcResult = await invoke<CommandResult<void>>('open_in_vlc', { url: streamUrl });
    
    if (!vlcResult.success) {
      console.error('VLC open failed:', vlcResult.error);
      alert(`URL de streaming:\n\n${streamUrl}\n\nAbre VLC → Archivo → Abrir ubicación de red`);
    }
    
  } catch (err) {
    console.error('Failed to stream:', err);
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
      <Tag :value="stats.state" :severity="tagSeverity" />
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
        <i class="pi pi-download stat-icon"></i>
        <span class="stat-value">{{ formatSpeed(stats.download_speed) }}</span>
      </div>
      <div class="stat">
        <i class="pi pi-upload stat-icon"></i>
        <span class="stat-value">{{ formatSpeed(stats.upload_speed) }}</span>
      </div>
      <div class="stat">
        <i class="pi pi-users stat-icon"></i>
        <span class="stat-value">{{ stats.peers_connected }} / {{ stats.peers_total }}</span>
      </div>
      <div class="stat" v-if="stats.eta_seconds">
        <i class="pi pi-clock stat-icon"></i>
        <span class="stat-value">{{ formatEta(stats.eta_seconds) }}</span>
      </div>
    </div>
    
    <!-- Streamable files (if torrent info available) -->
    <div v-if="streamableFiles.length > 0" class="stream-section">
      <p class="stream-label">Stream in VLC:</p>
      <div class="stream-files">
        <Button
          v-for="file in streamableFiles.slice(0, 3)"
          :key="file.index"
          @click="playInVlc(file.index)"
          :label="file.path.split('/').pop()"
          icon="pi pi-play"
          size="small"
          outlined
          class="stream-btn"
          :title="file.path"
        />
        <span v-if="streamableFiles.length > 3" class="more-files">
          +{{ streamableFiles.length - 3 }} more
        </span>
      </div>
    </div>
    
    <div class="card-actions">
      <Button
        @click="togglePause"
        :icon="isPaused ? 'pi pi-play' : 'pi pi-pause'"
        :label="isPaused ? 'Resume' : 'Pause'"
        size="small"
        outlined
      />
      <Button
        @click="emit('delete', stats.id)"
        icon="pi pi-trash"
        label="Delete"
        size="small"
        severity="danger"
        outlined
      />
    </div>
  </div>
</template>

<style scoped>
.torrent-card {
  background: var(--card-bg, #2a2420);
  border-radius: 12px;
  padding: 1.25rem;
  margin-bottom: 1rem;
  transition: transform 0.2s, box-shadow 0.2s;
}

.torrent-card:hover {
  transform: translateY(-2px);
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
}

.torrent-card.completed {
  border-left: 4px solid var(--success-color, #8fb573);
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
  color: var(--text-color, #f5f0ea);
  word-break: break-word;
  flex: 1;
  margin-right: 1rem;
}

.progress-section {
  margin-bottom: 1rem;
}

.progress-bar {
  height: 8px;
  background: var(--progress-bg, #3d352d);
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
  color: var(--accent-color, #9d8a78);
  font-weight: 600;
}

.progress-size {
  color: var(--text-muted, #a09080);
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
  color: var(--text-color, #f5f0ea);
}

.stat-value {
  font-size: 0.85rem;
  color: var(--text-color, #f5f0ea);
}

.stream-section {
  margin-bottom: 1rem;
  padding-top: 0.75rem;
  border-top: 1px solid var(--border-color, #3d352d);
}

.stream-label {
  font-size: 0.8rem;
  color: var(--text-muted, #a09080);
  margin: 0 0 0.5rem 0;
}

.stream-files {
  display: flex;
  gap: 0.5rem;
  flex-wrap: wrap;
  align-items: center;
}

.stream-btn {
  max-width: 200px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.more-files {
  font-size: 0.75rem;
  color: var(--text-muted, #a09080);
}

.card-actions {
  display: flex;
  gap: 0.5rem;
  padding-top: 0.75rem;
  border-top: 1px solid var(--border-color, #3d352d);
}
</style>
