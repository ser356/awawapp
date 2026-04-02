<script setup lang="ts">
import { ref, computed } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { CommandResult, TorrentInfo } from '../types';
import { isValidMagnetLink } from '../types';
import InputText from 'primevue/inputtext';
import Button from 'primevue/button';

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
      <InputText
        v-model="magnetLink"
        placeholder="Paste magnet link here..."
        @paste="handlePaste"
        @keyup.enter="addMagnet"
        :disabled="isLoading"
        class="magnet-field"
        autocomplete="off"
        spellcheck="false"
      />
      <Button
        @click="addMagnet"
        :disabled="!isValidInput || isLoading"
        :loading="isLoading"
        label="Add Torrent"
        icon="pi pi-plus"
        class="add-button"
      />
    </div>
    <p v-if="errorMessage" class="error-message">{{ errorMessage }}</p>
  </div>
</template>

<style scoped>
.magnet-input {
  padding: 1rem;
  background: var(--card-bg, #2a2420);
  border-radius: 12px;
  margin-bottom: 1rem;
}

.input-container {
  display: flex;
  gap: 0.75rem;
}

.magnet-field {
  flex: 1;
  background: var(--input-bg, #1e1a17) !important;
  border-color: var(--border-color, #3d352d) !important;
  color: var(--text-color, #f5f0ea) !important;
}

.magnet-field:focus {
  border-color: var(--accent-color, #9d8a78) !important;
  box-shadow: 0 0 0 2px rgba(157, 138, 120, 0.2) !important;
}

.add-button {
  background: var(--accent-color, #9d8a78) !important;
  border-color: var(--accent-color, #9d8a78) !important;
  min-width: 140px;
}

.add-button:hover:not(:disabled) {
  background: var(--accent-hover, #b5a08c) !important;
  border-color: var(--accent-hover, #b5a08c) !important;
}

.add-button:disabled {
  opacity: 0.5;
}

.error-message {
  color: var(--error-color, #c75a5a);
  font-size: 0.875rem;
  margin-top: 0.5rem;
  margin-bottom: 0;
}
</style>
