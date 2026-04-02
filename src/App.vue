<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import MagnetInput from './components/MagnetInput.vue';
import FileSelector from './components/FileSelector.vue';
import TorrentCard from './components/TorrentCard.vue';
import HistoryPanel from './components/HistoryPanel.vue';

import Dialog from 'primevue/dialog';
import Button from 'primevue/button';
import Toast from 'primevue/toast';
import { useToast } from 'primevue/usetoast';

import type { TorrentInfo, TorrentStats, CommandResult } from './types';

const toast = useToast();

// State
const activeTorrents = ref<Map<number, { info: TorrentInfo; stats: TorrentStats }>>(new Map());
const selectedTorrent = ref<TorrentInfo | null>(null);
const showFileSelector = ref(false);
const errorMessage = ref('');
const deleteConfirmId = ref<number | null>(null);
const showDeleteDialog = ref(false);

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
  showDeleteDialog.value = true;
}

// Confirm delete action
async function confirmDelete() {
  const id = deleteConfirmId.value;
  if (id === null) return;
  
  showDeleteDialog.value = false;
  deleteConfirmId.value = null;
  
  try {
    const result = await invoke<CommandResult<void>>('delete_torrent', {
      torrentId: id,
      deleteFiles: true  // always clean up temp files
    });
    
    if (result.success) {
      activeTorrents.value.delete(id);
      toast.add({ severity: 'success', summary: 'Deleted', detail: 'Torrent removed', life: 3000 });
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
  showDeleteDialog.value = false;
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
  toast.add({ severity: 'error', summary: 'Error', detail: message, life: 5000 });
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

// Easter egg: "awawa" sound
const keyBuffer = ref('');

function playAwawaSound() {
  // Create new Audio instance each time for reliable playback
  const audio = new Audio('/awawa.ogg');
  audio.volume = 1.0;
  audio.play().catch(err => console.log('Audio play error:', err));
}

function handleGlobalKeydown(event: KeyboardEvent) {
  // Only track letter keys
  if (event.key.length === 1 && /[a-zA-Z]/.test(event.key)) {
    keyBuffer.value += event.key.toLowerCase();
    
    // Keep only last 5 characters
    if (keyBuffer.value.length > 5) {
      keyBuffer.value = keyBuffer.value.slice(-5);
    }
    
    // Check for "awawa"
    if (keyBuffer.value === 'awawa') {
      playAwawaSound();
      keyBuffer.value = ''; // Reset buffer
      
      // Show toast notification
      toast.add({
        severity: 'info',
        summary: '🐾 awawa!',
        detail: 'You found the secret!',
        life: 2000
      });
    }
  }
}

onMounted(() => {
  setupStatsListener();
  window.addEventListener('keydown', handleGlobalKeydown);
});

onUnmounted(() => {
  unlistenStats?.();
  window.removeEventListener('keydown', handleGlobalKeydown);
});
</script>

<template>
  <div class="app app-dark">
    <!-- Toast for notifications -->
    <Toast position="top-right" />
    
    <!-- Header -->
    <header class="app-header">
      <div class="logo-container">
        <img src="/mascot.png" alt="awawapp mascot" class="mascot-logo" />
        <h1>awawapp</h1>
      </div>
      <p class="subtitle">Stream torrents to VLC</p>
    </header>
    
    <!-- Main Content -->
    <div class="app-content">
      <!-- Left Panel: Main Actions -->
      <main class="main-panel">
        <!-- Magnet Input -->
        <MagnetInput
          @torrent-added="onTorrentAdded"
          @error="onError"
        />
        
        <!-- File Selector Dialog -->
        <Dialog
          v-model:visible="showFileSelector"
          modal
          :closable="true"
          :draggable="false"
          :showHeader="false"
          :style="{ width: '600px', maxWidth: '90vw' }"
          :pt="{ content: { style: 'padding: 0' } }"
        >
          <FileSelector
            :torrent="selectedTorrent"
            @streaming-started="onStreamingStarted"
            @cancel="cancelFileSelection"
          />
        </Dialog>
        
        <!-- Delete Confirmation Dialog -->
        <Dialog
          v-model:visible="showDeleteDialog"
          modal
          header="Delete Torrent?"
          :style="{ width: '400px' }"
        >
          <p class="confirm-text">This will remove the torrent from the list. Downloaded files will not be deleted.</p>
          <template #footer>
            <Button label="Cancel" severity="secondary" outlined @click="cancelDelete" />
            <Button label="Delete" severity="danger" @click="confirmDelete" />
          </template>
        </Dialog>
        
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
  /* Mascot-inspired color scheme: warm browns, black, white */
  --bg-color: #1a1614;
  --card-bg: #2a2420;
  --input-bg: #1e1a17;
  --border-color: #3d352d;
  --text-color: #f5f0ea;
  --text-muted: #a09080;
  --accent-color: #9d8a78;
  --accent-hover: #b5a08c;
  --success-color: #8fb573;
  --success-hover: #7aa35e;
  --warning-color: #d9a85c;
  --error-color: #c75a5a;
  --hover-bg: rgba(157, 138, 120, 0.1);
  --btn-secondary: #3d352d;
  --btn-secondary-hover: #4d433a;
  --progress-bg: #3d352d;
  
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

.logo-container {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 0.75rem;
}

.mascot-logo {
  width: 48px;
  height: 48px;
  object-fit: contain;
}

.app-header h1 {
  font-size: 1.75rem;
  font-weight: 700;
  margin: 0;
  color: #fff;
}

.app-header .subtitle {
  color: var(--text-muted);
  font-size: 0.9rem;
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
/* PrimeVue overrides for dark theme */
.p-dialog {
  background: var(--card-bg) !important;
  border: 1px solid var(--border-color) !important;
  border-radius: 12px !important;
  box-shadow: 0 20px 60px rgba(0, 0, 0, 0.5) !important;
}

.p-dialog-header {
  background: var(--card-bg) !important;
  color: var(--text-color) !important;
  border-bottom: 1px solid var(--border-color) !important;
  padding: 1.25rem !important;
}

.p-dialog-header .p-dialog-title {
  color: var(--text-color) !important;
  font-weight: 600 !important;
}

.p-dialog-header-close {
  color: var(--text-muted) !important;
  background: transparent !important;
  border: none !important;
}

.p-dialog-header-close:hover {
  color: var(--text-color) !important;
  background: var(--hover-bg) !important;
}

.p-dialog-content {
  background: var(--card-bg) !important;
  color: var(--text-color) !important;
  padding: 1.25rem !important;
}

.p-dialog-footer {
  background: var(--card-bg) !important;
  border-top: 1px solid var(--border-color) !important;
  padding: 1rem 1.25rem !important;
}

.p-dialog-mask {
  background: rgba(0, 0, 0, 0.7) !important;
}

.confirm-text {
  color: var(--text-muted);
  margin: 0;
  font-size: 0.9rem;
}

/* PrimeVue Button customization */
.p-button {
  background: var(--accent-color) !important;
  border-color: var(--accent-color) !important;
  color: white !important;
}

.p-button:hover {
  background: var(--accent-hover) !important;
  border-color: var(--accent-hover) !important;
}

.p-button.p-button-outlined {
  color: var(--accent-color) !important;
  border-color: var(--accent-color) !important;
  background: transparent !important;
}

.p-button.p-button-outlined:hover {
  background: var(--hover-bg) !important;
  color: var(--accent-color) !important;
}

.p-button.p-button-secondary {
  background: var(--btn-secondary) !important;
  border-color: var(--btn-secondary) !important;
  color: var(--text-color) !important;
}

.p-button.p-button-secondary:hover {
  background: var(--btn-secondary-hover) !important;
  border-color: var(--btn-secondary-hover) !important;
}

.p-button.p-button-secondary.p-button-outlined {
  background: transparent !important;
  color: var(--text-muted) !important;
  border-color: var(--border-color) !important;
}

.p-button.p-button-secondary.p-button-outlined:hover {
  background: var(--hover-bg) !important;
  color: var(--text-color) !important;
}

.p-button.p-button-danger {
  background: var(--error-color) !important;
  border-color: var(--error-color) !important;
}

.p-button.p-button-danger:hover {
  background: #a84a4a !important;
  border-color: #a84a4a !important;
}

.p-button.p-button-danger.p-button-outlined {
  background: transparent !important;
  color: var(--error-color) !important;
  border-color: var(--error-color) !important;
}

.p-button.p-button-danger.p-button-outlined:hover {
  background: rgba(199, 90, 90, 0.1) !important;
}

/* PrimeVue InputText */
.p-inputtext {
  background: var(--input-bg) !important;
  border-color: var(--border-color) !important;
  color: var(--text-color) !important;
}

.p-inputtext:focus {
  border-color: var(--accent-color) !important;
  box-shadow: 0 0 0 2px rgba(157, 138, 120, 0.2) !important;
}

.p-inputtext::placeholder {
  color: var(--text-muted) !important;
}

/* PrimeVue Tag */
.p-tag {
  background: var(--btn-secondary) !important;
  color: var(--text-muted) !important;
}

.p-tag.p-tag-info {
  background: rgba(157, 138, 120, 0.2) !important;
  color: var(--accent-color) !important;
}

.p-tag.p-tag-success {
  background: rgba(143, 181, 115, 0.2) !important;
  color: var(--success-color) !important;
}

.p-tag.p-tag-warn {
  background: rgba(217, 168, 92, 0.2) !important;
  color: var(--warning-color) !important;
}

.p-tag.p-tag-danger {
  background: rgba(199, 90, 90, 0.2) !important;
  color: var(--error-color) !important;
}

/* PrimeVue Toast customization */
.p-toast {
  opacity: 0.98;
}

.p-toast-message {
  background: var(--card-bg) !important;
  border: 1px solid var(--border-color) !important;
  color: var(--text-color) !important;
}

.p-toast-message-content {
  color: var(--text-color) !important;
}

.p-toast-summary {
  color: var(--text-color) !important;
}

.p-toast-detail {
  color: var(--text-muted) !important;
}

.p-toast-message-success {
  border-left: 4px solid var(--success-color) !important;
}

.p-toast-message-error {
  border-left: 4px solid var(--error-color) !important;
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