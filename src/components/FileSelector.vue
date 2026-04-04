<script setup lang="ts">
import { computed, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { invoke } from '@tauri-apps/api/core';
import type { TorrentInfo, CommandResult } from '../types';
import { formatBytes } from '../types';
import Button from 'primevue/button';

const { t } = useI18n();

const props = defineProps<{
  torrent: TorrentInfo | null;
}>();

const emit = defineEmits<{
  (e: 'cancel'): void;
  (e: 'streaming-started'): void;
  (e: 'play', data: { url: string; title: string; torrentId: number; fileIndex: number }): void;
}>();

const loadingFile = ref<number | null>(null);

function isStreamableFile(path: string): boolean {
  const streamableExtensions = ['.mp4', '.mkv', '.avi', '.mov', '.wmv', '.webm', '.m4v'];
  const lowerPath = path.toLowerCase();
  return streamableExtensions.some(ext => lowerPath.endsWith(ext));
}

function isSubtitleFile(path: string): boolean {
  const subtitleExtensions = ['.srt', '.vtt', '.sub', '.ass', '.ssa'];
  const lowerPath = path.toLowerCase();
  return subtitleExtensions.some(ext => lowerPath.endsWith(ext));
}

function getFileIcon(path: string): string {
  const lowerPath = path.toLowerCase();
  if (isStreamableFile(lowerPath)) return '🎬';
  if (lowerPath.endsWith('.mp3') || lowerPath.endsWith('.flac') || lowerPath.endsWith('.wav')) return '🎵';
  if (lowerPath.endsWith('.jpg') || lowerPath.endsWith('.png') || lowerPath.endsWith('.gif')) return '🖼️';
  if (lowerPath.endsWith('.txt') || lowerPath.endsWith('.nfo')) return '📄';
  if (isSubtitleFile(lowerPath)) return '💬';
  return '📁';
}

const streamableFiles = computed(() => {
  if (!props.torrent) return [];
  return props.torrent.files.filter(f => isStreamableFile(f.path));
});

async function getStreamUrl(fileIndex: number): Promise<string | null> {
  if (!props.torrent) return null;
  
  const result = await invoke<CommandResult<string>>('start_stream', {
    torrentId: props.torrent.id,
    fileIndex
  });
  
  if (!result.success || !result.data) {
    console.error('Failed to start stream:', result.error);
    alert(t('fileSelector.streamError') + ': ' + (result.error || 'Unknown error'));
    return null;
  }
  
  return result.data;
}

async function playFile(fileIndex: number) {
  if (!props.torrent) return;
  loadingFile.value = fileIndex;
  
  try {
    const streamUrl = await getStreamUrl(fileIndex);
    if (!streamUrl) {
      loadingFile.value = null;
      return;
    }
    
    const file = props.torrent.files.find(f => f.index === fileIndex);
    const title = file?.path.split('/').pop() || props.torrent.name;
    
    // Emit play event — MpvPlayer will handle playback
    emit('play', {
      url: streamUrl,
      title,
      torrentId: props.torrent.id,
      fileIndex,
    });
    
    emit('streaming-started');
  } catch (err) {
    console.error('Stream error:', err);
    alert('Error: ' + String(err));
  } finally {
    loadingFile.value = null;
  }
}
</script>

<template>
  <div v-if="torrent" class="file-selector">
    <div class="header">
      <div class="header-top">
        <h2>{{ torrent.name }}</h2>
        <Button
          @click="emit('cancel')"
          icon="pi pi-times"
          text
          rounded
          severity="secondary"
          class="close-btn"
        />
      </div>
      <p class="subtitle">{{ t('fileSelector.selectFile') }}</p>
      <p class="total-size">{{ t('fileSelector.totalSize') }}: {{ formatBytes(torrent.total_size) }}</p>
    </div>
    
    <div v-if="streamableFiles.length === 0" class="no-streamable">
      <p>{{ t('fileSelector.noStreamableFiles') }}</p>
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
        <div class="file-actions">
          <Button
            :label="'▶ ' + (t('player.play') || 'Reproducir')"
            icon="pi pi-play"
            @click="playFile(file.index)"
            size="small"
            :loading="loadingFile === file.index"
          />
        </div>
      </div>
    </div>
    
    <div class="other-files" v-if="torrent.files.length > streamableFiles.length">
      <details>
        <summary>{{ t('fileSelector.otherFiles') }} ({{ torrent.files.length - streamableFiles.length }})</summary>
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
  </div>
</template>

<style scoped>
.file-selector {
  background: transparent;
  padding: 1.5rem;
  max-height: 70vh;
  display: flex;
  flex-direction: column;
  min-width: 400px;
}

.header {
  margin-bottom: 1rem;
}

.header-top {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  gap: 1rem;
}

.header h2 {
  margin: 0 0 0.5rem 0;
  font-size: 1.25rem;
  color: var(--text-color, #f5f0ea);
  word-break: break-word;
  flex: 1;
}

.close-btn {
  flex-shrink: 0;
  color: var(--text-muted) !important;
}

.close-btn:hover {
  color: var(--text-color) !important;
  background: var(--hover-bg) !important;
}

.subtitle {
  color: var(--text-muted, #a09080);
  font-size: 0.9rem;
  margin: 0 0 0.25rem 0;
}

.total-size {
  color: var(--text-muted, #a09080);
  font-size: 0.8rem;
  margin: 0;
}

.no-streamable {
  padding: 2rem;
  text-align: center;
  color: var(--text-muted, #a09080);
}

.file-list {
  flex: 1;
  overflow-y: auto;
  border: 1px solid var(--border-color, #3d352d);
  border-radius: 8px;
  max-height: 300px;
  margin-bottom: 1rem;
}

.file-item {
  display: flex;
  align-items: center;
  padding: 0.75rem 1rem;
  border-bottom: 1px solid var(--border-color, #3d352d);
  transition: background 0.2s;
}

.file-item:last-child {
  border-bottom: none;
}

.file-item:hover {
  background: var(--hover-bg, rgba(157, 138, 120, 0.1));
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
  color: var(--text-color, #f5f0ea);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.file-size {
  color: var(--text-muted, #a09080);
  font-size: 0.75rem;
  margin-top: 0.25rem;
}

.file-actions {
  flex-shrink: 0;
  margin-left: 0.5rem;
}

.other-files {
  margin-top: 1rem;
}

.other-files summary {
  cursor: pointer;
  color: var(--text-muted, #a09080);
  font-size: 0.85rem;
  padding: 0.5rem;
}

.other-file-list {
  border: 1px solid var(--border-color, #3d352d);
  border-radius: 8px;
  max-height: 150px;
  overflow-y: auto;
  margin-top: 0.5rem;
}

.other-file-item {
  display: flex;
  align-items: center;
  padding: 0.5rem 1rem;
  border-bottom: 1px solid var(--border-color, #3d352d);
  font-size: 0.8rem;
  color: var(--text-muted, #a09080);
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
  color: var(--text-muted, #a09080);
}

.other-file-item .file-size {
  font-size: 0.7rem;
  margin-left: 0.5rem;
}
</style>
