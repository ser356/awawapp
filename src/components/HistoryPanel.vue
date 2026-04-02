<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { TorrentHistory, CommandResult } from '../types';
import { formatBytes, formatDate } from '../types';

const emit = defineEmits<{
  (e: 'load-magnet', magnetLink: string): void;
}>();

const history = ref<TorrentHistory[]>([]);
const searchQuery = ref('');
const isLoading = ref(false);
const errorMessage = ref('');

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
    case 'completed': return 'var(--success-color, #10b981)';
    case 'downloading': return 'var(--accent-color, #6366f1)';
    case 'error': return 'var(--error-color, #ef4444)';
    default: return 'var(--text-muted, #888)';
  }
}

onMounted(loadHistory);
</script>

<template>
  <div class="history-panel">
    <div class="history-header">
      <h3>History</h3>
      <button @click="loadHistory" class="refresh-btn" :disabled="isLoading">
        🔄
      </button>
    </div>
    
    <div class="search-box">
      <input
        v-model="searchQuery"
        type="text"
        placeholder="Search history..."
        @input="searchHistory"
        maxlength="100"
      />
    </div>
    
    <div v-if="isLoading" class="loading">Loading...</div>
    
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
          <button
            @click="loadMagnet(item.magnet_link)"
            class="action-btn"
            title="Load again"
          >
            ↻
          </button>
          <button
            @click="deleteFromHistory(item.id)"
            class="action-btn delete"
            title="Delete"
          >
            ×
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.history-panel {
  background: var(--card-bg, #1a1a2e);
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
  color: var(--text-color, #fff);
}

.refresh-btn {
  background: transparent;
  border: none;
  font-size: 1.1rem;
  cursor: pointer;
  padding: 0.25rem;
  border-radius: 4px;
  transition: background 0.2s;
}

.refresh-btn:hover:not(:disabled) {
  background: var(--hover-bg, rgba(255, 255, 255, 0.1));
}

.refresh-btn:disabled {
  opacity: 0.5;
}

.search-box input {
  width: 100%;
  padding: 0.5rem 0.75rem;
  background: var(--input-bg, #0f0f1a);
  border: 1px solid var(--border-color, #333);
  border-radius: 6px;
  color: var(--text-color, #fff);
  font-size: 0.85rem;
  margin-bottom: 0.75rem;
}

.search-box input:focus {
  outline: none;
  border-color: var(--accent-color, #6366f1);
}

.loading,
.error,
.empty {
  text-align: center;
  padding: 2rem 1rem;
  color: var(--text-muted, #888);
  font-size: 0.9rem;
}

.error {
  color: var(--error-color, #ef4444);
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
  border-bottom: 1px solid var(--border-color, #333);
  transition: background 0.2s;
}

.history-item:last-child {
  border-bottom: none;
}

.history-item:hover {
  background: var(--hover-bg, rgba(255, 255, 255, 0.05));
}

.item-info {
  flex: 1;
  min-width: 0;
}

.item-name {
  display: block;
  font-size: 0.85rem;
  color: var(--text-color, #fff);
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
  color: var(--text-muted, #888);
}

.item-status {
  text-transform: capitalize;
}

.item-size {
  color: var(--text-muted, #888);
}

.item-actions {
  display: flex;
  gap: 0.25rem;
  margin-left: 0.5rem;
}

.action-btn {
  width: 24px;
  height: 24px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: none;
  color: var(--text-muted, #888);
  font-size: 1rem;
  cursor: pointer;
  border-radius: 4px;
  transition: all 0.2s;
}

.action-btn:hover {
  background: var(--hover-bg, rgba(255, 255, 255, 0.1));
  color: var(--text-color, #fff);
}

.action-btn.delete:hover {
  background: var(--error-color, #ef4444);
  color: white;
}
</style>
