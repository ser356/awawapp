<script setup lang="ts">
import { ref, computed, watch } from 'vue';
import type { TorrentInfo } from '../types';
import { formatBytes } from '../types';

const props = defineProps<{
  torrent: TorrentInfo | null;
}>();

const emit = defineEmits<{
  (e: 'start-download', torrentId: number, fileIndices: number[]): void;
  (e: 'cancel'): void;
}>();

const selectedFiles = ref<Set<number>>(new Set());

// Initialize selection when torrent changes
watch(() => props.torrent, (newTorrent) => {
  if (newTorrent) {
    selectedFiles.value = new Set(
      newTorrent.files
        .filter(f => isStreamableFile(f.path))
        .map(f => f.index)
    );
  }
}, { immediate: true });

const totalSelectedSize = computed(() => {
  if (!props.torrent) return 0;
  return props.torrent.files
    .filter(f => selectedFiles.value.has(f.index))
    .reduce((sum, f) => sum + f.size, 0);
});

const hasSelection = computed(() => selectedFiles.value.size > 0);

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

function toggleFile(index: number) {
  if (selectedFiles.value.has(index)) {
    selectedFiles.value.delete(index);
  } else {
    selectedFiles.value.add(index);
  }
  selectedFiles.value = new Set(selectedFiles.value); // Trigger reactivity
}

function selectAll() {
  if (props.torrent) {
    selectedFiles.value = new Set(props.torrent.files.map(f => f.index));
  }
}

function selectNone() {
  selectedFiles.value = new Set();
}

function selectStreamable() {
  if (props.torrent) {
    selectedFiles.value = new Set(
      props.torrent.files
        .filter(f => isStreamableFile(f.path))
        .map(f => f.index)
    );
  }
}

function startDownload() {
  if (props.torrent && hasSelection.value) {
    emit('start-download', props.torrent.id, Array.from(selectedFiles.value));
  }
}
</script>

<template>
  <div v-if="torrent" class="file-selector">
    <div class="header">
      <h2>{{ torrent.name }}</h2>
      <p class="total-size">Total: {{ formatBytes(torrent.total_size) }}</p>
    </div>
    
    <div class="selection-controls">
      <button @click="selectAll" class="control-btn">Select All</button>
      <button @click="selectNone" class="control-btn">Select None</button>
      <button @click="selectStreamable" class="control-btn primary">Video Only</button>
    </div>
    
    <div class="file-list">
      <div
        v-for="file in torrent.files"
        :key="file.index"
        class="file-item"
        :class="{ selected: selectedFiles.has(file.index), streamable: isStreamableFile(file.path) }"
        @click="toggleFile(file.index)"
      >
        <span class="file-icon">{{ getFileIcon(file.path) }}</span>
        <span class="file-name">{{ file.path }}</span>
        <span class="file-size">{{ formatBytes(file.size) }}</span>
        <input
          type="checkbox"
          :checked="selectedFiles.has(file.index)"
          @click.stop
          @change="toggleFile(file.index)"
        />
      </div>
    </div>
    
    <div class="actions">
      <div class="selection-info">
        <span>{{ selectedFiles.size }} files selected</span>
        <span class="selected-size">{{ formatBytes(totalSelectedSize) }}</span>
      </div>
      <div class="buttons">
        <button @click="emit('cancel')" class="cancel-btn">Cancel</button>
        <button
          @click="startDownload"
          :disabled="!hasSelection"
          class="download-btn"
        >
          Start Download
        </button>
      </div>
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
}

.header {
  margin-bottom: 1rem;
}

.header h2 {
  margin: 0 0 0.25rem 0;
  font-size: 1.25rem;
  color: var(--text-color, #fff);
  word-break: break-word;
}

.total-size {
  color: var(--text-muted, #888);
  font-size: 0.875rem;
  margin: 0;
}

.selection-controls {
  display: flex;
  gap: 0.5rem;
  margin-bottom: 1rem;
}

.control-btn {
  padding: 0.5rem 0.75rem;
  background: var(--btn-secondary, #2a2a40);
  color: var(--text-color, #fff);
  border: none;
  border-radius: 6px;
  font-size: 0.8rem;
  cursor: pointer;
  transition: background 0.2s;
}

.control-btn:hover {
  background: var(--btn-secondary-hover, #3a3a50);
}

.control-btn.primary {
  background: var(--accent-color, #6366f1);
}

.control-btn.primary:hover {
  background: var(--accent-hover, #4f46e5);
}

.file-list {
  flex: 1;
  overflow-y: auto;
  border: 1px solid var(--border-color, #333);
  border-radius: 8px;
  max-height: 300px;
}

.file-item {
  display: flex;
  align-items: center;
  padding: 0.75rem 1rem;
  border-bottom: 1px solid var(--border-color, #333);
  cursor: pointer;
  transition: background 0.2s;
}

.file-item:last-child {
  border-bottom: none;
}

.file-item:hover {
  background: var(--hover-bg, rgba(255, 255, 255, 0.05));
}

.file-item.selected {
  background: var(--selected-bg, rgba(99, 102, 241, 0.15));
}

.file-item.streamable {
  border-left: 3px solid var(--accent-color, #6366f1);
}

.file-icon {
  font-size: 1.25rem;
  margin-right: 0.75rem;
}

.file-name {
  flex: 1;
  font-size: 0.9rem;
  color: var(--text-color, #fff);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.file-size {
  color: var(--text-muted, #888);
  font-size: 0.8rem;
  margin-left: 1rem;
  margin-right: 0.75rem;
}

.file-item input[type="checkbox"] {
  width: 18px;
  height: 18px;
  accent-color: var(--accent-color, #6366f1);
}

.actions {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-top: 1rem;
  padding-top: 1rem;
  border-top: 1px solid var(--border-color, #333);
}

.selection-info {
  display: flex;
  flex-direction: column;
  font-size: 0.875rem;
  color: var(--text-muted, #888);
}

.selected-size {
  color: var(--accent-color, #6366f1);
  font-weight: 600;
}

.buttons {
  display: flex;
  gap: 0.75rem;
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

.download-btn {
  padding: 0.75rem 1.5rem;
  background: var(--success-color, #10b981);
  color: white;
  border: none;
  border-radius: 8px;
  font-weight: 600;
  cursor: pointer;
  transition: background 0.2s;
}

.download-btn:hover:not(:disabled) {
  background: var(--success-hover, #059669);
}

.download-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
</style>
