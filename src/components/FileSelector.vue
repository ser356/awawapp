<script setup lang="ts">
import { computed } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { TorrentInfo, CommandResult } from '../types';
import { formatBytes } from '../types';

const props = defineProps<{
  torrent: TorrentInfo | null;
}>();

const emit = defineEmits<{
  (e: 'cancel'): void;
  (e: 'streaming-started'): void;
}>();

function isStreamableFile(path: string): boolean {
  const streamableExtensions = ['.mp4', '.mkv', '.avi', '.mov', '.wmv', '.webm', '.m4v'];
  const lowerPath = path.toLowerCase();
  return streamableExtensions.some(ext => lowerPath.endsWith(ext));
}

function getFileIcon(path: string): string {
  const lowerPath = path.toLowerCase();
  if (isStreamableFile(lowerPath)) return '🎬';
  if (lowerPath.endsWith('.mp3') || lowerPath.endsWith('.flac') || lowerPath.endsWith('.wav')) return '🎵';
  if (lowerPath.endsWith('.jpg') || lowerPath.endsWith('.png') || lowerPath.endsWith('.gif')) return '🖼️';
  if (lowerPath.endsWith('.txt') || lowerPath.endsWith('.nfo')) return '📄';
  if (lowerPath.endsWith('.srt') || lowerPath.endsWith('.sub')) return '💬';
  return '📁';
}

const streamableFiles = computed(() => {
  if (!props.torrent) return [];
  return props.torrent.files.filter(f => isStreamableFile(f.path));
});

async function streamFile(fileIndex: number) {
  if (!props.torrent) return;
  
  try {
    // Start streaming - this sets up only this file for download and returns URL
    const result = await invoke<CommandResult<string>>('start_stream', {
      torrentId: props.torrent.id,
      fileIndex
    });
    
    if (!result.success || !result.data) {
      console.error('Failed to start stream:', result.error);
      alert('Error al iniciar streaming: ' + (result.error || 'Unknown error'));
      return;
    }
    
    const streamUrl = result.data;
    console.log('Streaming URL:', streamUrl);
    
    // Open VLC with the stream URL via backend command
    const vlcResult = await invoke<CommandResult<void>>('open_in_vlc', { url: streamUrl });
    
    if (!vlcResult.success) {
      console.error('VLC open failed:', vlcResult.error);
      alert(`No se pudo abrir VLC.\n\nURL de streaming:\n${streamUrl}\n\nAbre VLC → Archivo → Abrir ubicación de red`);
    }
    
    emit('streaming-started');
    
  } catch (err) {
    console.error('Stream error:', err);
    alert('Error: ' + String(err));
  }
}
</script>

<template>
  <div v-if="torrent" class="file-selector">
    <div class="header">
      <h2>{{ torrent.name }}</h2>
      <p class="subtitle">Selecciona un archivo para hacer streaming</p>
      <p class="total-size">Tamaño total: {{ formatBytes(torrent.total_size) }}</p>
    </div>
    
    <div v-if="streamableFiles.length === 0" class="no-streamable">
      <p>No se encontraron archivos de video para streaming.</p>
    </div>
    
    <div v-else class="file-list">
      <div
        v-for="file in streamableFiles"
        :key="file.index"
        class="file-item"
      >
        <span class="file-icon">{{ getFileIcon(file.path) }}</span>
        <div class="file-info">
          <span class="file-name">{{ file.path }}</span>
          <span class="file-size">{{ formatBytes(file.size) }}</span>
        </div>
        <button @click="streamFile(file.index)" class="stream-btn">
          ▶️ Stream
        </button>
      </div>
    </div>
    
    <div class="other-files" v-if="torrent.files.length > streamableFiles.length">
      <details>
        <summary>Otros archivos ({{ torrent.files.length - streamableFiles.length }})</summary>
        <div class="other-file-list">
          <div
            v-for="file in torrent.files.filter(f => !isStreamableFile(f.path))"
            :key="file.index"
            class="other-file-item"
          >
            <span class="file-icon">{{ getFileIcon(file.path) }}</span>
            <span class="file-name">{{ file.path }}</span>
            <span class="file-size">{{ formatBytes(file.size) }}</span>
          </div>
        </div>
      </details>
    </div>
    
    <div class="actions">
      <button @click="emit('cancel')" class="cancel-btn">Cerrar</button>
    </div>
  </div>
</template>

<style scoped>
.file-selector {
  background: var(--card-bg, #1a1a2e);
  border-radius: 12px;
  padding: 1.5rem;
  max-height: 70vh;
  display: flex;
  flex-direction: column;
  min-width: 400px;
}

.header {
  margin-bottom: 1rem;
}

.header h2 {
  margin: 0 0 0.5rem 0;
  font-size: 1.25rem;
  color: var(--text-color, #fff);
  word-break: break-word;
}

.subtitle {
  color: var(--text-muted, #888);
  font-size: 0.9rem;
  margin: 0 0 0.25rem 0;
}

.total-size {
  color: var(--text-muted, #888);
  font-size: 0.8rem;
  margin: 0;
}

.no-streamable {
  padding: 2rem;
  text-align: center;
  color: var(--text-muted, #888);
}

.file-list {
  flex: 1;
  overflow-y: auto;
  border: 1px solid var(--border-color, #333);
  border-radius: 8px;
  max-height: 300px;
  margin-bottom: 1rem;
}

.file-item {
  display: flex;
  align-items: center;
  padding: 0.75rem 1rem;
  border-bottom: 1px solid var(--border-color, #333);
  transition: background 0.2s;
}

.file-item:last-child {
  border-bottom: none;
}

.file-item:hover {
  background: var(--hover-bg, rgba(255, 255, 255, 0.05));
}

.file-icon {
  font-size: 1.5rem;
  margin-right: 0.75rem;
}

.file-info {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
}

.file-name {
  font-size: 0.9rem;
  color: var(--text-color, #fff);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.file-size {
  color: var(--text-muted, #888);
  font-size: 0.75rem;
  margin-top: 0.25rem;
}

.stream-btn {
  padding: 0.5rem 1rem;
  background: var(--accent-color, #6366f1);
  color: white;
  border: none;
  border-radius: 6px;
  font-weight: 600;
  cursor: pointer;
  transition: background 0.2s;
  white-space: nowrap;
}

.stream-btn:hover {
  background: var(--accent-hover, #4f46e5);
}

.other-files {
  margin-bottom: 1rem;
}

.other-files summary {
  cursor: pointer;
  color: var(--text-muted, #888);
  font-size: 0.85rem;
  padding: 0.5rem;
}

.other-file-list {
  border: 1px solid var(--border-color, #333);
  border-radius: 8px;
  max-height: 150px;
  overflow-y: auto;
  margin-top: 0.5rem;
}

.other-file-item {
  display: flex;
  align-items: center;
  padding: 0.5rem 1rem;
  border-bottom: 1px solid var(--border-color, #333);
  font-size: 0.8rem;
  color: var(--text-muted, #888);
}

.other-file-item:last-child {
  border-bottom: none;
}

.other-file-item .file-icon {
  font-size: 1rem;
}

.other-file-item .file-name {
  flex: 1;
  font-size: 0.8rem;
  color: var(--text-muted, #888);
}

.other-file-item .file-size {
  font-size: 0.7rem;
  margin-left: 0.5rem;
}

.actions {
  display: flex;
  justify-content: flex-end;
  padding-top: 1rem;
  border-top: 1px solid var(--border-color, #333);
}

.cancel-btn {
  padding: 0.75rem 1.25rem;
  background: transparent;
  color: var(--text-muted, #888);
  border: 1px solid var(--border-color, #333);
  border-radius: 8px;
  cursor: pointer;
  transition: all 0.2s;
}

.cancel-btn:hover {
  background: var(--hover-bg, rgba(255, 255, 255, 0.05));
  color: var(--text-color, #fff);
}
</style>
