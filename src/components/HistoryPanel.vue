<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { TorrentHistory, CommandResult } from '../types';
import { formatBytes, formatDate } from '../types';
import InputText from 'primevue/inputtext';
import Button from 'primevue/button';

const emit = defineEmits<{
  (e: 'load-magnet', magnetLink: string): void;
}>();

const history = ref<TorrentHistory[]>([]);
const searchQuery = ref('');
const isLoading = ref(false);
const errorMessage = ref('');

let unlistenHistoryUpdate: UnlistenFn | null = null;

async function loadHistory() {
  isLoading.value = true;
  errorMessage.value = '';
  
  try {
    const result = await invoke<CommandResult<TorrentHistory[]>>('get_history', {
      limit: 50
    });
    
    if (result.success && result.data) {
      history.value = result.data;
    } else {
      errorMessage.value = result.error || 'Failed to load history';
    }
  } catch (err) {
    console.error('Load history error:', err);
    errorMessage.value = 'Failed to connect';
  } finally {
    isLoading.value = false;
  }
}

async function searchHistory() {
  // Sanitize search input
  const sanitizedQuery = searchQuery.value.trim().slice(0, 100);
  
  if (!sanitizedQuery) {
    loadHistory();
    return;
  }
  
  isLoading.value = true;
  
  try {
    const result = await invoke<CommandResult<TorrentHistory[]>>('search_history', {
      query: sanitizedQuery
    });
    
    if (result.success && result.data) {
      history.value = result.data;
    }
  } catch (err) {
    console.error('Search error:', err);
  } finally {
    isLoading.value = false;
  }
}

async function deleteFromHistory(id: number) {
  try {
    const result = await invoke<CommandResult<void>>('delete_from_history', { id });
    
    if (result.success) {
      history.value = history.value.filter(h => h.id !== id);
    }
  } catch (err) {
    console.error('Delete error:', err);
  }
}

function loadMagnet(magnetLink: string) {
  emit('load-magnet', magnetLink);
}

function getStatusColor(status: string): string {
  switch (status.toLowerCase()) {
    case 'completed': return 'var(--success-color, #8fb573)';
    case 'downloading': return 'var(--accent-color, #9d8a78)';
    case 'error': return 'var(--error-color, #c75a5a)';
    default: return 'var(--text-muted, #a09080)';
  }
}

async function setupHistoryListener() {
  unlistenHistoryUpdate = await listen('history-updated', () => {
    // Reload history when a new torrent is added
    loadHistory();
  });
}

onMounted(() => {
  loadHistory();
  setupHistoryListener();
});

onUnmounted(() => {
  if (unlistenHistoryUpdate) {
    unlistenHistoryUpdate();
  }
});
</script>

<template>
  <div class="history-panel">
    <div class="history-header">
      <h3>History</h3>
      <Button
        @click="loadHistory"
        :loading="isLoading"
        icon="pi pi-refresh"
        text
        rounded
        size="small"
        class="refresh-btn"
      />
    </div>
    
    <div class="search-box">
      <InputText
        v-model="searchQuery"
        placeholder="Search history..."
        @input="searchHistory"
        maxlength="100"
        size="small"
        class="search-input"
      />
    </div>
    
    <div v-if="isLoading && !history.length" class="loading">Loading...</div>
    
    <div v-else-if="errorMessage" class="error">{{ errorMessage }}</div>
    
    <div v-else-if="history.length === 0" class="empty">
      No torrents in history
    </div>
    
    <div v-else class="history-list">
      <div
        v-for="item in history"
        :key="item.id"
        class="history-item"
      >
        <div class="item-info">
          <span class="item-name" :title="item.name">{{ item.name }}</span>
          <span class="item-meta">
            <span class="item-date">{{ formatDate(item.added_at) }}</span>
            <span
              class="item-status"
              :style="{ color: getStatusColor(item.status) }"
            >
              {{ item.status }}
            </span>
            <span v-if="item.total_size > 0" class="item-size">
              {{ formatBytes(item.total_size) }}
            </span>
          </span>
        </div>
        <div class="item-actions">
          <Button
            @click="loadMagnet(item.magnet_link)"
            icon="pi pi-replay"
            text
            rounded
            size="small"
            v-tooltip.left="'Load again'"
          />
          <Button
            @click="deleteFromHistory(item.id)"
            icon="pi pi-times"
            text
            rounded
            size="small"
            severity="danger"
            v-tooltip.left="'Delete'"
          />
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.history-panel {
  background: var(--card-bg, #2a2420);
  border-radius: 12px;
  padding: 1rem;
  height: 100%;
  display: flex;
  flex-direction: column;
}

.history-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 0.75rem;
}

.history-header h3 {
  margin: 0;
  font-size: 1rem;
  color: var(--text-color, #f5f0ea);
}

.refresh-btn {
  color: var(--text-color, #f5f0ea) !important;
}

.refresh-btn:hover {
  color: #fff !important;
  background: var(--hover-bg) !important;
}

.search-box {
  margin-bottom: 0.75rem;
}

.search-input {
  width: 100%;
  background: var(--input-bg, #1e1a17) !important;
  border-color: var(--border-color, #3d352d) !important;
  color: var(--text-color, #f5f0ea) !important;
}

.search-input:focus {
  border-color: var(--accent-color, #9d8a78) !important;
  box-shadow: 0 0 0 2px rgba(157, 138, 120, 0.2) !important;
}

.loading,
.error,
.empty {
  text-align: center;
  padding: 2rem 1rem;
  color: var(--text-muted, #a09080);
  font-size: 0.9rem;
}

.error {
  color: var(--error-color, #c75a5a);
}

.history-list {
  flex: 1;
  overflow-y: auto;
}

.history-item {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  padding: 0.75rem;
  border-bottom: 1px solid var(--border-color, #3d352d);
  transition: background 0.2s;
}

.history-item:last-child {
  border-bottom: none;
}

.history-item:hover {
  background: var(--hover-bg, rgba(157, 138, 120, 0.1));
}

.item-info {
  flex: 1;
  min-width: 0;
}

.item-name {
  display: block;
  font-size: 0.85rem;
  color: var(--text-color, #f5f0ea);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  margin-bottom: 0.25rem;
}

.item-meta {
  display: flex;
  gap: 0.5rem;
  font-size: 0.7rem;
  flex-wrap: wrap;
}

.item-date {
  color: var(--text-muted, #a09080);
}

.item-status {
  text-transform: capitalize;
}

.item-size {
  color: var(--text-muted, #a09080);
}

.item-actions {
  display: flex;
  gap: 0.25rem;
  margin-left: 0.5rem;
}
</style>
