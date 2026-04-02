<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import MagnetInput from './components/MagnetInput.vue';
import FileSelector from './components/FileSelector.vue';
import TorrentCard from './components/TorrentCard.vue';
import HistoryPanel from './components/HistoryPanel.vue';

import type { TorrentInfo, TorrentStats, CommandResult } from './types';

// State
const activeTorrents = ref<Map<number, { info: TorrentInfo; stats: TorrentStats }>>(new Map());
const selectedTorrent = ref<TorrentInfo | null>(null);
const showFileSelector = ref(false);
const errorMessage = ref('');
const deleteConfirmId = ref<number | null>(null);

// Event listener cleanup
let unlistenStats: UnlistenFn | null = null;

// Handle new torrent added
function onTorrentAdded(info: TorrentInfo) {
  // Show file selector for new torrent
  selectedTorrent.value = info;
  showFileSelector.value = true;
  
  // Add to active list with empty stats
  activeTorrents.value.set(info.id, {
    info,
    stats: {
      id: info.id,
      name: info.name,
      progress: 0,
      download_speed: 0,
      upload_speed: 0,
      peers_connected: 0,
      peers_total: 0,
      downloaded_bytes: 0,
      total_bytes: info.total_size,
      state: 'Ready',
      eta_seconds: null
    }
  });
}

// Called when user starts streaming from FileSelector
function onStreamingStarted() {
  // Keep the modal open so user can stream more files
  // Or close it:
  // showFileSelector.value = false;
  // selectedTorrent.value = null;
}

// Cancel file selection
function cancelFileSelection() {
  showFileSelector.value = false;
  selectedTorrent.value = null;
}

// Pause torrent
async function pauseTorrent(id: number) {
  try {
    await invoke<CommandResult<void>>('pause_download', { torrentId: id });
  } catch (err) {
    console.error('Pause error:', err);
  }
}

// Resume torrent - reopens file selector to choose what to stream
function resumeTorrent(id: number) {
  const torrent = activeTorrents.value.get(id);
  if (torrent?.info) {
    selectedTorrent.value = torrent.info;
    showFileSelector.value = true;
  }
}

// Delete torrent
async function deleteTorrent(id: number) {
  // Show confirmation modal
  deleteConfirmId.value = id;
}

// Confirm delete action
async function confirmDelete() {
  const id = deleteConfirmId.value;
  if (id === null) return;
  
  deleteConfirmId.value = null;
  
  try {
    const result = await invoke<CommandResult<void>>('delete_torrent', {
      torrentId: id,
      deleteFiles: false
    });
    
    if (result.success) {
      activeTorrents.value.delete(id);
    } else {
      console.error('Delete failed:', result.error);
      onError(result.error || 'Failed to delete torrent');
    }
  } catch (err) {
    console.error('Delete error:', err);
    onError('Failed to delete torrent');
  }
}

// Cancel delete action
function cancelDelete() {
  deleteConfirmId.value = null;
}

// Load magnet from history
function loadMagnetFromHistory(magnetLink: string) {
  // Trigger add via the MagnetInput component's method would be cleaner,
  // but for now we can just set it in the input
  const magnetInput = document.querySelector('.magnet-field') as HTMLInputElement;
  if (magnetInput) {
    magnetInput.value = magnetLink;
    magnetInput.dispatchEvent(new Event('input'));
  }
}

// Handle errors
function onError(message: string) {
  errorMessage.value = message;
  setTimeout(() => {
    errorMessage.value = '';
  }, 5000);
}

// Setup stats listener
async function setupStatsListener() {
  unlistenStats = await listen<TorrentStats[]>('torrent-stats', (event) => {
    for (const stats of event.payload) {
      const existing = activeTorrents.value.get(stats.id);
      if (existing) {
        activeTorrents.value.set(stats.id, {
          ...existing,
          stats
        });
      }
    }
  });
}

onMounted(() => {
  setupStatsListener();
});

onUnmounted(() => {
  unlistenStats?.();
});
</script>

<template>
  <div class="app">
    <!-- Header -->
    <header class="app-header">
      <h1>🧲 awawapp</h1>
      <p class="subtitle">Stream torrents to VLC</p>
    </header>
    
    <!-- Error Toast -->
    <div v-if="errorMessage" class="error-toast">
      {{ errorMessage }}
    </div>
    
    <!-- Main Content -->
    <div class="app-content">
      <!-- Left Panel: Main Actions -->
      <main class="main-panel">
        <!-- Magnet Input -->
        <MagnetInput
          @torrent-added="onTorrentAdded"
          @error="onError"
        />
        
        <!-- File Selector Modal -->
        <div v-if="showFileSelector && selectedTorrent" class="modal-overlay">
          <div class="modal">
            <FileSelector
              :torrent="selectedTorrent"
              @streaming-started="onStreamingStarted"
              @cancel="cancelFileSelection"
            />
          </div>
        </div>
        
        <!-- Delete Confirmation Modal -->
        <div v-if="deleteConfirmId !== null" class="modal-overlay" @click.self="cancelDelete">
          <div class="modal confirm-modal">
            <h3>Delete Torrent?</h3>
            <p>This will remove the torrent from the list. Downloaded files will not be deleted.</p>
            <div class="confirm-actions">
              <button class="btn-secondary" @click="cancelDelete">Cancel</button>
              <button class="btn-danger" @click="confirmDelete">Delete</button>
            </div>
          </div>
        </div>
        
        <!-- Active Torrents -->
        <section class="torrents-section">
          <h2 v-if="activeTorrents.size > 0">Active Downloads</h2>
          
          <div v-if="activeTorrents.size === 0" class="empty-state">
            <p>No active torrents</p>
            <p class="hint">Paste a magnet link above to get started</p>
          </div>
          
          <TorrentCard
            v-for="[id, torrent] in activeTorrents"
            :key="id"
            :stats="torrent.stats"
            :torrent-info="torrent.info"
            @pause="pauseTorrent"
            @resume="resumeTorrent"
            @delete="deleteTorrent"
          />
        </section>
      </main>
      
      <!-- Right Panel: History -->
      <aside class="history-sidebar">
        <HistoryPanel @load-magnet="loadMagnetFromHistory" />
      </aside>
    </div>
  </div>
</template>

<style>
:root {
  /* Color scheme */
  --bg-color: #0f0f1a;
  --card-bg: #1a1a2e;
  --input-bg: #0f0f1a;
  --border-color: #2a2a40;
  --text-color: #f0f0f5;
  --text-muted: #888;
  --accent-color: #6366f1;
  --accent-hover: #4f46e5;
  --success-color: #10b981;
  --success-hover: #059669;
  --warning-color: #f59e0b;
  --error-color: #ef4444;
  --hover-bg: rgba(255, 255, 255, 0.05);
  --btn-secondary: #2a2a40;
  --btn-secondary-hover: #3a3a50;
  --progress-bg: #2a2a40;
  
  /* Typography */
  font-family: 'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, sans-serif;
  font-size: 16px;
  line-height: 1.5;
  
  /* Rendering */
  font-synthesis: none;
  text-rendering: optimizeLegibility;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
}

* {
  box-sizing: border-box;
  margin: 0;
  padding: 0;
}

body {
  background: var(--bg-color);
  color: var(--text-color);
  min-height: 100vh;
}

.app {
  display: flex;
  flex-direction: column;
  min-height: 100vh;
  padding: 1rem;
}

.app-header {
  text-align: center;
  padding: 1rem 0 1.5rem;
}

.app-header h1 {
  font-size: 1.75rem;
  font-weight: 700;
  margin-bottom: 0.25rem;
}

.app-header .subtitle {
  color: var(--text-muted);
  font-size: 0.9rem;
}

.error-toast {
  position: fixed;
  top: 1rem;
  right: 1rem;
  background: var(--error-color);
  color: white;
  padding: 0.75rem 1.25rem;
  border-radius: 8px;
  font-size: 0.9rem;
  z-index: 1000;
  animation: slideIn 0.3s ease;
}

@keyframes slideIn {
  from {
    transform: translateX(100%);
    opacity: 0;
  }
  to {
    transform: translateX(0);
    opacity: 1;
  }
}

.app-content {
  display: grid;
  grid-template-columns: 1fr 300px;
  gap: 1rem;
  flex: 1;
}

.main-panel {
  display: flex;
  flex-direction: column;
}

.torrents-section {
  flex: 1;
}

.torrents-section h2 {
  font-size: 1rem;
  font-weight: 600;
  margin-bottom: 1rem;
  color: var(--text-muted);
}

.empty-state {
  text-align: center;
  padding: 3rem 1rem;
  color: var(--text-muted);
}

.empty-state p {
  margin-bottom: 0.5rem;
}

.empty-state .hint {
  font-size: 0.85rem;
  opacity: 0.7;
}

.history-sidebar {
  min-width: 0;
}

/* Modal */
.modal-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: rgba(0, 0, 0, 0.7);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 100;
}

.modal {
  width: 90%;
  max-width: 600px;
  max-height: 80vh;
  overflow: auto;
}

/* Confirm Modal */
.confirm-modal {
  background: var(--card-bg);
  border-radius: 12px;
  padding: 1.5rem;
  max-width: 400px;
  text-align: center;
}

.confirm-modal h3 {
  margin: 0 0 0.5rem 0;
  color: var(--text-color);
}

.confirm-modal p {
  color: var(--text-muted);
  margin: 0 0 1.5rem 0;
  font-size: 0.9rem;
}

.confirm-actions {
  display: flex;
  gap: 1rem;
  justify-content: center;
}

.btn-secondary {
  background: var(--btn-secondary);
  color: var(--text-color);
  border: none;
  padding: 0.75rem 1.5rem;
  border-radius: 8px;
  cursor: pointer;
  font-size: 1rem;
  transition: background 0.2s;
}

.btn-secondary:hover {
  background: var(--btn-secondary-hover);
}

.btn-danger {
  background: var(--error-color);
  color: white;
  border: none;
  padding: 0.75rem 1.5rem;
  border-radius: 8px;
  cursor: pointer;
  font-size: 1rem;
  transition: background 0.2s;
}

.btn-danger:hover {
  background: #dc2626;
}

/* Responsive */
@media (max-width: 800px) {
  .app-content {
    grid-template-columns: 1fr;
  }
  
  .history-sidebar {
    order: -1;
    max-height: 200px;
  }
}
</style>