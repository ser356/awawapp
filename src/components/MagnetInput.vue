<script setup lang="ts">
import { ref, computed } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { CommandResult, TorrentInfo } from '../types';
import { isValidMagnetLink } from '../types';

const emit = defineEmits<{
  (e: 'torrent-added', info: TorrentInfo): void;
  (e: 'error', message: string): void;
}>();

const magnetLink = ref('');
const isLoading = ref(false);
const errorMessage = ref('');

// Input sanitization - remove potentially dangerous characters
function sanitizeInput(input: string): string {
  return input.trim().replace(/[\x00-\x1F\x7F]/g, '');
}

const isValidInput = computed(() => {
  const sanitized = sanitizeInput(magnetLink.value);
  return sanitized.length > 0 && isValidMagnetLink(sanitized);
});

async function addMagnet() {
  const sanitizedLink = sanitizeInput(magnetLink.value);
  
  if (!isValidMagnetLink(sanitizedLink)) {
    errorMessage.value = 'Invalid magnet link format';
    return;
  }
  
  isLoading.value = true;
  errorMessage.value = '';
  
  try {
    const result = await invoke<CommandResult<TorrentInfo>>('add_magnet', {
      magnetUri: sanitizedLink
    });
    
    if (result.success && result.data) {
      emit('torrent-added', result.data);
      magnetLink.value = '';
    } else {
      errorMessage.value = result.error || 'Failed to add torrent';
      emit('error', errorMessage.value);
    }
  } catch (err) {
    errorMessage.value = 'Connection error. Please try again.';
    emit('error', errorMessage.value);
    console.error('Add magnet error:', err);
  } finally {
    isLoading.value = false;
  }
}

function handlePaste(event: ClipboardEvent) {
  const pastedText = event.clipboardData?.getData('text') || '';
  if (isValidMagnetLink(pastedText.trim())) {
    magnetLink.value = pastedText.trim();
  }
}
</script>

<template>
  <div class="magnet-input">
    <div class="input-container">
      <input
        v-model="magnetLink"
        type="text"
        placeholder="Paste magnet link here..."
        @paste="handlePaste"
        @keyup.enter="addMagnet"
        :disabled="isLoading"
        class="magnet-field"
        autocomplete="off"
        spellcheck="false"
      />
      <button
        @click="addMagnet"
        :disabled="!isValidInput || isLoading"
        class="add-button"
      >
        <span v-if="isLoading" class="spinner"></span>
        <span v-else>Add Torrent</span>
      </button>
    </div>
    <p v-if="errorMessage" class="error-message">{{ errorMessage }}</p>
  </div>
</template>

<style scoped>
.magnet-input {
  padding: 1rem;
  background: var(--card-bg, #1a1a2e);
  border-radius: 12px;
  margin-bottom: 1rem;
}

.input-container {
  display: flex;
  gap: 0.75rem;
}

.magnet-field {
  flex: 1;
  padding: 0.875rem 1rem;
  border: 2px solid var(--border-color, #333);
  border-radius: 8px;
  background: var(--input-bg, #0f0f1a);
  color: var(--text-color, #fff);
  font-size: 0.95rem;
  transition: border-color 0.2s;
}

.magnet-field:focus {
  outline: none;
  border-color: var(--accent-color, #6366f1);
}

.magnet-field:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.add-button {
  padding: 0.875rem 1.5rem;
  background: var(--accent-color, #6366f1);
  color: white;
  border: none;
  border-radius: 8px;
  font-weight: 600;
  cursor: pointer;
  transition: background 0.2s, transform 0.1s;
  min-width: 140px;
  display: flex;
  align-items: center;
  justify-content: center;
}

.add-button:hover:not(:disabled) {
  background: var(--accent-hover, #4f46e5);
  transform: translateY(-1px);
}

.add-button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.spinner {
  width: 20px;
  height: 20px;
  border: 2px solid rgba(255, 255, 255, 0.3);
  border-top-color: white;
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

.error-message {
  color: var(--error-color, #ef4444);
  font-size: 0.875rem;
  margin-top: 0.5rem;
  margin-bottom: 0;
}
</style>
